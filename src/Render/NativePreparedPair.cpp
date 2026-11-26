#include "Render/NativePreparedPair.hpp"
#include "Render/GpuResidencyLease.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/StereoGeometryCommands.hpp"
#include "Render/StereoGeometryChain.hpp"
#include "Render/StereoMeshPlan.hpp"
#include "Render/GpuGeometryCompare.hpp"
#include "Render/GpuFinerVrs.hpp"
#include "Render/StereoVrsSnapshot.hpp"
#include "Render/SourceDescriptorViews.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeOutputCopy.hpp"
#include "Render/PreparedMembershipBank.hpp"
#include <windows.h>
#include <algorithm>
#include <map>
#include <mutex>
#include <set>
#include <intrin.h>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The orchestration is Rust's (rust/crates/cvr-render/src/prepared_pair_native.rs: the work, the pairing, the
// banks, the recording, the submissions, the readbacks, the membership banks); this half keeps the sources (lent as
// views) and lends their tables' SRV coverage receipts and the export's targets.
#include "Render/NativeIndirectPolicy.hpp"
extern "C" bool cvr_rl_same_owner(ID3D12Pageable*,ID3D12Pageable*);   // residency_lease.rs
extern "C" bool cvr_npp_full_t5(uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t);
namespace cvr::detail {
using PrivateExecuteFn=void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
PrivateExecuteFn command_queue_original_execute(ID3D12CommandQueue*);
}
namespace cvr::stereo::prepared_pair {
namespace {
struct CvrLease {ID3D12Resource* resource;uint64_t gpu,bytes;};
struct CvrResources {CvrLease index;std::array<CvrLease,16> vertices;const native_probe::DrawRecord* draw;const uint8_t* instances;size_t instanceCount;};
struct CvrState {ID3D12PipelineState* pipeline;ID3D12Resource* rateImage;scene_state::Bindings::Rate rate;int32_t topology;uint32_t stencil;float blend[4];};
struct CvrTable {UINT parameter,offset,count;ID3D12DescriptorHeap* cpuHeap;IUnknown* const* leases;size_t leaseCount;uint64_t identity,version,storageScope;
    UINT storageExtent;};
struct CvrMaterial {UINT parameter,offset,kind;bool hasBuffer;descriptors::DescriptorStamp stamp;ID3D12Resource* resource;ID3D12Resource* counter;
    IUnknown* immutableOwner;ID3D12Resource* immutableResource;IUnknown* bufferOwner;ID3D12Resource* bufferResource;D3D12_SHADER_RESOURCE_VIEW_DESC bufferView;
    const texture_read::ImmutableLease* immutable;const descriptors::SourceResourceBinding* binding;const material_buffer_snapshot::Copy* buffer;};
struct CvrInput {ID3D12Resource* resource;IUnknown* owner;uint64_t offset;const uint8_t* expected;size_t expectedLength;};
struct CvrExit {const void* identity;ID3D12Resource* readback;UINT width,height,eye,sourceSide,frame;uint64_t list,generation;const NativeExitPlane* planes;
    size_t planeCount;};
struct CvrSource {
    uint64_t ordinal;const native_recording::Lease* recording;ID3D12CommandQueue* sourceQueue;CvrResources owned;CvrState ownedState;
    const native_probe::DrawRecord* originalDraw;ID3D12PipelineState* originalPipeline;ID3D12RootSignature* root;const void* shader;
    const uint8_t* shaderRoot;size_t shaderRootLength;const source_cbv_snapshot::PrivateCbv* cbvs;size_t cbvCount;
    const source_cbv_snapshot::PreparedCopies* cbvReceipt;const CvrTable* tables;size_t tableCount;const CvrTable* referenceTables;
    size_t referenceTableCount;const CvrMaterial* materials;size_t materialCount;const CvrInput* inputs;size_t inputCount;
    const source_binding_assembly::GuardedT5View* t5;const Source::FullT5* fullT5;ID3D12Resource* rateImage;const void* baseline;
    bool baselineCaptured,allowVrsRefinement;scene_state::detail::GeometryTargetsRaw baselineTargets;const CvrExit* nativeExit;
    ID3D12Resource* passDepth;void* rateCapture;IUnknown* const* owners;size_t ownerCount;
};
struct CvrPlan {const uint8_t* serialized;size_t serializedLength;const PairedBindingLocation* bindings;size_t bindingCount;
    std::array<UINT,2> parameters,descriptors;UINT originalParameters,rootDwords;};
struct CvrFamily {ID3D12PipelineState* pipeline;ID3D12RootSignature* sourceRoot;ID3D12RootSignature* root;CvrPlan plan;const families::ResourceUse* uses;
    size_t useCount;D3D12_DEPTH_STENCIL_DESC depthState;UINT renderTargets;bool pairedRoot,instanceRebaseSafe,depthStateKnown;};
static_assert(sizeof(CvrResources)==432 && sizeof(CvrState)==72 && sizeof(CvrTable)==72 && sizeof(CvrMaterial)==304 && sizeof(CvrInput)==40 &&
    sizeof(CvrExit)==72 && sizeof(CvrSource)==1072 && offsetof(CvrSource,baselineTargets)==704 && offsetof(CvrSource,nativeExit)==1032 &&
    sizeof(CvrPlan)==56 && sizeof(CvrFamily)==160 && offsetof(CvrFamily,depthState)==96 &&
    sizeof(source_cbv_snapshot::PrivateCbv)==72 && sizeof(source_cbv_snapshot::PreparedCopies)==48 && sizeof(source_binding_assembly::GuardedT5View)==72 &&
    sizeof(Source::FullT5)==152 && offsetof(Source::FullT5,range)==72 && sizeof(structured_buffer_snapshot::PreparedCopy)==72 &&
    sizeof(native_recording::Lease)==40 && sizeof(NativeExitPlane)==56 && sizeof(NativeOutputCopy::RustTarget)==56 &&
    sizeof(Microsoft::WRL::ComPtr<IUnknown>)==sizeof(IUnknown*) && std::is_standard_layout_v<StereoVrsSnapshot> && sizeof(StereoVrsSnapshot)==8,
    "mirrored by prepared_pair_native.rs");
// The sources and the C++ objects of one work: the Rust work owns it.
struct Aux {
    std::vector<Source> sources;
    std::vector<CvrSource> views;
    std::vector<CvrExit> exits;
    std::vector<std::vector<CvrTable>> tables,referenceTables;
    std::vector<std::vector<std::vector<IUnknown*>>> tableLeases,referenceLeases;
    std::vector<std::vector<CvrMaterial>> materials;
    std::vector<std::vector<CvrInput>> inputs;   // the immutable uses: prepared_pair_native.rs'
    // The residency leases: prepared_pair_native.rs'; the export targets' borrowed pageables stay here for them.
    std::array<gpu::residency::BorrowedTarget,8> exportTargets;std::array<target_lifetime::BorrowedPageable,8> exportPageables;
    target_access::Receipt exportReceipt;
    std::array<target_lifetime::BorrowedScope,8> exportLifetimes;
};
CvrTable LendTable(const SharedBankTable& table,std::vector<IUnknown*>& leases){
    leases.clear();for(const auto& lease:table.leases)leases.push_back(lease.Get());
    return {table.parameter,table.offset,table.count,table.cpuHeap.Get(),leases.data(),leases.size(),table.identity,table.version,table.storageScope,
        table.storageExtent};
}
void LendTables(const std::vector<SharedBankTable>& source,std::vector<CvrTable>& rows,std::vector<std::vector<IUnknown*>>& leases){
    leases.resize(source.size());rows.clear();rows.reserve(source.size());
    for(size_t i=0;i<source.size();++i)rows.push_back(LendTable(source[i],leases[i]));
}
void Lend(Aux& a){
    const auto n=a.sources.size();
    a.views.assign(n,CvrSource{});a.exits.assign(n,CvrExit{});a.tables.resize(n);a.referenceTables.resize(n);a.tableLeases.resize(n);
    a.referenceLeases.resize(n);a.materials.resize(n);a.inputs.resize(n);
    for(size_t i=0;i<n;++i){const auto& s=a.sources[i];auto& v=a.views[i];
        v.ordinal=s.ordinal;v.recording=&s.sourceRecording;v.sourceQueue=s.sourceQueue.Get();
        const auto lease=[](const geometry::BufferLease& value){return CvrLease{value.resource.Get(),value.gpu,value.bytes};};
        v.owned.index=lease(s.owned.index);for(UINT k=0;k<16;++k)v.owned.vertices[k]=lease(s.owned.vertices[k]);
        v.owned.draw=&s.owned.draw;v.owned.instances=s.owned.instances.data();v.owned.instanceCount=s.owned.instances.size();
        const auto& state=s.owned.state;
        v.ownedState={state.original.Get(),state.rateImage.Get(),state.rate,int32_t(state.topology),state.stencil,
            {state.blendFactor[0],state.blendFactor[1],state.blendFactor[2],state.blendFactor[3]}};
        v.originalDraw=&s.original.draw;v.originalPipeline=s.original.state.original.Get();v.root=s.root.Get();v.shader=s.shader.get();
        if(s.shader){v.shaderRoot=s.shader->root.data();v.shaderRootLength=s.shader->root.size();}
        v.cbvs=s.cbvs.data();v.cbvCount=s.cbvs.size();v.cbvReceipt=&s.cbvReceipt;
        LendTables(s.tables,a.tables[i],a.tableLeases[i]);v.tables=a.tables[i].data();v.tableCount=a.tables[i].size();
        LendTables(s.referenceTables,a.referenceTables[i],a.referenceLeases[i]);
        v.referenceTables=a.referenceTables[i].data();v.referenceTableCount=a.referenceTables[i].size();
        auto& materials=a.materials[i];materials.reserve(s.materials.size());
        for(const auto& m:s.materials){const auto& b=m.binding;
            materials.push_back({b.parameter,b.offset,UINT(b.type),bool(m.buffer),b.stamp,b.resource.Get(),b.counter.Get(),m.immutable.owner.Get(),
                m.immutable.source.resource.Get(),m.buffer?m.buffer->owner.Get():nullptr,m.buffer?m.buffer->resource.Get():nullptr,
                m.buffer?m.buffer->view:D3D12_SHADER_RESOURCE_VIEW_DESC{},&m.immutable,&b,m.buffer?&*m.buffer:nullptr});}
        v.materials=materials.data();v.materialCount=materials.size();
        auto& inputs=a.inputs[i];inputs.reserve(s.inputChecks.size());
        for(const auto& check:s.inputChecks)inputs.push_back({check.resource.Get(),check.owner.Get(),check.offset,check.expected.data(),check.expected.size()});
        v.inputs=inputs.data();v.inputCount=inputs.size();
        v.t5=s.t5?&*s.t5:nullptr;v.fullT5=s.fullT5?&*s.fullT5:nullptr;v.rateImage=s.rateImage.Get();v.baseline=s.passBaseline.get();
        v.baselineCaptured=s.passBaseline && s.passBaseline->Captured();v.allowVrsRefinement=s.allowVrsRefinement;
        if(v.baselineCaptured){const auto& targets=s.passBaseline->Targets();
            for(UINT k=0;k<5;++k){v.baselineTargets.resources[k]=targets.resources[k].Get();v.baselineTargets.views[k]=targets.views[k];
                v.baselineTargets.handles[k]=targets.handles[k];}
            v.baselineTargets.colors=targets.colors;}
        if(s.nativeExit){const auto& x=*s.nativeExit;
            a.exits[i]={s.nativeExit.get(),x.readback.Get(),x.width,x.height,x.eye,x.sourceSide,x.frame,uint64_t(x.list),x.generation,x.planes.data(),
                x.planes.size()};
            v.nativeExit=&a.exits[i];}
        v.passDepth=s.passDepth.Get();v.rateCapture=s.rateCapture?*reinterpret_cast<void* const*>(s.rateCapture.get()):nullptr;
        v.owners=reinterpret_cast<IUnknown* const*>(s.owners.data());v.ownerCount=s.owners.size();
    }
}
Aux* Make(std::vector<Source> sources){auto aux=std::make_unique<Aux>();aux->sources=std::move(sources);Lend(*aux);return aux.release();}
// The families, the signature receipts, the original Execute and the submission order: prepared_pair_native.rs.
struct Host {
    void(*auxFree)(void*);
    bool(*coverage)(void*,size_t,uint32_t,size_t,const source_binding_assembly::Scope*,uint32_t,const descriptors::DescriptorStamp*);
    ID3D12Pageable*(*targetPageable)(void*,uint32_t,ID3D12Device*);bool(*targetReauth)(void*,uint32_t,ID3D12Device*);
    uint32_t(*exportDescribe)(void*,ID3D12Device*,const void*,const void*,ID3D12CommandQueue*,uint32_t,const uint64_t*,NativeOutputCopy::RustTarget(*)[8]);
    bool(*exportValidate)(void*,ID3D12CommandQueue*,uint32_t,bool*);
    void*(*exportNew)();
};
const Host kHost{
    [](void* aux){delete static_cast<Aux*>(aux);},
    // The material checks, the full T5 view, the membership table and banks: prepared_pair_native.rs.
    [](void* aux,size_t index,uint32_t reference,size_t tableIndex,const source_binding_assembly::Scope* scope,uint32_t offset,
       const descriptors::DescriptorStamp* stamp){
        const auto& source=static_cast<Aux*>(aux)->sources[index];const auto& tables=reference?source.referenceTables:source.tables;
        try{if(tableIndex>=tables.size())return false;source_binding_assembly::OwnedSrvCoverageView coverage;
            return stamp?source_binding_assembly::MatchesOwnedSrvCoverage(tables[tableIndex],*scope,offset,*stamp,coverage):
                source_binding_assembly::DescribeOwnedSrvCoverage(tables[tableIndex],*scope,coverage);}
        catch(...){return false;}},
    [](void* aux,uint32_t index,ID3D12Device* device)->ID3D12Pageable*{
        auto& a=*static_cast<Aux*>(aux);
        try{const auto& target=a.exportTargets[index];auto& kept=a.exportPageables[index];
            return target_lifetime::GetBorrowedPageable(device,target.identity,target.lifetime,kept)?kept.pageable.Get():nullptr;}
        catch(...){return nullptr;}},
    [](void* aux,uint32_t index,ID3D12Device* device){
        const auto& kept=static_cast<Aux*>(aux)->exportPageables[index];
        try{target_lifetime::BorrowedPageable now;
            return target_lifetime::GetBorrowedPageable(device,kept.identity,kept.lifetime,now) && cvr_rl_same_owner(now.pageable.Get(),kept.pageable.Get());}
        catch(...){return false;}},
    // The immutable uses: prepared_pair_native.rs.
    [](void* aux,ID3D12Device* device,const void* receipt,const void* lifetimes,ID3D12CommandQueue* queue,uint32_t frame,const uint64_t* exits,
       NativeOutputCopy::RustTarget(*rows)[8])->uint32_t{
        auto& a=*static_cast<Aux*>(aux);
        try{const auto& r=*static_cast<const target_access::Receipt*>(receipt);
            const auto& l=*static_cast<const std::array<target_lifetime::BorrowedScope,8>*>(lifetimes);
            a.exportReceipt=r;a.exportLifetimes=l;
            std::array<target_handoff::SourceSnapshot,2> targets;
            if(!target_access::Describe(r,queue,frame,targets))return 2;
            std::array<gpu::residency::BorrowedTarget,8> resident;
            for(UINT side=0;side<2;++side){const auto& source=targets[side];const UINT other=1-side;
                if(source.side!=side || source.frame!=frame || source.recording.list!=exits[other*2] ||
                    source.recording.generation!=exits[other*2+1])return 3;
                for(UINT i=0;i<4;++i){const auto& target=source.targets[i];
                    if(target.slot!=(i==3?4:i) || !target_lifetime::Validate(l[side*4+i]))return 4;
                    const auto& c=target.identity.creation;
                    (*rows)[side*4+i]={c.kind,c.resource.Get(),c.heap.Get(),c.heapOffset,c.allocationBytes,target.planes,
                        {target.finalStates[0],target.finalStates[1]}};
                    resident[side*4+i]={target.identity,l[side*4+i]};
                }
            }
            a.exportTargets=resident;return 0;}   // their residency: prepared_pair_native.rs (targetPageable)
        catch(...){return 10;}},
    [](void* aux,ID3D12CommandQueue* queue,uint32_t frame,bool* live){
        auto& a=*static_cast<Aux*>(aux);
        try{const bool valid=target_access::Validate(a.exportReceipt,queue,frame);bool alive=true;
            for(const auto& lifetime:a.exportLifetimes)alive=target_lifetime::Validate(lifetime) && alive;*live=alive;return valid;}
        catch(...){*live=false;return false;}},
    // The export-only holder of a Rust caller's sources (pair_source.rs).
    []()->void*{try{return new Aux();}catch(...){return nullptr;}}
};
}
}
extern "C" {
void cvr_npp2_attach(const void*);
bool cvr_npp2_start(ID3D12Device*,void*,const void*,size_t,bool,void*);
bool cvr_npp2_start_current(ID3D12Device*,void*,const void*,size_t,ID3D12CommandQueue*,uint32_t);
uint32_t cvr_npp2_compatibility(const void*,const void*);
bool cvr_npp2_wants_native_export(uint32_t);
void cvr_npp2_cancel_current(uint32_t);
bool cvr_npp2_prepare_export(ID3D12CommandQueue*,uint32_t,const void*,const void*);
bool cvr_npp2_test_binding_modes();
bool cvr_npp2_test_released_reset_race();
}
namespace cvr::stereo::prepared_pair {
namespace {[[maybe_unused]] const bool kAttached=(cvr_npp2_attach(&kHost),true);}
uint32_t CompatibilityFailure(const Source& a,const Source& b){
    std::unique_ptr<Aux> lent(Make({a,b}));return cvr_npp2_compatibility(&lent->views[0],&lent->views[1]);
}
bool Start(ID3D12Device* device,std::array<Source,2> sources,std::shared_ptr<const families::Family> family)try{
    std::vector<Source> input;input.reserve(2);for(auto& source:sources)input.push_back(std::move(source));
    auto* aux=Make(std::move(input));
    // The family's lookup key: the left draw's pipeline (prepared_pair_native.rs looks it up in families_native.rs).
    return cvr_npp2_start(device,aux,aux->views.data(),aux->views.size(),false,family?aux->sources[0].owned.state.original.Get():nullptr);
}catch(...){return false;}
bool StartSequence(ID3D12Device* device,std::vector<Source> sources)try{
    auto* aux=Make(std::move(sources));return cvr_npp2_start(device,aux,aux->views.data(),aux->views.size(),true,nullptr);
}catch(...){return false;}
bool StartCurrentSequence(ID3D12Device* device,std::vector<Source> sources,ID3D12CommandQueue* queue,UINT frame)try{
    auto* aux=Make(std::move(sources));return cvr_npp2_start_current(device,aux,aux->views.data(),aux->views.size(),queue,frame);
}catch(...){return false;}
bool WantsNativeExport(UINT frame){return cvr_npp2_wants_native_export(frame);}
void CancelCurrent(UINT frame){cvr_npp2_cancel_current(frame);}
bool PrepareNativeExport(ID3D12CommandQueue* queue,UINT frame,const target_access::Receipt& receipt,
                         const std::array<target_lifetime::BorrowedScope,8>& lifetimes){
    return cvr_npp2_prepare_export(queue,frame,&receipt,&lifetimes);
}
#ifdef CVR_PREPARED_PAIR_TESTING
bool TestNativeBindingModes(){return cvr_npp2_test_binding_modes();}
bool TestReleasedResetRace(){return cvr_npp2_test_released_reset_race();}
#endif
}
#endif
#else
#ifdef CVR_PREPARED_PAIR_TESTING
#include <thread>
#endif

