#include "Render/StereoGeometryCommands.hpp"
#include "Render/CommandResources.hpp"
#include "Render/NativeIndirectPolicy.hpp"
#include "Render/RenderProbeScope.hpp"
#include <algorithm>
#include <map>
#include <tuple>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The commands: rust/crates/cvr-render/src/geometry_commands.rs (tools/single_pass_tests/geometry_commands_test runs its
// cases against them in its Rust variant).
extern "C" {
HRESULT cvr_sgc_create(ID3D12Device*,const void*,size_t,void**);
HRESULT cvr_sgc_create_sources(ID3D12Device*,const void*,size_t,void(*)(void*,void*),void**);
bool cvr_sgc_record(void*,ID3D12GraphicsCommandList*,UINT,bool,bool(*)(void*,void*),void(*)(void*,void*));
void cvr_sgc_counts(void*,UINT*,UINT*,UINT64*);
HRESULT cvr_sgc_capture_indirect(ID3D12GraphicsCommandList*,const void*,const void*,const void*,void(*)(void*,void*),
    bool(*)(void*,void*,UINT,UINT64,UINT64),void*,bool(*)(void*,void*),void*);
}
#endif
#endif
namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
#if CVR_RUST_VRIK
// The storages are Rust objects (their IUnknown vtables are Rust's); the inputs are lent as mirrors, the command
// resources' Keep, the signature receipts and the copy validation are callbacks.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's commands are geometry_commands.rs' (the Rust test variants use these)
namespace {
struct RustLease {ID3D12Resource* resource;UINT64 gpu,bytes;};
struct RustResources {RustLease index;std::array<RustLease,16> vertices;const native_probe::DrawRecord* draw;const uint8_t* instances;size_t instanceCount;};
struct RustIndirect {ID3D12CommandSignature* signature;UINT maxCommandCount;ID3D12Resource* arguments;UINT64 argumentOffset;
    ID3D12Resource* countBuffer;UINT64 countOffset;};
struct RustPair {std::array<RustResources,2> eyes;bool instanceRebaseSafe;};
struct RustSource {RustResources geometry;const RustIndirect* indirect;};
struct RustProof {ID3D12Resource* resource;bool stateKnown;D3D12_RESOURCE_STATES state;UINT64 recordingGeneration,commandPosition;IUnknown* allocationOwner;};
struct RustCaptured {ID3D12Resource* arguments;ID3D12Resource* countBuffer;IndirectSnapshotReceipt receipt;IUnknown* owner;};
static_assert(sizeof(RustLease)==24 && sizeof(RustResources)==432 && sizeof(RustIndirect)==48 && sizeof(RustPair)==872 &&
    sizeof(RustSource)==440 && sizeof(RustProof)==40 && sizeof(IndirectSnapshotReceipt)==64 && sizeof(RustCaptured)==88 &&
    sizeof(native_probe::DrawRecord)==1096,"mirrored by geometry_commands.rs");
RustLease Lend(const geometry::BufferLease& lease){return {lease.resource.Get(),lease.gpu,lease.bytes};}
RustResources Lend(const geometry::DrawResources& value){
    RustResources lent{Lend(value.index),{},&value.draw,value.instances.data(),value.instances.size()};
    for(UINT slot=0;slot<16;++slot)lent.vertices[slot]=Lend(value.vertices[slot]);return lent;
}
RustIndirect Lend(const PureIndirectSource& value){
    return {value.signature.Get(),value.maxCommandCount,value.arguments.Get(),value.argumentOffset,value.countBuffer.Get(),value.countOffset};
}
RustProof Lend(const IndirectCopyProof& value){
    return {value.resource.Get(),value.state.has_value(),value.state.value_or(D3D12_RESOURCE_STATES{}),value.recordingGeneration,
        value.commandPosition,value.allocationOwner.Get()};
}
bool Keep(void* list,void* object)noexcept{
    try{return gpu::KeepCommandResources(static_cast<ID3D12GraphicsCommandList*>(list),{static_cast<IUnknown*>(object)});}catch(...){return false;}
}
void Describe(void* signature,void* receipt)noexcept{
    *static_cast<indirect_policy::detail::Receipt*>(receipt)=indirect_policy::Describe(static_cast<ID3D12CommandSignature*>(signature));
}
struct Validation {ValidateIndirectCopy validate;std::array<const IndirectCopyProof*,2> proofs;};
bool Validate(void* context,void* list,UINT which,UINT64 offset,UINT64 bytes)noexcept{
    const auto& value=*static_cast<const Validation*>(context);
    try{return which<2 && value.proofs[which] && value.validate(static_cast<ID3D12GraphicsCommandList*>(list),*value.proofs[which],offset,bytes);}
    catch(...){return false;}
}
struct Counts {UINT draws{},sources{};UINT64 bytes{};};
Counts CountsOf(IUnknown* storage){Counts counts;if(storage)cvr_sgc_counts(storage,&counts.draws,&counts.sources,&counts.bytes);return counts;}
}
struct StereoGeometryCommands::Storage:IUnknown {};
StereoGeometryCommands::StereoGeometryCommands()=default;
StereoGeometryCommands::~StereoGeometryCommands()=default;
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
HRESULT StereoGeometryCommands::Create(ID3D12Device* device,std::span<const GeometryCommandPair> inputs)try{
    if(storage_ || !device || inputs.empty() || inputs.size()>2048)return E_INVALIDARG;
    std::vector<RustPair> lent;lent.reserve(inputs.size());
    for(const auto& input:inputs)lent.push_back({{Lend(input.eyes[0]),Lend(input.eyes[1])},input.instanceRebaseSafe});
    void* created{};const auto hr=cvr_sgc_create(device,lent.data(),lent.size(),&created);if(FAILED(hr))return hr;
    storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool StereoGeometryCommands::RecordGeometry(ID3D12GraphicsCommandList* list,UINT index)const{
    return storage_ && list && cvr_sgc_record(storage_.Get(),list,index,false,&Keep,&Describe);
}
UINT StereoGeometryCommands::DrawCount()const{return CountsOf(storage_.Get()).draws;}
UINT64 StereoGeometryCommands::InstanceBytes()const{return CountsOf(storage_.Get()).bytes;}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
HRESULT StereoGeometryCommands::CreateSources(ID3D12Device* device,std::span<const geometry::DrawResources> inputs){
    return CreateSourcesImpl(device,inputs,{});
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's source commands are retained_geometry_native.rs' (CompileSources)
HRESULT StereoGeometryCommands::CreateMixedSources(ID3D12Device* device,std::span<const GeometrySourceCommand> inputs){
    return CreateSourcesImpl(device,{},inputs);
}
HRESULT StereoGeometryCommands::CreateSourcesImpl(ID3D12Device* device,std::span<const geometry::DrawResources> direct,
    std::span<const GeometrySourceCommand> mixed)try{
    const auto count=direct.empty()?mixed.size():direct.size();
    if(storage_ || !device || !count || count>MaxSourceCommands || (!direct.empty() && !mixed.empty()))return E_INVALIDARG;
    std::vector<RustIndirect> indirects;indirects.reserve(count);std::vector<RustSource> lent;lent.reserve(count);
    for(size_t i=0;i<count;++i){
        const auto& input=direct.empty()?mixed[i].geometry:direct[i];const RustIndirect* indirect{};
        if(direct.empty() && mixed[i].indirect){indirects.push_back(Lend(*mixed[i].indirect));indirect=&indirects.back();}
        lent.push_back({Lend(input),indirect});
    }
    void* created{};const auto hr=cvr_sgc_create_sources(device,lent.data(),lent.size(),&Describe,&created);if(FAILED(hr))return hr;
    storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool StereoGeometryCommands::RecordSourceGeometry(ID3D12GraphicsCommandList* list,UINT index)const{
    return storage_ && list && cvr_sgc_record(storage_.Get(),list,index,true,&Keep,&Describe);
}
#endif
UINT StereoGeometryCommands::SourceCount()const{return CountsOf(storage_.Get()).sources;}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
HRESULT StereoGeometryCommands::CaptureIndirectSource(ID3D12GraphicsCommandList* list,const PureIndirectSource& source,
    const IndirectCopyProof& arguments,const IndirectCopyProof* count,ValidateIndirectCopy validate,CapturedIndirectSource& output)try{
    if(!list || !validate || output.owner || output.replay.signature || output.replay.arguments || output.replay.countBuffer)return E_INVALIDARG;
    const auto lentSource=Lend(source);const auto lentArguments=Lend(arguments);RustProof lentCount{};if(count)lentCount=Lend(*count);
    const Validation context{validate,{&arguments,count}};RustCaptured captured{};CapturedIndirectSource result;
    const auto hr=cvr_sgc_capture_indirect(list,&lentSource,&lentArguments,count?&lentCount:nullptr,&Describe,&Validate,
        const_cast<Validation*>(&context),&Keep,&captured);
    if(FAILED(hr))return hr;
    result.replay.arguments.Attach(captured.arguments);result.replay.countBuffer.Attach(captured.countBuffer);result.owner.Attach(captured.owner);
    result.replay.signature=source.signature;result.replay.maxCommandCount=source.maxCommandCount;result.receipt=captured.receipt;
    output=std::move(result);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
#endif
#else
struct StereoGeometryCommands::Storage final: IUnknown {
    struct Draw {
        D3D12_INDEX_BUFFER_VIEW index{};std::array<D3D12_VERTEX_BUFFER_VIEW,16> vertices{};
        UINT mask{},indices{},instances{},first{},start{};INT base{};UINT64 instanceOffset{};
        bool indexBound{};std::optional<PureIndirectSource> indirect;
        indirect_policy::detail::Receipt signatureReceipt{};
        std::vector<ComPtr<ID3D12Resource>> owners;
    };
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;ComPtr<ID3D12Resource> instances;
    std::vector<Draw> draws,sources;UINT64 bytes{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
struct StereoGeometryCommands::IndirectStorage final: IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;
    PureIndirectSource source,replay;std::array<IndirectCopyProof,2> proofs;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
StereoGeometryCommands::StereoGeometryCommands()=default;
StereoGeometryCommands::~StereoGeometryCommands()=default;
namespace {
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* resource) {
    if(!resource)return false;ComPtr<ID3D12Device> actual;return SUCCEEDED(resource->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool Range(ID3D12Device* device,const geometry::BufferLease& lease,uint64_t address,uint64_t bytes) {
    return SameDevice(device,lease.resource.Get()) && bytes && address>=lease.gpu && address-lease.gpu<=lease.bytes && bytes<=lease.bytes-(address-lease.gpu);
}
bool SourceRange(ID3D12Device* device,const geometry::BufferLease& lease,uint64_t address,uint64_t bytes) {
    if(!lease.gpu || !lease.bytes || lease.bytes>UINT64_MAX-lease.gpu || !address || bytes>UINT64_MAX-address ||
        !Range(device,lease,address,bytes))return false;
    const auto desc=lease.resource->GetDesc();
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && lease.gpu==lease.resource->GetGPUVirtualAddress() && lease.bytes<=desc.Width;
}
bool IndirectRange(ID3D12Device* device,ID3D12Resource* resource,UINT64 offset,UINT64 bytes) {
    if(offset%4 || !SameDevice(device,resource))return false;const auto desc=resource->GetDesc();
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && offset<=desc.Width && bytes<=desc.Width-offset;
}
bool CopyProof(ID3D12Device* device,const IndirectCopyProof& proof,ID3D12Resource* resource,UINT64 offset,UINT64 bytes) {
    if(!proof.resource || proof.resource.Get()!=resource || !proof.state || !proof.recordingGeneration || !proof.commandPosition ||
        !proof.allocationOwner || !IndirectRange(device,resource,offset,bytes))return false;
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};if(FAILED(resource->GetHeapProperties(&heap,&flags)))return false;
    if(heap.Type==D3D12_HEAP_TYPE_UPLOAD)return *proof.state==D3D12_RESOURCE_STATE_GENERIC_READ;
    constexpr UINT reads=D3D12_RESOURCE_STATE_GENERIC_READ;
    const auto before=UINT(*proof.state);
    return heap.Type==D3D12_HEAP_TYPE_DEFAULT && (!(before&~reads) || before==D3D12_RESOURCE_STATE_COPY_DEST ||
        before==D3D12_RESOURCE_STATE_UNORDERED_ACCESS || before==D3D12_RESOURCE_STATE_STREAM_OUT);
}
HRESULT IndirectBuffer(ID3D12Device* device,UINT64 bytes,ComPtr<ID3D12Resource>& output) {
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=std::max(UINT64(4),bytes);
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    return device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&output));
}
bool EqualGeometry(const native_probe::DrawRecord& a,const native_probe::DrawRecord& b) {
    if(a.side!=1 || b.side!=0 || !a.frame || a.frame!=b.frame || !a.indices || !a.instances ||
       a.pipeline!=b.pipeline || a.root!=b.root || a.indices!=b.indices || a.instances!=b.instances ||
       a.firstIndex!=b.firstIndex || a.baseVertex!=b.baseVertex || a.inputSlotMask!=b.inputSlotMask ||
       (a.flags&191)!=191 || (b.flags&191)!=191 || ((a.flags|b.flags)&64) || !(a.inputSlotMask&128) || (a.inputSlotMask&~65535u) ||
       std::memcmp(&a.index,&b.index,sizeof(a.index)) || std::memcmp(&a.viewport,&b.viewport,sizeof(a.viewport)) ||
       std::memcmp(&a.scissor,&b.scissor,sizeof(a.scissor)))return false;
    for(UINT slot=0;slot<16;++slot)if(slot!=7 && (a.inputSlotMask&(1u<<slot)) &&
        std::memcmp(&a.vertices[slot],&b.vertices[slot],sizeof(a.vertices[slot])))return false;
    return true;
}
}
HRESULT StereoGeometryCommands::Create(ID3D12Device* device,std::span<const GeometryCommandPair> inputs) try {
    if(storage_ || !device || inputs.empty() || inputs.size()>2048)return E_INVALIDARG;
    ComPtr<Storage> pending;pending.Attach(new Storage);pending->device=device;
    std::map<std::pair<UINT,std::vector<uint8_t>>,UINT64> data;
    for(const auto& input:inputs) {
        const auto& a=input.eyes[0];const auto& b=input.eyes[1];const auto& draw=a.draw;
        if(!input.instanceRebaseSafe || !EqualGeometry(draw,b.draw) || a.instances!=b.instances || a.instances.empty() || a.instances.size()>16*1024)return E_INVALIDARG;
        const auto stride=draw.vertices[7].StrideInBytes;
        if((stride!=48 && stride!=64) || b.draw.vertices[7].StrideInBytes!=stride || a.instances.size()!=uint64_t(draw.instances)*stride)return E_INVALIDARG;
        for(const auto* eye:{&a,&b}) {
            const auto& d=eye->draw;const auto& v=d.vertices[7];
            if((uint64_t(d.firstInstance)+d.instances)*stride>v.SizeInBytes || !Range(device,eye->vertices[7],v.BufferLocation,v.SizeInBytes))return E_INVALIDARG;
        }
        const UINT indexBytes=draw.index.Format==DXGI_FORMAT_R16_UINT?2:draw.index.Format==DXGI_FORMAT_R32_UINT?4:0;
        if(!indexBytes || (uint64_t(draw.firstIndex)+draw.indices)*indexBytes>draw.index.SizeInBytes ||
           a.index.resource.Get()!=b.index.resource.Get() || !Range(device,a.index,draw.index.BufferLocation,draw.index.SizeInBytes))return E_INVALIDARG;
        Storage::Draw command;command.index=draw.index;command.vertices=draw.vertices;command.mask=draw.inputSlotMask;
        command.indices=draw.indices;command.instances=draw.instances;command.first=draw.firstIndex;command.base=draw.baseVertex;command.owners.push_back(a.index.resource);
        for(UINT slot=0;slot<16;++slot)if(slot!=7 && (command.mask&(1u<<slot))) {
            const auto& view=draw.vertices[slot];
            if(a.vertices[slot].resource.Get()!=b.vertices[slot].resource.Get() || !Range(device,a.vertices[slot],view.BufferLocation,view.SizeInBytes))return E_INVALIDARG;
            command.owners.push_back(a.vertices[slot].resource);
        }
        const auto key=std::make_pair(stride,a.instances);const auto found=data.find(key);
        if(found!=data.end())command.instanceOffset=found->second;
        else {
            if(a.instances.size()>32*1024*1024-pending->bytes)return E_OUTOFMEMORY;
            command.instanceOffset=pending->bytes;data.emplace(key,pending->bytes);pending->bytes+=a.instances.size();
        }
        command.vertices[7]={0,UINT(a.instances.size()),stride};pending->draws.push_back(std::move(command));
    }
    probe::Scope internal;D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=pending->bytes;
    desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES properties{};properties.Type=D3D12_HEAP_TYPE_UPLOAD;
    auto hr=device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&pending->instances));if(FAILED(hr))return hr;
    void* mapped{};const D3D12_RANGE none{};hr=pending->instances->Map(0,&none,&mapped);if(FAILED(hr))return hr;if(!mapped){pending->instances->Unmap(0,&none);return E_FAIL;}
    for(const auto& [key,offset]:data)std::memcpy(static_cast<uint8_t*>(mapped)+offset,key.second.data(),key.second.size());
    const D3D12_RANGE written{0,SIZE_T(pending->bytes)};pending->instances->Unmap(0,&written);
    const auto base=pending->instances->GetGPUVirtualAddress();for(auto& draw:pending->draws)draw.vertices[7].BufferLocation=base+draw.instanceOffset;
    storage_=std::move(pending);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool StereoGeometryCommands::RecordGeometry(ID3D12GraphicsCommandList* list,UINT index)const {
    if(!storage_ || !list || index>=storage_->draws.size() || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(storage_->device.Get(),list))return false;
    if(!gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    probe::Scope internal;const auto& draw=storage_->draws[index];list->IASetIndexBuffer(&draw.index);
    for(UINT slot=0;slot<16;++slot)if(draw.mask&(1u<<slot))list->IASetVertexBuffers(slot,1,&draw.vertices[slot]);
    list->DrawIndexedInstanced(draw.indices,draw.instances,draw.first,draw.base,0);return true;
}
UINT StereoGeometryCommands::DrawCount()const{return storage_?UINT(storage_->draws.size()):0;}
UINT64 StereoGeometryCommands::InstanceBytes()const{return storage_?storage_->bytes:0;}
HRESULT StereoGeometryCommands::CreateSources(ID3D12Device* device,std::span<const geometry::DrawResources> inputs) {
    return CreateSourcesImpl(device,inputs,{});
}
HRESULT StereoGeometryCommands::CreateMixedSources(ID3D12Device* device,std::span<const GeometrySourceCommand> inputs) {
    return CreateSourcesImpl(device,{},inputs);
}
HRESULT StereoGeometryCommands::CreateSourcesImpl(ID3D12Device* device,std::span<const geometry::DrawResources> direct,
    std::span<const GeometrySourceCommand> mixed) try {
    const auto count=direct.empty()?mixed.size():direct.size();
    if(storage_ || !device || !count || count>MaxSourceCommands || (!direct.empty() && !mixed.empty()))return E_INVALIDARG;
    ComPtr<Storage> pending;pending.Attach(new Storage);pending->device=device;pending->sources.reserve(count);
    for(size_t i=0;i<count;++i) {
        const auto& input=direct.empty()?mixed[i].geometry:direct[i];
        const auto* indirect=direct.empty() && mixed[i].indirect?&*mixed[i].indirect:nullptr;
        const auto& draw=input.draw;const auto mask=draw.inputSlotMask;
        if(!(draw.flags&128) || (mask&~65535u) || (!indirect && (!draw.indices || !draw.instances || !(mask&128))))return E_INVALIDARG;
        Storage::Draw command;UINT kind{};
        if(indirect) {
            command.signatureReceipt=indirect_policy::Describe(indirect->signature.Get());
            kind=indirect_policy::detail::DrawKind(command.signatureReceipt);
            const auto stride=indirect_policy::detail::PureDrawStride(command.signatureReceipt);
            if(!kind || !SameDevice(device,indirect->signature.Get()) || indirect->countOffset%4 ||
                !IndirectRange(device,indirect->arguments.Get(),indirect->argumentOffset,UINT64(indirect->maxCommandCount)*stride) ||
                (indirect->countBuffer && !IndirectRange(device,indirect->countBuffer.Get(),indirect->countOffset,4)))return E_INVALIDARG;
            command.indirect=*indirect;
        }
        // Pure DRAW does not consume the current IB. Its diagnostic view may
        // still describe a previous indexed draw without supplying an IB lease.
        command.indexBound=!indirect || kind==1 || input.index.resource;
        const UINT indexBytes=draw.index.Format==DXGI_FORMAT_R16_UINT?2:draw.index.Format==DXGI_FORMAT_R32_UINT?4:0;
        if(command.indexBound && (!indexBytes || draw.index.BufferLocation%indexBytes || draw.index.SizeInBytes%indexBytes ||
            (!indirect && (uint64_t(draw.firstIndex)+draw.indices)*indexBytes>draw.index.SizeInBytes) ||
            !SourceRange(device,input.index,draw.index.BufferLocation,draw.index.SizeInBytes)))return E_INVALIDARG;
        if(mask&128) {
            const auto& instance=draw.vertices[7];const auto stride=instance.StrideInBytes;
            if((stride!=48 && stride!=64) || (!indirect && (uint64_t(draw.firstInstance)+draw.instances)*stride>instance.SizeInBytes))return E_INVALIDARG;
        }
        command.index=draw.index;command.vertices=draw.vertices;command.mask=mask;
        command.indices=draw.indices;command.instances=draw.instances;command.first=draw.firstIndex;command.base=draw.baseVertex;command.start=draw.firstInstance;
        if(command.indexBound)command.owners.push_back(input.index.resource);
        for(UINT slot=0;slot<16;++slot)if(mask&(1u<<slot)) {
            const auto& view=draw.vertices[slot];
            if(!SourceRange(device,input.vertices[slot],view.BufferLocation,view.SizeInBytes))return E_INVALIDARG;
            command.owners.push_back(input.vertices[slot].resource);
        }
        pending->sources.push_back(std::move(command));
    }
    storage_=std::move(pending);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool StereoGeometryCommands::RecordSourceGeometry(ID3D12GraphicsCommandList* list,UINT index)const {
    if(!storage_ || !list || index>=storage_->sources.size() || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(storage_->device.Get(),list))return false;
    const auto& draw=storage_->sources[index];
    if(draw.indirect) {
        const auto receipt=indirect_policy::Describe(draw.indirect->signature.Get());
        if(std::memcmp(&receipt,&draw.signatureReceipt,sizeof(receipt)) || !indirect_policy::detail::DrawKind(receipt))return false;
    }
    if(!gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    probe::Scope internal;if(draw.indexBound)list->IASetIndexBuffer(&draw.index);
    for(UINT slot=0;slot<16;++slot)if(draw.mask&(1u<<slot))list->IASetVertexBuffers(slot,1,&draw.vertices[slot]);
    if(draw.indirect){const auto& indirect=*draw.indirect;
        list->ExecuteIndirect(indirect.signature.Get(),indirect.maxCommandCount,indirect.arguments.Get(),indirect.argumentOffset,indirect.countBuffer.Get(),indirect.countOffset);
    }else list->DrawIndexedInstanced(draw.indices,draw.instances,draw.first,draw.base,draw.start);
    return true;
}
UINT StereoGeometryCommands::SourceCount()const{return storage_?UINT(storage_->sources.size()):0;}
HRESULT StereoGeometryCommands::CaptureIndirectSource(ID3D12GraphicsCommandList* list,const PureIndirectSource& source,
    const IndirectCopyProof& arguments,const IndirectCopyProof* count,ValidateIndirectCopy validate,CapturedIndirectSource& output) try {
    if(!list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !validate || output.owner || output.replay.signature || output.replay.arguments || output.replay.countBuffer ||
        bool(source.countBuffer)!=(count!=nullptr) || source.countOffset%4)return E_INVALIDARG;
    const auto signature=indirect_policy::Describe(source.signature.Get());
    const auto kind=indirect_policy::detail::DrawKind(signature),stride=indirect_policy::detail::PureDrawStride(signature);
    const UINT64 bytes=UINT64(source.maxCommandCount)*stride,countBytes=count?4u:0u;
    if(!kind || bytes>MaxIndirectSnapshotBytes-countBytes)return E_INVALIDARG;
    ComPtr<ID3D12Device> device;auto hr=list->GetDevice(IID_PPV_ARGS(&device));if(FAILED(hr))return hr;
    if(!SameDevice(device.Get(),source.signature.Get()) || !CopyProof(device.Get(),arguments,source.arguments.Get(),source.argumentOffset,bytes) ||
        (count && (!CopyProof(device.Get(),*count,source.countBuffer.Get(),source.countOffset,4) ||
            count->recordingGeneration!=arguments.recordingGeneration || count->commandPosition!=arguments.commandPosition ||
            (count->resource==arguments.resource && count->state!=arguments.state))))return E_INVALIDARG;
    if(!validate(list,arguments,source.argumentOffset,bytes) || (count && !validate(list,*count,source.countOffset,4)))return E_ACCESSDENIED;
    ComPtr<IndirectStorage> pending;pending.Attach(new IndirectStorage);pending->device=device;pending->source=source;
    pending->proofs[0]=arguments;if(count)pending->proofs[1]=*count;
    pending->replay.signature=source.signature;pending->replay.maxCommandCount=source.maxCommandCount;
    {probe::Scope internal;
        hr=IndirectBuffer(device.Get(),std::max(bytes,UINT64(stride)),pending->replay.arguments);if(FAILED(hr))return hr;
        if(count){hr=IndirectBuffer(device.Get(),4,pending->replay.countBuffer);if(FAILED(hr))return hr;}
    }
    CapturedIndirectSource result;result.replay=pending->replay;result.owner=pending.Get();
    result.receipt={reinterpret_cast<uintptr_t>(list),arguments.recordingGeneration,arguments.commandPosition,kind,stride,
        {source.argumentOffset,source.countOffset},{bytes,countBytes}};
    std::array<D3D12_RESOURCE_BARRIER,2> sourceBarriers{},destinations{};UINT sourceCount{},destinationCount{};
    const auto addSource=[&](const IndirectCopyProof& proof){
        if(UINT(*proof.state)&D3D12_RESOURCE_STATE_COPY_SOURCE)return;
        for(UINT i=0;i<sourceCount;++i)if(sourceBarriers[i].Transition.pResource==proof.resource.Get())return;
        auto& barrier=sourceBarriers[sourceCount++];barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        const auto before=UINT(*proof.state);
        const auto after=before&~UINT(D3D12_RESOURCE_STATE_GENERIC_READ)?UINT(D3D12_RESOURCE_STATE_COPY_SOURCE):before|UINT(D3D12_RESOURCE_STATE_COPY_SOURCE);
        barrier.Transition={proof.resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,*proof.state,D3D12_RESOURCE_STATES(after)};
    };
    if(bytes)addSource(arguments);if(count)addSource(*count);
    for(auto* resource:{pending->replay.arguments.Get(),pending->replay.countBuffer.Get()})if(resource){
        auto& barrier=destinations[destinationCount++];barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT};
    }
    if(!gpu::KeepCommandResources(list,{pending.Get()}))return E_OUTOFMEMORY;
    const auto current=indirect_policy::Describe(source.signature.Get());
    if(std::memcmp(&current,&signature,sizeof(signature)) || !validate(list,arguments,source.argumentOffset,bytes) ||
        (count && !validate(list,*count,source.countOffset,4)))return E_ACCESSDENIED;
    probe::Scope internal;
    if(sourceCount)list->ResourceBarrier(sourceCount,sourceBarriers.data());
    if(bytes)list->CopyBufferRegion(result.replay.arguments.Get(),0,source.arguments.Get(),source.argumentOffset,bytes);
    if(count)list->CopyBufferRegion(result.replay.countBuffer.Get(),0,source.countBuffer.Get(),source.countOffset,4);
    for(UINT i=0;i<sourceCount;++i)std::swap(sourceBarriers[i].Transition.StateBefore,sourceBarriers[i].Transition.StateAfter);
    if(sourceCount)list->ResourceBarrier(sourceCount,sourceBarriers.data());
    list->ResourceBarrier(destinationCount,destinations.data());output=std::move(result);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}catch(...){return E_FAIL;}
#endif
}
