#include "Render/StereoDescriptorAudit.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/ObservationProfile.hpp"
#include "Render/SourceDescriptorViews.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeTextureRead.hpp"
#include <cmath>
#include <wrl/client.h>
#include <bit>
#include <cstring>
#include <mutex>
#include <map>
#include <unordered_map>
#include <set>
#include <cfloat>
using Microsoft::WRL::ComPtr;
#if CVR_RUST_VRIK
extern "C" {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
__declspec(dllexport) uint32_t CyberpunkVR_DescriptorBindingBytes=sizeof(cvr::stereo::descriptors::Binding),CyberpunkVR_DescriptorValueBytes=sizeof(cvr::stereo::descriptors::Value);
#endif
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
__declspec(dllexport) cvr::stereo::descriptors::RootInfo CyberpunkVR_DescriptorRoots[128]{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's exports are rust/crates/cvr-render/src/render_moved_exports.rs'
// The observation profile's header exports (inline: emitted only where C++ names them) -- read by Rust.
extern const void* const CyberpunkVR_ObservationProfileRefs[]{&CyberpunkVR_ObservationProfileEnable,CyberpunkVR_ObservationProfileStats};
#endif
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's audit runtime is descriptor_audit.rs' (mod plugin)
// What the fixed ranges still call: the retired lifetimes drained, the fixed range snapshot and its check.
extern "C" void cvr_sda_drain(void(*)(uint64_t));
extern "C" void cvr_sda_fixed_snapshot(const void*,const void*,cvr::stereo::descriptors::Value*,uint64_t*);
extern "C" bool cvr_sda_fixed_canonical(const void*,const cvr::stereo::descriptors::Value*,size_t,const void*);
struct CvrDescriptorAuditRuntime {
    void* store;const void* tracking;bool(*observeBindings)();const std::atomic<uint32_t>* nativeProbeState;size_t drawCapacity;
    std::atomic<uint32_t>* captureFailure;std::atomic<uint32_t>* captureSeq;std::atomic<uint32_t>* denseSlots;std::atomic<uint32_t>* denseActive;
    std::atomic<uint32_t>* liveMetadata;std::atomic<uint32_t>* liveMetadataActive;std::atomic<uint32_t>* pendingLists;
    uint32_t* rootCount;cvr::stereo::descriptors::RootInfo* roots;size_t rootsCapacity;uint64_t* storeStats;uint64_t* heapSlots;
    uint64_t* draws;std::atomic<uint32_t>* drawCount;uint64_t* bindings;std::atomic<uint32_t>* bindingCount;uint64_t* ids;
    std::atomic<uint32_t>* idCount;uint64_t* values;std::atomic<uint32_t>* valueCount;
    const std::atomic<uint32_t>* profileEnable;std::atomic<uint64_t>* profileStats;
};
static_assert(sizeof(CvrDescriptorAuditRuntime)==27*8,"mirrored by descriptor_audit.rs");
extern "C" void cvr_sda_audit_attach(const CvrDescriptorAuditRuntime*);
extern "C" void cvr_sda_audit_lock(bool);
extern "C" uint64_t cvr_sda_audit_write(uint32_t,ID3D12Device*,uintptr_t,uint64_t,uint64_t,uint64_t,void*,void*);
extern "C" uint64_t cvr_sda_audit_held_cookie(ID3D12Resource*);
extern "C" void cvr_sda_audit_copy(ID3D12Device*,UINT,const uint64_t*,const UINT*,UINT,const uint64_t*,const UINT*,int32_t);
extern "C" bool cvr_sda_audit_capture(uint32_t,uint64_t,uint64_t,uint32_t);
extern "C" void cvr_sda_audit_snapshot(const void*);
extern "C" void cvr_sda_audit_submit(uint32_t,const uint64_t*);
extern "C" bool cvr_sda_audit_find_binding(uint64_t,uint32_t,uint32_t,uint32_t,uint32_t,uint32_t*,uint32_t*);
extern "C" bool cvr_sda_audit_cbv(uint32_t,uint64_t,uint64_t,uint32_t,uint32_t,void*);
extern "C" bool cvr_sda_audit_stamp(uint32_t,uint64_t,uint64_t,uint32_t,uint32_t,void*,IUnknown**);
extern "C" bool cvr_sda_audit_srv(uint64_t,uint64_t,uint32_t,uint32_t,void*,ID3D12Resource**,D3D12_SHADER_RESOURCE_VIEW_DESC*);
extern "C" bool cvr_sda_audit_sampler(uint64_t,uint64_t,uint32_t,uint32_t,void*,D3D12_SAMPLER_DESC*);
namespace cvr::stereo::descriptors {
namespace {
// The metadata lock is the Rust audit core's (rust/crates/cvr-render/src/descriptor_audit.rs), recursive as this was.
struct MetadataMutex {
    void lock(){cvr_sda_audit_lock(true);}
    void unlock(){cvr_sda_audit_lock(false);}
};
MetadataMutex mutex;
Store store;
BorrowedRangeTracking borrowedTracking;
// The audit core in Rust: the store's Rust half and this tracking object, the probe's switch and the exports it writes.
const CvrDescriptorAuditRuntime kAuditRuntime{const_cast<void*>(store.Rust()),&borrowedTracking,
    []{return native_probe::ObserveBindings();},&CyberpunkVR_NativeStereoProbeState,native_probe::DrawCapacity,
    &CyberpunkVR_DescriptorCaptureFailure,&CyberpunkVR_DescriptorCaptureSeq,&CyberpunkVR_DescriptorDenseSlots,&CyberpunkVR_DescriptorDenseActive,
    &CyberpunkVR_DescriptorLiveMetadata,&CyberpunkVR_DescriptorLiveMetadataActive,&CyberpunkVR_DescriptorPendingLists,
    &CyberpunkVR_DescriptorRootCount,CyberpunkVR_DescriptorRoots,std::size(CyberpunkVR_DescriptorRoots),CyberpunkVR_DescriptorStoreStats,
    &CyberpunkVR_DescriptorHeapSlots,&CyberpunkVR_DescriptorDraws,&CyberpunkVR_DescriptorDrawCount,&CyberpunkVR_DescriptorBindings,
    &CyberpunkVR_DescriptorBindingCount,&CyberpunkVR_DescriptorIds,&CyberpunkVR_DescriptorIdCount,&CyberpunkVR_DescriptorValues,
    &CyberpunkVR_DescriptorValueCount,&CyberpunkVR_ObservationProfileEnable,CyberpunkVR_ObservationProfileStats};
const bool kAuditAttached=(cvr_sda_audit_attach(&kAuditRuntime),true);
extern "C" void cvr_sda_bsr_stage(uint32_t,uint64_t,uint64_t,uint64_t,uint64_t);
extern "C" uint32_t cvr_sda_bsr_last(std::array<uint64_t,4>*);
void BorrowedStage(uint32_t value,uint64_t index=UINT64_MAX,uint64_t gpu=0,uint64_t expected=0,uint64_t actual=0){
    cvr_sda_bsr_stage(value,index,gpu,expected,actual);
}
// Object private data survives for exactly that COM object's lifetime. A reused
// address cannot inherit a resource/heap identity from a destroyed object.
constexpr GUID cookieGuid{0xc7ec4301,0x790e,0x4b32,{0x93,0x87,0x7c,0xc1,0x19,0x52,0xd1,0x44}};
// The known heaps are Rust's; the store removes a retired heap's slots through the callback.
// A known resource is the Rust lifetime (its layout mirrored here); its pointer stays valid under the metadata mutex,
// which every drain takes too.
void HeapRemoved(uint64_t cookie){store.HeapRemoved(cookie);}
void DrainRetired(){cvr_sda_drain(&HeapRemoved);}
// The tracking failure, the storage selection and the failure itself are the Rust core's.
}
bool Enabled(){return native_probe::ObserveBindings() && !probe::internalCommands;}
MutationScope::MutationScope():entered_(borrowedTracking.MutationBegin()){}
MutationScope::~MutationScope(){if(entered_)borrowedTracking.MutationEnd();}
void BorrowedRangeObserverReady(bool ready){borrowedTracking.Ready(ready);}
uint32_t BorrowedRangeLastFailure(){return cvr_sda_bsr_last(nullptr);}
std::array<uint64_t,4> BorrowedRangeLastDetails(){std::array<uint64_t,4> details{};cvr_sda_bsr_last(&details);return details;}
void Initialize(ID3D12Device* device){cvr_sda_audit_write(0,device,0,0,0,0,nullptr,nullptr);}
void HeapCreated(ID3D12Device* device,ID3D12DescriptorHeap* heap){cvr_sda_audit_write(1,device,0,0,0,0,heap,nullptr);}
void HeapsBound(ID3D12Device* device,UINT count,ID3D12DescriptorHeap* const* heaps){
    cvr_sda_audit_write(7,device,reinterpret_cast<uintptr_t>(heaps),count,0,0,nullptr,nullptr);
}
void RootCreated(ID3D12RootSignature* root,const void* bytes,size_t size){
    cvr_sda_audit_write(2,nullptr,reinterpret_cast<uintptr_t>(bytes),size,0,0,root,nullptr);
}
uint64_t SourceResourceCookieAtCreation(ID3D12Resource* resource){return cvr_sda_audit_write(8,nullptr,0,0,0,0,resource,nullptr);}
bool MatchesHeldResourceCookie(ID3D12Resource* resource,uint64_t cookie)noexcept{
    return resource && cookie && cvr_sda_audit_held_cookie(resource)==cookie;
}
bool GetHeldResourceCookie(const ComPtr<ID3D12Resource>& resource,uint64_t& cookie)noexcept{
    cookie=resource?cvr_sda_audit_held_cookie(resource.Get()):0;return cookie!=0;
}
void Cbv(const D3D12_CONSTANT_BUFFER_VIEW_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu,const CbvOrigin* origin){
    cvr_sda_audit_write(3,nullptr,reinterpret_cast<uintptr_t>(d),cpu.ptr,origin?origin->cookie:0,origin?origin->id:0,
        origin?origin->owner.Get():nullptr,nullptr);
}
void Srv(ID3D12Resource* resource,const D3D12_SHADER_RESOURCE_VIEW_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu){
    cvr_sda_audit_write(4,nullptr,reinterpret_cast<uintptr_t>(d),cpu.ptr,0,0,resource,nullptr);
}
void Uav(ID3D12Resource* resource,ID3D12Resource* counter,const D3D12_UNORDERED_ACCESS_VIEW_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu){
    cvr_sda_audit_write(5,nullptr,reinterpret_cast<uintptr_t>(d),cpu.ptr,0,0,resource,counter);
}
void Sampler(const D3D12_SAMPLER_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu){
    cvr_sda_audit_write(6,nullptr,reinterpret_cast<uintptr_t>(d),cpu.ptr,0,0,nullptr,nullptr);
}
void Copy(ID3D12Device* device,UINT n,const D3D12_CPU_DESCRIPTOR_HANDLE* dest,const UINT* sizes,UINT m,const D3D12_CPU_DESCRIPTOR_HANDLE* source,const UINT* counts,D3D12_DESCRIPTOR_HEAP_TYPE type){
    cvr_sda_audit_copy(device,n,reinterpret_cast<const uint64_t*>(dest),sizes,m,reinterpret_cast<const uint64_t*>(source),counts,int32_t(type));
}
void BeginCapture(){cvr_sda_audit_capture(0,0,0,0);}
void Snapshot(const native_probe::DrawRecord& draw){cvr_sda_audit_snapshot(&draw);}
void EndCapture(){cvr_sda_audit_capture(1,0,0,0);}
bool BeginBindings(){return cvr_sda_audit_capture(2,0,0,0);}
void EndBindings(){cvr_sda_audit_capture(3,0,0,0);}
bool Healthy(){return cvr_sda_audit_capture(4,0,0,0);}
// The table metadata snapshot: rust/crates/cvr-render/src/audit_table_snapshot.rs holds the root, device and heaps,
// the layout and the summary; the pimpl is its handle and the summary is copied out here.
struct RustTableSummary {
    uint64_t root,trackingEpoch;UINT rootParameters,rootFlags,layoutReasons,boundTables,unboundTables,unknownFlags;uint64_t unknownSlots;
    bool complete;const TableMetadataSummary::Range* ranges;size_t rangeCount;const Value* values;size_t valueCount;
};
static_assert(sizeof(RustTableSummary)==88 && sizeof(BoundTableMetadata)==16 && sizeof(ComPtr<ID3D12DescriptorHeap>)==sizeof(void*),
    "mirrored by audit_table_snapshot.rs");
extern "C" void* cvr_sda_tm_capture(ID3D12RootSignature*,ID3D12DescriptorHeap* const*,size_t,const BoundTableMetadata*,size_t);
extern "C" void cvr_sda_tm_free(void*);
extern "C" bool cvr_sda_tm_validate(const void*);
extern "C" bool cvr_sda_tm_describe(const void*,RustTableSummary*);
extern "C" uint32_t cvr_sda_tm_failure(uint32_t);
struct TableMetadataSnapshot::Data {
    void* rust{};
    explicit Data(void* handle):rust(handle){}
    ~Data(){cvr_sda_tm_free(rust);}
    Data(const Data&)=delete;
    Data& operator=(const Data&)=delete;
};
TableMetadataFailure TableMetadataLastFailure(){return TableMetadataFailure(cvr_sda_tm_failure(UINT32_MAX));}
bool CaptureTableMetadata(const ComPtr<ID3D12RootSignature>& root,std::span<const ComPtr<ID3D12DescriptorHeap>> heaps,
    std::span<const BoundTableMetadata> bound,TableMetadataSnapshot& output){
    output={};
    void* handle=cvr_sda_tm_capture(root.Get(),reinterpret_cast<ID3D12DescriptorHeap* const*>(heaps.data()),heaps.size(),bound.data(),bound.size());
    if(!handle)return false;
    try{output.data_=std::make_shared<const TableMetadataSnapshot::Data>(handle);}
    catch(...){cvr_sda_tm_free(handle);output={};cvr_sda_tm_failure(UINT(TableMetadataFailure::Exception));return false;}
    return true;
}
bool ValidateTableMetadataNoOverwrite(const TableMetadataSnapshot& value){
    const auto held=value.data_;if(!held){cvr_sda_tm_failure(UINT(TableMetadataFailure::Arguments));return false;}
    return cvr_sda_tm_validate(held->rust);
}
bool DescribeTableMetadata(const TableMetadataSnapshot& value,TableMetadataSummary& output)try{
    output={};const auto held=value.data_;if(!held){cvr_sda_tm_failure(UINT(TableMetadataFailure::Arguments));return false;}
    RustTableSummary view{};if(!cvr_sda_tm_describe(held->rust,&view))return false;
    TableMetadataSummary summary;summary.root=view.root;summary.trackingEpoch=view.trackingEpoch;summary.rootParameters=view.rootParameters;
    summary.rootFlags=view.rootFlags;summary.layoutReasons=view.layoutReasons;summary.boundTables=view.boundTables;
    summary.unboundTables=view.unboundTables;summary.unknownFlags=view.unknownFlags;summary.unknownSlots=view.unknownSlots;
    summary.complete=view.complete;summary.ranges.assign(view.ranges,view.ranges+view.rangeCount);
    summary.values.assign(view.values,view.values+view.valueCount);
    if(!cvr_sda_tm_validate(held->rust))return false;output=std::move(summary);return true;
}catch(...){output={};cvr_sda_tm_failure(UINT(TableMetadataFailure::Exception));return false;}
// The borrowed source ranges' held half: rust/crates/cvr-render/src/audit_borrowed_ranges.rs (the device, list, root,
// pipeline and heaps, the pin, the ranges and the inventory cache). The family, the recording lease and the bindings stay.
struct RustBorrowedRange {BorrowedUsedRange use;UINT heapIndex{};uint64_t table{};RangeIdentity identity;};
struct RustBorrowedView {uint64_t root,pipeline;const RustBorrowedRange* ranges;size_t rangeCount;};
static_assert(sizeof(RustBorrowedRange)==88 && sizeof(RustBorrowedView)==32 && sizeof(BorrowedShaderUse)==20,"mirrored by audit_borrowed_ranges.rs");
extern "C" void* cvr_sda_bsr_capture(ID3D12GraphicsCommandList*,ID3D12RootSignature*,ID3D12PipelineState*,ID3D12DescriptorHeap* const*,UINT,
    uint64_t,const D3D12_GPU_DESCRIPTOR_HANDLE*,const BorrowedShaderUse*,size_t);
extern "C" void cvr_sda_bsr_free(void*);
extern "C" bool cvr_sda_bsr_validate(const void*);
extern "C" bool cvr_sda_bsr_matches(const void*,bool);
extern "C" void cvr_sda_bsr_view(const void*,RustBorrowedView*);
extern "C" bool cvr_sda_bsr_inventory(const void*,UINT,BorrowedDescriptorInventory*);
namespace borrowed_fixed_detail {
// The rules: rust/crates/cvr-render/src/borrowed_fixed.rs (tools/single_pass_tests/descriptor_audit_test runs its cases
// against them in its Rust variant).
extern "C" {
uint32_t cvr_bfd_classify(const Value*,D3D12_SHADER_RESOURCE_VIEW_DESC*);
bool cvr_bfd_view_matches(const Value*,const D3D12_RESOURCE_DESC*,const D3D12_SHADER_RESOURCE_VIEW_DESC*);
}
DescriptorClass Classify(const Value& value,D3D12_SHADER_RESOURCE_VIEW_DESC& output)noexcept{
    return DescriptorClass(cvr_bfd_classify(&value,&output));
}
bool ViewMatches(const Value& value,const D3D12_RESOURCE_DESC& resource,const D3D12_SHADER_RESOURCE_VIEW_DESC& view)noexcept{
    return cvr_bfd_view_matches(&value,&resource,&view);
}
}
struct BorrowedSourceRanges::Data {
    void* rust{};
    std::shared_ptr<const families::Family> family;native_recording::Lease recording;scene_state::Bindings bindings;
    // BorrowedFixedRange reads a range's identity and matches under the lock it holds.
    struct Ranges {
        const Data* owner;
        struct Entry {RangeIdentity identity;};
        Entry operator[](size_t index)const{RustBorrowedView view{};cvr_sda_bsr_view(owner->rust,&view);return {view.ranges[index].identity};}
    } ranges{this};
    explicit Data(void* handle):rust(handle){}
    ~Data(){cvr_sda_bsr_free(rust);}
    Data(const Data&)=delete;
    Data& operator=(const Data&)=delete;
    bool MatchesLocked()const{return cvr_sda_bsr_matches(rust,false);}
};
namespace {
bool SameBorrowedBindings(const scene_state::Bindings& a,const scene_state::Bindings& b){
    if(a.root!=b.root || a.heapCount!=b.heapCount || a.heaps!=b.heaps || a.tableMask!=b.tableMask)return false;
    for(UINT p=0;p<64;++p)if((a.tableMask&(uint64_t(1)<<p)) && a.tables[p].ptr!=b.tables[p].ptr)return false;
    return true;
}
bool ShaderHash(const std::string& value){
    return value.size()==64 && std::all_of(value.begin(),value.end(),[](char c){return (c>='0' && c<='9') || (c>='a' && c<='f');});
}
bool CurrentBorrowedFamily(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root,
                          std::shared_ptr<const families::Family>& family,ID3D12PipelineState*& pipeline){
    family.reset();pipeline=nullptr;native_probe::DrawRecord current;
    if(!native_probe::CurrentState(list,current) || current.list!=reinterpret_cast<uintptr_t>(list) ||
        current.root!=reinterpret_cast<uintptr_t>(root) || !current.pipeline)return false;
    auto bound=families::Bound(list);auto* pso=reinterpret_cast<ID3D12PipelineState*>(current.pipeline);
    if(!bound || bound!=families::Lookup(pso) || bound->sourceRoot.Get()!=root || !ShaderHash(bound->rootSha256) ||
        !ShaderHash(bound->vertexSha256) || (bound->pass.pixelShader?!ShaderHash(bound->pixelSha256):!bound->pixelSha256.empty()) ||
        bound->uses.empty() || bound->uses.size()>4096)return false;
    family=std::move(bound);pipeline=pso;return true;
}
}
bool CaptureBorrowedSourceRanges(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,
    const scene_state::Bindings& bindings,BorrowedSourceRanges& output)try{
    output={};BorrowedStage(2);
    if(!Enabled() || !borrowedTracking.Ready())return false;
    BorrowedStage(1);
    if(!list || !recording.token || !recording.generation ||
        recording.list!=reinterpret_cast<uintptr_t>(list) || !bindings.root || !bindings.heapCount || bindings.heapCount>2 ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    BorrowedStage(3);
    if(!context || !native_recording::ValidateRecording(recording,list,context))return false;
    scene_state::Bindings actual;
    BorrowedStage(4);
    if(!scene_state::CaptureSourceBindings(list,actual) || !SameBorrowedBindings(bindings,actual))return false;
    std::shared_ptr<const families::Family> family;ID3D12PipelineState* pipeline{};
    BorrowedStage(5);
    if(!CurrentBorrowedFamily(list,bindings.root,family,pipeline))return false;
    std::vector<BorrowedShaderUse> uses;uses.reserve(family->uses.size());
    for(const auto& use:family->uses)uses.push_back({use.type,use.visibility,use.space,use.shaderRegister,use.count});
    void* handle=cvr_sda_bsr_capture(list,bindings.root,pipeline,bindings.heaps.data(),bindings.heapCount,bindings.tableMask,
        bindings.tables.data(),uses.data(),uses.size());
    if(!handle)return false;
    std::shared_ptr<BorrowedSourceRanges::Data> next;
    try{next=std::make_shared<BorrowedSourceRanges::Data>(handle);}catch(...){cvr_sda_bsr_free(handle);throw;}
    next->family=family;next->recording=recording;next->bindings=bindings;
    BorrowedStage(3);
    if(camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))!=context ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    BorrowedStage(4);
    if(!scene_state::CaptureSourceBindings(list,actual) || !SameBorrowedBindings(bindings,actual))return false;
    std::shared_ptr<const families::Family> finalFamily;ID3D12PipelineState* finalPipeline{};
    BorrowedStage(5);
    if(!CurrentBorrowedFamily(list,bindings.root,finalFamily,finalPipeline) || finalFamily!=family || finalPipeline!=pipeline)return false;
    if(!cvr_sda_bsr_matches(next->rust,true))return false;
    output.data_=std::move(next);BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
bool ValidateBorrowedSourceRanges(const BorrowedSourceRanges& value)try{
    BorrowedStage(1);
    const auto held=value.data_;if(!held)return false;
    if(!held->family){BorrowedStage(6);return false;}
    return cvr_sda_bsr_validate(held->rust);
}catch(...){BorrowedStage(12);return false;}
namespace {
BorrowedRangeSummary::Range DescribedRange(const RustBorrowedRange& range){
    const auto& identity=range.identity;const auto& use=range.use;
    return {use.parameter,use.type,use.offset,identity.count,identity.stride,use.shaderDeclaredCount,
        range.table,identity.gpu,identity.heap,identity.version,use.physicalRemainder};
}
}
bool DescribeBorrowedSourceRanges(const BorrowedSourceRanges& value,BorrowedRangeSummary& output)try{
    output={};BorrowedStage(1);const auto held=value.data_;if(!held || !ValidateBorrowedSourceRanges(value))return false;
    RustBorrowedView view{};cvr_sda_bsr_view(held->rust,&view);
    BorrowedRangeSummary next;next.list=held->recording.list;next.generation=held->recording.generation;
    next.root=view.root;next.pipeline=view.pipeline;next.family=held->family->id;
    next.ranges.reserve(view.rangeCount);
    for(size_t i=0;i<view.rangeCount;++i)next.ranges.push_back(DescribedRange(view.ranges[i]));
    if(!ValidateBorrowedSourceRanges(value))return false;output=std::move(next);return true;
}catch(...){output={};BorrowedStage(12);return false;}
bool InventoryBorrowedSourceRange(const BorrowedSourceRanges& value,UINT index,BorrowedRangeInventory& output)try{
    output={};if(!ValidateBorrowedSourceRanges(value))return false;const auto held=value.data_;
    BorrowedDescriptorInventory counts{};if(!cvr_sda_bsr_inventory(held->rust,index,&counts))return false;
    RustBorrowedView view{};cvr_sda_bsr_view(held->rust,&view);
    BorrowedRangeInventory next;next.list=held->recording.list;next.generation=held->recording.generation;
    next.root=view.root;next.pipeline=view.pipeline;next.family=held->family->id;next.rangeIndex=index;
    next.range=DescribedRange(view.ranges[index]);next.counts=counts;
    if(!ValidateBorrowedSourceRanges(value))return false;output=std::move(next);BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
struct BorrowedFixedRange::Data {
    BorrowedSourceRanges witness;
    BorrowedFixedRangeSummary summary;
    std::vector<Value> canonical;
    std::vector<texture_read::ImmutableLease> leases;
};
namespace {
bool FixedRangeIdentity(const BorrowedRangeSummary::Range& expected,const BorrowedRangeSummary::Range& actual){
    return expected.parameter==actual.parameter && expected.type==actual.type && expected.firstOffset==actual.firstOffset &&
        expected.count==actual.count && expected.stride==actual.stride && expected.table==actual.table && expected.gpu==actual.gpu &&
        expected.heap==actual.heap && expected.version==actual.version && expected.physicalRemainder && actual.physicalRemainder;
}
bool FixedCanonical(const std::vector<Value>& saved,const RangeIdentity& range){
    return cvr_sda_fixed_canonical(store.Rust(),saved.data(),saved.size(),&range);
}
bool SameFixedRecording(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.list==b.list && a.generation==b.generation && a.context==b.context && a.registry==b.registry;
}
}
bool CaptureBorrowedFixedRange(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,
    const BorrowedSourceRanges& witness,UINT index,BorrowedFixedRange& output)try{
    output={};BorrowedStage(1);
    if(!list || probe::internalCommands || !Enabled() || !witness.data_ || !SameFixedRecording(recording,witness.data_->recording) ||
        recording.list!=reinterpret_cast<uintptr_t>(list))return false;
    const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    BorrowedStage(3);if(!context || !native_recording::ValidateRecording(recording,list,context))return false;
    BorrowedRangeSummary rangeSummary;if(!DescribeBorrowedSourceRanges(witness,rangeSummary))return false;
    BorrowedStage(13,index);
    if(index>=rangeSummary.ranges.size() || !rangeSummary.ranges[index].physicalRemainder ||
        rangeSummary.ranges[index].type!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV)return false;
    const auto held=witness.data_;const auto range=held->ranges[index].identity;
    if(!range.count || range.count>Store::SlotLimit)return false;
    auto next=std::make_shared<BorrowedFixedRange::Data>();next->witness=witness;
    auto& summary=next->summary;summary.list=rangeSummary.list;summary.generation=rangeSummary.generation;
    summary.root=rangeSummary.root;summary.pipeline=rangeSummary.pipeline;summary.family=rangeSummary.family;
    summary.rangeIndex=index;summary.range=rangeSummary.ranges[index];summary.slots.resize(range.count);next->canonical.resize(range.count);
    {
        std::lock_guard lock(mutex);DrainRetired();if(!held->MatchesLocked())return false;
        std::vector<uint64_t> candidates(range.count);cvr_sda_fixed_snapshot(store.Rust(),&range,next->canonical.data(),candidates.data());
        for(UINT i=0;i<range.count;++i){auto& slot=summary.slots[i];slot.offset=summary.range.firstOffset+i;
            slot.cookie=next->canonical[i][2];slot.candidate=candidates[i];
        }
        if(!held->MatchesLocked())return false;
    }
    std::map<std::pair<uint64_t,uint64_t>,UINT> assets;
    for(UINT i=0;i<range.count;++i){auto& slot=summary.slots[i];const auto& value=next->canonical[i];
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};
        using C=borrowed_fixed_detail::DescriptorClass;
        switch(borrowed_fixed_detail::Classify(value,view)){
        case C::Unknown:continue;
        case C::TypedNull:slot.status=BorrowedFixedStatus::TypedNull;continue;
        case C::UnsupportedDescriptor:slot.status=BorrowedFixedStatus::UnsupportedDescriptor;continue;
        case C::UnsupportedView:slot.status=BorrowedFixedStatus::UnsupportedView;continue;
        case C::UnsupportedShape:slot.status=BorrowedFixedStatus::UnsupportedShape;continue;
        case C::Candidate:break;
        }
        if(!slot.candidate){slot.status=BorrowedFixedStatus::MissingCandidate;continue;}
        const auto key=std::pair{slot.cookie,slot.candidate};const auto found=assets.find(key);
        if(found!=assets.end())slot.asset=found->second;
        else{
            slot.asset=UINT(summary.assets.size());assets.emplace(key,slot.asset);summary.assets.emplace_back();next->leases.emplace_back();
            auto& asset=summary.assets.back();asset.cookie=slot.cookie;asset.candidate=slot.candidate;
            texture_read::FixedAssetPins pins;asset.stage=1;
            if(!texture_read::PinFixedAsset(list,slot.candidate,pins)){
                asset.status=BorrowedFixedStatus::PinRefused;asset.failure=texture_read::LastFailure();asset.pinDetails=texture_read::FixedPinLastDetails();
            }else{
                asset.pinDetails=texture_read::FixedPinLastDetails();ComPtr<ID3D12Resource> resource;asset.stage=2;
                if(!texture_read::GetFixedPinnedResource(pins,resource) || reinterpret_cast<uintptr_t>(resource.Get())!=slot.candidate){
                    asset.status=BorrowedFixedStatus::PinRefused;asset.failure=texture_read::LastFailure();if(!asset.failure)asset.failure=1;
                }else{
                    asset.stage=3;UINT bytes=sizeof(asset.actualCookie);
                    const auto hr=resource->GetPrivateData(cookieGuid,&bytes,&asset.actualCookie);
                    if(FAILED(hr) || bytes!=sizeof(asset.actualCookie) || asset.actualCookie!=slot.cookie){
                        asset.status=BorrowedFixedStatus::CookieMismatch;asset.failure=FAILED(hr)?UINT(hr):1;
                    }else{
                        asset.stage=4;
                        if(!texture_read::AcquirePinnedImmutable(list,pins,next->leases.back())){
                            asset.status=BorrowedFixedStatus::ImmutableRefused;asset.failure=texture_read::LastFailure();
                        }else{asset.status=BorrowedFixedStatus::Immutable;asset.stage=asset.failure=0;}
                    }
                }
            }
        }
        const auto& asset=summary.assets[slot.asset];slot.status=asset.status;
        if(slot.status==BorrowedFixedStatus::Immutable){const auto& lease=next->leases[slot.asset];
            if(!lease.source.resource || !borrowed_fixed_detail::ViewMatches(value,lease.source.resource->GetDesc(),view))
                slot.status=BorrowedFixedStatus::UnsupportedView;
        }
    }
    for(UINT i=0;i<next->leases.size();++i)if(summary.assets[i].status==BorrowedFixedStatus::Immutable &&
        !texture_read::ValidateImmutable(next->leases[i])){
        auto& asset=summary.assets[i];asset.status=BorrowedFixedStatus::ImmutableRefused;asset.stage=5;asset.failure=texture_read::LastFailure();
        for(auto& slot:summary.slots)if(slot.asset==i && slot.status==BorrowedFixedStatus::Immutable)slot.status=asset.status;
    }
    BorrowedStage(3);
    if(camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))!=context ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    {
        std::lock_guard lock(mutex);DrainRetired();if(!held->MatchesLocked() || !FixedCanonical(next->canonical,range))return false;
    }
    BorrowedStage(3);
    if(camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))!=context ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    std::array<UINT,10> sampled{};
    for(UINT index=0;index<summary.slots.size();++index){const auto& slot=summary.slots[index];
        ++summary.counts[UINT(slot.status)];
        if(sampled[UINT(slot.status)]++<4)summary.samples.push_back({slot.offset,slot.asset,slot.status,next->canonical[index]});
    }
    summary.allSlotsQualified=summary.counts[UINT(BorrowedFixedStatus::TypedNull)]+summary.counts[UINT(BorrowedFixedStatus::Immutable)]==range.count;
    output.data_=std::move(next);BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
bool ValidateBorrowedFixedRange(const BorrowedFixedRange& fixed,const BorrowedSourceRanges& witness,UINT index)try{
    BorrowedStage(1);const auto held=fixed.data_;if(!held || !held->summary.allSlotsQualified || !witness.data_)return false;
    BorrowedRangeSummary current;if(!DescribeBorrowedSourceRanges(witness,current))return false;
    BorrowedStage(11,index);
    if(index>=current.ranges.size() || !FixedRangeIdentity(held->summary.range,current.ranges[index]))return false;
    for(const auto& lease:held->leases)if(!lease.owner || !texture_read::ValidateImmutable(lease))return false;
    const auto borrowed=witness.data_;std::lock_guard lock(mutex);DrainRetired();
    if(!borrowed->MatchesLocked() || !FixedCanonical(held->canonical,borrowed->ranges[index].identity))return false;
    BorrowedStage(0);return true;
}catch(...){BorrowedStage(12);return false;}
bool DescribeBorrowedFixedRange(const BorrowedFixedRange& fixed,BorrowedFixedRangeSummary& output)try{
    output={};BorrowedStage(1);if(!fixed.data_)return false;
    output=fixed.data_->summary;BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
void BeforeSubmit(UINT n,ID3D12CommandList* const* lists){cvr_sda_audit_submit(n,reinterpret_cast<const uint64_t*>(lists));}
void Reset(ID3D12GraphicsCommandList* list){cvr_sda_audit_capture(5,reinterpret_cast<uintptr_t>(list),0,0);}
void Root(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root){
    cvr_sda_audit_capture(6,reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(root),0);
}
void Table(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE address){
    cvr_sda_audit_capture(7,reinterpret_cast<uintptr_t>(list),address.ptr,index);
}
void ClearBindings(ID3D12GraphicsCommandList* list){cvr_sda_audit_capture(8,reinterpret_cast<uintptr_t>(list),0,0);}
bool FindBinding(uint64_t root,D3D12_DESCRIPTOR_RANGE_TYPE type,UINT space,UINT shaderRegister,
                 D3D12_SHADER_VISIBILITY visibility,UINT& parameter,UINT& tableOffset){
    return cvr_sda_audit_find_binding(root,UINT(type),space,shaderRegister,UINT(visibility),&parameter,&tableOffset);
}
static_assert(sizeof(CbvBinding)==32 && sizeof(DescriptorStamp)==176,"mirrored by descriptor_audit.rs");
bool ResolveCbv(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,CbvBinding& output){
    return cvr_sda_audit_cbv(0,reinterpret_cast<uintptr_t>(list),root,parameter,offset,&output);
}
bool ResolveCbvAddress(uint64_t descriptor,CbvBinding& output){return cvr_sda_audit_cbv(1,descriptor,0,0,0,&output);}
bool CaptureDescriptor(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,DescriptorStamp& output){
    output={};return cvr_sda_audit_stamp(0,reinterpret_cast<uintptr_t>(list),root,parameter,offset,&output,nullptr);
}
bool MatchesDescriptor(const DescriptorStamp& expected){
    return cvr_sda_audit_stamp(1,0,0,0,0,const_cast<DescriptorStamp*>(&expected),nullptr);
}
bool CaptureCbvSource(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,DescriptorStamp& output,ComPtr<IUnknown>& origin){
    output={};origin.Reset();DescriptorStamp stamp;IUnknown* owner{};
    if(!cvr_sda_audit_stamp(2,reinterpret_cast<uintptr_t>(list),root,parameter,offset,&stamp,&owner))return false;
    output=stamp;origin.Attach(owner);return true;
}
bool ResolveSrvAtDraw(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,SrvBinding& output){
    output={};DescriptorStamp stamp;ID3D12Resource* resource{};D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    if(!cvr_sda_audit_srv(reinterpret_cast<uintptr_t>(list),root,parameter,offset,&stamp,&resource,&view))return false;
    SrvBinding resolved;resolved.description=stamp.value;resolved.stamp=stamp;resolved.resource.Attach(resource);resolved.view=view;
    output=std::move(resolved);return true;
}
bool ResolveSamplerAtDraw(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,SamplerBinding& output){
    output={};return cvr_sda_audit_sampler(reinterpret_cast<uintptr_t>(list),root,parameter,offset,&output.stamp,&output.view);
}
bool ResolveCpuCbvAddress(uint64_t descriptor,CbvBinding& output){return cvr_sda_audit_cbv(2,descriptor,0,0,0,&output);}
// The layouts: rust/crates/cvr-render/src/source_layout.rs (tools/single_pass_tests/descriptor_audit_test runs its cases
// against them in its Rust variant).
struct SourceSlotPlan {uint32_t kind;bool used;};
static_assert(sizeof(Binding)==64 && sizeof(SourceSlotUse)==12 && sizeof(SourceSlotPlan)==8,"mirrored by source_layout.rs");
// The source tables: rust/crates/cvr-render/src/audit_source_tables.rs (the layouts, walks and re-checks, the provider
// called through the trampoline below, the owner and capture-receipt proofs, the cache). The public types are built
// here from the rows it emits on success.
struct RustSourceRequest {UINT parameter,count;const SourceSlotUse* uses;size_t useCount;};
struct RustSourceBindings {ID3D12RootSignature* root;const D3D12_GPU_DESCRIPTOR_HANDLE* tables;uint64_t tableMask;
    ID3D12DescriptorHeap* const* heaps;UINT heapCount;bool heapsSeen;};
struct RustOwnedTableRow {UINT parameter,count;ID3D12DescriptorHeap* cpuHeap;IUnknown* owner;uint64_t identity,version,sourceGpu,sourceHeap;
    IUnknown* provenance;};
struct RustOwnedSlotsRow {UINT parameter,count;ID3D12DescriptorHeap* cpuHeap;IUnknown* owner;uint64_t identity,version,sourceGpu,sourceHeap;
    const OwnedSourceSlot* slots;size_t slotCount;};
struct RustSourceDependency {UINT parameter,offset;D3D12_DESCRIPTOR_RANGE_TYPE type;DescriptorStamp stamp;ID3D12Resource* resource;ID3D12Resource* counter;};
static_assert(sizeof(RustSourceRequest)==24 && sizeof(RustSourceBindings)==40 && sizeof(RustOwnedTableRow)==64 &&
    sizeof(RustOwnedSlotsRow)==72 && sizeof(RustSourceDependency)==208 && sizeof(OwnedSourceTable)==64 && sizeof(SrvBinding)==384 &&
    sizeof(OwnedSourceSlot)==192 && sizeof(SourceTableFailure)==12,"mirrored by audit_source_tables.rs");
using RustSourceProvider=uint32_t(*)(const void*,ID3D12GraphicsCommandList*,UINT,UINT,const DescriptorStamp*,IUnknown*,ID3D12Resource**,IUnknown**);
using RustSourceEmit=void(*)(void*,uint32_t,const void*);
extern "C" bool cvr_sda_st_capture(bool,ID3D12Device*,ID3D12GraphicsCommandList*,const RustSourceBindings*,const RustSourceRequest*,size_t,
    RustSourceProvider,const void*,void**,bool,RustSourceEmit,void*);
extern "C" void cvr_sda_st_cache_free(void*);
extern "C" uint32_t cvr_sda_st_cache_count(const void*,uint32_t);
extern "C" void cvr_sda_st_failure(const SourceTableFailure*,SourceTableFailure*);
extern "C" bool cvr_sda_st_matches(const OwnedSourceTable*,UINT,const SrvBinding*);
extern "C" bool cvr_sda_st_matches_slot(const OwnedSourceSlot*);
extern "C" bool cvr_sda_st_matches_cpu(uint64_t,const DescriptorStamp*);
namespace {
struct SourceProviderCall {SourceCbvProvider provider;void* user;};
uint32_t CallSourceProvider(const void* context,ID3D12GraphicsCommandList* list,UINT parameter,UINT offset,const DescriptorStamp* stamp,
    IUnknown* origin,ID3D12Resource** resource,IUnknown** owner)noexcept{
    try{
        const auto* call=static_cast<const SourceProviderCall*>(context);if(!call->provider)return 0;
        const ComPtr<IUnknown> held(origin);SourceCbvBacking backing;
        if(!call->provider(call->user,list,parameter,offset,*stamp,held,backing))return 0;
        *resource=backing.resource.Detach();*owner=backing.owner.Detach();return 1;
    }catch(...){return 2;}
}
struct SourceRows {std::vector<OwnedSourceTable>* tables;std::vector<OwnedSourceSlots>* slots;std::vector<SourceResourceBinding>* dependencies;};
void EmitSourceRow(void* context,uint32_t which,const void* row){
    auto& rows=*static_cast<SourceRows*>(context);
    if(which==0){
        const auto& r=*static_cast<const RustOwnedTableRow*>(row);OwnedSourceTable table{r.parameter,r.count};
        table.cpuHeap.Attach(r.cpuHeap);table.owner.Attach(r.owner);table.identity=r.identity;table.version=r.version;
        table.sourceGpu=r.sourceGpu;table.sourceHeap=r.sourceHeap;table.provenance.Attach(r.provenance);rows.tables->push_back(std::move(table));
    }else if(which==1){
        const auto& r=*static_cast<const RustOwnedSlotsRow*>(row);OwnedSourceSlots owned;owned.parameter=r.parameter;owned.count=r.count;
        owned.cpuHeap.Attach(r.cpuHeap);owned.owner.Attach(r.owner);owned.identity=r.identity;owned.version=r.version;owned.sourceGpu=r.sourceGpu;
        owned.sourceHeap=r.sourceHeap;owned.slots.assign(r.slots,r.slots+r.slotCount);rows.slots->push_back(std::move(owned));
    }else{
        const auto& r=*static_cast<const RustSourceDependency*>(row);SourceResourceBinding binding{r.parameter,r.offset,r.type,r.stamp};
        binding.resource.Attach(r.resource);binding.counter.Attach(r.counter);rows.dependencies->push_back(std::move(binding));
    }
}
bool SourceRefused(UINT reason=1){const SourceTableFailure failure{reason};cvr_sda_st_failure(&failure,nullptr);return false;}
bool CaptureSourceRows(bool slots,ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
    std::span<const SourceTableRequest> requests,SourceCbvProvider provider,void* user,void*& cache,bool dependencies,SourceRows& rows){
    std::vector<RustSourceRequest> lent;lent.reserve(requests.size());
    for(const auto& request:requests)lent.push_back({request.parameter,request.count,request.uses.data(),request.uses.size()});
    const RustSourceBindings lentBindings{bindings.root,bindings.tables.data(),bindings.tableMask,bindings.heaps.data(),bindings.heapCount,bindings.heapsSeen};
    const SourceProviderCall call{provider,user};
    return cvr_sda_st_capture(slots,device,list,&lentBindings,lent.data(),lent.size(),&CallSourceProvider,&call,&cache,dependencies,&EmitSourceRow,&rows);
}
}
struct SourceTableCache::Storage {
    void* rust{};
    explicit Storage(void* handle):rust(handle){}
    ~Storage(){cvr_sda_st_cache_free(rust);}
    Storage(const Storage&)=delete;
    Storage& operator=(const Storage&)=delete;
};
SourceTableCache::SourceTableCache()=default;
SourceTableCache::~SourceTableCache()=default;
void SourceTableCache::Clear(){storage_.reset();}
UINT SourceTableCache::TableCount()const{return storage_?cvr_sda_st_cache_count(storage_->rust,0):0;}
UINT SourceTableCache::DescriptorCount()const{return storage_?cvr_sda_st_cache_count(storage_->rust,1):0;}
UINT SourceTableCache::SlotTableCount()const{return storage_?cvr_sda_st_cache_count(storage_->rust,2):0;}
UINT SourceTableCache::SlotDescriptorCount()const{return storage_?cvr_sda_st_cache_count(storage_->rust,3):0;}
SourceTableFailure LastSourceTableFailure(){SourceTableFailure failure;cvr_sda_st_failure(nullptr,&failure);return failure;}
bool MatchesCapturedSourceSrv(const OwnedSourceTable& table,UINT offset,const SrvBinding& binding)noexcept{
    return cvr_sda_st_matches(&table,offset,&binding);
}
bool MatchesSourceSlot(const OwnedSourceSlot& slot){return cvr_sda_st_matches_slot(&slot);}
bool MatchesCpuSourceSrv(uint64_t cpu,const DescriptorStamp& expected){return cvr_sda_st_matches_cpu(cpu,&expected);}
bool CaptureSourceTablesAtDraw(ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
    std::span<const SourceTableRequest> requests,SourceCbvProvider provider,void* user,SourceTableCache& cache,std::vector<OwnedSourceTable>& output,
    std::vector<SourceResourceBinding>* dependencies)try{
    if(!output.empty() || (dependencies && !dependencies->empty()) || requests.size()>64)return SourceRefused();
    std::vector<OwnedSourceTable> result;std::vector<SourceResourceBinding> used;SourceRows rows{&result,nullptr,&used};
    void* handle=cache.storage_?cache.storage_->rust:nullptr;
    const bool captured=CaptureSourceRows(false,device,list,bindings,requests,provider,user,handle,dependencies!=nullptr,rows);
    if(handle && !cache.storage_){
        try{cache.storage_=std::make_unique<SourceTableCache::Storage>(handle);}catch(...){cvr_sda_st_cache_free(handle);throw;}
    }
    if(!captured)return false;
    output.swap(result);if(dependencies)dependencies->swap(used);return true;
}catch(...){return SourceRefused(13);}
bool CaptureSourceSlotsAtDraw(ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
    std::span<const SourceTableRequest> requests,SourceCbvProvider provider,void* user,SourceTableCache& cache,std::vector<OwnedSourceSlots>& output,
    std::vector<SourceResourceBinding>* dependencies)try{
    if(!output.empty() || (dependencies && !dependencies->empty()) || requests.size()>64)return SourceRefused();
    std::vector<OwnedSourceSlots> result;std::vector<SourceResourceBinding> used;SourceRows rows{nullptr,&result,&used};
    void* handle=cache.storage_?cache.storage_->rust:nullptr;
    const bool captured=CaptureSourceRows(true,device,list,bindings,requests,provider,user,handle,dependencies!=nullptr,rows);
    if(handle && !cache.storage_){
        try{cache.storage_=std::make_unique<SourceTableCache::Storage>(handle);}catch(...){cvr_sda_st_cache_free(handle);throw;}
    }
    if(!captured)return false;
    output.swap(result);if(dependencies)dependencies->swap(used);return true;
}catch(...){return SourceRefused(13);}
}
#endif
#else
extern "C" {
__declspec(dllexport) uint32_t CyberpunkVR_DescriptorBindingBytes=sizeof(cvr::stereo::descriptors::Binding),CyberpunkVR_DescriptorValueBytes=sizeof(cvr::stereo::descriptors::Value);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DescriptorCaptureSeq{},CyberpunkVR_DescriptorDrawCount{},CyberpunkVR_DescriptorBindingCount{},CyberpunkVR_DescriptorIdCount{},CyberpunkVR_DescriptorValueCount{},CyberpunkVR_DescriptorCaptureFailure{};
__declspec(dllexport) uint64_t CyberpunkVR_DescriptorDraws{},CyberpunkVR_DescriptorBindings{},CyberpunkVR_DescriptorIds{},CyberpunkVR_DescriptorValues{};
__declspec(dllexport) uint64_t CyberpunkVR_DescriptorStoreStats[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_DescriptorHeapSlots{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DescriptorDenseSlots{1},CyberpunkVR_DescriptorDenseActive{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DescriptorLiveMetadata{1},CyberpunkVR_DescriptorLiveMetadataActive{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DescriptorPendingLists{};
__declspec(dllexport) uint32_t CyberpunkVR_DescriptorRootCount{};
__declspec(dllexport) cvr::stereo::descriptors::RootInfo CyberpunkVR_DescriptorRoots[128]{};
}
namespace cvr::stereo::descriptors {
namespace {
class MetadataMutex {
    CRITICAL_SECTION section{};std::mutex fallback;bool native{};
public:
    MetadataMutex():native(InitializeCriticalSectionEx(&section,4000,CRITICAL_SECTION_NO_DEBUG_INFO)!=FALSE){}
    ~MetadataMutex(){if(native)DeleteCriticalSection(&section);}
    void lock(){if(native)EnterCriticalSection(&section);else fallback.lock();}
    void unlock(){if(native)LeaveCriticalSection(&section);else fallback.unlock();}
};
MetadataMutex mutex;
Store store;
BorrowedRangeTracking borrowedTracking;
thread_local uint32_t borrowedFailure{};
thread_local std::array<uint64_t,4> borrowedDetails{};
thread_local TableMetadataFailure tableMetadataFailure{};
void BorrowedStage(uint32_t value,uint64_t index=UINT64_MAX,uint64_t gpu=0,uint64_t expected=0,uint64_t actual=0){
    borrowedFailure=value;borrowedDetails={index,gpu,expected,actual};
}
DescriptorMutationMode MutationMode(){return DescriptorMutationObservation(probe::internalCommands!=0,Enabled(),borrowedTracking.Pins()!=0);}
struct OriginEntry {Value value{};ComPtr<IUnknown> owner;};
std::unordered_map<uint64_t,OriginEntry> cbvOrigins;
void RetireDescriptorValue(const Value& value){if(value[0]==1 && value[3])cbvOrigins.erase(value[3]);}
void ObserveOrigins(){store.SetValueRetirement(RetireDescriptorValue);}
std::atomic<uint64_t> nextCookie{1};
// Object private data survives for exactly that COM object's lifetime. A reused
// address cannot inherit a resource/heap identity from a destroyed object.
constexpr GUID cookieGuid{0xc7ec4301,0x790e,0x4b32,{0x93,0x87,0x7c,0xc1,0x19,0x52,0xd1,0x44}};
uint64_t Cookie(ID3D12Object* object) {
    if(!object)return 0;uint64_t value{};UINT bytes=sizeof(value);
    if(SUCCEEDED(object->GetPrivateData(cookieGuid,&bytes,&value)) && bytes==sizeof(value) && value)return value;
    value=nextCookie.fetch_add(1,std::memory_order_relaxed);
    return SUCCEEDED(object->SetPrivateData(cookieGuid,sizeof(value),&value))?value:0;
}
struct RootLayout {std::vector<Binding> ranges;bool supported=true;UINT parameters{},reasons{},flags{};};
std::unordered_map<ID3D12RootSignature*,RootLayout> roots;
std::unordered_map<uint64_t,bool> knownHeaps;
constexpr GUID heapLifetimeGuid{0xc67d21d0,0x4d42,0x4526,{0x82,0x30,0x79,0xaf,0x13,0x01,0x6c,0x07}};
struct HeapLifetime;
std::atomic<HeapLifetime*> retiredHeaps{};
struct HeapLifetime final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t cookie{};HeapLifetime* next{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n){
        // D3D12 may release private data under its own object lock. Defer all
        // store locking and reclamation to the next metadata operation.
        next=retiredHeaps.load(std::memory_order_relaxed);
        while(!retiredHeaps.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
    }return n;}
};
constexpr GUID resourceLifetimeGuid{0x35d4c78a,0x4e19,0x4c02,{0x91,0x6d,0xac,0x83,0xc1,0x74,0x8d,0x28}};
struct ResourceLifetime;
std::atomic<ResourceLifetime*> retiredResources{};
std::unordered_map<uint64_t,ResourceLifetime*> knownResources;
struct ResourceLifetime final:IUnknown {
    std::atomic<ULONG> refs{1};uint64_t cookie{};ID3D12Resource* resource{};ResourceLifetime* next{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n){
        next=retiredResources.load(std::memory_order_relaxed);
        while(!retiredResources.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
    }return n;}
};
void DrainRetired(){
    auto* head=retiredHeaps.exchange(nullptr,std::memory_order_acquire);
    while(head){auto* next=head->next;store.HeapRemoved(head->cookie);knownHeaps.erase(head->cookie);delete head;head=next;}
    auto* resource=retiredResources.exchange(nullptr,std::memory_order_acquire);
    while(resource){auto* next=resource->next;
        const auto found=knownResources.find(resource->cookie);
        if(found!=knownResources.end() && found->second==resource)knownResources.erase(found);
        delete resource;resource=next;
    }
}
bool TrackSrvResource(ID3D12Resource* resource,uint64_t cookie){
    if(!resource)return true;
    DrainRetired();if(!cookie)return false;
    const auto found=knownResources.find(cookie);
    if(found!=knownResources.end())return found->second->resource==resource;
    if(knownResources.size()>=131072)return false;
    ComPtr<ResourceLifetime> owner;owner.Attach(new ResourceLifetime);owner->cookie=cookie;owner->resource=resource;
    if(FAILED(resource->SetPrivateDataInterface(resourceLifetimeGuid,owner.Get())))return false;
    knownResources.emplace(cookie,owner.Get());return true;
}
std::vector<DrawBindings> draws;
std::vector<Binding> bindings;
std::vector<uint32_t> ids;
std::vector<Value> values;
std::map<Value,uint32_t> exportedValues;
std::unordered_map<ID3D12CommandList*,std::vector<uint32_t>> pending;
struct Tables {ID3D12RootSignature* root{};uint64_t mask{};std::array<uint64_t,64> addresses{};};
std::unordered_map<ID3D12GraphicsCommandList*,Tables> tables;
bool recording{},bindingOnly{};
bool liveMetadata{};
std::atomic<uint32_t> trackingFailure{};
void Fail(uint32_t reason){trackingFailure.store(reason);CyberpunkVR_DescriptorCaptureFailure=reason;}
void SelectStorage(){
    const auto dense=CyberpunkVR_DescriptorDenseSlots.load(std::memory_order_relaxed);
    if(dense>1){Fail(7);return;}
    if(store.Dense()!=(dense!=0)){store.Dense(dense!=0);CyberpunkVR_DescriptorDenseActive.store(dense,std::memory_order_release);}
    const auto live=CyberpunkVR_DescriptorLiveMetadata.load(std::memory_order_relaxed);
    if(live>1){Fail(8);return;}
    liveMetadata=live && bindingOnly && !recording && pending.empty();
    store.TrackVersions(borrowedTracking.VersionsRequired(liveMetadata));
    CyberpunkVR_DescriptorLiveMetadataActive.store(liveMetadata,std::memory_order_release);
}
UINT resourceStride{},samplerStride{};
uint64_t Bits(float f){return std::bit_cast<uint32_t>(f);}
void DescribeSrvResource(ID3D12Resource* resource,Value& value) {
    if(!resource)return;
    const auto d=resource->GetDesc();value[1]|=4;
    value[13]=uint64_t(d.Dimension)|(uint64_t(d.Format)<<32);value[14]=d.Width;
    value[15]=uint64_t(d.Height)|(uint64_t(d.DepthOrArraySize)<<32)|(uint64_t(d.MipLevels)<<48);
    value[16]=uint64_t(d.Flags)|(uint64_t(d.Layout)<<32);
    value[17]=uint64_t(d.SampleDesc.Count)|(uint64_t(d.SampleDesc.Quality)<<32);
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(SUCCEEDED(resource->GetHeapProperties(&heap,&flags))){
        value[1]|=8;value[18]=uint64_t(heap.Type)|(uint64_t(heap.CPUPageProperty)<<16)|(uint64_t(heap.MemoryPoolPreference)<<32);
    }
    if(d.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER)value[19]=resource->GetGPUVirtualAddress();
}
void Publish() {
    UINT rootIndex{};for(const auto& [root,layout]:roots)CyberpunkVR_DescriptorRoots[rootIndex++]={reinterpret_cast<uintptr_t>(root),layout.parameters,UINT(layout.supported),UINT(layout.ranges.size()),layout.reasons};
    CyberpunkVR_DescriptorRootCount=rootIndex;
    const auto stats=store.Stats();std::copy(stats.begin(),stats.end(),CyberpunkVR_DescriptorStoreStats);
    CyberpunkVR_DescriptorHeapSlots=store.HeapSlots();
    CyberpunkVR_DescriptorDraws=reinterpret_cast<uintptr_t>(draws.data());CyberpunkVR_DescriptorDrawCount=uint32_t(draws.size());
    CyberpunkVR_DescriptorBindings=reinterpret_cast<uintptr_t>(bindings.data());CyberpunkVR_DescriptorBindingCount=uint32_t(bindings.size());
    CyberpunkVR_DescriptorIds=reinterpret_cast<uintptr_t>(ids.data());CyberpunkVR_DescriptorIdCount=uint32_t(ids.size());
    CyberpunkVR_DescriptorValues=reinterpret_cast<uintptr_t>(values.data());CyberpunkVR_DescriptorValueCount=uint32_t(values.size());
}
void Put(D3D12_CPU_DESCRIPTOR_HANDLE cpu,const Value& value,UINT stride) {
    if(!stride){Fail(1);return;}
    DrainRetired();
    SelectStorage();
    if(liveMetadata)store.WriteValue(cpu.ptr,stride,value);
    else store.Write(cpu.ptr,stride,store.Intern(value));
}
void InvalidateInternal(D3D12_CPU_DESCRIPTOR_HANDLE destination,UINT count,UINT stride){
    try{std::lock_guard lock(mutex);DrainRetired();SelectStorage();
        if(store.InvalidateTracked(destination.ptr,count,stride)==TrackedDescriptorWrite::Rejected)Fail(11);
    }catch(...){Fail(2);}
}
uint32_t Export(uint32_t id) {
    if(!id)return 0;
    const auto* value=store.Get(id);if(!value || values.size()>=Store::ValueLimit)return 0;
    const auto found=exportedValues.find(*value);if(found!=exportedValues.end())return found->second;
    const auto index=uint32_t(values.size());values.push_back(*value);exportedValues[*value]=index;return index;
}
void RegisterHeap(ID3D12Device* device,ID3D12DescriptorHeap* heap,bool fresh) {
    if(!heap || !device)return;
    DrainRetired();
    const auto d=heap->GetDesc();if(d.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && d.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER)return;
    const auto cookie=Cookie(heap);if(!cookie)return;
    if(!fresh && knownHeaps.contains(cookie))return;
    const auto stride=device->GetDescriptorHandleIncrementSize(d.Type);
    if(d.Type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER)samplerStride=stride;else resourceStride=stride;
    const auto gpu=(d.Flags&D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE)?heap->GetGPUDescriptorHandleForHeapStart().ptr:0;
    if(!knownHeaps.contains(cookie)){
        ComPtr<HeapLifetime> owner;owner.Attach(new HeapLifetime);owner->cookie=cookie;
        if(FAILED(heap->SetPrivateDataInterface(heapLifetimeGuid,owner.Get()))){Fail(6);return;}
    }
    if(store.HeapCreated(heap->GetCPUDescriptorHandleForHeapStart().ptr,gpu,d.NumDescriptors,stride,cookie))knownHeaps[cookie]=true;
}
}
bool Enabled(){return native_probe::ObserveBindings() && !probe::internalCommands;}
MutationScope::MutationScope():entered_(borrowedTracking.MutationBegin()){}
MutationScope::~MutationScope(){if(entered_)borrowedTracking.MutationEnd();}
void BorrowedRangeObserverReady(bool ready){borrowedTracking.Ready(ready);}
uint32_t BorrowedRangeLastFailure(){return borrowedFailure;}
std::array<uint64_t,4> BorrowedRangeLastDetails(){return borrowedDetails;}
void Initialize(ID3D12Device* device) {
    if(!Enabled() || !device)return;std::lock_guard lock(mutex);
    resourceStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    samplerStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
}
void HeapCreated(ID3D12Device* device,ID3D12DescriptorHeap* heap) {
    if(MutationMode()!=DescriptorMutationMode::Canonical)return;
    try{std::lock_guard lock(mutex);RegisterHeap(device,heap,true);}catch(...){Fail(2);}
}
void HeapsBound(ID3D12Device* device,UINT count,ID3D12DescriptorHeap* const* heaps) {
    if(!Enabled() || !heaps || count>2)return;observation_profile::Scope profile(observation_profile::Phase::BindHeaps);
    try{std::lock_guard lock(mutex);for(UINT i=0;i<count;++i)RegisterHeap(device,heaps[i],false);}catch(...){Fail(2);}
}
void RootCreated(ID3D12RootSignature* root,const void* bytes,size_t size) {
    if(!Enabled() || !root)return;
    try {
        ComPtr<ID3D12VersionedRootSignatureDeserializer> reader;
        if(FAILED(D3D12CreateVersionedRootSignatureDeserializer(bytes,size,IID_PPV_ARGS(&reader))))return;
        const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* d{};if(FAILED(reader->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&d)))return;
        RootLayout result;result.parameters=d->Desc_1_1.NumParameters;result.flags=UINT(d->Desc_1_1.Flags);result.supported=result.parameters<=64;if(!result.supported)result.reasons|=1;
        for(UINT i=0;i<d->Desc_1_1.NumParameters && i<64;++i) {
            const auto& p=d->Desc_1_1.pParameters[i];
            if(p.ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE){result.supported=false;result.reasons|=2;continue;}
            uint64_t offset{};
            for(UINT j=0;j<p.DescriptorTable.NumDescriptorRanges;++j) {
                const auto& r=p.DescriptorTable.pDescriptorRanges[j];
                if(r.OffsetInDescriptorsFromTableStart!=D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND)offset=r.OffsetInDescriptorsFromTableStart;
                if(offset>UINT32_MAX || !r.NumDescriptors || result.ranges.size()>=256){result.supported=false;result.reasons|=4;break;}
                Binding b;b.parameter=i;b.type=r.RangeType;b.space=r.RegisterSpace;b.shaderRegister=r.BaseShaderRegister;
                b.count=r.NumDescriptors;b.tableOffset=UINT(offset);b.visibility=p.ShaderVisibility;result.ranges.push_back(b);offset+=r.NumDescriptors;
            }
        }
        std::lock_guard lock(mutex);if(roots.size()<128 || roots.contains(root))roots[root]=std::move(result);
    }catch(...){Fail(2);}
}
uint64_t SourceResourceCookieAtCreation(ID3D12Resource* resource){
    if(!Enabled() || !resource)return 0;
    try{std::lock_guard lock(mutex);const auto cookie=Cookie(resource);return cookie && TrackSrvResource(resource,cookie)?cookie:0;}
    catch(...){return 0;}
}
bool MatchesHeldResourceCookie(ID3D12Resource* resource,uint64_t cookie)noexcept{
    if(!resource || !cookie)return false;uint64_t current{};UINT bytes=sizeof(current);
    return SUCCEEDED(resource->GetPrivateData(cookieGuid,&bytes,&current)) && bytes==sizeof(current) && current==cookie;
}
bool GetHeldResourceCookie(const ComPtr<ID3D12Resource>& resource,uint64_t& cookie)noexcept{
    cookie=0;if(!resource)return false;uint64_t current{};UINT bytes=sizeof(current);
    if(FAILED(resource->GetPrivateData(cookieGuid,&bytes,&current)) || bytes!=sizeof(current) || !current)return false;
    cookie=current;return true;
}
void Cbv(const D3D12_CONSTANT_BUFFER_VIEW_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu,const CbvOrigin* origin) {
    const auto mode=MutationMode();if(mode==DescriptorMutationMode::Ignore)return;
    if(mode==DescriptorMutationMode::InvalidateTracked){InvalidateInternal(cpu,1,resourceStride);return;}
    observation_profile::Scope profile(observation_profile::Phase::Cbv);
    try{std::lock_guard lock(mutex);ObserveOrigins();Value v{};if(d){v[0]=1;v[4]=d->BufferLocation;v[5]=d->SizeInBytes;}
        if(d && origin && origin->cookie && origin->id && origin->owner){
            v[2]=origin->cookie;v[3]=origin->id;
            const auto found=cbvOrigins.find(origin->id);
            if(found!=cbvOrigins.end() && (found->second.value!=v || found->second.owner.Get()!=origin->owner.Get())){Fail(10);return;}
            cbvOrigins[origin->id]={v,origin->owner};
        }
        Put(cpu,v,resourceStride);
        if(v[3]){const auto* stored=store.Get(store.At(cpu.ptr));if(!stored || *stored!=v)cbvOrigins.erase(v[3]);}
    }catch(...){Fail(2);}
}
void Srv(ID3D12Resource* resource,const D3D12_SHADER_RESOURCE_VIEW_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu) {
    const auto mode=MutationMode();if(mode==DescriptorMutationMode::Ignore)return;
    if(mode==DescriptorMutationMode::InvalidateTracked){InvalidateInternal(cpu,1,resourceStride);return;}
    observation_profile::Scope profile(observation_profile::Phase::OtherDescriptor);
    try{std::lock_guard lock(mutex);Value v{};v[0]=2;v[2]=Cookie(resource);v[1]=resource?1:0;
        if(resource && !v[2])v[0]=0;
        if(!d){v[1]|=2;if(!resource)v[0]=0;}
        else {
            v[4]=d->Format;v[5]=d->ViewDimension;v[6]=d->Shader4ComponentMapping;
            switch(d->ViewDimension) {
            case D3D12_SRV_DIMENSION_BUFFER:v[7]=d->Buffer.FirstElement;v[8]=d->Buffer.NumElements;v[9]=d->Buffer.StructureByteStride;v[10]=d->Buffer.Flags;break;
            case D3D12_SRV_DIMENSION_TEXTURE1D:v[7]=d->Texture1D.MostDetailedMip;v[8]=d->Texture1D.MipLevels;v[9]=Bits(d->Texture1D.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_TEXTURE1DARRAY:v[7]=d->Texture1DArray.MostDetailedMip;v[8]=d->Texture1DArray.MipLevels;v[9]=d->Texture1DArray.FirstArraySlice;v[10]=d->Texture1DArray.ArraySize;v[11]=Bits(d->Texture1DArray.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_TEXTURE2D:v[7]=d->Texture2D.MostDetailedMip;v[8]=d->Texture2D.MipLevels;v[9]=d->Texture2D.PlaneSlice;v[10]=Bits(d->Texture2D.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_TEXTURE2DARRAY:v[7]=d->Texture2DArray.MostDetailedMip;v[8]=d->Texture2DArray.MipLevels;v[9]=d->Texture2DArray.FirstArraySlice;v[10]=d->Texture2DArray.ArraySize;v[11]=d->Texture2DArray.PlaneSlice;v[12]=Bits(d->Texture2DArray.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_TEXTURE2DMS:break;
            case D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY:v[7]=d->Texture2DMSArray.FirstArraySlice;v[8]=d->Texture2DMSArray.ArraySize;break;
            case D3D12_SRV_DIMENSION_TEXTURE3D:v[7]=d->Texture3D.MostDetailedMip;v[8]=d->Texture3D.MipLevels;v[9]=Bits(d->Texture3D.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_TEXTURECUBE:v[7]=d->TextureCube.MostDetailedMip;v[8]=d->TextureCube.MipLevels;v[9]=Bits(d->TextureCube.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_TEXTURECUBEARRAY:v[7]=d->TextureCubeArray.MostDetailedMip;v[8]=d->TextureCubeArray.MipLevels;v[9]=d->TextureCubeArray.First2DArrayFace;v[10]=d->TextureCubeArray.NumCubes;v[11]=Bits(d->TextureCubeArray.ResourceMinLODClamp);break;
            case D3D12_SRV_DIMENSION_RAYTRACING_ACCELERATION_STRUCTURE:v[7]=d->RaytracingAccelerationStructure.Location;break;
            default:v[0]=0;break;
            }
        }if(v[0])DescribeSrvResource(resource,v);
        if(v[0] && !TrackSrvResource(resource,v[2])){v={};Fail(9);}
        Put(cpu,v,resourceStride);
    }catch(...){Fail(2);}
}
void Uav(ID3D12Resource* resource,ID3D12Resource* counter,const D3D12_UNORDERED_ACCESS_VIEW_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu) {
    const auto mode=MutationMode();if(mode==DescriptorMutationMode::Ignore)return;
    if(mode==DescriptorMutationMode::InvalidateTracked){InvalidateInternal(cpu,1,resourceStride);return;}
    observation_profile::Scope profile(observation_profile::Phase::OtherDescriptor);
    try{std::lock_guard lock(mutex);Value v{};v[0]=3;v[2]=Cookie(resource);v[3]=Cookie(counter);v[1]=resource?1:0;
        if((resource && !v[2]) || (counter && !v[3]))v[0]=0;
        if(!d){v[1]|=2;if(!resource)v[0]=0;}
        else {v[4]=d->Format;v[5]=d->ViewDimension;
            switch(d->ViewDimension) {
            case D3D12_UAV_DIMENSION_BUFFER:v[7]=d->Buffer.FirstElement;v[8]=d->Buffer.NumElements;v[9]=d->Buffer.StructureByteStride;v[10]=d->Buffer.CounterOffsetInBytes;v[11]=d->Buffer.Flags;break;
            case D3D12_UAV_DIMENSION_TEXTURE1D:v[7]=d->Texture1D.MipSlice;break;
            case D3D12_UAV_DIMENSION_TEXTURE1DARRAY:v[7]=d->Texture1DArray.MipSlice;v[8]=d->Texture1DArray.FirstArraySlice;v[9]=d->Texture1DArray.ArraySize;break;
            case D3D12_UAV_DIMENSION_TEXTURE2D:v[7]=d->Texture2D.MipSlice;v[8]=d->Texture2D.PlaneSlice;break;
            case D3D12_UAV_DIMENSION_TEXTURE2DARRAY:v[7]=d->Texture2DArray.MipSlice;v[8]=d->Texture2DArray.FirstArraySlice;v[9]=d->Texture2DArray.ArraySize;v[10]=d->Texture2DArray.PlaneSlice;break;
            case D3D12_UAV_DIMENSION_TEXTURE3D:v[7]=d->Texture3D.MipSlice;v[8]=d->Texture3D.FirstWSlice;v[9]=d->Texture3D.WSize;break;
            default:v[0]=0;break;
            }
        }
        if(v[0] && (!TrackSrvResource(resource,v[2]) || !TrackSrvResource(counter,v[3]))){v={};Fail(9);}
        Put(cpu,v,resourceStride);
    }catch(...){Fail(2);}
}
void Sampler(const D3D12_SAMPLER_DESC* d,D3D12_CPU_DESCRIPTOR_HANDLE cpu) {
    const auto mode=MutationMode();if(mode==DescriptorMutationMode::Ignore)return;
    if(mode==DescriptorMutationMode::InvalidateTracked){InvalidateInternal(cpu,1,samplerStride);return;}
    observation_profile::Scope profile(observation_profile::Phase::OtherDescriptor);
    try{std::lock_guard lock(mutex);Value v{};
        if(d){v[0]=4;v[4]=d->Filter;v[5]=d->AddressU;v[6]=d->AddressV;v[7]=d->AddressW;v[8]=Bits(d->MipLODBias);
            v[9]=d->MaxAnisotropy;v[10]=d->ComparisonFunc;for(int i=0;i<4;++i)v[11+i]=Bits(d->BorderColor[i]);v[15]=Bits(d->MinLOD);v[16]=Bits(d->MaxLOD);}
        Put(cpu,v,samplerStride);
    }catch(...){Fail(2);}
}
void Copy(ID3D12Device* device,UINT n,const D3D12_CPU_DESCRIPTOR_HANDLE* dest,const UINT* sizes,UINT m,const D3D12_CPU_DESCRIPTOR_HANDLE* source,const UINT* counts,D3D12_DESCRIPTOR_HEAP_TYPE type) {
    const auto mode=MutationMode();
    if(mode==DescriptorMutationMode::Ignore || (type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && type!=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER))return;
    if(mode==DescriptorMutationMode::InvalidateTracked){
        if(!device || (n && !dest) || n>65536){Fail(11);return;}
        try{std::lock_guard lock(mutex);DrainRetired();SelectStorage();
            const auto stride=device->GetDescriptorHandleIncrementSize(type);
            for(UINT i=0;i<n;++i)if(store.InvalidateTracked(dest[i].ptr,sizes?sizes[i]:1,stride)==TrackedDescriptorWrite::Rejected){Fail(11);return;}
        }catch(...){Fail(2);}
        return;
    }
    if(!device || !dest || !source || n>65536 || m>65536){if(borrowedTracking.Pins())Fail(3);return;}
    observation_profile::Scope profile(observation_profile::Phase::CopyDescriptors);
    try{std::lock_guard lock(mutex);DrainRetired();SelectStorage();const auto stride=device->GetDescriptorHandleIncrementSize(type);
        uint64_t a{},b{};for(UINT i=0;i<n;++i)a+=sizes?sizes[i]:1;for(UINT i=0;i<m;++i)b+=counts?counts[i]:1;
        if(a!=b || a>Store::SlotLimit){Fail(3);return;}
        UINT di{},si{},da{},sa{};while(di<n && si<m){const UINT dn=sizes?sizes[di]:1,sn=counts?counts[si]:1;
            const auto step=std::min(dn-da,sn-sa);if(step)store.Copy(dest[di].ptr+uint64_t(da)*stride,source[si].ptr+uint64_t(sa)*stride,step,stride);
            da+=step;sa+=step;if(da==dn){++di;da=0;}if(sa==sn){++si;sa=0;}}
    }catch(...){Fail(2);}
}
void BeginCapture() {
    if(!Enabled())return;std::lock_guard lock(mutex);DrainRetired();CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
    borrowedTracking.Invalidate();
    draws.clear();bindings.clear();ids.clear();values.assign(1,Value{});exportedValues.clear();pending.clear();tables.clear();CyberpunkVR_DescriptorPendingLists=0;
    bindingOnly=false;recording=true;SelectStorage();CyberpunkVR_DescriptorCaptureFailure=store.Overflow()?4:trackingFailure.load();Publish();CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
}
void Snapshot(const native_probe::DrawRecord& draw) {
    if(!Enabled())return;std::lock_guard lock(mutex);if(!recording)return;DrainRetired();
    CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
    try {
        DrawBindings result{uint32_t(draw.sequence),uint32_t(bindings.size()),0,0};
        const auto root=roots.find(reinterpret_cast<ID3D12RootSignature*>(draw.root));
        const auto table=tables.find(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list));
        if(root!=roots.end() && root->second.supported && !store.Overflow()) {
            result.complete=(root->second.parameters<=32 || (table!=tables.end() && reinterpret_cast<uintptr_t>(table->second.root)==draw.root))?1u:0u;
            for(auto b:root->second.ranges) {
                const bool tracked=table!=tables.end() && reinterpret_cast<uintptr_t>(table->second.root)==draw.root;
                const auto mask=tracked?table->second.mask:uint64_t(draw.tableMask);
                if(!(mask&(uint64_t(1)<<b.parameter)))continue;
                const auto stride=b.type==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER?samplerStride:resourceStride;
                const auto base=tracked?table->second.addresses[b.parameter]:(b.parameter<32?draw.tables[b.parameter]:0);
                if(!stride || base>UINT64_MAX-uint64_t(b.tableOffset)*stride){result.complete=0;continue;}
                b.gpu=base+uint64_t(b.tableOffset)*stride;const auto range=store.Range(b.gpu,b.count);
                if(range.valid){b.heap=range.heap;b.version=range.version;b.status=1;b.count=range.count;
                    if(b.count<=128){b.valueOffset=uint32_t(ids.size());b.valueCount=b.count;b.status=2;
                        for(UINT i=0;i<b.count;++i){const auto id=Export(store.At(range.cpu+uint64_t(i)*stride));ids.push_back(id);if(!id)b.status=1;}}
                }else result.complete=0;
                if(bindings.size()>=1048576 || ids.size()>8*1048576){CyberpunkVR_DescriptorCaptureFailure=5;recording=false;break;}
                bindings.push_back(b);++result.count;
            }
        }
        if(draws.size()<native_probe::DrawCapacity){pending[reinterpret_cast<ID3D12CommandList*>(draw.list)].push_back(uint32_t(draws.size()));
            draws.push_back(result);CyberpunkVR_DescriptorPendingLists=uint32_t(pending.size());}else CyberpunkVR_DescriptorCaptureFailure=5;
    }catch(...){CyberpunkVR_DescriptorCaptureFailure=2;recording=false;}
    Publish();CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
}
void EndCapture(){if(!Enabled())return;std::lock_guard lock(mutex);recording=false;}
bool BeginBindings(){
    if(!Enabled())return false;std::lock_guard lock(mutex);DrainRetired();
    if(recording || !pending.empty() || store.Overflow() || trackingFailure.load())return false;
    borrowedTracking.Invalidate();tables.clear();bindingOnly=true;SelectStorage();return !trackingFailure.load();
}
void EndBindings(){std::lock_guard lock(mutex);borrowedTracking.Invalidate();bindingOnly=false;tables.clear();SelectStorage();}
bool Healthy(){if(!Enabled())return false;std::lock_guard lock(mutex);DrainRetired();return !store.Overflow() && !trackingFailure.load();}
struct TableMetadataSnapshot::Data {
    struct Heap {ComPtr<ID3D12DescriptorHeap> object;D3D12_DESCRIPTOR_HEAP_DESC desc{};uint64_t cookie{},cpu{},gpu{};UINT stride{};};
    ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12Device> device;
    std::vector<Heap> heaps;RootLayout layout;TableMetadataSummary summary;bool pinned{};
    ~Data(){if(pinned)borrowedTracking.Release();}
    bool Physical()const{
        if(!root || !device)return false;
        for(const auto& heap:heaps){
            if(!heap.object)return false;const auto desc=heap.object->GetDesc();ComPtr<ID3D12Device> actual;
            if(desc.Type!=heap.desc.Type || desc.NumDescriptors!=heap.desc.NumDescriptors || desc.Flags!=heap.desc.Flags ||
                desc.NodeMask!=heap.desc.NodeMask || heap.object->GetCPUDescriptorHandleForHeapStart().ptr!=heap.cpu ||
                heap.object->GetGPUDescriptorHandleForHeapStart().ptr!=heap.gpu ||
                FAILED(heap.object->GetDevice(IID_PPV_ARGS(&actual))) || actual!=device ||
                device->GetDescriptorHandleIncrementSize(desc.Type)!=heap.stride)return false;
        }
        return true;
    }
    bool MatchesLocked()const{
        if(!pinned || !borrowedTracking.Stable(summary.trackingEpoch) || !store.TrackVersions() ||
            store.Overflow() || trackingFailure.load())return false;
        const auto found=roots.find(root.Get());if(found==roots.end())return false;const auto& actual=found->second;
        if(actual.parameters!=layout.parameters || actual.flags!=layout.flags || actual.reasons!=layout.reasons ||
            actual.supported!=layout.supported || actual.ranges.size()!=layout.ranges.size())return false;
        for(size_t i=0;i<layout.ranges.size();++i){const auto& a=actual.ranges[i];const auto& b=layout.ranges[i];
            if(a.parameter!=b.parameter || a.type!=b.type || a.space!=b.space || a.shaderRegister!=b.shaderRegister ||
                a.count!=b.count || a.tableOffset!=b.tableOffset || a.visibility!=b.visibility)return false;
        }
        for(const auto& heap:heaps){uint64_t cookie{};UINT bytes=sizeof(cookie);
            if(FAILED(heap.object->GetPrivateData(cookieGuid,&bytes,&cookie)) || bytes!=sizeof(cookie) ||
                cookie!=heap.cookie || !knownHeaps.contains(cookie))return false;
            const auto full=store.Range(heap.gpu,heap.desc.NumDescriptors);
            if(!full.valid || full.heap!=cookie || full.cpu!=heap.cpu || full.stride!=heap.stride || full.count!=heap.desc.NumDescriptors)return false;
        }
        for(const auto& range:summary.ranges)
            if(!SameBorrowedRange(range.identity,store.Range(range.identity.gpu,range.declaredCount)))return false;
        return borrowedTracking.Stable(summary.trackingEpoch);
    }
};
TableMetadataFailure TableMetadataLastFailure(){return tableMetadataFailure;}
bool CaptureTableMetadata(const ComPtr<ID3D12RootSignature>& root,std::span<const ComPtr<ID3D12DescriptorHeap>> heaps,
    std::span<const BoundTableMetadata> bound,TableMetadataSnapshot& output)try{
    output={};tableMetadataFailure=TableMetadataFailure::Arguments;
    if(!root || heaps.size()>2 || bound.size()>64 || (!bound.empty() && heaps.empty()))return false;
    std::array<uint64_t,64> tables{};uint64_t boundMask{};
    for(const auto& binding:bound){
        if(binding.parameter>=64 || !binding.table.ptr || (boundMask&(uint64_t(1)<<binding.parameter)))return false;
        boundMask|=uint64_t(1)<<binding.parameter;tables[binding.parameter]=binding.table.ptr;
    }
    tableMetadataFailure=TableMetadataFailure::Coverage;
    if(!Enabled() || !borrowedTracking.Ready())return false;
    auto next=std::make_shared<TableMetadataSnapshot::Data>();next->root=root;
    tableMetadataFailure=TableMetadataFailure::Heap;
    if(FAILED(root->GetDevice(IID_PPV_ARGS(&next->device))) || !next->device || next->device->GetNodeCount()!=1)return false;
    for(const auto& object:heaps){TableMetadataSnapshot::Data::Heap heap;heap.object=object;
        if(!object)return false;heap.desc=object->GetDesc();ComPtr<ID3D12Device> actual;
        if((heap.desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && heap.desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER) ||
            heap.desc.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE || !heap.desc.NumDescriptors ||
            heap.desc.NumDescriptors>Store::SlotLimit || heap.desc.NodeMask>1 ||
            FAILED(object->GetDevice(IID_PPV_ARGS(&actual))) || actual!=next->device)return false;
        for(const auto& prior:next->heaps)if(prior.object==object || prior.desc.Type==heap.desc.Type)return false;
        heap.cpu=object->GetCPUDescriptorHandleForHeapStart().ptr;heap.gpu=object->GetGPUDescriptorHandleForHeapStart().ptr;
        heap.stride=next->device->GetDescriptorHandleIncrementSize(heap.desc.Type);
        if(!heap.cpu || !heap.gpu || !heap.stride || uint64_t(heap.desc.NumDescriptors)>std::min(UINT64_MAX-heap.cpu,UINT64_MAX-heap.gpu)/heap.stride)return false;
        next->heaps.push_back(std::move(heap));
    }
    {
        std::lock_guard lock(mutex);DrainRetired();tableMetadataFailure=TableMetadataFailure::Coverage;
        if(!borrowedTracking.Pin())return false;next->pinned=true;SelectStorage();
        auto& summary=next->summary;summary.root=reinterpret_cast<uintptr_t>(root.Get());summary.trackingEpoch=borrowedTracking.Epoch();
        if(!borrowedTracking.Stable(summary.trackingEpoch) || !store.TrackVersions() || store.Overflow() || trackingFailure.load())return false;
        tableMetadataFailure=TableMetadataFailure::RootLayout;
        const auto found=roots.find(root.Get());
        if(found==roots.end() || found->second.parameters>64 || found->second.ranges.size()>256 || (found->second.reasons&~2u))return false;
        next->layout=found->second;const auto& layout=next->layout;
        if(layout.parameters<64 && boundMask>>layout.parameters)return false;
        summary.rootParameters=layout.parameters;summary.rootFlags=layout.flags;summary.layoutReasons=layout.reasons;summary.boundTables=UINT(bound.size());
        if(layout.reasons&2u || layout.flags&D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE)summary.unknownFlags|=TableMetadataOtherRootParameters;
        if(layout.flags&(D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED|D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED))
            summary.unknownFlags|=TableMetadataDirectIndexing;
        uint64_t declaredMask{};for(const auto& range:layout.ranges){
            if(range.parameter>=layout.parameters || range.type>3 || range.visibility>7 || !range.count)return false;
            declaredMask|=uint64_t(1)<<range.parameter;
        }
        if(boundMask&~declaredMask)return false;
        summary.unboundTables=UINT(std::popcount(declaredMask&~boundMask));
        if(summary.unboundTables)summary.unknownFlags|=TableMetadataUnboundTables;
        tableMetadataFailure=TableMetadataFailure::Heap;
        for(auto& heap:next->heaps){UINT bytes=sizeof(heap.cookie);
            if(FAILED(heap.object->GetPrivateData(cookieGuid,&bytes,&heap.cookie)) || bytes!=sizeof(heap.cookie) || !heap.cookie ||
                !knownHeaps.contains(heap.cookie))return false;
            const auto full=store.Range(heap.gpu,heap.desc.NumDescriptors);
            if(!full.valid || full.heap!=heap.cookie || full.cpu!=heap.cpu || full.stride!=heap.stride || full.count!=heap.desc.NumDescriptors)return false;
        }
        for(const auto& declared:layout.ranges){
            if(!(boundMask&(uint64_t(1)<<declared.parameter)))continue;
            const auto type=declared.type==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER?D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER:D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
            const auto heap=std::find_if(next->heaps.begin(),next->heaps.end(),[&](const auto& value){return value.desc.Type==type;});
            tableMetadataFailure=TableMetadataFailure::Heap;if(heap==next->heaps.end())return false;
            const auto table=tables[declared.parameter],delta=uint64_t(declared.tableOffset)*heap->stride;
            tableMetadataFailure=TableMetadataFailure::Bounds;
            if(table<heap->gpu || table-heap->gpu>=uint64_t(heap->desc.NumDescriptors)*heap->stride ||
                (table-heap->gpu)%heap->stride || table>UINT64_MAX-delta)return false;
            const auto identity=store.Range(table+delta,declared.count);
            if(!identity.valid || identity.heap!=heap->cookie || identity.stride!=heap->stride ||
                identity.cpu!=heap->cpu+(identity.gpu-heap->gpu))return false;
            tableMetadataFailure=TableMetadataFailure::Capacity;
            if(summary.ranges.size()==256 || identity.count>TableMetadataSlotLimit-summary.values.size())return false;
            summary.ranges.push_back({declared.parameter,declared.type,declared.space,declared.shaderRegister,declared.count,
                declared.tableOffset,declared.visibility,UINT(summary.values.size()),table,declared.count==UINT_MAX,identity});
            constexpr UINT kinds[]{2,3,1,4};
            for(UINT index=0;index<identity.count;++index){
                const auto* value=store.Get(store.At(identity.cpu+uint64_t(index)*identity.stride));
                summary.values.push_back(value?*value:Value{});
                if(!value || !(*value)[0] || (*value)[0]>4){++summary.unknownSlots;summary.unknownFlags|=TableMetadataUnknownSlots;}
                else if((*value)[0]!=kinds[declared.type])summary.unknownFlags|=TableMetadataWrongDescriptorKind;
                else if(((*value)[0]==2 || (*value)[0]==3) && ((*value)[1]&2))summary.unknownFlags|=TableMetadataImplicitViews;
            }
        }
        summary.complete=!summary.unknownFlags;
        tableMetadataFailure=TableMetadataFailure::Changed;if(!next->MatchesLocked())return false;
    }
    tableMetadataFailure=TableMetadataFailure::Heap;if(!next->Physical())return false;
    {
        std::lock_guard lock(mutex);DrainRetired();tableMetadataFailure=TableMetadataFailure::Changed;
        if(!next->MatchesLocked())return false;
    }
    output.data_=std::move(next);tableMetadataFailure=TableMetadataFailure::None;return true;
}catch(...){output={};tableMetadataFailure=TableMetadataFailure::Exception;return false;}
bool ValidateTableMetadataNoOverwrite(const TableMetadataSnapshot& value)try{
    tableMetadataFailure=TableMetadataFailure::Arguments;const auto held=value.data_;if(!held)return false;
    tableMetadataFailure=TableMetadataFailure::Heap;if(!held->Physical())return false;
    std::lock_guard lock(mutex);DrainRetired();tableMetadataFailure=TableMetadataFailure::Changed;
    if(!held->MatchesLocked())return false;tableMetadataFailure=TableMetadataFailure::None;return true;
}catch(...){tableMetadataFailure=TableMetadataFailure::Exception;return false;}
bool DescribeTableMetadata(const TableMetadataSnapshot& value,TableMetadataSummary& output)try{
    output={};tableMetadataFailure=TableMetadataFailure::Arguments;const auto held=value.data_;if(!held || !ValidateTableMetadataNoOverwrite(value))return false;
    auto summary=held->summary;if(!ValidateTableMetadataNoOverwrite(value))return false;output=std::move(summary);return true;
}catch(...){output={};tableMetadataFailure=TableMetadataFailure::Exception;return false;}
struct BorrowedSourceRanges::Data {
    struct Range {BorrowedUsedRange use;UINT heapIndex{};uint64_t table{};RangeIdentity identity;};
    struct Heap {ComPtr<ID3D12DescriptorHeap> object;D3D12_DESCRIPTOR_HEAP_DESC desc{};uint64_t cookie{},cpu{},gpu{};UINT stride{};};
    bool pinned{};uint64_t epoch{};
    ComPtr<ID3D12Device> device;ComPtr<ID3D12GraphicsCommandList> list;ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pipeline;std::shared_ptr<const families::Family> family;
    native_recording::Lease recording;scene_state::Bindings bindings;
    std::array<Heap,2> heaps;UINT heapCount{};std::vector<Range> ranges;
    struct Inventory {RangeIdentity identity;BorrowedDescriptorInventory counts;std::vector<uint64_t> cookies;};
    mutable std::vector<Inventory> inventory;mutable uint64_t inventorySlots{};
    ~Data(){if(pinned)borrowedTracking.Release();}
    bool Physical()const{
        BorrowedStage(6);
        if(!device || !list || !root || !pipeline || !family || !heapCount || heapCount>heaps.size())return false;
        for(UINT i=0;i<heapCount;++i){const auto& heap=heaps[i];if(!heap.object)return false;
            BorrowedStage(6,i,heap.gpu,heap.cookie);
            const auto now=heap.object->GetDesc();ComPtr<ID3D12Device> actual;
            if(now.Type!=heap.desc.Type || now.NumDescriptors!=heap.desc.NumDescriptors || now.Flags!=heap.desc.Flags ||
                now.NodeMask!=heap.desc.NodeMask || heap.object->GetCPUDescriptorHandleForHeapStart().ptr!=heap.cpu ||
                heap.object->GetGPUDescriptorHandleForHeapStart().ptr!=heap.gpu ||
                FAILED(heap.object->GetDevice(IID_PPV_ARGS(&actual))) || actual!=device ||
                device->GetDescriptorHandleIncrementSize(now.Type)!=heap.stride)return false;
        }
        return true;
    }
    bool MatchesLocked()const{
        BorrowedStage(2);if(!borrowedTracking.Ready())return false;
        BorrowedStage(7,UINT64_MAX,0,epoch,borrowedTracking.Epoch());
        if(!pinned || !borrowedTracking.Stable(epoch) || !store.TrackVersions() || store.Overflow() || trackingFailure.load() || ranges.empty()){
            borrowedDetails=BorrowedTrackingDetails(pinned,borrowedTracking.Ready(),epoch,borrowedTracking.Epoch(),
                borrowedTracking.Pins(),borrowedTracking.Mutations(),store.TrackVersions(),uint32_t(store.Stats()[0]),trackingFailure.load(),ranges.empty());
            return false;
        }
        for(UINT i=0;i<heapCount;++i){const auto& heap=heaps[i];
            uint64_t cookie{};UINT bytes=sizeof(cookie);
            if(FAILED(heap.object->GetPrivateData(cookieGuid,&bytes,&cookie)) || bytes!=sizeof(cookie) || cookie!=heap.cookie ||
                !knownHeaps.contains(cookie)){BorrowedStage(6,i,heap.gpu,heap.cookie,cookie);return false;}
        }
        for(size_t i=0;i<ranges.size();++i){const auto& range=ranges[i];
            if(range.heapIndex>=heapCount){BorrowedStage(10,i,range.identity.gpu);return false;}
            const auto current=store.Range(range.identity.gpu,range.use.count);
            if(!SameBorrowedRange(range.identity,current)){BorrowedStage(11,i,range.identity.gpu,range.identity.version,current.version);return false;}
        }
        BorrowedStage(7,UINT64_MAX,0,epoch,borrowedTracking.Epoch());
        if(!borrowedTracking.Stable(epoch)){
            borrowedDetails=BorrowedTrackingDetails(pinned,borrowedTracking.Ready(),epoch,borrowedTracking.Epoch(),
                borrowedTracking.Pins(),borrowedTracking.Mutations(),store.TrackVersions(),uint32_t(store.Stats()[0]),trackingFailure.load(),ranges.empty());
            return false;
        }
        return true;
    }
};
namespace {
bool SameBorrowedBindings(const scene_state::Bindings& a,const scene_state::Bindings& b){
    if(a.root!=b.root || a.heapCount!=b.heapCount || a.heaps!=b.heaps || a.tableMask!=b.tableMask)return false;
    for(UINT p=0;p<64;++p)if((a.tableMask&(uint64_t(1)<<p)) && a.tables[p].ptr!=b.tables[p].ptr)return false;
    return true;
}
bool ShaderHash(const std::string& value){
    return value.size()==64 && std::all_of(value.begin(),value.end(),[](char c){return (c>='0' && c<='9') || (c>='a' && c<='f');});
}
bool CurrentBorrowedFamily(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root,
                          std::shared_ptr<const families::Family>& family,ID3D12PipelineState*& pipeline){
    family.reset();pipeline=nullptr;native_probe::DrawRecord current;
    if(!native_probe::CurrentState(list,current) || current.list!=reinterpret_cast<uintptr_t>(list) ||
        current.root!=reinterpret_cast<uintptr_t>(root) || !current.pipeline)return false;
    auto bound=families::Bound(list);auto* pso=reinterpret_cast<ID3D12PipelineState*>(current.pipeline);
    if(!bound || bound!=families::Lookup(pso) || bound->sourceRoot.Get()!=root || !ShaderHash(bound->rootSha256) ||
        !ShaderHash(bound->vertexSha256) || (bound->pass.pixelShader?!ShaderHash(bound->pixelSha256):!bound->pixelSha256.empty()) ||
        bound->uses.empty() || bound->uses.size()>4096)return false;
    family=std::move(bound);pipeline=pso;return true;
}
}
bool CaptureBorrowedSourceRanges(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,
    const scene_state::Bindings& bindings,BorrowedSourceRanges& output)try{
    output={};BorrowedStage(2);
    if(!Enabled() || !borrowedTracking.Ready())return false;
    BorrowedStage(1);
    if(!list || !recording.token || !recording.generation ||
        recording.list!=reinterpret_cast<uintptr_t>(list) || !bindings.root || !bindings.heapCount || bindings.heapCount>2 ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    BorrowedStage(3);
    if(!context || !native_recording::ValidateRecording(recording,list,context))return false;
    scene_state::Bindings actual;
    BorrowedStage(4);
    if(!scene_state::CaptureSourceBindings(list,actual) || !SameBorrowedBindings(bindings,actual))return false;
    std::shared_ptr<const families::Family> family;ID3D12PipelineState* pipeline{};
    BorrowedStage(5);
    if(!CurrentBorrowedFamily(list,bindings.root,family,pipeline))return false;
    auto next=std::make_shared<BorrowedSourceRanges::Data>();next->list=list;next->root=bindings.root;
    next->pipeline=pipeline;next->family=family;
    next->recording=recording;next->bindings=bindings;next->heapCount=bindings.heapCount;
    BorrowedStage(6);
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&next->device))) || !next->device || next->device->GetNodeCount()!=1)return false;
    ComPtr<ID3D12Device> rootDevice,pipelineDevice;
    if(FAILED(next->root->GetDevice(IID_PPV_ARGS(&rootDevice))) || rootDevice!=next->device ||
        FAILED(next->pipeline->GetDevice(IID_PPV_ARGS(&pipelineDevice))) || pipelineDevice!=next->device)return false;
    for(UINT i=0;i<bindings.heapCount;++i){auto& heap=next->heaps[i];heap.object=bindings.heaps[i];
        BorrowedStage(6,i);
        if(!heap.object)return false;heap.desc=heap.object->GetDesc();
        if((heap.desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && heap.desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER) ||
            heap.desc.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE || !heap.desc.NumDescriptors ||
            heap.desc.NumDescriptors>Store::SlotLimit || heap.desc.NodeMask>1)return false;
        for(UINT j=0;j<i;++j)if(next->heaps[j].desc.Type==heap.desc.Type || next->heaps[j].object==heap.object)return false;
        heap.cpu=heap.object->GetCPUDescriptorHandleForHeapStart().ptr;heap.gpu=heap.object->GetGPUDescriptorHandleForHeapStart().ptr;
        heap.stride=next->device->GetDescriptorHandleIncrementSize(heap.desc.Type);
        if(!heap.cpu || !heap.gpu || !heap.stride || uint64_t(heap.desc.NumDescriptors)>std::min(UINT64_MAX-heap.cpu,UINT64_MAX-heap.gpu)/heap.stride)return false;
    }
    {
        std::lock_guard lock(mutex);DrainRetired();
        BorrowedStage(7);
        if(!borrowedTracking.Pin()){
            borrowedDetails=BorrowedTrackingDetails(false,borrowedTracking.Ready(),borrowedTracking.Epoch(),borrowedTracking.Epoch(),
                borrowedTracking.Pins(),borrowedTracking.Mutations(),store.TrackVersions(),uint32_t(store.Stats()[0]),trackingFailure.load(),false);
            return false;
        }
        next->pinned=true;SelectStorage();next->epoch=borrowedTracking.Epoch();
        if(!borrowedTracking.Stable(next->epoch) || !store.TrackVersions() || store.Overflow() || trackingFailure.load()){
            borrowedDetails=BorrowedTrackingDetails(next->pinned,borrowedTracking.Ready(),next->epoch,borrowedTracking.Epoch(),
                borrowedTracking.Pins(),borrowedTracking.Mutations(),store.TrackVersions(),uint32_t(store.Stats()[0]),trackingFailure.load(),false);
            return false;
        }
        const auto layout=roots.find(bindings.root);
        constexpr UINT forbidden=D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE|D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED|
            D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED;
        BorrowedStage(8);
        if(layout==roots.end() || !layout->second.supported || !layout->second.parameters || layout->second.parameters>64 ||
            layout->second.flags&forbidden || layout->second.ranges.empty() || layout->second.ranges.size()>256)return false;
        if(layout->second.parameters<64 && bindings.tableMask>>layout->second.parameters)return false;
        std::vector<BorrowedRootRange> declarations;std::vector<BorrowedShaderUse> shaderUses;std::vector<BorrowedUsedRange> used;
        declarations.reserve(layout->second.ranges.size());shaderUses.reserve(family->uses.size());
        for(const auto& range:layout->second.ranges)declarations.push_back({range.parameter,range.type,range.space,range.shaderRegister,
            range.count,range.tableOffset,range.visibility});
        for(const auto& use:family->uses)shaderUses.push_back({use.type,use.visibility,use.space,use.shaderRegister,use.count});
        BorrowedStage(9);
        if(!PlanBorrowedShaderRanges(layout->second.parameters,layout->second.supported,bool(layout->second.flags&forbidden),
            declarations,shaderUses,used))return false;
        for(UINT i=0;i<next->heapCount;++i){auto& heap=next->heaps[i];
            BorrowedStage(6,i,heap.gpu);
            heap.cookie=Cookie(heap.object.Get());if(!heap.cookie || !knownHeaps.contains(heap.cookie))return false;
            const auto full=store.Range(heap.gpu,heap.desc.NumDescriptors);
            if(!full.valid || full.heap!=heap.cookie || full.cpu!=heap.cpu || full.stride!=heap.stride || full.count!=heap.desc.NumDescriptors)return false;
        }
        next->ranges.reserve(used.size());
        for(const auto& rangeUse:used){
            BorrowedStage(10,next->ranges.size());
            if(rangeUse.parameter>=64 || !(bindings.tableMask&(uint64_t(1)<<rangeUse.parameter)) || !rangeUse.count)return false;
            const auto type=rangeUse.type==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER?D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER:D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
            UINT index{};for(;index<next->heapCount;++index)if(next->heaps[index].desc.Type==type)break;
            if(index==next->heapCount)return false;const auto& heap=next->heaps[index];const auto table=bindings.tables[rangeUse.parameter].ptr;
            const auto delta=uint64_t(rangeUse.offset)*heap.stride;
            if(!table || table<heap.gpu || (table-heap.gpu)%heap.stride || table-heap.gpu>=uint64_t(heap.desc.NumDescriptors)*heap.stride ||
                table>UINT64_MAX-delta)return false;
            const auto range=store.Range(table+delta,rangeUse.count);
            BorrowedStage(10,next->ranges.size(),table+delta,rangeUse.count,range.count);
            if(!range.valid || range.heap!=heap.cookie || range.stride!=heap.stride || range.cpu!=heap.cpu+(range.gpu-heap.gpu))return false;
            next->ranges.push_back({rangeUse,index,table,range});
        }
        if(!next->MatchesLocked())return false;
    }
    if(!next->Physical())return false;
    BorrowedStage(3);
    if(camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))!=context ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    BorrowedStage(4);
    if(!scene_state::CaptureSourceBindings(list,actual) || !SameBorrowedBindings(bindings,actual))return false;
    std::shared_ptr<const families::Family> finalFamily;ID3D12PipelineState* finalPipeline{};
    BorrowedStage(5);
    if(!CurrentBorrowedFamily(list,bindings.root,finalFamily,finalPipeline) || finalFamily!=family || finalPipeline!=pipeline)return false;
    {
        std::lock_guard lock(mutex);DrainRetired();if(!next->MatchesLocked())return false;
    }
    output.data_=std::move(next);BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
bool ValidateBorrowedSourceRanges(const BorrowedSourceRanges& value)try{
    BorrowedStage(1);
    const auto held=value.data_;if(!held || !held->Physical())return false;
    std::lock_guard lock(mutex);DrainRetired();if(!held->MatchesLocked())return false;BorrowedStage(0);return true;
}catch(...){BorrowedStage(12);return false;}
bool DescribeBorrowedSourceRanges(const BorrowedSourceRanges& value,BorrowedRangeSummary& output)try{
    output={};BorrowedStage(1);const auto held=value.data_;if(!held || !ValidateBorrowedSourceRanges(value))return false;
    BorrowedRangeSummary next;next.list=held->recording.list;next.generation=held->recording.generation;
    next.root=reinterpret_cast<uintptr_t>(held->root.Get());next.pipeline=reinterpret_cast<uintptr_t>(held->pipeline.Get());next.family=held->family->id;
    next.ranges.reserve(held->ranges.size());
    for(const auto& range:held->ranges){const auto& identity=range.identity;const auto& use=range.use;
        next.ranges.push_back({use.parameter,use.type,use.offset,identity.count,identity.stride,use.shaderDeclaredCount,
            range.table,identity.gpu,identity.heap,identity.version,use.physicalRemainder});
    }
    if(!ValidateBorrowedSourceRanges(value))return false;output=std::move(next);return true;
}catch(...){output={};BorrowedStage(12);return false;}
bool InventoryBorrowedSourceRange(const BorrowedSourceRanges& value,UINT index,BorrowedRangeInventory& output)try{
    output={};if(!ValidateBorrowedSourceRanges(value))return false;const auto held=value.data_;
    BorrowedStage(13,index);
    if(index>=held->ranges.size() || !held->ranges[index].use.physicalRemainder)return false;
    const auto& range=held->ranges[index];const auto& identity=range.identity;
    if(!identity.count || identity.count>Store::SlotLimit || !identity.stride || !identity.cpu ||
        uint64_t(identity.count)>(UINT64_MAX-identity.cpu)/identity.stride)return false;
    BorrowedRangeInventory next;next.list=held->recording.list;next.generation=held->recording.generation;
    next.root=reinterpret_cast<uintptr_t>(held->root.Get());next.pipeline=reinterpret_cast<uintptr_t>(held->pipeline.Get());
    next.family=held->family->id;next.rangeIndex=index;
    next.range={range.use.parameter,range.use.type,range.use.offset,identity.count,identity.stride,range.use.shaderDeclaredCount,
        range.table,identity.gpu,identity.heap,identity.version,range.use.physicalRemainder};
    {
        std::lock_guard lock(mutex);DrainRetired();if(!held->MatchesLocked())return false;
        auto found=std::find_if(held->inventory.begin(),held->inventory.end(),[&](const auto& cached){return SameBorrowedRange(cached.identity,identity);});
        if(found==held->inventory.end()){
            BorrowedStage(13,index,identity.gpu,Store::SlotLimit,held->inventorySlots+identity.count);
            if(identity.count>Store::SlotLimit-held->inventorySlots)return false;
            BorrowedSourceRanges::Data::Inventory captured;captured.identity=identity;
            for(uint32_t i=0;i<identity.count;++i){
                const auto* descriptor=store.Get(store.At(identity.cpu+uint64_t(i)*identity.stride));
                descriptor_views::SrvResult decoded;
                const bool srvValid=descriptor && (*descriptor)[0]==2 && descriptor_views::DecodeSrv(*descriptor,decoded);
                const auto cookie=InventoryBorrowedDescriptor(descriptor,srvValid,captured.counts);
                if(cookie)captured.cookies.push_back(cookie);
            }
            std::sort(captured.cookies.begin(),captured.cookies.end());
            captured.cookies.erase(std::unique(captured.cookies.begin(),captured.cookies.end()),captured.cookies.end());
            captured.counts.uniqueCookies=captured.cookies.size();
            held->inventory.push_back(std::move(captured));held->inventorySlots+=identity.count;found=std::prev(held->inventory.end());
        }
        next.counts=found->counts;DrainRetired();
        for(const auto cookie:found->cookies)if(knownResources.contains(cookie))++next.counts.trackedCookies;else ++next.counts.untrackedCookies;
        if(!held->MatchesLocked())return false;
    }
    if(!ValidateBorrowedSourceRanges(value))return false;output=std::move(next);BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
namespace borrowed_fixed_detail {
DescriptorClass Classify(const Value& value,D3D12_SHADER_RESOURCE_VIEW_DESC& output)noexcept{
    output={};if(!value[0])return DescriptorClass::Unknown;
    if(value[0]!=2)return DescriptorClass::UnsupportedDescriptor;
    if((value[1]&2) || (value[5]!=D3D12_SRV_DIMENSION_TEXTURE2D && value[5]!=D3D12_SRV_DIMENSION_TEXTURE2DARRAY))
        return DescriptorClass::UnsupportedView;
    descriptor_views::SrvResult decoded;
    if(!descriptor_views::DecodeSrv(value,decoded) || decoded.implicit)return DescriptorClass::UnsupportedDescriptor;
    if(!decoded.resourceCookie){
        if(value[1] || value[2] || value[3])return DescriptorClass::UnsupportedDescriptor;
        UINT first{},mips{},plane{};float clamp{};
        if(decoded.view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2D){const auto& view=decoded.view.Texture2D;
            first=view.MostDetailedMip;mips=view.MipLevels;plane=view.PlaneSlice;clamp=view.ResourceMinLODClamp;
        }else{const auto& view=decoded.view.Texture2DArray;
            if(!view.ArraySize || view.ArraySize==UINT_MAX || view.FirstArraySlice>=D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION ||
                view.ArraySize>D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION-view.FirstArraySlice)return DescriptorClass::UnsupportedDescriptor;
            first=view.MostDetailedMip;mips=view.MipLevels;plane=view.PlaneSlice;clamp=view.ResourceMinLODClamp;
        }
        if(!mips || first>=D3D12_REQ_MIP_LEVELS || (mips!=UINT_MAX && mips>D3D12_REQ_MIP_LEVELS-first) || plane ||
            !std::isfinite(clamp) || clamp<0 || clamp>D3D12_REQ_MIP_LEVELS-1)return DescriptorClass::UnsupportedDescriptor;
        output=decoded.view;return DescriptorClass::TypedNull;
    }
    if(decoded.metadata.described && (decoded.metadata.dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || decoded.metadata.flags))
        return DescriptorClass::UnsupportedShape;
    output=decoded.view;return DescriptorClass::Candidate;
}
namespace {
bool Format(DXGI_FORMAT resource,DXGI_FORMAT view)noexcept{
    if(resource==view)return true;
    const auto within=[&](DXGI_FORMAT first,DXGI_FORMAT last){return view>=first && view<=last;};
    switch(resource){
    case DXGI_FORMAT_R32G32B32A32_TYPELESS:return within(DXGI_FORMAT_R32G32B32A32_FLOAT,DXGI_FORMAT_R32G32B32A32_SINT);
    case DXGI_FORMAT_R32G32B32_TYPELESS:return within(DXGI_FORMAT_R32G32B32_FLOAT,DXGI_FORMAT_R32G32B32_SINT);
    case DXGI_FORMAT_R16G16B16A16_TYPELESS:return within(DXGI_FORMAT_R16G16B16A16_FLOAT,DXGI_FORMAT_R16G16B16A16_SINT);
    case DXGI_FORMAT_R32G32_TYPELESS:return within(DXGI_FORMAT_R32G32_FLOAT,DXGI_FORMAT_R32G32_SINT);
    case DXGI_FORMAT_R10G10B10A2_TYPELESS:return within(DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UINT);
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:return within(DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R8G8B8A8_SINT);
    case DXGI_FORMAT_R16G16_TYPELESS:return within(DXGI_FORMAT_R16G16_FLOAT,DXGI_FORMAT_R16G16_SINT);
    case DXGI_FORMAT_R32_TYPELESS:return within(DXGI_FORMAT_R32_FLOAT,DXGI_FORMAT_R32_SINT);
    case DXGI_FORMAT_R8G8_TYPELESS:return within(DXGI_FORMAT_R8G8_UNORM,DXGI_FORMAT_R8G8_SINT);
    case DXGI_FORMAT_R16_TYPELESS:return view==DXGI_FORMAT_R16_FLOAT || within(DXGI_FORMAT_R16_UNORM,DXGI_FORMAT_R16_SINT);
    case DXGI_FORMAT_R8_TYPELESS:return within(DXGI_FORMAT_R8_UNORM,DXGI_FORMAT_R8_SINT);
    case DXGI_FORMAT_BC1_TYPELESS:return within(DXGI_FORMAT_BC1_UNORM,DXGI_FORMAT_BC1_UNORM_SRGB);
    case DXGI_FORMAT_BC2_TYPELESS:return within(DXGI_FORMAT_BC2_UNORM,DXGI_FORMAT_BC2_UNORM_SRGB);
    case DXGI_FORMAT_BC3_TYPELESS:return within(DXGI_FORMAT_BC3_UNORM,DXGI_FORMAT_BC3_UNORM_SRGB);
    case DXGI_FORMAT_BC4_TYPELESS:return within(DXGI_FORMAT_BC4_UNORM,DXGI_FORMAT_BC4_SNORM);
    case DXGI_FORMAT_BC5_TYPELESS:return within(DXGI_FORMAT_BC5_UNORM,DXGI_FORMAT_BC5_SNORM);
    case DXGI_FORMAT_BC6H_TYPELESS:return within(DXGI_FORMAT_BC6H_UF16,DXGI_FORMAT_BC6H_SF16);
    case DXGI_FORMAT_BC7_TYPELESS:return within(DXGI_FORMAT_BC7_UNORM,DXGI_FORMAT_BC7_UNORM_SRGB);
    case DXGI_FORMAT_B8G8R8A8_TYPELESS:return view==DXGI_FORMAT_B8G8R8A8_UNORM || view==DXGI_FORMAT_B8G8R8A8_UNORM_SRGB;
    case DXGI_FORMAT_B8G8R8X8_TYPELESS:return view==DXGI_FORMAT_B8G8R8X8_UNORM || view==DXGI_FORMAT_B8G8R8X8_UNORM_SRGB;
    default:return false;
    }
}
}
bool ViewMatches(const Value& value,const D3D12_RESOURCE_DESC& resource,const D3D12_SHADER_RESOURCE_VIEW_DESC& view)noexcept{
    if(!texture_read::detail::ImmutableShape(resource) || resource.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
        !Format(resource.Format,view.Format) || !descriptor_views::detail::TypedFormat(view.Format) ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || (value[1]&2))return false;
    if(value[1]&4){
        if(value[13]!=(uint64_t(resource.Dimension)|(uint64_t(resource.Format)<<32)) || value[14]!=resource.Width ||
            value[15]!=(uint64_t(resource.Height)|(uint64_t(resource.DepthOrArraySize)<<32)|(uint64_t(resource.MipLevels)<<48)) ||
            value[16]!=(uint64_t(resource.Flags)|(uint64_t(resource.Layout)<<32)) ||
            value[17]!=(uint64_t(resource.SampleDesc.Count)|(uint64_t(resource.SampleDesc.Quality)<<32)))return false;
    }
    UINT first{},mips{};float clamp{};
    if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2D){
        if(resource.DepthOrArraySize!=1 || view.Texture2D.PlaneSlice)return false;
        first=view.Texture2D.MostDetailedMip;mips=view.Texture2D.MipLevels;clamp=view.Texture2D.ResourceMinLODClamp;
    }else if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2DARRAY){
        const auto& array=view.Texture2DArray;
        if(array.PlaneSlice || !array.ArraySize || array.FirstArraySlice>=resource.DepthOrArraySize ||
            array.ArraySize>resource.DepthOrArraySize-array.FirstArraySlice)return false;
        first=array.MostDetailedMip;mips=array.MipLevels;clamp=array.ResourceMinLODClamp;
    }else return false;
    return first<resource.MipLevels && mips && (mips==UINT_MAX || mips<=UINT(resource.MipLevels)-first) &&
        std::isfinite(clamp) && clamp>=0 && clamp<=resource.MipLevels-1;
}
}
struct BorrowedFixedRange::Data {
    BorrowedSourceRanges witness;
    BorrowedFixedRangeSummary summary;
    std::vector<Value> canonical;
    std::vector<texture_read::ImmutableLease> leases;
};
namespace {
bool FixedRangeIdentity(const BorrowedRangeSummary::Range& expected,const BorrowedRangeSummary::Range& actual){
    return expected.parameter==actual.parameter && expected.type==actual.type && expected.firstOffset==actual.firstOffset &&
        expected.count==actual.count && expected.stride==actual.stride && expected.table==actual.table && expected.gpu==actual.gpu &&
        expected.heap==actual.heap && expected.version==actual.version && expected.physicalRemainder && actual.physicalRemainder;
}
bool FixedCanonical(const std::vector<Value>& saved,const RangeIdentity& range){
    if(saved.size()!=range.count || !range.valid || !range.cpu || !range.stride || range.count>Store::SlotLimit)return false;
    for(UINT i=0;i<range.count;++i){const auto* current=store.Get(store.At(range.cpu+uint64_t(i)*range.stride));
        if(current?*current!=saved[i]:saved[i]!=Value{})return false;
    }
    return true;
}
bool SameFixedRecording(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.list==b.list && a.generation==b.generation && a.context==b.context && a.registry==b.registry;
}
}
bool CaptureBorrowedFixedRange(ID3D12GraphicsCommandList* list,const native_recording::Lease& recording,
    const BorrowedSourceRanges& witness,UINT index,BorrowedFixedRange& output)try{
    output={};BorrowedStage(1);
    if(!list || probe::internalCommands || !Enabled() || !witness.data_ || !SameFixedRecording(recording,witness.data_->recording) ||
        recording.list!=reinterpret_cast<uintptr_t>(list))return false;
    const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
    BorrowedStage(3);if(!context || !native_recording::ValidateRecording(recording,list,context))return false;
    BorrowedRangeSummary rangeSummary;if(!DescribeBorrowedSourceRanges(witness,rangeSummary))return false;
    BorrowedStage(13,index);
    if(index>=rangeSummary.ranges.size() || !rangeSummary.ranges[index].physicalRemainder ||
        rangeSummary.ranges[index].type!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV)return false;
    const auto held=witness.data_;const auto range=held->ranges[index].identity;
    if(!range.count || range.count>Store::SlotLimit)return false;
    auto next=std::make_shared<BorrowedFixedRange::Data>();next->witness=witness;
    auto& summary=next->summary;summary.list=rangeSummary.list;summary.generation=rangeSummary.generation;
    summary.root=rangeSummary.root;summary.pipeline=rangeSummary.pipeline;summary.family=rangeSummary.family;
    summary.rangeIndex=index;summary.range=rangeSummary.ranges[index];summary.slots.resize(range.count);next->canonical.resize(range.count);
    {
        std::lock_guard lock(mutex);DrainRetired();if(!held->MatchesLocked())return false;
        for(UINT i=0;i<range.count;++i){auto& slot=summary.slots[i];slot.offset=summary.range.firstOffset+i;
            const auto* value=store.Get(store.At(range.cpu+uint64_t(i)*range.stride));if(!value)continue;
            next->canonical[i]=*value;slot.cookie=(*value)[2];
            const auto found=knownResources.find(slot.cookie);
            if(found!=knownResources.end())slot.candidate=reinterpret_cast<uintptr_t>(found->second->resource);
        }
        if(!held->MatchesLocked())return false;
    }
    std::map<std::pair<uint64_t,uint64_t>,UINT> assets;
    for(UINT i=0;i<range.count;++i){auto& slot=summary.slots[i];const auto& value=next->canonical[i];
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};
        using C=borrowed_fixed_detail::DescriptorClass;
        switch(borrowed_fixed_detail::Classify(value,view)){
        case C::Unknown:continue;
        case C::TypedNull:slot.status=BorrowedFixedStatus::TypedNull;continue;
        case C::UnsupportedDescriptor:slot.status=BorrowedFixedStatus::UnsupportedDescriptor;continue;
        case C::UnsupportedView:slot.status=BorrowedFixedStatus::UnsupportedView;continue;
        case C::UnsupportedShape:slot.status=BorrowedFixedStatus::UnsupportedShape;continue;
        case C::Candidate:break;
        }
        if(!slot.candidate){slot.status=BorrowedFixedStatus::MissingCandidate;continue;}
        const auto key=std::pair{slot.cookie,slot.candidate};const auto found=assets.find(key);
        if(found!=assets.end())slot.asset=found->second;
        else{
            slot.asset=UINT(summary.assets.size());assets.emplace(key,slot.asset);summary.assets.emplace_back();next->leases.emplace_back();
            auto& asset=summary.assets.back();asset.cookie=slot.cookie;asset.candidate=slot.candidate;
            texture_read::FixedAssetPins pins;asset.stage=1;
            if(!texture_read::PinFixedAsset(list,slot.candidate,pins)){
                asset.status=BorrowedFixedStatus::PinRefused;asset.failure=texture_read::LastFailure();asset.pinDetails=texture_read::FixedPinLastDetails();
            }else{
                asset.pinDetails=texture_read::FixedPinLastDetails();ComPtr<ID3D12Resource> resource;asset.stage=2;
                if(!texture_read::GetFixedPinnedResource(pins,resource) || reinterpret_cast<uintptr_t>(resource.Get())!=slot.candidate){
                    asset.status=BorrowedFixedStatus::PinRefused;asset.failure=texture_read::LastFailure();if(!asset.failure)asset.failure=1;
                }else{
                    asset.stage=3;UINT bytes=sizeof(asset.actualCookie);
                    const auto hr=resource->GetPrivateData(cookieGuid,&bytes,&asset.actualCookie);
                    if(FAILED(hr) || bytes!=sizeof(asset.actualCookie) || asset.actualCookie!=slot.cookie){
                        asset.status=BorrowedFixedStatus::CookieMismatch;asset.failure=FAILED(hr)?UINT(hr):1;
                    }else{
                        asset.stage=4;
                        if(!texture_read::AcquirePinnedImmutable(list,pins,next->leases.back())){
                            asset.status=BorrowedFixedStatus::ImmutableRefused;asset.failure=texture_read::LastFailure();
                        }else{asset.status=BorrowedFixedStatus::Immutable;asset.stage=asset.failure=0;}
                    }
                }
            }
        }
        const auto& asset=summary.assets[slot.asset];slot.status=asset.status;
        if(slot.status==BorrowedFixedStatus::Immutable){const auto& lease=next->leases[slot.asset];
            if(!lease.source.resource || !borrowed_fixed_detail::ViewMatches(value,lease.source.resource->GetDesc(),view))
                slot.status=BorrowedFixedStatus::UnsupportedView;
        }
    }
    for(UINT i=0;i<next->leases.size();++i)if(summary.assets[i].status==BorrowedFixedStatus::Immutable &&
        !texture_read::ValidateImmutable(next->leases[i])){
        auto& asset=summary.assets[i];asset.status=BorrowedFixedStatus::ImmutableRefused;asset.stage=5;asset.failure=texture_read::LastFailure();
        for(auto& slot:summary.slots)if(slot.asset==i && slot.status==BorrowedFixedStatus::Immutable)slot.status=asset.status;
    }
    BorrowedStage(3);
    if(camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))!=context ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    {
        std::lock_guard lock(mutex);DrainRetired();if(!held->MatchesLocked() || !FixedCanonical(next->canonical,range))return false;
    }
    BorrowedStage(3);
    if(camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)))!=context ||
        !native_recording::ValidateRecording(recording,list,context))return false;
    std::array<UINT,10> sampled{};
    for(UINT index=0;index<summary.slots.size();++index){const auto& slot=summary.slots[index];
        ++summary.counts[UINT(slot.status)];
        if(sampled[UINT(slot.status)]++<4)summary.samples.push_back({slot.offset,slot.asset,slot.status,next->canonical[index]});
    }
    summary.allSlotsQualified=summary.counts[UINT(BorrowedFixedStatus::TypedNull)]+summary.counts[UINT(BorrowedFixedStatus::Immutable)]==range.count;
    output.data_=std::move(next);BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool ValidateBorrowedFixedRange(const BorrowedFixedRange& fixed,const BorrowedSourceRanges& witness,UINT index)try{
    BorrowedStage(1);const auto held=fixed.data_;if(!held || !held->summary.allSlotsQualified || !witness.data_)return false;
    BorrowedRangeSummary current;if(!DescribeBorrowedSourceRanges(witness,current))return false;
    BorrowedStage(11,index);
    if(index>=current.ranges.size() || !FixedRangeIdentity(held->summary.range,current.ranges[index]))return false;
    for(const auto& lease:held->leases)if(!lease.owner || !texture_read::ValidateImmutable(lease))return false;
    const auto borrowed=witness.data_;std::lock_guard lock(mutex);DrainRetired();
    if(!borrowed->MatchesLocked() || !FixedCanonical(held->canonical,borrowed->ranges[index].identity))return false;
    BorrowedStage(0);return true;
}catch(...){BorrowedStage(12);return false;}
#endif
bool DescribeBorrowedFixedRange(const BorrowedFixedRange& fixed,BorrowedFixedRangeSummary& output)try{
    output={};BorrowedStage(1);if(!fixed.data_)return false;
    output=fixed.data_->summary;BorrowedStage(0);return true;
}catch(...){output={};BorrowedStage(12);return false;}
void BeforeSubmit(UINT n,ID3D12CommandList* const* lists) {
    if(!Enabled() || !lists || !CyberpunkVR_DescriptorPendingLists.load())return;
    std::lock_guard lock(mutex);DrainRetired();CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
    for(UINT i=0;i<n;++i){const auto found=pending.find(lists[i]);if(found==pending.end())continue;
        for(auto index:found->second){auto& draw=draws[index];draw.complete|=2;
            for(UINT j=draw.offset;j<draw.offset+draw.count;++j){const auto& b=bindings[j];if(!b.status)continue;
                const auto current=store.Range(b.gpu,b.count);
                if(!current.valid || current.heap!=b.heap || current.version!=b.version)draw.complete=(draw.complete&~1u)|4u;}}
        pending.erase(found);
    }
    CyberpunkVR_DescriptorPendingLists=uint32_t(pending.size());Publish();CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
}
void Reset(ID3D12GraphicsCommandList* list) {
    if(!Enabled())return;std::lock_guard lock(mutex);tables.erase(list);
    const auto found=pending.find(list);if(found==pending.end())return;
    CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);for(auto index:found->second)draws[index].complete=8;
    pending.erase(found);CyberpunkVR_DescriptorPendingLists=uint32_t(pending.size());Publish();CyberpunkVR_DescriptorCaptureSeq.fetch_add(1);
}
void Root(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root) {
    if(!Enabled() || CyberpunkVR_NativeStereoProbeState.load()!=1)return;std::lock_guard lock(mutex);if(!recording && !bindingOnly)return;
    if(tables.size()>=512 && !tables.contains(list))return;auto& t=tables[list];if(t.root!=root){t={};t.root=root;}
}
void Table(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE address) {
    if(index>=64 || !Enabled() || CyberpunkVR_NativeStereoProbeState.load()!=1)return;std::lock_guard lock(mutex);if(!recording && !bindingOnly)return;
    const auto found=tables.find(list);if(found!=tables.end()){found->second.addresses[index]=address.ptr;found->second.mask|=uint64_t(1)<<index;}
}
void ClearBindings(ID3D12GraphicsCommandList* list) {
    if(!Enabled())return;std::lock_guard lock(mutex);const auto found=tables.find(list);
    if(found!=tables.end()){found->second.addresses={};found->second.mask=0;}
}
bool FindBinding(uint64_t root,D3D12_DESCRIPTOR_RANGE_TYPE type,UINT space,UINT shaderRegister,
                 D3D12_SHADER_VISIBILITY visibility,UINT& parameter,UINT& tableOffset){
    parameter=tableOffset=UINT_MAX;
    if(!Enabled() || !root || UINT(type)>D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER ||
        UINT(visibility)>D3D12_SHADER_VISIBILITY_MESH)return false;
    std::lock_guard lock(mutex);if(trackingFailure.load())return false;
    const auto found=roots.find(reinterpret_cast<ID3D12RootSignature*>(root));
    if(found==roots.end() || (found->second.reasons&5))return false;
    UINT matches{},matchedParameter{},matchedOffset{};
    for(const auto& range:found->second.ranges){
        if(range.type!=UINT(type) || range.space!=space ||
            (range.visibility!=D3D12_SHADER_VISIBILITY_ALL && range.visibility!=UINT(visibility)) ||
            shaderRegister<range.shaderRegister)continue;
        const uint64_t relative=uint64_t(shaderRegister)-range.shaderRegister;
        if(range.count!=UINT_MAX){
            if(uint64_t(range.shaderRegister)+range.count>uint64_t(UINT_MAX)+1)return false;
            if(relative>=range.count)continue;
        }
        const auto offset=uint64_t(range.tableOffset)+relative;
        if(range.parameter>=64 || offset>UINT_MAX || ++matches!=1)return false;
        matchedParameter=range.parameter;matchedOffset=UINT(offset);
    }
    if(matches!=1)return false;parameter=matchedParameter;tableOffset=matchedOffset;return true;
}
namespace {
bool CbvAddress(uint64_t address,CbvBinding& output) {
    if(!resourceStride || store.Overflow())return false;
    const auto range=store.Range(address,1);if(!range.valid)return false;
    const auto* value=store.Get(store.At(range.cpu));
    if(!value || (*value)[0]!=1 || !(*value)[4] || !(*value)[5] || (*value)[5]>65536 || ((*value)[5]&255) || ((*value)[4]&255))return false;
    output={address,range.heap,(*value)[4],uint32_t((*value)[5])};return true;
}
bool DescriptorAddress(uint64_t address,DescriptorStamp& output) {
    if(!address || store.Overflow() || trackingFailure.load())return false;
    const auto range=store.Range(address,1);if(!range.valid || !range.heap || !knownHeaps.contains(range.heap))return false;
    const auto* value=store.Get(store.At(range.cpu));if(!value)return false;
    const auto& v=*value;
    if(v[0]==1){if(!v[4] || !v[5] || v[5]>65536 || (v[4]&255) || (v[5]&255))return false;}
    else if(v[0]==2){if((v[1]&3)!=1 || !v[2] || !knownResources.contains(v[2]))return false;}
    else if(v[0]!=4)return false;
    output={address,range.heap,v};return true;
}
}
bool ResolveCbv(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,CbvBinding& output) {
    if(!Enabled() || parameter>=64)return false;std::lock_guard lock(mutex);DrainRetired();
    const auto found=tables.find(list);if(found==tables.end() || reinterpret_cast<uintptr_t>(found->second.root)!=root ||
        !(found->second.mask&(uint64_t(1)<<parameter)))return false;
    const auto base=found->second.addresses[parameter];const uint64_t delta=uint64_t(offset)*resourceStride;
    if(base>UINT64_MAX-delta)return false;return CbvAddress(base+delta,output);
}
bool ResolveCbvAddress(uint64_t descriptor,CbvBinding& output) {
    if(!Enabled())return false;std::lock_guard lock(mutex);DrainRetired();return CbvAddress(descriptor,output);
}
bool CaptureDescriptor(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,DescriptorStamp& output){
    output={};if(!Enabled() || !list || parameter>=64 || !resourceStride)return false;
    std::lock_guard lock(mutex);DrainRetired();
    const auto found=tables.find(list);
    if(found==tables.end() || reinterpret_cast<uintptr_t>(found->second.root)!=root || !(found->second.mask&(uint64_t(1)<<parameter)))return false;
    const auto base=found->second.addresses[parameter];const uint64_t delta=uint64_t(offset)*resourceStride;
    if(base>UINT64_MAX-delta)return false;
    DescriptorStamp stamp;
    if(!DescriptorAddress(base+delta,stamp) || (stamp.value[0]!=1 && stamp.value[0]!=2))return false;
    output=stamp;return true;
}
bool MatchesDescriptor(const DescriptorStamp& expected){
    if(!native_probe::ObserveBindings() || !expected.gpu || !expected.heap)return false;
    std::lock_guard lock(mutex);DrainRetired();DescriptorStamp current;
    return DescriptorAddress(expected.gpu,current) && current.heap==expected.heap && current.value==expected.value;
}
bool CaptureCbvSource(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,DescriptorStamp& output,ComPtr<IUnknown>& origin){
    output={};origin.Reset();if(!Enabled() || !list || parameter>=64)return false;
    std::lock_guard lock(mutex);DrainRetired();
    const auto found=tables.find(list);
    if(found==tables.end() || reinterpret_cast<uintptr_t>(found->second.root)!=root || !(found->second.mask&(uint64_t(1)<<parameter)))return false;
    const auto base=found->second.addresses[parameter];const auto delta=uint64_t(offset)*resourceStride;
    DescriptorStamp stamp;if(!resourceStride || base>UINT64_MAX-delta || !DescriptorAddress(base+delta,stamp) || stamp.value[0]!=1)return false;
    if(stamp.value[3]){const auto owner=cbvOrigins.find(stamp.value[3]);
        if(owner==cbvOrigins.end() || owner->second.value!=stamp.value)return false;origin=owner->second.owner;}
    output=stamp;return true;
}
bool ResolveSrvAtDraw(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,SrvBinding& output){
    output={};if(!Enabled() || !list || parameter>=64)return false;
    std::lock_guard lock(mutex);DrainRetired();if(!resourceStride || store.Overflow() || trackingFailure.load())return false;
    const auto found=tables.find(list);
    if(found==tables.end() || reinterpret_cast<uintptr_t>(found->second.root)!=root || !(found->second.mask&(uint64_t(1)<<parameter)))return false;
    const auto base=found->second.addresses[parameter];const uint64_t delta=uint64_t(offset)*resourceStride;
    if(base>UINT64_MAX-delta)return false;
    DescriptorStamp stamp;if(!DescriptorAddress(base+delta,stamp) || stamp.value[0]!=2)return false;
    const auto source=knownResources.find(stamp.value[2]);if(source==knownResources.end())return false;
    SrvBinding resolved;resolved.description=stamp.value;resolved.stamp=stamp;resolved.resource=source->second->resource;
    uint64_t cookie{};UINT bytes=sizeof(cookie);
    if(!resolved.resource || FAILED(resolved.resource->GetPrivateData(cookieGuid,&bytes,&cookie)) || bytes!=sizeof(cookie) || cookie!=stamp.value[2])return false;
    const auto& v=stamp.value;auto& view=resolved.view;
    view.Format=DXGI_FORMAT(v[4]);view.ViewDimension=D3D12_SRV_DIMENSION(v[5]);view.Shader4ComponentMapping=UINT(v[6]);
    if(view.ViewDimension==D3D12_SRV_DIMENSION_BUFFER)view.Buffer={v[7],UINT(v[8]),UINT(v[9]),D3D12_BUFFER_SRV_FLAGS(v[10])};
    else if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2D)view.Texture2D={UINT(v[7]),UINT(v[8]),UINT(v[9]),std::bit_cast<float>(uint32_t(v[10]))};
    else if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2DARRAY)view.Texture2DArray={UINT(v[7]),UINT(v[8]),UINT(v[9]),UINT(v[10]),UINT(v[11]),std::bit_cast<float>(uint32_t(v[12]))};
    else return false;
    output=std::move(resolved);return true;
}
bool ResolveSamplerAtDraw(ID3D12GraphicsCommandList* list,uint64_t root,UINT parameter,UINT offset,SamplerBinding& output){
    output={};if(!Enabled() || !list || parameter>=64)return false;
    std::lock_guard lock(mutex);DrainRetired();if(!samplerStride)return false;
    const auto found=tables.find(list);
    if(found==tables.end() || reinterpret_cast<uintptr_t>(found->second.root)!=root || !(found->second.mask&(uint64_t(1)<<parameter)))return false;
    const auto base=found->second.addresses[parameter];const uint64_t delta=uint64_t(offset)*samplerStride;
    if(base>UINT64_MAX-delta)return false;
    SamplerBinding resolved;if(!DescriptorAddress(base+delta,resolved.stamp) || resolved.stamp.value[0]!=4)return false;
    const auto& v=resolved.stamp.value;auto& view=resolved.view;
    view.Filter=D3D12_FILTER(v[4]);view.AddressU=D3D12_TEXTURE_ADDRESS_MODE(v[5]);
    view.AddressV=D3D12_TEXTURE_ADDRESS_MODE(v[6]);view.AddressW=D3D12_TEXTURE_ADDRESS_MODE(v[7]);
    view.MipLODBias=std::bit_cast<float>(uint32_t(v[8]));view.MaxAnisotropy=UINT(v[9]);view.ComparisonFunc=D3D12_COMPARISON_FUNC(v[10]);
    for(UINT i=0;i<4;++i)view.BorderColor[i]=std::bit_cast<float>(uint32_t(v[11+i]));
    view.MinLOD=std::bit_cast<float>(uint32_t(v[15]));view.MaxLOD=std::bit_cast<float>(uint32_t(v[16]));
    output=resolved;return true;
}
bool ResolveCpuCbvAddress(uint64_t descriptor,CbvBinding& output) {
    if(!native_probe::ObserveBindings() || !descriptor)return false;std::lock_guard lock(mutex);DrainRetired();
    if(store.Overflow() || trackingFailure.load())return false;
    const auto* value=store.Get(store.At(descriptor));
    if(!value || (*value)[0]!=1 || !(*value)[4] || !(*value)[5] || (*value)[5]>65536 || ((*value)[4]&255) || ((*value)[5]&255))return false;
    output={descriptor,0,(*value)[4],uint32_t((*value)[5])};return true;
}
namespace {
thread_local SourceTableFailure sourceFailure;
std::atomic<uint64_t> nextSourceIdentity{1};
struct __declspec(uuid("18495B39-DC38-43BD-88FB-763265F404A9")) SourceOwnerProof:IUnknown {
    virtual bool MatchesSrv(const OwnedSourceTable&,UINT,const SrvBinding&)const noexcept=0;
};
struct __declspec(uuid("66C72536-4657-4498-A4C0-6F118D661B95")) SourceCaptureProof:IUnknown {
    virtual bool MatchesSrv(const OwnedSourceTable&,UINT,const SrvBinding&)const noexcept=0;
};
bool SourceSrvView(const Value& value,const D3D12_SHADER_RESOURCE_VIEW_DESC& view){
    descriptor_views::SrvResult decoded;if(!descriptor_views::DecodeSrv(value,decoded) || decoded.implicit)return false;
    const auto& expected=decoded.view;
    if(view.Format!=expected.Format || view.ViewDimension!=expected.ViewDimension || view.Shader4ComponentMapping!=expected.Shader4ComponentMapping)return false;
    if(view.ViewDimension==D3D12_SRV_DIMENSION_BUFFER)return view.Buffer.FirstElement==expected.Buffer.FirstElement &&
        view.Buffer.NumElements==expected.Buffer.NumElements && view.Buffer.StructureByteStride==expected.Buffer.StructureByteStride && view.Buffer.Flags==expected.Buffer.Flags;
    if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2D)return view.Texture2D.MostDetailedMip==expected.Texture2D.MostDetailedMip &&
        view.Texture2D.MipLevels==expected.Texture2D.MipLevels && view.Texture2D.PlaneSlice==expected.Texture2D.PlaneSlice &&
        std::bit_cast<uint32_t>(view.Texture2D.ResourceMinLODClamp)==std::bit_cast<uint32_t>(expected.Texture2D.ResourceMinLODClamp);
    if(view.ViewDimension==D3D12_SRV_DIMENSION_TEXTURE2DARRAY)return view.Texture2DArray.MostDetailedMip==expected.Texture2DArray.MostDetailedMip &&
        view.Texture2DArray.MipLevels==expected.Texture2DArray.MipLevels && view.Texture2DArray.FirstArraySlice==expected.Texture2DArray.FirstArraySlice &&
        view.Texture2DArray.ArraySize==expected.Texture2DArray.ArraySize && view.Texture2DArray.PlaneSlice==expected.Texture2DArray.PlaneSlice &&
        std::bit_cast<uint32_t>(view.Texture2DArray.ResourceMinLODClamp)==std::bit_cast<uint32_t>(expected.Texture2DArray.ResourceMinLODClamp);
    return false;
}
struct SourceTableOwner final:SourceOwnerProof {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12DescriptorHeap> heap;
    std::vector<Value> key;
    std::vector<ComPtr<IUnknown>> leases;
    std::map<uint64_t,ComPtr<ID3D12Resource>> resources;
    D3D12_DESCRIPTOR_HEAP_TYPE type{};uint64_t identity{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown) && id!=__uuidof(SourceOwnerProof))return E_NOINTERFACE;
        *output=static_cast<SourceOwnerProof*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs;if(!left)delete this;return left;}
    bool MatchesSrv(const OwnedSourceTable& table,UINT offset,const SrvBinding& binding)const noexcept override{
        if(table.owner.Get()!=static_cast<const IUnknown*>(this) || table.cpuHeap!=heap || table.identity!=identity || table.version!=1 ||
            table.count!=key.size() || offset>=key.size() || type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || !binding.resource)return false;
        const auto& value=key[offset];if(value[0]!=2 || !value[2] || value!=binding.description || value!=binding.stamp.value || !SourceSrvView(value,binding.view))return false;
        const auto found=resources.find(value[2]);
        return found!=resources.end() && found->second==binding.resource &&
            std::any_of(leases.begin(),leases.end(),[&](const auto& held){return held.Get()==static_cast<IUnknown*>(binding.resource.Get());});
    }
};
struct SourceCaptureTable;
struct SourceTableReceipt final:SourceCaptureProof {
    std::atomic<ULONG> refs{1};const OwnedSourceTable table;const UINT stride;
    SourceTableReceipt(const SourceCaptureTable&,SourceTableOwner*);
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown) && id!=__uuidof(SourceCaptureProof))return E_NOINTERFACE;
        *output=static_cast<SourceCaptureProof*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto left=--refs;if(!left)delete this;return left;}
    bool MatchesSrv(const OwnedSourceTable& held,UINT offset,const SrvBinding& binding)const noexcept override{
        if(held.parameter!=table.parameter || held.count!=table.count || held.identity!=table.identity || held.version!=table.version ||
            held.sourceGpu!=table.sourceGpu || held.sourceHeap!=table.sourceHeap || held.owner!=table.owner || held.cpuHeap!=table.cpuHeap ||
            offset>=table.count || !stride || uint64_t(offset)>(UINT64_MAX-table.sourceGpu)/stride ||
            binding.stamp.gpu!=table.sourceGpu+uint64_t(offset)*stride || binding.stamp.heap!=table.sourceHeap)return false;
        ComPtr<SourceOwnerProof> owner;return SUCCEEDED(table.owner.As(&owner)) && owner->MatchesSrv(table,offset,binding);
    }
};
struct SourceCaptureSlot {
    Value value{};DescriptorStamp stamp{};ComPtr<ID3D12Resource> resource,counter;
    ComPtr<IUnknown> origin,extra;uint64_t resourceCookie{};bool used{};UINT kind{};
};
struct SourceCaptureTable {
    UINT parameter{};D3D12_DESCRIPTOR_HEAP_TYPE type{};RangeIdentity range;
    std::vector<SourceCaptureSlot> slots;
};
SourceTableReceipt::SourceTableReceipt(const SourceCaptureTable& captured,SourceTableOwner* owner):
    table{captured.parameter,UINT(captured.slots.size()),owner->heap,owner,owner->identity,1,captured.range.gpu,captured.range.heap},
    stride(captured.range.stride){}
