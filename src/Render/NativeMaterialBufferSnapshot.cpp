#include "Render/NativeMaterialBufferSnapshot.hpp"
#include "Render/NativeStructuredBufferSnapshot.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The material buffer snapshots: rust/crates/cvr-render/src/material_snapshot_receipt.rs (the arena, the receipt with
// its pins, the physical identity at the call and at submission, the private view after the fence). This half
// lends the graph parent's revalidation.
namespace cvr::stereo::material_buffer_snapshot {
static_assert(sizeof(Epoch)==32 && sizeof(Copy)==80 && offsetof(Copy,sourceOffset)==56 && sizeof(source_srv::SourceSrvBacking)==1112,
    "mirrored by material_snapshot_receipt.rs");
extern "C" {
void cvr_mbsn_scope_enter(uint64_t,uint64_t,uint64_t,indirect_copy::detail::ScopeStamp*);
void cvr_mbsn_scope_exit(const indirect_copy::detail::ScopeStamp*);
bool cvr_mbsn_matches(const Copy*,const descriptors::SourceResourceBinding*,bool,const native_recording::Lease*,ID3D12CommandQueue*);
}
struct Arena::Storage:IUnknown {};   // the Rust storage's IUnknown face
Arena::Arena()=default;Arena::~Arena()=default;
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence){
    cvr_mbsn_scope_enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,&stamp_);}
ActualCallScope::~ActualCallScope(){cvr_mbsn_scope_exit(&stamp_);}
bool MatchesPrivate(const Copy& copy,const descriptors::SourceResourceBinding& binding){
    return cvr_mbsn_matches(&copy,&binding,false,nullptr,nullptr);}
bool MatchesStagedPrivate(const Copy& copy,const descriptors::SourceResourceBinding& binding,
    const native_recording::Lease& recording,ID3D12CommandQueue* queue){
    return cvr_mbsn_matches(&copy,&binding,true,&recording,queue);}
}  // namespace cvr::stereo::material_buffer_snapshot
#endif
#else

namespace cvr::stereo::material_buffer_snapshot {
using Microsoft::WRL::ComPtr;
namespace {
constexpr GUID receiptType{0x94ab56f3,0x848e,0x4c71,{0xa2,0x6f,0x92,0xd0,0x7a,0xa1,0x80,0x57}};
thread_local indirect_copy::detail::ScopeLedger scopes;
thread_local Failure failure{};
bool Read(void*,uint64_t address,void* output,size_t bytes){SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,bytes,&copied) && copied==bytes;}
uint64_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
struct StorageBase:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;ComPtr<ID3D12Resource> resource;
    native_recording::Lease recording;camera_upload::Context context;uint64_t bytes{};bool finalized{},failed{};
    virtual ~StorageBase()=default;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*output=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
struct Receipt final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<StorageBase> arena;
    source_srv::SourceSrvBacking source;constants::ResourceCreationInfo creation;
    source_cbv_details::Allocation allocation;source_cbv_details::Allocator allocator;ia_snapshot::detail::NativePins pins;
    ia_snapshot::detail::GeometryPool pool;ia_snapshot::detail::PlacedAllocation placement;
    preparation::GraphParentRecord parent;Epoch epoch;uint64_t root{},offset{},bytes{};bool recorded{},validated{},complete{};
    ComPtr<ID3D12CommandQueue> queue;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown) && id!=receiptType)return E_NOINTERFACE;*output=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
bool SameAllocation(const source_cbv_details::Allocation& a,const source_cbv_details::Allocation& b){
    using O=source_cbv_details::Observation;return a.observation==O::Known && b.observation==O::Known && a.address==b.address && a.vtable==b.vtable &&
        a.pimpl==b.pimpl && a.resource==b.resource && a.heap==b.heap && a.bytes==b.bytes && a.block==b.block && a.handle==b.handle && a.kind==b.kind;
}
bool Unprotected(ID3D12Resource* resource){ComPtr<ID3D12Resource1> extended;const auto hr=resource->QueryInterface(IID_PPV_ARGS(&extended));
    if(hr==E_NOINTERFACE)return true;if(FAILED(hr))return false;ComPtr<IUnknown> session;const auto result=extended->GetProtectedResourceSession(IID_PPV_ARGS(&session));
    return structured_buffer_snapshot::detail::NoProtectedSession(result,bool(session));}
