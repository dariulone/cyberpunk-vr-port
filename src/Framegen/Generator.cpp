#include "Framegen/CapturedColor.hpp"
#ifdef CVR_V557_RECONSTRUCTION
#include "Framegen/V557Generator.hpp"
#endif
#include "Framegen/SourceTimeline.hpp"
#include "Framegen/CaptureTiming.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Framegen/Generator.hpp"
#include "Framegen/SynthesisQueue.hpp"
#include "Framegen/GpuCorrelation.hpp"
#include "Framegen/RefreshCadence.hpp"
#include "Framegen/DlssFrameGen.hpp"
#include "Framegen/FrameCapture.hpp"
#include <FidelityFX/host/backends/dx12/ffx_dx12.h>
#include <FidelityFX/host/ffx_frameinterpolation.h>
#include <FidelityFX/host/ffx_opticalflow.h>
#include <nvOpticalFlowD3D12.h>
#include <d3dcompiler.h>
#include <dxgi1_4.h>
#include "Utils/TestMarker.hpp"
#include <algorithm>
#include <array>
#include <cstring>
#include <vector>

extern void Log(const char*,...);
namespace cvr::framegen {
namespace {
using Microsoft::WRL::ComPtr;
constexpr auto kRead=D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
constexpr auto kUav=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
constexpr auto kCopy=D3D12_RESOURCE_STATE_COPY_SOURCE;
constexpr char shader[]=R"(
Texture2D<float4> color : register(t0);
Texture2D<int2> rawFlow : register(t1);
Texture2D<float4> previous : register(t2);
RWTexture2D<float4> packed : register(u0);
RWTexture2D<int2> flow : register(u1);
RWTexture2D<uint> scd : register(u2);
SamplerState linearClamp : register(s0);
cbuffer Params : register(b0) { uint sw,sh,dw,dh,reset,clearScd; }
[numthreads(8,8,1)] void Pack(uint3 id:SV_DispatchThreadID) {
    if(id.x<dw && id.y<dh) packed[id.xy]=float4(color.SampleLevel(linearClamp,(id.xy+.5)/float2(dw,dh),0).rgb,1);
}
[numthreads(8,8,1)] void Flow(uint3 id:SV_DispatchThreadID) {
    if(id.x<dw && id.y<dh) {
        uint2 p=min(uint2((id.xy+.5)*float2(sw,sh)/float2(dw,dh)),uint2(sw-1,sh-1));
        flow[id.xy]=reset ? int2(0,0) : rawFlow.Load(int3(p,0));
    }
    if(clearScd!=0 && id.x==0 && id.y==0) {scd[uint2(0,0)]=0;scd[uint2(1,0)]=0;scd[uint2(2,0)]=0;}
}
groupshared float difference[64];
[numthreads(8,8,1)] void SceneCut(uint3 id:SV_GroupThreadID,uint index:SV_GroupIndex) {
    float2 uv=(id.xy+.5)/8;
    float3 a=color.SampleLevel(linearClamp,uv,0).rgb,b=previous.SampleLevel(linearClamp,uv,0).rgb;
    difference[index]=dot(abs(a-b),float3(.2126,.7152,.0722));GroupMemoryBarrierWithGroupSync();
    for(uint step=32;step;step/=2) { if(index<step) difference[index]+=difference[index+step];GroupMemoryBarrierWithGroupSync(); }
    if(index==0) { uint cut=reset || difference[0]/64>.45 ? 15 : 0;scd[uint2(0,0)]=cut;scd[uint2(1,0)]=cut;scd[uint2(2,0)]=cut; }
}
)";
constexpr char prepareInputsShader[]=R"(
Texture2D<float> sourceDepth : register(t0);
Texture2D<float4> sourceMotion : register(t1);
RWTexture2D<float> targetDepth : register(u0);
RWTexture2D<float2> targetMotion : register(u1);
cbuffer Params : register(b0) { uint sw,sh,dw,dh,inverted,padding; }
[numthreads(8,8,1)] void PrepareInputs(uint3 id:SV_DispatchThreadID) {
    if(id.x>=dw || id.y>=dh)return;
    float2 pixel=(id.xy+.5)*float2(sw,sh)/float2(dw,dh);
    int2 chosen=min(int2(pixel),int2(sw-1,sh-1));
    float depth=sourceDepth.Load(int3(chosen,0));
    if(sw>dw || sh>dh) {
        int2 base=int2(floor(pixel-.5));
        [unroll] for(int y=0;y<2;++y) [unroll] for(int x=0;x<2;++x) {
            int2 p=clamp(base+int2(x,y),int2(0,0),int2(sw-1,sh-1));
            float z=sourceDepth.Load(int3(p,0));
            if(inverted ? z>depth : z<depth) {depth=z;chosen=p;}
        }
    }
    uint mw,mh;sourceMotion.GetDimensions(mw,mh);
    uint2 motionPixel=min(uint2((chosen+.5)*float2(mw,mh)/float2(sw,sh)),uint2(mw-1,mh-1));
    targetDepth[id.xy]=depth;
    targetMotion[id.xy]=sourceMotion.Load(int3(motionPixel,0)).xy;
}
)";
void Barrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b) {
    if(a==b)return;D3D12_RESOURCE_BARRIER v{};v.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,a,b};list->ResourceBarrier(1,&v);
}
struct Texture {
    ComPtr<ID3D12Resource> resource;
    DXGI_FORMAT format{};
    D3D12_RESOURCE_STATES state{};
    uint32_t width{},height{};
    uint64_t bytes{};
    bool Create(ID3D12Device* device,uint32_t w,uint32_t h,DXGI_FORMAT f,const wchar_t* name,
                D3D12_RESOURCE_STATES initial=kUav) {
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=w;d.Height=h;
        d.DepthOrArraySize=1;d.MipLevels=1;d.Format=f;d.SampleDesc.Count=1;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        D3D12_HEAP_PROPERTIES p{};p.Type=D3D12_HEAP_TYPE_DEFAULT;
        if(FAILED(device->CreateCommittedResource(&p,D3D12_HEAP_FLAG_NONE,&d,initial,nullptr,IID_PPV_ARGS(&resource))))return false;
        format=f;state=initial;width=w;height=h;bytes=device->GetResourceAllocationInfo(0,1,&d).SizeInBytes;
        resource->SetName(name);return true;
    }
    void To(ID3D12GraphicsCommandList* list,D3D12_RESOURCE_STATES target) {Barrier(list,resource.Get(),state,target);state=target;}
};
FfxResource Resource(ID3D12Resource* resource,FfxResourceStates state,DXGI_FORMAT overrideFormat=DXGI_FORMAT_UNKNOWN) {
    auto desc=ffxGetResourceDescriptionDX12(resource);
    if(overrideFormat!=DXGI_FORMAT_UNKNOWN)desc.format=ffxGetSurfaceFormatDX12(overrideFormat);
    return ffxGetResourceDX12(resource,desc,L"CVR_Framegen",state);
}
FfxResource Resource(const Texture& t,FfxResourceStates state=FFX_RESOURCE_STATE_UNORDERED_ACCESS) {
    return Resource(t.resource.Get(),state,t.format);
}
bool Ffx(FfxErrorCode code,const char* operation) {
    if(code==FFX_OK)return true;Log("[framegen] %s failed: %u\n",operation,unsigned(code));Status(operation);return false;
}
struct NvFlow {
    HMODULE module{};NV_OF_D3D12_API_FUNCTION_LIST api{};NvOFHandle context{};
    NvOFGPUBufferHandle colors[2]{},flow{};
    ComPtr<ID3D12Fence> fence;uint64_t value{};
    void Unregister() {
        for(auto* h:{&colors[0],&colors[1],&flow})if(*h && api.nvOFUnregisterResourceD3D12) {
            NV_OF_UNREGISTER_RESOURCE_PARAMS_D3D12 p{};p.hOFGpuBuffer=*h;api.nvOFUnregisterResourceD3D12(&p);*h=nullptr;
        }
    }
    ~NvFlow() {
        Unregister();
        if(context && api.nvOFDestroy)api.nvOFDestroy(context);
        if(module)FreeLibrary(module);
    }
    bool Idle() const {return !fence || fence->GetCompletedValue()>=value;}
    bool Create(ID3D12Device* device,ID3D12CommandQueue* queue,ID3D12Fence* ready,
        Texture (&input)[2],Texture& output,int quality) {
        module=LoadLibraryExW(L"nvofapi64.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);if(!module)return false;
        using CreateFn=NV_OF_STATUS (NVOFAPI*)(uint32_t,NV_OF_D3D12_API_FUNCTION_LIST*);
        const auto create=reinterpret_cast<CreateFn>(GetProcAddress(module,"NvOFAPICreateInstanceD3D12"));
        if(!create || create(NV_OF_API_VERSION,&api)!=NV_OF_SUCCESS ||
           api.nvCreateOpticalFlowD3D12(device,&context)!=NV_OF_SUCCESS)return false;
        NV_OF_INIT_PARAMS init{};init.width=input[0].width;init.height=input[0].height;
        init.outGridSize=NV_OF_OUTPUT_VECTOR_GRID_SIZE_4;init.mode=NV_OF_MODE_OPTICALFLOW;
        init.perfLevel=quality==0 ? NV_OF_PERF_LEVEL_FAST : (quality==1 ? NV_OF_PERF_LEVEL_MEDIUM : NV_OF_PERF_LEVEL_SLOW);
        init.inputBufferFormat=NV_OF_BUFFER_FORMAT_ABGR8;
        if(api.nvOFInit(context,&init)!=NV_OF_SUCCESS || FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))))return false;
        ID3D12Resource* resources[]={input[0].resource.Get(),input[1].resource.Get(),output.resource.Get()};
        NvOFGPUBufferHandle* handles[]={&colors[0],&colors[1],&flow};
        for(unsigned i=0;i<3;++i) {
            NV_OF_REGISTER_RESOURCE_PARAMS_D3D12 p{};p.resource=resources[i];p.hOFGpuBuffer=handles[i];
            p.inputFencePoint={ready,0};p.outputFencePoint={fence.Get(),value+1};
            if(api.nvOFRegisterResourceD3D12(context,&p)!=NV_OF_SUCCESS)return false;
            ++value;
        }
        return SUCCEEDED(queue->Wait(fence.Get(),value));
    }
    bool Run(ID3D12Fence* packed,uint64_t packedValue,unsigned current) {
        NV_OF_FENCE_POINT waits[]={{packed,packedValue},{fence.Get(),value}};
        NV_OF_EXECUTE_INPUT_PARAMS_D3D12 input{};input.inputFrame=colors[current];input.referenceFrame=colors[current^1];
        input.disableTemporalHints=NV_OF_TRUE;input.numFencePoints=2;input.fencePoint=waits;
        NV_OF_FENCE_POINT completed{fence.Get(),value+1};
        NV_OF_EXECUTE_OUTPUT_PARAMS_D3D12 output{};output.outputBuffer=flow;output.fencePoint=&completed;
        if(api.nvOFExecuteD3D12(context,&input,&output)!=NV_OF_SUCCESS)return false;
        ++value;return true;
    }
};
struct Eye {
    std::vector<std::byte> scratch;
    FfxInterface backend{};FfxFrameInterpolationContext fi{};FfxOpticalflowContext of{};
    bool fiCreated{},ofCreated{};
    Texture packed[2],rawFlow,flow,scd,depth,motion,previousDepth,output,preparedDepth,preparedMotion;
    NvFlow nv;
    Backend mode{};
    // DLSS-G: one NGX feature per eye, a typed copy of a typeless capture, and the camera NGX's
    // history was last fed with.
    dlssg::FeaturePtr dlss;Texture backbuffer;
    CameraData previousCamera{};bool hasPreviousCamera{};
    uint32_t width{},height{},renderWidth{},renderHeight{},flowWidth{},flowHeight{},current{};
    bool opticalFlow=true;
    CameraData camera{};
    bool prepareInputs{};
    ~Eye() {
        if(fiCreated)ffxFrameInterpolationContextDestroy(&fi);
        if(ofCreated)ffxOpticalflowContextDestroy(&of);
        // The registered D3D12 allocations must be released before destroying
        // their OFA context (also the teardown order used by OFXR). Reversing
        // these steps crashed nvwgf2umx during our hardware retirement test.
        nv.Unregister();packed[0].resource.Reset();packed[1].resource.Reset();rawFlow.resource.Reset();
    }
    bool Create(ID3D12Device* device,ID3D12CommandQueue* queue,ID3D12Fence* fence,
                ID3D12Resource* color,const Inputs& inputs,const Settings& settings) {
        const auto desc=color->GetDesc();width=uint32_t(desc.Width);height=desc.Height;
        if(desc.Format!=DXGI_FORMAT_R8G8B8A8_TYPELESS && desc.Format!=DXGI_FORMAT_R8G8B8A8_UNORM && desc.Format!=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB) {
            Status("Framegen requires the current SDR RGBA8 capture format");return false;
        }
        renderWidth=std::min(inputs.width,inputs.motionWidth);renderHeight=std::min(inputs.height,inputs.motionHeight);camera=inputs.camera;
        mode=settings.backend;
        if(mode==Backend::DlssG) {
            if(!output.Create(device,width,height,DXGI_FORMAT_R8G8B8A8_UNORM,L"CVR_FG_Output",kCopy))return false;
            // NGX views the backbuffer in its own format, which a typeless capture does not have.
            return desc.Format!=DXGI_FORMAT_R8G8B8A8_TYPELESS ||
                backbuffer.Create(device,width,height,DXGI_FORMAT_R8G8B8A8_UNORM,L"CVR_FG_DlssBackbuffer",kRead);
        }
        // DLAA does not need full-display-resolution depth/MV preparation for
        // interpolation. Keep real color untouched and bound only FI's inputs.
        if(inputs.width==width && inputs.height==height) {
            renderWidth=std::min(renderWidth,(width+1)/2);renderHeight=std::min(renderHeight,(height+1)/2);
        }
        prepareInputs=renderWidth!=inputs.width || renderHeight!=inputs.height ||
            renderWidth!=inputs.motionWidth || renderHeight!=inputs.motionHeight;
        if(!renderWidth || !renderHeight)return false;
        flowWidth=std::max(64u,(width*uint32_t(settings.flowScale)+99)/100);
        flowHeight=std::max(64u,(height*uint32_t(settings.flowScale)+99)/100);
        const bool nvidia=settings.backend==Backend::Nvidia;
        opticalFlow=settings.flowScale>0;
        // Both modes use FidelityFX's depth/MV-aware interpolation. NVIDIA
        // replaces only the optical-flow stage, with the hardware OFA engine.
        scratch.resize(ffxGetScratchMemorySizeDX12(2));
        if(!Ffx(ffxGetInterfaceDX12(&backend,ffxGetDeviceDX12(device),scratch.data(),scratch.size(),2),"FidelityFX backend"))return false;
        FfxFrameInterpolationContextDescription config{};config.backendInterface=backend;
        config.maxRenderSize={renderWidth,renderHeight};config.displaySize={width,height};
        config.backBufferFormat=config.previousInterpolationSourceFormat=FFX_SURFACE_FORMAT_R8G8B8A8_UNORM;
        if(camera.inverted)config.flags|=FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED;
        if(camera.infinite)config.flags|=FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INFINITE;
        if(camera.jittered)config.flags|=FFX_FRAMEINTERPOLATION_ENABLE_JITTER_MOTION_VECTORS;
        if(!prepareInputs && inputs.motionWidth==width && inputs.motionHeight==height)config.flags|=FFX_FRAMEINTERPOLATION_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS;
        if(!Ffx(ffxFrameInterpolationContextCreate(&fi,&config),"Frame interpolation context"))return false;fiCreated=true;
        if(!nvidia && opticalFlow) {
            FfxOpticalflowContextDescription optics{};optics.backendInterface=backend;optics.resolution={flowWidth,flowHeight};
            if(!Ffx(ffxOpticalflowContextCreate(&of,&optics),"Optical flow context"))return false;ofCreated=true;
        }
        const uint32_t block=nvidia?4u:8u;
        // Without optical flow, interpolation runs on the engine's vectors alone and reads an all-zero flow.
        if(opticalFlow && (!packed[0].Create(device,flowWidth,flowHeight,DXGI_FORMAT_R8G8B8A8_UNORM,L"CVR_FG_PackedA",D3D12_RESOURCE_STATE_COMMON) ||
           (nvidia && !packed[1].Create(device,flowWidth,flowHeight,DXGI_FORMAT_R8G8B8A8_UNORM,L"CVR_FG_PackedB",D3D12_RESOURCE_STATE_COMMON)) ||
           !rawFlow.Create(device,(flowWidth+block-1)/block,(flowHeight+block-1)/block,DXGI_FORMAT_R16G16_SINT,L"CVR_FG_RawFlow",nvidia?D3D12_RESOURCE_STATE_COMMON:kUav)))return false;
        if(!flow.Create(device,(width+7)/8,(height+7)/8,DXGI_FORMAT_R16G16_SINT,L"CVR_FG_Flow") ||
           !scd.Create(device,3,1,DXGI_FORMAT_R32_UINT,L"CVR_FG_SceneChange") ||
           !depth.Create(device,renderWidth,renderHeight,DXGI_FORMAT_R32_FLOAT,L"CVR_FG_DilatedDepth") ||
           !motion.Create(device,renderWidth,renderHeight,DXGI_FORMAT_R16G16_FLOAT,L"CVR_FG_DilatedMotion") ||
           !previousDepth.Create(device,renderWidth,renderHeight,DXGI_FORMAT_R32_UINT,L"CVR_FG_PreviousDepth") ||
           !output.Create(device,width,height,DXGI_FORMAT_R8G8B8A8_UNORM,L"CVR_FG_Output",kCopy))return false;
        if(prepareInputs && (!preparedDepth.Create(device,renderWidth,renderHeight,DXGI_FORMAT_R32_FLOAT,L"CVR_FG_PreparedDepth") ||
            !preparedMotion.Create(device,renderWidth,renderHeight,DXGI_FORMAT_R32G32_FLOAT,L"CVR_FG_PreparedMotion")))return false;
        return !nvidia || !opticalFlow || nv.Create(device,queue,fence,packed,rawFlow,settings.quality);
    }
    uint64_t Bytes() const {
        uint64_t bytes=0;for(const auto* t:{&packed[0],&packed[1],&rawFlow,&flow,&scd,&depth,&motion,&previousDepth,&output,&preparedDepth,&preparedMotion,&backbuffer})bytes+=t->bytes;
        if(dlss)bytes+=dlssg::Bytes(*dlss);
        FfxEffectMemoryUsage usage{};
        if(fiCreated && ffxFrameInterpolationContextGetGpuMemoryUsage(const_cast<FfxFrameInterpolationContext*>(&fi),&usage)==FFX_OK)bytes+=usage.totalUsageInBytes;
        if(ofCreated && ffxOpticalflowContextGetGpuMemoryUsage(const_cast<FfxOpticalflowContext*>(&of),&usage)==FFX_OK)bytes+=usage.totalUsageInBytes;
        return bytes;
    }
    bool Interpolate(ID3D12GraphicsCommandList* list,ID3D12Resource* color,const Inputs& inputs,
                     double frameMs,bool reset,uint64_t frameId,bool nvidia) {
        FfxFrameInterpolationPrepareDescription p{};p.commandList=ffxGetCommandListDX12(list);p.renderSize={renderWidth,renderHeight};
        p.jitterOffset={inputs.camera.jitter[0]*renderWidth/inputs.width,inputs.camera.jitter[1]*renderHeight/inputs.height};
        // Streamline's normalized displacement scale -> FFX's pixel displacement scale.
        p.motionVectorScale={inputs.camera.motionScale[0]*(prepareInputs?renderWidth:inputs.motionWidth),
            inputs.camera.motionScale[1]*(prepareInputs?renderHeight:inputs.motionHeight)};
        p.frameTimeDelta=float(frameMs);p.cameraNear=inputs.camera.nearPlane;p.cameraFar=inputs.camera.farPlane;
        p.cameraFovAngleVertical=inputs.camera.verticalFov;p.viewSpaceToMetersFactor=1;
        p.depth=Resource(prepareInputs?preparedDepth.resource.Get():inputs.depth.Get(),FFX_RESOURCE_STATE_COMPUTE_READ);
        p.motionVectors=Resource(prepareInputs?preparedMotion.resource.Get():inputs.motion.Get(),FFX_RESOURCE_STATE_COMPUTE_READ);
        p.frameID=frameId;p.dilatedDepth=Resource(depth);p.dilatedMotionVectors=Resource(motion);p.reconstructedPrevDepth=Resource(previousDepth);
        std::memcpy(&p.cameraPosition,inputs.camera.position,12);std::memcpy(&p.cameraUp,inputs.camera.up,12);
        std::memcpy(&p.cameraRight,inputs.camera.right,12);std::memcpy(&p.cameraForward,inputs.camera.forward,12);
        if(!Ffx(ffxFrameInterpolationPrepare(&fi,&p),"Frame interpolation prepare"))return false;
        FfxFrameInterpolationDispatchDescription d{};d.commandList=p.commandList;d.displaySize={width,height};d.renderSize=p.renderSize;
        d.currentBackBuffer=Resource(color,captured_color::Shared(color)?FFX_RESOURCE_STATE_GENERIC_READ:FFX_RESOURCE_STATE_COPY_SRC,DXGI_FORMAT_R8G8B8A8_UNORM);
        d.output=Resource(output,FFX_RESOURCE_STATE_COPY_SRC);d.interpolationRect={0,0,int32_t(width),int32_t(height)};
        d.opticalFlowVector=Resource(flow);d.opticalFlowSceneChangeDetection=Resource(scd);d.opticalFlowBufferSize={flow.width,flow.height};
        const float units=nvidia?32.0f:1.0f;
        d.opticalFlowScale={1.0f/(units*flowWidth),1.0f/(units*flowHeight)};d.opticalFlowBlockSize=8;
        d.cameraNear=p.cameraNear;d.cameraFar=p.cameraFar;d.cameraFovAngleVertical=p.cameraFovAngleVertical;
        d.viewSpaceToMetersFactor=1;d.frameTimeDelta=p.frameTimeDelta;d.reset=reset;d.frameID=frameId;
        d.backBufferTransferFunction=FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;d.minMaxLuminance[0]=0;d.minMaxLuminance[1]=1;
        d.dilatedDepth=p.dilatedDepth;d.dilatedMotionVectors=p.dilatedMotionVectors;d.reconstructedPrevDepth=p.reconstructedPrevDepth;
        return Ffx(ffxFrameInterpolationDispatch(&fi,&d),"Frame interpolation dispatch");
    }
};
}
struct Generator::Impl {
    ComPtr<ID3D12Device> device;ComPtr<ID3D12CommandQueue> queue;
    SynthesisQueue routing;bool isolatedRequested{},untrackedGpuWork{};uint64_t signaledValue{};
    ComPtr<ID3D12Fence> fence,consumerFence;uint64_t value{},consumerValue{},frequency{};
    ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12PipelineState> pipelines[4];
    Backend mode{};
    ComPtr<ID3D12QueryHeap> queries;ComPtr<ID3D12Resource> readback;
    uint64_t lastTimingFence{};bool timingUnavailable{};unsigned probeSerial{};
    struct Work {
        ComPtr<ID3D12CommandAllocator> allocator,finishAllocator;
        ComPtr<ID3D12GraphicsCommandList> list,finish;
        ComPtr<ID3D12DescriptorHeap> heap;
        uint64_t fence{};uint32_t descriptors{};bool timing{};
        std::shared_ptr<const Inputs> inputs[2];
        std::shared_ptr<void> colorLease; // retain capture pins through partial/failed dispatch
        GpuCorrelation correlation;
        std::shared_ptr<const CaptureTiming> captureTiming;
        ComPtr<ID3D12Fence> probeCapture;uint64_t probeCaptureValue{};
        bool probe{},inputsKnown{},inputsPending{};
    };
    std::array<Work,3> work;
    std::unique_ptr<Eye> eyes[2];
    Settings settings;unsigned cursor{};bool failed{};
#ifdef CVR_FRAMEGEN_TEST_RECEIPTS
    uint64_t testPackedValue{};
#endif
    struct Signature {uint64_t width{};uint32_t height{},renderWidth{},renderHeight{},motionWidth{},motionHeight{};bool inverted{},infinite{},jittered{};} signatures[2];
    bool Idle() const {
        if(untrackedGpuWork)return false;
        if(fence && fence->GetCompletedValue()<value)return false;
        if(consumerFence && consumerFence->GetCompletedValue()<consumerValue)return false;
        for(const auto& eye:eyes)if(eye && !eye->nv.Idle())return false;
        return true;
    }
    bool Create(ID3D12Device* d,ID3D12CommandQueue* q,ID3D12Resource* colors[2],
                const std::shared_ptr<const Inputs> (&inputs)[2],const Settings& s) {
        device=d;settings=s;mode=s.backend;
        // Scope this candidate to SteamVR OFA/FFX. Preserve the native DLSS-G
        // SDK queue contract and all VDXR behavior.
        isolatedRequested=refresh::Targeted() && mode!=Backend::DlssG;
        if(!routing.Initialize(d,q,isolatedRequested))return false;
        queue=routing.Queue();
        if(isolatedRequested && !routing.Private())flight::Count(flight::Event::SynthesisQueueFallback);
        Log("[framegen-queue] producer=%p synthesis=%p private=%d priority=%d create=0x%08X backend=%d\n",
            q,queue.Get(),routing.Private(),queue->GetDesc().Priority,unsigned(routing.CreationResult()),int(mode));
        for(unsigned i=0;i<2;++i) {
            const auto desc=colors[i]->GetDesc();const auto& input=*inputs[i];
            signatures[i]={desc.Width,desc.Height,input.width,input.height,input.motionWidth,input.motionHeight,input.camera.inverted,input.camera.infinite,input.camera.jittered};
        }
        if(FAILED(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))))return false;
        D3D12_DESCRIPTOR_RANGE ranges[2]{};ranges[0]={D3D12_DESCRIPTOR_RANGE_TYPE_SRV,3,0,0,0};ranges[1]={D3D12_DESCRIPTOR_RANGE_TYPE_UAV,3,0,0,0};
        D3D12_ROOT_PARAMETER params[3]{};
        for(int i=0;i<2;++i) {params[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[i].DescriptorTable={1,&ranges[i]};}
        params[2].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;params[2].Constants={0,0,6};
        D3D12_STATIC_SAMPLER_DESC sampler{};sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_LINEAR;
        sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_ALWAYS;sampler.MaxAnisotropy=1;
        sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;sampler.MaxLOD=D3D12_FLOAT32_MAX;
        D3D12_ROOT_SIGNATURE_DESC desc{};desc.NumParameters=3;desc.pParameters=params;desc.NumStaticSamplers=1;desc.pStaticSamplers=&sampler;
        ComPtr<ID3DBlob> blob,error;
        if(FAILED(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&blob,&error)) ||
            FAILED(d->CreateRootSignature(0,blob->GetBufferPointer(),blob->GetBufferSize(),IID_PPV_ARGS(&root))))return false;
        const char* names[]={"Pack","Flow","SceneCut","PrepareInputs"};
        for(unsigned i=0;i<4 && (mode==Backend::FidelityFX || mode==Backend::Nvidia);++i) {
            if(i==2 && s.backend==Backend::FidelityFX)continue;
            if(FAILED(D3DCompile(i==3?prepareInputsShader:shader,i==3?sizeof(prepareInputsShader)-1:sizeof(shader)-1,
                "FramegenPrepare",nullptr,nullptr,names[i],"cs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&blob,&error))) {
                if(error)Log("[framegen] shader %s: %s\n",names[i],static_cast<const char*>(error->GetBufferPointer()));return false;
            }
            D3D12_COMPUTE_PIPELINE_STATE_DESC p{};p.pRootSignature=root.Get();p.CS={blob->GetBufferPointer(),blob->GetBufferSize()};
            if(FAILED(d->CreateComputePipelineState(&p,IID_PPV_ARGS(&pipelines[i]))))return false;
        }
        for(auto& w:work) {
            if(FAILED(d->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&w.allocator))) ||
               FAILED(d->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&w.finishAllocator))) ||
               FAILED(d->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,w.allocator.Get(),nullptr,IID_PPV_ARGS(&w.list))) ||
               FAILED(d->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,w.finishAllocator.Get(),nullptr,IID_PPV_ARGS(&w.finish))))return false;
            w.list->Close();w.finish->Close();
            D3D12_DESCRIPTOR_HEAP_DESC h{};h.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;h.NumDescriptors=48;h.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
            if(FAILED(d->CreateDescriptorHeap(&h,IID_PPV_ARGS(&w.heap))))return false;
        }
        for(unsigned eye=0;eye<2;++eye) {
            eyes[eye]=std::make_unique<Eye>();
            if(!eyes[eye]->Create(d,queue.Get(),fence.Get(),colors[eye],*inputs[eye],s))return false;
        }
        return mode!=Backend::DlssG || CreateDlssFeatures(inputs);
    }
    // NGX records the features' creation into a command list; it runs before the first pair.
    bool CreateDlssFeatures(const std::shared_ptr<const Inputs> (&inputs)[2]) {
        auto& w=work[0];
        if(FAILED(w.allocator->Reset()) || FAILED(w.list->Reset(w.allocator.Get(),nullptr)))return false;
        bool created=true;
        for(unsigned i=0;i<2 && created;++i) {
            auto& eye=*eyes[i];const auto& input=*inputs[i];
            eye.dlss=dlssg::Create(w.list.Get(),eye.width,eye.height,input.width,input.height,DXGI_FORMAT_R8G8B8A8_UNORM,
                input.motion->GetDesc().Format,DXGI_FORMAT_R32_FLOAT);
            created=eye.dlss!=nullptr;
        }
        const bool submitted=Submit(w.list.Get(),w);
        if(!created)Status("DLSS Frame Generation could not create its features");
        return created && submitted;
    }
    bool RecordDlss(ID3D12GraphicsCommandList* list,Eye& eye,ID3D12Resource* color,const Inputs& inputs,bool reset) {
        ID3D12Resource* backbuffer=color;
        if(eye.backbuffer.resource) {
            eye.backbuffer.To(list,D3D12_RESOURCE_STATE_COPY_DEST);list->CopyResource(eye.backbuffer.resource.Get(),color);
            eye.backbuffer.To(list,kRead);backbuffer=eye.backbuffer.resource.Get();
        } else if(!captured_color::Shared(color))Barrier(list,color,kCopy,kRead);
        eye.output.To(list,kUav);
        // Every real frame is evaluated, resets included: NGX keeps the previous backbuffer itself.
        dlssg::Evaluation e{};e.backbuffer=backbuffer;e.depth=inputs.depth.Get();e.motion=inputs.motion.Get();e.output=eye.output.resource.Get();
        e.width=eye.width;e.height=eye.height;e.depthWidth=inputs.width;e.depthHeight=inputs.height;
        e.motionWidth=inputs.motionWidth;e.motionHeight=inputs.motionHeight;e.camera=&inputs.camera;
        e.reset=reset || !eye.hasPreviousCamera;e.previous=e.reset?nullptr:&eye.previousCamera;
        const bool evaluated=dlssg::Evaluate(*eye.dlss,list,e);
        eye.output.To(list,kCopy);
        if(!eye.backbuffer.resource && !captured_color::Shared(color))Barrier(list,color,kRead,kCopy);
        eye.previousCamera=inputs.camera;eye.hasPreviousCamera=true;
        return evaluated;
    }
    void RetireTiming() {
        if(MetricsEnabled()||flight::Enabled())return;
        timingUnavailable=false;
        if(!queries || (lastTimingFence && fence->GetCompletedValue()<lastTimingFence))return;
        for(auto& slot:work)slot.timing=false;
        queries.Reset();readback.Reset();lastTimingFence=frequency=0;
    }
    bool TimingReady() {
        if(!MetricsEnabled()&&!flight::Enabled()) {RetireTiming();return false;}
        if(queries)return true;if(timingUnavailable)return false;
        D3D12_QUERY_HEAP_DESC query{};query.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;query.Count=12;
        D3D12_RESOURCE_DESC read{};read.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;read.Width=96;read.Height=1;read.DepthOrArraySize=1;
        read.MipLevels=1;read.SampleDesc.Count=1;read.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
        ComPtr<ID3D12QueryHeap> createdQueries;ComPtr<ID3D12Resource> createdReadback;
        if(FAILED(device->CreateQueryHeap(&query,IID_PPV_ARGS(&createdQueries))) ||
           FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&read,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&createdReadback))) ||
           FAILED(queue->GetTimestampFrequency(&frequency)) || !frequency) {timingUnavailable=true;return false;}
        queries=std::move(createdQueries);readback=std::move(createdReadback);return true;
    }
    bool Compatible(ID3D12Resource* colors[2],const std::shared_ptr<const Inputs> (&inputs)[2],const Settings& s) const {
        if(s.backend!=settings.backend || s.quality!=settings.quality || s.flowScale!=settings.flowScale)return false;
        for(unsigned i=0;i<2;++i) {
            if(!inputs[i] || !colors[i])return false;
            const auto d=colors[i]->GetDesc();const auto& eye=signatures[i];const auto& camera=inputs[i]->camera;
            if(d.Width!=eye.width || d.Height!=eye.height || inputs[i]->width!=eye.renderWidth || inputs[i]->height!=eye.renderHeight ||
                inputs[i]->motionWidth!=eye.motionWidth || inputs[i]->motionHeight!=eye.motionHeight ||
                camera.inverted!=eye.inverted || camera.infinite!=eye.infinite || camera.jittered!=eye.jittered)return false;
        }
        return true;
    }
    void Dispatch(Work& w,ID3D12GraphicsCommandList* list,unsigned pass,ID3D12Resource* color,DXGI_FORMAT colorFormat,
        Texture* raw,Texture* previous,Texture* packed,Texture* flow,Texture* scd,uint32_t sw,uint32_t sh,uint32_t dw,uint32_t dh,bool reset,
        bool clearScd=false) {
        const auto step=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        auto cpu=w.heap->GetCPUDescriptorHandleForHeapStart();cpu.ptr+=size_t(w.descriptors)*step;
        auto gpu=w.heap->GetGPUDescriptorHandleForHeapStart();gpu.ptr+=uint64_t(w.descriptors)*step;w.descriptors+=6;
        ID3D12Resource* sources[]={color,raw?raw->resource.Get():nullptr,previous?previous->resource.Get():nullptr};
        DXGI_FORMAT formats[]={colorFormat,raw?raw->format:DXGI_FORMAT_R16G16_SINT,DXGI_FORMAT_R8G8B8A8_UNORM};
        for(unsigned i=0;i<3;++i) {
            D3D12_SHADER_RESOURCE_VIEW_DESC d{};d.Format=formats[i];d.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
            d.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;d.Texture2D.MipLevels=1;
            device->CreateShaderResourceView(sources[i],&d,{cpu.ptr+size_t(i)*step});
        }
        Texture* outputs[]={packed,flow,scd};
        DXGI_FORMAT outFormats[]={DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R16G16_SINT,DXGI_FORMAT_R32_UINT};
        for(unsigned i=0;i<3;++i) {
            D3D12_UNORDERED_ACCESS_VIEW_DESC d{};d.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;d.Format=outFormats[i];
            device->CreateUnorderedAccessView(outputs[i]?outputs[i]->resource.Get():nullptr,nullptr,&d,{cpu.ptr+size_t(i+3)*step});
        }
        ID3D12DescriptorHeap* heaps[]={w.heap.Get()};list->SetDescriptorHeaps(1,heaps);list->SetComputeRootSignature(root.Get());
        list->SetPipelineState(pipelines[pass].Get());list->SetComputeRootDescriptorTable(0,gpu);
        list->SetComputeRootDescriptorTable(1,{gpu.ptr+uint64_t(3)*step});
        const uint32_t params[]={sw,sh,dw,dh,reset?1u:0u,clearScd?1u:0u};list->SetComputeRoot32BitConstants(2,6,params,0);
        list->Dispatch(pass==2?1:(dw+7)/8,pass==2?1:(dh+7)/8,1);
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_UAV;list->ResourceBarrier(1,&barrier);
    }
    bool Submit(ID3D12GraphicsCommandList* list,Work& w) {
        if(FAILED(list->Close()))return false;
        if(w.probe && list==w.list.Get()) {
            // Sample one pair in sixteen, recorder-only, on private synthesis.
            // Calibration is adjacent to enqueue, not a persistent mapping
            // that can drift across idle periods. No extra GPU lists/queries.
            w.probe=false;flight::Count(flight::Event::GenerationProbe);
            const auto pending=[](ID3D12Fence* f,uint64_t v){return f && v && f->GetCompletedValue()<v;};
            const bool capturePending=pending(w.probeCapture.Get(),w.probeCaptureValue);
            bool guidesPending=false;w.inputsKnown=w.probeCapture && w.probeCaptureValue;
            for(const auto& in:w.inputs) {
                w.inputsKnown=w.inputsKnown && in && in->readyFence && in->fenceValue;
                if(in)guidesPending|=pending(in->readyFence.Get(),in->fenceValue);
            }
            const bool consumerPending=pending(consumerFence.Get(),consumerValue);
            w.inputsPending=capturePending || guidesPending || consumerPending;
            if(capturePending)flight::Count(flight::Event::GenerationCapturePending);
            if(guidesPending)flight::Count(flight::Event::GenerationGuidesPending);
            if(consumerPending)flight::Count(flight::Event::GenerationConsumerPending);
            w.probeCapture.Reset();
            LARGE_INTEGER hz{},before{},after{};QueryPerformanceFrequency(&hz);QueryPerformanceCounter(&before);
            const auto calibrated=queue->GetClockCalibration(&w.correlation.gpu,&w.correlation.cpu);
            QueryPerformanceCounter(&after);
            if(hz.QuadPart>0)flight::Time(flight::Timing::GenerationCalibrationCpu,double(after.QuadPart-before.QuadPart)*1000.0/double(hz.QuadPart));
            if(SUCCEEDED(calibrated) && hz.QuadPart>0) {
                w.correlation.cpuHz=uint64_t(hz.QuadPart);w.correlation.gpuHz=frequency;
                LARGE_INTEGER submitted{};QueryPerformanceCounter(&submitted);w.correlation.submitted=uint64_t(submitted.QuadPart);
            } else {w.correlation={};flight::Count(flight::Event::GenerationCalibrationFailed);}
        }
        ID3D12CommandList* lists[]={list};queue->ExecuteCommandLists(1,lists);w.fence=++value;
        if(FAILED(queue->Signal(fence.Get(),value))){untrackedGpuWork=true;failed=true;return false;}
        signaledValue=value;return true;
    }
    void PrepareInputs(Work& w,ID3D12GraphicsCommandList* list,Eye& eye,const Inputs& inputs) {
        if(!eye.prepareInputs)return;
        eye.preparedDepth.To(list,kUav);eye.preparedMotion.To(list,kUav);
        const auto step=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        auto cpu=w.heap->GetCPUDescriptorHandleForHeapStart();cpu.ptr+=size_t(w.descriptors)*step;
        auto gpu=w.heap->GetGPUDescriptorHandleForHeapStart();gpu.ptr+=uint64_t(w.descriptors)*step;w.descriptors+=6;
        ID3D12Resource* sources[]{inputs.depth.Get(),inputs.motion.Get(),nullptr};
        for(unsigned i=0;i<3;++i) {
            D3D12_SHADER_RESOURCE_VIEW_DESC d{};d.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
            d.Format=sources[i]?sources[i]->GetDesc().Format:DXGI_FORMAT_R32_FLOAT;
            d.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;d.Texture2D.MipLevels=1;
            device->CreateShaderResourceView(sources[i],&d,{cpu.ptr+size_t(i)*step});
        }
        Texture* targets[]{&eye.preparedDepth,&eye.preparedMotion,nullptr};
        for(unsigned i=0;i<3;++i) {
            D3D12_UNORDERED_ACCESS_VIEW_DESC d{};d.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;
            d.Format=targets[i]?targets[i]->format:DXGI_FORMAT_R32_UINT;
            device->CreateUnorderedAccessView(targets[i]?targets[i]->resource.Get():nullptr,nullptr,&d,{cpu.ptr+size_t(i+3)*step});
        }
        ID3D12DescriptorHeap* heaps[]{w.heap.Get()};list->SetDescriptorHeaps(1,heaps);list->SetComputeRootSignature(root.Get());
        list->SetPipelineState(pipelines[3].Get());list->SetComputeRootDescriptorTable(0,gpu);
        list->SetComputeRootDescriptorTable(1,{gpu.ptr+uint64_t(3)*step});
        const uint32_t params[]{inputs.width,inputs.height,eye.renderWidth,eye.renderHeight,inputs.camera.inverted?1u:0u,0};
        list->SetComputeRoot32BitConstants(2,6,params,0);list->Dispatch((eye.renderWidth+7)/8,(eye.renderHeight+7)/8,1);
        eye.preparedDepth.To(list,kRead);eye.preparedMotion.To(list,kRead);
    }
    void Poll() {
        for(auto& old:work)if(old.fence && fence->GetCompletedValue()>=old.fence) {
            old.inputs[0].reset();old.inputs[1].reset();old.colorLease.reset();
        }
    }
    bool Generate(ID3D12Resource* colors[2],const std::shared_ptr<const Inputs> (&inputs)[2],double frameMs,bool reset,uint64_t frameId,const EyePair (&cameras)[2],ID3D12Fence* colorReady,uint64_t colorReadyValue,std::shared_ptr<void> colorLease,std::shared_ptr<const CaptureTiming> captureTiming) {
        Poll();
        auto& w=work[cursor];if(w.fence && fence->GetCompletedValue()<w.fence) {flight::Count(flight::Event::GeneratorBusy);OnSkip();return false;}
        // NVIDIA packing/OFA only touches captured color and internal packed/flow
        // textures. Previous generation is ordered on this same synthesis queue;
        // display reads only the final output. Defer its receipt until the finish
        // list, retaining capture/guide waits and all input leases here.
        const bool overlapConsumer=routing.Private() && mode==Backend::Nvidia && eyes[0]->opticalFlow;
        if(!routing.Begin(colorReady,colorReadyValue,
                          overlapConsumer?nullptr:consumerFence.Get(),overlapConsumer?0:consumerValue)) {
            failed=true;flight::Count(flight::Event::SynthesisSyncFailed);return false;
        }
        for(const auto& in:inputs)if(in->readyFence && in->fenceValue && !routing.WaitInput(in->readyFence.Get(),in->fenceValue)) {
            failed=true;flight::Count(flight::Event::SynthesisSyncFailed);return false;
        }
        flight::Count(routing.Private()?flight::Event::SynthesisPrivate:flight::Event::SynthesisShared);
        if(routing.Private() && (!colorReady || !colorReadyValue))flight::Count(flight::Event::SynthesisInputFallback);
        const bool timed=TimingReady();
        if(timed && w.timing) {
            // FOUR STAMPS A PAIR: the first list (colour packing, optical flow inputs) begins and ends, the finishing
            // list (flow resolve, interpolation) begins and ends. Between the two the queue waits on NVIDIA's optical
            // flow engine and runs whatever else was submitted meanwhile, so the first stamp to the last is the
            // generation's latency, not its cost: the cost is the two lists' own spans.
            void* pointer=nullptr;D3D12_RANGE range{cursor*32,cursor*32+32};
            if(SUCCEEDED(readback->Map(0,&range,&pointer))) {
                const auto* times=reinterpret_cast<const uint64_t*>(static_cast<const uint8_t*>(pointer)+cursor*32);
                if(times[1]>=times[0] && times[3]>=times[2] && times[2]>=times[1])
                    OnGeneration(double((times[1]-times[0])+(times[3]-times[2]))*1000.0/double(frequency),
                                 double(times[3]-times[0])*1000.0/double(frequency));
                if(w.correlation.submitted) {
                    if(const auto r=w.correlation.Resolve(times[0],times[3])) {
                        flight::Time(flight::Timing::GenerationQueue,r->queueMs);
                        flight::Time(flight::Timing::GenerationTotal,r->totalMs);
                        if(w.inputsPending)flight::Time(flight::Timing::GenerationQueueInputsPending,r->queueMs);
                        else if(w.inputsKnown)flight::Time(flight::Timing::GenerationQueueInputsReady,r->queueMs);
                        if(w.captureTiming) {
                            const auto cap=w.captureTiming->Read();
                            const auto enqueue=double(w.correlation.submitted)*1000.0/double(w.correlation.cpuHz);
                            const auto split=cap?AttributeCaptureStart(cap->endQpcMs,enqueue,enqueue+r->queueMs):std::nullopt;
                            if(cap && split) {
                                source_timing::Report(w.captureTiming->source,*cap,enqueue,enqueue+r->queueMs);
                                flight::Count(flight::Event::CaptureProbePaired);
                                flight::Time(flight::Timing::CaptureQueue,cap->queueMs);
                                flight::Time(flight::Timing::CaptureGpu,cap->spanMs);
                                flight::Time(flight::Timing::CaptureTotal,cap->totalMs);
                                flight::Time(flight::Timing::GenerationBeforeCaptureReady,split->beforeReadyMs);
                                flight::Time(flight::Timing::GenerationAfterCaptureReady,split->afterReadyMs);
                            } else flight::Count(flight::Event::CaptureProbeInvalid);
                        }
                    } else flight::Count(flight::Event::GenerationCalibrationInvalid);
                }
                D3D12_RANGE none{};readback->Unmap(0,&none);
            }
        }
        w.timing=false;w.correlation={};w.probeCapture.Reset();
        w.probe=timed && flight::Enabled() && routing.Private() && ((probeSerial++%16==0) || captureTiming);
        w.captureTiming=w.probe?std::move(captureTiming):nullptr;
        if(w.probe){w.probeCapture=colorReady;w.probeCaptureValue=colorReadyValue;}
        if(FAILED(w.allocator->Reset()) || FAILED(w.list->Reset(w.allocator.Get(),nullptr)) ||
           FAILED(w.finishAllocator->Reset()) || FAILED(w.finish->Reset(w.finishAllocator.Get(),nullptr)))return false;
        w.descriptors=0;w.inputs[0]=inputs[0];w.inputs[1]=inputs[1];w.colorLease=routing.Private()?std::move(colorLease):std::shared_ptr<void>{};
        // Any diagnostic capture is ordered behind the same input waits, and
        // every subsequent return path signals/retains this work's leases.
        CaptureFrame(device.Get(),queue.Get(),colors,inputs,cameras);
        auto* list=w.list.Get();if(timed)list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4);
        if(mode==Backend::DlssG) {
            w.finish->Close();
            for(unsigned i=0;i<2;++i) {
                if(!RecordDlss(list,*eyes[i],colors[i],*inputs[i],reset)) {
                    failed=true;Status("DLSS Frame Generation evaluation failed");Submit(list,w);return false;
                }
            }
            if(timed) {
                for(unsigned q=1;q<4;++q)list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4+q);
                list->ResolveQueryData(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4,4,readback.Get(),cursor*32);
            }
            const bool submitted=Submit(list,w);
            if(timed)lastTimingFence=value;
            if(!submitted) {failed=true;return false;}
            w.timing=timed;cursor=(cursor+1)%3;return true;
        }
        const bool nvidia=settings.backend==Backend::Nvidia;
        for(unsigned i=0;i<2 && eyes[0]->opticalFlow;++i) {
            auto& eye=*eyes[i];auto& packed=eye.packed[eye.current];
            if(!captured_color::Shared(colors[i]))Barrier(list,colors[i],kCopy,kRead);packed.To(list,kUav);
            Dispatch(w,list,0,colors[i],DXGI_FORMAT_R8G8B8A8_UNORM,nullptr,nullptr,&packed,nullptr,nullptr,
                eye.width,eye.height,eye.flowWidth,eye.flowHeight,reset);
            packed.To(list,D3D12_RESOURCE_STATE_COMMON);if(!captured_color::Shared(colors[i]))Barrier(list,colors[i],kRead,kCopy);
            if(!nvidia) {
                FfxOpticalflowDispatchDescription d{};d.commandList=ffxGetCommandListDX12(list);
                d.color=Resource(packed,FFX_RESOURCE_STATE_COMMON);d.opticalFlowVector=Resource(eye.rawFlow);
                d.opticalFlowSCD=Resource(eye.scd);d.reset=reset;d.backbufferTransferFunction=FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;
                d.minMaxLuminance={0,1};
                if(!Ffx(ffxOpticalflowContextDispatch(&eye.of,&d),"Optical flow dispatch")) {failed=true;Submit(list,w);w.finish->Close();return false;}
            }
        }
        if(eyes[0]->opticalFlow) {
            // The optical flow's inputs are submitted on their own: NVIDIA's OFA runs between the two lists.
            if(timed)list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4+1);
            const bool packedSubmitted=Submit(list,w);
            if(timed)lastTimingFence=value;
            if(!packedSubmitted) {failed=true;w.finish->Close();return false;}
            const auto packedValue=value;
