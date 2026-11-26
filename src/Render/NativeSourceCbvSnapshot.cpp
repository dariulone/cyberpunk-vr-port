#include "Render/NativeSourceCbvSnapshot.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <new>
#include <mutex>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The snapshots at the actual call: rust/crates/cvr-render/src/source_cbv_snapshot_receipt.rs (the arena, the receipt
// with its pins and pool ranges, the registration and its recovery, the proofs; the switches and the pool failure
// row are the moved exports'). This half lends the graph parent's revalidation.
namespace cvr::stereo::source_cbv_snapshot {
static_assert(sizeof(Request)==488 && sizeof(PrivateCbv)==72 && sizeof(PreparedCopies)==48,"mirrored by source_cbv_snapshot_receipt.rs");
extern "C" {
void cvr_scsn_scope_enter(uint64_t,uint64_t,uint64_t,indirect_copy::detail::ScopeStamp*);
void cvr_scsn_scope_exit(const indirect_copy::detail::ScopeStamp*);
bool cvr_scsn_matches_recorded(const PreparedCopies*,const PrivateCbv*);
}
struct SnapshotArena::Storage:IUnknown {};   // the Rust storage's IUnknown face
SnapshotArena::SnapshotArena()=default;SnapshotArena::~SnapshotArena()=default;
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence)noexcept{
    cvr_scsn_scope_enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,&stamp_);}
ActualCallScope::~ActualCallScope(){cvr_scsn_scope_exit(&stamp_);}
bool MatchesRecordedPrivateCbv(const PreparedCopies& proof,const PrivateCbv& value)noexcept{return cvr_scsn_matches_recorded(&proof,&value);}
}  // namespace cvr::stereo::source_cbv_snapshot
#endif
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SourceCbvSnapshotsEnable{},CyberpunkVR_SourceCbvSnapshotsUploadEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_SourceCbvSnapshotsPoolFailure[16]{};
}
namespace cvr::stereo::source_cbv_snapshot {
using Microsoft::WRL::ComPtr;
namespace {
constexpr GUID receiptType{0xe1992d61,0x199b,0x47d3,{0x82,0x9c,0x41,0x8b,0x79,0x30,0x01,0xb2}};
thread_local indirect_copy::detail::ScopeLedger scopes;
thread_local Failure failure{};
std::mutex uploadCodeMutex;
uintptr_t uploadCodeBase{};bool uploadCodeAttempted{},uploadCodeValid{};
bool Enabled(){return CyberpunkVR_SourceCbvSnapshotsEnable.load(std::memory_order_relaxed)!=0;}
bool Read(void*,uint64_t at,void* output,size_t bytes){SIZE_T copied{};return at && at<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(at),output,bytes,&copied) && copied==bytes;}
struct ArenaStorage:IUnknown {
    std::atomic<ULONG> references{1};ComPtr<ID3D12Resource> resource;ComPtr<ID3D12Device> device;
    uint64_t list{},generation{},gpu{},capacity{},cursor{};native_recording::Lease recording;uint64_t state{};
    std::atomic<UINT> pending{};std::atomic<bool> finalized{},failed{};
    virtual ~ArenaStorage()=default;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--references;if(!n)delete this;return n;}
};
struct Pin {ComPtr<IUnknown> allocator,allocation;}; // Reverse destruction releases allocation first.
struct Entry {
    Request request;source_cbv_details::Detail observed;constants::ResourceCreationInfo creation;
    detail::Kind kind{};Pin pin;geometry::BufferLease lease;buffer_read::Proof range;
    uint64_t destination{};uint32_t beforeState{},requiredState{};bool mutated{};
};
void PoolFailure(UINT phase,const Entry& entry,const buffer_read::Proof* current=nullptr){
    const auto details=buffer_read::LastFailureDetails();const auto& source=entry.request.source;
    const uint64_t values[]{phase,entry.request.parameter,entry.request.offset,entry.request.reflectedBytes,source.sourceOffset,source.native.handle,
        buffer_read::LastFailure(),details[0],details[1],details[2],details[3],entry.range.epoch,current?current->epoch:0,
        entry.range.recording,current?current->recording:0,reinterpret_cast<uintptr_t>(source.resource.Get())};
    std::copy(std::begin(values),std::end(values),CyberpunkVR_SourceCbvSnapshotsPoolFailure);
}
enum class Stage:uint32_t {Prepared,Recording,Recorded,Failed};
struct Receipt final:IUnknown {
    std::atomic<ULONG> references{1};std::atomic<Stage> stage{Stage::Prepared};
    ComPtr<ArenaStorage> arena;std::vector<Entry> entries;std::vector<PrivateCbv> views;
    preparation::GraphParentRecord parent;ReadEpochCertificate epoch;indirect_copy::detail::ScopeStamp scope;
    uint64_t root{},base{},copied{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown) && id!=receiptType)return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--references;if(!n)delete this;return n;}
};
ComPtr<Receipt> Owner(const PreparedCopies& proof){ComPtr<Receipt> result;
    if(proof.owner)proof.owner->QueryInterface(receiptType,reinterpret_cast<void**>(result.GetAddressOf()));return result;}