struct SourceCaptureSlots {
    UINT parameter{},count{};D3D12_DESCRIPTOR_HEAP_TYPE type{};RangeIdentity range;
    std::vector<UINT> offsets;std::vector<SourceCaptureSlot> slots;
};
UINT DescriptorKind(UINT type){
    return type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV?1:type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV?2:
        type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV?3:type==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER?4:0;
}
bool SourceError(UINT reason,UINT parameter=UINT_MAX,UINT offset=UINT_MAX){sourceFailure={reason,parameter,offset};return false;}
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* object){
    ComPtr<ID3D12Device> actual;return object && SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool SourceBindingsMatch(ID3D12GraphicsCommandList* list,const scene_state::Bindings& expected,std::span<const SourceTableRequest> requests){
    const auto found=tables.find(list);
    if(found==tables.end() || found->second.root!=expected.root)return false;
    for(const auto& request:requests)if(request.parameter>=64 || !(found->second.mask&(uint64_t(1)<<request.parameter)) ||
        found->second.addresses[request.parameter]!=expected.tables[request.parameter].ptr)return false;
    return true;
}
bool SourceResource(uint64_t cookie,ID3D12Device* device,ComPtr<ID3D12Resource>& output){
    if(!cookie)return true;
    const auto found=knownResources.find(cookie);
    if(found==knownResources.end() || !found->second->refs.load(std::memory_order_acquire) || !found->second->resource)return false;
    ComPtr<ID3D12Resource> held=found->second->resource;uint64_t current{};UINT bytes=sizeof(current);
    if(!SameDevice(device,held.Get()) || FAILED(held->GetPrivateData(cookieGuid,&bytes,&current)) || bytes!=sizeof(current) || current!=cookie)return false;
    output=std::move(held);return true;
}
bool SourceLayout(const RootLayout& root,const SourceTableRequest& request,SourceCaptureTable& output){
    if(request.parameter>=root.parameters || !request.count || request.count>1000000 || request.uses.empty())return false;
    output.parameter=request.parameter;output.slots.resize(request.count);bool found{},sampler{},tail{};uint64_t extent{};
    for(const auto& range:root.ranges)if(range.parameter==request.parameter){
        const auto kind=DescriptorKind(range.type);if(!kind || tail)return false;
        const bool isSampler=kind==4;if(found && sampler!=isSampler)return false;found=true;sampler=isSampler;
        const uint64_t start=range.tableOffset;uint64_t count=range.count;
        if(count==UINT_MAX){tail=true;if(start>=request.count)return false;count=request.count-start;}
        if(start>request.count || count>request.count-start || uint64_t(range.shaderRegister)+count>uint64_t(UINT_MAX)+1)return false;
        extent=std::max(extent,start+count);
        for(uint64_t at=start;at<start+count;++at){auto& slot=output.slots[size_t(at)];if(slot.kind && slot.kind!=kind)return false;slot.kind=kind;}
    }
    if(!found || (!tail && extent!=request.count) || (sampler && request.count>2048))return false;
    output.type=sampler?D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER:D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    for(const auto& use:request.uses){const auto kind=DescriptorKind(UINT(use.type));
        if(!kind || !use.count || use.offset>request.count || use.count>request.count-use.offset)return false;
        for(UINT n=0;n<use.count;++n){auto& slot=output.slots[use.offset+n];if(slot.kind!=kind)return false;slot.used=true;}
    }
    for(auto& slot:output.slots)if(!slot.kind)slot.kind=sampler?4:2;
    return true;
}
bool SourceSlotLayout(const RootLayout& root,const SourceTableRequest& request,SourceCaptureSlots& output){
    if(request.parameter>=root.parameters || !request.count || request.count>1000000 || request.uses.empty() ||
        request.uses.size()>SourceSlotCaptureLimit)return false;
    struct Interval {uint64_t start{},end{};UINT kind{};};std::vector<Interval> ranges;
    bool found{},sampler{},tail{};uint64_t extent{};
    for(const auto& range:root.ranges)if(range.parameter==request.parameter){
        const auto kind=DescriptorKind(range.type);if(!kind || tail)return false;
        const bool isSampler=kind==4;if(found && sampler!=isSampler)return false;found=true;sampler=isSampler;
        const uint64_t start=range.tableOffset;uint64_t count=range.count;
        if(count==UINT_MAX){tail=true;if(start>=request.count)return false;count=request.count-start;}
        if(start>request.count || count>request.count-start || uint64_t(range.shaderRegister)+count>uint64_t(UINT_MAX)+1)return false;
        for(const auto& previous:ranges)if(start<previous.end && previous.start<start+count && previous.kind!=kind)return false;
        ranges.push_back({start,start+count,kind});extent=std::max(extent,start+count);
    }
    if(!found || (!tail && extent!=request.count) || (sampler && request.count>2048))return false;
    std::map<UINT,UINT> selected;
    for(const auto& use:request.uses){const auto kind=DescriptorKind(UINT(use.type));
        if(!kind || !use.count || use.count>SourceSlotCaptureLimit || use.offset>request.count || use.count>request.count-use.offset)return false;
        for(UINT n=0;n<use.count;++n){const UINT offset=use.offset+n;
            if(std::none_of(ranges.begin(),ranges.end(),[&](const auto& range){return offset>=range.start && offset<range.end && range.kind==kind;}))return false;
            const auto entry=selected.emplace(offset,kind).first;if(entry->second!=kind || selected.size()>SourceSlotCaptureLimit)return false;
        }
    }
    output.parameter=request.parameter;output.count=request.count;output.type=sampler?D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER:D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    output.offsets.reserve(selected.size());output.slots.reserve(selected.size());
    for(const auto& [offset,kind]:selected){SourceCaptureSlot slot;slot.kind=kind;slot.used=true;output.offsets.push_back(offset);output.slots.push_back(std::move(slot));}
    return true;
}
bool SourceDescriptorValid(const Value& value,UINT kind){
    if(value[0]!=kind)return false;
    if(kind==1){descriptor_views::CbvResult view;return descriptor_views::DecodeCbv(value,view);}
    if(kind==2){descriptor_views::SrvResult view;return descriptor_views::DecodeSrv(value,view);}
    if(kind==3){descriptor_views::UavResult view;return descriptor_views::DecodeUav(value,view);}
    descriptor_views::SamplerResult view;return descriptor_views::DecodeSampler(value,view);
}
void EmitSourceDescriptor(ID3D12Device* device,const SourceCaptureSlot& slot,D3D12_CPU_DESCRIPTOR_HANDLE target){
    if(!slot.used){
        if(slot.kind==1){device->CreateConstantBufferView(nullptr,target);return;}
        if(slot.kind==3){D3D12_UNORDERED_ACCESS_VIEW_DESC view{};view.Format=DXGI_FORMAT_R32_UINT;view.ViewDimension=D3D12_UAV_DIMENSION_BUFFER;view.Buffer.NumElements=1;
            device->CreateUnorderedAccessView(nullptr,nullptr,&view,target);return;}
        if(slot.kind==4){D3D12_SAMPLER_DESC view{};view.Filter=D3D12_FILTER_MIN_MAG_MIP_POINT;
            view.AddressU=view.AddressV=view.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;view.MaxAnisotropy=1;view.ComparisonFunc=D3D12_COMPARISON_FUNC_NEVER;view.MaxLOD=FLT_MAX;
            device->CreateSampler(&view,target);return;}
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=DXGI_FORMAT_R32_FLOAT;view.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;
        view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Buffer.NumElements=1;device->CreateShaderResourceView(nullptr,&view,target);return;
    }
    if(slot.kind==1){descriptor_views::CbvResult view;descriptor_views::DecodeCbv(slot.value,view);device->CreateConstantBufferView(&view.view,target);}
    else if(slot.kind==2){descriptor_views::SrvResult view;descriptor_views::DecodeSrv(slot.value,view);device->CreateShaderResourceView(slot.resource.Get(),view.Description(),target);}
    else if(slot.kind==3){descriptor_views::UavResult view;descriptor_views::DecodeUav(slot.value,view);device->CreateUnorderedAccessView(slot.resource.Get(),slot.counter.Get(),view.Description(),target);}
    else{descriptor_views::SamplerResult view;descriptor_views::DecodeSampler(slot.value,view);device->CreateSampler(&view.view,target);}
}
}
struct SourceTableCache::Storage {
    ComPtr<ID3D12Device> device;std::vector<ComPtr<SourceTableOwner>> entries;UINT resources{},samplers{};
    std::vector<ComPtr<SourceTableOwner>> slotEntries;UINT slotResources{},slotSamplers{};
};
SourceTableCache::SourceTableCache()=default;
SourceTableCache::~SourceTableCache()=default;
void SourceTableCache::Clear(){storage_.reset();}
UINT SourceTableCache::TableCount()const{return storage_?UINT(storage_->entries.size()):0;}
UINT SourceTableCache::DescriptorCount()const{return storage_?storage_->resources+storage_->samplers:0;}
UINT SourceTableCache::SlotTableCount()const{return storage_?UINT(storage_->slotEntries.size()):0;}
UINT SourceTableCache::SlotDescriptorCount()const{return storage_?storage_->slotResources+storage_->slotSamplers:0;}
SourceTableFailure LastSourceTableFailure(){return sourceFailure;}
bool MatchesCapturedSourceSrv(const OwnedSourceTable& table,UINT offset,const SrvBinding& binding)noexcept{
    if(!table.provenance || !table.owner || !table.cpuHeap || !table.identity || !table.version || !table.count || !table.sourceGpu || !table.sourceHeap)return false;
    ComPtr<SourceCaptureProof> capture;return SUCCEEDED(table.provenance.As(&capture)) && capture->MatchesSrv(table,offset,binding);
}
bool MatchesSourceSlot(const OwnedSourceSlot& slot)try{
    const auto& expected=slot.stamp;const auto kind=DescriptorKind(UINT(slot.type));
    if(!native_probe::ObserveBindings() || !expected.gpu || !expected.heap || !kind || !SourceDescriptorValid(expected.value,kind))return false;
    std::lock_guard lock(mutex);DrainRetired();if(store.Overflow() || trackingFailure.load())return false;
    const auto range=store.Range(expected.gpu,1);
    if(!range.valid || range.heap!=expected.heap || !knownHeaps.contains(range.heap))return false;
    const auto* current=store.Get(store.At(range.cpu));return current && *current==expected.value;
}catch(...){return false;}
bool MatchesCpuSourceSrv(uint64_t cpu,const DescriptorStamp& expected)try{
    if(!native_probe::ObserveBindings() || !cpu || !expected.gpu || !expected.heap || expected.value[0]!=2 ||
        !SourceDescriptorValid(expected.value,2))return false;
    std::lock_guard lock(mutex);DrainRetired();if(store.Overflow() || trackingFailure.load())return false;
    const auto range=store.Range(expected.gpu,1);
    if(!range.valid || range.heap!=expected.heap || !knownHeaps.contains(range.heap))return false;
    const auto* original=store.Get(store.At(range.cpu));const auto* current=store.Get(store.At(cpu));
    return original && current && *original==expected.value && *current==expected.value;
}catch(...){return false;}
bool CaptureSourceTablesAtDraw(ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
    std::span<const SourceTableRequest> requests,SourceCbvProvider provider,void* user,SourceTableCache& cache,std::vector<OwnedSourceTable>& output,
    std::vector<SourceResourceBinding>* dependencies)try{
    sourceFailure={};if(!Enabled() || !device || !list || !bindings.root || !output.empty() || (dependencies && !dependencies->empty()) || requests.size()>64 || !bindings.heapsSeen || bindings.heapCount>2 ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(device,list) || !SameDevice(device,bindings.root))return SourceError(1);
    if(cache.storage_ && cache.storage_->device.Get()!=device)return SourceError(10);
    std::vector<SourceCaptureTable> captured;captured.reserve(requests.size());std::set<UINT> parameters;
    {
        std::lock_guard lock(mutex);DrainRetired();
        if(store.Overflow() || trackingFailure.load() || !SourceBindingsMatch(list,bindings,requests))return SourceError(2);
        const auto layout=roots.find(bindings.root);
        if(layout==roots.end() || !layout->second.supported || layout->second.flags&(D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE|
            D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED|D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED))return SourceError(3);
        for(const auto& request:requests){
            if(request.parameter>=64 || !parameters.insert(request.parameter).second || !(bindings.tableMask&(uint64_t(1)<<request.parameter)))return SourceError(4,request.parameter);
            SourceCaptureTable table;if(!SourceLayout(layout->second,request,table))return SourceError(4,request.parameter);
            table.range=store.Range(bindings.tables[request.parameter].ptr,request.count);
            if(!table.range.valid || !knownHeaps.contains(table.range.heap))return SourceError(5,request.parameter);
            bool boundHeap{};
            for(UINT n=0;n<bindings.heapCount;++n){auto* heap=bindings.heaps[n];
                if(!heap || !SameDevice(device,heap))continue;const auto desc=heap->GetDesc();
                if(desc.Type==table.type && desc.Flags==D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE && Cookie(heap)==table.range.heap)boundHeap=true;
            }
            if(!boundHeap || table.range.stride!=device->GetDescriptorHandleIncrementSize(table.type))return SourceError(5,request.parameter);
            for(UINT n=0;n<request.count;++n){auto& slot=table.slots[n];if(!slot.used)continue;
                const auto* value=store.Get(store.At(table.range.cpu+uint64_t(n)*table.range.stride));
                if(!value)return SourceError(5,request.parameter,n);slot.value=*value;
                if(!SourceDescriptorValid(slot.value,slot.kind))return SourceError(6,request.parameter,n);
                slot.stamp={table.range.gpu+uint64_t(n)*table.range.stride,table.range.heap,slot.value};
                if(slot.kind==1 && slot.value[3]){const auto origin=cbvOrigins.find(slot.value[3]);
                    if(origin==cbvOrigins.end() || origin->second.value!=slot.value)return SourceError(7,request.parameter,n);slot.origin=origin->second.owner;}
                if((slot.kind==2 || slot.kind==3) && (!SourceResource(slot.value[2],device,slot.resource) ||
                    (slot.kind==3 && !SourceResource(slot.value[3],device,slot.counter))))return SourceError(7,request.parameter,n);
            }
            captured.push_back(std::move(table));
        }
    }
    for(auto& table:captured)for(UINT n=0;n<table.slots.size();++n){auto& slot=table.slots[n];if(!slot.used || slot.kind!=1)continue;
        SourceCbvBacking backing;
        if(!provider || !provider(user,list,table.parameter,n,slot.stamp,slot.origin,backing) || !backing.resource || !SameDevice(device,backing.resource.Get()))return SourceError(8,table.parameter,n);
        const auto desc=backing.resource->GetDesc();const auto address=backing.resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !address || slot.value[4]<address || slot.value[4]-address>desc.Width || slot.value[5]>desc.Width-(slot.value[4]-address))return SourceError(8,table.parameter,n);
        slot.resource=std::move(backing.resource);slot.extra=std::move(backing.owner);
    }
    {
        std::lock_guard lock(mutex);DrainRetired();if(!SourceBindingsMatch(list,bindings,requests))return SourceError(9);
        for(auto& table:captured){const auto range=store.Range(table.range.gpu,UINT(table.slots.size()));
            if(!range.valid || range.heap!=table.range.heap || range.cpu!=table.range.cpu || range.stride!=table.range.stride)return SourceError(9,table.parameter);
            for(UINT n=0;n<table.slots.size();++n){auto& slot=table.slots[n];if(!slot.used)continue;
                const auto* value=store.Get(store.At(range.cpu+uint64_t(n)*range.stride));if(!value || *value!=slot.value)return SourceError(9,table.parameter,n);
                if(slot.kind==1){slot.resourceCookie=Cookie(slot.resource.Get());
                    if(!slot.resourceCookie || (slot.value[2] && slot.value[2]!=slot.resourceCookie))return SourceError(7,table.parameter,n);}
            }
        }
    }
    if(!cache.storage_){auto storage=std::make_unique<SourceTableCache::Storage>();storage->device=device;cache.storage_=std::move(storage);}
    std::vector<ComPtr<SourceTableOwner>> pending;std::vector<OwnedSourceTable> result;result.reserve(captured.size());
    UINT resources=cache.storage_->resources,samplers=cache.storage_->samplers;
    for(const auto& table:captured){
        std::vector<Value> key;key.reserve(table.slots.size());std::vector<ComPtr<IUnknown>> leases;std::set<IUnknown*> held;
        std::map<uint64_t,ComPtr<ID3D12Resource>> resourceMap;
        for(const auto& slot:table.slots){Value value{};
            if(slot.used){value=slot.value;
                for(auto* resource:{slot.resource.Get(),slot.counter.Get()})if(resource && held.insert(resource).second)leases.emplace_back(resource);
                for(auto* owner:{slot.origin.Get(),slot.extra.Get()})if(owner && held.insert(owner).second)leases.emplace_back(owner);
                if(slot.kind==1){value[2]=slot.resourceCookie;value[6]=reinterpret_cast<uintptr_t>(slot.extra.Get());value[7]=reinterpret_cast<uintptr_t>(slot.origin.Get());}
                for(const auto& [cookie,resource]:std::array<std::pair<uint64_t,ID3D12Resource*>,2>{{{value[2],slot.resource.Get()},{slot.kind==3?value[3]:0,slot.counter.Get()}}})
                    if(cookie && resource){const auto [entry,inserted]=resourceMap.emplace(cookie,resource);
                        if(!inserted && entry->second.Get()!=resource)return SourceError(7,table.parameter);}
            }else{value[0]=slot.kind?slot.kind:2;value[1]=UINT64_MAX;}
            key.push_back(value);
        }
        ComPtr<SourceTableOwner> owner;
        for(const auto& entry:cache.storage_->entries)if(entry->type==table.type && entry->key==key){owner=entry;break;}
        if(!owner)for(const auto& entry:pending)if(entry->type==table.type && entry->key==key){owner=entry;break;}
        if(owner && owner->resources!=resourceMap)return SourceError(7,table.parameter);
        if(!owner){auto& count=table.type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER?samplers:resources;const UINT limit=table.type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER?2048:1000000;
            if(key.size()>limit-count)return SourceError(12,table.parameter);count+=UINT(key.size());
            owner.Attach(new SourceTableOwner);owner->key=std::move(key);owner->leases=std::move(leases);owner->type=table.type;
            owner->resources=std::move(resourceMap);
            owner->identity=nextSourceIdentity.fetch_add(1,std::memory_order_relaxed);if(!owner->identity)return SourceError(12,table.parameter);
            const D3D12_DESCRIPTOR_HEAP_DESC desc{table.type,UINT(table.slots.size()),D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
            probe::Scope internal;
            if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&owner->heap))))return SourceError(11,table.parameter);
            const auto start=owner->heap->GetCPUDescriptorHandleForHeapStart();const auto stride=device->GetDescriptorHandleIncrementSize(table.type);
            if(!start.ptr || !stride || table.slots.size()-1>(SIZE_MAX-start.ptr)/stride)return SourceError(11,table.parameter);
            for(UINT n=0;n<table.slots.size();++n)EmitSourceDescriptor(device,table.slots[n],{start.ptr+SIZE_T(n)*stride});
            pending.push_back(owner);
        }
        OwnedSourceTable resultTable{table.parameter,UINT(table.slots.size()),owner->heap,owner.Get(),owner->identity,1,table.range.gpu,table.range.heap};
        resultTable.provenance.Attach(new SourceTableReceipt(table,owner.Get()));result.push_back(std::move(resultTable));
    }
    std::vector<SourceResourceBinding> resourcesUsed;
    if(dependencies)for(const auto& table:captured)for(UINT n=0;n<table.slots.size();++n){const auto& slot=table.slots[n];
        if(slot.used && (slot.kind==2 || slot.kind==3))resourcesUsed.push_back({table.parameter,n,
            slot.kind==2?D3D12_DESCRIPTOR_RANGE_TYPE_SRV:D3D12_DESCRIPTOR_RANGE_TYPE_UAV,slot.stamp,slot.resource,slot.counter});}
    cache.storage_->entries.reserve(cache.storage_->entries.size()+pending.size());
    cache.storage_->entries.insert(cache.storage_->entries.end(),pending.begin(),pending.end());
    cache.storage_->resources=resources;cache.storage_->samplers=samplers;output.swap(result);
    if(dependencies)dependencies->swap(resourcesUsed);return true;
}catch(...){return SourceError(13);}
bool CaptureSourceSlotsAtDraw(ID3D12Device* device,ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
    std::span<const SourceTableRequest> requests,SourceCbvProvider provider,void* user,SourceTableCache& cache,std::vector<OwnedSourceSlots>& output,
    std::vector<SourceResourceBinding>* dependencies)try{
    sourceFailure={};if(!Enabled() || !device || !list || !bindings.root || !output.empty() || (dependencies && !dependencies->empty()) || requests.size()>64 || !bindings.heapsSeen || bindings.heapCount>2 ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(device,list) || !SameDevice(device,bindings.root))return SourceError(1);
    if(cache.storage_ && cache.storage_->device.Get()!=device)return SourceError(10);
    std::vector<SourceCaptureSlots> captured;captured.reserve(requests.size());std::set<UINT> parameters;UINT selected{};
    {
        std::lock_guard lock(mutex);DrainRetired();
        if(store.Overflow() || trackingFailure.load() || !SourceBindingsMatch(list,bindings,requests))return SourceError(2);
        const auto layout=roots.find(bindings.root);
        if(layout==roots.end() || !layout->second.supported || layout->second.flags&(D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE|
            D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED|D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED))return SourceError(3);
        for(const auto& request:requests){
            if(request.parameter>=64 || !parameters.insert(request.parameter).second || !(bindings.tableMask&(uint64_t(1)<<request.parameter)))return SourceError(4,request.parameter);
            SourceCaptureSlots table;if(!SourceSlotLayout(layout->second,request,table) || table.slots.size()>SourceSlotCaptureLimit-selected)return SourceError(4,request.parameter);
            selected+=UINT(table.slots.size());table.range=store.Range(bindings.tables[request.parameter].ptr,request.count);
            if(!table.range.valid || !knownHeaps.contains(table.range.heap))return SourceError(5,request.parameter);
            bool boundHeap{};
            for(UINT n=0;n<bindings.heapCount;++n){auto* heap=bindings.heaps[n];
                if(!heap || !SameDevice(device,heap))continue;const auto desc=heap->GetDesc();
                if(desc.Type==table.type && desc.Flags==D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE && Cookie(heap)==table.range.heap)boundHeap=true;
            }
            if(!boundHeap || table.range.stride!=device->GetDescriptorHandleIncrementSize(table.type))return SourceError(5,request.parameter);
            for(UINT n=0;n<table.slots.size();++n){auto& slot=table.slots[n];const UINT offset=table.offsets[n];const uint64_t delta=uint64_t(offset)*table.range.stride;
                if(table.range.cpu>UINT64_MAX-delta || table.range.gpu>UINT64_MAX-delta)return SourceError(5,request.parameter,offset);
                const auto* value=store.Get(store.At(table.range.cpu+delta));if(!value)return SourceError(5,request.parameter,offset);slot.value=*value;
                if(!SourceDescriptorValid(slot.value,slot.kind))return SourceError(6,request.parameter,offset);
                slot.stamp={table.range.gpu+delta,table.range.heap,slot.value};
                if(slot.kind==1 && slot.value[3]){const auto origin=cbvOrigins.find(slot.value[3]);
                    if(origin==cbvOrigins.end() || origin->second.value!=slot.value)return SourceError(7,request.parameter,offset);slot.origin=origin->second.owner;}
                if((slot.kind==2 || slot.kind==3) && (!SourceResource(slot.value[2],device,slot.resource) ||
                    (slot.kind==3 && !SourceResource(slot.value[3],device,slot.counter))))return SourceError(7,request.parameter,offset);
            }
            captured.push_back(std::move(table));
        }
    }
    for(auto& table:captured)for(UINT n=0;n<table.slots.size();++n){auto& slot=table.slots[n];if(slot.kind!=1)continue;const UINT offset=table.offsets[n];
        SourceCbvBacking backing;
        if(!provider || !provider(user,list,table.parameter,offset,slot.stamp,slot.origin,backing) || !backing.resource || !SameDevice(device,backing.resource.Get()))return SourceError(8,table.parameter,offset);
        const auto desc=backing.resource->GetDesc();const auto address=backing.resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !address || slot.value[4]<address || slot.value[4]-address>desc.Width || slot.value[5]>desc.Width-(slot.value[4]-address))return SourceError(8,table.parameter,offset);
        slot.resource=std::move(backing.resource);slot.extra=std::move(backing.owner);
    }
    {
        std::lock_guard lock(mutex);DrainRetired();if(!SourceBindingsMatch(list,bindings,requests))return SourceError(9);
        for(auto& table:captured){const auto range=store.Range(table.range.gpu,table.count);
            if(!range.valid || range.heap!=table.range.heap || range.cpu!=table.range.cpu || range.stride!=table.range.stride)return SourceError(9,table.parameter);
            for(UINT n=0;n<table.slots.size();++n){auto& slot=table.slots[n];const UINT offset=table.offsets[n];
                const auto* value=store.Get(store.At(range.cpu+uint64_t(offset)*range.stride));if(!value || *value!=slot.value)return SourceError(9,table.parameter,offset);
                if(slot.kind==1){slot.resourceCookie=Cookie(slot.resource.Get());
                    if(!slot.resourceCookie || (slot.value[2] && slot.value[2]!=slot.resourceCookie))return SourceError(7,table.parameter,offset);}
            }
        }
    }
    if(!cache.storage_){auto storage=std::make_unique<SourceTableCache::Storage>();storage->device=device;cache.storage_=std::move(storage);}
    std::vector<ComPtr<SourceTableOwner>> pending;std::vector<OwnedSourceSlots> result;result.reserve(captured.size());
    UINT resources=cache.storage_->slotResources,samplers=cache.storage_->slotSamplers;
    for(const auto& table:captured){
        std::vector<Value> key;key.reserve(table.slots.size());std::vector<ComPtr<IUnknown>> leases;std::set<IUnknown*> held;
        for(const auto& slot:table.slots){auto value=slot.value;
            for(auto* resource:{slot.resource.Get(),slot.counter.Get()})if(resource && held.insert(resource).second)leases.emplace_back(resource);
            for(auto* owner:{slot.origin.Get(),slot.extra.Get()})if(owner && held.insert(owner).second)leases.emplace_back(owner);
            if(slot.kind==1){value[2]=slot.resourceCookie;value[6]=reinterpret_cast<uintptr_t>(slot.extra.Get());value[7]=reinterpret_cast<uintptr_t>(slot.origin.Get());}key.push_back(value);
        }
        ComPtr<SourceTableOwner> owner;
        for(const auto& entry:cache.storage_->slotEntries)if(entry->type==table.type && entry->key==key){owner=entry;break;}
        if(!owner)for(const auto& entry:pending)if(entry->type==table.type && entry->key==key){owner=entry;break;}
        if(!owner){auto& count=table.type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER?samplers:resources;const UINT limit=table.type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER?2048:1000000;
            if(key.size()>limit-count)return SourceError(12,table.parameter);count+=UINT(key.size());
            owner.Attach(new SourceTableOwner);owner->key=std::move(key);owner->leases=std::move(leases);owner->type=table.type;
            owner->identity=nextSourceIdentity.fetch_add(1,std::memory_order_relaxed);if(!owner->identity)return SourceError(12,table.parameter);
            const D3D12_DESCRIPTOR_HEAP_DESC desc{table.type,UINT(table.slots.size()),D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};probe::Scope internal;
            if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&owner->heap))))return SourceError(11,table.parameter);
            const auto start=owner->heap->GetCPUDescriptorHandleForHeapStart();const auto stride=device->GetDescriptorHandleIncrementSize(table.type);
            if(!start.ptr || !stride || table.slots.size()-1>(SIZE_MAX-start.ptr)/stride)return SourceError(11,table.parameter);
            for(UINT n=0;n<table.slots.size();++n)EmitSourceDescriptor(device,table.slots[n],{start.ptr+SIZE_T(n)*stride});pending.push_back(owner);
        }
        OwnedSourceSlots owned;owned.parameter=table.parameter;owned.count=table.count;owned.cpuHeap=owner->heap;owned.owner=owner.Get();
        owned.identity=owner->identity;owned.version=1;owned.sourceGpu=table.range.gpu;owned.sourceHeap=table.range.heap;owned.slots.reserve(table.slots.size());
        for(UINT n=0;n<table.slots.size();++n){const auto kind=table.slots[n].kind;const auto type=kind==1?D3D12_DESCRIPTOR_RANGE_TYPE_CBV:kind==2?D3D12_DESCRIPTOR_RANGE_TYPE_SRV:
            kind==3?D3D12_DESCRIPTOR_RANGE_TYPE_UAV:D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;owned.slots.push_back({table.offsets[n],n,type,table.slots[n].stamp});}
        result.push_back(std::move(owned));
    }
    std::vector<SourceResourceBinding> resourcesUsed;
    if(dependencies)for(const auto& table:captured)for(UINT n=0;n<table.slots.size();++n){const auto& slot=table.slots[n];
        if(slot.kind==2 || slot.kind==3)resourcesUsed.push_back({table.parameter,table.offsets[n],
            slot.kind==2?D3D12_DESCRIPTOR_RANGE_TYPE_SRV:D3D12_DESCRIPTOR_RANGE_TYPE_UAV,slot.stamp,slot.resource,slot.counter});}
    cache.storage_->slotEntries.reserve(cache.storage_->slotEntries.size()+pending.size());cache.storage_->slotEntries.insert(cache.storage_->slotEntries.end(),pending.begin(),pending.end());
    cache.storage_->slotResources=resources;cache.storage_->slotSamplers=samplers;output.swap(result);
    if(dependencies)dependencies->swap(resourcesUsed);return true;
}catch(...){return SourceError(13);}
}
#endif
