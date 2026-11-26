#include "Render/NativePreparation.hpp"
#include "Render/NativeDepthPreparation.hpp"
#include <windows.h>
#include <mutex>

extern "C" {
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EagerDepthMode{},CyberpunkVR_EagerDepthCompatible{},CyberpunkVR_EagerDepthVerified{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
__declspec(dllexport) extern uint32_t CyberpunkVR_EagerDepthStats[cvr::stereo::eager_depth::CounterCount];
#else
__declspec(dllexport) uint32_t CyberpunkVR_EagerDepthStats[cvr::stereo::eager_depth::CounterCount]{};
#endif
}
#if CVR_RUST_VRIK
// The cache and the interception: rust/crates/cvr-render/src/eager_depth.rs (its plugin table there). A Rust test
// variant attaches the exports above and the preparation run's state here.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES
namespace {
using namespace cvr::stereo;
struct EagerDepthAttach {
    const std::atomic<uint32_t>* mode;const std::atomic<uint32_t>* compatible;const std::atomic<uint32_t>* verified;uint32_t* stats;
    bool(*runActive)();bool(*active)();
    const std::atomic<uint32_t>* pairState;const std::atomic<uint32_t>* runFrames;const uint64_t* pairData;const std::atomic<uint32_t>* runDepth;
};
static_assert(sizeof(std::atomic<uint32_t>)==4);
const EagerDepthAttach eagerDepthTable{&CyberpunkVR_EagerDepthMode,&CyberpunkVR_EagerDepthCompatible,&CyberpunkVR_EagerDepthVerified,
    CyberpunkVR_EagerDepthStats,[]{return preparation::RunActive();},[]{return preparation::Active();},
    &CyberpunkVR_PreparationPairState,&CyberpunkVR_PreparationRunFrames,CyberpunkVR_PreparationPairData,CyberpunkVR_PreparationRunDepth};
extern "C" void cvr_eager_depth_attach(const EagerDepthAttach*);
const bool eagerDepthAttached=(cvr_eager_depth_attach(&eagerDepthTable),true);
}
#endif
#else
namespace cvr::stereo::eager_depth {
namespace {
std::mutex mutex;Cache cache;
bool ReadMemory(uint64_t address,void* destination,size_t bytes){SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),destination,bytes,&copied) && copied==bytes;}
bool Compatible(){
    static const bool result=[]{
        struct Region {uint32_t address,size;uint64_t hash;};
        // Existing detour prologues are excluded; their native bodies remain intact.
        const Region regions[]{{0x229D400,0x4A,0xB0A35CF1F2670A13ull},{0x4EE6DC,0x9D,0xC31F8861D27704F0ull},
            {0x229D260,0xE0,0xD35284DC4BA124BEull},{0x426B04,0x3D5,0x09821EC69F3EAB56ull},{0x4270A4,0x91C,0x4F9E97361DB83C3Bull}};
        std::array<uint8_t,4096> bytes{};const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        for(const auto& region:regions){if(!ReadMemory(base+region.address,bytes.data(),region.size))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.size;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=region.hash)return false;}
        return true;
    }();
    CyberpunkVR_EagerDepthCompatible=result?1:2;return result;
}
}
void BeginRun(){
    std::lock_guard lock(mutex);cache.Reset();std::fill_n(CyberpunkVR_EagerDepthStats,CounterCount,0u);
    if(CyberpunkVR_EagerDepthMode.load()==1)CyberpunkVR_EagerDepthVerified=0;
}
void EndRun(){std::lock_guard lock(mutex);cache.Reset();}
bool Intercept(Prepare prepare,Gather gather,Filter filter,Finalize finalize,void* context,void* buckets,void* output,uint32_t flags0,uint32_t flags1){
    const auto mode=CyberpunkVR_EagerDepthMode.load(std::memory_order_relaxed);
    if(!mode || !preparation::RunActive() || preparation::Active() || CyberpunkVR_PreparationPairState.load()!=2 ||
        (mode==2 && !CyberpunkVR_EagerDepthVerified.load()) || !Compatible())return false;
    std::lock_guard lock(mutex);const auto frame=CyberpunkVR_PreparationRunFrames.load();
    const bool handled=cache.Intercept({ReadMemory,prepare,gather,filter,finalize},frame,CyberpunkVR_PreparationPairData[4],CyberpunkVR_PreparationPairData[3],
        CyberpunkVR_PreparationRunDepth[0].load()+1==frame,mode,context,buckets,output,flags0,flags1);
    std::copy(cache.counts.begin(),cache.counts.end(),CyberpunkVR_EagerDepthStats);
    if(mode==1){
        if(cache.counts[OutputDifferent] || cache.counts[Failure])CyberpunkVR_EagerDepthVerified=0;
        else if(cache.counts[OutputSame]>=30)CyberpunkVR_EagerDepthVerified=1;
    }
    return handled;
}
}
#endif