bool PublicProof(const PreparedCopies& proof,const Receipt& owner){return owner.arena && proof.list==owner.epoch.list &&
    proof.generation==owner.epoch.generation && proof.occurrence==owner.epoch.occurrence && proof.copiedBytes==owner.copied && proof.count==owner.entries.size();}
bool SameCreation(const constants::ResourceCreationInfo& a,const constants::ResourceCreationInfo& b){return a.kind==b.kind &&
    a.initialStateKnown==b.initialStateKnown && a.initialState==b.initialState && a.resource==b.resource && a.heap==b.heap &&
    a.heapOffset==b.heapOffset && a.allocationBytes==b.allocationBytes;}
bool SameAllocation(const source_cbv_details::Allocation& a,const source_cbv_details::Allocation& b){
    using O=source_cbv_details::Observation;return a.observation==O::Known && b.observation==O::Known && a.address==b.address &&
        a.vtable==b.vtable && a.pimpl==b.pimpl && a.bytes==b.bytes && a.resource==b.resource && a.heap==b.heap &&
        a.block==b.block && a.handle==b.handle && a.kind==b.kind;
}
bool SameAllocator(const source_cbv_details::Allocator& a,const source_cbv_details::Allocator& b){
    using O=source_cbv_details::Observation;return a.observation==O::Known && b.observation==O::Known && a.address==b.address &&
        a.vtable==b.vtable && a.pimpl==b.pimpl && b.associationKnown && b.pimplMatches;
}
bool SamePool(const source_cbv_details::Pool& a,const source_cbv_details::Pool& b){using O=source_cbv_details::Observation;
    return a.observation==O::Known && b.observation==O::Known && a.entry==b.entry && a.resource==b.resource && a.owner==b.owner &&
        a.unit==b.unit && a.packed==b.packed && a.group==b.group && a.slot==b.slot && a.occupied && b.occupied;
}
detail::Admission Observe(const Request& request,const source_cbv_details::Detail& d,const constants::ResourceCreationInfo& c){
    using O=source_cbv_details::Observation;const auto& s=request.source;detail::Admission a;
    a.resource=reinterpret_cast<uintptr_t>(s.resource.Get());a.gpu=d.resource.gpu;a.width=d.resource.width;a.comOffset=s.sourceOffset;
    a.nativeGpu=s.native.gpu;a.nativeBytes=s.native.bytes;a.reflectedBytes=request.reflectedBytes;a.viewBytes=s.descriptorBytes;
    a.heap=UINT(d.resource.heap.Type);a.flags=UINT(d.resource.flags);a.nativeKind=s.native.kind;a.handle=s.native.handle;a.creator=s.creatorRva;
    a.creationKind=c.kind;a.initialKnown=c.initialStateKnown;a.initialState=UINT(c.initialState);a.creationHeap=reinterpret_cast<uintptr_t>(c.heap.Get());
    a.creationBytes=c.allocationBytes;a.creationOffset=c.heapOffset;a.creationKnown=c.resource==s.resource && (c.kind==1 || c.kind==2);
    a.sourceCurrent=d.status==source_cbv_details::Status::Captured;a.detailKnown=d.resource.observation==O::Known && d.context.observation==O::Known &&
        !d.context.pending && !d.context.submitted;a.state=d.state;a.stateKnown=d.stateQueried && d.stateValueKnown;
    a.poolKnown=d.pool.observation==O::Known && d.pool.unit==512 && d.pool.occupied;
    a.allocationKnown=d.allocation.observation==O::Known && d.allocation.associationKnown && d.allocation.resourceMatches && d.allocation.extentCovers;
    a.allocatorKnown=d.allocator.observation==O::Known && d.allocator.associationKnown && d.allocator.pimplMatches;
    a.allocation=d.allocation.address;a.allocationResource=d.allocation.resource;a.allocationBytes=d.allocation.bytes;a.allocationHeap=d.allocation.heap;
    a.allocationKind=d.allocation.kind;a.allocationPimpl=d.allocation.pimpl;a.allocatorPimpl=d.allocator.pimpl;
    a.direct=d.allocationPath==source_cbv_details::AllocationPath::Direct;
    a.aliasFieldsEmpty=d.record.observation==O::Known && !source_cbv_details::detail::Field<uint64_t>(d.record.bytes,0x20) &&
        !source_cbv_details::detail::Field<uint64_t>(d.record.bytes,0x28);return a;
}
bool Physical(const Entry& entry,uint64_t base){
    constants::ResourceCreationInfo current;if(!constants::QueryResourceCreation(entry.request.source.resource,current) || !SameCreation(entry.creation,current))return false;
    if(entry.kind==detail::Kind::Upload)return !entry.pin.allocation && !entry.pin.allocator;
    const auto allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,entry.observed.allocation.address);
    const auto allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,base,entry.request.source.native.registry,allocation);
    return entry.pin.allocation && entry.pin.allocator && reinterpret_cast<uintptr_t>(entry.pin.allocation.Get())==allocation.address &&
        reinterpret_cast<uintptr_t>(entry.pin.allocator.Get())==allocator.address && SameAllocation(entry.observed.allocation,allocation) && SameAllocator(entry.observed.allocator,allocator);
}
bool Source(ID3D12GraphicsCommandList* list,const Receipt& owner,const Entry& entry,source_cbv_details::Detail& current,bool ready){
    if(!source_cbv_details::CaptureAtSource(list,owner.root,entry.request.parameter,entry.request.offset,entry.request.source,current) || !Physical(entry,owner.base))return false;
    if(current.resource.heap.Type!=entry.observed.resource.heap.Type || current.resource.heapFlags!=entry.observed.resource.heapFlags ||
        current.resource.flags!=entry.observed.resource.flags || current.resource.width!=entry.observed.resource.width || current.resource.gpu!=entry.observed.resource.gpu ||
        current.context!=entry.observed.context)return false;
    if(entry.kind==detail::Kind::Pool)return SamePool(entry.observed.pool,current.pool);
    if(entry.kind==detail::Kind::Tracked)return current.stateQueried && current.stateValueKnown && detail::ReadMask(current.state,ready) &&
        (!ready || (current.state.value&entry.requiredState)==entry.requiredState);
    return UploadCompatible() && detail::InlineUpload(entry.request.source,owner.epoch,entry.request.reflectedBytes) &&
        current.stateQueried && current.stateValueKnown && current.state.status==1 && current.state.value==D3D12_RESOURCE_STATE_GENERIC_READ;
}
bool Scope(ID3D12GraphicsCommandList* list,const Receipt& owner){return list && owner.arena &&
    scopes.Matches(owner.scope,reinterpret_cast<uintptr_t>(list),owner.epoch.generation,owner.epoch.occurrence,GetCurrentThreadId()) &&
    detail::Epoch(owner.epoch,reinterpret_cast<uintptr_t>(list),owner.arena->generation,owner.scope.position) &&
    !owner.arena->failed.load(std::memory_order_acquire) && !owner.arena->finalized.load(std::memory_order_acquire) &&
    owner.base==reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) && preparation::RevalidateGraphParent(owner.parent);
}
bool Check(ID3D12GraphicsCommandList* list,Receipt& owner,bool ready){
    failure=Failure::Scope;if(!Scope(list,owner))return false;
    for(auto& entry:owner.entries){source_cbv_details::Detail current;failure=Failure::Changed;
        if(!Source(list,owner,entry,current,ready))return false;
        if(!ready && entry.kind==detail::Kind::Tracked && current.state.value!=entry.beforeState)return false;
        if(entry.kind==detail::Kind::Pool){
            failure=Failure::Pool;
            if(ready){if(!buffer_read::ValidateRange(entry.lease,entry.range,entry.request.source.sourceOffset,entry.request.reflectedBytes)){PoolFailure(4,entry);return false;}}
            else{buffer_read::Proof fresh;if(!buffer_read::PreflightRange(list,entry.lease,entry.request.source.sourceOffset,entry.request.reflectedBytes,fresh)){PoolFailure(2,entry,&fresh);return false;}
                if(!buffer_read::detail::SameRangeObservation(entry.range,fresh)){PoolFailure(3,entry,&fresh);return false;}}
        }
    }return true;
}
bool Recover(ID3D12GraphicsCommandList* list,Receipt& owner){
    using Register=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
    const auto registerRead=reinterpret_cast<Register>(owner.base+0x337DA0);const auto flush=reinterpret_cast<Flush>(owner.base+0x3E3BB4);
    for(auto& entry:owner.entries)if(entry.mutated){
        if(!Scope(list,owner) || !Physical(entry,owner.base))return false;
        source_cbv::SourceCbvBacking fresh;
        const auto& source=entry.request.source;const auto status=source_cbv::ReadAtSource(list,owner.root,entry.request.parameter,entry.request.offset,source.stamp,source.originOwner.Get(),fresh);
        if((status!=source_cbv::Status::Registered && status!=source_cbv::Status::Created) || !source_cbv_details::detail::SameSource(source,fresh))return false;
        auto state=geometry::ReadStateAtDraw(list,entry.lease);
        if(state.status==4){{probe::Scope internal;flush(reinterpret_cast<void*>(source.recording.context));}state=geometry::ReadStateAtDraw(list,entry.lease);}
        uint32_t required{};if(!detail::RecoveryMask(entry.beforeState,state,required))return false;
        if((state.value&required)!=required){probe::Scope internal;registerRead(reinterpret_cast<void*>(source.recording.context),source.native.handle,required);flush(reinterpret_cast<void*>(source.recording.context));}
        source_cbv_details::Detail current;
        if(!source_cbv_details::CaptureAtSource(list,owner.root,entry.request.parameter,entry.request.offset,source,current) ||
            !Physical(entry,owner.base) || !current.stateValueKnown || !detail::ReadMask(current.state) || (current.state.value&required)!=required)return false;
    }return true;
}
}
struct SnapshotArena::Storage final:ArenaStorage {};
SnapshotArena::SnapshotArena()=default;SnapshotArena::~SnapshotArena()=default;
Failure LastFailure(){return failure;}
void PrepareUploadCodeGuards(uintptr_t base)noexcept {
    std::lock_guard lock(uploadCodeMutex);if(uploadCodeAttempted)return;uploadCodeAttempted=true;uploadCodeBase=base;
    struct Code{uint32_t rva,bytes;uint64_t hash;};
    constexpr Code contract[]{
        {0x3E9860,185,0x0990890D546C54BDull},{0x3E96EC,82,0x792C0A460B932B2Eull},
        {0x3E7D34,106,0x0A0DB7DBCE20E1CDull},{0x3E6CD4,38,0xB56EF3B145CCE822ull},
        {0x5B4028,229,0x4BC3EFD38EC58363ull}};
    std::array<uint8_t,256> body{};bool valid=base && base==reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    for(const auto& code:contract){
        if(!valid || base>UINTPTR_MAX-code.rva-code.bytes || code.bytes>body.size() || !Read(nullptr,base+code.rva,body.data(),code.bytes)){
            valid=false;break;
        }
        if(native_recording::detail::Fingerprint({body.data(),code.bytes})!=code.hash){valid=false;break;}
    }
    uploadCodeValid=valid;
}
bool UploadCompatible()noexcept {std::lock_guard lock(uploadCodeMutex);
    return uploadCodeValid && uploadCodeBase==reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence)noexcept {
    stamp_=scopes.Enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,GetCurrentThreadId());
}
ActualCallScope::~ActualCallScope(){if(stamp_.nonce)scopes.Exit(stamp_);}
bool ActualCallScope::Active()const noexcept{return scopes.Matches(stamp_,stamp_.list,stamp_.generation,stamp_.position,GetCurrentThreadId());}
HRESULT SnapshotArena::Create(ID3D12Device* device,ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t capacity)try{
    if(storage_ || !device || !list || !generation || !capacity || capacity>detail::MaxArenaBytes || (capacity&255) ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return E_INVALIDARG;
    ComPtr<ID3D12Device> actual;if(FAILED(list->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device)return E_INVALIDARG;
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->list=reinterpret_cast<uintptr_t>(list);next->generation=generation;next->capacity=capacity;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;heap.CreationNodeMask=heap.VisibleNodeMask=1;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=capacity;desc.Height=1;desc.DepthOrArraySize=1;
    desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    HRESULT hr;{probe::Scope internal;hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&next->resource));}
    if(FAILED(hr))return hr;next->gpu=next->resource->GetGPUVirtualAddress();if(!next->gpu || next->gpu>UINT64_MAX-capacity)return E_FAIL;
    storage_=std::move(next);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
bool SnapshotArena::Finalized()const{return storage_ && storage_->finalized.load(std::memory_order_acquire) && !storage_->failed.load(std::memory_order_acquire);}
uint64_t SnapshotArena::Bytes()const{return storage_?storage_->cursor:0;}
bool SnapshotArena::Finalize(ID3D12GraphicsCommandList* list){
    failure=Failure::Arguments;if(!storage_ || !list || reinterpret_cast<uintptr_t>(list)!=storage_->list || storage_->finalized || storage_->failed || storage_->pending || !storage_->cursor)return false;
    failure=Failure::Retention;if(!gpu::HasCommandResource(list,storage_.Get()))return false;
    const camera_upload::Context context{storage_->recording.context,storage_->state,storage_->list};
    failure=Failure::Source;if(!native_recording::ValidateRecording(storage_->recording,list,context))return false;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={storage_->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_GENERIC_READ};
    {probe::Scope internal;list->ResourceBarrier(1,&barrier);}storage_->finalized.store(true,std::memory_order_release);failure=Failure::None;return true;
}
bool PrepareAtActualCall(ID3D12GraphicsCommandList* list,uint64_t root,std::span<const Request> requests,
    const preparation::GraphParentRecord& parent,const ReadEpochCertificate& epoch,SnapshotArena& arena,PreparedCopies& output)try{
    std::fill_n(CyberpunkVR_SourceCbvSnapshotsPoolFailure,16,0ull);
    failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
    failure=Failure::Arguments;if(output.owner || !list || !root || requests.empty() || requests.size()>detail::MaxRequests || !arena.storage_ ||
        arena.storage_->list!=reinterpret_cast<uintptr_t>(list) || arena.storage_->generation!=epoch.generation || arena.storage_->finalized || arena.storage_->failed)return false;
    failure=Failure::Scope;const auto scope=scopes.Current();if(!scopes.Matches(scope,reinterpret_cast<uintptr_t>(list),epoch.generation,epoch.occurrence,GetCurrentThreadId()) ||
        !detail::Epoch(epoch,reinterpret_cast<uintptr_t>(list),arena.storage_->generation,scope.position))return false;
    failure=Failure::Code;if(!indirect_copy::Compatible())return false;
    failure=Failure::Parent;if(!indirect_copy::detail::Parent(parent) || !preparation::RevalidateGraphParent(parent))return false;
    ComPtr<Receipt> owner;owner.Attach(new Receipt);owner->arena=arena.storage_.Get();owner->root=root;owner->parent=parent;owner->epoch=epoch;owner->scope=scope;
    owner->base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));owner->entries.reserve(requests.size());owner->views.reserve(requests.size());
    std::vector<uint32_t> views;views.reserve(requests.size());
    for(const auto& request:requests){
        failure=Failure::Arguments;if(request.source.recording.list!=epoch.list || request.source.recording.generation!=epoch.generation ||
            !request.reflectedBytes || request.parameter>=64)return false;
        for(const auto& entry:owner->entries)if(entry.request.parameter==request.parameter && entry.request.offset==request.offset)return false;
        Entry entry;entry.request=request;failure=Failure::Source;
        if(!source_cbv_details::CaptureAtSource(list,root,request.parameter,request.offset,request.source,entry.observed))return false;
        failure=Failure::Creation;if(!constants::QueryResourceCreation(request.source.resource,entry.creation))return false;
        failure=detail::Admit(Observe(request,entry.observed,entry.creation),entry.kind);if(failure!=Failure::None)return false;
        // Only a completed, nonescaping producer in this original recording
        // can inherit its certified epoch. No foreign/replayed source reads.
        if(entry.kind==detail::Kind::Upload && (!CyberpunkVR_SourceCbvSnapshotsUploadEnable.load(std::memory_order_relaxed) ||
            !UploadCompatible() || !detail::InlineUpload(request.source,epoch,request.reflectedBytes))){failure=Failure::Class;return false;}
        entry.lease.resource=request.source.resource;entry.lease.gpu=request.source.gpu;entry.lease.bytes=request.source.backingBytes;
        entry.lease.heapType=UINT(entry.observed.resource.heap.Type);entry.lease.flags=UINT(entry.observed.resource.flags);entry.lease.native=request.source.native;
        entry.beforeState=entry.observed.state.value;entry.requiredState=entry.beforeState|D3D12_RESOURCE_STATE_COPY_SOURCE;
        if(entry.kind!=detail::Kind::Upload){entry.pin.allocator=reinterpret_cast<IUnknown*>(entry.observed.allocator.address);
            entry.pin.allocation=reinterpret_cast<IUnknown*>(entry.observed.allocation.address);failure=Failure::Allocation;if(!Physical(entry,owner->base))return false;}
        if(entry.kind==detail::Kind::Pool){failure=Failure::Pool;if(!buffer_read::PreflightRange(list,entry.lease,request.source.sourceOffset,request.reflectedBytes,entry.range)){PoolFailure(1,entry);return false;}}
        views.push_back(request.source.descriptorBytes);owner->copied+=request.reflectedBytes;owner->entries.push_back(std::move(entry));
    }
    std::vector<uint64_t> offsets;uint64_t end{};failure=Failure::Capacity;
    if(!detail::Reserve(owner->arena->cursor,owner->arena->capacity,views,offsets,end))return false;
    for(size_t i=0;i<owner->entries.size();++i){auto& entry=owner->entries[i];entry.destination=offsets[i];PrivateCbv view;
        view.parameter=entry.request.parameter;view.offset=entry.request.offset;view.reflectedBytes=entry.request.reflectedBytes;view.paddedBytes=entry.request.source.descriptorBytes;
        view.sourceOffset=entry.request.source.sourceOffset;view.privateOffset=offsets[i];view.copiedBytes=entry.request.reflectedBytes;
        view.resource=owner->arena->resource;view.owner=owner->arena.Get();view.view={owner->arena->gpu+offsets[i],view.paddedBytes};owner->views.push_back(std::move(view));}
    if(!Check(list,*owner.Get(),false))return false;
    failure=Failure::Retention;if(!gpu::KeepCommandResources(list,{owner.Get(),owner->arena.Get()}))return false;
    owner->arena->recording=requests[0].source.recording;owner->arena->state=owner->entries[0].observed.context.state;owner->arena->cursor=end;++owner->arena->pending;
    output={owner,epoch.list,epoch.generation,epoch.occurrence,owner->copied,UINT(owner->entries.size())};failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool ValidateAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopies& proof)try{
    failure=Failure::Arguments;const auto owner=Owner(proof);if(!owner || !PublicProof(proof,*owner.Get()))return false;
    const auto stage=owner->stage.load(std::memory_order_acquire);if(stage!=Stage::Prepared && stage!=Stage::Recorded)return false;
    failure=Failure::Retention;if(!gpu::HasCommandResource(list,owner.Get()))return false;
    if(!Check(list,*owner.Get(),stage==Stage::Recorded))return false;failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool RecordAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopies& proof,std::vector<PrivateCbv>& output){
    ComPtr<Receipt> owner;bool recording{};
    try{
    failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
    failure=Failure::Arguments;owner=Owner(proof);if(!owner || !PublicProof(proof,*owner.Get()) || !output.empty())return false;
    for(const auto& entry:owner->entries)if(entry.kind==detail::Kind::Upload && !CyberpunkVR_SourceCbvSnapshotsUploadEnable.load(std::memory_order_relaxed)){failure=Failure::Class;return false;}
    std::vector<PrivateCbv> result=owner->views;if(!ValidateAtActualCall(list,proof))return false;
    auto expected=Stage::Prepared;failure=Failure::AlreadyRecorded;if(!owner->stage.compare_exchange_strong(expected,Stage::Recording))return false;
    recording=true;
    bool ok=true;
    for(auto& entry:owner->entries){
        if(entry.kind==detail::Kind::Pool){failure=Failure::Registration;
            if(!buffer_read::CaptureRange(list,entry.lease,entry.request.source.sourceOffset,entry.request.reflectedBytes,entry.range)){ok=false;break;}}
        else if(entry.kind==detail::Kind::Tracked){
            source_cbv_details::Detail current;failure=Failure::Changed;
            if(!Scope(list,*owner.Get()) || !Source(list,*owner.Get(),entry,current,false) ||
                (current.state.value&entry.beforeState)!=entry.beforeState){ok=false;break;}
            entry.requiredState=current.state.value|D3D12_RESOURCE_STATE_COPY_SOURCE;
            if(current.state.value&D3D12_RESOURCE_STATE_COPY_SOURCE)continue;
            using Register=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
            entry.mutated=true;{probe::Scope internal;
                reinterpret_cast<Register>(owner->base+0x337DA0)(reinterpret_cast<void*>(entry.request.source.recording.context),entry.request.source.native.handle,entry.requiredState);
                reinterpret_cast<Flush>(owner->base+0x3E3BB4)(reinterpret_cast<void*>(entry.request.source.recording.context));}
        }
    }
    if(ok)ok=Check(list,*owner.Get(),true);
    if(!ok){const bool mutated=std::any_of(owner->entries.begin(),owner->entries.end(),[](const auto& entry){return entry.mutated;});
        if(mutated)failure=Recover(list,*owner.Get())?Failure::RecoveryVerified:Failure::RecoveryFatal;
        owner->arena->failed.store(true,std::memory_order_release);--owner->arena->pending;owner->stage.store(Stage::Failed,std::memory_order_release);return false;}
    {probe::Scope internal;for(const auto& entry:owner->entries)list->CopyBufferRegion(owner->arena->resource.Get(),entry.destination,
        entry.request.source.resource.Get(),entry.request.source.sourceOffset,entry.request.reflectedBytes);}
    output.swap(result);--owner->arena->pending;owner->stage.store(Stage::Recorded,std::memory_order_release);failure=Failure::None;return true;
    }catch(...){
        failure=Failure::Exception;
        if(recording && owner){
            const bool mutated=std::any_of(owner->entries.begin(),owner->entries.end(),[](const auto& entry){return entry.mutated;});
            if(mutated){bool recovered{};try{recovered=Recover(list,*owner.Get());}catch(...){}failure=recovered?Failure::RecoveryVerified:Failure::RecoveryFatal;}
            owner->arena->failed.store(true,std::memory_order_release);--owner->arena->pending;owner->stage.store(Stage::Failed,std::memory_order_release);
        }return false;
    }
}
bool MatchesRecordedPrivateCbv(const PreparedCopies& proof,const PrivateCbv& value)noexcept{
    try{
        failure=Failure::Arguments;const auto owner=Owner(proof);
        if(!owner || !PublicProof(proof,*owner.Get()) || !proof.count || proof.count>detail::MaxRequests ||
            !detail::RecordedCopyFields(proof,owner->epoch,owner->copied,owner->entries.size()) ||
            owner->views.size()!=owner->entries.size())return false;
        failure=Failure::Retention;const auto& arena=*owner->arena.Get();
        if(owner->stage.load(std::memory_order_acquire)!=Stage::Recorded || !arena.finalized.load(std::memory_order_acquire) ||
            arena.failed.load(std::memory_order_acquire) || arena.pending.load(std::memory_order_acquire) || !arena.resource ||
            !detail::Epoch(owner->epoch,arena.list,arena.generation,owner->scope.position) ||
            owner->scope.list!=proof.list || owner->scope.generation!=proof.generation)return false;
        failure=Failure::Changed;
        const auto found=std::find_if(owner->views.begin(),owner->views.end(),[&](const PrivateCbv& view){
            return view.parameter==value.parameter && view.offset==value.offset;
        });
        if(found==owner->views.end() || std::find_if(found+1,owner->views.end(),[&](const PrivateCbv& view){
            return view.parameter==value.parameter && view.offset==value.offset;
        })!=owner->views.end())return false;
        const auto index=size_t(found-owner->views.begin());const auto& entry=owner->entries[index];const auto& source=entry.request.source;
        if(!detail::RecordedPrivateIdentity(detail::PrivateIdentity(value),detail::PrivateIdentity(*found),
            reinterpret_cast<uintptr_t>(arena.resource.Get()),reinterpret_cast<uintptr_t>(owner->arena.Get()),
            arena.gpu,arena.cursor,arena.capacity) || entry.destination!=value.privateOffset ||
            entry.request.parameter!=value.parameter || entry.request.offset!=value.offset ||
            entry.request.reflectedBytes!=value.reflectedBytes || source.sourceOffset!=value.sourceOffset ||
            source.descriptorBytes!=value.paddedBytes || !source.recording.token ||
            source.recording.list!=proof.list || source.recording.generation!=proof.generation ||
            source.recording.token!=arena.recording.token || source.recording.context!=arena.recording.context ||
            source.recording.registry!=arena.recording.registry)return false;
        // Finalization publishes all arena metadata; the only later change is
        // failure. Do not renew an expired native epoch merely to read our copy.
        if(owner->stage.load(std::memory_order_acquire)!=Stage::Recorded || !arena.finalized.load(std::memory_order_acquire) ||
            arena.failed.load(std::memory_order_acquire) || arena.pending.load(std::memory_order_acquire))return false;
        failure=Failure::None;return true;
    }catch(...){failure=Failure::Exception;return false;}
}
bool ValidateSourceSubmission(const PreparedCopies& proof,ID3D12CommandQueue* queue)try{
    failure=Failure::Arguments;const auto owner=Owner(proof);if(!queue || !owner || !PublicProof(proof,*owner.Get()) || owner->stage!=Stage::Recorded ||
        !owner->arena->finalized.load(std::memory_order_acquire) || owner->arena->failed.load(std::memory_order_acquire))return false;
    for(const auto& entry:owner->entries){failure=Failure::Source;
        if(!native_recording::ValidateSubmission(entry.request.source.recording,queue) || !Physical(entry,owner->base))return false;
        if(entry.kind==detail::Kind::Upload && (!UploadCompatible() ||
            !detail::InlineUpload(entry.request.source,owner->epoch,entry.request.reflectedBytes) ||
            !native_recording::ValidateSubmission(entry.request.source.creationRecording,queue)))return false;
        if(entry.kind==detail::Kind::Upload && entry.request.source.writerRecording.token &&
            !native_recording::ValidateSubmission(entry.request.source.writerRecording,queue))return false;
        if(entry.kind==detail::Kind::Pool){failure=Failure::Pool;if(!buffer_read::ValidateRange(entry.lease,entry.range,
            entry.request.source.sourceOffset,entry.request.reflectedBytes,queue))return false;}}
    failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
}
#endif