namespace cvr::detail {
using PrivateExecuteFn=void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
PrivateExecuteFn command_queue_original_execute(ID3D12CommandQueue*);
}

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedPairRequest{},CyberpunkVR_PreparedPairState{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedPairSelection{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedPairSelectionCount{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedPairInputFault{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedNativeExportEnable{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PreparedActualMembershipEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedActualMembershipStats[24]{},CyberpunkVR_PreparedActualMembershipRows[32*512]{};
__declspec(dllexport) uint32_t CyberpunkVR_PreparedActualMembershipCount{},CyberpunkVR_PreparedActualMembershipRowBytes=32*sizeof(uint64_t);
__declspec(dllexport) uint64_t CyberpunkVR_PreparedNativeExport[24]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairCandidates[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairStateMismatch[24]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairVrs[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairVrsGpu[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairGpu[36]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairColor[12]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairDepthState[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairMeshStats[12]{},CyberpunkVR_PreparedPairMeshResults[512]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairInputStats[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedBatchStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedBatchSources[1024]{},CyberpunkVR_PreparedBatchPlan[4096]{};
__declspec(dllexport) uint32_t CyberpunkVR_PreparedBatchSourceCount{},CyberpunkVR_PreparedBatchPlanCount{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedBatchDescriptors[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedPairResidency[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedCurrent[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedBaseline[24]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedNativeExit[24]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedNativeExitSamples[160]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedNativeRate[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedVrsAttribution[12]{};
__declspec(dllexport) uint64_t CyberpunkVR_PreparedNativeAcceptance[8]{};
}
namespace cvr::stereo::prepared_pair {
using Microsoft::WRL::ComPtr;
namespace {
struct Plane {UINT eye{},slot{},plane{},rows{},stride{},component{},bytes{};std::array<D3D12_PLACED_SUBRESOURCE_FOOTPRINT,4> layouts;};
struct Pair {
    std::array<UINT,2> sources{};std::shared_ptr<const families::Family> family;
    std::vector<BankPair> bindings;std::vector<SharedBankTable> shared;UINT command=UINT_MAX;bool ready{};
    bool actualBindings{};
};
struct Work {
    ~Work(){if(ownedList)gpu::ResetCommandResources(ownedList.Get());
        if(exportList)gpu::ResetCommandResources(exportList.Get());
        if(residency.Status()==gpu::residency::State::Acquired)residency.Cancel();
        if(targetResidency.Status()==gpu::residency::State::Acquired)targetResidency.Cancel();}
    ComPtr<ID3D12Device> device;std::vector<Source> sources;
    std::array<std::vector<UINT>,2> eyes;std::vector<std::unique_ptr<Pair>> pairs;std::vector<UINT> pairForSource;MeshPlan plan;bool sequence{};
    GpuGeometryCompare mesh;ComPtr<ID3D12Resource> imageReadback,predicate;
    GpuGeometryCompare inputs;ComPtr<ID3D12Resource> expectedInputs;
    bool actualMembership{},membershipFault{};
    std::vector<prepared_membership::Bank> membershipBanks;
    GpuMultilayerMembership membership;
    ComPtr<ID3D12Resource> membershipReadback;
    GpuFinerVrs rates;
    std::vector<uint8_t> commonRates;std::array<std::vector<uint8_t>,2> originalRates;
    UINT rateTile{},rateWidth{},rateHeight{};bool rateMapsReady{};
    ComPtr<ID3D12QueryHeap> pipelineQueries,occlusionQueries;ComPtr<ID3D12Resource> queryReadback;
    uint64_t meshBytes{},imageBytes{};std::vector<Plane> planes;
    std::array<std::shared_ptr<const NativeExit>,2> nativeExits;
    bool allowVrsRefinement{};
    StereoGeometryCommands referenceGeometry;StereoBindingBank referenceBank,pairedBank;
    StereoGeometryChain referenceTargets,pairedTargets,nativeRateTargets;UINT colors{},width{},height{};
    native_recording::Lease recording;ComPtr<ID3D12Fence> fence;ComPtr<ID3D12CommandQueue> queue;
    std::vector<texture_read::ImmutableUse> uses;bool submitted{},failed{},complete{};UINT stage=3;
    gpu::residency::Lease residency;
    gpu::residency::Lease targetResidency;
    bool exportRequested{},exportPrepared{},exportSubmitted{};
    target_access::Receipt exportReceipt;
    std::array<target_lifetime::BorrowedScope,8> exportLifetimes;
    std::unique_ptr<NativeOutputCopy> outputCopy;
    ComPtr<ID3D12CommandAllocator> exportAllocator;
    ComPtr<ID3D12GraphicsCommandList> exportList;
    bool current{},closed{},executionIssued{};UINT sourceFrame{};uint64_t ownedSerial{};
    ComPtr<ID3D12CommandAllocator> ownedAllocator;ComPtr<ID3D12GraphicsCommandList> ownedList;
    gpu::ResourceSubmission ownedFlight;Work* quarantinedNext{};
};
constexpr UINT QueryStride=sizeof(D3D12_QUERY_DATA_PIPELINE_STATISTICS)+sizeof(uint64_t);
static_assert(QueryStride==12*sizeof(uint64_t));
constexpr UINT PredicateReadbackOffset=3*QueryStride;
constexpr UINT MeshReadbackOffset=PredicateReadbackOffset+8;
constexpr UINT InputReadbackOffset=MeshReadbackOffset+256*16;
std::mutex mutex;std::unique_ptr<Work> work;std::atomic<bool> active{};
uint64_t nextOwnedSerial{};Work* quarantinedWork{};
UINT RecordCommands(Work&,ID3D12GraphicsCommandList*,ID3D12GraphicsCommandList1*,ID3D12GraphicsCommandList5*);
bool RecordCurrent();
#ifdef CVR_PREPARED_PAIR_TESTING
std::atomic<bool> testResetEntered{};
#endif
void CancelUnsubmittedResidency(Work& w){
    if(w.targetResidency.Status()==gpu::residency::State::Acquired)CyberpunkVR_PreparedNativeExport[19]=UINT(w.targetResidency.Cancel());
    else if(w.targetResidency.Status()==gpu::residency::State::Submitted)w.targetResidency.Quarantine();
    if(!w.residency.SourceCount())return;
    if(w.residency.Status()==gpu::residency::State::Acquired)CyberpunkVR_PreparedPairResidency[6]=UINT(w.residency.Cancel());
    else if(w.residency.Status()==gpu::residency::State::Submitted)w.residency.Quarantine();
    CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());CyberpunkVR_PreparedPairResidency[7]=gpu::residency::QuarantinedCount();
}
void Error(UINT reason){if(!CyberpunkVR_PreparedPairStats[10])CyberpunkVR_PreparedPairStats[10]=reason;
    if(work){work->failed=true;if(work->stage==3)CancelUnsubmittedResidency(*work);}}
void Publish(){if(!work)return;CyberpunkVR_PreparedPairStats[11]=work->stage;
    CyberpunkVR_PreparedCurrent[12]=CyberpunkVR_PreparedPairStats[10];
    CyberpunkVR_PreparedPairState.store(work->complete?(work->failed?6:5):work->stage,std::memory_order_release);}
void FailCurrent(UINT reason){
    if(!work || !work->current)return;Error(reason);work->complete=true;active=false;
    if(work->executionIssued){work->residency.Quarantine();
        work->targetResidency.Quarantine();
        CyberpunkVR_PreparedPairResidency[2]=UINT(work->residency.Status());CyberpunkVR_PreparedPairResidency[7]=gpu::residency::QuarantinedCount();
        Publish();work->quarantinedNext=quarantinedWork;quarantinedWork=work.release();
    }else{CancelUnsubmittedResidency(*work);Publish();work.reset();}
}
bool MaterialMatches(const Work& w,const Source& source,const Material& material){
    return material.buffer && (w.current?
        material_buffer_snapshot::MatchesStagedPrivate(*material.buffer,material.binding,source.sourceRecording,source.sourceQueue.Get()):
        material_buffer_snapshot::MatchesPrivate(*material.buffer,material.binding));
}
bool HasBaseline(const Source& source){
    if(!source.passBaseline || !source.passBaseline->Captured() || source.passBaseline->Targets().colors!=3)return false;
    const auto& baseline=source.passBaseline->Targets();
    if(source.passDepth && source.passDepth!=baseline.resources[4])return false;
    for(UINT slot:{0u,1u,2u,4u})if(!baseline.resources[slot])return false;return true;
}
bool NativeExitMatchesSource(const Work& w,const Source& source,UINT eye){
    const auto& exit=*source.nativeExit;
    return exit.width==w.width && exit.height==w.height && exit.eye==eye && exit.sourceSide==1-eye &&
        exit.frame==w.sourceFrame && exit.frame==source.original.draw.frame && exit.frame==source.owned.draw.frame &&
        source.original.draw.side==exit.sourceSide && source.owned.draw.side==exit.sourceSide && exit.list &&
        exit.list==source.sourceRecording.list && exit.list==source.original.draw.list && exit.list==source.owned.draw.list &&
        exit.generation && exit.generation==source.sourceRecording.generation;
}
bool ValidateNativeExit(const Work& w,const Source& source){
    const auto& exit=*source.nativeExit;
    if(!exit.readback || !native_exit_detail::CompletePlanes(exit.planes))return false;
    ComPtr<ID3D12Device> actual;D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    const auto buffer=exit.readback->GetDesc();
    if(!native_exit_detail::ReadbackShape(buffer) || FAILED(exit.readback->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=w.device.Get() ||
        FAILED(exit.readback->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_READBACK)return false;
    std::vector<std::pair<uint64_t,uint64_t>> intervals;
    for(const auto& row:exit.planes){
        const auto desc=source.passBaseline->Targets().resources[row.slot]->GetDesc();
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT expected{};UINT rows{};UINT64 rowBytes{},total{};
        w.device->GetCopyableFootprints(&desc,row.plane,1,0,&expected,&rows,&rowBytes,&total);
        if(total==UINT64_MAX || !native_exit_detail::ValidatePlane(row,w.width,w.height,buffer.Width,expected.Footprint,rows,rowBytes))return false;
        uint64_t end{};if(!native_exit_detail::PlaneBounds(row,w.width,w.height,buffer.Width,&end))return false;
        for(const auto& interval:intervals)if(row.layout.Offset<interval.second && interval.first<end)return false;
        intervals.emplace_back(row.layout.Offset,end);
    }
    return true;
}
void CompareNativeExit(Work& w,std::span<const uint8_t> image){
    if(!w.nativeExits[0])return;
    CyberpunkVR_PreparedNativeExit[22]=CyberpunkVR_PreparedBaseline[22];
    CyberpunkVR_PreparedNativeRate[15]=CyberpunkVR_PreparedBaseline[22];
    bool complete=true,rateComplete=true,attributed=w.rateMapsReady;UINT attributedPlanes{};
    for(UINT eye=0;eye<2;++eye){const auto& exit=*w.nativeExits[eye];const auto bytes=exit.readback->GetDesc().Width;
        void* mapped{};const D3D12_RANGE range{0,SIZE_T(bytes)};
        if(FAILED(exit.readback->Map(0,&range,&mapped)) || !mapped){complete=rateComplete=attributed=false;continue;}
        const std::span<const uint8_t> nativeData{static_cast<const uint8_t*>(mapped),SIZE_T(bytes)};
        for(const auto& native:exit.planes){
            const auto found=std::find_if(w.planes.begin(),w.planes.end(),[&](const auto& row){
                return row.eye==eye && row.slot==native.slot && row.plane==native.plane;});
            if(found==w.planes.end()){complete=rateComplete=attributed=false;continue;}
            const auto& row=*found;
            const NativeExitPlane reference{row.slot,row.plane,row.rows,row.stride,row.component,row.bytes,row.layouts[0]};
            const NativeExitPlane optimized{row.slot,row.plane,row.rows,row.stride,row.component,row.bytes,row.layouts[1]};
            native_exit_detail::Comparison comparison;
            if(!native_exit_detail::ComparePlane(w.width,w.height,reference,image,optimized,image,native,
                nativeData,comparison)){complete=rateComplete=attributed=false;continue;}
            native_exit_detail::Accumulate(CyberpunkVR_PreparedNativeExit,eye,native,w.width,comparison);
            const auto& a=comparison.reference;const auto& b=comparison.optimized;
            const uint64_t sample[]{eye,native.slot,native.plane,a.count,b.count,a.count?a.x:UINT64_MAX,a.count?a.y:UINT64_MAX,
                a.value,a.nativeValue,b.count?b.x:UINT64_MAX,b.count?b.y:UINT64_MAX,b.value,b.nativeValue,native.stride,native.component,native.bytes};
            std::copy(std::begin(sample),std::end(sample),CyberpunkVR_PreparedNativeExitSamples+(eye*5+native_exit_detail::PlaneIndex(native.slot,native.plane))*16);
            const NativeExitPlane nativeRate{row.slot,row.plane,row.rows,row.stride,row.component,row.bytes,row.layouts[3]};
            native_exit_detail::Comparison rateComparison;
            if(native_exit_detail::ComparePlane(w.width,w.height,nativeRate,image,nativeRate,image,native,nativeData,rateComparison)){
                const UINT delta=native.slot<3?5:native.plane?9:7;
                ++CyberpunkVR_PreparedNativeRate[3];CyberpunkVR_PreparedNativeRate[4]+=uint64_t(w.width)*native.rows*native.bytes;
                CyberpunkVR_PreparedNativeRate[delta+eye]+=rateComparison.reference.count;
                CyberpunkVR_PreparedNativeRate[11+eye]+=rateComparison.reference.count;
            }else rateComplete=false;
            if(native.slot<3 && w.rateMapsReady){uint64_t inside{},outside{};
                if(native_exit_detail::AttributeColor(w.width,w.height,reference,image,native,nativeData,w.rateTile,w.rateWidth,w.rateHeight,
                    w.commonRates,w.originalRates[eye],inside,outside)){
                    CyberpunkVR_PreparedVrsAttribution[5+eye]+=inside;CyberpunkVR_PreparedVrsAttribution[7+eye]+=outside;++attributedPlanes;
                }else attributed=false;
            }
        }
        const D3D12_RANGE none{};exit.readback->Unmap(0,&none);
    }
    CyberpunkVR_PreparedNativeExit[20]=complete && CyberpunkVR_PreparedNativeExit[2]==10;
    CyberpunkVR_PreparedNativeExit[21]=CyberpunkVR_PreparedNativeExit[20] &&
        std::all_of(CyberpunkVR_PreparedNativeExit+16,CyberpunkVR_PreparedNativeExit+20,[](auto value){return value==0;});
    CyberpunkVR_PreparedNativeRate[13]=rateComplete && CyberpunkVR_PreparedNativeRate[3]==10;
    CyberpunkVR_PreparedNativeRate[14]=CyberpunkVR_PreparedNativeRate[13] && !CyberpunkVR_PreparedNativeRate[15] &&
        !CyberpunkVR_PreparedNativeRate[11] && !CyberpunkVR_PreparedNativeRate[12];
    CyberpunkVR_PreparedVrsAttribution[11]=attributed && attributedPlanes==6 && CyberpunkVR_PreparedNativeExit[20] &&
        CyberpunkVR_PreparedVrsAttribution[5]+CyberpunkVR_PreparedVrsAttribution[7]==CyberpunkVR_PreparedNativeExit[4] &&
        CyberpunkVR_PreparedVrsAttribution[6]+CyberpunkVR_PreparedVrsAttribution[8]==CyberpunkVR_PreparedNativeExit[5];
    const auto acceptance=native_exit_detail::Acceptance(w.allowVrsRefinement,CyberpunkVR_PreparedNativeExit,CyberpunkVR_PreparedNativeRate,
        CyberpunkVR_PreparedVrsAttribution,std::span<const uint64_t,2>(CyberpunkVR_PreparedPairStats+6,2));
    std::copy(acceptance.begin(),acceptance.end(),CyberpunkVR_PreparedNativeAcceptance);
    if(!CyberpunkVR_PreparedNativeExit[20] || (!CyberpunkVR_PreparedNativeExit[22] && !CyberpunkVR_PreparedNativeAcceptance[7]))Error(43);
}
HRESULT Readback(ID3D12Device* device,uint64_t bytes,ComPtr<ID3D12Resource>& out){
    if(!bytes || bytes>512*1024*1024)return E_INVALIDARG;D3D12_RESOURCE_DESC desc{};
    desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;
    desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    return device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&out));
}
bool SameState(const scene_state::DrawState& a,const scene_state::DrawState& b){
    return a.original && a.original==b.original && a.topology==D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST && a.topology==b.topology &&
        a.stencil==b.stencil && a.blendFactor==b.blendFactor && a.rate.known && b.rate.known &&
        a.rate.value==b.rate.value && a.rate.combiners==b.rate.combiners;
}
const Material* MaterialAt(const Source& source,UINT parameter,UINT offset){
    const auto at=std::find_if(source.materials.begin(),source.materials.end(),[&](const auto& value){return value.binding.parameter==parameter && value.binding.offset==offset;});
    return at==source.materials.end()?nullptr:&*at;
}
bool FullT5View(const Source& source){
    if(!source.fullT5)return false;const auto& full=*source.fullT5;D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    return full.copy.list==source.sourceRecording.list && full.copy.generation==source.sourceRecording.generation &&
        full.copy.occurrence==source.ordinal+1 && !full.range.sourceOffset &&
        full.range.copiedBytes==structured_buffer_snapshot::detail::SourceBytes &&
        structured_buffer_snapshot::RecordedView(full.copy,full.range,view) &&
        structured_snapshot_view::detail::SameBufferView(view,full.view);
}
bool MembershipError(UINT reason,uint64_t ordinal=UINT64_MAX,UINT builder=0){
    auto* stats=CyberpunkVR_PreparedActualMembershipStats;
    if(!stats[15]){stats[15]=reason;stats[16]=ordinal==UINT64_MAX?0:ordinal+1;stats[17]=builder;}return false;
}
void ResetMembership(bool requested,UINT frame){
    std::fill_n(CyberpunkVR_PreparedActualMembershipStats,24,0ull);CyberpunkVR_PreparedActualMembershipStats[0]=2;
    std::fill_n(CyberpunkVR_PreparedActualMembershipRows,32*512,0ull);CyberpunkVR_PreparedActualMembershipCount=0;
    CyberpunkVR_PreparedActualMembershipStats[1]=requested;
    if(requested)CyberpunkVR_PreparedActualMembershipStats[2]=frame;
}
const source_cbv_snapshot::PrivateCbv* MembershipB4(const Source& source){
    const source_cbv_snapshot::PrivateCbv* result{};
    for(const auto& cbv:source.cbvs)if(cbv.parameter==19 && cbv.offset==0){if(result)return nullptr;result=&cbv;}
    return result && source_cbv_snapshot::MatchesRecordedPrivateCbv(source.cbvReceipt,*result)?result:nullptr;
}
bool PrepareMembershipBanks(Work& w){
    if(!w.actualMembership)return true;
    if(!w.current || !w.sequence || !w.nativeExits[0] || !w.nativeExits[1] || w.sources.size()>512)return MembershipError(1);
    auto* stats=CyberpunkVR_PreparedActualMembershipStats;stats[3]=w.sources.size();
    for(const auto& source:w.sources){prepared_membership::Bank bank;prepared_membership::Failure failure;
        if(!prepared_membership::Build(source,bank,failure))return MembershipError(2,source.ordinal,UINT(failure));
        const auto& input=*bank.Source();const auto& summary=*bank.Statistics();
        auto* row=CyberpunkVR_PreparedActualMembershipRows+w.membershipBanks.size()*32;
        const uint64_t values[]{source.ordinal,source.original.draw.side,w.sourceFrame,reinterpret_cast<uintptr_t>(input.b4.Get()),input.b4Offset,
            reinterpret_cast<uintptr_t>(input.t5.Get()),input.t5Offset,input.t5Bytes,summary.sourcePassStorageExtent,summary.materialSlots,
            summary.coveredSlots,summary.immutableSlots,summary.nullSlots,summary.omittedSlots,summary.maximumBit};
        std::copy(std::begin(values),std::end(values),row);
        stats[5]+=summary.coveredSlots;stats[6]+=summary.omittedSlots;w.membershipBanks.push_back(std::move(bank));
        stats[4]=w.membershipBanks.size();CyberpunkVR_PreparedActualMembershipCount=UINT(w.membershipBanks.size());
    }
    return true;
}
bool MembershipTable(const Source& source,const SharedBankTable& table,UINT extent){
    const SharedBankTable* reference{};
    for(const auto& value:source.referenceTables)if(value.parameter==31){if(reference)return false;reference=&value;}
    if(!reference || reference->cpuHeap!=table.cpuHeap || reference->offset!=table.offset || reference->count!=table.count ||
        reference->storageScope!=table.storageScope || reference->storageExtent!=table.storageExtent ||
        reference->identity!=table.identity || reference->version!=table.version || extent>table.storageExtent)return false;
    const source_binding_assembly::Scope scope{reinterpret_cast<uintptr_t>(source.root.Get()),source.sourceRecording.list,
        source.sourceRecording.generation,source.ordinal+1,source.ordinal+1,source.original.draw.side};
    source_binding_assembly::OwnedSrvCoverageView coverage;
    if(!source_binding_assembly::DescribeOwnedSrvCoverage(table,scope,coverage))return false;
    for(const auto& material:source.materials)if(material.binding.parameter==31 && material.binding.offset<extent &&
        !source_binding_assembly::MatchesOwnedSrvCoverage(table,scope,material.binding.offset,material.binding.stamp,coverage))return false;
    return true;
}
bool BuildBank(Work& w,Pair& candidate){
    const auto& family=candidate.family;std::vector<BankPair> pairs;UINT b4Checks{},tableChecks{};
    for(const auto& location:family->pairedRoot->plan.bindings){BankPair pair;pair.binding=location.binding;
        for(UINT eye=0;eye<2;++eye){const auto& source=w.sources[candidate.sources[eye]];auto& value=pair.eyes[eye];
            if(location.binding.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){
                const auto found=std::find_if(source.cbvs.begin(),source.cbvs.end(),[&](const auto& cbv){return cbv.parameter==location.sourceParameter && cbv.offset==location.sourceOffset;});
                if(found==source.cbvs.end() || !found->resource || !found->owner)return false;
                if(w.actualMembership && location.sourceParameter==19 && location.sourceOffset==0){
                    if(MembershipB4(source)!=&*found)return false;++b4Checks;
                }
                value.source=found->resource;value.owner=found->owner;value.gpuCbv=found->view;
            }else if(location.binding.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV){
                const auto* material=MaterialAt(source,location.sourceParameter,location.sourceOffset);if(!material)return false;
                descriptor_views::SrvResult decoded;if(!descriptor_views::DecodeSrv(material->binding.stamp.value,decoded))return false;
                value.srv=decoded.view;
                if(!material->binding.resource){if(decoded.implicit)return false;continue;}
                value.borrowedSrv=true;
                if(w.actualMembership && location.sourceParameter==23 && location.sourceOffset==5 && material->buffer)return false;
                if(material->buffer){if(!MaterialMatches(w,source,*material))return false;
                    value.source=material->buffer->resource;value.owner=material->buffer->owner;value.srv=material->buffer->view;}
                else if(location.sourceParameter==23 && location.sourceOffset==5 && source.fullT5){
                    if(!FullT5View(source))return false;
                    value.source=source.fullT5->range.resource;value.owner=source.fullT5->copy.owner;value.srv=source.fullT5->view;
                }else if(location.sourceParameter==23 && location.sourceOffset==5 && source.t5){
                    if(!std::any_of(source.inputChecks.begin(),source.inputChecks.end(),[&](const auto& check){return check.resource==source.t5->resource &&
                        check.owner==source.t5->owner && check.offset==source.t5->offset && check.expected.size()==source.t5->bytes;}))return false;
                    value.source=source.t5->resource;value.owner=source.t5->owner;value.srv=source.t5->srv;}
                else{if(!material->immutable.owner || !texture_read::ValidateImmutable(material->immutable))return false;
                    value.source=material->binding.resource;value.owner=material->immutable.owner;value.implicitSrv=decoded.implicit;}
            }else return false;
        }pairs.push_back(std::move(pair));
    }
    std::set<UINT> sharedParameters;
    for(const auto& use:family->uses){
        const bool paired=use.space==0 && use.count==1 && std::any_of(family->pairedRoot->plan.bindings.begin(),family->pairedRoot->plan.bindings.end(),[&](const auto& value){
            return UINT(value.binding.type)==use.type && UINT(value.binding.visibility)==use.visibility && value.binding.sourceRegister==use.shaderRegister;});
        if(paired)continue;
        if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV || use.type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV)return false;
        UINT parameter{},offset{};if(!descriptors::FindBinding(reinterpret_cast<uintptr_t>(family->sourceRoot.Get()),D3D12_DESCRIPTOR_RANGE_TYPE(use.type),use.space,
            use.shaderRegister,D3D12_SHADER_VISIBILITY(use.visibility),parameter,offset))return false;
        sharedParameters.insert(parameter);
    }
    std::vector<SharedBankTable> shared;
    for(const auto parameter:sharedParameters){
        const auto find=[&](const Source& source){return std::find_if(source.tables.begin(),source.tables.end(),[&](const auto& table){return table.parameter==parameter;});};
        const auto& left=w.sources[candidate.sources[0]];const auto& right=w.sources[candidate.sources[1]];
        const auto a=find(left),b=find(right);
        if(a==left.tables.end() || b==right.tables.end() || a->cpuHeap!=b->cpuHeap || a->offset!=b->offset ||
            a->storageScope!=b->storageScope || a->storageExtent!=b->storageExtent || a->identity!=b->identity || a->version!=b->version)return false;
        auto table=*a;table.count=(std::max)(a->count,b->count);table.leases.insert(table.leases.end(),b->leases.begin(),b->leases.end());
        if(w.actualMembership && parameter==31){
            for(UINT eye=0;eye<2;++eye){const auto sourceIndex=candidate.sources[eye];const auto& source=w.sources[sourceIndex];
                const auto& summary=*w.membershipBanks[sourceIndex].Statistics();
                if((summary.maximumBit!=UINT_MAX && summary.maximumBit>=table.count) ||
                    !MembershipTable(source,eye?*b:*a,table.count))return false;++tableChecks;
            }
        }
        for(const auto* source:{&left,&right})for(const auto& material:source->materials)if(material.immutable.owner)table.leases.push_back(material.immutable.owner);
        shared.push_back(std::move(table));
    }
    if(w.actualMembership && (b4Checks!=2 || tableChecks!=2))return false;
    candidate.actualBindings=w.actualMembership;candidate.bindings=std::move(pairs);candidate.shared=std::move(shared);return true;
}
bool PrepareGeometry(Work& w){
    std::vector<geometry::DrawResources> reference;std::vector<SourceBankDraw> banks;
    std::vector<std::vector<SharedBankTable>> referenceTables;referenceTables.reserve(w.sources.size());
    std::vector<uint64_t> ids(w.sources.size());
    for(const auto& source:w.sources){reference.push_back(source.owned);
        if(source.referenceTables.empty())return false;
        auto& tables=referenceTables.emplace_back(source.referenceTables);
        for(auto& table:tables){std::vector<const Material*> overrides;
            if(w.actualMembership && table.parameter==19){
                const auto* b4=MembershipB4(source);const auto* input=w.membershipBanks[banks.size()].Source();
                if(!b4 || table.count!=1 || !input || input->b4!=b4->resource || input->b4Offset!=b4->privateOffset)
                    return MembershipError(3,source.ordinal);
                ComPtr<ID3D12DescriptorHeap> heap;const D3D12_DESCRIPTOR_HEAP_DESC desc{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,1,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
                probe::Scope internal;if(FAILED(w.device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap))))return MembershipError(3,source.ordinal);
                w.device->CreateConstantBufferView(&b4->view,heap->GetCPUDescriptorHandleForHeapStart());
                table.cpuHeap=std::move(heap);table.offset=0;table.identity=table.version=table.storageScope=table.storageExtent=0;table.srvCoverage.reset();
                table.leases.push_back(b4->resource);table.leases.push_back(b4->owner);table.leases.push_back(source.cbvReceipt.owner);
                auto& bound=CyberpunkVR_PreparedActualMembershipRows[banks.size()*32+15];
                if(bound)return MembershipError(3,source.ordinal);bound=1;++CyberpunkVR_PreparedActualMembershipStats[19];continue;
            }
            for(const auto& material:source.materials)if(material.buffer && material.binding.parameter==table.parameter)overrides.push_back(&material);
            const bool fullT5=table.parameter==23 && bool(source.fullT5);
            if(overrides.empty() && !fullT5)continue;
            const auto description=table.cpuHeap->GetDesc();if(description.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || table.count!=8 || table.parameter!=23)return false;
            ComPtr<ID3D12DescriptorHeap> heap;const D3D12_DESCRIPTOR_HEAP_DESC desc{description.Type,table.count,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
            probe::Scope internal;if(FAILED(w.device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap))))return false;
            const auto stride=w.device->GetDescriptorHandleIncrementSize(description.Type);auto from=table.cpuHeap->GetCPUDescriptorHandleForHeapStart();from.ptr+=SIZE_T(table.offset)*stride;
            const auto start=heap->GetCPUDescriptorHandleForHeapStart();w.device->CopyDescriptorsSimple(table.count,start,from,description.Type);
            for(const auto* material:overrides){const auto& copy=*material->buffer;
                if(material->binding.offset>=table.count || !MaterialMatches(w,source,*material))return false;
                w.device->CreateShaderResourceView(copy.resource.Get(),&copy.view,{start.ptr+SIZE_T(material->binding.offset)*stride});table.leases.push_back(copy.owner);
            }
            if(fullT5){
                if(!FullT5View(source))return false;const auto& full=*source.fullT5;
                w.device->CreateShaderResourceView(full.range.resource.Get(),&full.view,{start.ptr+SIZE_T(5)*stride});
                table.leases.push_back(full.copy.owner);table.leases.push_back(full.range.owner);
            }
            table.cpuHeap=heap;table.offset=0;table.identity=table.version=table.storageScope=table.storageExtent=0;table.srvCoverage.reset();
        }
        banks.push_back({source.root,source.shader->root,tables});}
    if(w.actualMembership && CyberpunkVR_PreparedActualMembershipStats[19]!=w.sources.size())return MembershipError(3);
    if(FAILED(w.referenceGeometry.CreateSources(w.device.Get(),reference)) || FAILED(w.referenceBank.CreateSourcePass(w.device.Get(),banks)))return false;
    for(UINT i=0;i<w.pairs.size();++i){auto& pair=*w.pairs[i];
        pair.ready=BuildBank(w,pair);if(!pair.ready){++CyberpunkVR_PreparedBatchStats[10];if(!w.sequence)return false;continue;}
        ids[pair.sources[0]]=ids[pair.sources[1]]=i+1;
    }
    std::array<std::vector<uint64_t>,2> eyeIds;
    for(UINT eye=0;eye<2;++eye)for(const auto source:w.eyes[eye])eyeIds[eye].push_back(ids[source]);
    if(!BuildStereoMeshPlan(eyeIds[0],eyeIds[1],w.plan) || !w.plan.common)return false;
    std::vector<BankDraw> pairedBanks;std::array<UINT,2> cursor{};
    for(const auto& step:w.plan.draws){
        for(UINT eye=0;eye<2;++eye)if(step.mask&(1u<<eye))if(step.source[eye]!=cursor[eye]++)return false;
        if(step.mask!=3)continue;
        auto& candidate=*w.pairs[size_t(eyeIds[0][step.source[0]]-1)];
        candidate.command=UINT(pairedBanks.size());
        if(w.actualMembership){if(!candidate.actualBindings)return MembershipError(4);CyberpunkVR_PreparedActualMembershipStats[20]+=2;}
        pairedBanks.push_back({candidate.family->pairedRoot->root.Get(),&candidate.family->pairedRoot->plan,candidate.bindings,candidate.shared,nullptr});
    }
    if(cursor[0]!=w.eyes[0].size() || cursor[1]!=w.eyes[1].size() ||
        FAILED(w.pairedBank.CreatePass(w.device.Get(),pairedBanks)))return false;
    CyberpunkVR_PreparedBatchDescriptors[0]=w.referenceBank.DescriptorCount();CyberpunkVR_PreparedBatchDescriptors[1]=w.pairedBank.DescriptorCount();
    CyberpunkVR_PreparedBatchDescriptors[2]=w.pairedBank.DrawCount();CyberpunkVR_PreparedBatchDescriptors[3]=w.pairedBank.SnapshotCount();
    CyberpunkVR_PreparedBatchStats[3]=w.plan.common;CyberpunkVR_PreparedBatchStats[6]=w.plan.draws.size();CyberpunkVR_PreparedBatchStats[11]=1;
    CyberpunkVR_PreparedBatchPlanCount=UINT(w.plan.draws.size());
    for(UINT i=0;i<w.plan.draws.size();++i){const auto& step=w.plan.draws[i];auto* row=CyberpunkVR_PreparedBatchPlan+i*4;
        row[0]=step.mask;row[1]=row[2]=row[3]=UINT64_MAX;
        for(UINT eye=0;eye<2;++eye)if(step.mask&(1u<<eye))row[1+eye]=w.sources[w.eyes[eye][step.source[eye]]].ordinal;
        if(step.mask==3)row[3]=w.pairs[size_t(eyeIds[0][step.source[0]]-1)]->command;
        else ++CyberpunkVR_PreparedBatchStats[step.mask==1?4:5];
    }
    return true;
}
bool PrepareInputs(Work& w){
    std::vector<uint8_t> expected;std::vector<GeometryCompareRange> ranges;
    if(w.actualMembership){
        std::vector<MultilayerMembershipSource> inputs;inputs.reserve(w.membershipBanks.size());
        for(const auto& bank:w.membershipBanks){inputs.push_back(*bank.Source());if(w.membershipFault)inputs.back().membership.fill(0);}
        if(inputs.empty() || FAILED(w.membership.Create(w.device.Get(),inputs,w.predicate.Get())) ||
            FAILED(Readback(w.device.Get(),w.membership.ReadbackBytes(),w.membershipReadback)))return MembershipError(5);
        const uint64_t accepted=1;expected.resize(inputs.size()*sizeof(accepted));
        for(UINT index=0;index<inputs.size();++index){
            std::memcpy(expected.data()+index*8,&accepted,8);
            GeometryCompareRange range;range.resources[0]=w.membership.Predicates();
            range.offsets={GpuMultilayerMembership::PredicateOffset(index),uint64_t(index)*8};range.bytes=8;ranges.push_back(std::move(range));
        }
        CyberpunkVR_PreparedPairInputStats[0]=inputs.size();CyberpunkVR_PreparedActualMembershipStats[8]=w.membership.ReadbackBytes();
    }else for(const auto& source:w.sources){
        if(!source.inputChecks.empty())++CyberpunkVR_PreparedPairInputStats[0];
        for(const auto& input:source.inputChecks){
            if(!input.resource || !input.owner || input.expected.empty() || input.expected.size()>65536 || expected.size()>4*1024*1024-input.expected.size())return false;
            GeometryCompareRange range;range.resources[0]=input.resource;range.offsets={input.offset,expected.size()};range.bytes=input.expected.size();
            ranges.push_back(std::move(range));expected.insert(expected.end(),input.expected.begin(),input.expected.end());expected.resize((expected.size()+3)&~size_t(3));
        }
    }
    if(ranges.empty())return true;
    if(!w.actualMembership && CyberpunkVR_PreparedPairInputFault.load(std::memory_order_relaxed))expected.front()^=1;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=expected.size();
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;probe::Scope internal;
    if(FAILED(w.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&w.expectedInputs))))return false;
    void* mapped{};const D3D12_RANGE none{};if(FAILED(w.expectedInputs->Map(0,&none,&mapped)) || !mapped)return false;
    std::memcpy(mapped,expected.data(),expected.size());_mm_sfence();const D3D12_RANGE written{0,expected.size()};w.expectedInputs->Unmap(0,&written);
    for(auto& range:ranges)range.resources[1]=w.expectedInputs;
    if(FAILED(w.inputs.Create(w.device.Get(),1,ranges,w.predicate.Get())))return false;
    CyberpunkVR_PreparedPairInputStats[1]=ranges.size();CyberpunkVR_PreparedPairInputStats[2]=w.inputs.ComparedBytes();
    CyberpunkVR_PreparedPairInputStats[4]=expected.size();CyberpunkVR_PreparedPairInputStats[7]=16;return true;
}
ID3D12Resource* DrawPermission(Work& w){return w.inputs.Predicates()?w.inputs.Predicates():w.predicate.Get();}
bool PrepareMesh(Work& w){
    std::vector<GeometryCompareRange> ranges;
    for(const auto& candidate:w.pairs){const auto& pair=*candidate;if(pair.command==UINT_MAX)continue;
        for(UINT slot=0;slot<17;++slot){
            if(slot && !(w.sources[pair.sources[0]].owned.draw.inputSlotMask&(1u<<(slot-1))))continue;
            GeometryCompareRange range;range.pair=pair.command;
            for(UINT eye=0;eye<2;++eye){const auto& geometry=w.sources[pair.sources[eye]].owned;
                const auto& buffer=slot?geometry.vertices[slot-1]:geometry.index;
                auto address=slot?geometry.draw.vertices[slot-1].BufferLocation:geometry.draw.index.BufferLocation;
                UINT64 bytes=slot?geometry.draw.vertices[slot-1].SizeInBytes:geometry.draw.index.SizeInBytes;
                if(slot==8){const auto stride=geometry.draw.vertices[7].StrideInBytes;
                    address+=UINT64(geometry.draw.firstInstance)*stride;bytes=UINT64(geometry.draw.instances)*stride;}
                if(!buffer.resource || !bytes || address<buffer.gpu || address-buffer.gpu>buffer.bytes || bytes>buffer.bytes-(address-buffer.gpu) ||
                    (eye && bytes!=range.bytes))return false;
                range.resources[eye]=buffer.resource;range.offsets[eye]=address-buffer.gpu;range.bytes=bytes;
            }
            ranges.push_back(std::move(range));
        }
    }
    if(FAILED(w.mesh.Create(w.device.Get(),w.pairedBank.DrawCount(),ranges,DrawPermission(w))) ||
        FAILED(Readback(w.device.Get(),w.inputs.Predicates()?InputReadbackOffset+16:MeshReadbackOffset+UINT64(w.mesh.PairCount())*16,w.queryReadback)))return false;
    w.meshBytes=w.mesh.ComparedBytes()*2;
    CyberpunkVR_PreparedPairMeshStats[0]=w.mesh.PairCount();CyberpunkVR_PreparedPairMeshStats[1]=w.mesh.RangeCount();
    CyberpunkVR_PreparedPairMeshStats[2]=w.mesh.ComparedBytes();CyberpunkVR_PreparedPairMeshStats[7]=UINT64(w.mesh.PairCount())*16;
    return true;
}
struct Restore {
    ID3D12GraphicsCommandList* list;native_probe::DrawRecord draw;scene_state::Bindings bindings;ComPtr<ID3D12GraphicsCommandList1> views;
    ComPtr<ID3D12GraphicsCommandList5> rate;bool valid{};
    Restore(ID3D12GraphicsCommandList* value,const native_probe::DrawRecord& original):list(value),draw(original){
        valid=(draw.flags&191)==191 && !(draw.flags&64) && scene_state::CaptureSourceBindings(list,bindings) &&
            bindings.root==reinterpret_cast<ID3D12RootSignature*>(draw.root) && SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&views)));
        list->QueryInterface(IID_PPV_ARGS(&rate));
    }
    ~Restore(){if(!valid)return;probe::Scope internal;bindings.Restore(list);if(!bindings.computeRoot)list->SetComputeRootSignature(nullptr);views->SetViewInstanceMask(UINT_MAX);
        list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));list->IASetIndexBuffer(&draw.index);
        list->IASetVertexBuffers(0,16,draw.vertices.data());
        list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
        if(rate && bindings.rate.known){rate->RSSetShadingRate(bindings.rate.value,bindings.rate.combiners.data());rate->RSSetShadingRateImage(bindings.rate.image);}
        std::array<D3D12_CPU_DESCRIPTOR_HANDLE,8> colors{};for(UINT i=0;i<8;++i)colors[i]={draw.targets[i]};const D3D12_CPU_DESCRIPTOR_HANDLE depth{draw.depth};
        list->OMSetRenderTargets(draw.rtCount,draw.rtCount?colors.data():nullptr,BOOL(draw.rtContiguous),draw.depth?&depth:nullptr);
        list->RSSetViewports(1,&draw.viewport);list->RSSetScissorRects(1,&draw.scissor);
    }
};
bool CaptureDestination(ID3D12GraphicsCommandList* list){
    auto& w=*work;const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    w.recording={};w.queue.Reset();w.submitted=false;w.uses.clear();
    if(!native_recording::Capture(list,context,w.recording) || FAILED(w.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&w.fence))))return false;
    CyberpunkVR_PreparedPairStats[12]=w.recording.list;CyberpunkVR_PreparedPairStats[13]=w.recording.generation;return true;
}
bool ImageCopies(Work& w,ID3D12GraphicsCommandList* list){
    uint64_t size{};const UINT versions=w.nativeExits[0]?4:3;
    for(UINT eye=0;eye<2;++eye)for(UINT slot=0;slot<5;++slot)if(slot<w.colors || slot==4){
        const auto desc=w.referenceTargets.Resource(slot)->GetDesc();
        const auto& baseline=w.sources[w.eyes[eye].front()].passBaseline->Targets();
        const auto baselineDesc=baseline.resources[slot]->GetDesc();
        for(UINT plane=0;plane<(slot==4?2u:1u);++plane){Plane row;row.eye=eye;row.slot=slot;row.plane=plane;
            UINT counts[4]{};UINT64 widths[4]{};
            for(UINT version=0;version<versions;++version){UINT64 total{};
                w.device->GetCopyableFootprints(version==2?&baselineDesc:&desc,version==2?plane:eye+plane*2,1,size,
                    &row.layouts[version],&counts[version],&widths[version],&total);
                if(total==UINT64_MAX || size>UINT64_MAX-total || size+total>UINT64_MAX-511)return false;size=(size+total+511)&~uint64_t(511);}
            if(counts[0]!=counts[1] || counts[0]!=counts[2] || counts[0]!=w.height || widths[0]!=widths[1] || widths[0]!=widths[2] || widths[0]%w.width)return false;
            if(versions==4 && (counts[3]!=counts[0] || widths[3]!=widths[0]))return false;
            row.rows=counts[0];row.stride=UINT(widths[0]/w.width);row.bytes=slot==4?(plane?1u:4u):4u;
            row.component=slot==4 && plane && row.stride==8?4u:0u;
            if(!row.stride || row.component+row.bytes>row.stride)return false;w.planes.push_back(row);
        }
    }
    if(FAILED(Readback(w.device.Get(),size,w.imageReadback)) || !gpu::KeepCommandResources(list,{w.imageReadback.Get()}))return false;w.imageBytes=size;
    for(UINT eye=0;eye<2;++eye)for(UINT slot:{0u,1u,2u,4u})
        if(!gpu::KeepCommandResources(list,{w.sources[w.eyes[eye].front()].passBaseline->Targets().resources[slot].Get()}))return false;
    probe::Scope internal;
    for(UINT slot=0;slot<5;++slot)if(slot<w.colors || slot==4)for(auto* chain:{&w.referenceTargets,&w.pairedTargets,&w.nativeRateTargets}){
        if(chain==&w.nativeRateTargets && !w.nativeExits[0])continue;
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;barrier.Transition={chain->Resource(slot),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&barrier);
    }
    for(UINT eye=0;eye<2;++eye)for(UINT slot:{0u,1u,2u,4u}){
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={w.sources[w.eyes[eye].front()].passBaseline->Targets().resources[slot].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&barrier);
    }
    for(const auto& row:w.planes)for(UINT version=0;version<versions;++version){D3D12_TEXTURE_COPY_LOCATION from{},to{};
        from.pResource=version==2?w.sources[w.eyes[row.eye].front()].passBaseline->Targets().resources[row.slot].Get():
            (version==3?w.nativeRateTargets:version?w.pairedTargets:w.referenceTargets).Resource(row.slot);
        from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;from.SubresourceIndex=version==2?row.plane:row.eye+row.plane*2;
        to.pResource=w.imageReadback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=row.layouts[version];list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
    }
    for(UINT eye=0;eye<2;++eye)for(UINT slot:{0u,1u,2u,4u}){
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={w.sources[w.eyes[eye].front()].passBaseline->Targets().resources[slot].Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            D3D12_RESOURCE_STATE_COPY_SOURCE,slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET};list->ResourceBarrier(1,&barrier);
    }
    return true;
}
}
uint32_t CompatibilityFailure(const Source& a,const Source& b){
    const auto& x=a.original.draw;const auto& y=b.original.draw;
    if(x.side!=1 || y.side!=0)return 1;
    if(!x.frame || x.frame!=y.frame)return 2;
    if(x.pipeline!=y.pipeline || x.root!=y.root)return 3;
    if(x.indices!=y.indices || !x.indices || x.instances!=y.instances || !x.instances || x.firstIndex!=y.firstIndex || x.baseVertex!=y.baseVertex)return 4;
    if(x.inputSlotMask!=y.inputSlotMask || !(x.inputSlotMask&128) || std::memcmp(&x.index,&y.index,sizeof(x.index)))return 5;
    if(std::memcmp(&x.viewport,&y.viewport,sizeof(x.viewport)) || std::memcmp(&x.scissor,&y.scissor,sizeof(x.scissor)))return 6;
    if(a.owned.draw.vertices[7].StrideInBytes!=b.owned.draw.vertices[7].StrideInBytes)return 7;
    if(!SameState(a.owned.state,b.owned.state)){
        if(!CyberpunkVR_PreparedPairStateMismatch[2]){const auto& p=a.owned.state;const auto& q=b.owned.state;
            const uint64_t values[]{a.ordinal,b.ordinal,reinterpret_cast<uintptr_t>(p.original.Get()),reinterpret_cast<uintptr_t>(q.original.Get()),
                UINT(p.topology),UINT(q.topology),p.stencil,q.stencil,p.rate.known,q.rate.known,UINT(p.rate.value),UINT(q.rate.value),
                reinterpret_cast<uintptr_t>(p.rate.image),reinterpret_cast<uintptr_t>(q.rate.image),reinterpret_cast<uintptr_t>(p.rateImage.Get()),reinterpret_cast<uintptr_t>(q.rateImage.Get()),
                UINT(p.rate.combiners[0]),UINT(p.rate.combiners[1]),UINT(q.rate.combiners[0]),UINT(q.rate.combiners[1])};
            std::copy(std::begin(values),std::end(values),CyberpunkVR_PreparedPairStateMismatch);
            std::memcpy(CyberpunkVR_PreparedPairStateMismatch+20,p.blendFactor.data(),16);std::memcpy(CyberpunkVR_PreparedPairStateMismatch+22,q.blendFactor.data(),16);
        }return 8;
    }
    for(UINT slot=0;slot<16;++slot)if(slot!=7 && (x.inputSlotMask&(1u<<slot)) && std::memcmp(&x.vertices[slot],&y.vertices[slot],sizeof(x.vertices[slot])))return 9;
    if(bool(a.owned.state.rate.image)!=bool(a.rateImage) || bool(b.owned.state.rate.image)!=bool(b.rateImage) || bool(a.rateImage)!=bool(b.rateImage))return 10;
    if(a.rateImage){const auto p=a.rateImage->GetDesc(),q=b.rateImage->GetDesc();
        if(p.Width!=q.Width || p.Height!=q.Height)return 10;}
    if(!HasBaseline(a) || !HasBaseline(b))return 11;
    return 0;
}
bool CompatibleSources(const Source& a,const Source& b){return CompatibilityFailure(a,b)==0;}
namespace {
bool StartInternal(ID3D12Device* device,std::vector<Source> sources,bool sequence,std::shared_ptr<const families::Family> singleFamily={},
    ID3D12CommandQueue* currentQueue=nullptr,UINT currentFrame=0)try{
    std::lock_guard lock(mutex);if(active || !device || sources.size()<2 || sources.size()>512)return false;
    auto next=std::make_unique<Work>();next->device=device;next->sources=std::move(sources);next->sequence=sequence;
    next->current=currentQueue || currentFrame;next->queue=currentQueue;next->sourceFrame=currentFrame;
    next->exportRequested=next->current && CyberpunkVR_PreparedNativeExportEnable.load()!=0;
    next->actualMembership=next->current && CyberpunkVR_PreparedActualMembershipEnable.load()!=0;
    next->membershipFault=next->actualMembership && CyberpunkVR_PreparedPairInputFault.load()!=0;
    ResetMembership(next->actualMembership,currentFrame);CyberpunkVR_PreparedActualMembershipStats[18]=next->membershipFault;
    std::fill_n(CyberpunkVR_PreparedNativeExport,24,0ull);
    CyberpunkVR_PreparedNativeExport[0]=next->exportRequested;CyberpunkVR_PreparedNativeExport[12]=currentFrame;
    std::fill_n(CyberpunkVR_PreparedCurrent,16,0ull);CyberpunkVR_PreparedCurrent[0]=next->current;
    CyberpunkVR_PreparedCurrent[1]=currentFrame;CyberpunkVR_PreparedCurrent[8]=next->sources.size();
    if(next->current){ComPtr<ID3D12Device> actual;std::fill_n(CyberpunkVR_PreparedPairStats,16,0ull);
        if(!currentQueue || !currentFrame || currentQueue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
            FAILED(currentQueue->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device)return false;}
    next->pairForSource.assign(next->sources.size(),UINT_MAX);
    std::fill_n(CyberpunkVR_PreparedPairVrs,8,0ull);
    std::fill_n(CyberpunkVR_PreparedPairVrsGpu,4,0ull);
    std::fill_n(CyberpunkVR_PreparedPairGpu,36,0ull);
    std::fill_n(CyberpunkVR_PreparedPairColor,12,0ull);
    std::fill_n(CyberpunkVR_PreparedPairDepthState,16,0ull);
    std::fill_n(CyberpunkVR_PreparedPairMeshStats,12,0ull);std::fill_n(CyberpunkVR_PreparedPairMeshResults,512,0ull);
    std::fill_n(CyberpunkVR_PreparedPairInputStats,8,0ull);
    std::fill_n(CyberpunkVR_PreparedPairResidency,8,0ull);
    std::fill_n(CyberpunkVR_PreparedBaseline,24,0ull);
    std::fill_n(CyberpunkVR_PreparedNativeExit,24,0ull);CyberpunkVR_PreparedNativeExit[23]=1;
    std::fill_n(CyberpunkVR_PreparedNativeExitSamples,160,0ull);
    std::fill_n(CyberpunkVR_PreparedNativeRate,16,0ull);std::fill_n(CyberpunkVR_PreparedVrsAttribution,12,0ull);
    std::fill_n(CyberpunkVR_PreparedNativeAcceptance,8,0ull);
    const bool nativeExitMode=std::any_of(next->sources.begin(),next->sources.end(),[](const auto& source){return bool(source.nativeExit);});
    next->allowVrsRefinement=next->sources.front().allowVrsRefinement;CyberpunkVR_PreparedNativeAcceptance[0]=next->allowVrsRefinement;
    CyberpunkVR_PreparedNativeExit[0]=nativeExitMode;
    CyberpunkVR_PreparedNativeRate[0]=nativeExitMode;
    if((next->allowVrsRefinement && !nativeExitMode) ||
        std::any_of(next->sources.begin(),next->sources.end(),[&](const auto& source){return source.allowVrsRefinement!=next->allowVrsRefinement;}))return false;
    if(nativeExitMode && (!next->current || !sequence))return false;
    std::fill_n(CyberpunkVR_PreparedBatchStats,16,0ull);CyberpunkVR_PreparedBatchSourceCount=CyberpunkVR_PreparedBatchPlanCount=0;
    std::fill_n(CyberpunkVR_PreparedBatchDescriptors,4,0ull);
    const auto& draw=next->sources[0].owned.draw;next->width=UINT(draw.viewport.Width);next->height=UINT(draw.viewport.Height);next->colors=3;
    if(!next->width || !next->height || draw.viewport.TopLeftX || draw.viewport.TopLeftY)return false;
    std::vector<std::shared_ptr<const families::Family>> sourceFamilies;
    for(UINT i=0;i<next->sources.size();++i){const auto& source=next->sources[i];const auto& actual=source.owned.draw;
        const auto family=sequence?families::Lookup(source.owned.state.original.Get()):singleFamily;
        if(!source.root || reinterpret_cast<uintptr_t>(source.root.Get())!=source.original.draw.root || actual.rtCount!=3 ||
            families::NativeGeometryStatus(source.owned.state.original.Get())!=1 ||
            !source.shader || source.shader->root.empty() || !HasBaseline(source) || actual.side>1 || actual.frame!=draw.frame || !actual.frame ||
            actual.viewport.Width!=draw.viewport.Width || actual.viewport.Height!=draw.viewport.Height || actual.viewport.TopLeftX || actual.viewport.TopLeftY ||
            !source.owned.state.rate.known || bool(source.owned.state.rate.image)!=bool(source.rateImage) ||
            (family && family->sourceRoot!=source.root))return false;
        if(next->current && (actual.frame!=currentFrame || source.original.draw.frame!=currentFrame ||
            (!next->actualMembership && source.inputChecks.empty()) ||
            source.sourceQueue.Get()!=currentQueue || !source.sourceRecording.token || !source.sourceRecording.generation ||
            !source.sourceRecording.context || !source.sourceRecording.registry || source.sourceRecording.list!=source.original.draw.list ||
            source.sourceRecording.list!=actual.list))return false;
        const auto eye=1-actual.side;auto& indices=next->eyes[eye];
        if(!indices.empty() && (source.ordinal<=next->sources[indices.back()].ordinal || source.passBaseline!=next->sources[indices.front()].passBaseline))return false;
        for(UINT slot:{0u,1u,2u,4u}){const auto baseline=source.passBaseline->Targets().resources[slot]->GetDesc();
            if(baseline.Width!=next->width || baseline.Height!=next->height)return false;}
        if(nativeExitMode){
            if(!source.nativeExit || (!indices.empty() && source.nativeExit!=next->nativeExits[eye]) ||
                !NativeExitMatchesSource(*next,source,eye) || (indices.empty() && !ValidateNativeExit(*next,source)))return false;
            next->nativeExits[eye]=source.nativeExit;
        }
        indices.push_back(i);sourceFamilies.push_back(family);
        CyberpunkVR_PreparedBatchSources[i*2]=source.ordinal;CyberpunkVR_PreparedBatchSources[i*2+1]=eye;
    }
    if(next->eyes[0].empty() || next->eyes[1].empty())return false;
    if(nativeExitMode){
        if(next->nativeExits[0]==next->nativeExits[1] || next->nativeExits[0]->readback==next->nativeExits[1]->readback)return false;
        CyberpunkVR_PreparedNativeExit[1]=2;
        CyberpunkVR_PreparedNativeRate[1]=2;
    }
    if(next->sources[next->eyes[0].front()].passBaseline==next->sources[next->eyes[1].front()].passBaseline)return false;
    for(UINT a:{0u,1u,2u,4u})for(UINT b:{0u,1u,2u,4u})
        if(next->sources[next->eyes[0].front()].passBaseline->Targets().resources[a]==
            next->sources[next->eyes[1].front()].passBaseline->Targets().resources[b])return false;
    CyberpunkVR_PreparedBaseline[0]=2;CyberpunkVR_PreparedBaseline[1]=next->colors;
    for(const auto left:next->eyes[0])for(const auto right:next->eyes[1]){
        if(next->pairForSource[right]!=UINT_MAX || !CompatibleSources(next->sources[left],next->sources[right]))continue;
        const auto& family=sourceFamilies[left];if(!family || !family->pipeline || !family->pairedRoot || !family->instanceRebaseSafe ||
            !family->pass.depthStateKnown || family->pass.targets.NumRenderTargets!=3)continue;
        auto pair=std::make_unique<Pair>();pair->sources={left,right};pair->family=family;
        next->pairForSource[left]=next->pairForSource[right]=UINT(next->pairs.size());next->pairs.push_back(std::move(pair));break;
    }
    if(next->pairs.empty())return false;
    const auto& ds=next->pairs.front()->family->pass.depthState;
    const uint64_t depthValues[]{UINT(ds.DepthEnable),UINT(ds.DepthFunc),UINT(ds.DepthWriteMask),UINT(ds.StencilEnable),ds.StencilReadMask,ds.StencilWriteMask,
        UINT(ds.FrontFace.StencilFunc),UINT(ds.BackFace.StencilFunc),next->sources[0].owned.state.stencil,
        UINT(ds.FrontFace.StencilFailOp),UINT(ds.FrontFace.StencilDepthFailOp),UINT(ds.FrontFace.StencilPassOp),UINT(ds.BackFace.StencilPassOp)};
    std::copy(std::begin(depthValues),std::end(depthValues),CyberpunkVR_PreparedPairDepthState);
    {probe::Scope internal;if(FAILED(next->referenceTargets.Create(device,next->width,next->height)) ||
        FAILED(next->pairedTargets.Create(device,next->width,next->height)))return false;
        if(nativeExitMode && FAILED(next->nativeRateTargets.Create(device,next->width,next->height)))return false;
        D3D12_QUERY_HEAP_DESC queries{D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS,3,0};
        if(FAILED(device->CreateQueryHeap(&queries,IID_PPV_ARGS(&next->pipelineQueries))))return false;
        queries.Type=D3D12_QUERY_HEAP_TYPE_OCCLUSION;
        if(FAILED(device->CreateQueryHeap(&queries,IID_PPV_ARGS(&next->occlusionQueries))))return false;
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=8;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;
        desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
        if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&next->predicate))))return false;
        void* value{};const D3D12_RANGE none{};if(FAILED(next->predicate->Map(0,&none,&value)) || !value)return false;
        *static_cast<uint64_t*>(value)=0;_mm_sfence();const D3D12_RANGE written{0,8};next->predicate->Unmap(0,&written);
        if(next->sources[0].rateImage){std::vector<ID3D12Resource*> images;images.reserve(next->sources.size());
            for(const auto& source:next->sources){
                if(!source.rateImage || !source.rateCapture || source.rateCapture->SourceImage()!=source.rateImage.Get())return false;
                images.push_back(source.rateImage.Get());
            }
            if(FAILED(next->rates.Create(device,images)))return false;
            const auto image=next->rates.Image()->GetDesc();
            CyberpunkVR_PreparedPairVrs[0]=image.Width;CyberpunkVR_PreparedPairVrs[1]=image.Height;CyberpunkVR_PreparedPairVrs[2]=image.Width*image.Height;
            CyberpunkVR_PreparedPairVrs[6]=2;CyberpunkVR_PreparedPairVrsGpu[0]=next->rates.SourceCount();
            CyberpunkVR_PreparedPairVrsGpu[2]=next->rates.ReadbackBytes();
            if(nativeExitMode){D3D12_FEATURE_DATA_D3D12_OPTIONS6 options{};
                if(SUCCEEDED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6,&options,sizeof(options))) &&
                    options.VariableShadingRateTier>=D3D12_VARIABLE_SHADING_RATE_TIER_2)next->rateTile=options.ShadingRateImageTileSize;
                next->rateWidth=UINT(image.Width);next->rateHeight=image.Height;
                CyberpunkVR_PreparedVrsAttribution[0]=next->rateTile;CyberpunkVR_PreparedVrsAttribution[1]=next->rateWidth;
                CyberpunkVR_PreparedVrsAttribution[2]=next->rateHeight;
            }
        }else if(std::any_of(next->sources.begin(),next->sources.end(),[](const auto& source){return source.rateImage!=nullptr;}))return false;
    }
    if(!PrepareMembershipBanks(*next) || !PrepareGeometry(*next) || !PrepareInputs(*next) || !PrepareMesh(*next))return false;
    if(next->current && !next->inputs.Predicates())return false;
    std::vector<texture_read::ImmutableLease> assets;std::set<IUnknown*> assetOwners;
    for(const auto& source:next->sources)for(const auto& material:source.materials)
        if(material.immutable.owner && assetOwners.insert(material.immutable.owner.Get()).second)assets.push_back(material.immutable);
    CyberpunkVR_PreparedCurrent[13]=assets.size();
    if(!assets.empty()){
        const auto resident=next->residency.Acquire(device,assets);
        CyberpunkVR_PreparedPairResidency[6]=UINT(resident);
        if(FAILED(resident))return false;
        CyberpunkVR_PreparedPairResidency[0]=next->residency.SourceCount();
        CyberpunkVR_PreparedPairResidency[1]=next->residency.ObjectCount();
        CyberpunkVR_PreparedPairResidency[2]=UINT(next->residency.Status());
    }
    work=std::move(next);std::fill_n(CyberpunkVR_PreparedPairStats,16,0ull);CyberpunkVR_PreparedPairStats[0]=work->sources[work->eyes[0].front()].ordinal;
    CyberpunkVR_PreparedPairStats[1]=work->sources[work->eyes[1].front()].ordinal;CyberpunkVR_PreparedPairStats[2]=work->meshBytes;
    CyberpunkVR_PreparedBatchStats[0]=work->eyes[0].size();CyberpunkVR_PreparedBatchStats[1]=work->eyes[1].size();
    CyberpunkVR_PreparedBatchStats[2]=work->pairs.size();CyberpunkVR_PreparedBatchStats[15]=sequence;
    CyberpunkVR_PreparedBatchSourceCount=UINT(work->sources.size());active=true;
    if(work->current)return RecordCurrent();Publish();return true;
}catch(...){std::lock_guard lock(mutex);if(work && work->current && work->sourceFrame==currentFrame)FailCurrent(36);return false;}
}
bool Start(ID3D12Device* device,std::array<Source,2> sources,std::shared_ptr<const families::Family> family){
    std::vector<Source> input;input.reserve(2);for(auto& source:sources)input.push_back(std::move(source));
    return StartInternal(device,std::move(input),false,std::move(family));
}
bool StartSequence(ID3D12Device* device,std::vector<Source> sources){return StartInternal(device,std::move(sources),true);}
bool StartCurrentSequence(ID3D12Device* device,std::vector<Source> sources,ID3D12CommandQueue* queue,UINT frame){
    {std::lock_guard lock(mutex);if(Active())return false;
        ResetMembership(CyberpunkVR_PreparedActualMembershipEnable.load()!=0,frame);
        std::fill_n(CyberpunkVR_PreparedPairStats,16,0ull);std::fill_n(CyberpunkVR_PreparedCurrent,16,0ull);
        std::fill_n(CyberpunkVR_PreparedNativeExport,24,0ull);
        CyberpunkVR_PreparedNativeExport[0]=CyberpunkVR_PreparedNativeExportEnable.load()!=0;
        CyberpunkVR_PreparedNativeExport[12]=frame;
        std::fill_n(CyberpunkVR_PreparedPairCandidates,16,0ull);std::fill_n(CyberpunkVR_PreparedPairStateMismatch,24,0ull);
        std::fill_n(CyberpunkVR_PreparedPairGpu,36,0ull);std::fill_n(CyberpunkVR_PreparedPairColor,12,0ull);
        std::fill_n(CyberpunkVR_PreparedPairDepthState,16,0ull);std::fill_n(CyberpunkVR_PreparedPairMeshStats,12,0ull);
        std::fill_n(CyberpunkVR_PreparedPairMeshResults,512,0ull);std::fill_n(CyberpunkVR_PreparedPairVrs,8,0ull);
        std::fill_n(CyberpunkVR_PreparedPairVrsGpu,4,0ull);std::fill_n(CyberpunkVR_PreparedPairInputStats,8,0ull);
        std::fill_n(CyberpunkVR_PreparedPairResidency,8,0ull);std::fill_n(CyberpunkVR_PreparedBatchStats,16,0ull);
        std::fill_n(CyberpunkVR_PreparedBaseline,24,0ull);
        std::fill_n(CyberpunkVR_PreparedNativeExit,24,0ull);CyberpunkVR_PreparedNativeExit[23]=1;
        CyberpunkVR_PreparedNativeExit[0]=std::any_of(sources.begin(),sources.end(),[](const auto& source){return bool(source.nativeExit);});
        std::fill_n(CyberpunkVR_PreparedNativeExitSamples,160,0ull);
        std::fill_n(CyberpunkVR_PreparedNativeRate,16,0ull);std::fill_n(CyberpunkVR_PreparedVrsAttribution,12,0ull);
        CyberpunkVR_PreparedNativeRate[0]=CyberpunkVR_PreparedNativeExit[0];
        std::fill_n(CyberpunkVR_PreparedNativeAcceptance,8,0ull);
        CyberpunkVR_PreparedNativeAcceptance[0]=!sources.empty() && sources.front().allowVrsRefinement;
        std::fill_n(CyberpunkVR_PreparedBatchDescriptors,4,0ull);
        CyberpunkVR_PreparedBatchSourceCount=CyberpunkVR_PreparedBatchPlanCount=CyberpunkVR_PreparedPairSelectionCount=0;
        CyberpunkVR_PreparedCurrent[0]=1;CyberpunkVR_PreparedCurrent[1]=frame;CyberpunkVR_PreparedCurrent[8]=sources.size();
        CyberpunkVR_PreparedPairState=0;
        if(!queue || !frame){CyberpunkVR_PreparedPairStats[10]=35;CyberpunkVR_PreparedCurrent[12]=35;
            CyberpunkVR_PreparedPairState=6;return false;}}
    const bool started=StartInternal(device,std::move(sources),true,{},queue,frame);
    if(!started){std::lock_guard lock(mutex);if(!Active()){
        if(!CyberpunkVR_PreparedPairStats[10])CyberpunkVR_PreparedPairStats[10]=35;
        CyberpunkVR_PreparedCurrent[0]=1;CyberpunkVR_PreparedCurrent[1]=frame;
        CyberpunkVR_PreparedCurrent[12]=CyberpunkVR_PreparedPairStats[10];CyberpunkVR_PreparedPairState=6;}}
    return started;
}
bool Active(){return active.load(std::memory_order_acquire);}
bool NeedsNativeBindings(){
    if(!Active())return false;std::lock_guard lock(mutex);return Active() && work && !work->current;
}
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
bool CurrentActive(UINT frame){std::lock_guard lock(mutex);return Active() && work && work->current && work->sourceFrame==frame;}
#endif
bool WantsNativeExport(UINT frame){std::lock_guard lock(mutex);
    return work && work->current && work->sourceFrame==frame && work->exportRequested;
}
void CancelCurrent(UINT frame){std::lock_guard lock(mutex);
    if(work && work->current && work->sourceFrame==frame)FailCurrent(39);
}
namespace {
bool RefuseNativeExport(Work& w,UINT reason){
    if(!CyberpunkVR_PreparedNativeExport[11])CyberpunkVR_PreparedNativeExport[11]=reason;
    if(w.targetResidency.Status()==gpu::residency::State::Acquired)
        CyberpunkVR_PreparedNativeExport[19]=UINT(w.targetResidency.Cancel());
    if(w.exportList)gpu::ResetCommandResources(w.exportList.Get());
    w.exportList.Reset();w.exportAllocator.Reset();w.outputCopy.reset();w.exportPrepared=false;
    return false;
}
bool CompareNativeOutput(Work& w,std::span<const uint8_t> pairedData){
    if(!w.exportSubmitted)return true;
    auto* stats=CyberpunkVR_PreparedNativeExport;stats[10]=CyberpunkVR_PreparedBaseline[22];
    if(!w.outputCopy || !w.outputCopy->Recorded() || w.outputCopy->Planes().size()!=10 || !w.outputCopy->Readback())return false;
    const D3D12_RANGE range{0,SIZE_T(w.outputCopy->Bytes())};void* mapped{};
    if(FAILED(w.outputCopy->Readback()->Map(0,&range,&mapped)) || !mapped)return false;
    const std::span<const uint8_t> actual{static_cast<const uint8_t*>(mapped),SIZE_T(w.outputCopy->Bytes())};
    bool valid=true;UINT compared{};
    for(UINT side=0;side<2 && valid;++side){const UINT eye=1-side;void* original{};std::span<const uint8_t> expectedData=pairedData;
        if(stats[10]){
            const auto& native=w.nativeExits[eye];
            if(!native || !native->readback){valid=false;break;}
            const auto desc=native->readback->GetDesc();const D3D12_RANGE originalRange{0,SIZE_T(desc.Width)};
            if(!native_exit_detail::ReadbackShape(desc) || FAILED(native->readback->Map(0,&originalRange,&original)) || !original){valid=false;break;}
            expectedData={static_cast<const uint8_t*>(original),SIZE_T(desc.Width)};
        }
        for(const auto& plane:w.outputCopy->Planes())if(plane.side==side){
            NativeExitPlane expected;
            if(stats[10]){
                const auto& planes=w.nativeExits[eye]->planes;
                const auto found=std::find_if(planes.begin(),planes.end(),[&](const auto& value){return value.slot==plane.slot && value.plane==plane.plane;});
                if(found==planes.end()){valid=false;break;}expected=*found;
            }else{
                const auto found=std::find_if(w.planes.begin(),w.planes.end(),[&](const auto& value){return value.eye==eye && value.slot==plane.slot && value.plane==plane.plane;});
                if(found==w.planes.end()){valid=false;break;}
                expected={found->slot,found->plane,found->rows,found->stride,found->component,found->bytes,found->layouts[1]};
            }
            const NativeExitPlane copied{plane.slot,plane.plane,plane.rows,plane.stride,plane.component,plane.bytes,plane.footprint};
            native_exit_detail::Comparison result;
            if(!native_exit_detail::ComparePlane(w.width,w.height,expected,expectedData,expected,expectedData,copied,actual,result)){valid=false;break;}
            stats[6+eye]+=result.reference.count;++compared;
        }
        if(original){const D3D12_RANGE none{};w.nativeExits[eye]->readback->Unmap(0,&none);}
    }
    const D3D12_RANGE none{};w.outputCopy->Readback()->Unmap(0,&none);
    stats[8]=valid && compared==10;stats[9]=stats[8] && stats[10] && !stats[6] && !stats[7];
    return stats[8] && !stats[6] && !stats[7];
}
UINT RecordCommands(Work& w,ID3D12GraphicsCommandList* list,ID3D12GraphicsCommandList1* views,ID3D12GraphicsCommandList5* rate){
    if(!list || !views || (w.rates.Image() && !rate))return 3;
    for(const auto& source:w.sources)for(const auto& owner:source.owners)if(owner && !gpu::KeepCommandResources(list,{owner.Get()})){return 4;}
    for(const auto& source:w.sources)if(source.rateImage && !gpu::KeepCommandResources(list,{source.rateImage.Get()})){return 4;}
    if(!gpu::KeepCommandResources(list,{w.fence.Get(),w.predicate.Get()})){
        return 4;}
    for(const auto& source:w.sources)if(!gpu::KeepCommandResources(list,{source.owned.state.original.Get()})){return 4;}
    for(const auto& pair:w.pairs)if(pair->command!=UINT_MAX && !gpu::KeepCommandResources(list,{pair->family->pipeline.Get()})){return 4;}
    if(!gpu::KeepCommandResources(list,{w.pipelineQueries.Get(),w.occlusionQueries.Get(),w.queryReadback.Get()})){
        return 4;}
    w.stage=4; // Recording may now leave GPU commands in the destination.
    if(w.actualMembership){
        if(!gpu::KeepCommandResources(list,{w.membershipReadback.Get()}) || !w.membership.Record(list)){
            MembershipError(7);return 47;
        }
        CyberpunkVR_PreparedActualMembershipStats[7]=w.membership.DispatchCount();
    }
    if(w.inputs.Predicates()){
        if(!w.actualMembership)for(const auto& source:w.sources)for(const auto& input:source.inputChecks)
            if(!gpu::KeepCommandResources(list,{input.owner.Get()})){return 30;}
        const bool recorded=w.inputs.Record(list);
        if(!recorded){return 30;}CyberpunkVR_PreparedPairInputStats[3]=w.inputs.DispatchCount();
    }
    if(!w.mesh.Record(list)){return 2;}
    CyberpunkVR_PreparedPairMeshStats[3]=w.mesh.DispatchCount();
    if(w.rates.Image()){
        if(!w.rates.Record(list)){return 31;}
        CyberpunkVR_PreparedPairVrsGpu[1]=w.rates.DispatchCount();
    }
    for(UINT eye=0;eye<2;++eye)for(auto* targets:{&w.referenceTargets,&w.pairedTargets}){
        if(!targets->SeedPrivateEye(list,eye,w.sources[w.eyes[eye].front()].passBaseline->Targets()))return 25;
        ++CyberpunkVR_PreparedBatchStats[12];++CyberpunkVR_PreparedBaseline[2];
    }
    if(w.nativeExits[0])for(UINT eye=0;eye<2;++eye)
        if(!w.nativeRateTargets.SeedPrivateEye(list,eye,w.sources[w.eyes[eye].front()].passBaseline->Targets()))return 25;
    if(!w.pairedBank.CaptureEye(list,0) || !w.pairedBank.CaptureEye(list,1)){return 6;}
    {probe::Scope internal;
        const auto record=[&](UINT query,auto&& draw){
            list->BeginQuery(w.pipelineQueries.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,query);
            list->BeginQuery(w.occlusionQueries.Get(),D3D12_QUERY_TYPE_OCCLUSION,query);
            const bool result=draw();
            list->EndQuery(w.occlusionQueries.Get(),D3D12_QUERY_TYPE_OCCLUSION,query);
            list->EndQuery(w.pipelineQueries.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,query);
            ++CyberpunkVR_PreparedPairDepthState[15];return result;
        };
        list->SetPredication(DrawPermission(w),0,D3D12_PREDICATION_OP_EQUAL_ZERO);
        const auto setState=[&](const Source& source){const auto& state=source.owned.state;const auto& draw=source.owned.draw;
            list->IASetPrimitiveTopology(state.topology);list->OMSetStencilRef(state.stencil);list->OMSetBlendFactor(state.blendFactor.data());
            if(rate){rate->RSSetShadingRate(state.rate.value,state.rate.combiners.data());rate->RSSetShadingRateImage(w.rates.Image());}
            list->RSSetViewports(1,&draw.viewport);list->RSSetScissorRects(1,&draw.scissor);
        };
        for(UINT eye=0;eye<2;++eye){views->SetViewInstanceMask(UINT_MAX);
            if(!w.referenceTargets.Bind(list,1u<<eye,w.colors) || !record(eye,[&]{
                for(const auto index:w.eyes[eye]){const auto& source=w.sources[index];setState(source);list->SetPipelineState(source.owned.state.original.Get());
                    if(!w.referenceBank.BindSource(list,index) || !w.referenceGeometry.RecordSourceGeometry(list,index))return false;
                    ++CyberpunkVR_PreparedPairStats[4];++CyberpunkVR_PreparedBatchStats[7];}
                return true;
            })){return 7;}
        }
        if(!record(2,[&]{UINT boundMask{};
            for(const auto& step:w.plan.draws){
                if(boundMask!=step.mask){if(!w.pairedTargets.Bind(list,step.mask,w.colors))return false;boundMask=step.mask;}
                const auto eye=step.mask==2?1u:0u;const auto index=w.eyes[eye][step.source[eye]];const auto& source=w.sources[index];setState(source);
                if(step.mask==3){auto& pair=*w.pairs[w.pairForSource[index]];list->SetPipelineState(pair.family->pipeline.Get());views->SetViewInstanceMask(3);
                    list->SetPredication(w.mesh.Predicates(),GpuGeometryCompare::Offset(pair.command),D3D12_PREDICATION_OP_EQUAL_ZERO);
                    // The GPU compares every consumed mesh view and the exact
                    // instance slice. Reuse left IA directly, without CPU upload.
                    if(!w.pairedBank.Bind(list,pair.command) || !w.referenceGeometry.RecordSourceGeometry(list,pair.sources[0]))return false;
                    ++CyberpunkVR_PreparedPairMeshStats[10];++CyberpunkVR_PreparedBatchStats[8];
                    list->SetPredication(w.mesh.Predicates(),GpuGeometryCompare::Offset(pair.command,true),D3D12_PREDICATION_OP_EQUAL_ZERO);
                    for(UINT fallbackEye=0;fallbackEye<2;++fallbackEye){const auto fallbackIndex=pair.sources[fallbackEye];const auto& fallback=w.sources[fallbackIndex];
                        boundMask=1u<<fallbackEye;if(!w.pairedTargets.Bind(list,boundMask,w.colors))return false;
                        setState(fallback);list->SetPipelineState(fallback.owned.state.original.Get());views->SetViewInstanceMask(UINT_MAX);
                        if(!w.referenceBank.BindSource(list,fallbackIndex) || !w.referenceGeometry.RecordSourceGeometry(list,fallbackIndex))return false;
                        ++CyberpunkVR_PreparedPairMeshStats[11];++CyberpunkVR_PreparedBatchStats[8];}
                }else{list->SetPredication(DrawPermission(w),0,D3D12_PREDICATION_OP_EQUAL_ZERO);
                    list->SetPipelineState(source.owned.state.original.Get());views->SetViewInstanceMask(UINT_MAX);
                    if(!w.referenceBank.BindSource(list,index) || !w.referenceGeometry.RecordSourceGeometry(list,index))return false;}
                if(step.mask!=3)++CyberpunkVR_PreparedBatchStats[8];
            }return true;
        })){return 8;}
        if(w.nativeExits[0]){
            list->SetPredication(DrawPermission(w),0,D3D12_PREDICATION_OP_EQUAL_ZERO);views->SetViewInstanceMask(UINT_MAX);
            for(UINT eye=0;eye<2;++eye){
                if(!w.nativeRateTargets.Bind(list,1u<<eye,w.colors))return 7;
                for(const auto index:w.eyes[eye]){const auto& source=w.sources[index];setState(source);
                    if(rate)rate->RSSetShadingRateImage(source.rateImage.Get());
                    list->SetPipelineState(source.owned.state.original.Get());
                    if(!w.referenceBank.BindSource(list,index) || !w.referenceGeometry.RecordSourceGeometry(list,index))return 7;
                    ++CyberpunkVR_PreparedNativeRate[2];
                }
            }
        }
        list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
        for(UINT query=0;query<3;++query){
            list->ResolveQueryData(w.pipelineQueries.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,query,1,w.queryReadback.Get(),query*QueryStride);
            list->ResolveQueryData(w.occlusionQueries.Get(),D3D12_QUERY_TYPE_OCCLUSION,query,1,w.queryReadback.Get(),query*QueryStride+sizeof(D3D12_QUERY_DATA_PIPELINE_STATISTICS));
        }
        list->CopyBufferRegion(w.queryReadback.Get(),PredicateReadbackOffset,w.predicate.Get(),0,8);
        list->CopyBufferRegion(w.queryReadback.Get(),MeshReadbackOffset,w.mesh.Predicates(),0,UINT64(w.mesh.PairCount())*16);
        if(w.inputs.Predicates())list->CopyBufferRegion(w.queryReadback.Get(),InputReadbackOffset,w.inputs.Predicates(),0,16);
        if(w.actualMembership){const uint64_t predicateBytes=w.membership.SourceCount()*16ull;
            list->CopyBufferRegion(w.membershipReadback.Get(),0,w.membership.Predicates(),0,predicateBytes);
            list->CopyBufferRegion(w.membershipReadback.Get(),predicateBytes,w.membership.Results(),0,w.membership.SourceCount()*32ull);
        }
    }
    if(!ImageCopies(w,list))return 9;++CyberpunkVR_PreparedBatchStats[13];return 0;
}
bool RecordCurrent(){
    auto& w=*work;probe::Scope internal;
    if(nextOwnedSerial==UINT64_MAX || FAILED(w.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&w.ownedAllocator))) ||
        FAILED(w.device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,w.ownedAllocator.Get(),nullptr,IID_PPV_ARGS(&w.ownedList))) ||
        FAILED(w.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&w.fence)))){FailCurrent(36);return false;}
    w.ownedSerial=++nextOwnedSerial;CyberpunkVR_PreparedPairStats[12]=reinterpret_cast<uintptr_t>(w.ownedList.Get());
    CyberpunkVR_PreparedPairStats[13]=w.ownedSerial;
    ComPtr<ID3D12GraphicsCommandList1> views;ComPtr<ID3D12GraphicsCommandList5> rate;
    if(FAILED(w.ownedList.As(&views)) || !gpu::KeepCommandResources(w.ownedList.Get(),{w.ownedAllocator.Get()})){FailCurrent(36);return false;}
    w.ownedList.As(&rate);const auto reason=RecordCommands(w,w.ownedList.Get(),views.Get(),rate.Get());
    if(reason){FailCurrent(reason);return false;}
    CyberpunkVR_PreparedCurrent[2]=w.sourceFrame;CyberpunkVR_PreparedCurrent[4]=1;
    if(FAILED(w.ownedList->Close())){FailCurrent(38);return false;}
    w.closed=true;CyberpunkVR_PreparedCurrent[5]=1;Publish();return true;
}
}
bool PrepareNativeExport(ID3D12CommandQueue* queue,UINT frame,const target_access::Receipt& receipt,
                         const std::array<target_lifetime::BorrowedScope,8>& lifetimes)try{
    std::lock_guard lock(mutex);
    if(!work || !work->current || !work->exportRequested || work->sourceFrame!=frame)return false;
    auto& w=*work;
    if(w.exportPrepared || w.executionIssued || w.submitted)return false;
    if(!w.closed || w.failed || w.queue.Get()!=queue ||
        !w.sequence || !w.nativeExits[0] || !w.nativeExits[1] || !w.allowVrsRefinement)return RefuseNativeExport(w,1);
    std::array<target_handoff::SourceSnapshot,2> targets;
    if(!target_access::Describe(receipt,queue,frame,targets))return RefuseNativeExport(w,2);
    NativeOutputCopy::Targets physical;std::array<gpu::residency::BorrowedTarget,8> resident;
    for(UINT side=0;side<2;++side){const auto& source=targets[side];
        if(source.side!=side || source.frame!=frame || source.recording.list!=w.nativeExits[1-side]->list ||
            source.recording.generation!=w.nativeExits[1-side]->generation)return RefuseNativeExport(w,3);
        for(UINT i=0;i<4;++i){const auto& target=source.targets[i];
            if(target.slot!=(i==3?4:i) || !target_lifetime::Validate(lifetimes[side*4+i]))return RefuseNativeExport(w,4);
            physical[side][i]={target.identity.creation,target.planes,target.finalStates};
            resident[side*4+i]={target.identity,lifetimes[side*4+i]};
        }
    }
    const auto acquired=w.targetResidency.AcquireBorrowedTargets(w.device.Get(),resident);
    CyberpunkVR_PreparedNativeExport[19]=UINT(acquired);CyberpunkVR_PreparedNativeExport[15]=SUCCEEDED(acquired);
    if(FAILED(acquired))return RefuseNativeExport(w,5);
    CyberpunkVR_PreparedNativeExport[14]=w.targetResidency.ObjectCount();
    probe::Scope internal;
    auto copy=std::make_unique<NativeOutputCopy>();NativeOutputCopy::Arrays arrays;
    for(UINT i=0;i<4;++i)arrays[i]=w.pairedTargets.Resource(i==3?4:i);
    if(FAILED(copy->Create(w.device.Get(),arrays,physical,DrawPermission(w),0)))return RefuseNativeExport(w,6);
    if(FAILED(w.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&w.exportAllocator))) ||
        FAILED(w.device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,w.exportAllocator.Get(),nullptr,IID_PPV_ARGS(&w.exportList))))
        return RefuseNativeExport(w,7);
    if(!gpu::KeepCommandResources(w.exportList.Get(),{w.exportAllocator.Get(),w.fence.Get(),DrawPermission(w),copy->Readback()}))
        return RefuseNativeExport(w,8);
    for(const auto& array:arrays)if(!gpu::KeepCommandResources(w.exportList.Get(),{array.Get()}))return RefuseNativeExport(w,8);
    for(const auto& eye:physical)for(const auto& target:eye)
        if(!gpu::KeepCommandResources(w.exportList.Get(),{target.creation.resource.Get(),target.creation.heap.Get()}))return RefuseNativeExport(w,8);
    if(!copy->Record(w.exportList.Get()) || FAILED(w.exportList->Close()))return RefuseNativeExport(w,9);
    w.outputCopy=std::move(copy);w.exportReceipt=receipt;w.exportLifetimes=lifetimes;w.exportPrepared=true;
    CyberpunkVR_PreparedNativeExport[1]=1;CyberpunkVR_PreparedNativeExport[4]=w.outputCopy->Planes().size();
    CyberpunkVR_PreparedNativeExport[5]=w.outputCopy->Bytes();
    CyberpunkVR_PreparedNativeExport[13]=reinterpret_cast<uintptr_t>(w.exportList.Get());return true;
}catch(...){std::lock_guard lock(mutex);if(work)RefuseNativeExport(*work,10);return false;}
bool SubmitCurrent(ID3D12CommandQueue* queue,UINT frame)try{
    std::lock_guard lock(mutex);
    if(!work || !Active() || !work->current)return false;auto& w=*work;
    if(w.submitted || w.executionIssued)return false;
    if(!queue || w.queue.Get()!=queue || frame!=w.sourceFrame || !w.closed || !w.ownedList || !w.ownedAllocator || !w.fence || w.failed || w.stage!=4){
        FailCurrent(39);return false;}
    const auto original=cvr::detail::command_queue_original_execute(queue);
    if(!original){FailCurrent(40);return false;}
    for(const auto& source:w.sources){
        if(source.sourceQueue.Get()!=queue || source.owned.draw.frame!=frame || source.original.draw.frame!=frame){FailCurrent(39);return false;}
        for(const auto& material:source.materials){
            if(material.buffer && !MaterialMatches(w,source,material)){FailCurrent(39);return false;}
            if(material.immutable.owner && !texture_read::ValidateImmutable(material.immutable,queue)){FailCurrent(12);return false;}
        }
    }
    if(w.exportRequested){
        if(!w.exportPrepared)RefuseNativeExport(w,11);
        else{
            CyberpunkVR_PreparedNativeExport[20]=target_access::Validate(w.exportReceipt,queue,frame);
            bool live=true;for(const auto& lifetime:w.exportLifetimes)live=target_lifetime::Validate(lifetime) && live;
            CyberpunkVR_PreparedNativeExport[21]=live;
            if(!CyberpunkVR_PreparedNativeExport[20] || !live)RefuseNativeExport(w,12);
        }
    }
    void* mapped{};const D3D12_RANGE none{};
    if(FAILED(w.predicate->Map(0,&none,&mapped)) || !mapped){FailCurrent(23);return false;}
    *static_cast<uint64_t*>(mapped)=1;_mm_sfence();const D3D12_RANGE written{0,8};w.predicate->Unmap(0,&written);
    CyberpunkVR_PreparedPairDepthState[13]=1;
    ID3D12CommandList* lists[]{w.ownedList.Get(),w.exportList.Get()};
    const UINT listCount=w.exportPrepared?2u:1u;
    w.ownedFlight=gpu::PrepareCommandResources(listCount,lists);
    if(gpu::SubmissionEmpty(w.ownedFlight)){FailCurrent(4);return false;}
    auto resources=w.ownedFlight;
    std::array<gpu::residency::Lease*,2> leases{};UINT leaseCount{};
    if(w.residency.SourceCount())leases[leaseCount++]=&w.residency;
    if(w.exportPrepared)leases[leaseCount++]=&w.targetResidency;
    if(leaseCount){
        const auto resident=gpu::residency::Lease::MarkSubmittedTogether(queue,{leases.data(),leaseCount});
        if(w.residency.SourceCount()){
            CyberpunkVR_PreparedPairResidency[6]=UINT(resident);CyberpunkVR_PreparedPairResidency[3]=SUCCEEDED(resident);
            CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());
        }
        if(w.exportPrepared){CyberpunkVR_PreparedNativeExport[19]=UINT(resident);CyberpunkVR_PreparedNativeExport[16]=SUCCEEDED(resident);}
        if(FAILED(resident)){
            if(w.exportPrepared)CyberpunkVR_PreparedNativeExport[11]=13;
            FailCurrent(w.exportPrepared?44:33);return false;
        }
    }
    HRESULT signal{};
    {
        std::lock_guard order(constants::RegisteredQueueOrderMutex());probe::Scope internal;
        w.executionIssued=true;original(queue,listCount,lists);
        w.exportSubmitted=w.exportPrepared;CyberpunkVR_PreparedNativeExport[2]=w.exportSubmitted;
        CyberpunkVR_PreparedCurrent[3]=frame;CyberpunkVR_PreparedCurrent[6]=1;
        signal=queue->Signal(w.fence.Get(),1);
    }
    HRESULT resident=S_OK;
    if(w.residency.SourceCount()){
        resident=w.residency.AssociateFence(queue,w.fence.Get(),1,signal);
        CyberpunkVR_PreparedPairResidency[6]=UINT(resident);CyberpunkVR_PreparedPairResidency[4]=SUCCEEDED(resident);
        CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());CyberpunkVR_PreparedPairResidency[7]=gpu::residency::QuarantinedCount();
    }
    HRESULT targetResident=S_OK;
    if(w.exportSubmitted){
        targetResident=w.targetResidency.AssociateFence(queue,w.fence.Get(),1,signal);
        CyberpunkVR_PreparedNativeExport[19]=UINT(targetResident);CyberpunkVR_PreparedNativeExport[17]=SUCCEEDED(targetResident);
    }
    gpu::SubmitCommandResources(queue,std::move(resources));
    if(FAILED(signal) || FAILED(resident) || FAILED(targetResident)){FailCurrent(13);return false;}
    w.submitted=true;Publish();return true;
}catch(...){std::lock_guard lock(mutex);FailCurrent(41);return false;}
void Draw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& current)try{
    if(probe::internalCommands || !Active())return;
    std::lock_guard lock(mutex);if(!work || !Active() || work->current)return;auto& w=*work;if(w.stage!=3)return;
    if(current.rtCount!=3 || !current.depth || (current.flags&191)!=191 || current.flags&64 ||
        families::NativeGeometryStatus(reinterpret_cast<ID3D12PipelineState*>(current.pipeline))!=1)return;
    ++CyberpunkVR_PreparedCurrent[9];
    if(!CaptureDestination(list)){Error(1);w.complete=true;active=false;Publish();return;}
    Restore restore(list,current);if(!restore.valid){Error(3);w.complete=true;active=false;Publish();return;}
    std::set<IUnknown*> seen;
    for(const auto& source:w.sources)for(const auto& material:source.materials)if(material.immutable.owner && seen.insert(material.immutable.owner.Get()).second){
        texture_read::ImmutableUse use;if(!texture_read::RegisterImmutableUse(list,material.immutable,use)){Error(100+texture_read::LastFailure());w.complete=true;active=false;Publish();return;}
        w.uses.push_back(std::move(use));}
    CyberpunkVR_PreparedPairStats[14]=w.uses.size();
    const auto reason=RecordCommands(w,list,restore.views.Get(),restore.rate.Get());
    if(reason){Error(reason);if(w.stage==3){w.complete=true;active=false;}}Publish();
}catch(...){std::lock_guard lock(mutex);if(work && work->current)return;
    Error(10);if(work && work->stage==3){work->complete=true;active=false;}Publish();}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(probe::internalCommands || !Active() || !queue || !lists)return;std::lock_guard lock(mutex);if(!work || !Active() || work->current)return;auto& w=*work;
    if(w.submitted || w.stage!=4)return;UINT matches{};
    for(UINT n=0;n<count;++n)matches+=reinterpret_cast<uintptr_t>(lists[n])==w.recording.list;
    if(!matches)return;if(matches!=1 || !native_recording::ValidateSubmission(w.recording,queue))Error(11);
    for(const auto& use:w.uses)if(!texture_read::ValidateImmutableUse(use,queue))Error(12);
    if(w.residency.SourceCount()){
        const auto resident=w.residency.MarkSubmitted(queue);CyberpunkVR_PreparedPairResidency[6]=UINT(resident);
        CyberpunkVR_PreparedPairResidency[3]=SUCCEEDED(resident);CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());
        if(FAILED(resident))Error(33);
    }
    if(w.stage==4){void* mapped{};const D3D12_RANGE none{};
        if(FAILED(w.predicate->Map(0,&none,&mapped)) || !mapped)Error(23);
        else{CyberpunkVR_PreparedPairDepthState[13]=w.failed?0:1;*static_cast<uint64_t*>(mapped)=CyberpunkVR_PreparedPairDepthState[13];
            _mm_sfence();const D3D12_RANGE written{0,8};w.predicate->Unmap(0,&written);}}
    w.queue=queue;
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(probe::internalCommands || !Active() || !queue || !lists)return;std::lock_guard lock(mutex);if(!work || !Active() || work->current)return;auto& w=*work;
    if(w.submitted || w.queue.Get()!=queue)return;
    for(UINT n=0;n<count;++n)if(reinterpret_cast<uintptr_t>(lists[n])==w.recording.list){
        const auto signal=queue->Signal(w.fence.Get(),1);
        HRESULT resident=S_OK;
        if(w.residency.SourceCount()){
            resident=w.residency.AssociateFence(queue,w.fence.Get(),1,signal);
            CyberpunkVR_PreparedPairResidency[6]=UINT(resident);CyberpunkVR_PreparedPairResidency[4]=SUCCEEDED(resident);
            CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());CyberpunkVR_PreparedPairResidency[7]=gpu::residency::QuarantinedCount();
        }
        if(FAILED(signal) || FAILED(resident)){
            w.residency.Quarantine();CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());
            CyberpunkVR_PreparedPairResidency[7]=gpu::residency::QuarantinedCount();Error(13);w.complete=true;active=false;}
        else w.submitted=true;Publish();return;}
}
void Reset(ID3D12GraphicsCommandList* list){
    if(probe::internalCommands || !Active())return;
#ifdef CVR_PREPARED_PAIR_TESTING
    testResetEntered=true;testResetEntered.notify_one();
#endif
    std::lock_guard lock(mutex);
    if(!work || !Active() || work->current)return;
    if(work->recording.list==reinterpret_cast<uintptr_t>(list) && !work->submitted && work->stage==4){
        CancelUnsubmittedResidency(*work);Error(14);work->complete=true;active=false;Publish();}
}
bool PollMembership(Work& w){
    if(!w.actualMembership)return true;auto* stats=CyberpunkVR_PreparedActualMembershipStats;
    const UINT count=w.membership.SourceCount();
    if(!count || count!=w.sources.size() || count!=CyberpunkVR_PreparedActualMembershipCount || !w.membershipReadback)
        return MembershipError(6);
    const D3D12_RANGE range{0,SIZE_T(w.membership.ReadbackBytes())};void* mapped{};
    if(FAILED(w.membershipReadback->Map(0,&range,&mapped)) || !mapped)return MembershipError(8);
    const auto* bytes=static_cast<const uint8_t*>(mapped);bool valid=true;
    for(UINT i=0;i<count;++i){auto* row=CyberpunkVR_PreparedActualMembershipRows+i*32;std::array<UINT,8> values{};
        std::memcpy(values.data(),bytes+count*16ull+i*32ull,32);for(UINT n=0;n<8;++n)row[16+n]=values[n];
        std::memcpy(row+24,bytes+i*16ull,16);
        valid=valid && row[16]==1 && row[24]<=1 && row[25]<=1 && row[24]+row[25]==1 &&
            row[24]==uint64_t(row[17]==0) && !(row[17]&~UINT64_C(31)) && row[20]<=32 && row[21]==row[20]*5;
        stats[10]+=row[16];stats[11]+=row[24];stats[12]+=row[25];stats[13]+=row[20];stats[14]+=row[21];
    }
    const D3D12_RANGE none{};w.membershipReadback->Unmap(0,&none);stats[9]=1;
    valid=valid && CyberpunkVR_PreparedPairInputStats[5]==uint64_t(stats[11]==count) &&
        CyberpunkVR_PreparedPairInputStats[6]==uint64_t(stats[12]!=0);
    return valid?true:MembershipError(9);
}
void Poll()try{
    if(!Active())return;std::lock_guard lock(mutex);if(!work || !Active())return;auto& w=*work;if(!w.submitted || !w.fence)return;
    const auto completed=w.fence->GetCompletedValue();if(completed<1)return;
    if(completed==UINT64_MAX){if(w.current){FailCurrent(15);return;}Error(15);}
    else if(w.current)CyberpunkVR_PreparedCurrent[7]=1;
    if(w.exportSubmitted){
        CyberpunkVR_PreparedNativeExport[3]=1;
        const auto retired=w.targetResidency.Retire();CyberpunkVR_PreparedNativeExport[19]=UINT(retired);
        CyberpunkVR_PreparedNativeExport[18]=retired==S_OK;
        if(retired!=S_OK){CyberpunkVR_PreparedNativeExport[11]=14;Error(45);}
    }
    if(w.residency.SourceCount()){
        const auto retired=w.residency.Retire();CyberpunkVR_PreparedPairResidency[6]=UINT(retired);
        CyberpunkVR_PreparedPairResidency[2]=UINT(w.residency.Status());CyberpunkVR_PreparedPairResidency[5]=retired==S_OK;
        CyberpunkVR_PreparedPairResidency[7]=gpu::residency::QuarantinedCount();
        if(retired!=S_OK)Error(34);
    }
    if(!w.failed && w.stage==4){
        if(w.rates.Image()){
            std::vector<uint8_t> actual,expected,left,right;UINT invalid{};
            bool valid=SUCCEEDED(w.rates.Read(actual,invalid)) && !invalid;CyberpunkVR_PreparedPairVrs[7]=invalid;
            bool mapMetadata=true,finer=true;std::array<bool,2> uniform{true,true};std::array<UINT,2> mapCounts{};
            for(UINT i=0;valid && i<w.sources.size();++i){const auto& source=w.sources[i];std::vector<uint8_t> pixels,refined;
                valid=source.rateCapture && SUCCEEDED(source.rateCapture->ReadEye(0,pixels));if(!valid)break;
                if(i==w.eyes[0].front())left=pixels;if(i==w.eyes[1].front())right=pixels;
                if(w.nativeExits[0]){const UINT eye=1-source.owned.draw.side;
                    mapMetadata=mapMetadata && source.rateCapture->Width()==w.rateWidth && source.rateCapture->Height()==w.rateHeight &&
                        pixels.size()==size_t(w.rateWidth)*w.rateHeight;
                    if(!mapCounts[eye])w.originalRates[eye]=pixels;else uniform[eye]=uniform[eye] && w.originalRates[eye]==pixels;
                    ++mapCounts[eye];finer=finer && native_exit_detail::AlwaysFiner(actual,pixels);
                }
                if(expected.empty())expected=std::move(pixels);
                else{valid=BuildFinerVrs(expected,pixels,refined);expected=std::move(refined);}
            }
            if(valid && actual.size()==expected.size() && left.size()==actual.size() && right.size()==actual.size()){
                for(size_t i=0;i<actual.size();++i){CyberpunkVR_PreparedPairVrsGpu[3]+=actual[i]!=expected[i];
                    CyberpunkVR_PreparedPairVrs[3]+=left[i]!=right[i];CyberpunkVR_PreparedPairVrs[4]+=actual[i]!=left[i];CyberpunkVR_PreparedPairVrs[5]+=actual[i]!=right[i];}
                valid=!CyberpunkVR_PreparedPairVrsGpu[3];
            }else valid=false;
            if(w.nativeExits[0]){
                const bool allMaps=mapMetadata && mapCounts[0]==w.eyes[0].size() && mapCounts[1]==w.eyes[1].size();
                for(UINT eye=0;eye<2;++eye)CyberpunkVR_PreparedVrsAttribution[3+eye]=
                    mapCounts[eye]==w.eyes[eye].size() && uniform[eye];
                CyberpunkVR_PreparedVrsAttribution[9]=valid && allMaps;
                CyberpunkVR_PreparedVrsAttribution[10]=valid && allMaps && finer;
                w.rateMapsReady=valid && allMaps && finer && uniform[0] && uniform[1] &&
                    native_exit_detail::RateMapShape(w.width,w.height,w.rateTile,w.rateWidth,w.rateHeight,actual.size());
                if(w.rateMapsReady)w.commonRates=std::move(actual);
            }
            if(!valid)Error(32);
        }
        void* mapped{};const D3D12_RANGE range{0,SIZE_T(w.imageBytes)};
        const D3D12_RANGE queryRange{0,SIZE_T(w.queryReadback->GetDesc().Width)};void* queryData{};
        if(FAILED(w.queryReadback->Map(0,&queryRange,&queryData)) || !queryData)Error(26);
        else{std::memcpy(CyberpunkVR_PreparedPairGpu,queryData,PredicateReadbackOffset);CyberpunkVR_PreparedBaseline[23]=1;
            std::memcpy(CyberpunkVR_PreparedPairDepthState+14,static_cast<const uint8_t*>(queryData)+PredicateReadbackOffset,8);
            std::memcpy(CyberpunkVR_PreparedPairMeshResults,static_cast<const uint8_t*>(queryData)+MeshReadbackOffset,SIZE_T(w.mesh.PairCount())*16);
            if(w.inputs.Predicates()){
                std::memcpy(CyberpunkVR_PreparedPairInputStats+5,static_cast<const uint8_t*>(queryData)+InputReadbackOffset,16);
                if(CyberpunkVR_PreparedPairInputStats[5]!=1 || CyberpunkVR_PreparedPairInputStats[6])Error(29);
            }
            for(UINT i=0;i<w.mesh.PairCount();++i){const auto equal=CyberpunkVR_PreparedPairMeshResults[i*2],fallback=CyberpunkVR_PreparedPairMeshResults[i*2+1];
                if(equal>1 || fallback>1 || equal+fallback>1)Error(28);
                CyberpunkVR_PreparedPairMeshStats[4]+=equal;CyberpunkVR_PreparedPairMeshStats[5]+=fallback;CyberpunkVR_PreparedPairMeshStats[6]+=equal+fallback==0;}
            CyberpunkVR_PreparedBatchStats[9]=CyberpunkVR_PreparedPairMeshStats[5];
            CyberpunkVR_PreparedPairStats[3]=CyberpunkVR_PreparedPairMeshStats[4]>0 && !CyberpunkVR_PreparedPairMeshStats[6];
            const bool inputsAccepted=!w.inputs.Predicates() || (CyberpunkVR_PreparedPairInputStats[5]==1 && !CyberpunkVR_PreparedPairInputStats[6]);
            CyberpunkVR_PreparedBaseline[22]=!inputsAccepted;
            if(w.nativeExits[0])CyberpunkVR_PreparedNativeExit[22]=!inputsAccepted;
            if(w.nativeExits[0])CyberpunkVR_PreparedNativeRate[15]=!inputsAccepted;
            CyberpunkVR_PreparedBaseline[19]=!inputsAccepted && std::all_of(std::begin(CyberpunkVR_PreparedPairGpu),std::end(CyberpunkVR_PreparedPairGpu),[](auto value){return value==0;});
            bool executed=inputsAccepted;
            for(UINT query=0;query<3;++query)executed=executed && CyberpunkVR_PreparedPairGpu[query*12+2] &&
                CyberpunkVR_PreparedPairGpu[query*12+7] && CyberpunkVR_PreparedPairGpu[query*12+11];
            CyberpunkVR_PreparedBaseline[20]=executed;
            CyberpunkVR_PreparedPairStats[5]=inputsAccepted?w.plan.draws.size()+CyberpunkVR_PreparedPairMeshStats[5]:0;
            if(!inputsAccepted)CyberpunkVR_PreparedPairStats[4]=0;
            if(!CyberpunkVR_PreparedPairStats[3] || (!w.sequence && CyberpunkVR_PreparedPairMeshStats[5]))Error(17);
            const D3D12_RANGE none{};w.queryReadback->Unmap(0,&none);}
        if(!PollMembership(w))Error(47);
        if(!w.imageReadback || FAILED(w.imageReadback->Map(0,&range,&mapped)) || !mapped)Error(19);
        else{
            for(const auto& plane:w.planes){
                const UINT delta=plane.slot<w.colors?6:plane.plane?14:10;
                for(UINT y=0;y<plane.rows;++y){
                    const auto* a=static_cast<const uint8_t*>(mapped)+plane.layouts[0].Offset+uint64_t(y)*plane.layouts[0].Footprint.RowPitch+plane.component;
                    const auto* b=static_cast<const uint8_t*>(mapped)+plane.layouts[1].Offset+uint64_t(y)*plane.layouts[1].Footprint.RowPitch+plane.component;
                    const auto* baseline=static_cast<const uint8_t*>(mapped)+plane.layouts[2].Offset+uint64_t(y)*plane.layouts[2].Footprint.RowPitch+plane.component;
                    for(UINT x=0;x<w.width;++x){const auto offset=size_t(x)*plane.stride;
                        const bool referenceChanged=std::memcmp(a+offset,baseline+offset,plane.bytes)!=0;
                        CyberpunkVR_PreparedPairStats[6+plane.eye]+=std::memcmp(a+offset,b+offset,plane.bytes)!=0;
                        CyberpunkVR_PreparedBaseline[delta+plane.eye]+=referenceChanged;
                        CyberpunkVR_PreparedBaseline[delta+2+plane.eye]+=std::memcmp(b+offset,baseline+offset,plane.bytes)!=0;
                        if(plane.slot<w.colors){uint32_t reference{},paired{},before{};
                            std::memcpy(&reference,a+offset,4);std::memcpy(&paired,b+offset,4);std::memcpy(&before,baseline+offset,4);
                            CyberpunkVR_PreparedPairColor[plane.eye*3+plane.slot]+=reference!=0;
                            CyberpunkVR_PreparedPairColor[6+plane.eye*3+plane.slot]+=paired!=0;
                            CyberpunkVR_PreparedPairStats[8+plane.eye]+=referenceChanged;CyberpunkVR_PreparedBaseline[5]+=before!=0;}
                    }
                }
                ++CyberpunkVR_PreparedBaseline[3];CyberpunkVR_PreparedBaseline[4]+=uint64_t(w.width)*plane.rows*plane.bytes;
            }
            CyberpunkVR_PreparedBaseline[21]=w.planes.size()==2*(w.colors+2) && CyberpunkVR_PreparedBaseline[3]==w.planes.size();
            CompareNativeExit(w,{static_cast<const uint8_t*>(mapped),SIZE_T(w.imageBytes)});
            if(!CompareNativeOutput(w,{static_cast<const uint8_t*>(mapped),SIZE_T(w.imageBytes)})){
                if(!CyberpunkVR_PreparedNativeExport[11])CyberpunkVR_PreparedNativeExport[11]=15;Error(46);
            }
            const D3D12_RANGE none{};w.imageReadback->Unmap(0,&none);
        }
        if(CyberpunkVR_PreparedPairStats[6] || CyberpunkVR_PreparedPairStats[7])Error(20);
        if(CyberpunkVR_PreparedBaseline[22]){
            CyberpunkVR_PreparedBaseline[18]=CyberpunkVR_PreparedBaseline[21] &&
                std::all_of(CyberpunkVR_PreparedBaseline+6,CyberpunkVR_PreparedBaseline+18,[](auto value){return value==0;});
            if(!CyberpunkVR_PreparedBaseline[18] || !CyberpunkVR_PreparedBaseline[19])Error(42);
        }else{
            bool contribution=CyberpunkVR_PreparedBaseline[21] && CyberpunkVR_PreparedBaseline[20];
            for(UINT eye=0;eye<2;++eye)contribution=contribution &&
                (CyberpunkVR_PreparedBaseline[6+eye] || CyberpunkVR_PreparedBaseline[10+eye] || CyberpunkVR_PreparedBaseline[14+eye]);
            if(!contribution)Error(21);
        }
    }
    w.complete=true;active=false;Publish();
    // Results live in the exported POD arrays. GPU completion above permits
    // dropping descriptor banks and source pins before the next native capture.
    work.reset();
}catch(...){std::lock_guard lock(mutex);
    if(work && work->current && !CyberpunkVR_PreparedCurrent[7]){FailCurrent(22);return;}
    Error(22);if(work)work->complete=true;active=false;Publish();if(work && work->current)work.reset();}
