#include "Runtimes/HudQuad.hpp"
#include "Anim/WheelGrab.hpp"
#include "Camera/GazeCenter.hpp"
#include "Render/GpuStageProfile.hpp"
#include "Core/LiveControls.hpp"
#include "Utils/DebugGate.hpp"
#include "Utils/XrMath.hpp"
#include "Runtimes/InteractionDepth.hpp"
#include "Runtimes/HudClearance.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <Windows.h>
#include <algorithm>
#include <cmath>
#include <atomic>

extern LiveControls g_liveControls;
extern void Log(const char*, ...);
extern "C" {
__declspec(dllexport) cvr::hud::PoseDebug CyberpunkVR_HudPoseDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudPoseDebugSeq{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudSubmittedFrames{0};
// Placement selected for the current minimap layer; zero also covers native fallback.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_VehicleMinimapProjected{0};
// Internal A/B: 0 preserves the prior per-panel fence wait; 1 rotates fenced
// command allocators and reuses the last published HUD when all slots are busy.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HudSubmitRing{1};
}
extern "C" int CyberpunkVR_InteractionAnchor;
namespace { float anchorDistance=0.0f; }   // the interaction panel's smoothed distance
extern "C" int CyberpunkVR_HudClearance;
extern "C" float CyberpunkVR_HudClearanceGrow;
extern "C" int CyberpunkVR_HudClearanceHands;
extern "C" float CyberpunkVR_HudClearanceHandsScale;
namespace { float clearanceDistance[2]{};   // smoothed distances: main HUD, interaction prompt
float handOutScale[2]{1.0f,1.0f};         // out-of-view scale while a hand is in front: main HUD, prompt
}
namespace cvr::hud {
void Quad::RetireCompleted(uint64_t completed) {
    for(auto& slot:copies) if(slot.frame && slot.fenceValue<=completed) slot.frame.reset();
}
bool Quad::Ensure(XrSession session, ID3D12Device* device, uint32_t w, uint32_t h) {
    if(submissionFailed)return false;
    // THE SWAPCHAIN IS NOT REMADE FOR A CANVAS THAT FITS. It used to be made for each canvas size, and the minimap's
    // canvas has two (the map alone in the HUD, 380 x 386; projected in a car with the quest band under it, 380 x 594):
    // every switch between them destroyed and created it (2026-11-14: 85-95 ms frames while it flickered). Now it
    // grows to the largest canvas seen, both ways at once, and a smaller canvas goes into its corner.
    if (swapchain && w<=capacityW && h<=capacityH) return true;
    w=std::max(w,capacityW);h=std::max(h,capacityH);
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
    // Transparent rows and columns round a smaller canvas, never an older, larger one's: a committed resource is
    // created zeroed.
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=w;desc.Height=h;
    desc.DepthOrArraySize=1;desc.MipLevels=1;desc.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;desc.SampleDesc.Count=1;
    blank.Reset();
    if (FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_SOURCE,nullptr,IID_PPV_ARGS(&blank)))) return false;
    imageContent.assign(count,0);
    capacityW=w; capacityH=h; width=height=0; lastStamp=0;publishedTick=0;publishedMasked=publishedLootVisible=false;
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
    // A canvas the game's queue has not finished yet is not copied on another queue: the image shown keeps the last.
    if(frame->fence && frame->fence->GetCompletedValue()<frame->serial)return lastStamp!=0;
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
        const auto canvas=frame->canvas->GetDesc();
        const uint64_t size=uint64_t(canvas.Width)|(uint64_t(canvas.Height)<<32);
        auto* target=images[acquiredIndex].texture;
        if(canvas.Width==capacityW && canvas.Height==capacityH) commands->CopyResource(target,frame->canvas.Get());
        else {
            if(imageContent[acquiredIndex]!=size) commands->CopyResource(target,blank.Get());
            D3D12_TEXTURE_COPY_LOCATION dst{};dst.pResource=target;dst.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            D3D12_TEXTURE_COPY_LOCATION src{};src.pResource=frame->canvas.Get();src.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            commands->CopyTextureRegion(&dst,0,0,0,&src,nullptr);
        }
        imageContent[acquiredIndex]=size;
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
    {const auto shown=frame->canvas->GetDesc();width=static_cast<uint32_t>(shown.Width);height=shown.Height;}
    publishedMasked=frame->masked;publishedLootVisible=frame->lootVisible;
    publishedHudRect=frame->hudRect;publishedWindshieldMap=frame->windshieldMap;publishedWindshieldSplit=frame->windshieldSplit;
    return true;
}