bool Physical(const Receipt& owner){
    const auto& source=owner.source;const auto& saved=owner.creation;constants::ResourceCreationInfo creation;
    if(!owner.pins.allocation || !owner.pins.allocator || !constants::QueryResourceCreation(source.binding.resource,creation) ||
        creation.resource!=saved.resource || creation.kind!=saved.kind || creation.heap!=saved.heap || creation.heapOffset!=saved.heapOffset ||
        creation.allocationBytes!=saved.allocationBytes || !creation.initialStateKnown || creation.initialState!=saved.initialState ||
        (creation.initialState!=D3D12_RESOURCE_STATE_COMMON && creation.initialState!=D3D12_RESOURCE_STATE_GENERIC_READ))return false;
    const auto allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,Base(),reinterpret_cast<uintptr_t>(owner.pins.allocation.Get()));
    const auto allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,Base(),source.native.registry,allocation);
    using O=source_cbv_details::Observation;
    if(!SameAllocation(owner.allocation,allocation) || allocator.observation!=O::Known || !allocator.pimplMatches || allocator.address!=owner.allocator.address ||
        allocator.vtable!=owner.allocator.vtable || allocator.pimpl!=owner.allocator.pimpl || allocator.address!=reinterpret_cast<uintptr_t>(owner.pins.allocator.Get()) ||
        allocation.resource!=reinterpret_cast<uintptr_t>(source.binding.resource.Get()) || allocation.bytes<source.backingBytes)return false;
    if(owner.pool.owner){ia_snapshot::detail::GeometryPool current;
        if(!ia_snapshot::ReadFixedPool(source.native,source.rawRecord,source.gpu,source.backingBytes,source.gpu+source.sourceOffset,source.viewBytes,13,current) ||
            current!=owner.pool || allocation.address!=current.owner || allocation.kind || allocation.heap || creation.kind!=1 || creation.heap || creation.heapOffset || creation.allocationBytes)return false;
    }else if(creation.kind==2){ia_snapshot::detail::PlacedAllocation current;
        const ia_snapshot::detail::PlacedAllocation expected{allocation.address,allocation.resource,allocation.pimpl,allocation.block,allocation.handle,allocation.heap,allocation.bytes};
        if(allocation.kind!=1 || !creation.heap || allocation.heap!=reinterpret_cast<uintptr_t>(creation.heap.Get()) || allocation.bytes!=creation.allocationBytes ||
            !ia_snapshot::ReadPinnedPlacedOffset(expected,current) || current.offset!=creation.heapOffset || current!=owner.placement)return false;
        const auto heap=creation.heap->GetDesc();if(heap.Properties.Type!=D3D12_HEAP_TYPE_DEFAULT || creation.heapOffset>heap.SizeInBytes || creation.allocationBytes>heap.SizeInBytes-creation.heapOffset)return false;
    }else if(creation.kind!=1 || allocation.kind || allocation.heap || creation.heap || creation.heapOffset || creation.allocationBytes)return false;
    const auto desc=source.binding.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width==source.backingBytes && !desc.Flags && desc.Height==1 && desc.DepthOrArraySize==1 &&
        desc.MipLevels==1 && desc.Format==DXGI_FORMAT_UNKNOWN && desc.Layout==D3D12_TEXTURE_LAYOUT_ROW_MAJOR && desc.SampleDesc.Count==1 && !desc.SampleDesc.Quality &&
        (!desc.Alignment || desc.Alignment==D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT) && source.binding.resource->GetGPUVirtualAddress()==source.gpu &&
        SUCCEEDED(source.binding.resource->GetHeapProperties(&heap,&flags)) && heap.Type==D3D12_HEAP_TYPE_DEFAULT &&
        heap.CPUPageProperty==D3D12_CPU_PAGE_PROPERTY_UNKNOWN && heap.MemoryPoolPreference==D3D12_MEMORY_POOL_UNKNOWN && heap.CreationNodeMask<=1 && heap.VisibleNodeMask<=1 &&
        !(UINT(flags)&~UINT(D3D12_HEAP_FLAG_DENY_RT_DS_TEXTURES|D3D12_HEAP_FLAG_DENY_NON_RT_DS_TEXTURES)) && Unprotected(source.binding.resource.Get());
}
bool Current(ID3D12GraphicsCommandList* list,const Receipt& owner){
    const auto& source=owner.source;source_srv::SourceSrvBacking current;
    if(!preparation::RevalidateGraphParent(owner.parent) || source_srv::RevalidateAtSource(list,owner.root,source_srv::RootParameter,source.shaderRegister,source,current)!=source_srv::Status::Captured ||
        !Physical(owner))return false;
    const geometry::BufferLease buffer{source.binding.resource,source.gpu,source.backingBytes,source.heapType,source.flags,source.native};
    const auto state=geometry::ReadStateAtDraw(list,buffer);
    return state.kind==1 && state.status==8 && state.value==D3D12_RESOURCE_STATE_GENERIC_READ;
}
ComPtr<Receipt> Owner(const Copy& copy){ComPtr<Receipt> owner;if(copy.owner)copy.owner->QueryInterface(receiptType,reinterpret_cast<void**>(owner.GetAddressOf()));
    if(!owner || !owner->recorded || !owner->arena || copy.resource!=owner->arena->resource || copy.offset!=owner->offset || copy.bytes!=owner->bytes || copy.sourceOffset!=owner->source.sourceOffset)return {};
    D3D12_SHADER_RESOURCE_VIEW_DESC expected{};
    if(!detail::PrivateView(owner->source.binding.view,owner->source.shaderRegister,owner->offset,owner->bytes,expected) || !detail::SameBufferView(expected,copy.view))return {};
    return owner;
}
}
struct Arena::Storage final:StorageBase {};
Arena::Arena()=default;Arena::~Arena()=default;
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence){if(!probe::internalCommands)
    stamp_=scopes.Enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,GetCurrentThreadId());}
