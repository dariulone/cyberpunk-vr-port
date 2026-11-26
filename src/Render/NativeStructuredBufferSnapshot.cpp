#include "Render/NativeStructuredBufferSnapshot.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <new>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The snapshots at the actual call: rust/crates/cvr-render/src/structured_snapshot_receipt.rs (the arena, the receipt,
// the reuse of whole original views, the registration and its recovery, the proofs; the switch and the finalize
// diagnostic are declared_moved_exports.rs'). This half lends the graph parent's revalidation.
namespace cvr::stereo::structured_buffer_snapshot {
static_assert(sizeof(Request)==1136 && offsetof(Request,parameter)==1112 && sizeof(PreparedCopy)==72 && sizeof(PrivateRange)==40,
    "mirrored by structured_snapshot_receipt.rs");
extern "C" {
void cvr_sbsn_scope_enter(uint64_t,uint64_t,uint64_t,indirect_copy::detail::ScopeStamp*);
void cvr_sbsn_scope_exit(const indirect_copy::detail::ScopeStamp*);
bool cvr_sbsn_scope_active(const indirect_copy::detail::ScopeStamp*);
uint32_t cvr_sbsn_last_failure();
HRESULT cvr_sbsn_arena_create(void*,ID3D12Device*,ID3D12GraphicsCommandList*,uint64_t,uint64_t,uint32_t,void**);
void cvr_sbsn_arena_abort(void*);
bool cvr_sbsn_arena_finalized(void*);
uint64_t cvr_sbsn_arena_bytes(void*);
void* cvr_sbsn_arena_resource(void*);
bool cvr_sbsn_arena_finalize(void*,ID3D12GraphicsCommandList*);
bool cvr_sbsn_prepare(ID3D12GraphicsCommandList*,uint64_t,const Request*,const preparation::GraphParentRecord*,const ReadEpochCertificate*,
    void*,PreparedCopy*);
bool cvr_sbsn_validate(ID3D12GraphicsCommandList*,const PreparedCopy*);
bool cvr_sbsn_record(ID3D12GraphicsCommandList*,const PreparedCopy*,PrivateRange*);
bool cvr_sbsn_recorded_view(const PreparedCopy*,const PrivateRange*,D3D12_SHADER_RESOURCE_VIEW_DESC*);
bool cvr_sbsn_validate_submission(const PreparedCopy*,ID3D12CommandQueue*);
}
struct SnapshotArena::Storage:IUnknown {};   // the Rust storage's IUnknown face
SnapshotArena::SnapshotArena()=default;SnapshotArena::~SnapshotArena()=default;
Failure LastFailure(){return Failure(cvr_sbsn_last_failure());}
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence)noexcept{
    cvr_sbsn_scope_enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,&stamp_);}
ActualCallScope::~ActualCallScope(){cvr_sbsn_scope_exit(&stamp_);}
bool ActualCallScope::Active()const noexcept{return cvr_sbsn_scope_active(&stamp_);}
HRESULT SnapshotArena::Create(ID3D12Device* device,ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t capacity,Layout layout){
    void* made{};const HRESULT hr=cvr_sbsn_arena_create(storage_.Get(),device,list,generation,capacity,uint32_t(layout),&made);
    if(SUCCEEDED(hr))storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(made)));
    return hr;
}
void SnapshotArena::Abort()noexcept{cvr_sbsn_arena_abort(storage_.Get());}
bool SnapshotArena::Finalized()const{return cvr_sbsn_arena_finalized(storage_.Get());}
uint64_t SnapshotArena::Bytes()const{return cvr_sbsn_arena_bytes(storage_.Get());}
Microsoft::WRL::ComPtr<ID3D12Resource> SnapshotArena::Resource()const{
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;resource.Attach(static_cast<ID3D12Resource*>(cvr_sbsn_arena_resource(storage_.Get())));return resource;}
bool SnapshotArena::Finalize(ID3D12GraphicsCommandList* list){return cvr_sbsn_arena_finalize(storage_.Get(),list);}
bool PrepareAtActualCall(ID3D12GraphicsCommandList* list,uint64_t root,const Request& request,const preparation::GraphParentRecord& parent,
    const ReadEpochCertificate& epoch,SnapshotArena& arena,PreparedCopy& output){
    return cvr_sbsn_prepare(list,root,&request,&parent,&epoch,arena.storage_.Get(),&output);}
