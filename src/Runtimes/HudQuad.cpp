#include "Runtimes/HudQuad.hpp"
#include "Camera/GazeCenter.hpp"
#include "Render/GpuStageProfile.hpp"
#include "Core/LiveControls.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/XrMath.hpp"
#include <algorithm>
#include <cmath>
#include <atomic>

extern LiveControls g_liveControls;
extern void Log(const char*, ...);
extern "C" {
__declspec(dllexport) cvr::hud::PoseDebug CyberpunkVR_HudPoseDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudPoseDebugSeq{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudSubmittedFrames{0};
// Internal A/B: 0 preserves the prior per-panel fence wait; 1 rotates fenced
// command allocators and reuses the last published HUD when all slots are busy.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudSubmitRing{1};
}
namespace cvr::hud {
void Quad::RetireCompleted(uint64_t completed) {
    for(auto& slot:copies) if(slot.frame && slot.fenceValue<=completed) slot.frame.reset();
}
bool Quad::Ensure(XrSession session, ID3D12Device* device, uint32_t w, uint32_t h) {
    if(submissionFailed)return false;
    if (swapchain && w==width && h==height) return true;
    if (fence && fence->GetCompletedValue()<fenceValue) return false;
    if (swapchain) {
        if(acquired) {
            XrSwapchainImageWaitInfo wait{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};wait.timeout=0;
            if(xrWaitSwapchainImage(swapchain,&wait)!=XR_SUCCESS)return false;
            XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
            if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&release)))return false;
            acquired=false;
        }
        xrDestroySwapchain(swapchain);swapchain=XR_NULL_HANDLE;images.clear();
    }
    uint32_t count=0;
    if (XR_FAILED(xrEnumerateSwapchainFormats(session,0,&count,nullptr))) return false;
    std::vector<int64_t> formats(count);
    if (XR_FAILED(xrEnumerateSwapchainFormats(session,count,&count,formats.data())) ||
        std::find(formats.begin(),formats.end(),DXGI_FORMAT_R8G8B8A8_UNORM_SRGB)==formats.end()) return false;
    if (!fence && FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))) return false;
    if (!event) event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
    if (!event) return false;
    for(auto& slot:copies) {
        if(!slot.allocator && FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&slot.allocator)))) return false;
        if(!slot.commands) {
            if(FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,slot.allocator.Get(),nullptr,IID_PPV_ARGS(&slot.commands))))return false;
            slot.commands->Close();
        }
    }
    XrSwapchainCreateInfo ci{XR_TYPE_SWAPCHAIN_CREATE_INFO};
    ci.usageFlags=XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT|XR_SWAPCHAIN_USAGE_SAMPLED_BIT|XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT;
    ci.format=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    ci.sampleCount=1; ci.width=w; ci.height=h; ci.faceCount=1; ci.arraySize=1; ci.mipCount=1;
    if (XR_FAILED(xrCreateSwapchain(session,&ci,&swapchain))) return false;
    if (XR_FAILED(xrEnumerateSwapchainImages(swapchain,0,&count,nullptr)) || !count) return false;
    images.assign(count,{XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
    if (XR_FAILED(xrEnumerateSwapchainImages(swapchain,count,&count,
        reinterpret_cast<XrSwapchainImageBaseHeader*>(images.data())))) return false;
    width=w; height=h; lastStamp=0;publishedTick=0;publishedMasked=publishedLootVisible=false;
    Log("[hud-panel] XR layer ready %ux%u (%u images)\n",w,h,count);
    return true;
}

bool Quad::UpdateImage(const std::shared_ptr<Frame>& frame,ID3D12CommandQueue* queue,bool pipelined) {
    auto completed=fence->GetCompletedValue();
    if(completed==UINT64_MAX){submissionFailed=true;return false;}
    if(!pipelined && completed<fenceValue) {
        if(FAILED(fence->SetEventOnCompletion(fenceValue,event)) || WaitForSingleObject(event,100)!=WAIT_OBJECT_0)return false;
        completed=fence->GetCompletedValue();
        if(completed==UINT64_MAX){submissionFailed=true;return false;}
    }
    RetireCompleted(completed);
    if(lastStamp==frame->serial && !acquired)return true;
    CopySlot* freeSlot=nullptr;
    for(size_t i=0;i<copies.size();++i) {
        const auto index=(copyCursor+i)%copies.size();
        if(copies[index].fenceValue<=completed){freeSlot=&copies[index];copyCursor=(index+1)%copies.size();break;}
    }
    // The published swapchain image is still valid. Its source canvas need not
    // be retained after the GPU copy; only immutable display metadata is kept.
    if(!freeSlot)return lastStamp!=0;
    XrSwapchainImageAcquireInfo acquire{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
    if(!acquired) {
        if(XR_FAILED(xrAcquireSwapchainImage(swapchain,&acquire,&acquiredIndex)))return false;
        acquired=true;
    }
    XrSwapchainImageWaitInfo wait{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};wait.timeout=pipelined?0:100000000;
    const auto waited=xrWaitSwapchainImage(swapchain,&wait);
    if(waited==XR_TIMEOUT_EXPIRED)return pipelined && lastStamp!=0;
    if(waited!=XR_SUCCESS)return false;
    auto& slot=*freeSlot;
    auto release=[&] {
        XrSwapchainImageReleaseInfo info{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
        const auto result=xrReleaseSwapchainImage(swapchain,&info);acquired=false;return result;
    };
    if(acquiredIndex>=images.size() || FAILED(slot.allocator->Reset()) ||
       FAILED(slot.commands->Reset(slot.allocator.Get(),nullptr))) {release();return false;}
    auto* commands=slot.commands.Get();
    auto gpuProfile=cvr::gpu::profile::Begin(commands);
    {
        cvr::gpu::profile::Scope gpuScope(gpuProfile,cvr::gpu::profile::Stage::HudSubmit);
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={images[acquiredIndex].texture,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                      D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_DEST};
        commands->ResourceBarrier(1,&b);
        commands->CopyResource(images[acquiredIndex].texture,frame->canvas.Get());
        std::swap(b.Transition.StateBefore,b.Transition.StateAfter);commands->ResourceBarrier(1,&b);
    }
    if(FAILED(commands->Close())){release();return false;}
    // The exact source remains alive until THIS slot's submitted copy completes.
    // No slot or allocator is reused merely because another copy has completed.
    slot.frame=frame;slot.fenceValue=++fenceValue;
    ID3D12CommandList* lists[]={commands};queue->ExecuteCommandLists(1,lists);
    const bool signaled=SUCCEEDED(queue->Signal(fence.Get(),slot.fenceValue));
    if(signaled)cvr::gpu::profile::Submitted(gpuProfile,queue,fence.Get(),slot.fenceValue);
    else submissionFailed=true;
    const auto released=release();
    if(!signaled || XR_FAILED(released))return false;
    lastStamp=frame->serial;publishedTick=frame->stamp;
    publishedMasked=frame->masked;publishedLootVisible=frame->lootVisible;
    return true;
}

const XrCompositionLayerBaseHeader* Quad::Prepare(XrSession session, XrSpace space,
    ID3D12Device* device, ID3D12CommandQueue* queue, bool enabled,
    bool tracked, const XrPosef& head, uint64_t origin,
    XrTime displayTime, const XrVector3f* eyeOffsets, float bodyYaw) {
    secondEye=false;
    // THE SCANNER STAYS AT INFINITY with stereo depth on. Its reticle marks what is scanned, wherever that
    // is, and its quickhack list and key hints are drawn in the eye with no parallax; at the panel's
    // depth the reticle and the details sat on the target differently in each eye (17 px apart at 1.5 m on
    // the simulator, 2026-10-18) and split from the in-eye half of the scanner.
    const bool stereoDepth=g_liveControls.xrHudStereoDepth && channel!=Channel::Surveillance;
    if (!enabled || !g_liveControls.xrHudPanel || (channel==Channel::Interaction && !g_liveControls.xrInteractionPanel)) {
        if(fence && !submissionFailed)RetireCompleted(fence->GetCompletedValue());
        SetConsumerReady(false,channel); follow.Reset(); clock.Reset(); return nullptr;
    }
    auto frame=Latest(channel);
    if (frame && frame->empty) {
        // The channel has nothing visible: no layer, but the consumer stays ready so the native
        // slots stay masked and the next visible content appears here, not natively first.
        if(fence && !submissionFailed)RetireCompleted(fence->GetCompletedValue());
        const bool eyesReady=stereoDepth || eyeOffsets || eyeOffsetsValid;
        SetConsumerReady((tracked || follow.valid) && eyesReady,channel);
        return nullptr;
    }
    if (!frame || !device || !queue) {
        if(fence && !submissionFailed)RetireCompleted(fence->GetCompletedValue());
        SetConsumerReady(false,channel);return nullptr;
    }
    const auto desc=frame->canvas->GetDesc();
    if (!Ensure(session,device,static_cast<uint32_t>(desc.Width),desc.Height)) {
        SetConsumerReady(false,channel); return nullptr;
    }
    if(!UpdateImage(frame,queue,CyberpunkVR_HudSubmitRing.load(std::memory_order_relaxed)!=0) ||
       !lastStamp || GetTickCount64()-publishedTick>=300) {SetConsumerReady(false,channel);return nullptr;}
    const bool eyesReady=stereoDepth || eyeOffsets || eyeOffsetsValid;
    SetConsumerReady((tracked || follow.valid) && eyesReady,channel);
    if(!eyesReady)return nullptr;
    if (!publishedMasked) return nullptr; // Preparation succeeded; UI may now mask the native slots.
    // Loot and its tooltip share the native interaction texture. Carry its
    // visible content with the captured frame so the cone changes with the
    // image, without allocating a third canvas or resetting its yaw anchor.
    const float degrees=channel==Channel::Interaction
        ? (publishedLootVisible?g_liveControls.xrLootFollowDeg:g_liveControls.xrInteractionFollowDeg)
        : g_liveControls.xrHudFollowDeg;
    // A zero interaction or loot cone is the head-locked panel, like the hit marker's; so is a zero main HUD cone in
    // head-follow mode, so the main HUD can stay with head-locked prompts instead of lagging next to them.
    const bool headLocked=channel==Channel::Basilisk || channel==Channel::Surveillance || channel==Channel::HitMarker ||
        (channel==Channel::Interaction && degrees<=0.0f) ||
        (channel==Channel::Main && g_liveControls.xrHudFollowMode==0 && degrees<=0.0f);
    const int wantedMode=headLocked?2:(channel==Channel::Main?g_liveControls.xrHudFollowMode:0);
    if (originSerial!=origin || followMode!=wantedMode) {
        follow.Reset();clock.Reset();originSerial=origin;followMode=wantedMode;
    }
    const float dt=clock.Step(displayTime);
    if (tracked) {
        const auto& q=head.orientation;
        const float headYaw=HeadYaw(q.x,q.y,q.z,q.w,follow.valid?follow.yaw:bodyYaw);
        const float cone=std::clamp(degrees,5.0f,90.0f)*0.01745329252f;
        const float yaw=headLocked?headYaw:(followMode==1 && std::isfinite(bodyYaw) ? bodyYaw : follow.Update(headYaw,cone,clock.elapsed,DelayedFollow(channel)));
        if(headLocked || followMode==1) {follow.yaw=yaw;follow.valid=true;follow.following=false;follow.ResetDelay();}
        const float distance=std::clamp(static_cast<float>(channel==Channel::Interaction?g_liveControls.xrInteractionDistance:g_liveControls.xrHudDistance),0.5f,5.0f);
        anchor.orientation={0,sinf(yaw*0.5f),0,cosf(yaw*0.5f)};
        anchor.position={head.position.x-sinf(yaw)*distance,head.position.y,head.position.z-cosf(yaw)*distance};
        if(headLocked) {
            // Zero cone follows the complete HMD pose immediately: yaw, pitch
            // and roll, with no catch-up speed or three-second rest timer.
            anchor.orientation=head.orientation;
            const float gazeY=channel==Channel::Surveillance ? cvr::gaze::Read().tangentY : 0.f;
            const auto offset=RotateVector(head.orientation,{0,distance*gazeY,-distance});
            anchor.position={head.position.x+offset.x,head.position.y+offset.y,head.position.z+offset.z};
        }
        const float fov=std::clamp(static_cast<float>(channel==Channel::Interaction?g_liveControls.xrInteractionFov:g_liveControls.xrHudFov),30.0f,120.0f)*0.01745329252f;
        const float h=2.0f*distance*tanf(fov*0.5f);
        layer.size={h*width/height,h};
    } else { follow.ResetDelay(); clock.Reset(); }
    if (!follow.valid) return nullptr;
    layer.pose=anchor;
    layer.layerFlags=XR_COMPOSITION_LAYER_BLEND_TEXTURE_SOURCE_ALPHA_BIT;
    layer.space=space; layer.eyeVisibility=XR_EYE_VISIBILITY_BOTH;
    layer.subImage={swapchain,{{0,0},{static_cast<int32_t>(width),static_cast<int32_t>(height)}},0};
    if(eyeOffsets && tracked) {lastEyeOffsets[0]=eyeOffsets[0];lastEyeOffsets[1]=eyeOffsets[1];eyeOffsetsValid=true;}
    if(!stereoDepth && eyeOffsetsValid) {
        // Each eye sees the same angular direction, including canted/asymmetric
        // views. Cancel only eye translation; the common orientation is retained.
        rightLayer=layer;
        layer.eyeVisibility=XR_EYE_VISIBILITY_LEFT;
        rightLayer.eyeVisibility=XR_EYE_VISIBILITY_RIGHT;
        layer.pose.position.x+=lastEyeOffsets[0].x;layer.pose.position.y+=lastEyeOffsets[0].y;layer.pose.position.z+=lastEyeOffsets[0].z;
        rightLayer.pose.position.x+=lastEyeOffsets[1].x;rightLayer.pose.position.y+=lastEyeOffsets[1].y;rightLayer.pose.position.z+=lastEyeOffsets[1].z;
        secondEye=true;
    }
    if(channel==Channel::Main && cvr::RuntimeDiagnosticsEnabled()) {
    CyberpunkVR_HudPoseDebugSeq.fetch_add(1,std::memory_order_acq_rel);
    auto& debug=CyberpunkVR_HudPoseDebug;
    ++debug.frames;debug.displayTime=displayTime;debug.panelYaw=follow.yaw;debug.dt=dt;
    debug.sourceSerial=lastStamp;debug.sourceTick=publishedTick;
    const auto& q=head.orientation;
    debug.headYaw=HeadYaw(q.x,q.y,q.z,q.w,follow.yaw);
    debug.mode=followMode;debug.tracked=tracked;debug.layers=secondEye?2:1;
    debug.head=head.position;debug.left=layer.pose.position;debug.right=secondEye?rightLayer.pose.position:layer.pose.position;
    if(secondEye) {
        const float x=(debug.left.x-lastEyeOffsets[0].x)-(debug.right.x-lastEyeOffsets[1].x);
        const float y=(debug.left.y-lastEyeOffsets[0].y)-(debug.right.y-lastEyeOffsets[1].y);
        const float z=(debug.left.z-lastEyeOffsets[0].z)-(debug.right.z-lastEyeOffsets[1].z);
        debug.eyeError=sqrtf(x*x+y*y+z*z);
    } else debug.eyeError=0;
    CyberpunkVR_HudPoseDebugSeq.fetch_add(1,std::memory_order_release);
    }
    return reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layer);
}

void Quad::Shutdown() {
    SetConsumerReady(false,channel);
    if (fence && fence->GetCompletedValue()<fenceValue && event) {
        fence->SetEventOnCompletion(fenceValue,event);
        if (WaitForSingleObject(event,1000)!=WAIT_OBJECT_0) return;
    }
    if(swapchain && acquired) {
        XrSwapchainImageWaitInfo wait{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};wait.timeout=100000000;
        if(xrWaitSwapchainImage(swapchain,&wait)!=XR_SUCCESS)return;
        XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
        if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&release)))return;
        acquired=false;
    }
    if (swapchain) xrDestroySwapchain(swapchain);
    swapchain=XR_NULL_HANDLE; acquired=false; images.clear(); width=height=0; lastStamp=0; follow.Reset();clock.Reset();secondEye=false;
    for(auto& slot:copies){slot.frame.reset();slot.commands.Reset();slot.allocator.Reset();slot.fenceValue=0;}
    copyCursor=0;fence.Reset();fenceValue=0;publishedTick=0;publishedMasked=publishedLootVisible=false;submissionFailed=false;
    if (event) CloseHandle(event); event=nullptr;
}
}