#ifdef CVR_FRAMEGEN_TEST_RECEIPTS
            testPackedValue=packedValue;
#endif
            if(nvidia && !reset)for(auto& eye:eyes) {
                if(!eye->nv.Run(fence.Get(),packedValue,eye->current) || FAILED(queue->Wait(eye->nv.fence.Get(),eye->nv.value))) {
                    failed=true;Status("NVIDIA optical flow failed");w.finish->Close();return false;
                }
            }
            if(overlapConsumer) {
                if(consumerFence && consumerValue) {
                    flight::Count(flight::Event::SynthesisConsumerDeferred);
                    if(flight::Enabled() && consumerFence->GetCompletedValue()<consumerValue)
                        flight::Count(flight::Event::SynthesisConsumerLatePending);
                }
                if(!routing.WaitConsumer(consumerFence.Get(),consumerValue)) {
                    failed=true;flight::Count(flight::Event::SynthesisSyncFailed);
                    w.finish->Close();return false;
                }
            }
            list=w.finish.Get();
            if(timed)list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4+2);
        } else {
            w.finish->Close();   // no optical flow: one list for the whole pair
            if(timed)for(unsigned q=1;q<3;++q)list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4+q);
        }
        for(unsigned i=0;i<2;++i) {
            auto& eye=*eyes[i];
            if(eye.opticalFlow) {
                const auto rawState=eye.rawFlow.state;eye.rawFlow.To(list,kRead);
                Dispatch(w,list,1,nullptr,DXGI_FORMAT_R8G8B8A8_UNORM,&eye.rawFlow,nullptr,nullptr,&eye.flow,nullptr,
                    eye.rawFlow.width,eye.rawFlow.height,eye.flow.width,eye.flow.height,reset);
                eye.rawFlow.To(list,rawState);
            } else Dispatch(w,list,1,nullptr,DXGI_FORMAT_R8G8B8A8_UNORM,nullptr,nullptr,nullptr,&eye.flow,&eye.scd,
                1,1,eye.flow.width,eye.flow.height,true,true);
            if(nvidia && eye.opticalFlow) {
                auto& current=eye.packed[eye.current];auto& previous=eye.packed[reset?eye.current:(eye.current^1)];
                current.To(list,kRead);if(&previous!=&current)previous.To(list,kRead);
                Dispatch(w,list,2,current.resource.Get(),current.format,nullptr,&previous,nullptr,nullptr,&eye.scd,
                    eye.flowWidth,eye.flowHeight,3,1,reset);
                current.To(list,D3D12_RESOURCE_STATE_COMMON);if(&previous!=&current)previous.To(list,D3D12_RESOURCE_STATE_COMMON);
            }
            PrepareInputs(w,list,eye,*inputs[i]);
            if(!eye.Interpolate(list,colors[i],*inputs[i],frameMs,reset,frameId,nvidia)) {failed=true;Submit(list,w);return false;}
            if(nvidia && eye.opticalFlow)eye.current^=1;
        }
        if(timed) {
            list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4+3);
            list->ResolveQueryData(queries.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*4,4,readback.Get(),cursor*32);
        }
        const bool finishedSubmitted=Submit(list,w);
        if(timed)lastTimingFence=value;
        if(!finishedSubmitted) {failed=true;return false;}
        w.timing=timed;cursor=(cursor+1)%3;return true;
    }
};
namespace {
// NVIDIA's optical-flow engine needs an NVIDIA GPU and its driver library. Anywhere else (AMD, Intel)
// the FidelityFX optical flow stands in, so the NVIDIA setting -- the default -- still generates.
bool NvidiaOpticalFlow(ID3D12Device* device) {
    static ID3D12Device* checked{};static bool available{};
    if(device==checked)return available;
    checked=device;available=false;
    ComPtr<IDXGIFactory4> factory;ComPtr<IDXGIAdapter1> adapter;DXGI_ADAPTER_DESC1 desc{};
    if(SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
       SUCCEEDED(factory->EnumAdapterByLuid(device->GetAdapterLuid(),IID_PPV_ARGS(&adapter))) &&
       SUCCEEDED(adapter->GetDesc1(&desc)) && desc.VendorId==0x10DE)
        if(const HMODULE module=LoadLibraryExW(L"nvofapi64.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32)) {available=true;FreeLibrary(module);}
    // TEST ONLY: cvr_simulate_amd.txt next to the executable makes an NVIDIA GPU take the AMD path.
    if(available && cvr::TestMarker("cvr_simulate_amd.txt")) {available=false;desc.VendorId=0x1002;Log("[framegen] TEST: non-NVIDIA GPU simulated (cvr_simulate_amd.txt)\n");}
    Log("[framegen] GPU vendor 0x%04X: NVIDIA optical flow %s\n",unsigned(desc.VendorId),available?"available":"unavailable, FidelityFX optical flow is used");
    return available;
}
}
Generator::Generator()=default;
Generator::~Generator() {
    // Session teardown normally drained the queue. Never free an unknown live
    // allocation if a device shutdown bypassed that drain.
    if(impl && !impl->Idle()) {Log("[framegen] retaining in-flight resources through shutdown\n");impl.release();}
}
void Generator::Shutdown() {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557){v557->Shutdown();v557.reset();}
#endif
    if(!impl || !impl->routing.Private())return;
    // Capture leases call back into the manager. Retire them while that owner
    // and its lease table still exist, never during member destruction later.
    const auto deadline=GetTickCount64()+2000;
    while(!impl->Idle() && !impl->untrackedGpuWork && GetTickCount64()<deadline)Sleep(1);
    if(!Reset()) {
        Log("[framegen-queue] bounded shutdown: retaining unfinished synthesis resources and capture pins\n");
        impl.release(); // GPU/device failure: no unsafe free and no late owner callback
    }
}
bool Generator::Idle() const {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557 && !v557->Idle())return false;
#endif
    return !impl || impl->Idle();
}
void Generator::Poll() {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)v557->Poll();
#endif
    if(impl)impl->Poll();
}
bool Generator::Reset() {if(!Idle())return false;
#ifdef CVR_V557_RECONSTRUCTION
    v557.reset();
#endif
    impl.reset();return true;
}
uint64_t Generator::Bytes() const {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)return v557->Bytes();
#endif
    uint64_t result=0;if(impl)for(const auto& eye:impl->eyes)if(eye)result+=eye->Bytes();return result;}