bool ValidateAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopy& proof){return cvr_sbsn_validate(list,&proof);}
bool RecordAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopy& proof,PrivateRange& output){
    return cvr_sbsn_record(list,&proof,&output);}
bool RecordedView(const PreparedCopy& proof,const PrivateRange& range,D3D12_SHADER_RESOURCE_VIEW_DESC& output){
    return cvr_sbsn_recorded_view(&proof,&range,&output);}
bool ValidateSourceSubmission(const PreparedCopy& proof,ID3D12CommandQueue* queue){return cvr_sbsn_validate_submission(&proof,queue);}
}  // namespace cvr::stereo::structured_buffer_snapshot
#endif
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StructuredBufferSnapshotsEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_StructuredBufferFinalizeDiagnostic[8]{};
}
namespace cvr::stereo::structured_buffer_snapshot {
using Microsoft::WRL::ComPtr;
namespace {
constexpr GUID receiptType{0x9d673805,0xdd81,0x4c14,{0xaa,0x7e,0x6a,0x23,0x54,0x4f,0x15,0x82}};
thread_local indirect_copy::detail::ScopeLedger scopes;
thread_local Failure failure{};
bool Enabled(){return CyberpunkVR_StructuredBufferSnapshotsEnable.load(std::memory_order_relaxed)!=0;}
bool Read(void*,uint64_t address,void* output,size_t bytes){SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;}
struct FullCopyRecord {
    detail::FullVersion version;native_recording::Lease recording;
    ComPtr<ID3D12Resource> source,destination;
};
struct ArenaStorage:IUnknown {
    std::atomic<ULONG> references{1};ComPtr<ID3D12Resource> resource;ComPtr<ID3D12Device> device;
    uint64_t list{},generation{},capacity{},cursor{};Layout layout{};UINT thread{};native_recording::Lease recording;camera_upload::Context context;
    std::atomic<UINT> pending{};std::atomic<bool> finalized{},failed{};
    std::shared_ptr<copy_windows::Window> window;
    std::vector<std::shared_ptr<FullCopyRecord>> fullCopies;
    virtual ~ArenaStorage()=default;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--references;if(!count)delete this;return count;}
};
enum class Stage:uint32_t {Prepared,Recording,Recorded,Failed};
struct Receipt final:IUnknown {
    std::atomic<ULONG> references{1};std::atomic<Stage> stage{Stage::Prepared};ComPtr<ArenaStorage> arena;
    Request request;constants::ResourceCreationInfo creation;preparation::GraphParentRecord parent;
    ReadEpochCertificate epoch;indirect_copy::detail::ScopeStamp scope;detail::Admission observed;
    source_cbv_details::Context nativeContext;
    uint64_t root{},base{},destination{};uint32_t beforeState{},requiredState{};bool mutated{};
    bool reused{};copy_windows::Token reuseToken;std::shared_ptr<FullCopyRecord> fullCopy;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown) && id!=receiptType)return E_NOINTERFACE;*output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--references;if(!count)delete this;return count;}
};
ComPtr<Receipt> Owner(const PreparedCopy& proof){ComPtr<Receipt> result;
    if(proof.owner)proof.owner->QueryInterface(receiptType,reinterpret_cast<void**>(result.GetAddressOf()));return result;
}
bool PublicProof(const PreparedCopy& proof,const Receipt& owner){return owner.arena && proof.list==owner.epoch.list &&
    proof.generation==owner.epoch.generation && proof.occurrence==owner.epoch.occurrence && proof.sourceOffset==owner.request.sourceOffset &&
    proof.privateOffset==owner.destination && proof.copiedBytes==owner.request.bytes && proof.reused==owner.reused &&
    proof.gpuCopiedBytes==(owner.reused?0:owner.request.bytes);
}
bool ReuseValid(const Receipt& owner){
    if(!owner.reused)return true;
    if(!owner.fullCopy || !owner.arena->window || !copy_windows::Valid(owner.reuseToken))return false;
    const auto& original=*owner.fullCopy;const auto& source=owner.request.source;
    return owner.arena->window->List()==owner.epoch.list && owner.arena->window->Generation()==owner.epoch.generation &&
        original.source==source.binding.resource && original.destination==owner.arena->resource &&
        original.recording.token && original.recording.token==source.recording.token &&
        original.recording.list==source.recording.list && original.recording.generation==source.recording.generation &&
        original.recording.context==source.recording.context && original.recording.registry==source.recording.registry &&
        detail::ReuseMatches(original.version,owner.reuseToken->span,owner.epoch.list,owner.epoch.generation,
            reinterpret_cast<uintptr_t>(source.binding.resource.Get()),owner.observed.gpu,owner.epoch.occurrence,
            owner.arena->cursor,owner.arena->capacity);
}
bool SameCreation(const constants::ResourceCreationInfo& a,const constants::ResourceCreationInfo& b){return a.kind==b.kind && a.resource==b.resource &&
    a.heap==b.heap && a.heapOffset==b.heapOffset && a.allocationBytes==b.allocationBytes && a.initialStateKnown==b.initialStateKnown && a.initialState==b.initialState;
}
bool SameShape(const detail::Admission& a,const detail::Admission& b){const auto& x=a.desc;const auto& y=b.desc;
    return a.resource==b.resource && a.gpu==b.gpu && a.heapFlags==b.heapFlags &&
        a.heap.Type==b.heap.Type && a.heap.CPUPageProperty==b.heap.CPUPageProperty && a.heap.MemoryPoolPreference==b.heap.MemoryPoolPreference &&
        a.heap.CreationNodeMask==b.heap.CreationNodeMask && a.heap.VisibleNodeMask==b.heap.VisibleNodeMask &&
        x.Dimension==y.Dimension && x.Alignment==y.Alignment && x.Width==y.Width && x.Height==y.Height && x.DepthOrArraySize==y.DepthOrArraySize &&
        x.MipLevels==y.MipLevels && x.Format==y.Format && x.SampleDesc.Count==y.SampleDesc.Count && x.SampleDesc.Quality==y.SampleDesc.Quality && x.Layout==y.Layout && x.Flags==y.Flags;
}
bool Unprotected(ID3D12Resource* resource){
    ComPtr<ID3D12Resource1> extended;const auto query=resource->QueryInterface(IID_PPV_ARGS(&extended));
    if(query==E_NOINTERFACE)return true;if(FAILED(query) || !extended)return false;
    ComPtr<IUnknown> session;const auto result=extended->GetProtectedResourceSession(IID_PPV_ARGS(&session));
    return detail::NoProtectedSession(result,bool(session));
}
bool Observe(ID3D12GraphicsCommandList* list,const Request& request,const source_srv::SourceSrvBacking& source,
    const constants::ResourceCreationInfo& creation,Layout layout,detail::Admission& output){
    const auto& resource=source.binding.resource;if(!list || !resource)return false;detail::Admission value;
    value.resource=reinterpret_cast<uintptr_t>(resource.Get());value.gpu=resource->GetGPUVirtualAddress();value.desc=resource->GetDesc();
    D3D12_HEAP_FLAGS flags{};if(FAILED(resource->GetHeapProperties(&value.heap,&flags)))return false;value.heapFlags=UINT(flags);
    value.nativeResource=source.native.resource;value.nativeGpu=source.native.gpu;value.nativeBytes=source.native.bytes;
    value.viewOffset=source.sourceOffset;value.viewBytes=source.viewBytes;value.sourceOffset=request.sourceOffset;value.bytes=request.bytes;value.view=source.binding.view;
    value.creationKnown=creation.resource==resource;value.creationKind=creation.kind;value.creationHeap=reinterpret_cast<uintptr_t>(creation.heap.Get());
    value.creationOffset=creation.heapOffset;value.creationBytes=creation.allocationBytes;value.unprotected=Unprotected(resource.Get());
    ComPtr<ID3D12Device> device,recordingDevice;
    value.sameDevice=SUCCEEDED(resource->GetDevice(IID_PPV_ARGS(&device))) && SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&recordingDevice))) &&
        device==recordingDevice && device && device->GetNodeCount()==1;
    value.recordingCurrent=native_recording::ValidateRecording(source.recording,list,source.context);
    value.state=source.state;value.nativeKind=source.native.kind;value.handle=source.native.handle;value.stage=source.stage;value.shaderRegister=source.shaderRegister;
    value.parameter=request.parameter;value.offset=request.offset;value.sourceCurrent=true;value.extent=request.extent;value.layout=layout;output=value;return true;
}
bool Physical(ID3D12GraphicsCommandList* list,const Receipt& owner,detail::Admission& current){
    constants::ResourceCreationInfo creation;if(!constants::QueryResourceCreation(owner.request.source.binding.resource,creation) || !SameCreation(owner.creation,creation))return false;
    return Observe(list,owner.request,owner.request.source,creation,owner.arena->layout,current) && SameShape(owner.observed,current) && current.creationKnown && current.creationKind==1 &&
        !current.creationHeap && !current.creationOffset && !current.creationBytes && current.unprotected && current.sameDevice;
}
bool Scope(ID3D12GraphicsCommandList* list,const Receipt& owner){return list && owner.arena &&
    scopes.Matches(owner.scope,reinterpret_cast<uintptr_t>(list),owner.epoch.generation,owner.epoch.occurrence,GetCurrentThreadId()) &&
    detail::Epoch(owner.epoch,reinterpret_cast<uintptr_t>(list),owner.arena->generation,owner.scope.position) &&
    !owner.arena->failed.load(std::memory_order_acquire) && !owner.arena->finalized.load(std::memory_order_acquire) &&
    owner.base==reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) && preparation::RevalidateGraphParent(owner.parent);
}
bool Source(ID3D12GraphicsCommandList* list,const Receipt& owner,source_srv::SourceSrvBacking& current){
    return source_srv::RevalidateAtSource(list,owner.root,owner.request.parameter,owner.request.offset,owner.request.source,current)==source_srv::Status::Captured;
}
bool Check(ID3D12GraphicsCommandList* list,const Receipt& owner,bool ready){
    failure=Failure::Scope;if(!Scope(list,owner))return false;source_srv::SourceSrvBacking source;detail::Admission physical;
    failure=Failure::Changed;if(!Source(list,owner,source) || !Physical(list,owner,physical))return false;
    failure=Failure::State;if(!detail::ReadMask(source.state,source.native.kind,ready))return false;
    return ready?(source.state.value&owner.requiredState)==owner.requiredState:source.state.value==owner.beforeState;
}
bool RecoveryPreflight(ID3D12GraphicsCommandList* list,const Receipt& owner,source_cbv_details::Context& current){
    const auto& source=owner.request.source;if(!Scope(list,owner) || camera_upload::CurrentContext(owner.base)!=source.context ||
        source.recording.list!=reinterpret_cast<uintptr_t>(list) || source.recording.context!=source.context.context ||
        source.recording.registry!=source.native.registry || !native_recording::ValidateRecording(source.recording,list,source.context) ||
        !descriptors::MatchesDescriptor(source.binding.stamp))return false;
    current=source_cbv_details::detail::ReadContext(Read,nullptr,source.context.context,source.context.list);
    std::array<uint8_t,0xB0> record;geometry::NativeBuffer native;uint64_t address{};detail::Admission physical;
    return source_srv::detail::ReadRecord(Read,nullptr,source.native.registry,source.context.context,source.native.handle,record,address,native) &&
        detail::RecoveryIdentity(source.native,source.rawRecord,owner.nativeContext,source.recordAddress,native,record,current,address) &&
        source.context.state==current.state && Physical(list,owner,physical) &&
        camera_upload::CurrentContext(owner.base)==source.context && native_recording::ValidateRecording(source.recording,list,source.context);
}
bool Recover(ID3D12GraphicsCommandList* list,Receipt& owner){
    source_cbv_details::Context context;if(!RecoveryPreflight(list,owner,context))return false;
    using RegisterRead=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
    const auto flush=reinterpret_cast<Flush>(owner.base+0x3E3BB4);const auto registerRead=reinterpret_cast<RegisterRead>(owner.base+0x337DA0);
    if(context.pending){probe::Scope internal;flush(reinterpret_cast<void*>(context.address));}
    source_srv::SourceSrvBacking source;detail::Admission physical;
    if(!Source(list,owner,source) || !Physical(list,owner,physical))return false;
    uint32_t required{};if(!detail::RecoveryMask(owner.beforeState,source.state,source.native.kind,required))return false;
    if((source.state.value&required)!=required){probe::Scope internal;registerRead(reinterpret_cast<void*>(source.context.context),source.native.handle,required);flush(reinterpret_cast<void*>(source.context.context));}
    return Source(list,owner,source) && Physical(list,owner,physical) && detail::ReadMask(source.state,source.native.kind) && (source.state.value&required)==required;
}
void Failed(Receipt& owner){owner.arena->failed.store(true,std::memory_order_release);--owner.arena->pending;owner.stage.store(Stage::Failed,std::memory_order_release);}
}
struct SnapshotArena::Storage final:ArenaStorage {};
SnapshotArena::SnapshotArena()=default;SnapshotArena::~SnapshotArena()=default;
Failure LastFailure(){return failure;}
bool Compatible(){return source_srv::Compatible() && indirect_copy::Compatible() && native_recording::Compatible();}
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence)noexcept{
    stamp_=scopes.Enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,GetCurrentThreadId());
}
ActualCallScope::~ActualCallScope(){if(stamp_.nonce)scopes.Exit(stamp_);}
bool ActualCallScope::Active()const noexcept{return scopes.Matches(stamp_,stamp_.list,stamp_.generation,stamp_.position,GetCurrentThreadId());}
HRESULT SnapshotArena::Create(ID3D12Device* device,ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t capacity,Layout layout)try{
    if(storage_ || !device || !list || !generation || !capacity || capacity>detail::MaxArenaBytes || capacity%detail::Alignment ||
        device->GetNodeCount()!=1 || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        (layout!=Layout::Dense && layout!=Layout::OriginalIndices) || (layout==Layout::OriginalIndices && capacity<detail::SourceBytes))return E_INVALIDARG;
    ComPtr<ID3D12Device> actual;if(FAILED(list->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device)return E_INVALIDARG;
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->list=reinterpret_cast<uintptr_t>(list);next->generation=generation;next->capacity=capacity;next->thread=GetCurrentThreadId();
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;heap.CreationNodeMask=heap.VisibleNodeMask=1;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=capacity;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    HRESULT result;{probe::Scope internal;result=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&next->resource));}
    if(FAILED(result))return result;const auto gpu=next->resource->GetGPUVirtualAddress();if(!gpu || gpu>UINT64_MAX-capacity)return E_FAIL;
    next->layout=layout;storage_=std::move(next);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
