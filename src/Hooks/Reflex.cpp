#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
#include "Hooks/Reflex.hpp"
#if CVR_RUST_VRIK
#include "Hooks/Hook.hpp"
// rust/crates/cvr-rig/src/vrik/reflex_hook.rs: the detour, the selected mode and the timing report.
extern "C" bool cvr_hook_reflex_install();
CVR_HOOK("NvidiaReflex",::cvr::hooks::Stage::Boot,89,cvr_hook_reflex_install);
#else
#include "Hooks/ReflexOptions.hpp"
#include "Hooks/ReflexTiming.hpp"
#include "Hooks/Hook.hpp"
#include "Utils/DebugGate.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <MinHook.h>
#include <atomic>
#include <cstring>
#include <mutex>
#include <sstream>

namespace cvr::reflex {
namespace {
std::atomic<int> selectedMode{DefaultMode},appliedMode{-2};
using SetOptionsFn=int32_t(__fastcall*)(const Options*);
SetOptionsFn original{};
std::atomic<unsigned> timingRequest{0}; // idle / requested / collecting
std::mutex reportMutex;
std::string timingReport="{\"pending\":false,\"status\":\"not_requested\"}";

std::string CaptureTiming() {
    // This runs after native SetOptions on the same render thread, inside the
    // engine's existing Streamline critical section. GetState is not thread safe.
    if(!cvr::RuntimeDiagnosticsEnabled())return "{\"error\":\"diagnostics_disabled\"}";
    using FeatureFn=int32_t(*)(uint32_t,const char*,void**);
    using StateFn=int32_t(*)(TimingState*);
    const auto module=GetModuleHandleW(L"sl.interposer.dll");
    auto feature=reinterpret_cast<FeatureFn>(module?GetProcAddress(module,"slGetFeatureFunction"):nullptr);
    if(!feature)return "{\"error\":\"streamline_unavailable\"}";
    StateFn stateFn{};
    const auto lookup=feature(3,"slReflexGetState",reinterpret_cast<void**>(&stateFn));
    if(lookup || !stateFn)return "{\"error\":\"reflex_unavailable\"}";
    TimingState state;
    const auto result=stateFn(&state);
    std::ostringstream out;
    out<<"{\"pending\":false,\"result\":"<<result<<",\"available\":"<<(state.latencyReportAvailable?"true":"false")
       <<",\"appliedMode\":"<<appliedMode.load(std::memory_order_relaxed)<<",\"stamp\":"<<GetTickCount64()<<",\"frames\":[";
    bool first=true;
    if(!result && state.latencyReportAvailable)for(const auto& f:state.frames) {
        if(!f.frameId)continue;
        if(!first)out<<',';first=false;
        out<<"{\"id\":"<<f.frameId<<",\"input\":"<<f.input<<",\"simStart\":"<<f.simStart<<",\"simEnd\":"<<f.simEnd
           <<",\"renderStart\":"<<f.renderStart<<",\"renderEnd\":"<<f.renderEnd<<",\"presentStart\":"<<f.presentStart<<",\"presentEnd\":"<<f.presentEnd
           <<",\"driverStart\":"<<f.driverStart<<",\"driverEnd\":"<<f.driverEnd<<",\"queueStart\":"<<f.queueStart<<",\"queueEnd\":"<<f.queueEnd
           <<",\"gpuStart\":"<<f.gpuStart<<",\"gpuEnd\":"<<f.gpuEnd<<",\"gpuActiveUs\":"<<f.gpuActiveUs<<",\"gpuFrameUs\":"<<f.gpuFrameUs<<'}';
    }
    out<<"]}";return out.str();
}

int32_t __fastcall SetOptions(const Options* options) {
    const int mode=selectedMode.load(std::memory_order_relaxed);
    const bool known=options && KnownOptions(*options);
    Options adjusted;
    const auto* submitted=options;
    if(known && OverrideOptions(*options,mode,OpenXRManager::Get().IsSessionRunning(),adjusted))
        submitted=&adjusted;
    const int32_t result=original(submitted);
    if(known && result==0)appliedMode.store(submitted->mode,std::memory_order_relaxed);
    if(timingRequest.load(std::memory_order_acquire)==1) {
        unsigned expected=1;
        if(timingRequest.compare_exchange_strong(expected,2,std::memory_order_acq_rel)) {
            auto report=CaptureTiming();
            std::lock_guard lock(reportMutex);timingReport=std::move(report);
            timingRequest.store(0,std::memory_order_release);
        }
    }
    return result;
}
bool Install() {
    auto* base=reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    auto* target=base+0x22904f0;
    // Native slReflexSetOptions wrapper. Fail closed on a different executable.
    const uint8_t expected[]={0x40,0x53,0x48,0x83,0xec,0x20,0x48,0x8b,0x05,0x63,0x42,0x0a,0x02,0x48,0x8b,0xd9};
    if(std::memcmp(target,expected,sizeof(expected)))return false;
    if(MH_CreateHook(target,reinterpret_cast<void*>(&SetOptions),reinterpret_cast<void**>(&original))!=MH_OK)return false;
    if(MH_EnableHook(target)!=MH_OK) {MH_RemoveHook(target);original=nullptr;return false;}
    return true;
}
CVR_HOOK("NvidiaReflex",cvr::hooks::Stage::Boot,89,Install);
}
int GetMode() {return selectedMode.load(std::memory_order_relaxed);}
void SetMode(int mode) {selectedMode.store(NormalizeMode(mode),std::memory_order_relaxed);}
int GetAppliedMode() {return appliedMode.load(std::memory_order_relaxed);}
std::string TimingReport(bool request) {
    if(!cvr::RuntimeDiagnosticsEnabled())return "{\"error\":\"diagnostics_disabled\"}";
    std::lock_guard lock(reportMutex);
    if(request && timingRequest.load(std::memory_order_acquire)==0) {
        timingReport="{\"pending\":true}";
        timingRequest.store(1,std::memory_order_release);
    }
    return timingReport;
}
}
#endif
#endif
