// Per-eye interpolation, motion dilation and extrapolation adapted from iPowerTech/cyberpunk-vr-port.
#include "Framegen/Fsr3.hpp"
#include "Framegen/Framegen.hpp"
#include "Framegen/Fsr3Dilate.hpp"
#include "Framegen/Fsr3Extrapolate.hpp"
#include "Render/Afw.hpp"
#include "Utils/DebugGate.hpp"


#include <windows.h>
#include <d3d12.h>
#include <FidelityFX/host/ffx_frameinterpolation.h>
#include <FidelityFX/host/ffx_opticalflow.h>
#include <FidelityFX/host/backends/dx12/ffx_dx12.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <vector>

extern void Log(const char* fmt, ...);


namespace cvr::fsr3 {
namespace {
using Microsoft::WRL::ComPtr;

std::mutex settingsMutex;
Settings settings;
std::atomic<bool> enabled{false},opticalFlow{true},faulted{false},show{true};
std::atomic<int> mode{kInterpolation};

std::mutex statusMutex;
Status status;
void SetText(const char* text) {std::lock_guard lock(statusMutex);std::snprintf(status.text,sizeof(status.text),"%s",text);}

std::atomic<double> generationMs{0};   // the dispatch's GPU time, both eyes, smoothed: GenerationMs()

// ---- FidelityFX: one backend, per eye a frame interpolation and an optical flow context ----------
void* scratch=nullptr;
FfxInterface backend{};
bool backendReady=false;
FfxFrameInterpolationContext fiContext[2]{};
FfxOpticalflowContext ofContext[2]{};
bool fiValid[2]{},ofValid[2]{};
// Grow-only: the engine's dynamic resolution moves the render size, and a context dispatched beyond
// the size it was created for writes past its own buffers (the fork's first GPU hang).
uint32_t contextRenderW=0,contextRenderH=0,contextDisplayW=0,contextDisplayH=0;
bool firstDispatch[2]{};
ComPtr<ID3D12Resource> ofVector[2],ofScd[2];
int ofVectorW[2]{},ofVectorH[2]{};

void DestroyContexts() {
    for(int eye=0;eye<2;++eye) {
        if(fiValid[eye])ffxFrameInterpolationContextDestroy(&fiContext[eye]);
        if(ofValid[eye])ffxOpticalflowContextDestroy(&ofContext[eye]);
        fiValid[eye]=ofValid[eye]=false;
    }
}

bool EnsureContexts(ID3D12Device* device,uint32_t renderW,uint32_t renderH,uint32_t displayW,uint32_t displayH,bool inverted,bool infinite) {
    if(!backendReady) {
        // 2 eyes x (frame interpolation + optical flow).
        const size_t size=ffxGetScratchMemorySizeDX12(4);
        scratch=std::malloc(size);
        if(!scratch)return false;
        if(ffxGetInterfaceDX12(&backend,ffxGetDeviceDX12(device),scratch,size,4)!=FFX_OK) {
            std::free(scratch);scratch=nullptr;Log("[fsr3] ffxGetInterfaceDX12 failed\n");return false;
        }
        backendReady=true;
    }
    if(fiValid[0] && fiValid[1] && renderW<=contextRenderW && renderH<=contextRenderH &&
       displayW<=contextDisplayW && displayH<=contextDisplayH)return true;
    renderW=std::max(renderW,contextRenderW);renderH=std::max(renderH,contextRenderH);
    displayW=std::max(displayW,contextDisplayW);displayH=std::max(displayH,contextDisplayH);
    DestroyContexts();
    for(int eye=0;eye<2;++eye) {
        FfxFrameInterpolationContextDescription desc{};
        desc.backendInterface=backend;
        // The captures are the displayed SDR image (sRGB-encoded bytes), not scene-linear HDR.
        desc.flags=(inverted?FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED:0u)|(infinite?FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INFINITE:0u);
        desc.maxRenderSize={renderW,renderH};desc.displaySize={displayW,displayH};
        desc.backBufferFormat=FFX_SURFACE_FORMAT_R8G8B8A8_UNORM;
        desc.previousInterpolationSourceFormat=desc.backBufferFormat;
        if(ffxFrameInterpolationContextCreate(&fiContext[eye],&desc)!=FFX_OK) {
            Log("[fsr3] frame interpolation context eye %d failed\n",eye);DestroyContexts();return false;
        }
        fiValid[eye]=true;
        FfxOpticalflowContextDescription of{};
        of.backendInterface=backend;of.resolution={displayW,displayH};
        ofValid[eye]=ffxOpticalflowContextCreate(&ofContext[eye],&of)==FFX_OK;
        if(!ofValid[eye])Log("[fsr3] optical flow context eye %d failed: game motion vectors only\n",eye);
        // No history yet: the first dispatch after a (re)create resets.
        firstDispatch[eye]=true;
    }
    contextRenderW=renderW;contextRenderH=renderH;contextDisplayW=displayW;contextDisplayH=displayH;
    Log("[fsr3] contexts (re)created: render %ux%u display %ux%u\n",renderW,renderH,displayW,displayH);
    return true;
}

ID3D12Resource* MakeTexture(ID3D12Device* device,uint64_t w,uint32_t h,DXGI_FORMAT format,D3D12_RESOURCE_STATES state) {
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=w;d.Height=h;d.DepthOrArraySize=1;
    d.MipLevels=1;d.Format=format;d.SampleDesc.Count=1;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    ID3D12Resource* r=nullptr;
    return SUCCEEDED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r))) ? r : nullptr;
}