#ifdef CVR_PREPARED_PAIR_TESTING
bool TestNativeBindingModes()try{
    struct RestoreTest {
        std::unique_ptr<Work> previous;bool captured{},wasActive{};
        std::vector<std::pair<std::span<uint64_t>,std::vector<uint64_t>>> arrays;
        std::array<std::atomic<uint32_t>*,7> atomics{&CyberpunkVR_PreparedPairRequest,&CyberpunkVR_PreparedPairState,
            &CyberpunkVR_PreparedPairSelection,&CyberpunkVR_PreparedPairSelectionCount,&CyberpunkVR_PreparedPairInputFault,
            &CyberpunkVR_PreparedNativeExportEnable,&CyberpunkVR_PreparedActualMembershipEnable};
        std::array<uint32_t,7> values{};uint32_t sourceCount{},planCount{},membershipCount{};
        ~RestoreTest(){if(!captured)return;std::lock_guard lock(mutex);active=false;work=std::move(previous);
            for(const auto& saved:arrays)std::copy(saved.second.begin(),saved.second.end(),saved.first.begin());
            for(size_t i=0;i<atomics.size();++i)*atomics[i]=values[i];
            CyberpunkVR_PreparedBatchSourceCount=sourceCount;CyberpunkVR_PreparedBatchPlanCount=planCount;
            CyberpunkVR_PreparedActualMembershipCount=membershipCount;active=wasActive;
        }
    } saved;
    {std::lock_guard lock(mutex);if(Active())return false;
        const std::array<std::span<uint64_t>,24> arrays{CyberpunkVR_PreparedPairStats,CyberpunkVR_PreparedCurrent,CyberpunkVR_PreparedBaseline,
            CyberpunkVR_PreparedPairCandidates,CyberpunkVR_PreparedPairStateMismatch,CyberpunkVR_PreparedPairGpu,
            CyberpunkVR_PreparedPairColor,CyberpunkVR_PreparedPairDepthState,CyberpunkVR_PreparedPairMeshStats,
            CyberpunkVR_PreparedPairMeshResults,CyberpunkVR_PreparedPairVrs,CyberpunkVR_PreparedPairVrsGpu,
            CyberpunkVR_PreparedPairInputStats,CyberpunkVR_PreparedPairResidency,CyberpunkVR_PreparedBatchStats,CyberpunkVR_PreparedBatchDescriptors,
            CyberpunkVR_PreparedNativeExit,CyberpunkVR_PreparedNativeExitSamples,CyberpunkVR_PreparedNativeRate,CyberpunkVR_PreparedVrsAttribution,
            CyberpunkVR_PreparedNativeAcceptance,CyberpunkVR_PreparedNativeExport,CyberpunkVR_PreparedActualMembershipStats,
            CyberpunkVR_PreparedActualMembershipRows};
        for(const auto values:arrays)saved.arrays.emplace_back(values,std::vector<uint64_t>(values.begin(),values.end()));
        for(size_t i=0;i<saved.atomics.size();++i)saved.values[i]=saved.atomics[i]->load();
        saved.sourceCount=CyberpunkVR_PreparedBatchSourceCount;saved.planCount=CyberpunkVR_PreparedBatchPlanCount;
        saved.membershipCount=CyberpunkVR_PreparedActualMembershipCount;
        saved.wasActive=active.load();saved.previous=std::move(work);active=false;saved.captured=true;
    }
    bool valid=!NeedsNativeBindings();
    {std::lock_guard lock(mutex);work=std::make_unique<Work>();active=true;}
    valid=NeedsNativeBindings() && valid;
    const auto state=CyberpunkVR_PreparedPairState.load();const auto mode=CyberpunkVR_PreparedCurrent[0];
    valid=!StartCurrentSequence(nullptr,{},nullptr,0) && NeedsNativeBindings() &&
        CyberpunkVR_PreparedPairState.load()==state && CyberpunkVR_PreparedCurrent[0]==mode && valid;
    {std::lock_guard lock(mutex);work->current=true;}
    valid=!NeedsNativeBindings() && valid;
    for(UINT repeated=0;repeated<3;++repeated){
        NativeOutputCopy* copy{};
        {std::lock_guard lock(mutex);work->sourceFrame=73;work->exportRequested=true;
            work->exportPrepared=repeated==0;work->executionIssued=repeated==1;work->submitted=repeated==2;
            work->outputCopy=std::make_unique<NativeOutputCopy>();copy=work->outputCopy.get();
            std::fill_n(CyberpunkVR_PreparedNativeExport,24,17);
        }
        valid=!PrepareNativeExport(nullptr,73,{}, {}) && valid;
        {std::lock_guard lock(mutex);
            valid=work->outputCopy.get()==copy && work->exportPrepared==(repeated==0) &&
                work->executionIssued==(repeated==1) && work->submitted==(repeated==2) &&
                std::all_of(std::begin(CyberpunkVR_PreparedNativeExport),std::end(CyberpunkVR_PreparedNativeExport),
                    [](auto value){return value==17;}) && valid;
        }
    }
    {std::lock_guard lock(mutex);active=false;work.reset();}
    const std::array<std::pair<ID3D12CommandQueue*,UINT>,3> invalid{{{nullptr,0},{nullptr,7},
        {reinterpret_cast<ID3D12CommandQueue*>(uintptr_t(1)),0}}};
    for(const auto& [queue,frame]:invalid){
        {std::lock_guard lock(mutex);std::fill_n(CyberpunkVR_PreparedCurrent,16,UINT64_MAX);
            CyberpunkVR_PreparedPairStats[10]=99;CyberpunkVR_PreparedPairState=5;}
        valid=!StartCurrentSequence(nullptr,{},queue,frame) && !Active() && !NeedsNativeBindings() &&
            CyberpunkVR_PreparedCurrent[0]==1 && CyberpunkVR_PreparedCurrent[1]==frame &&
            !CyberpunkVR_PreparedCurrent[4] && !CyberpunkVR_PreparedCurrent[5] && !CyberpunkVR_PreparedCurrent[6] && !CyberpunkVR_PreparedCurrent[7] &&
            CyberpunkVR_PreparedCurrent[12]==35 && !CyberpunkVR_PreparedCurrent[13] &&
            CyberpunkVR_PreparedPairStats[10]==35 && CyberpunkVR_PreparedPairState.load()==6 && valid;
    }
    return valid;
}catch(...){return false;}
bool TestReleasedResetRace(){
    std::thread callback;
    {std::unique_lock lock(mutex);if(active || work)return false;
        work=std::make_unique<Work>();active=true;testResetEntered=false;
        callback=std::thread([]{Reset(nullptr);});
        testResetEntered.wait(false);
        active=false;work.reset();
    }
    callback.join();return !active && !work;
}
#endif
}
#endif
