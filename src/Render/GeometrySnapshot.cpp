#include "Render/GeometrySnapshot.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <map>
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's freeze is rust/crates/cvr-render/src/geometry_freeze.rs; the Rust test variants use the class

namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
#if CVR_RUST_VRIK
// The draw's admission, the copies' plan, the private buffers, the copy commands, the readback and the pairing:
// rust/crates/cvr-render/src/geometry_snapshot.rs. The COM owner, the rewritten draw's leases, the command-resource
// retention, the internal-commands scope and the fence stay here.
struct RustSnapshotSlot {ID3D12Resource* resource;uint64_t gpu,bytes;uint32_t heapType,status,value,pad;};
static_assert(sizeof(RustSnapshotSlot)==40 && sizeof(native_probe::DrawRecord)==1096,"mirrored by geometry_snapshot.rs");
extern "C" {
HRESULT cvr_gsn_plan(ID3D12GraphicsCommandList*,const native_probe::DrawRecord*,const RustSnapshotSlot*,bool,void**,uint64_t*);
void cvr_gsn_free(void*);
HRESULT cvr_gsn_create(void*,native_probe::DrawRecord*,ID3D12Resource**);
void cvr_gsn_record(void*,ID3D12GraphicsCommandList*);
HRESULT cvr_gsn_read(void*,ID3D12Fence*,uint64_t,const uint8_t**,size_t*);
bool cvr_gsn_pair(const void*,const native_probe::DrawRecord*,const void*,const native_probe::DrawRecord*);
}
struct GeometrySnapshot::Storage final: IUnknown {
    std::atomic<ULONG> refs{1};void* rust{};ComPtr<ID3D12Device> device;ComPtr<ID3D12Fence> fence;UINT64 value{},bytes{};
    ID3D12GraphicsCommandList* recording{};geometry::DrawResources draw;bool ready{};
    ~Storage(){cvr_gsn_free(rust);}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
GeometrySnapshot::GeometrySnapshot()=default;
GeometrySnapshot::~GeometrySnapshot()=default;
namespace {
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* object){ComPtr<ID3D12Device> actual;return object && SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;}
}
HRESULT GeometrySnapshot::Capture(ID3D12GraphicsCommandList* list,const geometry::DrawResources& input,
    const std::array<geometry::ReadState,17>& states,bool rebase) try {
    if(storage_ || !list)return E_INVALIDARG;
    std::array<RustSnapshotSlot,17> slots{};
    for(UINT slot=0;slot<17;++slot){const auto& lease=slot?input.vertices[slot-1]:input.index;
        slots[slot]={lease.resource.Get(),lease.gpu,lease.bytes,lease.heapType,states[slot].status,states[slot].value,0};}
    ComPtr<Storage> pending;pending.Attach(new Storage);
    auto hr=cvr_gsn_plan(list,&input.draw,slots.data(),rebase,&pending->rust,&pending->bytes);if(FAILED(hr))return hr;
    hr=list->GetDevice(IID_PPV_ARGS(&pending->device));if(FAILED(hr))return hr;
    pending->draw=input;pending->draw.instances.clear();pending->recording=list;
    probe::Scope internal;ID3D12Resource* privateCopy{};
    hr=cvr_gsn_create(pending->rust,&pending->draw.draw,&privateCopy);if(FAILED(hr))return hr;
    const geometry::BufferLease owned{privateCopy,privateCopy->GetGPUVirtualAddress(),pending->bytes,D3D12_HEAP_TYPE_DEFAULT,0};pending->draw.index=owned;
    for(UINT slot=0;slot<16;++slot)if(input.draw.inputSlotMask&(1u<<slot))pending->draw.vertices[slot]=owned;
    if(!gpu::KeepCommandResources(list,{pending.Get()}))return E_OUTOFMEMORY;
    cvr_gsn_record(pending->rust,list);storage_=std::move(pending);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
#else
struct GeometrySnapshot::Storage final: IUnknown {
    struct Range {ComPtr<ID3D12Resource> source;UINT64 from{},to{},bytes{};};
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;ComPtr<ID3D12Resource> gpu,readback;
    ComPtr<ID3D12Fence> fence;UINT64 value{},bytes{};ID3D12GraphicsCommandList* recording{};
    geometry::DrawResources draw;std::vector<Range> copies;std::array<UINT64,17> offsets{},sizes{};
    std::vector<uint8_t> cpu;bool ready{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
GeometrySnapshot::GeometrySnapshot()=default;
GeometrySnapshot::~GeometrySnapshot()=default;
namespace {
constexpr UINT ReadStates=D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_INDEX_BUFFER|
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|
    D3D12_RESOURCE_STATE_COPY_SOURCE|D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT;
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* object){ComPtr<ID3D12Device> actual;return object && SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;}
HRESULT Buffer(ID3D12Device* device,UINT64 bytes,D3D12_HEAP_TYPE type,ID3D12Resource** result) {
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;desc.Height=desc.SampleDesc.Count=1;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=type;return device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(result));
}
}
HRESULT GeometrySnapshot::Capture(ID3D12GraphicsCommandList* list,const geometry::DrawResources& input,
    const std::array<geometry::ReadState,17>& states,bool rebase) try {
    const auto& source=input.draw;
    if(storage_ || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !rebase || !source.indices || !source.instances ||
        (source.flags&191)!=191 || (source.flags&64) || !source.frame || source.side>1 || !(source.inputSlotMask&128) || (source.inputSlotMask&~65535u))return E_INVALIDARG;
    const UINT indexBytes=source.index.Format==DXGI_FORMAT_R16_UINT?2:source.index.Format==DXGI_FORMAT_R32_UINT?4:0;
    if(!indexBytes || (UINT64(source.firstIndex)+source.indices)*indexBytes>source.index.SizeInBytes)return E_INVALIDARG;
    const auto stride=source.vertices[7].StrideInBytes;
    if((stride!=48 && stride!=64) || (UINT64(source.firstInstance)+source.instances)*stride>source.vertices[7].SizeInBytes)return E_INVALIDARG;
    ComPtr<Storage> pending;pending.Attach(new Storage);auto hr=list->GetDevice(IID_PPV_ARGS(&pending->device));if(FAILED(hr))return hr;
    pending->draw=input;pending->draw.instances.clear();pending->recording=list;
    std::map<ID3D12Resource*,D3D12_RESOURCE_STATES> before;
    for(UINT slot=0;slot<17;++slot) {
        if(slot && !(source.inputSlotMask&(1u<<(slot-1))))continue;
        const auto& lease=slot?input.vertices[slot-1]:input.index;const auto proof=states[slot];
        const auto address=slot?source.vertices[slot-1].BufferLocation:source.index.BufferLocation;
        const UINT64 skip=slot==8?UINT64(source.firstInstance)*stride:0;
        const UINT64 bytes=slot==8?UINT64(source.instances)*stride:slot?source.vertices[slot-1].SizeInBytes:source.index.SizeInBytes;
        const UINT required=slot?D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER:D3D12_RESOURCE_STATE_INDEX_BUFFER;
        if(!SameDevice(pending->device.Get(),lease.resource.Get()) || (proof.status!=1 && proof.status!=2 && proof.status!=8 && proof.status!=9) ||
            (proof.value&required)!=required || (proof.value&~ReadStates) || !bytes || address<lease.gpu || address-lease.gpu>lease.bytes ||
            skip>lease.bytes-(address-lease.gpu) || bytes>lease.bytes-(address-lease.gpu)-skip)return E_INVALIDARG;
        if(lease.heapType!=D3D12_HEAP_TYPE_DEFAULT && lease.heapType!=D3D12_HEAP_TYPE_UPLOAD)return E_INVALIDARG;
        if(lease.heapType==D3D12_HEAP_TYPE_UPLOAD && proof.value!=D3D12_RESOURCE_STATE_GENERIC_READ)return E_INVALIDARG;
        const auto [it,inserted]=before.emplace(lease.resource.Get(),D3D12_RESOURCE_STATES(proof.value));if(!inserted && UINT(it->second)!=proof.value)return E_INVALIDARG;
        const auto offset=(pending->bytes+15)&~UINT64(15);
        if(offset>8*1024*1024 || bytes>8*1024*1024-offset)return E_OUTOFMEMORY;
        pending->copies.push_back({lease.resource,address-lease.gpu+skip,offset,bytes});pending->offsets[slot]=offset;pending->sizes[slot]=bytes;pending->bytes=offset+bytes;
    }
    probe::Scope internal;
    hr=Buffer(pending->device.Get(),pending->bytes,D3D12_HEAP_TYPE_DEFAULT,&pending->gpu);if(FAILED(hr))return hr;
    hr=Buffer(pending->device.Get(),pending->bytes,D3D12_HEAP_TYPE_READBACK,&pending->readback);if(FAILED(hr))return hr;
    const auto gpu=pending->gpu->GetGPUVirtualAddress();geometry::BufferLease owned{pending->gpu,gpu,pending->bytes,D3D12_HEAP_TYPE_DEFAULT,0};
    pending->draw.index=owned;pending->draw.draw.index.BufferLocation=gpu+pending->offsets[0];
    for(UINT slot=0;slot<16;++slot)if(source.inputSlotMask&(1u<<slot)) {
        pending->draw.vertices[slot]=owned;auto& view=pending->draw.draw.vertices[slot];view.BufferLocation=gpu+pending->offsets[slot+1];view.SizeInBytes=UINT(pending->sizes[slot+1]);
    }
    pending->draw.draw.firstInstance=0;
    if(!gpu::KeepCommandResources(list,{pending.Get()}))return E_OUTOFMEMORY;
    std::vector<D3D12_RESOURCE_BARRIER> barriers;
    for(const auto& [resource,state]:before)if(!(state&D3D12_RESOURCE_STATE_COPY_SOURCE)) {
        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,state,D3D12_RESOURCE_STATE_COPY_SOURCE};barriers.push_back(b);
    }
    if(!barriers.empty())list->ResourceBarrier(UINT(barriers.size()),barriers.data());
    for(const auto& copy:pending->copies)list->CopyBufferRegion(pending->gpu.Get(),copy.to,copy.source.Get(),copy.from,copy.bytes);
    for(auto& b:barriers)std::swap(b.Transition.StateBefore,b.Transition.StateAfter);
    if(!barriers.empty())list->ResourceBarrier(UINT(barriers.size()),barriers.data());
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={pending->gpu.Get(),0,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&b);
    list->CopyResource(pending->readback.Get(),pending->gpu.Get());b.Transition.StateBefore=D3D12_RESOURCE_STATE_COPY_SOURCE;b.Transition.StateAfter=D3D12_RESOURCE_STATE_GENERIC_READ;list->ResourceBarrier(1,&b);
    storage_=std::move(pending);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
#endif
bool GeometrySnapshot::Seal(ID3D12Fence* fence,UINT64 value) {
    if(!storage_ || storage_->fence || !fence || !value || value==UINT64_MAX || !SameDevice(storage_->device.Get(),fence) ||
        gpu::HasCommandResource(storage_->recording,storage_.Get()))return false;
    storage_->fence=fence;storage_->value=value;return true;
}
bool GeometrySnapshot::OnRecording()const{return storage_ && gpu::HasCommandResource(storage_->recording,storage_.Get());}
#if CVR_RUST_VRIK
HRESULT GeometrySnapshot::Read() try {
    if(!storage_)return E_UNEXPECTED;if(storage_->ready)return S_OK;if(!storage_->fence)return S_FALSE;
    const uint8_t* instances{};size_t count{};const auto hr=cvr_gsn_read(storage_->rust,storage_->fence.Get(),storage_->value,&instances,&count);
    if(hr!=S_OK)return hr;storage_->draw.instances.assign(instances,instances+count);storage_->ready=true;return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
#else
HRESULT GeometrySnapshot::Read() try {
    if(!storage_)return E_UNEXPECTED;if(storage_->ready)return S_OK;if(!storage_->fence)return S_FALSE;
    const auto completed=storage_->fence->GetCompletedValue();if(completed==UINT64_MAX)return DXGI_ERROR_DEVICE_REMOVED;
    if(completed<storage_->value)return S_FALSE;
    std::vector<uint8_t> bytes(SIZE_T(storage_->bytes));void* mapped{};const D3D12_RANGE range{0,SIZE_T(storage_->bytes)};
    auto hr=storage_->readback->Map(0,&range,&mapped);if(FAILED(hr))return hr;if(!mapped){storage_->readback->Unmap(0,nullptr);return E_FAIL;}
    std::memcpy(bytes.data(),mapped,bytes.size());const D3D12_RANGE none{};storage_->readback->Unmap(0,&none);
    storage_->draw.instances.assign(bytes.begin()+storage_->offsets[8],bytes.begin()+storage_->offsets[8]+storage_->sizes[8]);
    storage_->cpu=std::move(bytes);storage_->copies.clear();storage_->ready=true;return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
#endif
UINT64 GeometrySnapshot::Bytes()const{return storage_?storage_->bytes:0;}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool GeometrySnapshot::CopyDraw(geometry::DrawResources& result)const{if(!storage_ || !storage_->ready)return false;result=storage_->draw;return true;}
#endif
bool GeometrySnapshot::Pair(const GeometrySnapshot& left,const GeometrySnapshot& right,GeometryCommandPair& output) {
    if(!left.storage_ || !right.storage_ || !left.storage_->ready || !right.storage_->ready || left.storage_->device!=right.storage_->device)return false;
#if CVR_RUST_VRIK
    const auto& a=*left.storage_.Get();const auto& b=*right.storage_.Get();const auto& x=a.draw.draw;
    if(!cvr_gsn_pair(a.rust,&x,b.rust,&b.draw.draw))return false;   // the eyes' shape and every copied byte
#else
    const auto& a=*left.storage_.Get();const auto& b=*right.storage_.Get();const auto& x=a.draw.draw;const auto& y=b.draw.draw;
    if(x.side!=1 || y.side!=0 || x.frame!=y.frame || x.pipeline!=y.pipeline || x.root!=y.root || x.indices!=y.indices || x.instances!=y.instances ||
        x.firstIndex!=y.firstIndex || x.baseVertex!=y.baseVertex || x.inputSlotMask!=y.inputSlotMask || x.index.Format!=y.index.Format || a.sizes!=b.sizes)return false;
    for(UINT slot=0;slot<17;++slot)if(a.sizes[slot]) {
        if(slot && x.vertices[slot-1].StrideInBytes!=y.vertices[slot-1].StrideInBytes)return false;
        if(std::memcmp(a.cpu.data()+a.offsets[slot],b.cpu.data()+b.offsets[slot],SIZE_T(a.sizes[slot])))return false;
    }
#endif
    GeometryCommandPair result;result.instanceRebaseSafe=true;result.eyes={a.draw,b.draw};
    result.eyes[1].index=a.draw.index;result.eyes[1].draw.index=a.draw.draw.index;
    for(UINT slot=0;slot<16;++slot)if(x.inputSlotMask&(1u<<slot)) {
        result.eyes[1].vertices[slot]=a.draw.vertices[slot];result.eyes[1].draw.vertices[slot]=a.draw.draw.vertices[slot];
    }
    output=std::move(result);return true;
}
}
#endif
