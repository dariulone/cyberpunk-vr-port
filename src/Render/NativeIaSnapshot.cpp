#include "Render/NativeIaSnapshot.hpp"
#include "Render/NativeSourceCbvDetails.hpp"
#include "Render/NativeStructuredBufferSnapshot.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/CommandResources.hpp"
#include "Render/NativeCopyWindows.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <algorithm>
#include <new>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The snapshots at the actual call: rust/crates/cvr-render/src/ia_snapshot_receipt.rs (the collection, the arena, the
// receipt, the registration and its recovery, the proofs; CyberpunkVR_NativeIaSnapshotsEnable is defined there). This
// half lends its DrawResources copies, the list's current bindings and the graph parent's revalidation through its
// host table.
namespace cvr::stereo::ia_snapshot {
static_assert(offsetof(geometry::DrawResources,vertices)==80 && offsetof(geometry::DrawResources,draw)==1360,
    "read by ia_snapshot_receipt.rs");
static_assert(sizeof(Observations)==8720 && sizeof(ReadEpochCertificate)==32 && sizeof(PreparedCopies)==64 &&
    sizeof(PrivateIa)==112 && offsetof(PrivateIa,index)==80 && sizeof(native_recording::Lease)==40,
    "mirrored by ia_snapshot_receipt.rs");
extern "C" {
bool cvr_ias_same_lease(const void*,const void*);
void cvr_iasn_attach(const void*);
void cvr_iasn_scope_enter(uint64_t,uint64_t,uint64_t,indirect_copy::detail::ScopeStamp*);
void cvr_iasn_scope_exit(const indirect_copy::detail::ScopeStamp*);
bool cvr_iasn_read_pinned_placed_offset(const detail::PlacedAllocation*,detail::PlacedAllocation*);
bool cvr_iasn_copied_shape(const PreparedCopies*,geometry::DrawResources*);
}
namespace {
struct Host {
    void* (*clone)(const void*);
    void (*free)(void*);
    void (*assign)(void*,const void*);   // the current bindings' check is ia_snapshot_receipt.rs' (geometry_leases.rs)
};
const geometry::DrawResources& Draw(const void* draw){return *static_cast<const geometry::DrawResources*>(draw);}
const Host kHost{
    [](const void* draw)->void*{try{return new geometry::DrawResources(Draw(draw));}catch(...){return nullptr;}},
    [](void* draw){delete static_cast<geometry::DrawResources*>(draw);},
    [](void* output,const void* saved){try{*static_cast<geometry::DrawResources*>(output)=Draw(saved);}catch(...){}},
};
}  // namespace
namespace {
[[maybe_unused]] const bool kAttached=(cvr_iasn_attach(&kHost),true);
}
struct SnapshotArena::Storage:IUnknown {};   // the Rust storage's IUnknown face
SnapshotArena::SnapshotArena()=default;SnapshotArena::~SnapshotArena()=default;
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence)noexcept{
    cvr_iasn_scope_enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,&stamp_);}
ActualCallScope::~ActualCallScope(){cvr_iasn_scope_exit(&stamp_);}
bool ReadPinnedPlacedOffset(const detail::PlacedAllocation& expected,detail::PlacedAllocation& output){
    return cvr_iasn_read_pinned_placed_offset(&expected,&output);}
bool CopiedSourceShape(const PreparedCopies& proof,geometry::DrawResources& output){return cvr_iasn_copied_shape(&proof,&output);}
}  // namespace cvr::stereo::ia_snapshot
#endif
#else

extern "C" {__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NativeIaSnapshotsEnable{};}
namespace cvr::stereo::ia_snapshot {
using Microsoft::WRL::ComPtr;
namespace {
constexpr GUID receiptType{0x9944fe12,0x63b7,0x4b0b,{0x98,0x31,0x70,0x64,0x24,0xc8,0x32,0x1a}};
thread_local indirect_copy::detail::ScopeLedger scopes;
thread_local Failure failure{};
bool Enabled(){return CyberpunkVR_NativeIaSnapshotsEnable.load(std::memory_order_relaxed)!=0;}
bool Read(void*,uint64_t address,void* output,size_t bytes){SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,bytes,&copied) && copied==bytes;}
bool Compatible(){return geometry::NativeIaCompatible() && indirect_copy::Compatible() && native_recording::Compatible();}
bool GeometryPoolCompatible(){static const bool compatible=[] {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    struct Code {uint32_t rva,bytes;uint64_t hash;};
    constexpr Code contract[]{{0x3DF71C,0x6B,0x323015FE897D094Aull},{0x688F14,0x312,0x5A2C9974DB89880Full},
        {0x3445AC,0x5A4,0x5C372BC97D5DEB01ull},{0x3490F4,0x62,0xAF5DD974E8F5ED23ull}};
    std::array<uint8_t,2048> bytes{};for(const auto& code:contract){
        if(!base || base>UINT64_MAX-code.rva-code.bytes || !Read(nullptr,base+code.rva,bytes.data(),code.bytes))return false;
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        if(hash!=code.hash)return false;
    }return true;
}();return compatible;}
bool PlacementCompatible(){static const bool compatible=[] {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    struct Code {uint32_t rva,bytes;uint64_t hash;};
    constexpr Code contract[]{{0x349F5C,0x26,0xD8310396C182F071ull},{0x34AF20,0x23,0xAFAD89E225DAE8EDull}};
    std::array<uint8_t,64> bytes{};for(const auto& code:contract){
        if(!base || base>UINT64_MAX-code.rva-code.bytes || !Read(nullptr,base+code.rva,bytes.data(),code.bytes))return false;
        uint64_t hash=14695981039346656037ull;for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
        if(hash!=code.hash)return false;
    }return true;
}();return compatible;}
uint64_t ImageBytes(){static const uint64_t bytes=[]()->uint64_t {
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));IMAGE_DOS_HEADER dos{};IMAGE_NT_HEADERS64 nt{};
    if(!Read(nullptr,base,&dos,sizeof(dos)) || dos.e_magic!=IMAGE_DOS_SIGNATURE || dos.e_lfanew<=0 || dos.e_lfanew>0x100000 ||
        base>UINT64_MAX-UINT(dos.e_lfanew) || !Read(nullptr,base+UINT(dos.e_lfanew),&nt,sizeof(nt)) || nt.Signature!=IMAGE_NT_SIGNATURE ||
        nt.FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64 || nt.OptionalHeader.Magic!=IMAGE_NT_OPTIONAL_HDR64_MAGIC ||
        nt.OptionalHeader.SizeOfImage<=0x306EC50 || nt.OptionalHeader.SizeOfImage>UINT64_MAX-base)return 0;
    return nt.OptionalHeader.SizeOfImage;
}();return bytes;}
bool CallAllocationOffset(uint64_t base,uint64_t allocation,uint64_t& output)noexcept {
    __try{using Getter=uint64_t(__fastcall*)(const void*);output=reinterpret_cast<Getter>(base+0x349F5C)(reinterpret_cast<const void*>(allocation));return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){output=0;return false;}
}
bool AllocationOffset(void*,uint64_t allocation,uint64_t& output){
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));uint64_t block{},metadata{},table{},method{};MEMORY_BASIC_INFORMATION info{};
    if(!PlacementCompatible() || allocation>UINT64_MAX-0x50 || !Read(nullptr,allocation+0x48,&block,8) || !block || block>UINT64_MAX-0x48 ||
        !Read(nullptr,block+0x40,&metadata,8) || !metadata || !Read(nullptr,metadata,&table,8) || !table || table>UINT64_MAX-0x38 ||
        !Read(nullptr,table+0x30,&method,8) || !method || !VirtualQuery(reinterpret_cast<void*>(method),&info,sizeof(info)) ||
        info.State!=MEM_COMMIT || info.Type!=MEM_IMAGE || reinterpret_cast<uintptr_t>(info.AllocationBase)!=base ||
        !(info.Protect&(PAGE_EXECUTE|PAGE_EXECUTE_READ|PAGE_EXECUTE_READWRITE|PAGE_EXECUTE_WRITECOPY)) || (info.Protect&PAGE_GUARD))return false;
    probe::Scope internal;return CallAllocationOffset(base,allocation,output);
}
bool SameNative(const geometry::NativeBuffer& a,const geometry::NativeBuffer& b){return a.resource==b.resource && a.gpu==b.gpu &&
    a.bytes==b.bytes && a.registry==b.registry && a.context==b.context && a.handle==b.handle && a.kind==b.kind;}