// Optical flow's two outputs, sized as ffxOpticalflowGetSharedResourceDescriptions documents: one
// vector per 8x8 block of the display, and a fixed 3x1 scene change buffer.
bool EnsureOpticalFlow(ID3D12Device* device,int eye,int displayW,int displayH) {
    const int w=(displayW+7)/8,h=(displayH+7)/8;
    if(!ofVector[eye] || w>ofVectorW[eye] || h>ofVectorH[eye]) {
        ofVector[eye].Attach(MakeTexture(device,uint64_t(std::max(w,ofVectorW[eye])),uint32_t(std::max(h,ofVectorH[eye])),
            DXGI_FORMAT_R16G16_SINT,D3D12_RESOURCE_STATE_UNORDERED_ACCESS));
        if(!ofVector[eye])return false;
        ofVectorW[eye]=std::max(w,ofVectorW[eye]);ofVectorH[eye]=std::max(h,ofVectorH[eye]);
    }
    if(!ofScd[eye])ofScd[eye].Attach(MakeTexture(device,3,1,DXGI_FORMAT_R32_UINT,D3D12_RESOURCE_STATE_UNORDERED_ACCESS));
    return ofScd[eye]!=nullptr;
}

FfxResource Wrap(ID3D12Resource* resource,FfxResourceStates state,FfxSurfaceFormat format=FFX_SURFACE_FORMAT_UNKNOWN) {
    auto desc=ffxGetResourceDescriptionDX12(resource);
    if(format!=FFX_SURFACE_FORMAT_UNKNOWN)desc.format=format;
    return ffxGetResourceDX12(resource,desc,nullptr,state);
}

// ---- the interpolated frames, per eye, tagged with the capture they lead to --------------------
constexpr int kPoolSlots=3;
struct Slot {ComPtr<ID3D12Resource> texture;uint64_t serial{};};
std::mutex poolMutex;
Slot pool[2][kPoolSlots];
int poolNext[2]{};
// Generate retires consumers before changing the layout of this pool.


ID3D12Resource* PoolSlot(ID3D12Device* device,int eye,const D3D12_RESOURCE_DESC& color) {
    auto& slot=pool[eye][poolNext[eye]];
    if(slot.texture) {
        // About to be overwritten: nothing may ask for what it held.
        std::lock_guard lock(poolMutex);slot.serial=0;
        const auto d=slot.texture->GetDesc();
        if(d.Width==color.Width && d.Height==color.Height)return slot.texture.Get();
        slot={};
    }
    // The captures' format family, typeless as the second eye's captures are: the XR loop copies it
    // into the swapchain image or reads it through its sRGB decode exactly like a capture.
    ComPtr<ID3D12Resource> texture;
    texture.Attach(MakeTexture(device,color.Width,color.Height,DXGI_FORMAT_R8G8B8A8_TYPELESS,D3D12_RESOURCE_STATE_COPY_SOURCE));
    if(!texture)return nullptr;
    texture->SetName(L"CVR_FSR3_Interpolated");
    std::lock_guard lock(poolMutex);slot.texture=texture;slot.serial=0;
    return texture.Get();
}