ID3D12Resource* Generator::Output(unsigned eye) const {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)return v557->Output(eye);
#endif
    return impl && eye<2 && impl->eyes[eye] ? impl->eyes[eye]->output.resource.Get() : nullptr;}
#ifdef CVR_FRAMEGEN_TEST_RECEIPTS
bool Generator::TestPackedReady() const {return impl && impl->testPackedValue && impl->fence->GetCompletedValue()>=impl->testPackedValue;}
bool Generator::TestOutputReady() const {return impl && impl->signaledValue && impl->fence->GetCompletedValue()>=impl->signaledValue;}
#endif
bool Generator::CanPrepare(ID3D12Device* device,ID3D12CommandQueue* queue,ID3D12Resource* const colors[2],
    const std::shared_ptr<const Inputs> (&inputs)[2],const Settings& settings) const {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)return settings.backend==Backend::NvidiaV557 && v557->Compatible(device,queue,colors,settings) && v557->CanPrepare();
#endif
    if(!impl || impl->failed || !impl->routing.Private() || impl->mode!=Backend::Nvidia ||
       impl->device.Get()!=device || impl->routing.Producer()!=queue || !colors[0] || !colors[1] || !inputs[0] || !inputs[1])return false;
    ID3D12Resource* pair[]{colors[0],colors[1]};
    if(!impl->Compatible(pair,inputs,settings))return false; // never create/reconfigure SDK during a submit hold
    const auto& next=impl->work[impl->cursor];
    return !next.fence || (impl->fence->GetCompletedValue()!=UINT64_MAX && impl->fence->GetCompletedValue()>=next.fence);
}
bool Generator::OutputReady() const {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)return v557->OutputReady();
#endif
    if(!impl || !impl->fence || !impl->signaledValue)return false;
    const auto done=impl->fence->GetCompletedValue();return done!=UINT64_MAX && done>=impl->signaledValue;
}
bool Generator::JoinOutput(ID3D12CommandQueue* consumer){
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)return v557->JoinOutput(consumer);
#endif
    if(!impl || !impl->signaledValue)return false;
    if(impl->routing.JoinOutput(impl->fence.Get(),impl->signaledValue,consumer))return true;
    impl->failed=true;impl->untrackedGpuWork=true;flight::Count(flight::Event::SynthesisSyncFailed);return false;
}
void Generator::ConsumerFence(ID3D12Fence* fence,uint64_t value) {
#ifdef CVR_V557_RECONSTRUCTION
    if(v557)v557->ConsumerFence(fence,value);
#endif
    if(impl) {impl->consumerFence=fence;impl->consumerValue=value;}}