bool SameLease(const geometry::BufferLease& a,const geometry::BufferLease& b){return a.resource==b.resource && a.gpu==b.gpu &&
    a.bytes==b.bytes && a.heapType==b.heapType && a.flags==b.flags && SameNative(a.native,b.native);}
bool SameCreation(const constants::ResourceCreationInfo& a,const constants::ResourceCreationInfo& b){return a.kind==b.kind &&
    a.resource==b.resource && a.heap==b.heap && a.heapOffset==b.heapOffset && a.allocationBytes==b.allocationBytes &&
    a.initialStateKnown==b.initialStateKnown && a.initialState==b.initialState;}
bool SameAllocation(const source_cbv_details::Allocation& a,const source_cbv_details::Allocation& b){using O=source_cbv_details::Observation;
    return a.observation==O::Known && b.observation==O::Known && a.address==b.address && a.vtable==b.vtable && a.pimpl==b.pimpl &&
        a.bytes==b.bytes && a.resource==b.resource && a.heap==b.heap && a.block==b.block && a.handle==b.handle && a.kind==b.kind;}
bool SameAllocator(const source_cbv_details::Allocator& a,const source_cbv_details::Allocator& b){using O=source_cbv_details::Observation;
    return a.observation==O::Known && b.observation==O::Known && a.address==b.address && a.vtable==b.vtable && a.pimpl==b.pimpl && b.pimplMatches;}
bool SameDesc(const D3D12_RESOURCE_DESC& a,const D3D12_RESOURCE_DESC& b){return a.Dimension==b.Dimension && a.Alignment==b.Alignment &&
    a.Width==b.Width && a.Height==b.Height && a.DepthOrArraySize==b.DepthOrArraySize && a.MipLevels==b.MipLevels && a.Format==b.Format &&
    a.SampleDesc.Count==b.SampleDesc.Count && a.SampleDesc.Quality==b.SampleDesc.Quality && a.Layout==b.Layout && a.Flags==b.Flags;}
bool SameHeap(const D3D12_HEAP_PROPERTIES& a,const D3D12_HEAP_PROPERTIES& b){return a.Type==b.Type && a.CPUPageProperty==b.CPUPageProperty &&
    a.MemoryPoolPreference==b.MemoryPoolPreference && a.CreationNodeMask==b.CreationNodeMask && a.VisibleNodeMask==b.VisibleNodeMask;}
bool SameDevice(ID3D12GraphicsCommandList* list,ID3D12DeviceChild* child){ComPtr<ID3D12Device> a,b;return list && child &&
    SUCCEEDED(list->GetDevice(IID_PPV_ARGS(&a))) && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&b))) && a==b && a && a->GetNodeCount()==1;}
bool Unprotected(ID3D12Resource* resource,uint64_t& result,uint64_t& present){ComPtr<ID3D12Resource1> extended;ComPtr<IUnknown> session;
    const auto query=resource->QueryInterface(IID_PPV_ARGS(&extended));result=UINT(query);present=0;
    if(query==E_NOINTERFACE)return true;if(FAILED(query) || !extended)return false;
    const auto hr=extended->GetProtectedResourceSession(IID_PPV_ARGS(&session));result=UINT(hr);present=bool(session);
    return structured_buffer_snapshot::detail::NoProtectedSession(hr,bool(session));}
bool UnprotectedHeap(ID3D12Heap* heap){ComPtr<ID3D12Heap1> extended;ComPtr<IUnknown> session;
    const auto query=heap->QueryInterface(IID_PPV_ARGS(&extended));if(query==E_NOINTERFACE)return true;if(FAILED(query) || !extended)return false;
    const auto hr=extended->GetProtectedResourceSession(IID_PPV_ARGS(&session));return structured_buffer_snapshot::detail::NoProtectedSession(hr,bool(session));}
detail::RecoveryContext Context(const source_cbv_details::Context& value){return {value.address,value.list,value.state,value.submitted,value.kind,
    value.pending,value.observation==source_cbv_details::Observation::Known};}
