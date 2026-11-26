#pragma once
#include "Render/StereoCameraInput.hpp"
#include <atomic>

namespace cvr::stereo {
using CollectorInitialize=uintptr_t(*)(void*,void*,uint8_t,const void*,void*,void*,const void*);
struct CollectorApi {
    void*(*create)(void**);
    void*(*destroy)(void*,uint32_t);
    CollectorInitialize initialize;
};
struct CollectorInput {
    void* viewData{};
    void* scene{};
    uint8_t mode{};
    std::array<uint8_t,15> padding{};
    CameraInput camera;
    std::array<uint8_t,0x330> viewFlags{};
};
// The native wrapper owns its collector, metadata, arrays and locks. Camera and
// view parameters are copied here, while scene/viewData are borrowed only for
// the caller's current native preparation scope. No jobs are started by Create.
#if CVR_RUST_VRIK
static_assert(sizeof(CollectorInput)==0x730 && offsetof(CollectorInput,camera)==0x20 && offsetof(CollectorInput,viewFlags)==0x400,
    "mirrored by rust/crates/cvr-render/src/cpu_collector.rs");
}
// The owner: rust/crates/cvr-render/src/cpu_collector.rs.
extern "C" {
void* cvr_cpu_collector_new(const cvr::stereo::CollectorApi*);
void cvr_cpu_collector_free(void*);
bool cvr_cpu_collector_create(void*,const cvr::stereo::CollectorInput*);
void cvr_cpu_collector_reset(void*);
void* cvr_cpu_collector_storage(const void*);
void* cvr_cpu_collector_metadata(const void*);
const cvr::stereo::CollectorInput* cvr_cpu_collector_inputs(const void*);
}
namespace cvr::stereo {
class NativeCpuCollector {
public:
    explicit NativeCpuCollector(CollectorApi api):rust_(cvr_cpu_collector_new(&api)){}
    ~NativeCpuCollector(){cvr_cpu_collector_free(rust_);}
    NativeCpuCollector(const NativeCpuCollector&)=delete;
    NativeCpuCollector& operator=(const NativeCpuCollector&)=delete;
    bool Create(const CollectorInput& input){return cvr_cpu_collector_create(rust_,&input);}
    void Reset(){cvr_cpu_collector_reset(rust_);}
    void* Storage() const{return cvr_cpu_collector_storage(rust_);}
    void* Metadata() const{return cvr_cpu_collector_metadata(rust_);}
    const CollectorInput* Inputs()const{return cvr_cpu_collector_inputs(rust_);}
private:
    void* rust_;
};
#else
class NativeCpuCollector {
public:
    explicit NativeCpuCollector(CollectorApi api):api_(api){}
    ~NativeCpuCollector();
    NativeCpuCollector(const NativeCpuCollector&)=delete;
    NativeCpuCollector& operator=(const NativeCpuCollector&)=delete;
    bool Create(const CollectorInput&);
    void Reset();
    void* Storage() const;
    void* Metadata() const;
    const CollectorInput* Inputs()const{return owner_?&input_:nullptr;}
private:
    CollectorApi api_;
    void* owner_{};
    CollectorInput input_;
};
#endif
void PollCpuCollector();
bool CaptureCpuFine(void* query,int64_t* range,char partial,void* output,char& result);
bool CaptureCpuScene(void* manager,void* context,void* output,void* query,void* job,int64_t& result);
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_CpuCollectorRequest,CyberpunkVR_CpuCollectorState,CyberpunkVR_CpuCollectorCompatible;
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorResult[8];
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorQueryResult[8];
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_CpuCollectorQueryCompatible;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_CpuCollectorFineCompatible;
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorFineResult[12];
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_CpuCollectorSceneCompatible;
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorSceneResult[12];
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorSceneControl[8];
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_CpuCollectorPeerCompatible;
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorPeerResult[16];
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorPeerQuery[8],CyberpunkVR_CpuCollectorPeerFine[12];
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorPeerScene[12],CyberpunkVR_CpuCollectorPeerControl[8];
__declspec(dllexport) extern float CyberpunkVR_CpuCollectorPeerPlanes[64];
__declspec(dllexport) extern uint32_t CyberpunkVR_CpuCollectorPeerInputDiff[10];
}