// ---- our own command lists, on our own compute queue ---------------------------------------------
// COMPUTE, not DIRECT: everything recorded here -- the dilate pass, FidelityFX's frame interpolation and optical
// flow (their DX12 backend records dispatches, copies and barriers only) -- is compute work, and on a compute
// queue the GPU can run it alongside the game's graphics work. On the headset's DIRECT queue it took turns with
// the game's: fork67, same place, game 35-40 fps with FSR3 off, 33-34 with it interpolating but showing nothing.
ComPtr<ID3D12CommandQueue> computeQueue;
constexpr int kLists=3;
struct List {
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;
    uint64_t fence{};
    bool timed{};   // its two timestamps were resolved into the readback buffer
    std::shared_ptr<const framegen::Inputs> inputs[2];
    std::shared_ptr<const void> captureLease;
};
List lists[kLists];
int listNext=0;
ComPtr<ID3D12Fence> fence;
uint64_t fenceValue=0;
// Five timestamps per list -- start, then per eye after its dilate and after its interpolation (the last one
// is the end) -- read back once the list's fence has passed. Elapsed GPU time: whatever else the GPU runs in
// between (other queues, other processes) is counted in it.
constexpr int kStamps=5;
ComPtr<ID3D12QueryHeap> timestamps;
ComPtr<ID3D12Resource> timestampReadback;
double ticksToMs=0;
bool EnsureLists(ID3D12Device* device) {
    if(fence)return true;
    if(!computeQueue) {
        D3D12_COMMAND_QUEUE_DESC q{};q.Type=D3D12_COMMAND_LIST_TYPE_COMPUTE;q.Priority=D3D12_COMMAND_QUEUE_PRIORITY_NORMAL;
        if(FAILED(device->CreateCommandQueue(&q,IID_PPV_ARGS(&computeQueue))))return false;
        computeQueue->SetName(L"CVR_FSR3_Compute");
    }
    for(auto& l:lists) {
        if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COMPUTE,IID_PPV_ARGS(&l.allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_COMPUTE,l.allocator.Get(),nullptr,IID_PPV_ARGS(&l.list))))return false;
        l.list->Close();
    }
    D3D12_QUERY_HEAP_DESC qh{};qh.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;qh.Count=kLists*kStamps;
    D3D12_HEAP_PROPERTIES rb{};rb.Type=D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC rd{};rd.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;rd.Width=kLists*kStamps*sizeof(uint64_t);rd.Height=1;
    rd.DepthOrArraySize=1;rd.MipLevels=1;rd.SampleDesc.Count=1;rd.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if(FAILED(device->CreateQueryHeap(&qh,IID_PPV_ARGS(&timestamps))) ||
       FAILED(device->CreateCommittedResource(&rb,D3D12_HEAP_FLAG_NONE,&rd,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&timestampReadback))))
        timestamps.Reset();
    return SUCCEEDED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));
}

// The list at `index` has left the GPU: add its time to the report.
void ReadTimestamps(int index,ID3D12CommandQueue* queue) {
    auto& l=lists[index];
    if(!l.timed || !timestampReadback)return;
    l.timed=false;
    if(!ticksToMs) {UINT64 frequency=0;if(FAILED(queue->GetTimestampFrequency(&frequency)) || !frequency)return;ticksToMs=1000.0/double(frequency);}
    const D3D12_RANGE range{size_t(index)*kStamps*8,size_t(index+1)*kStamps*8};
    void* data=nullptr;
    if(FAILED(timestampReadback->Map(0,&range,&data)))return;
    const auto* t=static_cast<const uint64_t*>(data)+index*kStamps;
    if(t[0]<=t[1] && t[1]<=t[2] && t[2]<=t[3] && t[3]<=t[4] && t[4]>t[0]) {
        const double ms=double(t[4]-t[0])*ticksToMs,previous=generationMs.load();
        generationMs=previous>0 ? previous*0.9+ms*0.1 : ms;
        framegen::OnGeneration(ms,ms);
    }
    const D3D12_RANGE none{0,0};
    timestampReadback->Unmap(0,&none);
}

void Transition(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};list->ResourceBarrier(1,&b);
}

// What the previous capture was, for "does this one follow it".
std::atomic<uint64_t> lastSerial{0};   // 0 = nothing to follow (also set from the settings thread)
uint64_t lastOrigin=0;
uint32_t lastFrame=0;

void Count(uint64_t Status::*counter) {std::lock_guard lock(statusMutex);++(status.*counter);}

// A TURN THE MOTION DOES NOT CONTINUE. A snap turn moves the engine camera by the whole snap angle
// from one frame to the next and the engine does not mark it as a cut: the motion vectors carry it as
// motion, so extrapolation carries the view on past the snap for one frame (the likely cause of a
// glitch seen in the headset on snap turns only: the report's snap lines confirm it or rule it out),
// and interpolation shows a frame halfway round. So after each snap turn
// handed to the game (NoteSnapTurn), a capture among the next few whose camera turned more than half
// the snap is treated as a cut. Only then: a fast head turn alone is never one, whatever its speed.
constexpr int kSnapCaptures=6;   // the snap shows a frame or two after it is handed over, plus the copies queued
std::atomic<uint32_t> snapCount{};
std::atomic<float> snapDeg{};
uint32_t snapSeen=0;
int snapCapturesLeft=0;
float snapThresholdDeg=0;
bool snapFound=false;
float lastForward[3]{};
uint32_t lastCameraFrame=0;
uint64_t lastCameraOrigin=0;
std::atomic<uint32_t> snapsNoted{},cameraJumps{},snapsNotFound{};