bool Record(const geometry::NativeBuffer& native,std::array<uint8_t,0xB0>& raw,uint64_t& address){
    raw={};address=0;if(!native.registry || !native.context || !native.handle || native.handle>0x100000)return false;
    const auto offset=0x5C0AF8+uint64_t(native.handle-1)*0xB0;if(native.registry>UINT64_MAX-offset-raw.size())return false;
    address=native.registry+offset;if(source_cbv_details::detail::Bytes(Read,nullptr,address,raw)!=source_cbv_details::Observation::Known)return false;
    return source_cbv_details::detail::Field<uint32_t>(raw,0)==native.bytes && source_cbv_details::detail::Field<uint64_t>(raw,8)==native.gpu &&
        source_cbv_details::detail::Field<uint64_t>(raw,16)==native.resource && uint32_t(raw[6]>>4)==native.kind;
}
bool Residency(const geometry::NativeBuffer& source,uint64_t address){uint64_t resident{},tracker{};uint8_t rejected{};
    if(!address || address>UINT64_MAX-0x70 || !Read(nullptr,address+0x68,&resident,8))return false;
    if(!resident)return true;
    return resident==source.resource && source.context<=UINT64_MAX-0x638 && Read(nullptr,source.context+0x630,&tracker,8) && tracker &&
        tracker<=UINT64_MAX-38 && Read(nullptr,tracker+37,&rejected,1) && !rejected;
}
bool TransitionCapacity(const geometry::NativeBuffer& source){uint64_t data{};uint32_t capacity{},pending{};
    return source.context && source.context<=UINT64_MAX-0x538 && Read(nullptr,source.context+0x528,&data,8) &&
        Read(nullptr,source.context+0x530,&capacity,4) && Read(nullptr,source.context+0x534,&pending,4) &&
        detail::BarrierCapacity(data,capacity,pending);
}
struct Entry {
    Observation observation;geometry::BufferLease source;D3D12_INDEX_BUFFER_VIEW index{};D3D12_VERTEX_BUFFER_VIEW vertex{};
    constants::ResourceCreationInfo creation;D3D12_RESOURCE_DESC desc{};D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS heapFlags{};
    std::array<uint8_t,0xB0> raw{};source_cbv_details::Context context;
    detail::GeometryPool geometryPool;
    detail::PlacedAllocation placement;D3D12_HEAP_DESC placementHeap{};
    source_cbv_details::Allocation allocation;source_cbv_details::Allocator allocator;detail::NativePins pins;
    uint32_t before{},required{},reads{};uint64_t destination{},reserved{};bool mutated{};
};
bool PlacementShape(const Entry& entry,D3D12_HEAP_DESC& heap){
    using O=source_cbv_details::Observation;const auto& allocation=entry.allocation;const auto& creation=entry.creation;
    if(creation.kind!=2 || !creation.heap || creation.resource!=entry.source.resource || !creation.allocationBytes ||
        creation.heapOffset>UINT64_MAX-creation.allocationBytes || allocation.observation!=O::Known || allocation.kind!=1 ||
        !allocation.block || !allocation.handle || allocation.resource!=reinterpret_cast<uintptr_t>(entry.source.resource.Get()) ||
        allocation.heap!=reinterpret_cast<uintptr_t>(creation.heap.Get()) || allocation.bytes!=creation.allocationBytes ||
        allocation.bytes<entry.source.bytes || entry.allocator.observation!=O::Known || !entry.allocator.pimplMatches ||
        !entry.pins.allocation || !entry.pins.allocator || reinterpret_cast<uintptr_t>(entry.pins.allocation.Get())!=allocation.address ||
        reinterpret_cast<uintptr_t>(entry.pins.allocator.Get())!=entry.allocator.address ||
        entry.observation.aliasPayload || entry.observation.aliasControl || entry.observation.poolOwner)return false;
    heap=creation.heap->GetDesc();
    return heap.Properties.Type==D3D12_HEAP_TYPE_DEFAULT && SameHeap(heap.Properties,entry.heap) && heap.Flags==entry.heapFlags &&
        heap.SizeInBytes && creation.heapOffset<=heap.SizeInBytes && creation.allocationBytes<=heap.SizeInBytes-creation.heapOffset &&
        (!heap.Alignment || heap.Alignment==D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT || heap.Alignment==D3D12_DEFAULT_MSAA_RESOURCE_PLACEMENT_ALIGNMENT) &&
        UnprotectedHeap(creation.heap.Get());
}
bool PlacementValue(const Entry& entry,uint64_t base,detail::PlacedAllocation& output){D3D12_HEAP_DESC heap{};
    if(!PlacementCompatible() || !PlacementShape(entry,heap))return false;
    const auto allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,reinterpret_cast<uintptr_t>(entry.pins.allocation.Get()));
    const auto allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,base,entry.source.native.registry,allocation);
    if(!SameAllocation(entry.allocation,allocation) || !SameAllocator(entry.allocator,allocator))return false;
    const detail::PlacedAllocation expected{allocation.address,allocation.resource,allocation.pimpl,allocation.block,allocation.handle,allocation.heap,allocation.bytes};
    return detail::ReadPlacedOffset(Read,AllocationOffset,nullptr,base,ImageBytes(),expected,output) && output.offset==entry.creation.heapOffset;
}
struct ArenaStorage:IUnknown {
    std::atomic<ULONG> references{1};ComPtr<ID3D12Resource> resource;ComPtr<ID3D12Device> device;
    uint64_t list{},generation{},gpu{},capacity{},cursor{};UINT thread{};native_recording::Lease recording;camera_upload::Context context;
    std::atomic<UINT> pending{};std::atomic<bool> finalized{},failed{};
    std::shared_ptr<copy_windows::Window> window;
    virtual ~ArenaStorage()=default;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--references;if(!n)delete this;return n;}
};
struct Receipt final:IUnknown {
    std::atomic<ULONG> references{1};detail::Once stage;ComPtr<ArenaStorage> arena;std::vector<Entry> entries;std::vector<PrivateIa> views;
    geometry::DrawResources draw;bool indexed{};native_recording::Lease recording;preparation::GraphParentRecord parent;
    detail::CopyPlan copies;
    std::vector<detail::CopyRange> transfers;std::vector<copy_windows::Token> reused;
    std::vector<uint64_t> origins;uint64_t transferred{},reusedBytes{};
    ReadEpochCertificate epoch;indirect_copy::detail::ScopeStamp scope;uint64_t base{},copied{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown) && id!=receiptType)return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--references;if(!n)delete this;return n;}
};
ComPtr<Receipt> Owner(const PreparedCopies& proof){ComPtr<Receipt> value;if(proof.owner)proof.owner->QueryInterface(receiptType,
    reinterpret_cast<void**>(value.GetAddressOf()));return value;}
bool PublicProof(const PreparedCopies& proof,const Receipt& owner){return owner.arena && proof.list==owner.epoch.list &&
    proof.generation==owner.epoch.generation && proof.occurrence==owner.epoch.occurrence && proof.count==owner.entries.size() && proof.copiedBytes==owner.copied &&
    proof.transferredBytes==owner.transferred && proof.reusedBytes==owner.reusedBytes;}
bool CurrentIa(ID3D12GraphicsCommandList* list,const geometry::DrawResources& saved,bool indexed){geometry::DrawResources current;
    if(!geometry::CopyCurrentBindings(list,saved.draw,indexed,current))return false;
    if(indexed && !SameLease(saved.index,current.index))return false;
    for(UINT slot=0;slot<16;++slot)if(saved.draw.inputSlotMask&(1u<<slot))if(!SameLease(saved.vertices[slot],current.vertices[slot]))return false;
    return true;
}
bool Epoch(const ReadEpochCertificate& epoch,ID3D12GraphicsCommandList* list,const native_recording::Lease& recording){return epoch.certified &&
    epoch.list==reinterpret_cast<uintptr_t>(list) && epoch.list==recording.list && epoch.generation && epoch.generation==recording.generation && epoch.occurrence;}
bool Scope(ID3D12GraphicsCommandList* list,const Receipt& owner){return list && owner.arena && !owner.arena->failed && !owner.arena->finalized &&
    owner.base==reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) && scopes.Matches(owner.scope,reinterpret_cast<uintptr_t>(list),
        owner.epoch.generation,owner.epoch.occurrence,GetCurrentThreadId()) && Epoch(owner.epoch,list,owner.recording) &&
    preparation::RevalidateGraphParent(owner.parent);}
