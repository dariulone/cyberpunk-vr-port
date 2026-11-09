#include "Runtimes/HudQuad.hpp"
#include "Runtimes/PresentationQueue.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Render/GpuStageProfile.hpp"
#include "Core/LiveControls.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <chrono>
#include <cstdio>
#include <stdexcept>
#include <iostream>
#include <thread>

using Microsoft::WRL::ComPtr;
using namespace cvr::hud;
LiveControls g_liveControls{};
extern "C" int CyberpunkVR_RuntimeDiagnostics=0;
extern "C" std::atomic<uint32_t> CyberpunkVR_HudSubmitRing;
extern "C" cvr::hud::PoseDebug CyberpunkVR_HudPoseDebug;
void Log(const char*,...) {}
extern "C" int CyberpunkVR_InteractionAnchor=0;
extern "C" int CyberpunkVR_HudClearance=0;
extern "C" int CyberpunkVR_HudClearanceHands=0;
extern "C" float CyberpunkVR_HudClearanceGrow=0;
extern "C" float CyberpunkVR_HudClearanceHandsScale=1;
namespace cvr::interaction {float TargetDepth(uint64_t){return 0;}}
namespace cvr::hud::clearance {
float Published(uint64_t){return 0;}
float GrowFactor(float,float,float){return 1;}
}
OpenXRManager& OpenXRManager::Get(){throw std::runtime_error("hand tracking outside HUD queue test");}
bool OpenXRManager::GetHandPose(int,OpenXRHeadPose*) const{return false;}
namespace cvr::framegen::flight {
void Time(Timing,double){}
void Display(const DisplayDeadline&,double,double){}void Count(Event){}}
void Check(bool value,const char* message) {if(!value)throw std::runtime_error(message);}
void Hr(HRESULT value) {Check(SUCCEEDED(value),"D3D12 call failed");}
namespace cvr::gpu::profile {
BatchPtr Begin(ID3D12GraphicsCommandList*) {return {};}
Scope::Scope(const BatchPtr&,Stage) {}
Scope::~Scope() {}
void Submitted(BatchPtr,ID3D12CommandQueue*,ID3D12Fence*,UINT64) {}
}
namespace mock {
ID3D12Device* device{};
std::array<ComPtr<ID3D12Resource>,3> textures;
std::shared_ptr<Frame> source;
bool ready{},acquired{},waited{},timeout{};
uint32_t nextImage{},image{},acquires{},releases{},destroys{};
XrDuration lastTimeout{};
}
namespace cvr::hud {
std::shared_ptr<Frame> Latest(Channel) {return mock::source;}
void SetConsumerReady(bool ready,Channel) {mock::ready=ready;}
}
extern "C" {
XrResult XRAPI_CALL xrEnumerateSwapchainFormats(XrSession,uint32_t capacity,uint32_t* count,int64_t* values) {
    *count=1;if(capacity)values[0]=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrCreateSwapchain(XrSession,const XrSwapchainCreateInfo* ci,XrSwapchain* out) {
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width=ci->width;desc.Height=ci->height;desc.Format=DXGI_FORMAT_R8G8B8A8_TYPELESS;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    for(auto& texture:mock::textures)Hr(mock::device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&texture)));
    *out=reinterpret_cast<XrSwapchain>(1);mock::nextImage=0;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrEnumerateSwapchainImages(XrSwapchain,uint32_t capacity,uint32_t* count,XrSwapchainImageBaseHeader* out) {
    *count=3;if(capacity)for(unsigned i=0;i<3;++i)reinterpret_cast<XrSwapchainImageD3D12KHR*>(out)[i].texture=mock::textures[i].Get();return XR_SUCCESS;
}
XrResult XRAPI_CALL xrAcquireSwapchainImage(XrSwapchain,const XrSwapchainImageAcquireInfo*,uint32_t* image) {
    Check(!mock::acquired,"duplicate acquisition");mock::acquired=true;mock::waited=false;
    *image=mock::image=mock::nextImage++%3;++mock::acquires;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrWaitSwapchainImage(XrSwapchain,const XrSwapchainImageWaitInfo* info) {
    Check(mock::acquired,"wait without acquire");mock::lastTimeout=info->timeout;
    if(mock::timeout)return XR_TIMEOUT_EXPIRED;mock::waited=true;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrReleaseSwapchainImage(XrSwapchain,const XrSwapchainImageReleaseInfo*) {
    Check(mock::acquired && mock::waited,"unwaited image released");mock::acquired=false;++mock::releases;return XR_SUCCESS;
}
XrResult XRAPI_CALL xrDestroySwapchain(XrSwapchain) {
    Check(!mock::acquired,"acquired image destroyed");++mock::destroys;
    for(auto& texture:mock::textures)texture.Reset();return XR_SUCCESS;
}
}
int main() try {
    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));mock::device=device.Get();
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC qd{};Hr(device->CreateCommandQueue(&qd,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> done,gate;Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&done)));
    Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)));uint64_t doneValue{};
    auto drain=[&] {
        Hr(queue->Signal(done.Get(),++doneValue));HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
        Hr(done->SetEventOnCompletion(doneValue,event));auto status=WaitForSingleObject(event,5000);CloseHandle(event);Check(status==WAIT_OBJECT_0,"GPU timeout");
    };
    auto frame=[&](uint64_t serial,unsigned size=32) {
        auto f=std::make_shared<Frame>();f->serial=serial;f->stamp=GetTickCount64();f->masked=true;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=desc.Height=size;
        desc.DepthOrArraySize=desc.MipLevels=1;desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;desc.SampleDesc.Count=1;
        Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_SOURCE,nullptr,IID_PPV_ARGS(&f->canvas)));
        return f;
    };
    g_liveControls.xrHudStereoDepth=1;g_liveControls.xrHudFollowMode=0;
    XrPosef head{{0,0,0,1},{0,1.7f,0}};XrTime clock=1000000000;
    Quad quad(Channel::Interaction);
    auto prepare=[&](bool enabled=true) {
        clock+=11111111;
        const auto begin=std::chrono::steady_clock::now();
        auto result=quad.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),device.Get(),queue.Get(),enabled,true,head,1,clock);
        if(CyberpunkVR_HudSubmitRing)Check(std::chrono::steady_clock::now()-begin<std::chrono::milliseconds(80),"pipelined prepare blocked");
        return reinterpret_cast<const XrCompositionLayerQuad*>(result);
    };
    // All three pending sources must survive until their own copy completes.
    Hr(queue->Wait(gate.Get(),1));
    std::array<std::weak_ptr<Frame>,3> old;
    for(unsigned i=0;i<3;++i){mock::source=frame(i+1);old[i]=mock::source;Check(prepare()!=nullptr,"free slot unavailable");}
    Check(mock::acquires==3 && mock::releases==3,"ring did not submit three copies");
    mock::source=frame(4);mock::source->masked=false;mock::source->lootVisible=true;
    // Last published image was masked, not loot. Busy fallback must use that
    // metadata, and continue head follow without a new acquisition.
    head.orientation={0,.3f,0,.9539392f};
    Check(prepare()!=nullptr && mock::ready,"busy fallback hid the prior image");
    Check(mock::acquires==3,"busy ring acquired another image");
    for(auto& f:old)Check(!f.expired(),"in-flight HUD source released early");
    Hr(gate->Signal(1));drain();
    Check(prepare()==nullptr && mock::ready,"first unmasked publication handshake changed");
    for(auto& f:old)Check(f.expired(),"completed sources were not retired");
    drain();mock::source=frame(5);Check(prepare()!=nullptr,"new masked publication missing");drain();
    const auto reused=mock::acquires;Check(prepare()!=nullptr && mock::acquires==reused,"same serial copied twice");

    // A nonblocking XR timeout keeps the acquisition and prior published image.
    mock::source=frame(6);mock::timeout=true;
    Check(prepare()!=nullptr && mock::acquired && mock::lastTimeout==0,"XR timeout fallback failed");
    auto pendingAcquires=mock::acquires,pendingReleases=mock::releases;
    Check(prepare()!=nullptr && mock::acquires==pendingAcquires && mock::releases==pendingReleases,"timeout duplicated acquisition/release");
    mock::timeout=false;Check(prepare()!=nullptr && !mock::acquired,"acquisition was not resumed");drain();
    Check(prepare(false)==nullptr && !mock::ready,"disable did not suppress the layer");
    mock::source=frame(7);Check(prepare()!=nullptr,"re-enable failed");drain();

    // Resize with a pending acquisition must wait/release it before destruction.
    mock::source=frame(8);mock::timeout=true;Check(prepare()!=nullptr && mock::acquired,"pending resize setup");
    mock::source=frame(9,64);const auto destroyedBefore=mock::destroys;
    Check(prepare()==nullptr && mock::destroys==destroyedBefore,"unready image destroyed on resize");
    mock::timeout=false;Check(prepare()!=nullptr && mock::destroys==destroyedBefore+1,"resize recovery failed");drain();
    mock::source=frame(10,64);CyberpunkVR_HudSubmitRing=0;Check(prepare()!=nullptr,"legacy A/B path failed");drain();
    CyberpunkVR_HudSubmitRing=1;mock::source.reset();Check(prepare()==nullptr && !mock::ready,"missing source retained HUD");
    quad.Shutdown();Check(!mock::acquired && mock::acquires==mock::releases,"unbalanced XR lifecycle");

    // Readiness must distinguish preparing a cached layer from publishing a
    // fresh source and from the frame loop successfully submitting that layer.
    Quad mainHud;
    CyberpunkVR_RuntimeDiagnostics=1;mock::source=frame(11,64);
    auto mainLayer=mainHud.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),true,true,head,1,clock+11111111);
    Check(mainLayer && CyberpunkVR_HudPoseDebug.sourceSerial==mock::source->serial &&
        CyberpunkVR_HudPoseDebug.sourceTick==mock::source->stamp,"HUD source evidence missing");
    const auto observed=CyberpunkVR_HudPoseDebug.frames;
    drain();mainHud.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),true,true,head,1,clock+22222222);
    Check(CyberpunkVR_HudPoseDebug.frames==observed+1 && CyberpunkVR_HudPoseDebug.sourceSerial==11,
        "Cached HUD incorrectly reported a new source");
    Check(CyberpunkVR_HudSubmittedFrames==0,"Preparation is not a successful xrEndFrame");
    CyberpunkVR_RuntimeDiagnostics=0;drain();mainHud.Shutdown();mock::source.reset();

    // Exercise the actual Quad::Prepare/UpdateImage on a different queue.
    Check(cvr::xrqueue::Initialize(device.Get(),queue.Get(),true) && cvr::xrqueue::Private(),"private HUD binding unavailable");
    Quad isolatedHud;
    mock::source=frame(20,64);
    mock::source->readyFence=done;
    mock::source->readyValue=++doneValue;
    Hr(queue->Signal(done.Get(),doneValue));
    Hr(queue->Wait(gate.Get(),2)); // next game's RT/NR frame must not hold this HUD
    const auto tailValue=++doneValue;Hr(queue->Signal(done.Get(),tailValue));
    auto layer=isolatedHud.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),true,true,head,1,clock+33333333);
    Check(layer && mock::ready,"private HUD source not copied");
    auto pin=std::weak_ptr<Frame>(mock::source);mock::source.reset();
    Check(!pin.expired(),"private HUD source dropped before retirement");
    const auto mark=cvr::xrqueue::Mark();cvr::xrqueue::Drain();
    Check(cvr::xrqueue::Ready(mark),"private HUD copy blocked behind unrelated game tail");
    Check(done->GetCompletedValue()<tailValue,"HUD copy should finish while game queue remains blocked");
    isolatedHud.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),false,true,head,1,clock+44444444);
    Check(pin.expired(),"completed private HUD source not retired");
    isolatedHud.Shutdown();Hr(gate->Signal(2));drain();
    Quad missingReceipt;mock::source=frame(21,64);
    Check(!missingReceipt.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),
        device.Get(),queue.Get(),true,true,head,1,clock+55555555),"missing receipt must not publish an unsafe HUD canvas");
    Check(!mock::acquired,"missing receipt leaves a held XR image");
    missingReceipt.Shutdown();mock::source.reset();cvr::xrqueue::Shutdown();

    ComPtr<ID3D12InfoQueue> info;
    if(SUCCEEDED(device.As(&info)))for(UINT64 i=0;i<info->GetNumStoredMessages();++i) {
        SIZE_T bytes{};info->GetMessage(i,nullptr,&bytes);std::vector<uint8_t> storage(bytes);auto* message=reinterpret_cast<D3D12_MESSAGE*>(storage.data());
        info->GetMessage(i,message,&bytes);
        if(message->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<message->pDescription<<'\n';throw std::runtime_error("D3D12 validation failed");}
    }
    std::cout<<"PASS delayed GPU, three fenced slots, source lifetime, busy/stale metadata, XR timeout recovery, repeat, disable, resize and legacy switch\n";
    return 0;
} catch(const std::exception& e) {std::cerr<<e.what()<<'\n';return 1;}