void SnapshotArena::Abort()noexcept{if(storage_){storage_->failed.store(true,std::memory_order_release);if(storage_->window)storage_->window->Close();}}
bool SnapshotArena::Finalized()const{return storage_ && storage_->finalized.load(std::memory_order_acquire) && !storage_->failed.load(std::memory_order_acquire);}
uint64_t SnapshotArena::Bytes()const{return storage_?storage_->cursor:0;}
ComPtr<ID3D12Resource> SnapshotArena::Resource()const{return storage_?storage_->resource:ComPtr<ID3D12Resource>{};}
bool SnapshotArena::Finalize(ID3D12GraphicsCommandList* list){
    auto* diagnostic=CyberpunkVR_StructuredBufferFinalizeDiagnostic;
    diagnostic[0]=reinterpret_cast<uintptr_t>(list);diagnostic[1]=storage_?storage_->list:0;diagnostic[2]=GetCurrentThreadId();diagnostic[3]=storage_?storage_->thread:0;
    diagnostic[4]=storage_?storage_->pending.load():0;diagnostic[5]=storage_?storage_->cursor:0;diagnostic[6]=storage_?storage_->finalized.load():0;diagnostic[7]=storage_?storage_->failed.load():0;
    failure=Failure::Arguments;if(!storage_ || !list || reinterpret_cast<uintptr_t>(list)!=storage_->list ||
        storage_->finalized || storage_->failed || storage_->pending || !storage_->cursor)return false;
    failure=Failure::Retention;if(!gpu::HasCommandResource(list,storage_.Get()))return false;
    failure=Failure::Source;if(!native_recording::ValidateRecording(storage_->recording,list,storage_->context))return false;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    const auto read=storage_->layout==Layout::OriginalIndices?D3D12_RESOURCE_STATE_GENERIC_READ:D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition={storage_->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,read};
    {probe::Scope internal;list->ResourceBarrier(1,&barrier);}storage_->finalized.store(true,std::memory_order_release);
    if(storage_->window)storage_->window->Close();failure=Failure::None;return true;
}
bool PrepareAtActualCall(ID3D12GraphicsCommandList* list,uint64_t root,const Request& request,const preparation::GraphParentRecord& parent,
    const ReadEpochCertificate& epoch,SnapshotArena& arena,PreparedCopy& output)try{
    failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
    failure=Failure::Arguments;if(output.owner || !list || !root || !arena.storage_ || arena.storage_->list!=reinterpret_cast<uintptr_t>(list) ||
        arena.storage_->generation!=epoch.generation || arena.storage_->thread!=GetCurrentThreadId() || arena.storage_->failed || arena.storage_->finalized ||
        request.source.recording.list!=epoch.list || request.source.recording.generation!=epoch.generation)return false;
    failure=Failure::Scope;const auto scope=scopes.Current();if(!scopes.Matches(scope,reinterpret_cast<uintptr_t>(list),epoch.generation,epoch.occurrence,GetCurrentThreadId()) ||
        !detail::Epoch(epoch,reinterpret_cast<uintptr_t>(list),arena.storage_->generation,scope.position))return false;
    failure=Failure::Code;if(!Compatible())return false;
    failure=Failure::Parent;if(!indirect_copy::detail::Parent(parent) || !preparation::RevalidateGraphParent(parent))return false;
    ComPtr<Receipt> owner;owner.Attach(new Receipt);owner->arena=arena.storage_.Get();owner->request=request;owner->parent=parent;owner->epoch=epoch;owner->scope=scope;
    owner->root=root;owner->base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));source_srv::SourceSrvBacking source;
    failure=Failure::Source;if(!Source(list,*owner.Get(),source))return false;
    owner->nativeContext=source_cbv_details::detail::ReadContext(Read,nullptr,source.context.context,source.context.list);
    if(owner->nativeContext.observation!=source_cbv_details::Observation::Known || owner->nativeContext.pending || owner->nativeContext.submitted ||
        owner->nativeContext.state!=source.context.state)return false;
    failure=Failure::Creation;if(!constants::QueryResourceCreation(request.source.binding.resource,owner->creation))return false;
    failure=Failure::Backing;if(!Observe(list,request,source,owner->creation,owner->arena->layout,owner->observed))return false;
    failure=detail::Admit(owner->observed);if(failure!=Failure::None)return false;
    owner->beforeState=source.state.value;owner->requiredState=owner->beforeState|D3D12_RESOURCE_STATE_COPY_SOURCE;
    uint64_t end=owner->arena->cursor,viewEnd{};failure=Failure::Capacity;
    if(owner->arena->layout==Layout::OriginalIndices && (owner->observed.viewOffset || owner->observed.viewBytes!=detail::SourceBytes))return false;
    if(request.extent==Extent::FullOriginalView){
        if(!owner->arena->window){owner->arena->fullCopies.reserve(detail::MaxArenaBytes/detail::SourceBytes);
            owner->arena->window=copy_windows::Create(epoch.list,epoch.generation);if(!owner->arena->window)return false;}
        owner->reuseToken=owner->arena->window->Find(owner->observed.resource,0,detail::SourceBytes,epoch.occurrence);
        if(owner->reuseToken){
            for(const auto& candidate:owner->arena->fullCopies)if(detail::ReuseMatches(candidate->version,owner->reuseToken->span,
                epoch.list,epoch.generation,owner->observed.resource,owner->observed.gpu,epoch.occurrence,
                owner->arena->cursor,owner->arena->capacity)){owner->fullCopy=candidate;break;}
            failure=Failure::Changed;if(!owner->fullCopy)return false;
            owner->reused=true;owner->destination=owner->fullCopy->version.destination;if(!ReuseValid(*owner.Get()))return false;
        }
    }
    if(!owner->reused){failure=Failure::Capacity;
        if(!detail::ReserveExtent(request.extent,owner->arena->layout,owner->arena->cursor,owner->arena->capacity,
            request.sourceOffset,request.bytes,owner->destination,end,viewEnd))return false;
        if(request.extent==Extent::FullOriginalView){
            if(owner->arena->fullCopies.size()>=detail::MaxArenaBytes/detail::SourceBytes)return false;
            owner->fullCopy=std::make_shared<FullCopyRecord>();owner->fullCopy->recording=source.recording;
            owner->fullCopy->source=source.binding.resource;owner->fullCopy->destination=owner->arena->resource;
            owner->fullCopy->version={epoch.list,epoch.generation,owner->observed.resource,owner->observed.gpu,owner->destination,epoch.occurrence,false};
        }
    }
    if(!Check(list,*owner.Get(),false))return false;
    failure=Failure::Changed;if(!ReuseValid(*owner.Get()))return false;
    failure=Failure::Retention;if(!gpu::KeepCommandResources(list,{owner.Get(),owner->arena.Get()}))return false;
    if(owner->fullCopy && !owner->reused)owner->arena->fullCopies.push_back(owner->fullCopy);
    owner->arena->recording=source.recording;owner->arena->context=source.context;owner->arena->cursor=end;++owner->arena->pending;
    output={owner,epoch.list,epoch.generation,epoch.occurrence,request.sourceOffset,owner->destination,request.bytes,
        owner->reused,owner->reused?0:request.bytes};failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool ValidateAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopy& proof)try{
    failure=Failure::Arguments;const auto owner=Owner(proof);if(!owner || !PublicProof(proof,*owner.Get()))return false;
    const auto stage=owner->stage.load(std::memory_order_acquire);if(stage!=Stage::Prepared && stage!=Stage::Recorded)return false;
    failure=Failure::Changed;if(stage==Stage::Prepared && !ReuseValid(*owner.Get()))return false;
    failure=Failure::Retention;if(!gpu::HasCommandResource(list,owner.Get()))return false;
    if(!Check(list,*owner.Get(),stage==Stage::Recorded))return false;failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool RecordAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopy& proof,PrivateRange& output){
    ComPtr<Receipt> owner;bool recording{};try{
        failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
        failure=Failure::Arguments;owner=Owner(proof);if(!owner || !PublicProof(proof,*owner.Get()) || output.resource || output.owner)return false;
        PrivateRange result{owner->arena->resource,owner->arena.Get(),owner->request.sourceOffset,owner->destination,owner->request.bytes};
        if(!ValidateAtActualCall(list,proof))return false;auto expected=Stage::Prepared;failure=Failure::AlreadyRecorded;
        if(!owner->stage.compare_exchange_strong(expected,Stage::Recording))return false;recording=true;
        source_srv::SourceSrvBacking source;failure=Failure::Changed;bool okay=Scope(list,*owner.Get()) && Source(list,*owner.Get(),source) &&
            detail::ReadMask(source.state,source.native.kind) && (source.state.value&owner->beforeState)==owner->beforeState;
        if(okay){owner->requiredState=source.state.value|D3D12_RESOURCE_STATE_COPY_SOURCE;
            if(!(source.state.value&D3D12_RESOURCE_STATE_COPY_SOURCE)){using RegisterRead=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
                failure=Failure::Registration;owner->mutated=true;probe::Scope internal;
                reinterpret_cast<RegisterRead>(owner->base+0x337DA0)(reinterpret_cast<void*>(source.context.context),source.native.handle,owner->requiredState);
                reinterpret_cast<Flush>(owner->base+0x3E3BB4)(reinterpret_cast<void*>(source.context.context));}
            okay=Check(list,*owner.Get(),true);
        }
        if(okay && !ReuseValid(*owner.Get())){failure=Failure::Changed;okay=false;}
        if(!okay){if(owner->mutated)failure=Recover(list,*owner.Get())?Failure::RecoveryVerified:Failure::RecoveryFatal;Failed(*owner.Get());return false;}
        if(!owner->reused){
            {probe::Scope internal;list->CopyBufferRegion(owner->arena->resource.Get(),owner->destination,owner->request.source.binding.resource.Get(),owner->request.sourceOffset,owner->request.bytes);}
            if(owner->fullCopy){owner->fullCopy->version.recorded=true;
                owner->arena->window->Publish({owner->observed.resource,owner->observed.gpu,detail::SourceBytes,0,detail::SourceBytes,
                    owner->destination,owner->epoch.occurrence});}
        }
        output=std::move(result);--owner->arena->pending;owner->stage.store(Stage::Recorded,std::memory_order_release);failure=Failure::None;return true;
    }catch(...){failure=Failure::Exception;if(recording && owner){if(owner->mutated){bool recovered{};try{recovered=Recover(list,*owner.Get());}catch(...){}
                failure=recovered?Failure::RecoveryVerified:Failure::RecoveryFatal;}Failed(*owner.Get());}return false;}
}
bool RecordedView(const PreparedCopy& proof,const PrivateRange& range,D3D12_SHADER_RESOURCE_VIEW_DESC& output)try{
    output={};const auto owner=Owner(proof);
    if(!owner || !PublicProof(proof,*owner.Get()) || owner->stage!=Stage::Recorded || owner->arena->failed ||
        owner->arena->layout!=Layout::OriginalIndices || range.resource!=owner->arena->resource || range.owner.Get()!=owner->arena.Get() ||
        range.sourceOffset!=proof.sourceOffset || range.privateOffset!=proof.privateOffset || range.copiedBytes!=proof.copiedBytes ||
        range.privateOffset<range.sourceOffset || (range.privateOffset-range.sourceOffset)%detail::Stride)return false;
    const auto first=range.privateOffset-range.sourceOffset;
    if(first>owner->arena->capacity || detail::SourceBytes>owner->arena->capacity-first)return false;
    output=owner->observed.view;output.Buffer.FirstElement=first/detail::Stride;return true;
}catch(...){output={};return false;}
bool ValidateSourceSubmission(const PreparedCopy& proof,ID3D12CommandQueue* queue)try{
    failure=Failure::Arguments;const auto owner=Owner(proof);if(!queue || !owner || !PublicProof(proof,*owner.Get()) || owner->stage!=Stage::Recorded ||
        !owner->arena->finalized.load(std::memory_order_acquire) || owner->arena->failed.load(std::memory_order_acquire))return false;
    failure=Failure::Source;if(!native_recording::ValidateSubmission(owner->request.source.recording,queue))return false;
    failure=Failure::Changed;if(!descriptors::MatchesDescriptor(owner->request.source.binding.stamp))return false;
    // Committed physical lifetime is the held resource's implicit heap, not an MA
    // allocation or the now-later native descriptor binding/state-map entry.
    constants::ResourceCreationInfo creation;const auto& resource=owner->request.source.binding.resource;
    failure=Failure::Creation;if(!constants::QueryResourceCreation(resource,creation) || !SameCreation(owner->creation,creation) || !Unprotected(resource.Get()))return false;
    const auto desc=resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(FAILED(resource->GetHeapProperties(&heap,&flags)))return false;auto current=owner->observed;current.desc=desc;current.heap=heap;current.heapFlags=UINT(flags);current.gpu=resource->GetGPUVirtualAddress();
    failure=Failure::Changed;if(!SameShape(owner->observed,current))return false;failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
}
#endif