void Summarize(const std::vector<Entry>& entries,Observations& output){output={};output.count=UINT(entries.size());
    for(UINT i=0;i<output.count;++i){output.rows[i]=entries[i].observation;const auto reason=Failure(output.rows[i].reason);
        if(reason==Failure::None)++output.admitted;else if(output.first==Failure::None)output.first=reason;}}
bool Shape(Entry& entry,ID3D12GraphicsCommandList* list){auto& a=entry.observation;const auto& resource=entry.source.resource;if(!resource)return false;
    entry.desc=resource->GetDesc();entry.heapFlags={};if(FAILED(resource->GetHeapProperties(&entry.heap,&entry.heapFlags)))return false;
    a.resource=reinterpret_cast<uintptr_t>(resource.Get());a.gpu=resource->GetGPUVirtualAddress();a.width=entry.desc.Width;
    a.heap=entry.heap.Type;a.heapFlags=entry.heapFlags;a.resourceFlags=entry.desc.Flags;a.dimension=entry.desc.Dimension;a.alignment=entry.desc.Alignment;
    a.height=entry.desc.Height;a.depth=entry.desc.DepthOrArraySize;a.mips=entry.desc.MipLevels;
    a.heapNodeMasks=(uint64_t(entry.heap.CreationNodeMask)<<32)|entry.heap.VisibleNodeMask;
    a.shapeKnown=entry.desc.Format==DXGI_FORMAT_UNKNOWN && entry.desc.SampleDesc.Count==1 && !entry.desc.SampleDesc.Quality &&
        entry.desc.Layout==D3D12_TEXTURE_LAYOUT_ROW_MAJOR && entry.heap.CPUPageProperty==D3D12_CPU_PAGE_PROPERTY_UNKNOWN &&
        entry.heap.MemoryPoolPreference==D3D12_MEMORY_POOL_UNKNOWN;
    a.unprotected=Unprotected(resource.Get(),a.protectedResult,a.sessionPresent);
    if(SameDevice(list,resource.Get()))a.flags|=detail::SameDevice;
    if(a.viewGpu>=a.gpu)a.comOffset=a.viewGpu-a.gpu;
    return a.gpu==entry.source.gpu && a.width==entry.source.bytes && a.heap==entry.source.heapType && a.resourceFlags==entry.source.flags;
}
void Inspect(Entry& entry,ID3D12GraphicsCommandList* list,uint64_t base,const native_recording::Lease& recording){auto& a=entry.observation;
    const auto& n=entry.source.native;a.nativeResource=n.resource;a.nativeGpu=n.gpu;a.nativeBytes=n.bytes;a.registry=n.registry;
    a.context=n.context;a.handle=n.handle;a.nativeKind=n.kind;
    if(!Shape(entry,list)){a.reason=uint64_t(Failure::Changed);return;}
    if(Record(n,entry.raw,a.recordAddress)){a.flags|=detail::RecordKnown;a.lowType=entry.raw[6]&15;
        a.allocation=source_cbv_details::detail::Field<uint64_t>(entry.raw,0x18);
        a.aliasPayload=source_cbv_details::detail::Field<uint64_t>(entry.raw,0x20);a.aliasControl=source_cbv_details::detail::Field<uint64_t>(entry.raw,0x28);
        source_cbv_details::Record record;record.observation=source_cbv_details::Observation::Known;record.address=a.recordAddress;record.bytes=entry.raw;
        const auto pool=source_cbv_details::detail::ReadPool(Read,nullptr,base,n,record,a.gpu,a.width);
        a.poolObservation=UINT(pool.observation);a.poolOwner=pool.owner;a.poolSlot=pool.slot;a.poolUnit=pool.unit;
        if(n.kind==1 && a.lowType==5 && !a.allocation && !a.aliasPayload && !a.aliasControl && GeometryPoolCompatible() &&
            detail::ReadGeometryPool(Read,nullptr,base,n,entry.raw,a.gpu,a.width,a.viewGpu,a.viewBytes,entry.geometryPool)){
            a.flags|=detail::GeometryPoolKnown;a.poolObservation=UINT(source_cbv_details::Observation::Known);
            a.poolOwner=entry.geometryPool.owner;a.poolSlot=entry.geometryPool.slot;a.poolUnit=entry.geometryPool.unit;a.reserved=entry.geometryPool.packed;
        }
        if(!a.allocation && a.poolObservation==UINT(source_cbv_details::Observation::Known))a.allocation=a.poolOwner;
        if(!a.allocation && (a.aliasPayload || a.aliasControl)){const auto alias=source_cbv_details::detail::ReadAlias(Read,nullptr,a.aliasPayload,a.aliasControl);
            if(alias.observation==source_cbv_details::Observation::Known)a.allocation=alias.allocation;}
    }
    entry.context=source_cbv_details::detail::ReadContext(Read,nullptr,n.context,reinterpret_cast<uintptr_t>(list));
    const camera_upload::Context context{entry.context.address,entry.context.state,entry.context.list};
    if(entry.context.observation==source_cbv_details::Observation::Known && !entry.context.pending && !entry.context.submitted &&
        recording.context==n.context && recording.registry==n.registry && native_recording::ValidateRecording(recording,list,context))a.flags|=detail::ContextKnown;
    if(constants::QueryResourceCreation(entry.source.resource,entry.creation)){a.flags|=detail::CreationKnown;a.creationKind=entry.creation.kind;
        a.creationHeap=reinterpret_cast<uintptr_t>(entry.creation.heap.Get());a.creationOffset=entry.creation.heapOffset;a.creationBytes=entry.creation.allocationBytes;
        a.initialKnown=entry.creation.initialStateKnown;a.initialState=entry.creation.initialState;}
    entry.allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,a.allocation);
    entry.allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,base,n.registry,entry.allocation);
    a.allocationObservation=UINT(entry.allocation.observation);a.allocatorObservation=UINT(entry.allocator.observation);
    a.allocationKind=entry.allocation.kind;a.allocationResource=entry.allocation.resource;a.allocationHeap=entry.allocation.heap;
    a.allocationBytes=entry.allocation.bytes;a.allocationPimpl=entry.allocation.pimpl;a.allocator=entry.allocator.address;a.allocatorPimpl=entry.allocator.pimpl;
    if(entry.allocation.observation==source_cbv_details::Observation::Known)a.flags|=detail::AllocationKnown;
    if(entry.allocator.observation==source_cbv_details::Observation::Known && entry.allocator.pimplMatches)a.flags|=detail::AllocatorKnown;
    if(n.gpu==a.gpu && n.bytes==a.width)a.flags|=detail::FullBacking;
    if((a.flags&detail::RequiredFlags)==detail::RequiredFlags && (n.kind==0 || n.kind==1 || n.kind==3) &&
        entry.creation.kind==2 && entry.creation.heap && entry.allocation.observation==source_cbv_details::Observation::Known &&
        entry.allocation.kind==1 && entry.allocator.observation==source_cbv_details::Observation::Known && entry.allocator.pimplMatches &&
        !a.aliasPayload && !a.aliasControl && !a.poolOwner && (a.flags&detail::FullBacking) && a.heap==D3D12_HEAP_TYPE_DEFAULT && PlacementCompatible()){
        entry.pins.allocator=reinterpret_cast<IUnknown*>(entry.allocator.address);entry.pins.allocation=reinterpret_cast<IUnknown*>(entry.allocation.address);
        if(PlacementValue(entry,base,entry.placement) && PlacementShape(entry,entry.placementHeap)){
            a.flags|=detail::PlacedKnown;a.reserved=entry.placement.offset;
        }
    }
    const auto state=geometry::ReadStateAtDraw(list,entry.source);a.stateValue=state.value;a.stateStatus=state.status;a.stateKind=state.kind;
    entry.before=state.value;entry.reads=detail::ReadBit(Role(a.role));entry.required=entry.before|D3D12_RESOURCE_STATE_COPY_SOURCE;
    a.reason=uint64_t(detail::Admit(a));
    if(!a.reason && !Residency(n,a.recordAddress))a.reason=uint64_t(Failure::ResidencyDeferred);
    if(!a.reason && n.kind!=1 && !(state.value&D3D12_RESOURCE_STATE_COPY_SOURCE) && !TransitionCapacity(n))a.reason=uint64_t(Failure::Capacity);
}
bool Collect(ID3D12GraphicsCommandList* list,const geometry::DrawResources& draw,bool indexed,const native_recording::Lease& recording,
    const preparation::GraphParentRecord& parent,const ReadEpochCertificate& epoch,std::vector<Entry>& entries,Observations& output){
    output={};entries.clear();failure=Failure::Arguments;
    uint32_t copiedMask{};
    if(!list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || draw.draw.list!=reinterpret_cast<uintptr_t>(list) ||
        !(draw.draw.flags&128) || !detail::Selection(draw.draw.inputSlotMask,epoch.separateVertices,copiedMask))return false;
    const auto add=[&](Role role,UINT slot,const geometry::BufferLease& source){Entry entry;entry.source=source;auto& a=entry.observation;
        a.role=uint64_t(role);a.slot=slot;a.list=reinterpret_cast<uintptr_t>(list);a.generation=epoch.generation;a.occurrence=epoch.occurrence;
        if(role==Role::Index){entry.index=draw.draw.index;a.viewGpu=entry.index.BufferLocation;a.viewBytes=entry.index.SizeInBytes;a.strideOrFormat=entry.index.Format;}
        else{entry.vertex=draw.draw.vertices[slot];a.viewGpu=entry.vertex.BufferLocation;a.viewBytes=entry.vertex.SizeInBytes;a.strideOrFormat=entry.vertex.StrideInBytes;}
        entries.push_back(std::move(entry));};
    if(indexed)add(Role::Index,UINT_MAX,draw.index);
    for(UINT slot=0;slot<16;++slot)if(copiedMask&(1u<<slot))add(Role::Vertex,slot,draw.vertices[slot]);
    const auto stamp=scopes.Current();const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    uint64_t flags{};if(Epoch(epoch,list,recording) && scopes.Matches(stamp,reinterpret_cast<uintptr_t>(list),epoch.generation,epoch.occurrence,GetCurrentThreadId()))flags|=detail::ScopeCurrent;
    if(Compatible())flags|=detail::CodeKnown;
    if(indirect_copy::detail::Parent(parent) && preparation::RevalidateGraphParent(parent))flags|=detail::ParentCurrent;
    if(CurrentIa(list,draw,indexed))flags|=detail::BindingsCurrent;
    for(auto& entry:entries){entry.observation.flags=flags;Inspect(entry,list,base,recording);
        const auto& a=entry.observation;if(a.reason==0){D3D12_INDEX_BUFFER_VIEW ib;D3D12_VERTEX_BUFFER_VIEW vb;
            const bool valid=Role(a.role)==Role::Index?detail::IndexView(a.viewGpu,0,a.viewBytes,entry.index,ib):detail::VertexView(a.viewGpu,0,a.viewBytes,entry.vertex,vb);
            if(!valid)entry.observation.reason=uint64_t(Failure::Arguments);}}
    // Native tracked maps are per handle, while a buffer transition is whole-COM.
    // A second distinct handle for that COM backing has no uniform-state proof.
    for(size_t i=0;i<entries.size();++i)for(size_t j=0;j<i;++j)if(entries[i].source.resource==entries[j].source.resource &&
        detail::RequiresUniformStateProof(entries[i].source.native.kind,entries[j].source.native.kind) && !SameNative(entries[i].source.native,entries[j].source.native)){
        entries[i].observation.reason=entries[j].observation.reason=uint64_t(Failure::BackingDeferred);}
    Summarize(entries,output);failure=output.first;return true;
}
bool Physical(const Entry& entry,uint64_t base){constants::ResourceCreationInfo creation;if(!constants::QueryResourceCreation(entry.source.resource,creation) ||
    !SameCreation(entry.creation,creation) || !entry.pins.allocation || !entry.pins.allocator)return false;
    const auto allocation=source_cbv_details::detail::ReadAllocation(Read,nullptr,base,reinterpret_cast<uintptr_t>(entry.pins.allocation.Get()));
    const auto allocator=source_cbv_details::detail::ReadAllocator(Read,nullptr,base,entry.source.native.registry,allocation);
    if(!SameAllocation(entry.allocation,allocation) || !SameAllocator(entry.allocator,allocator) ||
        reinterpret_cast<uintptr_t>(entry.pins.allocator.Get())!=allocator.address ||
        allocation.kind!=((entry.observation.flags&detail::PlacedKnown)?1u:0u) ||
        (!(entry.observation.flags&detail::PlacedKnown) && allocation.heap) ||
        allocation.resource!=reinterpret_cast<uintptr_t>(entry.source.resource.Get()) || allocation.bytes<entry.source.bytes)return false;
    if(entry.observation.flags&detail::PlacedKnown){D3D12_HEAP_DESC heap{};detail::PlacedAllocation placement;
        if(!PlacementShape(entry,heap) || heap.SizeInBytes!=entry.placementHeap.SizeInBytes || heap.Alignment!=entry.placementHeap.Alignment ||
            !SameHeap(heap.Properties,entry.placementHeap.Properties) || heap.Flags!=entry.placementHeap.Flags ||
            !PlacementValue(entry,base,placement) || placement!=entry.placement)return false;
    }
    const auto desc=entry.source.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};uint64_t result{},session{};
    return SameDesc(entry.desc,desc) && entry.source.resource->GetGPUVirtualAddress()==entry.source.gpu &&
        SUCCEEDED(entry.source.resource->GetHeapProperties(&heap,&flags)) && SameHeap(entry.heap,heap) && flags==entry.heapFlags &&
        Unprotected(entry.source.resource.Get(),result,session);
}
bool PoolCurrent(const Entry& entry,uint64_t base){
    if(!(entry.observation.flags&detail::GeometryPoolKnown))return true;
    detail::GeometryPool current;
    return GeometryPoolCompatible() && entry.pins.allocation && reinterpret_cast<uintptr_t>(entry.pins.allocation.Get())==entry.geometryPool.owner &&
        detail::ReadGeometryPool(Read,nullptr,base,entry.source.native,entry.raw,entry.source.gpu,entry.source.bytes,
            entry.observation.viewGpu,entry.observation.viewBytes,current) && current==entry.geometryPool;
}
bool Identity(ID3D12GraphicsCommandList* list,const Receipt& owner,const Entry& entry,bool pending,source_cbv_details::Context& context){
    if(!Scope(list,owner) || !CurrentIa(list,owner.draw,owner.indexed) || !Physical(entry,owner.base) || !PoolCurrent(entry,owner.base))return false;
    std::array<uint8_t,0xB0> raw;uint64_t address{};if(!Record(entry.source.native,raw,address) || raw!=entry.raw || address!=entry.observation.recordAddress)return false;
    context=source_cbv_details::detail::ReadContext(Read,nullptr,entry.source.native.context,reinterpret_cast<uintptr_t>(list));
    if(context.observation!=source_cbv_details::Observation::Known || context.address!=entry.context.address || context.list!=entry.context.list ||
        context.state!=entry.context.state || context.kind!=entry.context.kind || context.submitted || (!pending && context.pending))return false;
    const camera_upload::Context expected{context.address,context.state,context.list};
    return native_recording::ValidateRecording(owner.recording,list,expected) && Residency(entry.source.native,address);
}
bool Check(ID3D12GraphicsCommandList* list,const Receipt& owner,bool ready){failure=Failure::Scope;if(!Scope(list,owner))return false;
    for(const auto& entry:owner.entries){source_cbv_details::Context context;failure=Failure::Changed;if(!Identity(list,owner,entry,false,context))return false;
        const auto state=geometry::ReadStateAtDraw(list,entry.source);failure=Failure::State;
        if(entry.source.native.kind==1){if(state.status!=8 || state.kind!=1 || state.value!=D3D12_RESOURCE_STATE_GENERIC_READ)return false;}
        else if(!detail::ReadMask(state,entry.source.native.kind,entry.reads,ready) ||
            (ready?(state.value&entry.required)!=entry.required:state.value!=entry.before))return false;
    }return true;
}
bool Recover(ID3D12GraphicsCommandList* list,Receipt& owner){using Register=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
    const auto flush=reinterpret_cast<Flush>(owner.base+0x3E3BB4);const auto use=reinterpret_cast<Register>(owner.base+0x337DA0);
    for(auto& entry:owner.entries)if(entry.mutated){source_cbv_details::Context context;if(!Identity(list,owner,entry,true,context))return false;
        std::array<uint8_t,0xB0> raw;uint64_t address{};
        if(!Record(entry.source.native,raw,address) || !detail::RecoveryIdentity(entry.source.native,entry.raw,entry.observation.recordAddress,
            Context(entry.context),entry.source.native,raw,address,Context(context)))return false;
        if(context.pending){probe::Scope internal;flush(reinterpret_cast<void*>(context.address));}
        if(!Identity(list,owner,entry,false,context))return false;const auto state=geometry::ReadStateAtDraw(list,entry.source);uint32_t required{};
        if(!detail::RecoveryMask(entry.before,state,entry.source.native.kind,entry.reads,required))return false;
        if((state.value&required)!=required){probe::Scope internal;use(reinterpret_cast<void*>(context.address),entry.source.native.handle,required);flush(reinterpret_cast<void*>(context.address));}
        if(!Identity(list,owner,entry,false,context) || !detail::ReadMask(geometry::ReadStateAtDraw(list,entry.source),entry.source.native.kind,entry.reads))return false;
        if((geometry::ReadStateAtDraw(list,entry.source).value&required)!=required)return false;
    }return true;
}
void Failed(Receipt& owner){owner.arena->failed.store(true,std::memory_order_release);--owner.arena->pending;owner.stage.Failed();}
}
struct SnapshotArena::Storage final:ArenaStorage {};
bool ReadPinnedPlacedOffset(const detail::PlacedAllocation& expected,detail::PlacedAllocation& output){
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    return PlacementCompatible() && detail::ReadPlacedOffset(Read,AllocationOffset,nullptr,base,ImageBytes(),expected,output);
}
bool ReadFixedPool(const geometry::NativeBuffer& native,const std::array<uint8_t,0xB0>& raw,uint64_t gpu,uint64_t bytes,
    uint64_t viewGpu,uint64_t viewBytes,uint8_t lowType,detail::GeometryPool& output){
    return GeometryPoolCompatible() && detail::ReadGeometryPool(Read,nullptr,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)),
        native,raw,gpu,bytes,viewGpu,viewBytes,output,lowType);
}
SnapshotArena::SnapshotArena()=default;SnapshotArena::~SnapshotArena()=default;
Failure LastFailure(){return failure;}
bool CopiedSourceShape(const PreparedCopies& proof,geometry::DrawResources& output){
    const auto owner=Owner(proof);
    if(!owner || !PublicProof(proof,*owner.Get()) || owner->stage.Current()!=detail::Stage::Recorded ||
        !owner->arena->finalized || owner->arena->failed)return false;
    output=owner->draw;return true;
}
ActualCallScope::ActualCallScope(ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t occurrence)noexcept{
    if(Enabled() && !probe::internalCommands)stamp_=scopes.Enter(reinterpret_cast<uintptr_t>(list),generation,occurrence,GetCurrentThreadId());}