const XrCompositionLayerBaseHeader* Quad::Prepare(XrSession session, XrSpace space,
    ID3D12Device* device, ID3D12CommandQueue* queue, bool enabled,
    bool tracked, const XrPosef& head, uint64_t origin,
    XrTime displayTime, const XrVector3f* eyeOffsets, float bodyYaw,
    const XrCompositionLayerBaseHeader* mainLayer,const XrCompositionLayerBaseHeader* mainSecondEye,bool completedSource) {
    secondEye=false;
    if(channel==Channel::VehicleMinimap)CyberpunkVR_VehicleMinimapProjected.store(0,std::memory_order_relaxed);
    // THE SCANNER STAYS AT INFINITY with stereo depth on. Its reticle marks what is scanned, wherever that
    // is, and its quickhack list and key hints are drawn in the eye with no parallax; at the panel's
    // depth the reticle and the details sat on the target differently in each eye (17 px apart at 1.5 m on
    // the simulator, 2026-10-18) and split from the in-eye half of the scanner.
    const bool stereoDepth=g_liveControls.xrHudStereoDepth && channel!=Channel::Surveillance;
    if (!enabled || !g_liveControls.xrHudPanel || (channel==Channel::Interaction && !g_liveControls.xrInteractionPanel)) {
        if(fence && !submissionFailed)RetireCompleted(fence->GetCompletedValue());
        // Switched off: the game's slots come back. A menu over the world only pauses the channel.
        if(!g_liveControls.xrHudPanel || (channel==Channel::Interaction && !g_liveControls.xrInteractionPanel))DropConsumer(channel);
        else SetConsumerReady(false,channel);
        follow.Reset(); clock.Reset(); return nullptr;
    }
    auto frame=Latest(channel,completedSource);
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
        DropConsumer(channel); return nullptr;
    }
    if(!UpdateImage(frame,queue,CyberpunkVR_HudSubmitRing.load(std::memory_order_relaxed)!=0) ||
       !lastStamp || GetTickCount64()-publishedTick>=300) {
        if(submissionFailed)DropConsumer(channel); else SetConsumerReady(false,channel);
        return nullptr;
    }
    if(channel==Channel::VehicleMinimap) {
        VehicleMinimapAnchor cabin{};
        const bool projected=tracked && cvr::anim::WheelReadMinimapAnchor(origin,cabin);
        const bool ordinary=mainLayer && mainLayer->type==XR_TYPE_COMPOSITION_LAYER_QUAD;
        if(!projected && !ordinary) {DropConsumer(channel);return nullptr;} // nowhere to show it: the game's own map
        // Do not briefly move the transparent windshield image back into the ordinary HUD on exit (or the
        // opaque HUD image onto the glass on entry). Keep the consumer ready while the new style reaches us.
        if(projected!=publishedWindshieldMap) {SetConsumerReady(true,channel);return nullptr;}
        layer={XR_TYPE_COMPOSITION_LAYER_QUAD};
        layer.layerFlags=XR_COMPOSITION_LAYER_BLEND_TEXTURE_SOURCE_ALPHA_BIT;
        layer.space=space;layer.eyeVisibility=XR_EYE_VISIBILITY_BOTH;
        layer.subImage={swapchain,{{0,0},{static_cast<int32_t>(width),static_cast<int32_t>(height)}},0};
        if(projected) {
            // The map at its placement's size (a car: about 8 x 5 degrees at the 1.4 m image distance), the quest
            // text under it: the image grows downward and the map stays where it was.
            const auto place=MinimapPlacementFor(cabin.bike);
            const float split=publishedWindshieldSplit>.05f && publishedWindshieldSplit<=1.0f ? publishedWindshieldSplit : 1.0f;
            const float height=place.height/split;
            const auto down=RotateVector(cabin.pose.orientation,{0,-(height-place.height)*.5f,0});
            layer.pose=cabin.pose;
            layer.pose.position.x+=down.x;layer.pose.position.y+=down.y;layer.pose.position.z+=down.z;
            layer.size={place.width,height};
        } else {
            auto place=[&](const XrCompositionLayerBaseHeader* source,XrCompositionLayerQuad& target) {
                const auto& main=*reinterpret_cast<const XrCompositionLayerQuad*>(source);
                target.pose=main.pose;target.eyeVisibility=main.eyeVisibility;
                const auto offset=RotateVector(main.pose.orientation,
                    {(publishedHudRect.x+publishedHudRect.width*.5f-.5f)*main.size.width,
                     (.5f-publishedHudRect.y-publishedHudRect.height*.5f)*main.size.height,0});
                target.pose.position.x+=offset.x;target.pose.position.y+=offset.y;target.pose.position.z+=offset.z;
                target.size={publishedHudRect.width*main.size.width,publishedHudRect.height*main.size.height};
            };
            place(mainLayer,layer);
            if(mainSecondEye && mainSecondEye->type==XR_TYPE_COMPOSITION_LAYER_QUAD) {
                rightLayer=layer;place(mainSecondEye,rightLayer);secondEye=true;
            }
        }
        const bool valid=Submittable(layer) && (!secondEye || Submittable(rightLayer));
        SetConsumerReady(valid,channel);
        if(!valid || !publishedMasked) {secondEye=false;return nullptr;}
        CyberpunkVR_VehicleMinimapProjected.store(projected?1u:0u,std::memory_order_relaxed);
        return reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layer);
    }
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
    // A zero interaction or loot cone is the head-locked panel, like the hit marker's.
    const bool headLocked=channel==Channel::Basilisk || channel==Channel::Surveillance || channel==Channel::HitMarker ||
        (channel==Channel::Interaction && degrees<=0.0f);
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
        float distance=std::clamp(static_cast<float>(channel==Channel::Interaction?g_liveControls.xrInteractionDistance:g_liveControls.xrHudDistance),0.5f,5.0f);
        // The prompt at the depth of its object (Runtimes/InteractionDepth.hpp). 10 cm in front of it so it
        // never sinks into a door; clamped to what is comfortable to read.
        if(channel==Channel::Interaction && CyberpunkVR_InteractionAnchor) {
            const float depth=cvr::interaction::TargetDepth(GetTickCount64());
            if(depth>0.0f) {
                anchorDistance=cvr::interaction::StepDistance(anchorDistance,std::clamp(depth-0.10f,0.35f,6.0f),dt);
                distance=anchorDistance;
            } else anchorDistance=0.0f;
        }
        float configuredDistance=0.0f;   // set when the clearance moved this panel (an anchored prompt keeps its size)
        // The nearest tracked hand inside this panel's field of view (Runtimes/HudClearance.hpp, HandFront).
        float handFront=cvr::hud::clearance::kFree;
        if(CyberpunkVR_HudClearance && CyberpunkVR_HudClearanceHands && (channel==Channel::Main || channel==Channel::Interaction)) {
            const float panelFov=std::clamp(static_cast<float>(channel==Channel::Interaction?g_liveControls.xrInteractionFov:g_liveControls.xrHudFov),30.0f,120.0f)*0.01745329252f;
            const float tanV=tanf(panelFov*0.5f),tanH=tanV*float(width)/float(height);
            for(int h=0;h<2;++h) {
                OpenXRHeadPose hand{};
                if(OpenXRManager::Get().GetHandPose(h,&hand))
                    handFront=std::min(handFront,cvr::hud::clearance::HandFront(hand.posX,hand.posY,hand.posZ,tanH,tanV));
            }
        }
        // Then in front of the world, never inside it (Runtimes/HudClearance.hpp): the main HUD and the prompt.
        // An anchored prompt already glides with its object, so it only takes the limit, unsmoothed: something
        // nearer than its object still wins, and the clearance's slow way out cannot hold it back.
        if(channel==Channel::Interaction && anchorDistance>0.0f && CyberpunkVR_HudClearance) {
            distance=cvr::hud::clearance::Limit(distance,cvr::hud::clearance::Published(GetTickCount64()));
            clearanceDistance[1]=distance;
        } else if((channel==Channel::Main || channel==Channel::Interaction) && CyberpunkVR_HudClearance) {
            configuredDistance=distance;
            float& current=clearanceDistance[channel==Channel::Main?0:1];
            const float limit=cvr::hud::clearance::Limit(distance,cvr::hud::clearance::Published(GetTickCount64()));
            current=cvr::hud::clearance::Step(current,limit,dt);
            distance=current;
        }
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
        // Out of view while a hand is in front and nearer than the panel (Runtimes/HudClearance.hpp, OutStep).
        float handOut=1.0f;
        if(channel==Channel::Main || channel==Channel::Interaction) {
            float& scale=handOutScale[channel==Channel::Main?0:1];
            scale=cvr::hud::clearance::OutStep(scale,handFront<distance,dt,std::clamp(CyberpunkVR_HudClearanceHandsScale,1.0f,3.0f));
            handOut=scale;
        }
        const float h=2.0f*distance*tanf(fov*0.5f)*
            (configuredDistance>0.0f ? cvr::hud::clearance::GrowFactor(configuredDistance,distance,CyberpunkVR_HudClearanceGrow) : 1.0f)*
            handOut;
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
    // A bad optional quad must not reject the world submission. Validate after
    // the eye offsets, including cached poses used during tracking loss.
    const auto* invalid=!Submittable(layer)?&layer:
        (secondEye && !Submittable(rightLayer)?&rightLayer:nullptr);
    if(invalid) {
        static bool logged=false;
        if(!logged) {
            logged=true;
            const auto& q=invalid->pose.orientation;const auto& p=invalid->pose.position;
            Log("[hud-panel] channel %d invalid quad dropped: eye %d orientation (%g,%g,%g,%g) "
                "position (%g,%g,%g) size %gx%g tracked %d\n",int(channel),int(invalid->eyeVisibility),
                q.x,q.y,q.z,q.w,p.x,p.y,p.z,invalid->size.width,invalid->size.height,int(tracked));
        }
        secondEye=false;
        return nullptr;
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
    if(channel==Channel::VehicleMinimap)CyberpunkVR_VehicleMinimapProjected.store(0,std::memory_order_relaxed);
    DropConsumer(channel);
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
    swapchain=XR_NULL_HANDLE; acquired=false; images.clear(); width=height=capacityW=capacityH=0; lastStamp=0;
    imageContent.clear();blank.Reset(); follow.Reset();clock.Reset();secondEye=false;
    for(auto& slot:copies){slot.frame.reset();slot.commands.Reset();slot.allocator.Reset();slot.fenceValue=0;}
    copyCursor=0;fence.Reset();fenceValue=0;publishedTick=0;publishedMasked=publishedLootVisible=false;submissionFailed=false;
    if (event) CloseHandle(event); event=nullptr;
}
}