bool Generator::Generate(ID3D12Device* device,ID3D12CommandQueue* queue,ID3D12Resource* colors[2],
    const std::shared_ptr<const Inputs> (&inputs)[2],const EyePair (&cameras)[2],const Settings& settings,double frameMs,bool reset,
    uint64_t frameId,bool* interpolated,ID3D12Fence* colorReady,uint64_t colorReadyValue,std::shared_ptr<void> colorLease,ID3D12CommandQueue* consumer,std::shared_ptr<const CaptureTiming> captureTiming,bool deferOutputJoin) {
    if(interpolated)*interpolated=false;
    if(!device || !queue || !colors[0] || !colors[1] || !inputs[0] || !inputs[1])return false;
    Settings effective=settings;
    // DLSS-G needs an RTX 40/50 and the SDK in this build; FidelityFX runs anywhere.
    if(effective.backend==Backend::DlssG && !dlssg::Available(device))effective.backend=Backend::FidelityFX;
    if(UsesNvidiaOFA(effective.backend) && (!effective.flowScale || !NvidiaOpticalFlow(device)))effective.backend=Backend::FidelityFX;
#ifndef CVR_V557_RECONSTRUCTION
    if(effective.backend==Backend::NvidiaV557)effective.backend=Backend::Nvidia;
#endif
    PublishActiveBackend(effective.backend);
#ifdef CVR_V557_RECONSTRUCTION
    if(effective.backend==Backend::NvidiaV557){
        if(impl || (v557 && !v557->Compatible(device,queue,colors,effective))){if(!Reset())return false;reset=true;}
        if(!v557){v557=std::make_unique<V557Generator>();
            if(!v557->Create(device,queue,colors,effective)){Status("V557 reconstruction initialization failed");return false;}
            reset=true;
        }
        if(v557->Failed()){Status("V557 failed; toggle framegen off/on");return false;}
        bool result=v557->Generate(colors,cameras,reset,interpolated,colorReady,colorReadyValue,std::move(colorLease),inputs,std::move(captureTiming));
        if(!deferOutputJoin && !v557->JoinOutput(consumer))result=false;
        if(MetricsEnabled())SetVram(Bytes()+InputVram());
        return result;
    }
    if(v557){if(!Reset())return false;reset=true;}
#endif
    if(impl && (impl->device.Get()!=device || impl->routing.Producer()!=queue || impl->isolatedRequested!=(refresh::Targeted() && effective.backend!=Backend::DlssG) || !impl->Compatible(colors,inputs,effective))) {if(!Reset())return false;reset=true;}
    if(impl)impl->RetireTiming();
    if(impl && impl->failed) {Status("Backend failed; toggle generation off/on or change the backend");return false;}
    if(!impl) {
        impl=std::make_unique<Impl>();
        if(!impl->Create(device,queue,colors,inputs,effective)) {impl->failed=true;Status("Framegen backend initialization failed");return false;}
        reset=true;
    }
    auto result=impl->Generate(colors,inputs,frameMs,reset,frameId,cameras,colorReady,colorReadyValue,std::move(colorLease),std::move(captureTiming));
    // Join all successfully signaled work, including priming and partial
    // dispatch failures, before the caller copies images on its display queue.
    // Never insert a wait on a value whose Signal failed.
    if(!deferOutputJoin && !impl->routing.JoinOutput(impl->fence.Get(),impl->signaledValue,consumer)) {
        impl->failed=true;impl->untrackedGpuWork=true;flight::Count(flight::Event::SynthesisSyncFailed);result=false;
    }
    if(result && interpolated)*interpolated=!reset;
    if(MetricsEnabled())SetVram(Bytes()+InputVram());return result;
}
}
