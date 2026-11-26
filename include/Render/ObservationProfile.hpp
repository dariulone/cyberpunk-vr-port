#pragma once
#include <windows.h>
#include <atomic>
#include <cstdint>

extern "C" {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES
__declspec(dllexport) inline std::atomic<uint32_t> CyberpunkVR_ObservationProfileEnable{};
__declspec(dllexport) inline std::atomic<uint64_t> CyberpunkVR_ObservationProfileStats[16]{};
#else
// The plugin's: rust/crates/cvr-render/src/render_moved_exports.rs.
extern std::atomic<uint32_t> CyberpunkVR_ObservationProfileEnable;
extern std::atomic<uint64_t> CyberpunkVR_ObservationProfileStats[16];
#endif
}
namespace cvr::stereo::observation_profile {
enum class Phase:uint32_t {Cbv,OtherDescriptor,CopyDescriptors,BindHeaps,TouchUpload,UnwindUpload,BindPipeline,BindRoot};
// Opt-in, inclusive CPU wall time. Nested phases and concurrent threads must not
// be added into a frame-time or CPU-utilization estimate.
class Scope {
    uint32_t index{};LARGE_INTEGER begin{};bool enabled{};
public:
    explicit Scope(Phase phase):index(uint32_t(phase)*2),enabled(CyberpunkVR_ObservationProfileEnable.load(std::memory_order_relaxed)!=0){
        if(enabled)QueryPerformanceCounter(&begin);
    }
    ~Scope(){if(enabled){LARGE_INTEGER end{};QueryPerformanceCounter(&end);
        CyberpunkVR_ObservationProfileStats[index].fetch_add(1,std::memory_order_relaxed);
        CyberpunkVR_ObservationProfileStats[index+1].fetch_add(uint64_t(end.QuadPart-begin.QuadPart),std::memory_order_relaxed);}}
    Scope(const Scope&)=delete;
    Scope& operator=(const Scope&)=delete;
};
}