ActualCallScope::~ActualCallScope(){if(stamp_.nonce)scopes.Exit(stamp_);}
bool ActualCallScope::Active()const noexcept{return scopes.Matches(stamp_,stamp_.list,stamp_.generation,stamp_.position,GetCurrentThreadId());}
bool ObserveAtActualCall(ID3D12GraphicsCommandList* list,const geometry::DrawResources& draw,bool indexed,const native_recording::Lease& recording,
    const preparation::GraphParentRecord& parent,const ReadEpochCertificate& epoch,Observations& output)try{
    output={};failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
    std::vector<Entry> entries;entries.reserve(Observations::MaxViews);return Collect(list,draw,indexed,recording,parent,epoch,entries,output);
}catch(...){failure=Failure::Exception;return false;}
HRESULT SnapshotArena::Create(ID3D12Device* device,ID3D12GraphicsCommandList* list,uint64_t generation,uint64_t capacity)try{
    failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return E_ACCESSDENIED;
    if(storage_ || !device || !list || device->GetNodeCount()!=1 || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !generation ||
        !capacity || capacity>detail::MaxArenaBytes || capacity%detail::Alignment)return E_INVALIDARG;
    ComPtr<ID3D12Device> actual;if(FAILED(list->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device)return E_INVALIDARG;
    ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->list=reinterpret_cast<uintptr_t>(list);next->generation=generation;
    next->capacity=capacity;next->thread=GetCurrentThreadId();D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;heap.CreationNodeMask=heap.VisibleNodeMask=1;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=capacity;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;
    desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;HRESULT hr;{probe::Scope internal;hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&next->resource));}if(FAILED(hr))return hr;
    next->gpu=next->resource->GetGPUVirtualAddress();if(!next->gpu || next->gpu>UINT64_MAX-capacity)return E_FAIL;
    next->window=copy_windows::Create(next->list,next->generation);
    storage_=std::move(next);failure=Failure::None;return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