// The engine camera's turn since the capture before, degrees; 0 with none to compare against (the first,
// another tracking space, or more than a few frames apart).
float CameraTurnDeg(const framegen::Inputs& in) {
    const float* f=in.camera.forward;
    const float length=std::sqrt(f[0]*f[0]+f[1]*f[1]+f[2]*f[2]);
    const uint32_t gap=in.frameId-lastCameraFrame;
    float turn=0;
    if(length>0 && lastCameraFrame && in.origin==lastCameraOrigin && gap>=1 && gap<=4) {
        const float dot=(f[0]*lastForward[0]+f[1]*lastForward[1]+f[2]*lastForward[2])/length;
        turn=std::acos(std::clamp(dot,-1.0f,1.0f))*57.29578f;
    }
    if(length>0)for(int i=0;i<3;++i)lastForward[i]=f[i]/length;
    lastCameraFrame=in.frameId;lastCameraOrigin=in.origin;
    return turn;
}
bool CameraJumped(const framegen::Inputs& in) {
    const float turn=CameraTurnDeg(in);
    const uint32_t count=snapCount.load();
    if(count!=snapSeen) {
        snapSeen=count;snapCapturesLeft=kSnapCaptures;snapThresholdDeg=0.5f*snapDeg.load();snapFound=false;
    }
    if(snapCapturesLeft<=0)return false;
    --snapCapturesLeft;
    if(turn>snapThresholdDeg) {
        snapFound=true;++cameraJumps;
        if(cvr::RuntimeDiagnosticsEnabled())Log("[fsr3] snap turn: camera turned %.1f deg from the capture before (frame %u, capture %d of %d after "
            "the snap): a cut, nothing generated\n",turn,in.frameId,kSnapCaptures-snapCapturesLeft,kSnapCaptures);
        return true;
    }
    if(snapCapturesLeft==0 && !snapFound) {
        ++snapsNotFound;
        if(cvr::RuntimeDiagnosticsEnabled())Log("[fsr3] snap turn of %.0f deg: no capture among the %d after it turned more than %.1f deg\n",
            2*snapThresholdDeg,kSnapCaptures,snapThresholdDeg);
    }
    return false;
}

