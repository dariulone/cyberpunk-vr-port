#include "Runtimes/PresentationQueue.hpp"
#include "Framegen/FlightRecorder.hpp"
#include <memory>
#include <algorithm>
#include <psapi.h>
extern void Log(const char*,...);
namespace cvr::xrqueue {
namespace {
std::unique_ptr<GpuQueue> route;
bool ExternalLayer() {
    HMODULE modules[1024]{};DWORD bytes{};
    if(!EnumProcessModules(GetCurrentProcess(),modules,sizeof(modules),&bytes))return true;
    for(size_t i=0;i<std::min<size_t>(bytes/sizeof(HMODULE),std::size(modules));++i){
        wchar_t name[MAX_PATH]{};GetModuleBaseNameW(GetCurrentProcess(),modules[i],name,MAX_PATH);
        if(_wcsnicmp(name,L"XR_APILAYER_XRFrameBridge",23)==0)return true;
    }
    return false;
}
void Failure(){framegen::flight::Count(framegen::flight::Event::PresentationSyncFailed);Log("[presentation-queue] synchronization failed\n");}
}
bool Initialize(ID3D12Device* device,ID3D12CommandQueue* queue,bool steamvr) {
    Shutdown();route=std::make_unique<GpuQueue>();
    const bool external=ExternalLayer();
    if(!route->Initialize(device,queue,steamvr&&!external)){route.reset();return false;}
    Log("[presentation-queue] application=%p runtime=%p private=%d create=0x%08X external=%d\n",
        queue,route->Queue(),route->Private(),unsigned(route->CreationResult()),external);
    return true;
}
ID3D12CommandQueue* Queue(){return route?route->Queue():nullptr;}
bool Private(){return route&&route->Private();}
bool WaitCapture(ID3D12Fence* fence,uint64_t value){
    if(!route)return false;
    const bool ok=route->WaitCapture(fence,value);if(!ok)Failure();return ok;
}
bool WaitPublished(ID3D12Fence* fence,uint64_t value){
    if(!route)return false;
    if(Private())framegen::flight::Count(fence && value && fence->GetCompletedValue()>=value ?
        framegen::flight::Event::OverlaySourceReady:framegen::flight::Event::OverlaySourcePending);
    const bool ok=route->WaitPublished(fence,value);if(!ok)Failure();return ok;
}
XrResult WaitApplicationImage(XrSwapchain swap,const XrSwapchainImageWaitInfo* info){
    const auto result=xrWaitSwapchainImage(swap,info);
    if(result!=XR_SUCCESS)return result; // timeout is not ownership
    if(route && !route->ToApplication()){Failure();return XR_ERROR_RUNTIME_FAILURE;}
    if(Private())framegen::flight::Count(framegen::flight::Event::PresentationApplicationWait);
    return result;
}
XrResult ReleaseApplicationImage(XrSwapchain swap,const XrSwapchainImageReleaseInfo* info){
    if(route && !route->ToRuntime()){Failure();return XR_ERROR_RUNTIME_FAILURE;}
    if(Private())framegen::flight::Count(framegen::flight::Event::PresentationRuntimeWait);
    return xrReleaseSwapchainImage(swap,info);
}
uint64_t Mark(){const auto v=route?route->Mark():0;if(Private()&&!v)Failure();return v;}
bool Ready(uint64_t v){return route&&route->Ready(v);}
namespace {
void CollectProbe(const framegen::DisplayDeadline& d,const std::optional<framegen::CaptureTiming::Result>& gpu){
    using namespace framegen;using E=flight::Event;using T=flight::Timing;
    if(!d.accepted){flight::Count(E::DisplayProbeRejected);return;}
    const auto result=gpu?AttributeDisplay(d,gpu->endQpcMs):std::nullopt;
    if(!result){flight::Count(E::DisplayProbeInvalid);return;}
    const auto& v=*result;
    flight::Display(d,gpu->endQpcMs,gpu->queueMs);
    flight::Count(d.kind==0?E::DisplayReal:d.kind==1?E::DisplaySynthetic:E::DisplayRepeat);
    if(v.futureBoundary)flight::Count(E::DisplayBoundaryFuture);
    flight::Time(T::DisplayQueue,gpu->queueMs);flight::Time(T::DisplayProbeWidth,d.probeWidthMs);
    flight::Time(T::DisplayRemaining,d.remainingMs);
    const auto split=[](double delta,T lead,T lag){flight::Time(delta<=0?lead:lag,std::abs(delta));};
    split(v.gpuFromBoundaryMs,T::DisplayGpuLeadBoundary,T::DisplayGpuLagBoundary);
    split(v.gpuFromHandoffMs,T::DisplayGpuLeadHandoff,T::DisplayGpuLagHandoff);
    split(v.handoffFromTargetMs,T::DisplayHandoffLeadTarget,T::DisplayHandoffLagTarget);
    split(v.gpuFromTargetMs,T::DisplayGpuLeadTarget,T::DisplayGpuLagTarget);
    // Cross-queue calibration and API sampling are finite-precision. Do not call
    // +/-0.25ms a definite miss. Boundary != proof of actual displayed content.
    constexpr double tolerance=.25;
    flight::Count(v.gpuFromHandoffMs < -tolerance?E::DisplayGpuBeforeHandoff:
        v.gpuFromHandoffMs > tolerance?E::DisplayGpuAfterHandoff:E::DisplayHandoffAmbiguous);
    flight::Count(v.handoffFromTargetMs < -tolerance?E::DisplayHandoffEarlyTarget:
        v.handoffFromTargetMs > tolerance?E::DisplayHandoffLateTarget:E::DisplayTargetAmbiguous);
    if(v.gpuFromBoundaryMs > tolerance){
        flight::Count(E::DisplayGpuLateBoundary);
        flight::Count(d.kind==0?E::DisplayRealGpuLate:d.kind==1?E::DisplaySyntheticGpuLate:E::DisplayRepeatGpuLate);
    }else flight::Count(v.gpuFromBoundaryMs < -tolerance?E::DisplayGpuEarlyBoundary:E::DisplayBoundaryAmbiguous);
    const double cpuBoundary=d.handoffMs-v.boundaryMs;
    if(std::abs(cpuBoundary)>tolerance && std::abs(v.gpuFromBoundaryMs)>tolerance)
        flight::Count(cpuBoundary<0?(v.gpuFromBoundaryMs<0?E::DisplayCpuAndGpuEarly:E::DisplayGpuOnlyLate):
            (v.gpuFromBoundaryMs<0?E::DisplayCpuOnlyLate:E::DisplayBothLate));
}
}
framegen::DisplayDeadline* ProbeBegin(bool enabled){
    if(!route || !route->Private())return nullptr;
    route->PollProbes(CollectProbe);
    // Odd stride avoids repeatedly sampling only one member of alternating pairs.
    static unsigned cadence{};
    if(!enabled){cadence=0;return nullptr;}
    if(++cadence<31)return nullptr;cadence=0;
    auto* sample=route->Probe();
    framegen::flight::Count(sample?framegen::flight::Event::DisplayProbeQueued:framegen::flight::Event::DisplayProbeSkipped);
    return sample;
}
void Drain(){if(route){if(!route->Drain())Log("[presentation-queue] bounded drain incomplete\n");route->PollProbes(CollectProbe);}}

void Shutdown(){
    if(route&&!route->Drain()){
        Log("[presentation-queue] retaining unfinished queue/fences through shutdown\n");route.release();
    }else route.reset();
}
}