void SnapshotArena::Abort()noexcept{if(storage_){storage_->failed.store(true,std::memory_order_release);if(storage_->window)storage_->window->Close();}}
bool SnapshotArena::Finalized()const{return storage_ && storage_->finalized.load(std::memory_order_acquire) && !storage_->failed.load(std::memory_order_acquire);}
uint64_t SnapshotArena::Bytes()const{return storage_?storage_->cursor:0;}
bool SnapshotArena::Finalize(ID3D12GraphicsCommandList* list){failure=Failure::Arguments;
    if(!storage_ || !list || reinterpret_cast<uintptr_t>(list)!=storage_->list ||
        storage_->pending || !storage_->cursor || storage_->finalized || storage_->failed)return false;
    failure=Failure::Retention;if(!gpu::HasCommandResource(list,storage_.Get()))return false;
    failure=Failure::Recording;if(!native_recording::ValidateRecording(storage_->recording,list,storage_->context))return false;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={storage_->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_GENERIC_READ};
    {probe::Scope internal;list->ResourceBarrier(1,&barrier);}storage_->finalized.store(true,std::memory_order_release);
    if(storage_->window)storage_->window->Close();failure=Failure::None;return true;
}
bool PrepareAtActualCall(ID3D12GraphicsCommandList* list,const geometry::DrawResources& draw,bool indexed,const native_recording::Lease& recording,
    const preparation::GraphParentRecord& parent,const ReadEpochCertificate& epoch,SnapshotArena& arena,PreparedCopies& output,Observations& observed)try{
    observed={};failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
    std::vector<Entry> entries;entries.reserve(Observations::MaxViews);
    if(!Collect(list,draw,indexed,recording,parent,epoch,entries,observed))return false;
    if(observed.first!=Failure::None){failure=observed.first;return false;}
    failure=Failure::Empty;if(entries.empty())return false;
    failure=Failure::Arguments;if(output.owner || !arena.storage_ || arena.storage_->list!=reinterpret_cast<uintptr_t>(list) ||
        arena.storage_->generation!=epoch.generation || arena.storage_->thread!=GetCurrentThreadId() || arena.storage_->failed || arena.storage_->finalized)return false;
    ComPtr<Receipt> owner;owner.Attach(new Receipt);owner->arena=arena.storage_.Get();owner->draw=draw;owner->indexed=indexed;owner->recording=recording;
    owner->parent=parent;owner->epoch=epoch;owner->scope=scopes.Current();owner->base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));owner->entries=std::move(entries);
    owner->views.reserve(owner->entries.size());uint64_t cursor=owner->arena->cursor;
    std::vector<detail::CopyRange> ranges;for(const auto& entry:owner->entries)
        ranges.push_back({reinterpret_cast<uintptr_t>(entry.source.resource.Get()),entry.observation.comOffset,entry.observation.viewBytes});
    failure=Failure::Capacity;if(!detail::PlanCopies(ranges,0,owner->arena->capacity,owner->copies)){
        owner->entries.front().observation.reason=uint64_t(failure);Summarize(owner->entries,observed);return false;}
    owner->copied=owner->copies.copied;
    for(auto& range:owner->copies.ranges){copy_windows::Token cached;
        const bool eligible=std::all_of(owner->entries.begin(),owner->entries.end(),[&](const auto& entry){return
            reinterpret_cast<uintptr_t>(entry.source.resource.Get())!=range.resource || entry.source.flags==0;});
        if(eligible && owner->arena->window)cached=owner->arena->window->Find(range.resource,range.sourceOffset,range.bytes,epoch.occurrence);
        if(cached){range.destination=cached->span.destination+range.sourceOffset-cached->span.offset;
            owner->origins.push_back(cached->span.occurrence);owner->reused.push_back(std::move(cached));owner->reusedBytes+=range.bytes;
        }else{const auto prefix=range.sourceOffset%detail::Alignment;uint64_t start{},end{};
            if(range.bytes>UINT64_MAX-prefix || !detail::Reserve(cursor,owner->arena->capacity,range.bytes+prefix,start,end)){
                owner->entries.front().observation.reason=uint64_t(failure);Summarize(owner->entries,observed);return false;}
            range.destination=start+prefix;cursor=end;owner->transfers.push_back(range);owner->origins.push_back(epoch.occurrence);owner->transferred+=range.bytes;
        }
    }
    owner->copies.next=cursor;
    for(size_t i=0;i<ranges.size();++i){const auto& range=ranges[i];
        const auto found=std::find_if(owner->copies.ranges.begin(),owner->copies.ranges.end(),[&](const auto& copy){return copy.resource==range.resource &&
            range.sourceOffset>=copy.sourceOffset && range.sourceOffset-copy.sourceOffset<=copy.bytes && range.bytes<=copy.bytes-(range.sourceOffset-copy.sourceOffset);});
        if(found==owner->copies.ranges.end())return false;
        owner->copies.destinations[i]=found->destination+range.sourceOffset-found->sourceOffset;
    }
    size_t viewIndex{};
    for(auto& entry:owner->entries){entry.destination=owner->copies.destinations[viewIndex++];entry.reserved=entry.observation.viewBytes;
        entry.observation.privateOffset=entry.destination;entry.observation.reservedBytes=entry.reserved;
        failure=Failure::AllocationDeferred;entry.pins.allocator=reinterpret_cast<IUnknown*>(entry.allocator.address);
        entry.pins.allocation=reinterpret_cast<IUnknown*>(entry.allocation.address);
        if(!Physical(entry,owner->base) || !PoolCurrent(entry,owner->base)){failure=Failure::Changed;entry.observation.reason=uint64_t(failure);Summarize(owner->entries,observed);return false;}
        entry.observation.flags|=detail::Pinned;
        PrivateIa view;view.role=Role(entry.observation.role);view.slot=UINT(entry.observation.slot);view.resource=owner->arena->resource;view.owner=owner->arena.Get();
        view.sourceOffset=entry.observation.comOffset;view.privateOffset=entry.destination;view.copiedBytes=entry.observation.viewBytes;view.reservedBytes=entry.reserved;
        for(size_t i=0;i<owner->copies.ranges.size();++i){const auto& range=owner->copies.ranges[i];
            if(range.resource==reinterpret_cast<uintptr_t>(entry.source.resource.Get()) && view.sourceOffset>=range.sourceOffset &&
                view.sourceOffset-range.sourceOffset<=range.bytes && view.copiedBytes<=range.bytes-(view.sourceOffset-range.sourceOffset)){
                view.originOccurrence=owner->origins[i];view.reused=view.originOccurrence!=epoch.occurrence;break;}}
        if(!view.originOccurrence)return false;
        const bool valid=view.role==Role::Index?detail::IndexView(owner->arena->gpu,entry.destination,owner->arena->capacity,entry.index,view.index):
            detail::VertexView(owner->arena->gpu,entry.destination,owner->arena->capacity,entry.vertex,view.vertex);
        if(!valid){failure=Failure::Arguments;return false;}owner->views.push_back(std::move(view));
    }
    for(auto& entry:owner->entries)for(const auto& other:owner->entries)if(SameNative(entry.source.native,other.source.native))entry.reads|=other.reads;
    if(!Check(list,*owner.Get(),false))return false;
    failure=Failure::Retention;if(!gpu::KeepCommandResources(list,{owner.Get(),owner->arena.Get()}))return false;
    owner->arena->recording=recording;const auto& context=owner->entries.front().context;
    owner->arena->context={context.address,context.state,context.list};owner->arena->cursor=cursor;++owner->arena->pending;
    output={owner,epoch.list,epoch.generation,epoch.occurrence,owner->copied,UINT(owner->entries.size()),owner->transferred,owner->reusedBytes};Summarize(owner->entries,observed);failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool ValidateAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopies& proof)try{failure=Failure::Arguments;
    const auto owner=Owner(proof);if(!owner || !PublicProof(proof,*owner.Get()))return false;
    const auto stage=owner->stage.Current();if(stage!=detail::Stage::Prepared && stage!=detail::Stage::Recorded)return false;
    if(stage==detail::Stage::Prepared && std::any_of(owner->reused.begin(),owner->reused.end(),[](const auto& token){return !copy_windows::Valid(token);})){failure=Failure::Changed;return false;}
    failure=Failure::Retention;if(!gpu::HasCommandResource(list,owner.Get()))return false;
    if(!Check(list,*owner.Get(),stage==detail::Stage::Recorded))return false;failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
bool RecordAtActualCall(ID3D12GraphicsCommandList* list,const PreparedCopies& proof,std::vector<PrivateIa>& output){ComPtr<Receipt> owner;bool recording{};
    try{failure=Failure::Disabled;if(!Enabled() || probe::internalCommands)return false;
        failure=Failure::Arguments;owner=Owner(proof);if(!owner || !PublicProof(proof,*owner.Get()) || !output.empty())return false;
        std::vector<PrivateIa> result=owner->views;if(!ValidateAtActualCall(list,proof))return false;
        failure=Failure::AlreadyRecorded;if(!owner->stage.Begin())return false;recording=true;
        bool ok=true;using Register=void(*)(void*,uint32_t,uint32_t);using Flush=void(*)(void*);
        for(size_t i=0;i<owner->entries.size();++i){auto& entry=owner->entries[i];bool duplicate{};
            for(size_t j=0;j<i;++j)duplicate|=SameNative(entry.source.native,owner->entries[j].source.native);if(duplicate)continue;
            source_cbv_details::Context context;failure=Failure::Changed;if(!Identity(list,*owner.Get(),entry,false,context)){ok=false;break;}
            const auto state=geometry::ReadStateAtDraw(list,entry.source);
            if(entry.source.native.kind==1?(state.status!=8 || state.kind!=1 || state.value!=D3D12_RESOURCE_STATE_GENERIC_READ):
                !detail::ReadMask(state,entry.source.native.kind,entry.reads)){failure=Failure::State;ok=false;break;}
            entry.required=state.value|D3D12_RESOURCE_STATE_COPY_SOURCE;
            if(entry.source.native.kind!=1 && (state.value&D3D12_RESOURCE_STATE_COPY_SOURCE))continue;
            if(entry.source.native.kind!=1 && !TransitionCapacity(entry.source.native)){failure=Failure::Capacity;ok=false;break;}
            failure=Failure::Registration;if(entry.source.native.kind!=1)entry.mutated=true;
            {probe::Scope internal;reinterpret_cast<Register>(owner->base+0x337DA0)(reinterpret_cast<void*>(context.address),entry.source.native.handle,entry.required);
                if(entry.source.native.kind!=1)reinterpret_cast<Flush>(owner->base+0x3E3BB4)(reinterpret_cast<void*>(context.address));}
        }
        if(ok)ok=Check(list,*owner.Get(),true);
        if(ok && std::any_of(owner->reused.begin(),owner->reused.end(),[](const auto& token){return !copy_windows::Valid(token);})){failure=Failure::Changed;ok=false;}
        if(!ok){if(std::any_of(owner->entries.begin(),owner->entries.end(),[](const auto& entry){return entry.mutated;}))
                failure=Recover(list,*owner.Get())?Failure::RecoveryVerified:Failure::RecoveryFatal;
            Failed(*owner.Get());return false;}
        {probe::Scope internal;for(const auto& copy:owner->transfers)list->CopyBufferRegion(owner->arena->resource.Get(),copy.destination,
            reinterpret_cast<ID3D12Resource*>(copy.resource),copy.sourceOffset,copy.bytes);}
        if(owner->arena->window)for(const auto& range:owner->transfers){const auto entry=std::find_if(owner->entries.begin(),owner->entries.end(),[&](const auto& item){return
                reinterpret_cast<uintptr_t>(item.source.resource.Get())==range.resource;});
            if(entry!=owner->entries.end() && !entry->source.flags)owner->arena->window->Publish({range.resource,entry->source.gpu,entry->source.bytes,range.sourceOffset,range.bytes,range.destination,owner->epoch.occurrence});}
        output.swap(result);--owner->arena->pending;owner->stage.Recorded();failure=Failure::None;return true;
    }catch(...){failure=Failure::Exception;if(recording && owner){if(std::any_of(owner->entries.begin(),owner->entries.end(),[](const auto& entry){return entry.mutated;})){
                bool recovered{};try{recovered=Recover(list,*owner.Get());}catch(...){}failure=recovered?Failure::RecoveryVerified:Failure::RecoveryFatal;}
            Failed(*owner.Get());}return false;}
}
bool ValidateSourceSubmission(const PreparedCopies& proof,ID3D12CommandQueue* queue)try{failure=Failure::Arguments;
    const auto owner=Owner(proof);if(!queue || !owner || !PublicProof(proof,*owner.Get()) || owner->stage.Current()!=detail::Stage::Recorded ||
        !owner->arena->finalized.load(std::memory_order_acquire) || owner->arena->failed.load(std::memory_order_acquire))return false;
    failure=Failure::Submission;if(!native_recording::ValidateSubmission(owner->recording,queue))return false;
    failure=Failure::Changed;for(const auto& entry:owner->entries)if(!Physical(entry,owner->base))return false;
    failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
}
#endif