void InterpolateUnguarded(ID3D12Device* device,const GpuWait (&waits)[2],ID3D12Resource* const colors[2],
    const std::shared_ptr<const framegen::Inputs> (&inputs)[2],uint64_t serial,double frameMs) {
    for(int eye=0;eye<2;++eye) {
        const auto* in=inputs[eye].get();
        if(!colors[eye] || !in || !in->hasDepth || !in->hasMotion || !in->depth || !in->motion) {
            lastSerial=0;Count(&Status::skipped);SetText("Waiting for depth and motion vectors in both eyes");return;
        }
        if(in->motionWidth!=in->width || in->motionHeight!=in->height) {
            lastSerial=0;Count(&Status::skipped);SetText("Motion vectors are not at the render resolution: not supported");return;
        }
    }
    const auto& camera=inputs[0]->camera;
    const auto color=colors[0]->GetDesc();
    if(!EnsureLists(device) || !EnsureContexts(device,inputs[0]->width,inputs[0]->height,uint32_t(color.Width),color.Height,
        camera.inverted,camera.infinite) || !Fsr3Dilate_Init(device)) {
        faulted=true;SetText("FSR3 could not start: see cyberpunkvrport.log");return;
    }
    {std::lock_guard lock(statusMutex);status.ready=true;}
    auto* queue=computeQueue.Get();
    // A capture that follows the previous one in the same tracking space interpolates; any other only
    // primes the history (reset) and shows nothing in between.
    const uint64_t previous=lastSerial.load();
    const bool jumped=CameraJumped(*inputs[0]);
    const bool follows=!jumped && previous && serial==previous+1 && inputs[0]->origin==lastOrigin &&
        inputs[0]->frameId==lastFrame+1 && inputs[1]->frameId==inputs[0]->frameId &&
        !inputs[0]->camera.reset && !inputs[1]->camera.reset && frameMs>0 && frameMs<=100;
    lastSerial=serial;lastOrigin=inputs[0]->origin;lastFrame=inputs[0]->frameId;

    const int index=listNext;
    auto& l=lists[index];listNext=(listNext+1)%kLists;
    if(l.fence && fence->GetCompletedValue()<l.fence) {
        Count(&Status::skipped);lastSerial=0;return;
    }
    ReadTimestamps(index,queue);
    l.inputs[0]=inputs[0];l.inputs[1]=inputs[1];
    if(FAILED(l.allocator->Reset()) || FAILED(l.list->Reset(l.allocator.Get(),nullptr))) {faulted=true;SetText("FSR3 command list failed");return;}
    auto* list=l.list.Get();
    // Always, not only for the report: GenerationMs() decides whether an interpolated frame fits (OpenXRFrameLoop.cpp).
    const bool timed=timestamps!=nullptr;
    const auto stamp=[&](int at) {if(timed)list->EndQuery(timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,UINT(index*kStamps+at));};
    stamp(0);
    const bool flow=opticalFlow.load(std::memory_order_relaxed);
    bool ok=true;
    for(int eye=0;eye<2 && ok;++eye) {
        const auto& in=*inputs[eye];
        const auto desc=colors[eye]->GetDesc();
        const int displayW=int(desc.Width),displayH=int(desc.Height);
        ID3D12Resource* output=PoolSlot(device,eye,desc);
        if(!output) {ok=false;break;}
        bool reset=!follows;
        if(firstDispatch[eye]) {reset=true;firstDispatch[eye]=false;}
        // The engine's vectors are UV offsets to the previous frame once its own scale is applied.
        const float scaleX=in.camera.motionScale[0]!=0 ? in.camera.motionScale[0] : 1.0f;
        const float scaleY=in.camera.motionScale[1]!=0 ? in.camera.motionScale[1] : 1.0f;
        const auto* dilated=Fsr3Dilate_Run(list,eye,in.depth.Get(),in.motion.Get(),int(in.width),int(in.height),scaleX,scaleY,in.camera.inverted);
        if(!dilated) {ok=false;break;}
        stamp(1+eye*2);
        Transition(list,colors[eye],D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        Transition(list,output,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        // Optical flow first, from the same color, as AMD's own FSR3 orchestration does.
        bool haveFlow=false;
        if(flow && ofValid[eye] && EnsureOpticalFlow(device,eye,displayW,displayH)) {
            FfxOpticalflowDispatchDescription of{};
            of.commandList=ffxGetCommandListDX12(list);
            of.color=Wrap(colors[eye],FFX_RESOURCE_STATE_COMPUTE_READ,FFX_SURFACE_FORMAT_R8G8B8A8_UNORM);
            of.opticalFlowVector=Wrap(ofVector[eye].Get(),FFX_RESOURCE_STATE_UNORDERED_ACCESS);
            of.opticalFlowSCD=Wrap(ofScd[eye].Get(),FFX_RESOURCE_STATE_UNORDERED_ACCESS);
            of.reset=reset;of.backbufferTransferFunction=FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;of.minMaxLuminance={0,1};
            haveFlow=ffxOpticalflowContextDispatch(&ofContext[eye],&of)==FFX_OK;
        }
        FfxFrameInterpolationDispatchDescription d{};
        d.commandList=ffxGetCommandListDX12(list);
        d.displaySize={uint32_t(displayW),uint32_t(displayH)};d.renderSize={in.width,in.height};
        d.currentBackBuffer=Wrap(colors[eye],FFX_RESOURCE_STATE_COMPUTE_READ,FFX_SURFACE_FORMAT_R8G8B8A8_UNORM);
        d.output=Wrap(output,FFX_RESOURCE_STATE_UNORDERED_ACCESS,FFX_SURFACE_FORMAT_R8G8B8A8_UNORM);
        d.dilatedDepth=Wrap(dilated->dilatedDepth,FFX_RESOURCE_STATE_UNORDERED_ACCESS);
        d.dilatedMotionVectors=Wrap(dilated->dilatedMotionVectors,FFX_RESOURCE_STATE_UNORDERED_ACCESS);
        d.reconstructedPrevDepth=Wrap(dilated->reconstructPrevNearDepth,FFX_RESOURCE_STATE_UNORDERED_ACCESS);
        d.interpolationRect={0,0,displayW,displayH};
        if(haveFlow) {
            d.opticalFlowVector=Wrap(ofVector[eye].Get(),FFX_RESOURCE_STATE_UNORDERED_ACCESS);
            d.opticalFlowSceneChangeDetection=Wrap(ofScd[eye].Get(),FFX_RESOURCE_STATE_UNORDERED_ACCESS);
            d.opticalFlowBufferSize={uint32_t((displayW+7)/8),uint32_t((displayH+7)/8)};
            d.opticalFlowScale={1.0f/float(displayW),1.0f/float(displayH)};
        }
        d.opticalFlowBlockSize=8;
        d.cameraNear=in.camera.nearPlane;d.cameraFar=in.camera.farPlane;
        d.cameraFovAngleVertical=in.camera.verticalFov;d.viewSpaceToMetersFactor=1.0f;
        d.frameID=serial;d.frameTimeDelta=float(frameMs);d.reset=reset;
        d.backBufferTransferFunction=FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;d.minMaxLuminance[0]=0;d.minMaxLuminance[1]=1;
        const auto result=ffxFrameInterpolationDispatch(&fiContext[eye],&d);
        Transition(list,output,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE);
        Transition(list,colors[eye],D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_SOURCE);
        stamp(2+eye*2);
        if(result!=FFX_OK) {
            static int logs=0;if(logs++<8)Log("[fsr3] dispatch eye %d failed 0x%08X\n",eye,unsigned(result));
            ok=false;
        }
    }
    // Both eyes recorded: all five timestamps were written.
    if(timed && ok)list->ResolveQueryData(timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,UINT(index*kStamps),kStamps,
        timestampReadback.Get(),uint64_t(index)*kStamps*8);
    l.timed=timed && ok;
    list->Close();
    // The barriers above leave every resource as it was found, so even a failed eye is submitted.
    for(const auto& wait:waits)if(wait.fence && wait.value)queue->Wait(wait.fence,wait.value);
    for(const auto& input:inputs)if(input->readyFence && input->fenceValue)queue->Wait(input->readyFence.Get(),input->fenceValue);
    ID3D12CommandList* submit[]={list};queue->ExecuteCommandLists(1,submit);
    l.fence=++fenceValue;queue->Signal(fence.Get(),l.fence);

    if(!ok) {lastSerial=0;Count(&Status::skipped);SetText("FSR3 dispatch failed: see cyberpunkvrport.log");return;}
    if(!follows) {Count(&Status::primed);SetText("Priming: waiting for two consecutive frames");return;}
    {
        std::lock_guard lock(poolMutex);
        for(int eye=0;eye<2;++eye) {pool[eye][poolNext[eye]].serial=serial;poolNext[eye]=(poolNext[eye]+1)%kPoolSlots;}
    }
    Count(&Status::interpolated);
    SetText(flow ? "Interpolating: engine motion vectors and depth, optical flow" : "Interpolating: engine motion vectors and depth");
}

// HALF A GAME FRAME AHEAD: the headset shows the extrapolated frame half the game's frame interval after its real one
// (OpenXRFrameLoop.cpp), and the pose it goes with is moved as far (Framegen/Presenter.cpp). A fixed half for the first
// test: whether the direction and the amount are right is what it is for.
constexpr float kExtrapolationFactor=0.5f;

bool ExtrapolateUnguarded(ID3D12Device* device,const GpuWait (&waits)[2],ID3D12Resource* const colors[2],
    const std::shared_ptr<const framegen::Inputs> (&inputs)[2],uint64_t serial) {
    for(int eye=0;eye<2;++eye) {
        const auto* in=inputs[eye].get();
        if(!colors[eye] || !in || !in->hasMotion || !in->motion || !in->depth) {
            Count(&Status::skipped);SetText("Waiting for depth and motion vectors in both eyes");return false;
        }
        if(in->motionWidth!=in->width || in->motionHeight!=in->height) {
            Count(&Status::skipped);SetText("Motion vectors are not at the render resolution: not supported");return false;
        }
        // A cut: the vectors lead from a picture that is not the one before. Nothing to move ahead.
        if(in->camera.reset)return false;
    }
    if(CameraJumped(*inputs[0]))return false;
    if(!EnsureLists(device) || !Fsr3Dilate_Init(device) || !Fsr3Extrapolate_Init(device)) {
        faulted=true;SetText("FSR3 could not start: see cyberpunkvrport.log");return false;
    }
    {std::lock_guard lock(statusMutex);status.ready=true;}
    auto* queue=computeQueue.Get();
    const int index=listNext;
    auto& l=lists[index];listNext=(listNext+1)%kLists;
    if(l.fence && fence->GetCompletedValue()<l.fence) {
        Count(&Status::skipped);return false;
    }
    ReadTimestamps(index,queue);
    l.inputs[0]=inputs[0];l.inputs[1]=inputs[1];
    if(FAILED(l.allocator->Reset()) || FAILED(l.list->Reset(l.allocator.Get(),nullptr))) {faulted=true;SetText("FSR3 command list failed");return false;}
    auto* list=l.list.Get();
    const bool timed=timestamps!=nullptr;
    const auto stamp=[&](int at) {if(timed)list->EndQuery(timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,UINT(index*kStamps+at));};
    stamp(0);
    bool ok=true;
    for(int eye=0;eye<2 && ok;++eye) {
        const auto& in=*inputs[eye];
        const auto desc=colors[eye]->GetDesc();
        ID3D12Resource* output=PoolSlot(device,eye,desc);
        if(!output) {ok=false;break;}
        const float scaleX=in.camera.motionScale[0]!=0 ? in.camera.motionScale[0] : 1.0f;
        const float scaleY=in.camera.motionScale[1]!=0 ? in.camera.motionScale[1] : 1.0f;
        const auto* dilated=Fsr3Dilate_Run(list,eye,in.depth.Get(),in.motion.Get(),int(in.width),int(in.height),scaleX,scaleY,in.camera.inverted);
        if(!dilated) {ok=false;break;}
        stamp(1+eye*2);
        Transition(list,colors[eye],D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        Transition(list,dilated->dilatedMotionVectors,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        Transition(list,output,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        ok=Fsr3Extrapolate_Run(list,colors[eye],dilated->dilatedMotionVectors,output,int(desc.Width),int(desc.Height),
            int(in.width),int(in.height),kExtrapolationFactor);
        Transition(list,output,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE);
        Transition(list,dilated->dilatedMotionVectors,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        Transition(list,colors[eye],D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_SOURCE);
        stamp(2+eye*2);
    }
    if(timed && ok)list->ResolveQueryData(timestamps.Get(),D3D12_QUERY_TYPE_TIMESTAMP,UINT(index*kStamps),kStamps,
        timestampReadback.Get(),uint64_t(index)*kStamps*8);
    l.timed=timed && ok;
    list->Close();
    // The barriers above leave every resource as it was found, so even a failed eye is submitted.
    for(const auto& wait:waits)if(wait.fence && wait.value)queue->Wait(wait.fence,wait.value);
    for(const auto& input:inputs)if(input->readyFence && input->fenceValue)queue->Wait(input->readyFence.Get(),input->fenceValue);
    ID3D12CommandList* submit[]={list};queue->ExecuteCommandLists(1,submit);
    l.fence=++fenceValue;queue->Signal(fence.Get(),l.fence);

    if(!ok) {Count(&Status::skipped);SetText("FSR3 dispatch failed: see cyberpunkvrport.log");return false;}
    {
        std::lock_guard lock(poolMutex);
        for(int eye=0;eye<2;++eye) {pool[eye][poolNext[eye]].serial=serial;poolNext[eye]=(poolNext[eye]+1)%kPoolSlots;}
    }
    Count(&Status::extrapolated);
    SetText("Extrapolating: engine motion vectors, half a game frame ahead");
    return true;
}
}  // namespace

void SetSettings(Settings value) {
    std::lock_guard lock(settingsMutex);
    if(value.enabled && !settings.enabled)faulted=false;
    if(value.enabled!=settings.enabled)Log("[fsr3] frame generation %s\n",value.enabled?"on":"off");
    if(value.mode!=settings.mode) {
        Log("[fsr3] mode %s\n",value.mode==kExtrapolation?"extrapolation":"interpolation");
        lastSerial=0;   // interpolation's history is not kept up while extrapolating: it primes again
    }
    settings=value;
    mode.store(value.mode,std::memory_order_relaxed);
    enabled.store(value.enabled,std::memory_order_relaxed);
    opticalFlow.store(value.opticalFlow,std::memory_order_relaxed);
    show.store(value.show,std::memory_order_relaxed);

    if(!value.enabled) {lastSerial=0;SetText("Off");}
}
void NoteSnapTurn(float degrees) {
    if(!Enabled() || degrees==0)return;
    snapDeg.store(std::abs(degrees));++snapCount;++snapsNoted;
}
// Off for an AFW launch (Render/Afw.hpp): its frames are drawn for alternating eyes, and interpolating between two
// of them would blend pictures of different eyes. The other eye's warp already stands in for an undrawn frame.
bool Enabled() {
    return enabled.load(std::memory_order_relaxed) && !faulted.load(std::memory_order_relaxed) && !cvr::afw::Launch();
}
bool ShowGenerated() {return Enabled() && show.load(std::memory_order_relaxed);}
static void RunInterpolation(ID3D12Device* d,const GpuWait (&w)[2],ID3D12Resource* const c[2],
        const std::shared_ptr<const framegen::Inputs> (&in)[2],uint64_t serial,double frameMs) {
    __try {InterpolateUnguarded(d,w,c,in,serial,frameMs);}
    __except(EXCEPTION_EXECUTE_HANDLER) {faulted=true;SetText("FSR3 interpolation fault");}
}
static void RunExtrapolation(ID3D12Device* d,const GpuWait (&w)[2],ID3D12Resource* const c[2],
        const std::shared_ptr<const framegen::Inputs> (&in)[2],uint64_t serial) {
    __try {ExtrapolateUnguarded(d,w,c,in,serial);}
    __except(EXCEPTION_EXECUTE_HANDLER) {faulted=true;SetText("FSR3 extrapolation fault");}
}
namespace {
struct InputLayout {
    ID3D12Device* device{};
    UINT width{},height{},renderWidth{},renderHeight{};
    bool inverted{},infinite{};
    bool operator==(const InputLayout&) const = default;
};
InputLayout layout;
ComPtr<ID3D12Fence> producerFence, consumerFence;
uint64_t producerValue{}, consumerValue{};
}

// Called by the single presentation owner. A format/size change retires both the compute writes
// and the last headset read before any SDK context, descriptor or output is recycled.
void ConsumerFence(ID3D12Fence* receipt,uint64_t value) {
    if(layout.device && receipt && value) {consumerFence=receipt;consumerValue=value;}
}
void Poll() {
    if(!fence)return;
    const auto completed=fence->GetCompletedValue();
    for(auto& list:lists) if(list.fence<=completed) {
        list.captureLease.reset();
        for(auto& input:list.inputs)input.reset();
    }
}
void ResetHistory() {
    lastSerial=0;
    lastCameraFrame=0;
    std::lock_guard lock(poolMutex);
    for(auto& eye:pool) for(auto& slot:eye) slot.serial=0;
}
bool ReleaseResources() {
    if(!layout.device)return true;
    Poll();
    if((fence && fence->GetCompletedValue()<fenceValue) ||
       (consumerFence && consumerFence->GetCompletedValue()<consumerValue)) return false;
    DestroyContexts();
    Fsr3Dilate_Shutdown();
    Fsr3Extrapolate_Shutdown();
    ResetHistory();
    for(auto& eye:pool) for(auto& slot:eye) slot={};
    for(auto& list:lists) list={};
    for(unsigned eye=0;eye<2;++eye) {
        ofVector[eye].Reset();ofScd[eye].Reset();ofVectorW[eye]=ofVectorH[eye]=poolNext[eye]=0;
    }
    std::free(scratch);scratch=nullptr;backend={};backendReady=false;
    contextRenderW=contextRenderH=contextDisplayW=contextDisplayH=0;
    timestamps.Reset();timestampReadback.Reset();computeQueue.Reset();fence.Reset();
    producerFence.Reset();consumerFence.Reset();
    listNext=0;fenceValue=producerValue=consumerValue=0;
    layout={};ticksToMs=0;generationMs=0;
    {std::lock_guard lock(statusMutex);status.ready=false;}
    return true;
}

bool Generate(ID3D12Device* device,ID3D12CommandQueue* producer,ID3D12Resource* const colors[2],
              const std::shared_ptr<const framegen::Inputs> (&inputs)[2],uint64_t serial,double frameMs,
              std::shared_ptr<const void> captureLease) {
    if(!Enabled() || !device || !producer || !colors[0] || !colors[1] || !inputs[0] || !inputs[1])return false;
    const auto color=colors[0]->GetDesc();
    if(color.Format!=DXGI_FORMAT_R8G8B8A8_TYPELESS && color.Format!=DXGI_FORMAT_R8G8B8A8_UNORM &&
       color.Format!=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) {
        SetText("FSR3 needs RGBA8 eye captures");return false;
    }
    const InputLayout requested{device,UINT(color.Width),color.Height,inputs[0]->width,inputs[0]->height,
                                inputs[0]->camera.inverted,inputs[0]->camera.infinite};
    const auto right=colors[1]->GetDesc();
    if(right.Width!=color.Width || right.Height!=color.Height || right.Format!=color.Format ||
       inputs[1]->width!=requested.renderWidth || inputs[1]->height!=requested.renderHeight) return false;
    if(layout.device && !(layout==requested) && !ReleaseResources())return false;
    layout=requested;
    if(!producerFence && FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&producerFence))))return false;
    if(FAILED(producer->Signal(producerFence.Get(),++producerValue)))return false;
    const GpuWait waits[2]={{producerFence.Get(),producerValue},{consumerFence.Get(),consumerValue}};
    // The public wrappers catch faults in the SDK without unwinding C++ resource owners.
    const int index=listNext;
    const uint64_t before=fenceValue;
    if(mode.load()==kExtrapolation) RunExtrapolation(device,waits,colors,inputs,serial);
    else RunInterpolation(device,waits,colors,inputs,serial,frameMs);
    if(fenceValue>before)lists[index].captureLease=std::move(captureLease);
    return Output(0,serial) && Output(1,serial);
}
void QueueWaitForLast(ID3D12CommandQueue* queue) {
    if(queue && fence && fenceValue)queue->Wait(fence.Get(),fenceValue);
}
double GenerationMs() {return generationMs.load(std::memory_order_relaxed);}
ID3D12Resource* Output(unsigned eye,uint64_t serial) {
    if(eye>1 || !serial)return nullptr;
    std::lock_guard lock(poolMutex);
    for(auto& slot:pool[eye])if(slot.serial==serial)return slot.texture.Get();
    return nullptr;
}
Status GetStatus() {
    std::lock_guard lock(statusMutex);auto result=status;result.faulted=faulted.load();
    if(result.faulted)std::snprintf(result.text,sizeof(result.text),"%s","Stopped by a fault: see cyberpunkvrport.log");
    else if(!enabled.load())std::snprintf(result.text,sizeof(result.text),"%s","Off");
    return result;
}
}
