#include "Render/NativeCpuCollector.hpp"
#include "Render/NativeJobLease.hpp"
#include "Render/NativeFineVisibility.hpp"
#include "Render/NativeSceneVisibility.hpp"
#include "Render/NativeBoundsTrace.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Core/VrCoreShared.hpp"
#include <windows.h>
#include <intrin.h>
#include <atomic>
#include <cstring>
#include <algorithm>
#include <iterator>
#include <mutex>
#include <memory>

#if !CVR_RUST_VRIK   // unused by the Rust build
struct ChangedBounds {uint64_t id{};uint32_t type{},prepare{};std::array<float,6> before{},after{};};
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
static_assert(sizeof(ChangedBounds)==64);
#endif
#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// The exported state is Rust's (rust/crates/cvr-render/src/early_depth_exports.rs, collector_probe.rs).
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_CpuCollectorRequest{},CyberpunkVR_CpuCollectorState{},CyberpunkVR_CpuCollectorCompatible{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorResult[8]{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorQueryResult[8]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_CpuCollectorQueryCompatible{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_CpuCollectorFineCompatible{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorFineResult[12]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_CpuCollectorSceneCompatible{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorSceneResult[12]{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorSceneControl[8]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_CpuCollectorPeerCompatible{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorPeerResult[16]{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorPeerQuery[8]{},CyberpunkVR_CpuCollectorPeerFine[12]{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorPeerScene[12]{},CyberpunkVR_CpuCollectorPeerControl[8]{};
__declspec(dllexport) float CyberpunkVR_CpuCollectorPeerPlanes[64]{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorPeerInputDiff[10]{};
__declspec(dllexport) uint32_t CyberpunkVR_CpuCollectorPeerChangesCount{};
__declspec(dllexport) ChangedBounds CyberpunkVR_CpuCollectorPeerChanges[64]{};
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
// The probe: rust/crates/cvr-render/src/collector_probe.rs (the scene pool's capture in scene_visibility.rs). The two
// detours' frames (their return addresses), the stereo state, the culling hooks' originals and the changed bounds
// stay here.
// The table is built in rust/crates/cvr-render/src/collector_probe.rs.
#else
namespace cvr::detail {
namespace {
cvr::stereo::CollectorInitialize original{};
using QueryInitialize=uintptr_t(*)(void*,void*,void*);
QueryInitialize originalQuery{};
std::mutex pendingMutex;
void RetainJob(void* group){InterlockedIncrement(reinterpret_cast<volatile LONG*>(static_cast<uint8_t*>(group)+0x1C));}
void ReleaseJob(void** group){reinterpret_cast<void(*)(void**)>(g_exe_base+0x2744CC)(group);}
bool JobComplete(void* group){return InterlockedCompareExchange(reinterpret_cast<volatile LONG*>(static_cast<uint8_t*>(group)+0x18),0,0)==0;}
struct FineRange {
    std::vector<cvr::stereo::NativeCullCandidate> candidates;
    std::vector<uint64_t> reference;
    char partial{};
    uintptr_t sourceBegin{};
};
struct QueryWork {
    cvr::stereo::NativeCpuCollector collector;
    cvr::stereo::NativeJobLease jobs{{RetainJob,ReleaseJob,JobComplete}};
    cvr::stereo::NativeJobLease sourceJobs{{RetainJob,ReleaseJob,JobComplete}};
    cvr::stereo::NativeJobLease cullJobs{{RetainJob,ReleaseJob,JobComplete}};
    alignas(16) std::array<uint8_t,0x50> context{};
    std::array<uintptr_t,2> sourceQuery{};
    bool captureFine{};
    uint32_t fineFailures{},fineBytes{};
    std::vector<FineRange> fineRanges;
    bool captureScene{},sceneStarted{},sceneProcessing{},sceneValid{},cullJobsRetained{};
    cvr::stereo::NativeSceneVisibility scene;
    std::unique_ptr<QueryWork> peer;
    // The peer's stand-in view. 2.5's query initializer also tests feature bit 98 (stereo) through
    // context+0x18, i.e. view+0x25B0+8 (sub_14033E358): the zeroed block reaches past it, so that test reads 0.
    alignas(16) std::array<uint8_t,0x25C0> queryView{};
    uint64_t epoch{};uintptr_t mainView{};
    bool boundsTrace{};
    std::array<std::vector<uint64_t>,2> earlyIds;
    explicit QueryWork(cvr::stereo::CollectorApi api):collector(api){}
    ~QueryWork() {
        collector.Reset();
        if(sourceQuery[0])reinterpret_cast<void(*)(void*)>(g_exe_base+0x4ED568)(sourceQuery.data());
        uintptr_t owner{};std::memcpy(&owner,context.data(),8);
        if(owner)reinterpret_cast<void(*)(void*)>(g_exe_base+0x32DE14)(reinterpret_cast<void*>(owner));
    }
};
// If a native completion cannot be proven, keep this bounded diagnostic owner
// alive through process exit rather than destroying it under pending tasks.
QueryWork* pending{};
std::atomic<bool> queryPending{};
std::atomic<bool> finePending{};
std::atomic<bool> scenePending{};
std::atomic<bool> peerPending{};
std::atomic<uint64_t> presentEpoch{};
bool Read(const void* address,void* out,size_t size){SIZE_T copied{};return address && ReadProcessMemory(GetCurrentProcess(),address,out,size,&copied) && copied==size;}
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The game-code contracts: rust/crates/cvr-render/src/collector_code.rs (0 collector, 1 query, 2 fine, 3 scene,
// 4 camera; the exports and their chain stay here).
extern "C" bool cvr_ccp_code(uint32_t,uint64_t);
// The comparisons: rust/crates/cvr-render/src/collector_compare.rs (through ReadProcessMemory).
struct RustQueryComparison {uint64_t cloneQuery,sourceType,cloneType;uint32_t differences;bool valid;uint8_t planes[2][128];};
static_assert(sizeof(RustQueryComparison)==288,"mirrored by collector_compare.rs");
extern "C" void cvr_ccp_compare_query(uint64_t,uint64_t,uint64_t,RustQueryComparison*);
extern "C" uint32_t cvr_ccp_different_ids(const uint64_t*,size_t,const uint64_t*,size_t);
// The query inputs: rust/crates/cvr-render/src/collector_input.rs (through ReadProcessMemory).
extern "C" bool cvr_ccp_vrcam_view(uint64_t,uint64_t);
extern "C" bool cvr_ccp_collector_input(uint64_t,uint64_t*,uint8_t*,uint8_t*,uint8_t*);
extern "C" bool cvr_ccp_inputs_unchanged(uint64_t,uint64_t,const uint8_t*,const uint8_t*);
// The peer view: rust/crates/cvr-render/src/collector_peer.rs (through ReadProcessMemory).
extern "C" bool cvr_ccp_peer_view(uint64_t,uint64_t,uint32_t*,uint8_t*);
extern "C" bool cvr_ccp_main_projection(uint64_t,float*);
extern "C" bool cvr_ccp_candidate_type(uint64_t,uint64_t,uint32_t*);
extern "C" void cvr_ccp_id_overlap(const uint64_t*,size_t,const uint64_t*,size_t,uint32_t*);
#endif
#endif
bool Compatible() {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
#if CVR_RUST_VRIK
    if(!cvr_ccp_code(0,base)){CyberpunkVR_CpuCollectorCompatible=2;return false;}
#else
    struct Region {uint32_t rva,bytes;uint64_t hash;};
    // 2.5: the collector block grew (0xC7FC0 -> 0xE2880 bytes, metadata +0xC5D00 -> +0xE05C0) and the initializer
    // gained an extra-projection pass (camera +0x399 bit 0, zero in a port copy); storage +0..+0x28 unchanged.
    // Whole functions; the destroyer's member teardown moved into sub_1411F3584.
    const Region regions[]{{0x928D28,0xB5,0xDA81906969062833ull},{0x11F3160,0x2B,0xBD3CE905B2F2F939ull},
        {0x11F318C,0x79,0x03BC714D958D6A3Eull},{0xC0F46C,0x160,0x08519173B2F5B1A8ull},
        {0x11F3520,0x64,0x203E9A724B4FDE8Bull},{0x11F3584,0x2E,0x584580F4CE6EA6C8ull},{0x25B47B4,0x24,0x333311B0E83E07CCull}};
    std::array<uint8_t,512> bytes{};
    for(const auto& region:regions){if(!Read(reinterpret_cast<void*>(base+region.rva),bytes.data(),region.bytes)){CyberpunkVR_CpuCollectorCompatible=2;return false;}
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        if(hash!=region.hash){CyberpunkVR_CpuCollectorCompatible=2;return false;}}
#endif
    CyberpunkVR_CpuCollectorCompatible=1;return true;
}
bool CheckClone(void* source,const cvr::stereo::NativeCpuCollector& clone) {
    std::array<uint8_t,0x30> a{},b{};const auto* input=clone.Inputs();
    if(!input || !Read(source,a.data(),a.size()) || !Read(clone.Storage(),b.data(),b.size()))return false;
    auto qword=[](const auto& bytes,size_t at){uintptr_t value{};std::memcpy(&value,bytes.data()+at,8);return value;};
    uint32_t sourceCount{},cloneCount{};std::memcpy(&sourceCount,a.data()+0x20,4);std::memcpy(&cloneCount,b.data()+0x20,4);
    uint32_t sourceBits{},cloneBits{};
    if(!Read(reinterpret_cast<const void*>(qword(a,0x28)+0x10),&sourceBits,4) ||
        !Read(static_cast<const uint8_t*>(clone.Metadata())+0x10,&cloneBits,4))return false;
    CyberpunkVR_CpuCollectorResult[0]=sourceCount;CyberpunkVR_CpuCollectorResult[1]=cloneCount;
    CyberpunkVR_CpuCollectorResult[2]=sourceBits;CyberpunkVR_CpuCollectorResult[3]=cloneBits;
    return qword(b,0)==reinterpret_cast<uintptr_t>(input->scene) && qword(b,8)==reinterpret_cast<uintptr_t>(input->viewData) &&
        qword(b,0x10)==reinterpret_cast<uintptr_t>(input->viewFlags.data()) && qword(b,0x18)==reinterpret_cast<uintptr_t>(input->camera.bytes.data()) &&
        qword(b,0x28)==reinterpret_cast<uintptr_t>(clone.Metadata()) && clone.Storage()!=source && qword(a,0x28)!=qword(b,0x28) &&
        sourceCount==cloneCount && sourceBits==cloneBits && sourceBits>0;
}
uintptr_t Hook(void* storage,void* data,uint8_t mode,const void* camera,void* scene,void* metadata,const void* flags) {
    const auto result=original(storage,data,mode,camera,scene,metadata,flags);
    if(CyberpunkVR_CpuCollectorRequest.load(std::memory_order_relaxed)!=1 || g_menuModeValue || t_view_side!=1 ||
        reinterpret_cast<uintptr_t>(_ReturnAddress())!=reinterpret_cast<uintptr_t>(g_exe_base)+0xC0F291)return result;
    if(!CyberpunkVR_CpuCollectorRequest.exchange(0))return result;
    CyberpunkVR_CpuCollectorState.store(1,std::memory_order_release);std::fill_n(CyberpunkVR_CpuCollectorResult,8,0u);
    try {
        cvr::stereo::CollectorInput input;input.viewData=data;input.scene=scene;input.mode=mode;
        if(!Read(camera,input.camera.bytes.data(),input.camera.bytes.size()) || !Read(flags,input.viewFlags.data(),input.viewFlags.size())){
            CyberpunkVR_CpuCollectorState=3;return result;}
        const auto base=reinterpret_cast<uintptr_t>(g_exe_base);const cvr::stereo::CollectorApi api{
            reinterpret_cast<void*(*)(void**)>(base+0x928D28),reinterpret_cast<void*(*)(void*,uint32_t)>(base+0x11F3160),original};
        bool valid{};
        {cvr::stereo::NativeCpuCollector owned(api);valid=owned.Create(input) && CheckClone(storage,owned);CyberpunkVR_CpuCollectorResult[4]=valid?1:0;}
        CyberpunkVR_CpuCollectorResult[5]=1;
        std::array<uint8_t,904> liveCamera{};std::array<uint8_t,0x330> liveFlags{};
        const bool unchanged=Read(camera,liveCamera.data(),liveCamera.size()) && Read(flags,liveFlags.data(),liveFlags.size()) &&
            liveCamera==input.camera.bytes && liveFlags==input.viewFlags;
        CyberpunkVR_CpuCollectorResult[6]=unchanged?1:0;
        CyberpunkVR_CpuCollectorState.store(valid && unchanged?2:3,std::memory_order_release);
    }catch(...){CyberpunkVR_CpuCollectorState=3;}
    return result;
}
// The collector answers a request for the second view (t_view_side 1); there is none in engine stereo.
bool Wanted(){return cvr::engine_stereo::SecondViewWanted() && Compatible();}
CVR_DETOUR_IF("[single-pass] owned CPU collector lifecycle",0xC0F46C,Hook,original,Wanted)
bool QueryCompatible() {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
    if(!cvr_ccp_code(1,base)){CyberpunkVR_CpuCollectorQueryCompatible=2;return false;}
    // The fine culling bodies beyond their detours' prologues, then the scene's, then the peer camera rebuild.
    const bool fineCompatible=cvr_ccp_code(2,base);
    CyberpunkVR_CpuCollectorFineCompatible=fineCompatible?1:2;
    const bool sceneCompatible=cvr_ccp_code(3,base) && fineCompatible;
    CyberpunkVR_CpuCollectorSceneCompatible=sceneCompatible?1:2;
    CyberpunkVR_CpuCollectorPeerCompatible=sceneCompatible && cvr_ccp_code(4,base)?1:2;
#endif
#else
    struct Region {uint32_t rva,bytes;uint64_t hash;};
    // 2.5: the query initializer reads its view-flag block 8/16 bytes lower and adds the stereo frustum
    // (feature 98, see queryView); the job builder/destructor keep the job layout (+0x10/+0x18 groups), the
    // destructor's pending tail moved into sub_140272F30. Whole functions.
    const Region regions[]{{0x5ABBD0,0x4E7,0xD837640E2319072Cull},{0x2746D8,0x12E,0x852D109C81F4C709ull},
        {0x272DB8,0x62,0x13B6B2B563D6DEF8ull},{0x272F30,0xD8,0x2DC24D5A2D13D9E6ull},{0x2744CC,0x4A,0xBF52C1744ED0577Cull},
        {0x333374,0x98,0x6B7C1293E9814833ull},{0x32DE14,0x15,0xE9AA994824C7FD8Bull},{0x4ED568,0x41,0xCFCFA044ECC25870ull}};
    std::array<uint8_t,2048> bytes{};
    for(const auto& region:regions){if(!Read(reinterpret_cast<void*>(base+region.rva),bytes.data(),region.bytes)){CyberpunkVR_CpuCollectorQueryCompatible=2;return false;}
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        if(hash!=region.hash){CyberpunkVR_CpuCollectorQueryCompatible=2;return false;}}
    // These entry points already have registered culling detours. Verify the
    // unchanged function bodies beyond their trampoline-covered prologues.
    bool fineCompatible=true;
    for(const auto& region:std::array<Region,2>{{{0x29D140,0x338,0x87E87660EA59BE32ull},{0x19B78C,0x67,0xAE9FD73DCB825F0Dull}}}){
        if(!Read(reinterpret_cast<void*>(base+region.rva),bytes.data(),region.bytes)){fineCompatible=false;break;}
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        if(hash!=region.hash){fineCompatible=false;break;}
    }
    CyberpunkVR_CpuCollectorFineCompatible=fineCompatible?1:2;
    bool sceneCompatible=fineCompatible;
    for(const auto& region:std::array<Region,3>{{{0x29CEE4,0x22C,0x37E53D58ACD7E571ull},
        {0x29CCEC,0x1F7,0x6EBE388EB49A585Full},{0xC651E4,0x31,0xF1919D5E26ED5C43ull}}}){
        if(!Read(reinterpret_cast<void*>(base+region.rva),bytes.data(),region.bytes)){sceneCompatible=false;break;}
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<region.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        if(hash!=region.hash){sceneCompatible=false;break;}
    }
    CyberpunkVR_CpuCollectorSceneCompatible=sceneCompatible?1:2;
    uint64_t cameraHash=14695981039346656037ull;
    // 2.5's camera rebuild adds the off-axis branch for camera +0x399 bit 1 (zero in a port copy). Whole function.
    bool cameraCompatible=Read(reinterpret_cast<void*>(base+0x296EA0),bytes.data(),0x62D);
    if(cameraCompatible)for(UINT i=0;i<0x62D;++i)cameraHash=(cameraHash^bytes[i])*1099511628211ull;
    CyberpunkVR_CpuCollectorPeerCompatible=sceneCompatible && cameraCompatible && cameraHash==0xF11C4A2E7D6CBFEDull?1:2;
#endif
    CyberpunkVR_CpuCollectorQueryCompatible=1;return true;
}
bool RetainSourceQuery(QueryWork& work,void* storage,void* sourceJob) {
    std::array<void*,2> groups{};
    if(!Read(static_cast<uint8_t*>(sourceJob)+0x10,groups.data(),sizeof(groups)) || !work.sourceJobs.Capture(groups[0],groups[1]))return false;
    if(!Read(static_cast<uint8_t*>(storage)+0xDF2F0,work.sourceQuery.data(),16) || !work.sourceQuery[0] || !work.sourceQuery[1]){
        work.sourceQuery={};return false;}
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(work.sourceQuery[1]));return true;
}
bool StartQuery(QueryWork& work,void* sourceJob) {
    alignas(16) std::array<uint8_t,0x40> job{};
    reinterpret_cast<void*(*)(void*,void*)>(g_exe_base+0x2746D8)(job.data(),sourceJob);
    originalQuery(work.collector.Storage(),work.context.data(),job.data());
    void* begin{},*end{};std::memcpy(&begin,job.data()+0x10,8);std::memcpy(&end,job.data()+0x18,8);
    const bool retained=work.jobs.Capture(begin,end);
    reinterpret_cast<void(*)(void*)>(g_exe_base+0x272DB8)(job.data());return retained;
}
bool CreatePeer(QueryWork& work,cvr::stereo::CollectorApi api,const cvr::stereo::CollectorInput& input,void* context) {
#if CVR_RUST_VRIK
    const auto main=g_main_view_ctx.load(std::memory_order_acquire);uint32_t mainSize[2]{};std::array<uint8_t,32> pose{};
    if(!cvr_ccp_peer_view(main,reinterpret_cast<uintptr_t>(context),mainSize,pose.data()))return false;
#else
    const auto main=g_main_view_ctx.load(std::memory_order_acquire);uintptr_t sourceView{};uint32_t mainSize[2]{},sourceSize[2]{};
    std::array<uint8_t,32> pose{};uint64_t name{};
    if(!main || !Read(static_cast<uint8_t*>(context)+0x18,&sourceView,8) || sourceView==main ||
        !Read(reinterpret_cast<void*>(main+0x28),&name,8) || name ||
        !Read(reinterpret_cast<void*>(main+0x34),mainSize,8) || !Read(reinterpret_cast<void*>(sourceView+0x34),sourceSize,8) ||
        std::memcmp(mainSize,sourceSize,8) || !Read(reinterpret_cast<void*>(main+0x2C50),pose.data(),pose.size()))return false;
#endif
    auto peer=std::make_unique<QueryWork>(api);auto predicted=input;
    // MAIN's own window under the asymmetric projection (its input block, as the view producer left it).
    cvr::stereo::PeerProjection mainProjection{};
#if CVR_RUST_VRIK
    float projection[3]{};const bool ownProjection=cvr::projection::Active() && cvr_ccp_main_projection(main,projection);
    if(ownProjection){mainProjection.aspect=projection[0];mainProjection.shiftX=projection[1];mainProjection.shiftY=projection[2];}
#else
    const bool ownProjection=cvr::projection::Active() &&
        Read(reinterpret_cast<void*>(main+0x2C78),&mainProjection.aspect,4) && Read(reinterpret_cast<void*>(main+0x2C80),&mainProjection.shiftX,8);
#endif
    if(!cvr::stereo::PrepareCameraAtPose(input.camera.bytes.data(),pose.data(),{},mainSize[0],mainSize[1],
        reinterpret_cast<cvr::stereo::RebuildCameraInput>(g_exe_base+0x296EA0),predicted.camera,
        ownProjection?&mainProjection:nullptr) || !peer->collector.Create(predicted))return false;
    reinterpret_cast<void*(*)(void*,const void*)>(g_exe_base+0x333374)(peer->context.data(),context);
    std::memcpy(peer->queryView.data()+0x34,mainSize,8);
    const auto view=reinterpret_cast<uintptr_t>(peer->queryView.data()),storage=reinterpret_cast<uintptr_t>(peer->collector.Storage());
    // The verified query initializer reads only width/height through context+18.
    // It independently removes jitter before constructing its culling frustum.
    std::memcpy(peer->context.data()+0x18,&view,8);std::memcpy(peer->context.data()+0x48,&storage,8);
    peer->captureFine=peer->captureScene=true;work.mainView=main;work.peer=std::move(peer);return true;
}
uintptr_t QueryHook(void* storage,void* context,void* sourceJob) {
    const bool expectedCaller=reinterpret_cast<uintptr_t>(_ReturnAddress())==reinterpret_cast<uintptr_t>(g_exe_base)+0x5ABB85;
    bool captureMain=false;
    if(expectedCaller && peerPending.load(std::memory_order_acquire)){
        std::lock_guard lock(pendingMutex);uintptr_t view{};
        if(pending && pending->peer && !pending->peer->sourceQuery[0] && Read(static_cast<uint8_t*>(context)+0x18,&view,8) && view==pending->mainView){
            captureMain=true;CyberpunkVR_CpuCollectorPeerResult[5]=pending->peer->jobs.Complete();}
    }
    const auto result=originalQuery(storage,context,sourceJob);
    if(captureMain){
        std::lock_guard lock(pendingMutex);
        if(pending && pending->peer && pending->epoch==presentEpoch.load() && !pending->peer->sourceQuery[0]){
            CyberpunkVR_CpuCollectorPeerResult[4]=RetainSourceQuery(*pending->peer,storage,sourceJob);
            if(!CyberpunkVR_CpuCollectorPeerResult[4])CyberpunkVR_CpuCollectorPeerResult[13]=2;
        }
        return result;
    }
    const auto request=CyberpunkVR_CpuCollectorRequest.load(std::memory_order_relaxed);
    if((request<2 || request>5) || g_menuModeValue || !expectedCaller)return result;
#if CVR_RUST_VRIK
    if(!cvr_ccp_vrcam_view(reinterpret_cast<uintptr_t>(context),g_vrcam_ctx_key.load()))return result;
#else
    uintptr_t view{};uint64_t name{};
    if(!Read(static_cast<uint8_t*>(context)+0x18,&view,8) || !view || !Read(reinterpret_cast<void*>(view+0x28),&name,8) || name!=g_vrcam_ctx_key.load())return result;
#endif
    std::lock_guard lock(pendingMutex);if(pending || !CyberpunkVR_CpuCollectorRequest.exchange(0))return result;
    CyberpunkVR_CpuCollectorState=1;std::fill_n(CyberpunkVR_CpuCollectorResult,8,0u);std::fill_n(CyberpunkVR_CpuCollectorQueryResult,8,0u);
    std::fill_n(CyberpunkVR_CpuCollectorFineResult,12,0u);
    std::fill_n(CyberpunkVR_CpuCollectorSceneResult,12,0u);
    std::fill_n(CyberpunkVR_CpuCollectorSceneControl,8,0u);
    std::fill_n(CyberpunkVR_CpuCollectorPeerResult,16,0u);std::fill_n(CyberpunkVR_CpuCollectorPeerQuery,8,0u);
    std::fill_n(CyberpunkVR_CpuCollectorPeerFine,12,0u);std::fill_n(CyberpunkVR_CpuCollectorPeerScene,12,0u);
    std::fill_n(CyberpunkVR_CpuCollectorPeerControl,8,0u);std::fill_n(CyberpunkVR_CpuCollectorPeerPlanes,64,0.0f);
    std::fill_n(CyberpunkVR_CpuCollectorPeerInputDiff,10,0u);
    CyberpunkVR_CpuCollectorPeerChangesCount=0;std::fill_n(CyberpunkVR_CpuCollectorPeerChanges,64,ChangedBounds{});
    if(request>=3 && (CyberpunkVR_CpuCollectorFineCompatible!=1 || CyberpunkVR_CullReuseMode || !g_orig_fine_materialize)){
        CyberpunkVR_CpuCollectorState=3;return result;}
    if(request>=4 && (CyberpunkVR_CpuCollectorSceneCompatible!=1 || !g_orig_main_cull_test)){
        CyberpunkVR_CpuCollectorState=3;return result;}
    if(request==5 && CyberpunkVR_CpuCollectorPeerCompatible!=1){CyberpunkVR_CpuCollectorState=3;return result;}
    try {
        const auto base=reinterpret_cast<uintptr_t>(g_exe_base);const cvr::stereo::CollectorApi api{
            reinterpret_cast<void*(*)(void**)>(base+0x928D28),reinterpret_cast<void*(*)(void*,uint32_t)>(base+0x11F3160),original};
        auto work=std::make_unique<QueryWork>(api);work->captureFine=request>=3;work->captureScene=request>=4;work->epoch=presentEpoch.load();
        cvr::stereo::CollectorInput input;std::array<uintptr_t,4> source{};
        if(!RetainSourceQuery(*work,storage,sourceJob)){CyberpunkVR_CpuCollectorState=3;return result;}
#if CVR_RUST_VRIK
        static_assert(sizeof(input.camera.bytes)==904 && sizeof(input.viewFlags)==0x330,"mirrored by collector_input.rs");
        if(!cvr_ccp_collector_input(reinterpret_cast<uintptr_t>(storage),source.data(),&input.mode,input.camera.bytes.data(),
            input.viewFlags.data())){CyberpunkVR_CpuCollectorState=3;return result;}
#else
        if(!Read(storage,source.data(),sizeof(source)) || !Read(static_cast<uint8_t*>(storage)+0xDF2C0,&input.mode,1) ||
           !Read(reinterpret_cast<void*>(source[3]),input.camera.bytes.data(),input.camera.bytes.size()) ||
           !Read(reinterpret_cast<void*>(source[2]),input.viewFlags.data(),input.viewFlags.size())){CyberpunkVR_CpuCollectorState=3;return result;}
#endif
        input.scene=reinterpret_cast<void*>(source[0]);input.viewData=reinterpret_cast<void*>(source[1]);
        if(!work->collector.Create(input) || !CheckClone(storage,work->collector)){CyberpunkVR_CpuCollectorState=3;return result;}
        CyberpunkVR_CpuCollectorResult[4]=1;
        reinterpret_cast<void*(*)(void*,const void*)>(base+0x333374)(work->context.data(),context);
        const auto target=reinterpret_cast<uintptr_t>(work->collector.Storage());std::memcpy(work->context.data()+0x48,&target,8);
        if(request==5){
            if(!CreatePeer(*work,api,input,context)){CyberpunkVR_CpuCollectorPeerResult[13]=1;CyberpunkVR_CpuCollectorState=3;return result;}
            CyberpunkVR_CpuCollectorPeerResult[0]=1;
        }
        // Once jobs can refer to the collector, even an allocation/validation
        // failure must not destroy it until both native completion groups finish.
        pending=work.release();queryPending=true;
        if(pending->peer)pending->boundsTrace=cvr::stereo::bounds_trace::Begin(reinterpret_cast<uintptr_t>(input.scene)+0x7E0F0);
        const bool retained=StartQuery(*pending,sourceJob);
        if(pending->peer){CyberpunkVR_CpuCollectorPeerResult[1]=StartQuery(*pending->peer,sourceJob);peerPending=true;}
#if CVR_RUST_VRIK
        CyberpunkVR_CpuCollectorResult[6]=cvr_ccp_inputs_unchanged(source[3],source[2],input.camera.bytes.data(),input.viewFlags.data());
#else
        std::array<uint8_t,904> cameraAfter{};std::array<uint8_t,0x330> flagsAfter{};
        CyberpunkVR_CpuCollectorResult[6]=Read(reinterpret_cast<void*>(source[3]),cameraAfter.data(),cameraAfter.size()) &&
            Read(reinterpret_cast<void*>(source[2]),flagsAfter.data(),flagsAfter.size()) && cameraAfter==input.camera.bytes && flagsAfter==input.viewFlags;
#endif
        CyberpunkVR_CpuCollectorQueryResult[0]=retained?1:0;
        finePending=pending->captureFine && retained;
        scenePending=pending->captureScene && retained;
        CyberpunkVR_CpuCollectorState.store(retained?4:3,std::memory_order_release);
    }catch(...){CyberpunkVR_CpuCollectorState=3;}
    return result;
}
bool QueryWanted(){return cvr::engine_stereo::SecondViewWanted() && QueryCompatible();}
CVR_DETOUR_IF("[single-pass] owned CPU query tasks",0x5ABBD0,QueryHook,originalQuery,QueryWanted)
QueryWork* FindQuery(void* query) {
    if(!pending)return nullptr;const auto address=reinterpret_cast<uintptr_t>(query);
    if(address && pending->sourceQuery[0]==address)return pending;
    return address && pending->peer && pending->peer->sourceQuery[0]==address?pending->peer.get():nullptr;
}
}
bool CaptureOwnedCpuFine(void* tester,int64_t* range,char partial,void* output,char& result) {
    if(!finePending.load(std::memory_order_relaxed))return false;
    std::lock_guard lock(pendingMutex);
    auto* work=FindQuery(tester);if(!work || !work->captureFine)return false;
    FineRange captured;std::array<uintptr_t,2> bounds{};cvr::stereo::NativeVisibleIds before{};size_t count{};
    bool valid=Read(range,bounds.data(),sizeof(bounds)) && cvr::stereo::FineCandidateCount(bounds[0],bounds[1],count) &&
        Read(output,&before,sizeof(before)) && before.count<=before.capacity && work->fineRanges.size()<4096 &&
        count*48<=4*1024*1024-work->fineBytes;
    try {
        if(valid){captured.candidates.resize(count);captured.partial=partial;captured.sourceBegin=bounds[0];
            valid=!count || Read(reinterpret_cast<void*>(bounds[0]),captured.candidates.data(),count*sizeof(cvr::stereo::NativeCullCandidate));}
    }catch(...){valid=false;}
    // Preserve the actual native call and its output exactly once, even when
    // diagnostic allocation/validation fails. Only its appended ID suffix is copied.
    result=g_orig_fine_materialize(tester,range,partial,output);
    cvr::stereo::NativeVisibleIds after{};
    if(valid)valid=Read(output,&after,sizeof(after)) && after.count>=before.count && after.count<=after.capacity && after.count-before.count<=count;
    try {
        if(valid){captured.reference.resize(after.count-before.count);
            valid=captured.reference.empty() || Read(after.data+before.count,captured.reference.data(),captured.reference.size()*8);
            if(valid){work->fineBytes+=static_cast<uint32_t>(count*40+captured.reference.size()*8);work->fineRanges.push_back(std::move(captured));}}
    }catch(...){valid=false;}
    if(!valid)++work->fineFailures;
    return true;
}
bool CaptureOwnedCpuScene(void* manager,void* context,void* output,void* query,void* job,int64_t& result) {
    if(!scenePending.load(std::memory_order_relaxed))return false;
    QueryWork* work{};
    {
        std::lock_guard lock(pendingMutex);
        work=FindQuery(query);if(!work || !work->captureScene || work->sceneStarted)return false;
        work->sceneStarted=true;work->sceneProcessing=true;
        try {
            uint32_t nativeCounter{};Read(static_cast<uint8_t*>(manager)+0x28,&nativeCounter,4);
            if(pending->boundsTrace)cvr::stereo::bounds_trace::Mark(work==pending?cvr::stereo::bounds_trace::Kind::VrcamBegin:cvr::stereo::bounds_trace::Kind::MainBegin,work==pending?1:0,nativeCounter);
            work->sceneValid=work->scene.Capture(static_cast<uint8_t*>(manager)+0x7E0F0,Read);
            if(pending->boundsTrace)cvr::stereo::bounds_trace::Mark(work==pending?cvr::stereo::bounds_trace::Kind::VrcamEnd:cvr::stereo::bounds_trace::Kind::MainEnd,work==pending?1:0,nativeCounter);
            if(work==pending && work->peer){
                auto* stats=CyberpunkVR_CpuCollectorPeerResult;
                stats[2]=work->jobs.Complete() && work->peer->jobs.Complete();
                bool valid=work->sceneValid && stats[2] && !work->peer->sourceQuery[0];
                QueryWork* eyes[]{work,work->peer.get()};uint32_t visible{};
                for(UINT eye=0;valid && eye<2;++eye){uintptr_t target{};
                    valid=Read(static_cast<uint8_t*>(eyes[eye]->collector.Storage())+0xDF2F0,&target,8) && target &&
                        work->scene.Evaluate(reinterpret_cast<cvr::stereo::NativeCoarseCull>(g_exe_base+0x29CEE4),
                            g_orig_fine_materialize,reinterpret_cast<void*>(target),work->earlyIds[eye],visible);
                    stats[6+eye]=static_cast<uint32_t>(work->earlyIds[eye].size());
                }
                stats[3]=valid;if(!valid)stats[13]=3;
            }
        }
        catch(...){work->sceneValid=false;work->scene.failure=11;}
    }
    // Submission may execute a fine-cull worker inline. Do not hold the capture
    // mutex across the native scheduler; sceneProcessing pins our diagnostic owner.
    result=g_orig_main_cull_test(manager,context,output,query,job);
    {
        std::lock_guard lock(pendingMutex);std::array<void*,2> groups{};
        work->cullJobsRetained=Read(static_cast<uint8_t*>(job)+0x10,groups.data(),sizeof(groups)) && work->cullJobs.Capture(groups[0],groups[1]);
        work->sceneProcessing=false;
    }
    return true;
}
bool CompareEye(QueryWork& work,uint32_t* queryStats,uint32_t* fineStats,uint32_t* sceneStats,uint32_t* control,float* planes=nullptr) {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);RustQueryComparison query{};
    cvr_ccp_compare_query(reinterpret_cast<uintptr_t>(work.collector.Storage()),work.sourceQuery[0],base,&query);
    const uintptr_t cloneQuery=query.cloneQuery,sourceType=query.sourceType,cloneType=query.cloneType;const bool valid=query.valid;
    if(planes){std::memcpy(planes,query.planes[0],128);std::memcpy(planes+32,query.planes[1],128);}
    const UINT differences=query.differences;
#endif
#else
    uintptr_t cloneQuery{},sourceType{},cloneType{},fine{};const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    bool valid=Read(static_cast<uint8_t*>(work.collector.Storage())+0xDF2F0,&cloneQuery,8) && cloneQuery &&
        Read(reinterpret_cast<void*>(work.sourceQuery[0]),&sourceType,8) && Read(reinterpret_cast<void*>(cloneQuery),&cloneType,8) &&
        sourceType==cloneType && Read(reinterpret_cast<void*>(sourceType+0x30),&fine,8) && fine==base+0x29D110;
    std::array<uint8_t,128> sourcePlanes{},clonePlanes{};std::array<uint8_t,16> sourceFlags{},cloneFlags{};
    if(valid)valid=Read(reinterpret_cast<void*>(work.sourceQuery[0]+0x1A0),sourcePlanes.data(),sourcePlanes.size()) &&
        Read(reinterpret_cast<void*>(cloneQuery+0x1A0),clonePlanes.data(),clonePlanes.size()) &&
        Read(reinterpret_cast<void*>(work.sourceQuery[0]+0x190),sourceFlags.data(),sourceFlags.size()) && Read(reinterpret_cast<void*>(cloneQuery+0x190),cloneFlags.data(),cloneFlags.size());
    if(planes){std::memcpy(planes,sourcePlanes.data(),128);std::memcpy(planes+32,clonePlanes.data(),128);}
    UINT differences{};if(valid){for(UINT i=0;i<128;++i)differences+=sourcePlanes[i]!=clonePlanes[i];
        for(UINT i:{0u,1u,4u,5u,6u,7u,8u,9u,10u,11u,12u,13u,14u})differences+=sourceFlags[i]!=cloneFlags[i];}
#endif
    queryStats[0]=work.jobs.Complete();queryStats[1]=valid;queryStats[2]=differences;queryStats[3]=cloneQuery!=work.sourceQuery[0];
    queryStats[4]=sourceType>=base && sourceType-base<=UINT_MAX?UINT(sourceType-base):0;
    queryStats[5]=cloneType>=base && cloneType-base<=UINT_MAX?UINT(cloneType-base):0;
    queryStats[6]=work.jobs.Complete() && work.sourceJobs.Complete();
    bool fineValid=true;
    if(work.captureFine){
        auto* stats=fineStats;stats[0]=static_cast<uint32_t>(work.fineRanges.size());
        stats[5]=work.fineFailures;stats[7]=work.fineBytes;
        uintptr_t sourceOcclusion{},cloneOcclusion{};uint8_t sourceActive{},cloneActive{};
        fineValid=valid && Read(reinterpret_cast<void*>(work.sourceQuery[0]+0x240),&sourceOcclusion,8) &&
            Read(reinterpret_cast<void*>(cloneQuery+0x240),&cloneOcclusion,8) &&
            Read(reinterpret_cast<void*>(work.sourceQuery[0]+0x462),&sourceActive,1) && Read(reinterpret_cast<void*>(cloneQuery+0x462),&cloneActive,1);
        stats[8]=sourceActive;stats[9]=cloneActive;stats[10]=sourceOcclusion!=0;stats[11]=cloneOcclusion!=0;
        if(fineValid)try {
            for(const auto& row:work.fineRanges){
                stats[1]+=static_cast<uint32_t>(row.candidates.size());stats[2]+=static_cast<uint32_t>(row.reference.size());
                std::vector<uint64_t> ids;
                if(!cvr::stereo::RunNativeFine(g_orig_fine_materialize,reinterpret_cast<void*>(cloneQuery),row.candidates,row.partial,ids)){++stats[6];continue;}
                stats[3]+=static_cast<uint32_t>(ids.size());stats[4]+=ids!=row.reference;
            }
        }catch(...){++stats[6];}
        fineValid=fineValid && stats[0] && stats[1] && !stats[4] && !stats[5] && !stats[6] && sourceActive==cloneActive && bool(sourceOcclusion)==bool(cloneOcclusion);
    }
    bool sceneValid=true;
    if(work.captureScene){
        auto* stats=sceneStats;stats[0]=work.sceneStarted;stats[1]=work.scene.failure;
        stats[2]=work.scene.declared;stats[3]=static_cast<uint32_t>(work.scene.buckets.size());
        stats[4]=work.scene.candidateCount;stats[9]=work.cullJobsRetained && work.cullJobs.Complete();stats[10]=work.scene.bytesOwned;
        sceneValid=valid && work.sceneValid && stats[0] && stats[9];
        if(sceneValid)try {
            std::vector<uint64_t> ids,reference,sourceIds;
            sceneValid=work.scene.Evaluate(reinterpret_cast<cvr::stereo::NativeCoarseCull>(base+0x29CEE4),
                g_orig_fine_materialize,reinterpret_cast<void*>(cloneQuery),ids,stats[5]);
            sceneValid=sceneValid && work.scene.Evaluate(reinterpret_cast<cvr::stereo::NativeCoarseCull>(base+0x29CEE4),
                g_orig_fine_materialize,reinterpret_cast<void*>(work.sourceQuery[0]),sourceIds,control[3]);
            control[4]=static_cast<uint32_t>(sourceIds.size());
            for(const auto& row:work.fineRanges){
                const bool inPool=std::any_of(work.scene.buckets.begin(),work.scene.buckets.end(),[&](const auto& bucket){
                    return bucket.sourceBegin==row.sourceBegin && bucket.candidates.size()==row.candidates.size();});
                if(inPool){++control[0];reference.insert(reference.end(),row.reference.begin(),row.reference.end());}
                else {++control[1];control[2]+=static_cast<uint32_t>(row.reference.size());}
            }
            stats[6]=static_cast<uint32_t>(ids.size());stats[7]=static_cast<uint32_t>(reference.size());
            std::sort(ids.begin(),ids.end());std::sort(reference.begin(),reference.end());std::sort(sourceIds.begin(),sourceIds.end());
#if CVR_RUST_VRIK
            stats[8]=cvr_ccp_different_ids(ids.data(),ids.size(),reference.data(),reference.size());
#else
            stats[8]=static_cast<uint32_t>(ids.size()>reference.size()?ids.size()-reference.size():reference.size()-ids.size());
            for(size_t i=0;i<(std::min)(ids.size(),reference.size());++i)stats[8]+=ids[i]!=reference[i];
#endif
            stats[11]=stats[5]==control[0];control[5]=sourceIds!=reference;control[6]=sourceIds!=ids;
            control[7]=control[0]+control[1]==work.fineRanges.size();
            sceneValid=sceneValid && !control[5] && !control[6] && control[7] && control[3]==control[0];
        }catch(...){sceneValid=false;stats[1]=12;}
        sceneValid=sceneValid && stats[3] && !stats[8] && stats[11];
    }
    return valid && !differences && fineValid && sceneValid && queryStats[0] && queryStats[3] && queryStats[6];
}
uint32_t DifferentIds(std::vector<uint64_t> a,std::vector<uint64_t> b) {
#if CVR_RUST_VRIK
    return cvr_ccp_different_ids(a.data(),a.size(),b.data(),b.size());
#else
    std::sort(a.begin(),a.end());std::sort(b.begin(),b.end());
    uint32_t count=static_cast<uint32_t>(a.size()>b.size()?a.size()-b.size():b.size()-a.size());
    for(size_t i=0;i<(std::min)(a.size(),b.size());++i)count+=a[i]!=b[i];return count;
#endif
}
bool Finished(const QueryWork& work) {
    return work.jobs.Complete() && (!work.sourceQuery[0] || work.sourceJobs.Complete()) &&
        !work.sceneProcessing && (!work.cullJobsRetained || work.cullJobs.Complete());
}
void PollOwnedCpuQuery() {
    presentEpoch.fetch_add(1,std::memory_order_release);
    if(!queryPending.load(std::memory_order_acquire))return;
    std::lock_guard lock(pendingMutex);
    if(!pending || !Finished(*pending) || (pending->peer && !Finished(*pending->peer)))return;
    if(pending->boundsTrace && !cvr::stereo::bounds_trace::End())return;
    finePending=false;scenePending=false;peerPending=false;
    bool valid=CompareEye(*pending,CyberpunkVR_CpuCollectorQueryResult,CyberpunkVR_CpuCollectorFineResult,
        CyberpunkVR_CpuCollectorSceneResult,CyberpunkVR_CpuCollectorSceneControl);
    if(pending->peer){
        auto* stats=CyberpunkVR_CpuCollectorPeerResult;
        const auto inputDiff=pending->scene.CandidateDifferences(pending->peer->scene);
        std::copy(inputDiff.begin(),inputDiff.end(),CyberpunkVR_CpuCollectorPeerInputDiff);
        const auto& first=pending->scene.buckets;const auto& second=pending->peer->scene.buckets;
        for(size_t i=0;i<(std::min)(first.size(),second.size());++i)
            for(size_t j=0;j<(std::min)(first[i].candidates.size(),second[i].candidates.size());++j){
                const auto& a=first[i].candidates[j];const auto& b=second[i].candidates[j];
                if(a.id!=b.id || !std::memcmp(a.bounds.data(),b.bounds.data(),24) || CyberpunkVR_CpuCollectorPeerChangesCount==64)continue;
                auto& row=CyberpunkVR_CpuCollectorPeerChanges[CyberpunkVR_CpuCollectorPeerChangesCount++];
#if CVR_RUST_VRIK
                row.id=a.id;row.before=a.bounds;row.after=b.bounds;uint32_t rvas[2]{};
                if(cvr_ccp_candidate_type(a.id,reinterpret_cast<uintptr_t>(g_exe_base),rvas)){row.type=rvas[0];row.prepare=rvas[1];}
#else
                row.id=a.id;row.before=a.bounds;row.after=b.bounds;uintptr_t type{},prepare{};
                const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
                if(a.id>=16 && Read(reinterpret_cast<void*>(a.id-16),&type,8) && Read(reinterpret_cast<void*>(type+0xF8),&prepare,8)){
                    row.type=type>=base && type-base<=UINT_MAX?UINT(type-base):0;
                    row.prepare=prepare>=base && prepare-base<=UINT_MAX?UINT(prepare-base):0;
                }
#endif
            }
        stats[10]=pending->scene.SameCandidates(pending->peer->scene);
        stats[11]=CompareEye(*pending->peer,CyberpunkVR_CpuCollectorPeerQuery,CyberpunkVR_CpuCollectorPeerFine,
            CyberpunkVR_CpuCollectorPeerScene,CyberpunkVR_CpuCollectorPeerControl,CyberpunkVR_CpuCollectorPeerPlanes);
        try {
            QueryWork* eyes[]{pending,pending->peer.get()};
            for(UINT eye=0;eye<2;++eye){std::vector<uint64_t> reference;
                for(const auto& row:eyes[eye]->fineRanges){
                    if(std::any_of(eyes[eye]->scene.buckets.begin(),eyes[eye]->scene.buckets.end(),[&](const auto& bucket){
                        return bucket.sourceBegin==row.sourceBegin && bucket.candidates.size()==row.candidates.size();}))
                        reference.insert(reference.end(),row.reference.begin(),row.reference.end());
                }
                stats[8+eye]=DifferentIds(pending->earlyIds[eye],reference);
            }
#if CVR_RUST_VRIK
            uint32_t overlap[2]{};const auto& early=pending->earlyIds;
            cvr_ccp_id_overlap(early[0].data(),early[0].size(),early[1].data(),early[1].size(),overlap);
            stats[14]=overlap[0];stats[15]=overlap[1];
#else
            auto a=pending->earlyIds[0],b=pending->earlyIds[1];std::sort(a.begin(),a.end());std::sort(b.begin(),b.end());
            std::vector<uint64_t> both;std::set_intersection(a.begin(),a.end(),b.begin(),b.end(),std::back_inserter(both));
            stats[14]=static_cast<uint32_t>(both.size());stats[15]=static_cast<uint32_t>(a.size()+b.size()-both.size()*2);
#endif
        }catch(...){stats[13]=4;}
        valid=valid && stats[0] && stats[1] && stats[2] && stats[3] && stats[4] && stats[5] &&
            stats[6] && stats[7] && !stats[8] && !stats[9] && stats[10] && stats[11] && !stats[13];
    }
    const bool peer=bool(pending->peer);delete pending;pending=nullptr;queryPending=false;
    CyberpunkVR_CpuCollectorResult[5]=1;CyberpunkVR_CpuCollectorQueryResult[7]=1;
    if(peer){CyberpunkVR_CpuCollectorPeerResult[12]=1;CyberpunkVR_CpuCollectorPeerQuery[7]=1;}
    CyberpunkVR_CpuCollectorState.store(valid && CyberpunkVR_CpuCollectorResult[6]?2:3,std::memory_order_release);
}
}
namespace cvr::stereo {
void PollCpuCollector(){cvr::detail::PollOwnedCpuQuery();}
bool CaptureCpuFine(void* query,int64_t* range,char partial,void* output,char& result){return cvr::detail::CaptureOwnedCpuFine(query,range,partial,output,result);}
bool CaptureCpuScene(void* manager,void* context,void* output,void* query,void* job,int64_t& result){return cvr::detail::CaptureOwnedCpuScene(manager,context,output,query,job,result);}
}
#endif
