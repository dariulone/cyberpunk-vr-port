#include "Render/NativePreparation.hpp"
#include "Render/NativeGraphParent.hpp"
#include "Render/NativeGraphSnapshot.hpp"
#include "Render/NativePreparationSchedule.hpp"
#include "Render/PairedPassFrontier.hpp"
#include "Render/PairedCpuFrontier.hpp"
#include "Render/NativeContextOrder.hpp"
#include "Render/OwnedNativeGraph.hpp"
#include "Render/NativeGraphJobs.hpp"
#include "Render/NativeGraphJobReceipt.hpp"
#include "Render/NativeResourceUses.hpp"
#include "Render/NativeDefinitionTracking.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Core/VrCoreShared.hpp"
#include <windows.h>
#include <intrin.h>
#include <mutex>

extern "C" {
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
__declspec(dllexport) uint64_t CyberpunkVR_PreparationRootObserved[16][8]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationRootObservedCount{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationGraphRequest{},CyberpunkVR_PreparationGraphState{},CyberpunkVR_PreparationGraphSeq{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationGraphNodeCount{},CyberpunkVR_PreparationGraphEdgeCount{},CyberpunkVR_PreparationGraphFailure{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphNodes{},CyberpunkVR_PreparationGraphEdges{},CyberpunkVR_PreparationGraphMeta[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationFrontierStats[16]{},CyberpunkVR_PreparationFrontierRoles[22]{},CyberpunkVR_PreparationFrontierCuts[6]{},CyberpunkVR_PreparationFrontierAdds[6]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationOwnedGraphMode{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationOwnedGraphStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationOwnedGraphNodes{},CyberpunkVR_PreparationOwnedGraphEdges{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationWholePassStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationContextOrderStats[24]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationContextOrderWitnesses{};
__declspec(dllexport) uint32_t CyberpunkVR_PreparationContextOrderWitnessCount{},CyberpunkVR_PreparationContextOrderWitnessBytes=24;
__declspec(dllexport) uint64_t CyberpunkVR_PreparationSharedOwnerStats[8]{},CyberpunkVR_PreparationSharedOwnerRows{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) uint32_t CyberpunkVR_PreparationSharedOwnerRowBytes=32;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationGraphJobsMode{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationGraphJobsEpoch{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphJobsStats[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphJobsCompatibility[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphJobsSelection[8]{},CyberpunkVR_PreparationGraphJobsPending[4][8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationGraphJobsSourceReject[8]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationGraphJobsPendingCount{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationResourceUsesStats[12]{},CyberpunkVR_PreparationResourceUsesRows{},CyberpunkVR_PreparationResourceUseIds{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationResourceAuxKeys{},CyberpunkVR_PreparationResourceBoundaryIds{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationResourceMarkerContract{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationScheduleMode{},CyberpunkVR_PreparationScheduleCompatible{},CyberpunkVR_PreparationScheduleSeq{};
__declspec(dllexport) uint32_t CyberpunkVR_PreparationScheduleStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationScheduleRanges[4]{},CyberpunkVR_PreparationScheduleEdges{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparationScheduleCache{1};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PreparationScheduleCpu[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparationScheduleCpuFrequency{};
#endif
}
#if CVR_RUST_VRIK
// The paired frame's engine hooks: rust/crates/cvr-render/src/paired_frame.rs (the six detours registered there with
// their code contracts; the graph captures, frontier plans, owned graph comparison, job receipts, resource uses and
// schedule cache; preparation's graph parent source, graph job poll and definition tracking are called in Rust); the
// exports' C++ definitions above serve the C++ build.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin registers them in rust/crates/cvr-render/src/engine_detours.rs cvr_static_registrations
namespace {
extern "C" void cvr_npf_attach();
const bool kPairedFrameAttached=(cvr_npf_attach(),true);
}
#endif
#else
#if CVR_RUST_VRIK
// The game-code contracts: rust/crates/cvr-render/src/paired_frame_code.rs (0 root, 1 graph, 2 sort, 3 resource uses,
// 4 resource markers, 5 proxy gather, 6 schedule; the switch, caches and exports stay here).
extern "C" bool cvr_npf_code(uint32_t,uint64_t);
extern "C" bool cvr_npf_graph_jobs(uint64_t,uint64_t*);
#endif

namespace cvr::detail {
namespace {
using RenderRoot=uintptr_t(*)(void*,void*);
RenderRoot original{};
using DeclareGraph=uintptr_t(*)(void*,void*,void*);
DeclareGraph originalDeclare{};
cvr::stereo::graph_snapshot::Snapshot graphSnapshot;
cvr::stereo::paired_frontier::Plan frontierPlan;
cvr::stereo::paired_cpu_frontier::Plan cpuFrontierPlan;
cvr::stereo::graph_snapshot::Snapshot ownedGraphSnapshot;
std::vector<std::array<uint32_t,6>> contextOrderWitnesses;
std::vector<std::array<uint64_t,4>> sharedOwnerRows;
std::mutex observationMutex;
std::mutex graphJobsMutex;
std::mutex graphCaptureMutex;
void RetainGraphOwner(void* owner);
void ReleaseGraphOwner(void* owner);
struct DeclarationOwner {
    explicit DeclarationOwner(uint64_t address):value(reinterpret_cast<void*>(uintptr_t(address))){RetainGraphOwner(value);}
    ~DeclarationOwner(){ReleaseGraphOwner(value);}
    void* value{};
};
std::unique_ptr<DeclarationOwner> declarationOwner;
std::vector<std::unique_ptr<cvr::stereo::graph_jobs::DeferredReceipt>> graphJobs;
uint64_t graphJobsEpochSeen{},graphCaptureEpoch{},graphCaptureGeneration{},graphCaptureOwner{};
std::mutex resourceUsesMutex;
cvr::stereo::resource_uses::Snapshot resourceUses;
uint64_t resourcePool{},resourceWrapper{};
void ArmGraphJobsLocked();
void PollOwnedGraphJobs(){
    cvr::stereo::definition_tracking::Poll();
    std::lock_guard lock(graphJobsMutex);
    ArmGraphJobsLocked();
    for(const auto& receipt:graphJobs)if(CyberpunkVR_PreparationGraphJobsMode.load()==1 && CyberpunkVR_PreparationGraphState.load()==2 &&
        receipt->Selected() && receipt->Epoch()==graphJobsEpochSeen && graphCaptureEpoch==graphJobsEpochSeen &&
        graphCaptureGeneration==CyberpunkVR_PreparationPairGeneration.load() &&
        receipt->Match(graphCaptureEpoch,graphCaptureGeneration,reinterpret_cast<uintptr_t>(g_exe_base),
            CyberpunkVR_PreparationGraphMeta[0],graphCaptureOwner,graphSnapshot) && receipt->Complete()){
        CyberpunkVR_PreparationGraphJobsStats[5]=1;CyberpunkVR_PreparationGraphJobsStats[0]=3;}
    std::erase_if(graphJobs,[](const auto& receipt){return receipt->Complete() &&
        (!CyberpunkVR_PreparationGraphJobsMode.load() || receipt->Epoch()!=CyberpunkVR_PreparationGraphJobsEpoch.load());});
    if(!CyberpunkVR_PreparationGraphJobsMode.load())declarationOwner.reset();
}
void ArmGraphJobsLocked(){
    const auto epoch=CyberpunkVR_PreparationGraphJobsEpoch.load();
    if(epoch!=graphJobsEpochSeen){graphJobsEpochSeen=epoch;graphCaptureEpoch=graphCaptureGeneration=graphCaptureOwner=0;
        declarationOwner.reset();
        std::fill_n(CyberpunkVR_PreparationGraphJobsSourceReject,8,0ull);
        std::fill_n(CyberpunkVR_PreparationGraphJobsStats,8,0ull);}
    std::erase_if(graphJobs,[epoch](const auto& receipt){return receipt->Epoch()!=epoch && receipt->Complete();});
}
void SelectGraphJobsLocked(){
    auto* selection=CyberpunkVR_PreparationGraphJobsSelection;
    selection[0]=graphCaptureEpoch;selection[1]=graphCaptureGeneration;selection[2]=graphCaptureOwner;
    selection[3]=CyberpunkVR_PreparationGraphJobsEpoch.load();selection[4]=CyberpunkVR_PreparationPairGeneration.load();
    selection[5]=CyberpunkVR_PreparationGraphMeta[0];selection[6]=CyberpunkVR_PreparationGraphState.load();selection[7]=declarationOwner!=nullptr;
    CyberpunkVR_PreparationGraphJobsPendingCount=uint32_t(graphJobs.size());
    for(size_t i=0;i<graphJobs.size();++i){const auto& receipt=graphJobs[i];const auto copy=receipt->Source();
        uint64_t mask{};mask|=(receipt->Epoch()==graphCaptureEpoch)?1:0;mask|=(receipt->Generation()==graphCaptureGeneration)?2:0;
        mask|=(receipt->GraphAddress()==CyberpunkVR_PreparationGraphMeta[0])?4:0;mask|=(receipt->FrameOwner()==graphCaptureOwner)?8:0;
        if(CyberpunkVR_PreparationGraphState.load()==2){mask|=(copy->nodes==graphSnapshot.nodes)?16:0;mask|=(copy->edges==graphSnapshot.edges)?32:0;}
        const uint64_t row[]{receipt->Epoch(),receipt->Generation(),receipt->FrameOwner(),receipt->GraphAddress(),
            cvr::stereo::paired_frontier::detail::Fingerprint(*copy,reinterpret_cast<uintptr_t>(g_exe_base)),mask,receipt->Complete(),receipt->Selected()};
        std::copy_n(row,8,CyberpunkVR_PreparationGraphJobsPending[i]);}
    if(!declarationOwner || CyberpunkVR_PreparationGraphJobsMode.load()!=1 || CyberpunkVR_PreparationGraphState.load()!=2 || !graphCaptureEpoch ||
        graphCaptureEpoch!=graphJobsEpochSeen || graphCaptureEpoch!=CyberpunkVR_PreparationGraphJobsEpoch.load() ||
        graphCaptureGeneration!=CyberpunkVR_PreparationPairGeneration.load())return;
    for(const auto& receipt:graphJobs)if(receipt->Select(graphCaptureEpoch,graphCaptureGeneration,
        reinterpret_cast<uintptr_t>(g_exe_base),CyberpunkVR_PreparationGraphMeta[0],graphCaptureOwner,graphSnapshot)){
        auto* stats=CyberpunkVR_PreparationGraphJobsStats;
        stats[1]=receipt->NodeCount();stats[2]=receipt->FrameOwner();stats[3]=receipt->GraphAddress();
        stats[4]=reinterpret_cast<uintptr_t>(g_exe_base)+cvr::stereo::graph_jobs::QueueReturn;
        stats[7]=frontierPlan.SourceFingerprint();stats[6]=0;
        stats[5]=receipt->Complete();stats[0]=stats[5]?3:2;return;}
}
bool Read(uintptr_t address,void* out,size_t bytes){SIZE_T copied{};return address && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),out,bytes,&copied) && copied==bytes;}
void Observe(void* data,uintptr_t caller){
    std::lock_guard lock(observationMutex);const auto index=CyberpunkVR_PreparationRootObservedCount.load();if(index>=16)return;
    uint64_t row[8]{};row[0]=caller-reinterpret_cast<uintptr_t>(g_exe_base);row[1]=reinterpret_cast<uintptr_t>(data);
    row[5]=cvr::stereo::preparation::Active();row[6]=CyberpunkVR_PreparationPairVerified.load();row[7]=CyberpunkVR_PreparationRootSkipped.load();
    if(data){const auto source=reinterpret_cast<uintptr_t>(data);uint32_t mode{},count{};uintptr_t names{};
        Read(source+0xFA0,&mode,4);Read(source+0x14C,&count,4);row[2]=mode;row[3]=count;
        if(count && Read(source+0x140,&names,8))Read(names,&row[4],8);}
    std::copy_n(row,8,CyberpunkVR_PreparationRootObserved[index]);CyberpunkVR_PreparationRootObservedCount.store(index+1,std::memory_order_release);
}
bool Compatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
#if CVR_RUST_VRIK
    const bool valid=cvr_npf_code(0,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    CyberpunkVR_PreparationRootCompatible=valid?1:2;return valid;
#else
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,0xA00> code{};SIZE_T copied{};
    bool valid=ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(base+0x409DD4),code.data(),code.size(),&copied) && copied==code.size();
    uint64_t hash=14695981039346656037ull;for(auto byte:code)hash=(hash^byte)*1099511628211ull;
    valid=valid && hash==0xDED3369EB748ED88ull;CyberpunkVR_PreparationRootCompatible=valid?1:2;return valid;
#endif
}
void* FrameData(void* context){
    __try {
        auto* owner=*reinterpret_cast<uint8_t**>(static_cast<uint8_t*>(context)+0x38);
        if(!owner)return nullptr;
        const auto table=*reinterpret_cast<uintptr_t**>(owner);
        return reinterpret_cast<void*(*)(void*)>(table[4])(owner);
    }__except(EXCEPTION_EXECUTE_HANDLER){return nullptr;}
}
uintptr_t Hook(void* renderer,void* context){
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    const auto generation=CyberpunkVR_PreparationPairGeneration.load();
    void* data{};const auto request=CyberpunkVR_PreparationPairRequest.load(std::memory_order_relaxed);
    if((request==2 || request==3) && cvr::stereo::preparation::Pairing() && !g_menuModeValue && !CyberpunkVR_CullReuseMode){
        if(caller==reinterpret_cast<uintptr_t>(g_exe_base)+0xB60377){
            data=FrameData(context);if(request==3 && cvr::stereo::preparation::Active())Observe(data,caller);
            if(cvr::stereo::preparation::SuppressRtt(data,g_vrcam_ctx_key.load(),true))return 0;
        }
    }
    const auto admission=cvr::stereo::definition_tracking::BeginRoot(reinterpret_cast<uintptr_t>(renderer),caller);
    const auto result=original(renderer,context);
    cvr::stereo::definition_tracking::EndRoot(admission);
    if(data)cvr::stereo::preparation::RootCompleted(data,generation);
    return result;
}
CVR_DETOUR_IF("[single-pass] bounded paired-frame root",0x409DD4,Hook,original,Compatible)
bool GraphCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
#if CVR_RUST_VRIK
    return cvr_npf_code(1,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
#else
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,0x5F> bytes{};
    if(!Read(base+0x547CF4,bytes.data(),bytes.size()))return false;
    uint64_t hash=14695981039346656037ull;for(auto byte:bytes)hash=(hash^byte)*1099511628211ull;
    return hash==0xB21BDD71777C2FE4ull;
#endif
}
bool NativeSortCompatible(){
#if CVR_RUST_VRIK
    static const bool valid=cvr_npf_code(2,reinterpret_cast<uintptr_t>(g_exe_base));return valid;
#else
    static const bool valid=[] {
        constexpr struct Code {UINT rva,bytes;uint64_t hash;} code[]{
            {0x8E9B28,0x1A2,0xBBD50CE198B2DC60ull},{0x8E9FEC,0xDE,0x7EDD805AFDD84978ull}};
        const auto base=reinterpret_cast<uintptr_t>(g_exe_base);std::array<uint8_t,0x200> bytes{};
        for(const auto& row:code){if(!Read(base+row.rva,bytes.data(),row.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<row.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=row.hash)return false;}
        return true;
    }();return valid;
#endif
}
bool NativeSort(void* graph){
    __try{reinterpret_cast<void(*)(void*)>(reinterpret_cast<uintptr_t>(g_exe_base)+0x8E9B28)(graph);return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
void CompareOwnedGraph(){
    const auto mode=CyberpunkVR_PreparationOwnedGraphMode.load();if(!mode)return;
    auto* stats=CyberpunkVR_PreparationOwnedGraphStats;std::fill_n(stats,16,0ull);stats[0]=1;stats[1]=mode;
    CyberpunkVR_PreparationOwnedGraphNodes=CyberpunkVR_PreparationOwnedGraphEdges=0;ownedGraphSnapshot={};
    std::fill_n(CyberpunkVR_PreparationWholePassStats,16,0ull);
    std::fill_n(CyberpunkVR_PreparationContextOrderStats,24,0ull);
    CyberpunkVR_PreparationContextOrderWitnesses=0;CyberpunkVR_PreparationContextOrderWitnessCount=0;contextOrderWitnesses.clear();
    std::fill_n(CyberpunkVR_PreparationSharedOwnerStats,8,0ull);CyberpunkVR_PreparationSharedOwnerRows=0;sharedOwnerRows.clear();
    const auto fail=[&](UINT reason){stats[14]=reason;stats[0]=4;};
    if((mode!=1 && mode!=2 && mode!=3) || !NativeSortCompatible() || frontierPlan.State()!=cvr::stereo::paired_frontier::Result::CandidateNeedsCpuAndResourceCompile){fail(1);return;}
    stats[2]=graphSnapshot.nodes.size();stats[3]=graphSnapshot.edges.size();stats[15]=frontierPlan.SourceFingerprint();
    cvr::stereo::OwnedNativeGraph reference;
    if(!reference.Create(graphSnapshot,frontierPlan.Original()) || !NativeSort(reference.SortableData()) || !reference.ValidateCompiled()){fail(2);return;}
    stats[4]=1;stats[5]=reference.SameSourceOrders();if(!stats[5]){fail(3);return;}
    if(mode==2 || mode==3){
        cvr::stereo::paired_frontier::Plan wholePass;
        const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
        if(mode==3 && wholePass.BuildCandidate(graphSnapshot,base,cvr::stereo::paired_frontier::Boundary::Discard)!=
            cvr::stereo::paired_frontier::Result::CandidateNeedsCpuAndResourceCompile){fail(7);return;}
        const auto& selected=mode==3?wholePass:frontierPlan;
        if(cpuFrontierPlan.BuildCandidate(selected)!=cvr::stereo::paired_cpu_frontier::Result::CandidateNeedsOwnershipAndResourceCompile){stats[13]=UINT(cpuFrontierPlan.Rejection());fail(4);return;}
        cvr::stereo::OwnedNativeGraph candidate;
        if(!candidate.Create(graphSnapshot,cpuFrontierPlan.Proposed()) || !NativeSort(candidate.SortableData()) || !candidate.ValidateCompiled()){fail(5);return;}
        ownedGraphSnapshot=candidate.Snapshot();stats[6]=1;
        for(size_t i=0;i<graphSnapshot.nodes.size();++i)for(UINT phase=0;phase<2;++phase)
            stats[7+phase]+=graphSnapshot.nodes[i].bytes[28+phase]!=ownedGraphSnapshot.nodes[i].bytes[28+phase];
        stats[9]=cpuFrontierPlan.Proposed()[0].size();stats[10]=cpuFrontierPlan.Proposed()[1].size();
        stats[11]=cpuFrontierPlan.Early().size();stats[12]=cpuFrontierPlan.LateBarriers().size();
        if(mode==3){
            auto* full=CyberpunkVR_PreparationWholePassStats;
            const auto& peer=selected.EyeRoles()[1];
            const uint64_t summary[]{3,1,selected.SourceFingerprint(),graphSnapshot.nodes.size(),
                selected.Original()[0].size(),selected.Original()[1].size(),cpuFrontierPlan.Proposed()[0].size(),
                cpuFrontierPlan.Proposed()[1].size(),cpuFrontierPlan.Early().size(),cpuFrontierPlan.LateBarriers().size(),
                cpuFrontierPlan.FrontierTargets().size(),cpuFrontierPlan.Removed()[0].size(),cpuFrontierPlan.Added()[0].size(),
                selected.EarlyTail(),peer.afterDiscard,0};
            std::copy_n(summary,16,full);
            for(const auto& edge:cpuFrontierPlan.SharedOwnerOrders()){
                const auto source=std::find_if(graphSnapshot.nodes.begin(),graphSnapshot.nodes.end(),[&](const auto& node){return (node.handle&0xDFFFFFFFu)==edge.first;});
                if(source==graphSnapshot.nodes.end()){fail(11);return;}
                sharedOwnerRows.push_back({cvr::stereo::graph_snapshot::Field<uint64_t>(source->bytes,16),edge.first,edge.second,
                    cpuFrontierPlan.ReversedOwnerOrders().contains(edge)?2u:cpuFrontierPlan.RestoredOwnerOrders().contains(edge)?1u:0u});
            }
            const uint64_t owners[]{3,selected.SourceFingerprint(),cpuFrontierPlan.SharedOwnerOrders().size(),
                cpuFrontierPlan.RestoredOwnerOrders().size(),cpuFrontierPlan.ReversedOwnerOrders().size(),
                cpuFrontierPlan.Proofs().sharedOwnerOrderPreserved,sharedOwnerRows.size(),0};
            std::copy_n(owners,8,CyberpunkVR_PreparationSharedOwnerStats);
            CyberpunkVR_PreparationSharedOwnerRows=reinterpret_cast<uintptr_t>(sharedOwnerRows.data());
            cvr::stereo::OwnedNativeGraph phaseOnly;
            if(!phaseOnly.Create(graphSnapshot,selected.Proposed()) || !NativeSort(phaseOnly.SortableData()) || !phaseOnly.ValidateCompiled()){fail(8);return;}
            const std::array<cvr::stereo::graph_snapshot::Snapshot,3> scenarios{graphSnapshot,phaseOnly.Snapshot(),ownedGraphSnapshot};
            for(UINT i=0;i<scenarios.size();++i){
                const auto proof=cvr::stereo::context_order::Analyze(scenarios[i],base,Read);
                const uint64_t row[]{UINT(proof.failure),proof.fingerprint,proof.publishers,proof.synchronizers,
                    proof.requiredPairs,proof.provedPairs,proof.missing.size(),0};
                std::copy_n(row,8,CyberpunkVR_PreparationContextOrderStats+i*8);
                for(const auto& witness:proof.missing)contextOrderWitnesses.push_back({i,witness.producer,witness.consumer,
                    witness.producerSlot,witness.consumerSlot,witness.kind});
                if(contextOrderWitnesses.size()>4096){fail(9);return;}
            }
            CyberpunkVR_PreparationContextOrderWitnesses=reinterpret_cast<uintptr_t>(contextOrderWitnesses.data());
            CyberpunkVR_PreparationContextOrderWitnessCount=UINT(contextOrderWitnesses.size());
            // This checks the native publication/cursor dependency, not ownership
            // or GPU aliasing. The candidate is sorted only, never executed here.
            if(CyberpunkVR_PreparationContextOrderStats[0] || CyberpunkVR_PreparationContextOrderStats[16]){fail(10);return;}
        }
    }else ownedGraphSnapshot=reference.Snapshot();
    CyberpunkVR_PreparationOwnedGraphNodes=reinterpret_cast<uintptr_t>(ownedGraphSnapshot.nodes.data());
    CyberpunkVR_PreparationOwnedGraphEdges=reinterpret_cast<uintptr_t>(ownedGraphSnapshot.edges.data());stats[0]=3;
}
uintptr_t GraphHook(void* graph,void* context,void* job){
    const auto declaration=cvr::stereo::definition_tracking::BeforeDeclare(reinterpret_cast<uintptr_t>(graph),reinterpret_cast<uintptr_t>(context),reinterpret_cast<uintptr_t>(job),reinterpret_cast<uintptr_t>(_ReturnAddress()));
    const auto declare=[&]{const auto result=originalDeclare(graph,context,job);cvr::stereo::definition_tracking::AfterDeclare(declaration);return result;};
    if(cvr::stereo::preparation::Active() && CyberpunkVR_PreparationPairState.load()==2 && CyberpunkVR_PreparationGraphRequest.load()){
        std::unique_lock captureLock(graphCaptureMutex);
        if(!cvr::stereo::preparation::Active() || CyberpunkVR_PreparationPairState.load()!=2 || !CyberpunkVR_PreparationGraphRequest.exchange(0)){
            captureLock.unlock();
            return declare();
        }
        const auto captureMode=CyberpunkVR_PreparationGraphJobsMode.load(),captureEpoch=CyberpunkVR_PreparationGraphJobsEpoch.load();
        const auto captureGeneration=CyberpunkVR_PreparationPairGeneration.load();
        CyberpunkVR_PreparationGraphState=1;CyberpunkVR_PreparationGraphSeq.fetch_add(1);
        {std::lock_guard lock(graphJobsMutex);ArmGraphJobsLocked();
            graphCaptureEpoch=captureEpoch;graphCaptureGeneration=captureGeneration;
            declarationOwner.reset();graphCaptureOwner=0;Read(reinterpret_cast<uintptr_t>(context),&graphCaptureOwner,8);
            int32_t references{};
            if(captureMode==1 && graphCaptureOwner && graphCaptureOwner<=UINT64_MAX-12 &&
                Read(graphCaptureOwner+8,&references,4) && references>0){
                try{declarationOwner=std::make_unique<DeclarationOwner>(graphCaptureOwner);}catch(...){graphCaptureOwner=0;}}
            else graphCaptureOwner=0;}
        bool valid=false;
        try{valid=cvr::stereo::definition_tracking::CopyOwnedDeclarationSnapshot(declaration,graphSnapshot) ||
            graphSnapshot.Capture(reinterpret_cast<uintptr_t>(graph),Read);}catch(...){graphSnapshot.failure=6;}
        CyberpunkVR_PreparationGraphMeta[0]=reinterpret_cast<uintptr_t>(graph);
        CyberpunkVR_PreparationGraphMeta[1]=reinterpret_cast<uintptr_t>(g_exe_base);
        CyberpunkVR_PreparationGraphMeta[2]=reinterpret_cast<uintptr_t>(context);
        CyberpunkVR_PreparationGraphMeta[3]=CyberpunkVR_PreparationRootSkipped.load();
        {std::lock_guard lock(resourceUsesMutex);resourceUses={};resourcePool=resourceWrapper=0;
            std::fill_n(CyberpunkVR_PreparationResourceUsesStats,12,0ull);CyberpunkVR_PreparationResourceUsesRows=CyberpunkVR_PreparationResourceUseIds=0;
            CyberpunkVR_PreparationResourceAuxKeys=CyberpunkVR_PreparationResourceBoundaryIds=0;
            if(Read(reinterpret_cast<uintptr_t>(context)+8,&resourceWrapper,8) && resourceWrapper)Read(resourceWrapper,&resourcePool,8);}
        CyberpunkVR_PreparationGraphNodes=reinterpret_cast<uintptr_t>(graphSnapshot.nodes.data());
        CyberpunkVR_PreparationGraphEdges=reinterpret_cast<uintptr_t>(graphSnapshot.edges.data());
        CyberpunkVR_PreparationGraphNodeCount=static_cast<uint32_t>(graphSnapshot.nodes.size());
        CyberpunkVR_PreparationGraphEdgeCount=static_cast<uint32_t>(graphSnapshot.edges.size());
        CyberpunkVR_PreparationGraphFailure=graphSnapshot.failure;
        cvr::stereo::preparation::PublishGraphParentSource(graphSnapshot,reinterpret_cast<uintptr_t>(g_exe_base));
        if(valid){
            const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
            frontierPlan.BuildCandidate(graphSnapshot,base);
            auto* stats=CyberpunkVR_PreparationFrontierStats;
            std::fill_n(stats,16,0ull);std::fill_n(CyberpunkVR_PreparationFrontierRoles,22,0ull);
            std::fill_n(CyberpunkVR_PreparationFrontierCuts,6,0ull);std::fill_n(CyberpunkVR_PreparationFrontierAdds,6,0ull);
            stats[0]=UINT(frontierPlan.State());stats[1]=UINT(frontierPlan.Rejection());stats[2]=frontierPlan.ExecutionReady();stats[3]=frontierPlan.SourceFingerprint();
            stats[4]=graphSnapshot.nodes.size();stats[5]=frontierPlan.Original()[0].size();stats[6]=frontierPlan.Original()[1].size();
            stats[7]=frontierPlan.Proposed()[0].size();stats[8]=frontierPlan.Proposed()[1].size();stats[9]=frontierPlan.CpuBlockerPath().size();
            stats[10]=frontierPlan.OrderBlockerPath().size();stats[11]=frontierPlan.PeerCpuClosure().size();stats[12]=frontierPlan.CpuIncoming().size();
            stats[13]=frontierPlan.Removed()[1].size();stats[14]=frontierPlan.Added()[1].size();
            const auto& proof=frontierPlan.Proofs();const bool checks[]{proof.originalCpuAcyclic,proof.originalOrderAcyclic,proof.candidateOrderAcyclic,
                proof.cpuUnchanged,proof.unrelatedOrderEdgesPreserved,proof.bothProducersBeforeMainDiscard,proof.mainGeometryBeforeLighting,proof.peerGeometryBeforeLighting,
                proof.cpuProducerClosureProved,proof.resourceOrderCompiled,proof.parentPassMappingProved};
            for(UINT i=0;i<std::size(checks);++i)if(checks[i])stats[15]|=UINT64(1)<<i;
            for(UINT eye=0;eye<2;++eye){const auto& r=frontierPlan.EyeRoles()[eye];const uint64_t values[]{r.collector,r.depth,r.solidReady,r.discard,r.postGBuffer,r.lighting,r.final2D};
                std::copy_n(values,7,CyberpunkVR_PreparationFrontierRoles+eye*11);
                for(size_t i=0;i<std::min(size_t(4),r.solid.size());++i)CyberpunkVR_PreparationFrontierRoles[eye*11+7+i]=r.solid[i];}
            UINT index{};for(const auto& [from,to]:frontierPlan.Removed()[1]){if(index==3)break;CyberpunkVR_PreparationFrontierCuts[index*2]=from;CyberpunkVR_PreparationFrontierCuts[index++*2+1]=to;}
            index=0;for(const auto& [from,to]:frontierPlan.Added()[1]){if(index==3)break;CyberpunkVR_PreparationFrontierAdds[index*2]=from;CyberpunkVR_PreparationFrontierAdds[index++*2+1]=to;}
            try{CompareOwnedGraph();}catch(...){CyberpunkVR_PreparationOwnedGraphStats[0]=4;CyberpunkVR_PreparationOwnedGraphStats[14]=6;}
        }
        CyberpunkVR_PreparationGraphSeq.fetch_add(1);
        if(!captureMode || (CyberpunkVR_PreparationGraphJobsMode.load()==captureMode && CyberpunkVR_PreparationGraphJobsEpoch.load()==captureEpoch &&
            CyberpunkVR_PreparationPairGeneration.load()==captureGeneration))CyberpunkVR_PreparationGraphState.store(valid?2:3,std::memory_order_release);
        if(valid)cvr::stereo::definition_tracking::Match(graphSnapshot,reinterpret_cast<uintptr_t>(graph),graphCaptureOwner,captureEpoch,captureGeneration);
        {std::lock_guard lock(graphJobsMutex);SelectGraphJobsLocked();}
    }
    return declare();
}
CVR_DETOUR_IF("[single-pass] owned paired graph snapshot",0x547CF4,GraphHook,originalDeclare,GraphCompatible)

using QueueGraphNodes=uintptr_t(*)(void*,void*,void*);
QueueGraphNodes originalQueueGraph{};
bool ScheduleCompatible();
void RetainGraphJob(void* group){InterlockedIncrement(reinterpret_cast<volatile LONG*>(static_cast<uint8_t*>(group)+0x1C));}
void ReleaseGraphJob(void** group){reinterpret_cast<void(*)(void**)>(reinterpret_cast<uintptr_t>(g_exe_base)+0x2744CC)(group);}
bool CompleteGraphJob(void* group){return InterlockedCompareExchange(reinterpret_cast<volatile LONG*>(static_cast<uint8_t*>(group)+0x18),0,0)==0;}
void RetainGraphOwner(void* owner){InterlockedIncrement(reinterpret_cast<volatile LONG*>(static_cast<uint8_t*>(owner)+8));}
void ReleaseGraphOwner(void* owner){reinterpret_cast<void(*)(void*)>(reinterpret_cast<uintptr_t>(g_exe_base)+0x32DE14)(owner);}
bool GraphJobsCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
    static const bool valid=[] {
    auto* diagnostic=CyberpunkVR_PreparationGraphJobsCompatibility;diagnostic[0]=2;
    // The schedule contract includes this function's original body. Cache it
    // before installing our queue observer, not after its detour is enabled.
    diagnostic[2]=ScheduleCompatible();if(!diagnostic[2])return false;
#if CVR_RUST_VRIK
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);diagnostic[1]=base;
    if(!cvr_npf_graph_jobs(base,diagnostic+3))return false;
#else
    constexpr struct Code {UINT rva,bytes;uint64_t hash;} code[]{
        {0x32D60C,0x12C,0x9C49D739DEEFCCACull},{0x2744CC,0x4A,0xBF52C1744ED0577Cull},
        {0x272B40,0x183,0xFDFF35624D704435ull},{0x2746D8,0x156,0xA28DBE6ABEB41274ull},
        {0x272DB8,0xCA,0x68358540D95CCC78ull},{0x32DE14,0x15,0xE9AA994824C7FD8Bull},
        {0x333374,0x98,0x6B7C1293E9814833ull}};
    // Queue/destructor bodies are also audited in the existing collector path.
    std::array<uint8_t,0x200> bytes{};const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    diagnostic[1]=base;
    for(const auto& row:code){diagnostic[3]=row.rva;diagnostic[4]=row.hash;
        if(!Read(base+row.rva,bytes.data(),row.bytes))return false;
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<row.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        diagnostic[5]=hash;if(hash!=row.hash)return false;}
#endif
    diagnostic[0]=1;return true;
    }();
    if(valid)cvr::stereo::preparation::SetGraphJobPoll(PollOwnedGraphJobs);
    return valid;
}
uintptr_t QueueGraphNodesHook(void* owner,void* graph,void* groups){
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    const auto replay=cvr::stereo::definition_tracking::BeginResourceQueue(reinterpret_cast<uintptr_t>(owner),reinterpret_cast<uintptr_t>(graph));
    const auto result=originalQueueGraph(owner,graph,groups);
    cvr::stereo::definition_tracking::EndResourceQueue(replay);
    if(CyberpunkVR_PreparationGraphJobsMode.load()==1){auto* d=CyberpunkVR_PreparationGraphJobsCompatibility;
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(d+6));
        InterlockedExchange64(reinterpret_cast<volatile LONG64*>(d+7),
            reinterpret_cast<uintptr_t>(graph)==CyberpunkVR_PreparationGraphMeta[0]?CyberpunkVR_PreparationGraphState.load():10);}
    if(CyberpunkVR_PreparationGraphJobsMode.load()==1 && cvr::stereo::preparation::Active() &&
       CyberpunkVR_PreparationPairState.load()==2 && caller==reinterpret_cast<uintptr_t>(g_exe_base)+cvr::stereo::graph_jobs::QueueReturn){
        uint64_t epoch{},generation{};
        if(!cvr::stereo::definition_tracking::QueueIdentity(reinterpret_cast<uintptr_t>(owner),reinterpret_cast<uintptr_t>(graph),caller,epoch,generation))return result;
        std::lock_guard lock(graphJobsMutex);auto* stats=CyberpunkVR_PreparationGraphJobsStats;
        ArmGraphJobsLocked();if(stats[0]==2 || stats[0]==3 || stats[0]==4)return result;
        const auto current=[&]{return CyberpunkVR_PreparationGraphJobsMode.load()==1 && CyberpunkVR_PreparationGraphJobsEpoch.load()==epoch &&
            CyberpunkVR_PreparationPairGeneration.load()==generation;};
        const auto fail=[&](UINT reason){if(current()){stats[6]=reason;stats[0]=4;}};
        try{
            cvr::stereo::graph_snapshot::Snapshot source;
            if(!source.Capture(reinterpret_cast<uintptr_t>(graph),Read))return result;
            cvr::stereo::paired_frontier::Plan frontier;
            if(frontier.BuildCandidate(source,reinterpret_cast<uintptr_t>(g_exe_base))!=cvr::stereo::paired_frontier::Result::CandidateNeedsCpuAndResourceCompile)return result;
            if(!epoch || !current())return result;
            uint64_t frameOwner{};if(!Read(reinterpret_cast<uintptr_t>(owner),&frameOwner,8))return result;
            for(const auto& receipt:graphJobs)if(receipt->Match(epoch,generation,reinterpret_cast<uintptr_t>(g_exe_base),
                reinterpret_cast<uintptr_t>(graph),frameOwner,source))return result;
            if(graphJobs.size()>=cvr::stereo::graph_jobs::MaxPendingReceipts){fail(102);return result;}
            stats[0]=1;
            cvr::stereo::graph_jobs::CapturePlan plan;
            if(!plan.Capture(reinterpret_cast<uintptr_t>(owner),reinterpret_cast<uintptr_t>(graph),reinterpret_cast<uintptr_t>(groups),caller,
                reinterpret_cast<uintptr_t>(g_exe_base),source,Read)){
                if(current())std::copy_n(plan.Diagnostic().data(),8,CyberpunkVR_PreparationGraphJobsSourceReject);
                fail(UINT(plan.Rejection()));return result;}
            auto receipt=std::make_unique<cvr::stereo::graph_jobs::DeferredReceipt>(cvr::stereo::JobLeaseApi{RetainGraphJob,ReleaseGraphJob,CompleteGraphJob},
                cvr::stereo::graph_jobs::FrameOwnerApi{RetainGraphOwner,ReleaseGraphOwner});
            if(!receipt->Capture(plan,epoch,generation,reinterpret_cast<uintptr_t>(g_exe_base),Read)){fail(100);return result;}
            if(!current())return result;
            graphJobs.push_back(std::move(receipt));SelectGraphJobsLocked();
        }catch(...){fail(101);}
    }
    return result;
}
CVR_DETOUR_IF("[single-pass] retain complete native graph jobs",0x32D60C,QueueGraphNodesHook,originalQueueGraph,GraphJobsCompatible)

using CompileResourcePool=uintptr_t(*)(void*,uint8_t,void*);
CompileResourcePool originalCompilePool{};
bool ResourceUsesCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
#if CVR_RUST_VRIK
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    if(!cvr_npf_code(3,base))return false;
    const bool valid=cvr_npf_code(4,base);
#else
    std::array<uint8_t,0x1FF> bytes{};const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    if(!Read(base+0x3E0330,bytes.data(),bytes.size()))return false;
    uint64_t hash=14695981039346656037ull;for(const auto byte:bytes)hash=(hash^byte)*1099511628211ull;
    if(hash!=0xEBAB3552ECE2A243ull)return false;
    constexpr struct Code {UINT rva,bytes;uint64_t hash;} markers[]{
        {0x332D5C,0x234,0x9A47DEBAE35DF58Bull},{0xB29460,0x2A,0x152DF25F77AF9491ull},
        {0x655FAC,0xF2,0x2FCB059521D7B173ull},{0x338C04,0xAE,0xF3B76F14B0B8A294ull},
        {0x3EB51C,0xA0E,0xBBDE080A046F1FEFull}};  // 2.5 inlines 2.31's cold chunk 0x1E43FD8 into sub_1403EB51C
    std::array<uint8_t,0xB00> body{};bool valid=true;
    for(const auto& row:markers){if(!Read(base+row.rva,body.data(),row.bytes)){valid=false;break;}
        uint64_t actual=14695981039346656037ull;for(UINT i=0;i<row.bytes;++i)actual=(actual^body[i])*1099511628211ull;
        if(actual!=row.hash){valid=false;break;}}
#endif
    CyberpunkVR_PreparationResourceMarkerContract=valid?1:2;
    return true;
}
uintptr_t CompilePoolHook(void* pool,uint8_t mode,void* job){
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    const auto reset=cvr::stereo::definition_tracking::BeforeResourceReset(reinterpret_cast<uintptr_t>(pool),mode,caller);
    const auto owned=cvr::stereo::definition_tracking::BeforeOwnedResourceCompile(reinterpret_cast<uintptr_t>(pool),mode,reinterpret_cast<uintptr_t>(job),caller);
    const auto sortingMode=CyberpunkVR_PreparationOwnedGraphMode.load();
    bool captured=false;uint64_t capturedSequence{},capturedFingerprint{};uint32_t capturedEpoch{};
    if(mode==2 && caller==reinterpret_cast<uintptr_t>(g_exe_base)+0xBB68B5 &&
       (sortingMode==2 || sortingMode==3) && CyberpunkVR_PreparationGraphState.load()==2 && cvr::stereo::preparation::Active()){
        std::lock_guard captureLock(graphCaptureMutex);
        std::lock_guard lock(resourceUsesMutex);auto* stats=CyberpunkVR_PreparationResourceUsesStats;
        if(CyberpunkVR_PreparationGraphState.load()==2 && !stats[0] && reinterpret_cast<uintptr_t>(pool)==resourcePool){
            stats[0]=1;stats[1]=reinterpret_cast<uintptr_t>(pool);stats[2]=resourceWrapper;stats[3]=frontierPlan.SourceFingerprint();
            const auto reader=[](void*,uint64_t address,void* out,size_t bytes){return Read(address,out,bytes);};
            try{
                if(!resourceUses.CapturePool(reinterpret_cast<uintptr_t>(pool),graphSnapshot,reader,nullptr,CyberpunkVR_PreparationGraphMeta[2])){
                    stats[10]=UINT(resourceUses.failure);stats[0]=4;
                }else{
                    stats[4]=resourceUses.useIds.size();stats[5]=resourceUses.declarations.size();stats[6]=resourceUses.auxiliaryKeys.size();stats[7]=resourceUses.boundaryIds.size();
                    stats[8]=sizeof(cvr::stereo::resource_uses::Declaration);stats[9]=resourceUses.opaqueStateUnsupported;
                    CyberpunkVR_PreparationResourceUsesRows=reinterpret_cast<uintptr_t>(resourceUses.declarations.data());
                    CyberpunkVR_PreparationResourceUseIds=reinterpret_cast<uintptr_t>(resourceUses.useIds.data());
                    CyberpunkVR_PreparationResourceAuxKeys=reinterpret_cast<uintptr_t>(resourceUses.auxiliaryKeys.data());
                    CyberpunkVR_PreparationResourceBoundaryIds=reinterpret_cast<uintptr_t>(resourceUses.boundaryIds.data());stats[0]=2;
                    captured=true;capturedSequence=CyberpunkVR_PreparationGraphSeq.load();capturedEpoch=CyberpunkVR_PreparationGraphJobsEpoch.load();
                    capturedFingerprint=stats[3];
                }
            }catch(...){stats[10]=100;stats[0]=4;}
        }
    }
    const auto result=originalCompilePool(pool,mode,job);
    cvr::stereo::definition_tracking::AfterResourceReset(reset,reinterpret_cast<uintptr_t>(pool),mode);
    cvr::stereo::definition_tracking::AfterOwnedResourceCompile(owned,reinterpret_cast<uintptr_t>(pool));
    if(captured && mode==2 && CyberpunkVR_PreparationOwnedGraphMode.load()==sortingMode){std::lock_guard lock(resourceUsesMutex);
        if(CyberpunkVR_PreparationResourceUsesStats[0]==2 && reinterpret_cast<uintptr_t>(pool)==resourceUses.pool &&
            CyberpunkVR_PreparationGraphState.load()==2 && capturedSequence==CyberpunkVR_PreparationGraphSeq.load() &&
            capturedEpoch==CyberpunkVR_PreparationGraphJobsEpoch.load() && capturedFingerprint==CyberpunkVR_PreparationResourceUsesStats[3]){
            uint8_t after{};if(Read(reinterpret_cast<uintptr_t>(pool)+2,&after,1) && after==2){CyberpunkVR_PreparationResourceUsesStats[11]=1;CyberpunkVR_PreparationResourceUsesStats[0]=3;}
            else{CyberpunkVR_PreparationResourceUsesStats[0]=4;CyberpunkVR_PreparationResourceUsesStats[10]=101;}
        }}
    return result;
}
CVR_DETOUR_IF("[single-pass] exact pre-compiler resource uses",0x3E0330,CompilePoolHook,originalCompilePool,ResourceUsesCompatible)

using ProxyGather=uintptr_t(*)(void*,void*);
ProxyGather originalProxyGather{};
bool ProxyGatherCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
#if CVR_RUST_VRIK
    return cvr_npf_code(5,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
#else
    std::array<uint8_t,0x108> bytes{};
    if(!Read(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr))+0x33020C,bytes.data(),bytes.size()))return false;
    uint64_t hash=14695981039346656037ull;for(auto byte:bytes)hash=(hash^byte)*1099511628211ull;
    return hash==0xDED60A539B29A3F2ull;
#endif
}
uintptr_t ProxyGatherHook(void* context,void* range){
    cvr::stereo::preparation::MaterializeTicket ticket;
    if(cvr::stereo::preparation::Active()){
        uint64_t view{},name{};int side=-1;
        if(Read(reinterpret_cast<uintptr_t>(context)+0x18,&view,8) && view && Read(view+0x28,&name,8)){
            if(!name)side=0;else if(name==g_vrcam_ctx_key.load())side=1;
        }
        ticket=cvr::stereo::preparation::BeginProxyGather(context,range,side);
    }
    const auto result=originalProxyGather(context,range);
    cvr::stereo::preparation::EndProxyGather(ticket);return result;
}
CVR_DETOUR_IF("[single-pass] native proxy preparation inputs",0x33020C,ProxyGatherHook,originalProxyGather,ProxyGatherCompatible)

using CreateDependencies=uintptr_t(*)(void*,void*,void*);
CreateDependencies originalDependencies{};
std::mutex scheduleMutex;
cvr::stereo::preparation_schedule::PlanCache scheduleCache;
std::shared_ptr<const cvr::stereo::preparation_schedule::PreparedPlan> publishedSchedule;
uint64_t ScheduleNow(){LARGE_INTEGER now{};QueryPerformanceCounter(&now);return now.QuadPart;}
void ScheduleTime(unsigned phase,uint64_t ticks){
    ++CyberpunkVR_PreparationScheduleCpu[phase*2];CyberpunkVR_PreparationScheduleCpu[phase*2+1]+=ticks;
}
bool ScheduleCompatible(){
    if(!GetEnvironmentVariableW(L"CVR_STEREO_PAIRED_ROOT",nullptr,0))return false;
    static const bool valid=[] {
#if CVR_RUST_VRIK
    const bool matched=cvr_npf_code(6,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
#else
    struct Region {uintptr_t offset;size_t size;uint64_t hash;};
    constexpr Region regions[]{{0x8EA1C8,0x132,0xC35ED8EB6A3F9C5Dull},{0x32D60C,0x12C,0x9C49D739DEEFCCACull},
        {0x3E8F58,0x199,0x80DA43519D13BA70ull},{0x40B10C,0x82,0x177EF1EE2846E667ull}};
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));bool matched=true;
    for(const auto& region:regions){std::array<uint8_t,0x200> bytes{};matched=matched && Read(base+region.offset,bytes.data(),region.size);
        uint64_t hash=14695981039346656037ull;for(size_t i=0;i<region.size;++i)hash=(hash^bytes[i])*1099511628211ull;
        matched=matched && hash==region.hash;}
#endif
    LARGE_INTEGER frequency{};QueryPerformanceFrequency(&frequency);CyberpunkVR_PreparationScheduleCpuFrequency=frequency.QuadPart;
    return matched;
    }();CyberpunkVR_PreparationScheduleCompatible=valid?1:2;return valid;
}
uintptr_t DependenciesHook(void* graph,void* groups,void* parent){
    const auto mode=CyberpunkVR_PreparationScheduleMode.load(std::memory_order_acquire);
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    if((mode!=1 && mode!=2) || !cvr::stereo::preparation::Pairing() || CyberpunkVR_PreparationPairState.load()!=2 ||
        reinterpret_cast<uintptr_t>(_ReturnAddress())!=base+0x3E9050)return originalDependencies(graph,groups,parent);
    cvr::stereo::graph_snapshot::Snapshot snapshot;
    std::shared_ptr<const cvr::stereo::preparation_schedule::PreparedPlan> selected;
    bool apply=false;
    {
        std::lock_guard lock(scheduleMutex);
        CyberpunkVR_PreparationScheduleSeq.fetch_add(1);
        auto* stats=CyberpunkVR_PreparationScheduleStats;
        uint32_t failure{};bool eligible=true;const auto captureBegin=ScheduleNow();uint64_t captureTicks{};
        try{
            const bool captured=snapshot.Capture(reinterpret_cast<uintptr_t>(graph),Read);captureTicks=ScheduleNow()-captureBegin;
            if(!captured)failure=10+snapshot.failure;
            else if(std::none_of(snapshot.nodes.begin(),snapshot.nodes.end(),[](const auto& n){return cvr::stereo::graph_snapshot::Field<uint32_t>(n.bytes,24)==1;})){
                eligible=false;
            }
            else{
                const auto planBegin=ScheduleNow();
                std::array<uint64_t,4> ranges{};
                const auto main=CyberpunkVR_PreparationPairData[4],eye=CyberpunkVR_PreparationPairData[3];
                const bool inputs=Read(main+0x2C40,ranges.data(),16) && Read(eye+0x2C40,ranges.data()+2,16) &&
                    ranges[0]==CyberpunkVR_PreparationPairData[6] && ranges[2]==CyberpunkVR_PreparationPairData[5];
                std::copy(ranges.begin(),ranges.end(),CyberpunkVR_PreparationScheduleRanges);
                if(!inputs || !cvr::stereo::preparation_schedule::Independent(ranges))failure=20;
                else{
                    bool hit{};selected=scheduleCache.Acquire(snapshot,base,CyberpunkVR_PreparationScheduleCache.load()!=0,hit);
                    if(!selected)failure=30+scheduleCache.failure;
                    else{
                        const auto& plan=selected->plan;publishedSchedule=selected;
                        CyberpunkVR_PreparationScheduleEdges=reinterpret_cast<uintptr_t>(selected->edges.data());
                        stats[5]=static_cast<uint32_t>(snapshot.nodes.size());
                        stats[6]=static_cast<uint32_t>(std::count_if(snapshot.edges.begin(),snapshot.edges.end(),[](const auto& e){return !cvr::stereo::graph_snapshot::Field<uint32_t>(e.bytes,4);}));
                        stats[7]=static_cast<uint32_t>(selected->edges.size());stats[8]=static_cast<uint32_t>(plan.early.size());
                        stats[9]=static_cast<uint32_t>(plan.ready.size());stats[10]=static_cast<uint32_t>(plan.barriers.size());stats[11]=static_cast<uint32_t>(plan.frontier.size());
                        ++stats[1];++stats[hit?12:13];
                        if(mode==2){apply=true;++stats[2];}
                    }
                }
                ScheduleTime(1,ScheduleNow()-planBegin);
            }
        }catch(...){failure=40;}
        if(eligible){++stats[0];ScheduleTime(0,captureTicks);}if(failure){++stats[3];stats[4]=failure;}
        CyberpunkVR_PreparationScheduleSeq.fetch_add(1);
    }
    // Retain this exact immutable shadow through the synchronous native call.
    const auto nativeBegin=ScheduleNow();
    const auto result=originalDependencies(apply?const_cast<uint8_t*>(selected->jobs.header.data()):graph,groups,parent);
    if(apply)ScheduleTime(2,ScheduleNow()-nativeBegin);
    return result;
}
CVR_DETOUR_IF("[single-pass] bounded CPU preparation schedule",0x8EA1C8,DependenciesHook,originalDependencies,ScheduleCompatible)
}
}
namespace cvr::stereo::preparation {
void PrepareGraphJobCodeGuards(){cvr::detail::GraphJobsCompatible();}
}
#endif