ActualCallScope::~ActualCallScope(){if(stamp_.nonce)scopes.Exit(stamp_);}
bool ActualCallScope::Active()const{return scopes.Matches(stamp_,stamp_.list,stamp_.generation,stamp_.position,GetCurrentThreadId());}
HRESULT Arena::Create(ID3D12Device* device,ID3D12GraphicsCommandList* list,uint64_t generation)try{
    if(storage_ || !device || !list || !generation || device->GetNodeCount()!=1 || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return E_INVALIDARG;
    ComPtr<ID3D12Device> actual;if(FAILED(list->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device)return E_INVALIDARG;
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->context=camera_upload::CurrentContext(Base());
    if(!native_recording::Capture(list,next->context,next->recording) || next->recording.generation!=generation)return E_ACCESSDENIED;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=Capacity;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;
    desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    HRESULT hr;{probe::Scope internal;hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&next->resource));}
    if(FAILED(hr))return hr;storage_=std::move(next);return S_OK;
}catch(...){return E_FAIL;}
void Arena::Abort(){if(storage_)storage_->failed=true;}
uint64_t Arena::Bytes()const{return storage_?storage_->bytes:0;}
bool Arena::Finalize(ID3D12GraphicsCommandList* list){
    if(!storage_ || storage_->failed || storage_->finalized || !storage_->bytes || !native_recording::ValidateRecording(storage_->recording,list,storage_->context) ||
        !gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={storage_->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_GENERIC_READ};
    {probe::Scope internal;list->ResourceBarrier(1,&barrier);}storage_->finalized=true;return true;
}
bool Capture(ID3D12GraphicsCommandList* list,uint64_t root,const source_srv::SourceSrvBacking& source,const preparation::GraphParentRecord& parent,
    const Epoch& epoch,Arena& arena,Copy& output)try{
    failure=Failure::Disabled;if(!CyberpunkVR_StructuredBufferSnapshotsEnable.load() || probe::internalCommands)return false;
    failure=Failure::Arguments;if(output.owner || !arena.storage_ || arena.storage_->failed || arena.storage_->finalized || !source.binding.resource ||
        source.native.kind!=1 || source.flags || source.heapType!=D3D12_HEAP_TYPE_DEFAULT || (source.shaderRegister!=4 && source.shaderRegister!=6) ||
        !source.viewBytes || source.viewBytes>98304 || !source.gpu)return false;
    const auto scope=scopes.Current();failure=Failure::Scope;
    if(!epoch.certified || !epoch.occurrence || !scopes.Matches(scope,epoch.list,epoch.generation,epoch.occurrence,GetCurrentThreadId()) ||
        epoch.list!=reinterpret_cast<uintptr_t>(list) || !detail::SameRecording(arena.storage_->recording,source.recording) || source.recording.generation!=epoch.generation ||
        !indirect_copy::detail::Parent(parent) || !preparation::RevalidateGraphParent(parent))return false;
    ComPtr<Receipt> owner;owner.Attach(new Receipt);owner->arena=arena.storage_.Get();owner->source=source;owner->parent=parent;owner->epoch=epoch;owner->root=root;owner->bytes=source.viewBytes;
    failure=Failure::Creation;if(!constants::QueryResourceCreation(source.binding.resource,owner->creation))return false;
    uint64_t allocation=source.allocation.address;
    if(source.lowType==13 && !allocation){failure=Failure::Pool;
        if(!ia_snapshot::ReadFixedPool(source.native,source.rawRecord,source.gpu,source.backingBytes,source.gpu+source.sourceOffset,source.viewBytes,13,owner->pool))return false;
        allocation=owner->pool.owner;
    }else if(source.native.gpu!=source.gpu || source.native.bytes!=source.backingBytes)return false;
    owner->allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,Base(),allocation);
    owner->allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,Base(),source.native.registry,owner->allocation);
    using O=source_cbv_details::Observation;failure=Failure::Allocation;
    if(owner->allocation.observation!=O::Known || owner->allocation.resource!=reinterpret_cast<uintptr_t>(source.binding.resource.Get()) ||
        owner->allocation.bytes<source.backingBytes || owner->allocator.observation!=O::Known || !owner->allocator.pimplMatches)return false;
    owner->pins.allocator=reinterpret_cast<IUnknown*>(owner->allocator.address);owner->pins.allocation=reinterpret_cast<IUnknown*>(owner->allocation.address);
    if(owner->creation.kind==2){const auto& a=owner->allocation;const ia_snapshot::detail::PlacedAllocation wanted{a.address,a.resource,a.pimpl,a.block,a.handle,a.heap,a.bytes};
        if(!ia_snapshot::ReadPinnedPlacedOffset(wanted,owner->placement))return false;}
    failure=Failure::Changed;if(!Current(list,*owner.Get()))return false;
    const auto start=(owner->arena->bytes+Alignment-1)&~(Alignment-1);const auto padded=(owner->bytes+Alignment-1)&~(Alignment-1);
    failure=Failure::Capacity;if(start>Capacity || padded>Capacity-start)return false;owner->offset=start;
    Copy copy;copy.owner=owner;copy.resource=owner->arena->resource;copy.sourceOffset=source.sourceOffset;copy.offset=start;copy.bytes=source.viewBytes;
    if(!detail::PrivateView(source.binding.view,source.shaderRegister,start,copy.bytes,copy.view))return false;
    failure=Failure::Retention;if(!gpu::KeepCommandResources(list,{owner.Get(),source.binding.resource.Get(),owner->arena.Get()}))return false;
    failure=Failure::Changed;if(!Current(list,*owner.Get()))return false;
    {probe::Scope internal;list->CopyBufferRegion(copy.resource.Get(),start,source.binding.resource.Get(),source.sourceOffset,copy.bytes);}
    owner->recorded=true;owner->arena->bytes=start+padded;output=std::move(copy);failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool ValidateSubmission(const Copy& copy,ID3D12CommandQueue* queue){
    const auto owner=Owner(copy);if(!owner || !queue || !owner->arena->finalized || owner->arena->failed || !Physical(*owner.Get()) ||
        !native_recording::ValidateSubmission(owner->source.recording,queue))return false;
    std::array<uint8_t,0xB0> raw{};geometry::NativeBuffer native;uint64_t address{};
    if(!source_srv::detail::ReadRecord(Read,nullptr,owner->source.native.registry,owner->source.context.context,owner->source.native.handle,raw,address,native) ||
        raw!=owner->source.rawRecord || address!=owner->source.recordAddress)return false;
    owner->queue=queue;owner->validated=true;return true;
}
bool Ready(const Copy& copy,const native_recording::Lease& recording,ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t signal,bool submitted){
    const auto owner=Owner(copy);if(!owner || !owner->validated || owner->arena->failed || !owner->arena->finalized || !submitted || !queue || owner->queue.Get()!=queue ||
        !detail::SameRecording(owner->source.recording,recording) || !fence || !signal)return false;
    const auto completed=fence->GetCompletedValue();owner->complete=completed!=UINT64_MAX && completed>=signal;return owner->complete;
}
bool MatchesPrivate(const Copy& copy,const descriptors::SourceResourceBinding& binding){
    const auto owner=Owner(copy);return owner && owner->complete && owner->arena->finalized && !owner->arena->failed &&
        detail::SourceBinding(owner->source,binding);
}
bool MatchesStagedPrivate(const Copy& copy,const descriptors::SourceResourceBinding& binding,
    const native_recording::Lease& recording,ID3D12CommandQueue* queue){
    const auto owner=Owner(copy);return owner && owner->validated && owner->arena->finalized && !owner->arena->failed &&
        queue && owner->queue.Get()==queue && detail::SameRecording(owner->source.recording,recording) &&
        detail::SameRecording(owner->arena->recording,recording) && detail::SourceBinding(owner->source,binding);
}
Failure LastFailure(){return failure;}
}
#endif
