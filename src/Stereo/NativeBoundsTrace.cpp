#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/NativeBoundsTrace.hpp"
#include "Render/NativeSceneUpdate.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include <windows.h>
#include <intrin.h>
#include <mutex>
#include <cstring>
#include <vector>
#include <algorithm>
#if CVR_RUST_VRIK
// The game-code contracts: rust/crates/cvr-render/src/stereo_code.rs (0 draw origin, 1 bounds, 2 particles, 3 scene
// update; through ReadProcessMemory).
#endif

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) uint32_t CyberpunkVR_BoundsTraceFormat=2;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_BoundsTraceCompatible{},CyberpunkVR_BoundsTraceCount{},CyberpunkVR_BoundsTraceLost{};
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/bounds_trace.rs
__declspec(dllexport) cvr::stereo::bounds_trace::Record CyberpunkVR_BoundsTraceRecords[cvr::stereo::bounds_trace::Capacity]{};
void* CvrBoundsOriginal{};
#endif
#if !CVR_RUST_VRIK
void CvrBoundsDetour();
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/bounds_trace.rs
void* CvrParticleOriginal{};
#endif
#if !CVR_RUST_VRIK
void CvrParticleDetour();
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ParticleTraceCompatible{},CyberpunkVR_ParticleTraceCount{},CyberpunkVR_ParticleTraceLost{};
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/bounds_trace.rs
__declspec(dllexport) cvr::stereo::bounds_trace::ParticleRecord CyberpunkVR_ParticleTraceRecords[cvr::stereo::bounds_trace::Capacity]{};
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
// The trace: rust/crates/cvr-render/src/bounds_trace.rs. The exports' arrays, the detours' registration over the thunks
// and the stereo state it reads stay here, lent through the runtime.
// The table is built in rust/crates/cvr-render/src/bounds_trace.rs.
// The two detours register from Rust (rust/crates/cvr-render/src/engine_detours.rs).
#else
namespace cvr::stereo::bounds_trace {
namespace {
std::mutex mutex;
std::atomic<uintptr_t> activePool{};
uint64_t nextSpan{};
bool closing{};
std::vector<uintptr_t> emitters;
bool Read(const void* p,void* out,size_t size){SIZE_T copied{};return p && ReadProcessMemory(GetCurrentProcess(),p,out,size,&copied) && copied==size;}
uint32_t Rva(uintptr_t p){const auto base=reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base);return p>=base && p-base<=UINT32_MAX?uint32_t(p-base):0;}
void Append(Record row) {
    const auto count=CyberpunkVR_BoundsTraceCount.load();
    if(count==Capacity){++CyberpunkVR_BoundsTraceLost;return;}
    LARGE_INTEGER now{};QueryPerformanceCounter(&now);row.sequence=count+1;row.qpc=now.QuadPart;row.thread=GetCurrentThreadId();
    CyberpunkVR_BoundsTraceRecords[count]=row;CyberpunkVR_BoundsTraceCount.store(count+1,std::memory_order_release);
}
bool Compatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_BOUNDS_TRACE",nullptr,0))return false;
#if CVR_RUST_VRIK
    const bool valid=cvr_stereo_code(1,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    CyberpunkVR_BoundsTraceCompatible=valid?1:2;return valid;
#else
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,0x1A9> code{};
    if(!Read(reinterpret_cast<void*>(base+0x29ABD8),code.data(),code.size())){CyberpunkVR_BoundsTraceCompatible=2;return false;}
    uint64_t hash=14695981039346656037ull;for(auto b:code)hash=(hash^b)*1099511628211ull;
    const bool valid=hash==0x92FA499B9D21DB2Cull;CyberpunkVR_BoundsTraceCompatible=valid?1:2;return valid;
#endif
}
bool ParticleCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_BOUNDS_TRACE",nullptr,0))return false;
#if CVR_RUST_VRIK
    const bool valid=cvr_stereo_code(2,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    CyberpunkVR_ParticleTraceCompatible=valid?1:2;return valid;
#else
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,2048> code{};
    struct Region{uint32_t rva,bytes;uint64_t hash;};
    for(const auto& row:std::array<Region,2>{{{0x29173C,0xD7,0x2D8AD96221D4BDBCull},{0x291814,0x79F,0x9700D29BBBBDD215ull}}}){
        if(!Read(reinterpret_cast<void*>(base+row.rva),code.data(),row.bytes)){CyberpunkVR_ParticleTraceCompatible=2;return false;}
        uint64_t hash=14695981039346656037ull;for(uint32_t i=0;i<row.bytes;++i)hash=(hash^code[i])*1099511628211ull;
        if(hash!=row.hash){CyberpunkVR_ParticleTraceCompatible=2;return false;}
    }
    CyberpunkVR_ParticleTraceCompatible=1;return true;
#endif
}
void ParticleSnapshot(uintptr_t emitter,void* context,uint32_t onscreen,uint32_t kind,uintptr_t caller,void* system=nullptr){
    if(!std::binary_search(emitters.begin(),emitters.end(),emitter))return;
    uintptr_t instances{},scene{},view{};uint32_t count{},sceneCounter{},particleEpoch{};
    const auto base=reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base);
    if(!Read(reinterpret_cast<void*>(emitter+0xB8),&instances,8) || !Read(reinterpret_cast<void*>(emitter+0xC4),&count,4) || count>128 || (count && !instances) ||
        !Read(reinterpret_cast<void*>(emitter+0x68),&scene,8) || !Read(reinterpret_cast<void*>(scene+0x28),&sceneCounter,4) ||
        !Read(reinterpret_cast<void*>(base+0x3BCD478),&particleEpoch,4)){++CyberpunkVR_ParticleTraceLost;return;}
    if(context)Read(static_cast<uint8_t*>(context)+0x18,&view,8);
    for(uint32_t i=0;i<count;++i){
        const auto at=CyberpunkVR_ParticleTraceCount.load();if(at==Capacity){++CyberpunkVR_ParticleTraceLost;return;}
        ParticleRecord row;row.id=emitter+16;row.view=view;row.kind=kind;row.thread=GetCurrentThreadId();row.caller=Rva(caller);
        row.node=Rva(cvr::detail::t_current_node_work);row.side=cvr::detail::t_view_side;row.epoch=particleEpoch;row.sceneCounter=sceneCounter;
        row.instanceIndex=i;row.totalInstances=count;row.onscreen=onscreen;row.anchor=CyberpunkVR_BoundsTraceCount.load();
        if(view){uint64_t name{};if(Read(reinterpret_cast<void*>(view+0x28),&name,8)){
            if(name==cvr::detail::g_vrcam_ctx_key.load())row.side=1;
            else if(cvr::detail::is_main_view(reinterpret_cast<void*>(view)))row.side=0;
        }}
        bool valid=Read(reinterpret_cast<void*>(instances+i*8),&row.instance,8) && row.instance &&
            Read(reinterpret_cast<void*>(row.instance+0x18),&row.state,8) && Read(reinterpret_cast<void*>(row.instance+0x24),&row.accumulated,4);
        if(valid && row.state)valid=Read(reinterpret_cast<void*>(row.state+0x7C),&row.stamp,4);
        if(system)valid=valid && Read(static_cast<uint8_t*>(system)+0x50,&row.systemDelta,4);
        valid=valid && Read(reinterpret_cast<void*>(emitter+0x1C0),&row.parameterDelta,4);
        row.valid=valid;row.sequence=at+1;LARGE_INTEGER now{};QueryPerformanceCounter(&now);row.qpc=now.QuadPart;
        CyberpunkVR_ParticleTraceRecords[at]=row;CyberpunkVR_ParticleTraceCount.store(at+1,std::memory_order_release);
    }
}
void Observe(void* pool,void* indices,const void* bounds,uintptr_t caller) {
    const auto poolAddress=reinterpret_cast<uintptr_t>(pool);
    scene_update::ObserveBoundsWrite(poolAddress,reinterpret_cast<uintptr_t>(indices),caller);
    if(!poolAddress || activePool.load(std::memory_order_relaxed)!=poolAddress)return;
    Record row;
    {
        std::lock_guard lock(mutex);
        if(activePool.load()!=poolAddress)return;
        row.span=++nextSpan;row.pool=poolAddress;row.kind=uint32_t(Kind::WriteBegin);row.result=UINT32_MAX;
        row.caller=Rva(caller);row.node=Rva(cvr::detail::t_current_node_work);row.side=cvr::detail::t_view_side;
        std::array<uint32_t,2> key{};std::array<float,8> box{};uintptr_t scene{};
        const auto indexAddress=reinterpret_cast<uintptr_t>(indices);
        if((row.caller==0x32C04B || row.caller==0x29ADE0 || row.caller==0x2EA47CD) && indexAddress>=0x74 &&
            Read(reinterpret_cast<void*>(indexAddress-12),&scene,8) && scene+0x7E0F0==poolAddress &&
            Read(indices,key.data(),8) && Read(bounds,box.data(),32)){
            row.id=indexAddress-0x64;row.key=key[0];row.slot=key[1];row.flags=1;
            for(size_t i=0;i<3;++i){row.bounds[i]=box[i];row.bounds[i+3]=box[i+4];}
        }
        void* frames[12]{};row.stackCount=CaptureStackBackTrace(1,12,frames,nullptr);
        for(uint32_t i=0;i<row.stackCount;++i)row.stack[i]=Rva(reinterpret_cast<uintptr_t>(frames[i]));
        Append(row);
        if(row.flags && row.id>=16)ParticleSnapshot(row.id-16,nullptr,UINT32_MAX,1,caller);
    }
}
CVR_DETOUR_IF("[single-pass] scene candidate bounds writes",0x29ABD8,CvrBoundsDetour,CvrBoundsOriginal,Compatible)
CVR_DETOUR_IF("[single-pass] particle instance preparation",0x29173C,CvrParticleDetour,CvrParticleOriginal,ParticleCompatible)
}
bool Begin(uintptr_t pool) {
    std::lock_guard lock(mutex);
    if(!pool || activePool.load() || CyberpunkVR_BoundsTraceCompatible!=1)return false;
    uintptr_t array{};uint32_t count{};
    if(pool<0x7E0F0 || !Read(reinterpret_cast<void*>(pool-0x7E0F0+0x7E190),&array,8) ||
        !Read(reinterpret_cast<void*>(pool-0x7E0F0+0x7E19C),&count,4) || count>65536 || (count && !array))return false;
    try {emitters.resize(count);}catch(...){return false;}
    if(count && !Read(reinterpret_cast<void*>(array),emitters.data(),count*8ull))return false;
    std::sort(emitters.begin(),emitters.end());
    nextSpan=0;closing=false;CyberpunkVR_BoundsTraceCount=0;CyberpunkVR_BoundsTraceLost=0;
    CyberpunkVR_ParticleTraceCount=0;CyberpunkVR_ParticleTraceLost=0;activePool=pool;
    Record row;row.kind=uint32_t(Kind::Begin);row.pool=pool;Append(row);return true;
}
void Mark(Kind kind,int side,uint32_t nativeCounter) {
    std::lock_guard lock(mutex);const auto pool=activePool.load();if(!pool)return;
    Record row;row.kind=uint32_t(kind);row.side=side;row.key=nativeCounter;row.pool=pool;
    row.node=Rva(cvr::detail::t_current_node_work);Append(row);
}
bool End() {
    std::lock_guard lock(mutex);
    if(!closing){const auto pool=activePool.exchange(0);Record row;row.kind=uint32_t(Kind::End);row.pool=pool;Append(row);closing=true;}
    return true;
}
void Before(void* pool,void* indices,const void* bounds,uintptr_t caller){Observe(pool,indices,bounds,caller);}
void BeforeParticle(void* system,void* context,void* emitter,const void* stack){
    if(!activePool.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);if(!activePool.load())return;
    uintptr_t caller{};uint8_t onscreen{};
    if(!Read(stack,&caller,8) || !Read(static_cast<const uint8_t*>(stack)+0x28,&onscreen,1)){++CyberpunkVR_ParticleTraceLost;return;}
    ParticleSnapshot(reinterpret_cast<uintptr_t>(emitter),context,onscreen,0,caller,system);
}
}
extern "C" void CvrBoundsBefore(void* pool,void* indices,const void* bounds,uintptr_t caller){cvr::stereo::bounds_trace::Before(pool,indices,bounds,caller);}
extern "C" void CvrParticleBefore(void* system,void* context,void* emitter,const void* stack){cvr::stereo::bounds_trace::BeforeParticle(system,context,emitter,stack);}
#endif
#endif
