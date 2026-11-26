#include "Render/InstanceSnapshotView.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <algorithm>
#include <array>
#include <atomic>
#include <cstring>
#include <set>
#include <tuple>

namespace cvr::stereo::instance_snapshot_view {
namespace detail {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's plans are instance_snapshot_native.rs'
// The plan's versions are Rust (rust/crates/cvr-render/src/instance_snapshot_native.rs); the plan holds one reference.
extern "C" void cvr_isv2_plan_free(const void*);
struct PlanData {
    const void* rust{};
    explicit PlanData(const void* value):rust(value){}
    ~PlanData(){cvr_isv2_plan_free(rust);}
};
#endif
#else
struct Version {
    Scope scope;Mapping mapping;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> pipeline;
    std::shared_ptr<const shader_snapshot::Snapshot> shader;
    uint64_t shaderId{},uploadOffset{};
    std::vector<shader_snapshot::Input> layout;
    D3D12_VERTEX_BUFFER_VIEW original{};D3D12_DRAW_INDEXED_ARGUMENTS draw{};
    std::vector<uint8_t> bytes;
};
struct PlanData {std::vector<Version> versions;uint64_t width{},copied{};};
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // only the test variants' host lays out C++ snapshots
// The layout's arithmetic: rust/crates/cvr-render/src/instance_snapshot_view.rs; each element lent with what the rule
// reads of its semantic. No allocation: at most MaxInputs elements are lent, more are refused there.
namespace {
struct RustElement {size_t semanticBytes;bool semanticNul;UINT index,format,slot,offset,classification,step;};
static_assert(sizeof(RustElement)==40,"mirrored by instance_snapshot_view.rs");
extern "C" bool cvr_isv_layout(const RustElement*,size_t,UINT,size_t);
}
bool Layout(std::span<const shader_snapshot::Input> input,UINT stride)noexcept {
    std::array<RustElement,shader_snapshot::MaxInputs> lent{};
    if(input.size()>lent.size())return false;
    for(size_t i=0;i<input.size();++i){const auto& element=input[i];
        lent[i]={element.semantic.size(),element.semantic.find('\0')!=std::string::npos,element.index,element.format,element.slot,
            element.offset,element.classification,element.step};}
    return cvr_isv_layout(lent.data(),input.size(),stride,shader_snapshot::MaxInputs);
}
#endif
#else
namespace {
UINT ElementBytes(UINT format)noexcept {
    switch(DXGI_FORMAT(format)){
    case DXGI_FORMAT_R32G32B32A32_FLOAT:case DXGI_FORMAT_R32G32B32A32_UINT:case DXGI_FORMAT_R32G32B32A32_SINT:return 16;
    case DXGI_FORMAT_R32G32B32_FLOAT:case DXGI_FORMAT_R32G32B32_UINT:case DXGI_FORMAT_R32G32B32_SINT:return 12;
    case DXGI_FORMAT_R32G32_FLOAT:case DXGI_FORMAT_R32G32_UINT:case DXGI_FORMAT_R32G32_SINT:
    case DXGI_FORMAT_R16G16B16A16_FLOAT:case DXGI_FORMAT_R16G16B16A16_UNORM:case DXGI_FORMAT_R16G16B16A16_SNORM:
    case DXGI_FORMAT_R16G16B16A16_UINT:case DXGI_FORMAT_R16G16B16A16_SINT:return 8;
    case DXGI_FORMAT_R32_FLOAT:case DXGI_FORMAT_R32_UINT:case DXGI_FORMAT_R32_SINT:
    case DXGI_FORMAT_R16G16_FLOAT:case DXGI_FORMAT_R16G16_UNORM:case DXGI_FORMAT_R16G16_SNORM:
    case DXGI_FORMAT_R16G16_UINT:case DXGI_FORMAT_R16G16_SINT:
    case DXGI_FORMAT_R8G8B8A8_UNORM:case DXGI_FORMAT_R8G8B8A8_SNORM:case DXGI_FORMAT_R8G8B8A8_UINT:case DXGI_FORMAT_R8G8B8A8_SINT:
    case DXGI_FORMAT_R10G10B10A2_UNORM:case DXGI_FORMAT_R10G10B10A2_UINT:case DXGI_FORMAT_R11G11B10_FLOAT:return 4;
    case DXGI_FORMAT_R16_FLOAT:case DXGI_FORMAT_R16_UNORM:case DXGI_FORMAT_R16_SNORM:case DXGI_FORMAT_R16_UINT:case DXGI_FORMAT_R16_SINT:
    case DXGI_FORMAT_R8G8_UNORM:case DXGI_FORMAT_R8G8_SNORM:case DXGI_FORMAT_R8G8_UINT:case DXGI_FORMAT_R8G8_SINT:return 2;
    case DXGI_FORMAT_R8_UNORM:case DXGI_FORMAT_R8_SNORM:case DXGI_FORMAT_R8_UINT:case DXGI_FORMAT_R8_SINT:return 1;
    default:return 0;
    }
}
}
bool Layout(std::span<const shader_snapshot::Input> input,UINT stride)noexcept {
    if(input.empty() || input.size()>shader_snapshot::MaxInputs || (stride!=48 && stride!=64))return false;
    std::array<UINT,D3D12_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT> next{},classes{},steps{};
    std::array<bool,D3D12_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT> seen{};bool instance{};
    for(const auto& element:input){
        const auto bytes=ElementBytes(element.format);const auto alignment=bytes<4?bytes:4u;
        if(element.semantic.empty() || element.semantic.size()>255 || element.semantic.find('\0')!=std::string::npos ||
            element.slot>=seen.size() || !bytes ||
            (element.classification!=D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA && element.classification!=D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA))return false;
        if(seen[element.slot] && (classes[element.slot]!=element.classification || steps[element.slot]!=element.step))return false;
        seen[element.slot]=true;classes[element.slot]=element.classification;steps[element.slot]=element.step;
        if(element.classification==D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA){if(element.slot!=Slot || element.step!=1)return false;instance=true;}
        else if(element.slot==Slot || element.step)return false;
        UINT offset=element.offset;
        if(offset==D3D12_APPEND_ALIGNED_ELEMENT){if(next[element.slot]>UINT_MAX-(alignment-1))return false;
            offset=(next[element.slot]+alignment-1)&~(alignment-1);}
        if(offset%alignment || offset>UINT_MAX-bytes)return false;
        if(element.slot==Slot && (offset>stride || bytes>stride-offset))return false;
        next[element.slot]=offset+bytes;
    }
    return instance;
}
#endif
}
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's shader snapshots are shader_snapshot_native.rs' records (instance_snapshot_native.rs mod plugin)
namespace rust_abi {
// The shader snapshots the Rust versions hold: std::shared_ptr<const Snapshot> copies through this host.
struct Host {
    const void*(*current)(void*);void*(*retain)(const void*);void(*release)(void*);const void*(*get)(const void*);
    uint64_t(*id)(const void*);bool(*layout)(const void*,uint32_t);
};
// An Input lent to Rust (its shader as a pointer to its shared_ptr).
struct Input {Scope scope;ID3D12PipelineState* pipeline;const void* shader;D3D12_VERTEX_BUFFER_VIEW original;D3D12_DRAW_INDEXED_ARGUMENTS draw;
    const uint8_t* bytes;size_t size;bool hasBytes;};
// A PrivateView's parts, borrowed from the owner (copied by Fill).
struct View {Scope scope;void* resource;void* pipeline;const void* shader;D3D12_VERTEX_BUFFER_VIEW vertex,original;D3D12_DRAW_INDEXED_ARGUMENTS draw;
    Mapping mapping;uint64_t copyList,copyGeneration;UINT version;};
static_assert(sizeof(Input)==120 && sizeof(View)==192 && sizeof(std::shared_ptr<const shader_snapshot::Snapshot>)==16 &&
    sizeof(DestinationRecording)==40 && sizeof(DestinationCompletion)==64 && sizeof(NativeCompletion)==72 && sizeof(native_recording::Lease)==40 &&
    sizeof(PrivateView)==208 && offsetof(PrivateView,mapping)==136 && offsetof(PrivateView,version)==200,"mirrored by instance_snapshot_native.rs");
}
extern "C" {
void cvr_isv2_attach(const rust_abi::Host*);
const void* cvr_isv2_build_plan(const rust_abi::Input*,size_t,uint64_t,uint32_t*);
void cvr_isv2_plan_free(const void*);
uint64_t cvr_isv2_plan_count(const void*,uint32_t);
HRESULT cvr_isv2_overlay_new(ID3D12Device*,const void*,void**);
uint32_t cvr_isv2_overlay_record(void*,ID3D12GraphicsCommandList*,const DestinationRecording*);
bool cvr_isv2_overlay_view(void*,UINT,rust_abi::View*);
uint64_t cvr_isv2_count(void*,uint32_t);
bool cvr_isv2_overlay_submitted(void*,const DestinationRecording*,ID3D12CommandQueue*);
bool cvr_isv2_view_check(bool,uint32_t,const PrivateView*,const void*,ID3D12GraphicsCommandList*,const DestinationRecording*,
    const DestinationCompletion*,const native_recording::Lease*,const NativeCompletion*);
HRESULT cvr_isv2_arena_new(ID3D12Device*,const native_recording::Lease*,UINT,uint64_t,void**);
bool cvr_isv2_arena_capture(void*,ID3D12GraphicsCommandList*,const rust_abi::Input*,RecordNativeCopy,void*,const Input*,rust_abi::View*);
bool cvr_isv2_arena_finalize(void*,ID3D12GraphicsCommandList*);
}
namespace {
// The plan's versions, the overlay and the arena owners and their views' checks are Rust
// (rust/crates/cvr-render/src/instance_snapshot_native.rs; their GPU half instance_snapshot_arena.rs).
using Shared=std::shared_ptr<const shader_snapshot::Snapshot>;
const rust_abi::Host host{
    [](void* pipeline)->const void*{try{return shader_snapshot::Capture(static_cast<ID3D12PipelineState*>(pipeline)).get();}catch(...){return nullptr;}},
    [](const void* shared)->void*{try{const auto& value=*static_cast<const Shared*>(shared);return value?new Shared(value):nullptr;}catch(...){return nullptr;}},
    [](void* shared){delete static_cast<Shared*>(shared);},
    [](const void* shared)->const void*{return static_cast<const Shared*>(shared)->get();},
    [](const void* shared)->uint64_t{const auto& value=*static_cast<const Shared*>(shared);return value?value->id:0;},
    [](const void* shared,uint32_t stride)->bool{try{const auto& value=*static_cast<const Shared*>(shared);
        return value && detail::Layout(value->input,stride);}catch(...){return false;}},
};
[[maybe_unused]] const bool attached=(cvr_isv2_attach(&host),true);
rust_abi::Input Lend(const Input& input)noexcept{
    return {input.scope,input.pipeline.Get(),&input.shader,input.original,input.draw,input.bytes?input.bytes->data():nullptr,
        input.bytes?input.bytes->size():0,bool(input.bytes)};
}
void Fill(const rust_abi::View& view,IUnknown* owner,PrivateView& output){
    output.scope=view.scope;output.resource=static_cast<ID3D12Resource*>(view.resource);output.owner=owner;
    output.pipeline=static_cast<ID3D12PipelineState*>(view.pipeline);output.shader=*static_cast<const Shared*>(view.shader);
    output.vertex=view.vertex;output.original=view.original;output.draw=view.draw;output.mapping=view.mapping;
    output.copyList=view.copyList;output.copyGeneration=view.copyGeneration;output.version=view.version;
}
}
UINT Plan::VersionCount()const noexcept{return data_?UINT(cvr_isv2_plan_count(data_->rust,0)):0;}
uint64_t Plan::RequiredBytes()const noexcept{return data_?cvr_isv2_plan_count(data_->rust,1):0;}
uint64_t Plan::CopiedBytes()const noexcept{return data_?cvr_isv2_plan_count(data_->rust,2):0;}
bool BuildPlan(std::span<const Input> inputs,Plan& output,Failure& failure,uint64_t budget)try{
    output={};failure=Failure::Arguments;if(inputs.size()>MaxVersions)return false;
    std::vector<rust_abi::Input> rows;rows.reserve(inputs.size());for(const auto& input:inputs)rows.push_back(Lend(input));
    uint32_t code{};const void* rust=cvr_isv2_build_plan(rows.data(),rows.size(),budget,&code);failure=Failure(code);if(!rust)return false;
    try{output.data_=std::make_shared<const detail::PlanData>(rust);}catch(...){cvr_isv2_plan_free(rust);throw;}
    return true;
}catch(...){output={};failure=Failure::Exception;return false;}
struct Overlay::Storage:IUnknown{};
Overlay::Overlay()=default;Overlay::~Overlay()=default;
HRESULT Overlay::Create(ID3D12Device* device,const Plan& plan){
    if(storage_ || !device || !plan.data_)return E_INVALIDARG;
    void* out{};const auto result=cvr_isv2_overlay_new(device,plan.data_->rust,&out);
    if(SUCCEEDED(result))storage_.Attach(static_cast<Storage*>(out));return result;
}
bool Overlay::Record(ID3D12GraphicsCommandList* list,const DestinationRecording& destination,Failure& failure){
    failure=Failure::Arguments;if(!storage_ || !list)return false;
    failure=Failure(cvr_isv2_overlay_record(storage_.Get(),list,&destination));return failure==Failure::None;
}
bool Overlay::GetView(UINT index,PrivateView& output)const {
    output={};rust_abi::View view{};if(!storage_ || !cvr_isv2_overlay_view(storage_.Get(),index,&view))return false;
    Fill(view,storage_.Get(),output);return true;
}
bool Overlay::ValidateDestinationSubmission(const DestinationRecording& destination,ID3D12CommandQueue* queue)const {
    return storage_ && cvr_isv2_overlay_submitted(storage_.Get(),&destination,queue);
}
bool ValidatePrivateView(const PrivateView& view){
    return cvr_isv2_view_check(false,0,&view,view.shader.get(),nullptr,nullptr,nullptr,nullptr,nullptr);
}
bool ValidatePrivateViewForRecording(const PrivateView& view,ID3D12GraphicsCommandList* list,const DestinationRecording& destination){
    return cvr_isv2_view_check(false,1,&view,view.shader.get(),list,&destination,nullptr,nullptr,nullptr);
}
bool ValidatePrivateViewAfterCompletion(const PrivateView& view,const DestinationCompletion& completion){
    return cvr_isv2_view_check(false,2,&view,view.shader.get(),nullptr,nullptr,&completion,nullptr,nullptr);
}
struct GpuCaptureArena::Storage:IUnknown{};
GpuCaptureArena::GpuCaptureArena()=default;GpuCaptureArena::~GpuCaptureArena()=default;
HRESULT GpuCaptureArena::Create(ID3D12Device* device,const native_recording::Lease& recording,UINT eye,uint64_t capacity){
    if(storage_ || !device)return E_INVALIDARG;
    void* out{};const auto result=cvr_isv2_arena_new(device,&recording,eye,capacity,&out);if(FAILED(result))return result;
    storage_.Attach(static_cast<Storage*>(out));return S_OK;
}
bool GpuCaptureArena::Capture(ID3D12GraphicsCommandList* list,const Input& input,RecordNativeCopy record,void* context,PrivateView& output){
    output={};if(!storage_ || !list || !record)return false;
    const auto row=Lend(input);rust_abi::View view{};
    if(!cvr_isv2_arena_capture(storage_.Get(),list,&row,record,context,&input,&view))return false;
    Fill(view,storage_.Get(),output);return true;
}
bool GpuCaptureArena::Finalize(ID3D12GraphicsCommandList* list){return storage_ && list && cvr_isv2_arena_finalize(storage_.Get(),list);}
UINT GpuCaptureArena::VersionCount()const{return storage_?UINT(cvr_isv2_count(storage_.Get(),0)):0;}
uint64_t GpuCaptureArena::RequiredBytes()const{return storage_?cvr_isv2_count(storage_.Get(),1):0;}
uint64_t GpuCaptureArena::CopiedBytes()const{return storage_?cvr_isv2_count(storage_.Get(),2):0;}
bool ValidateNativeView(const PrivateView& view){
    return cvr_isv2_view_check(true,0,&view,view.shader.get(),nullptr,nullptr,nullptr,nullptr,nullptr);
}
bool ValidateNativeViewForRecording(const PrivateView& view,ID3D12GraphicsCommandList* list,const native_recording::Lease& recording){
    return cvr_isv2_view_check(true,1,&view,view.shader.get(),list,nullptr,nullptr,&recording,nullptr);
}
bool ValidateNativeViewAfterSubmission(const PrivateView& view,const NativeCompletion& completion){
    return cvr_isv2_view_check(true,2,&view,view.shader.get(),nullptr,nullptr,nullptr,nullptr,&completion);
}
bool ValidateNativeViewAfterCompletion(const PrivateView& view,const NativeCompletion& completion){
    return cvr_isv2_view_check(true,3,&view,view.shader.get(),nullptr,nullptr,nullptr,nullptr,&completion);
}
#endif
#else
namespace {
constexpr GUID ownerType{0x0e1d90db,0x6f45,0x442e,{0x89,0x1c,0x95,0x0e,0xa5,0x59,0xe2,0x7c}};
enum class Stage:uint32_t {Created,Recording,Recorded,Failed};
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){
    Microsoft::WRL::ComPtr<ID3D12Device> actual;
    return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool SameInput(const shader_snapshot::Input& a,const shader_snapshot::Input& b){
    return a.semantic==b.semantic && a.index==b.index && a.format==b.format && a.slot==b.slot && a.offset==b.offset && a.classification==b.classification && a.step==b.step;
}
bool Qualified(const detail::Version& version){
    const auto current=shader_snapshot::Capture(version.pipeline.Get());
    return current && current==version.shader && version.shaderId && current->id==version.shaderId &&
        current->input.size()==version.layout.size() && std::equal(current->input.begin(),current->input.end(),version.layout.begin(),SameInput) &&
        detail::Layout(version.layout,version.original.StrideInBytes);
}
bool SameView(const D3D12_VERTEX_BUFFER_VIEW& a,const D3D12_VERTEX_BUFFER_VIEW& b){
    return a.BufferLocation==b.BufferLocation && a.SizeInBytes==b.SizeInBytes && a.StrideInBytes==b.StrideInBytes;
}
bool SameDraw(const D3D12_DRAW_INDEXED_ARGUMENTS& a,const D3D12_DRAW_INDEXED_ARGUMENTS& b){
    return a.IndexCountPerInstance==b.IndexCountPerInstance && a.InstanceCount==b.InstanceCount && a.StartIndexLocation==b.StartIndexLocation &&
        a.BaseVertexLocation==b.BaseVertexLocation && a.StartInstanceLocation==b.StartInstanceLocation;
}
D3D12_VERTEX_BUFFER_VIEW View(const detail::Version& version,ID3D12Resource* resource){
    auto result=version.original;result.BufferLocation=resource->GetGPUVirtualAddress()+version.mapping.viewOffset;return result;
}
HRESULT Buffer(ID3D12Device* device,uint64_t bytes,D3D12_HEAP_TYPE type,D3D12_RESOURCE_STATES state,ID3D12Resource** output){
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=type;heap.CreationNodeMask=heap.VisibleNodeMask=1;
    return device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(output));
}
}
UINT Plan::VersionCount()const noexcept{return data_?UINT(data_->versions.size()):0;}
uint64_t Plan::RequiredBytes()const noexcept{return data_?data_->width:0;}
uint64_t Plan::CopiedBytes()const noexcept{return data_?data_->copied:0;}
const Mapping* Plan::VersionMapping(UINT index)const noexcept{return data_ && index<data_->versions.size()?&data_->versions[index].mapping:nullptr;}
const Scope* Plan::VersionScope(UINT index)const noexcept{return data_ && index<data_->versions.size()?&data_->versions[index].scope:nullptr;}
bool BuildPlan(std::span<const Input> inputs,Plan& output,Failure& failure,uint64_t budget)try{
    output={};failure=Failure::Arguments;if(inputs.empty() || inputs.size()>MaxVersions || !budget || budget>MaxBytes)return false;
    auto pending=std::make_shared<detail::PlanData>();pending->versions.reserve(inputs.size());uint64_t cursor{};
    std::set<std::tuple<uint64_t,uint64_t,uint64_t,uint64_t,UINT>> seen;
    for(const auto& input:inputs){
        failure=Failure::Scope;
        if(!detail::ValidScope(input.scope) || !seen.emplace(input.scope.root,input.scope.list,input.scope.generation,input.scope.occurrence,input.scope.eye).second)return false;
        failure=Failure::Layout;
        if(!input.pipeline || !input.shader || !input.shader->id || shader_snapshot::Capture(input.pipeline.Get())!=input.shader ||
            !detail::Layout(input.shader->input,input.original.StrideInBytes))return false;
        failure=Failure::View;if(!detail::ViewShape(input.original))return false;
        failure=Failure::Range;if(!input.draw.IndexCountPerInstance)return false;
        detail::Version version;version.scope=input.scope;version.pipeline=input.pipeline;version.shader=input.shader;version.shaderId=input.shader->id;
        version.layout=input.shader->input;version.original=input.original;version.draw=input.draw;
        failure=Failure::Capacity;if(!detail::Reserve(cursor,input.original,input.draw.StartInstanceLocation,input.draw.InstanceCount,budget,version.mapping))return false;
        failure=Failure::Bytes;if(!input.bytes || input.bytes->size()!=version.mapping.copyBytes || input.bytes->empty())return false;
        version.bytes=*input.bytes;version.uploadOffset=pending->copied;
        cursor=version.mapping.nextFree;pending->width=(std::max)(pending->width,version.mapping.viewEnd);
        if(version.mapping.copyBytes>MaxBytes-pending->copied){failure=Failure::Capacity;return false;}
        pending->copied+=version.mapping.copyBytes;pending->versions.push_back(std::move(version));
    }
    output.data_=std::move(pending);failure=Failure::None;return true;
}catch(...){output={};failure=Failure::Exception;return false;}
struct Overlay::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};std::atomic<Stage> stage{Stage::Created};
    Microsoft::WRL::ComPtr<ID3D12Device> device;Microsoft::WRL::ComPtr<ID3D12Resource> resource,upload;
    std::shared_ptr<const detail::PlanData> plan;DestinationRecording destination;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** value)override{
        if(!value)return E_POINTER;*value=nullptr;if(id!=__uuidof(IUnknown) && id!=ownerType)return E_NOINTERFACE;*value=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count)delete this;return count;}
};
Overlay::Overlay()=default;Overlay::~Overlay()=default;
HRESULT Overlay::Create(ID3D12Device* device,const Plan& plan)try{
    if(storage_ || !device || !plan.data_ || !plan.VersionCount() || !plan.RequiredBytes() || plan.RequiredBytes()>MaxBytes ||
        !plan.CopiedBytes() || plan.CopiedBytes()>MaxBytes || device->GetNodeCount()!=1)return E_INVALIDARG;
    for(const auto& version:plan.data_->versions)if(!Qualified(version) || !SameDevice(device,version.pipeline.Get()))return E_INVALIDARG;
    Microsoft::WRL::ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->plan=plan.data_;
    HRESULT result;{probe::Scope internal;
        result=Buffer(device,plan.RequiredBytes(),D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_STATE_COPY_DEST,next->resource.GetAddressOf());
        if(SUCCEEDED(result))result=Buffer(device,plan.CopiedBytes(),D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ,next->upload.GetAddressOf());}
    if(FAILED(result))return result;
    const auto gpu=next->resource->GetGPUVirtualAddress();if(!gpu || gpu>UINT64_MAX-plan.RequiredBytes())return E_INVALIDARG;
    void* mapped{};const D3D12_RANGE none{};result=next->upload->Map(0,&none,&mapped);if(FAILED(result))return result;
    if(!mapped){next->upload->Unmap(0,&none);return E_FAIL;}
    for(const auto& version:plan.data_->versions)std::memcpy(static_cast<uint8_t*>(mapped)+version.uploadOffset,version.bytes.data(),version.bytes.size());
    const D3D12_RANGE written{0,SIZE_T(plan.CopiedBytes())};next->upload->Unmap(0,&written);storage_=std::move(next);return S_OK;
}catch(...){return E_OUTOFMEMORY;}
bool Overlay::Record(ID3D12GraphicsCommandList* list,const DestinationRecording& destination,Failure& failure)try{
    failure=Failure::Arguments;if(!storage_ || !list)return false;
    failure=Failure::Recording;
    if(!destination.certifiedCurrentRecording || !destination.generation || destination.list!=reinterpret_cast<uintptr_t>(list) || !destination.allocator || !destination.queue ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || destination.queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        !SameDevice(storage_->device.Get(),list) || !SameDevice(storage_->device.Get(),destination.allocator.Get()) || !SameDevice(storage_->device.Get(),destination.queue.Get()))return false;
    failure=Failure::AlreadyRecorded;if(storage_->stage.load(std::memory_order_acquire)!=Stage::Created)return false;
    failure=Failure::Changed;for(const auto& version:storage_->plan->versions)if(!Qualified(version))return false;
    failure=Failure::Retention;if(!gpu::KeepCommandResources(list,{storage_.Get(),destination.allocator.Get(),destination.queue.Get()}))return false;
    auto expected=Stage::Created;failure=Failure::AlreadyRecorded;
    if(!storage_->stage.compare_exchange_strong(expected,Stage::Recording,std::memory_order_acq_rel))return false;
    storage_->destination=destination;
    {probe::Scope internal;
        for(const auto& version:storage_->plan->versions)list->CopyBufferRegion(storage_->resource.Get(),version.mapping.targetOffset,storage_->upload.Get(),version.uploadOffset,version.mapping.copyBytes);
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={storage_->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER|D3D12_RESOURCE_STATE_COPY_SOURCE)};list->ResourceBarrier(1,&barrier);}
    storage_->stage.store(Stage::Recorded,std::memory_order_release);failure=Failure::None;return true;
}catch(...){if(storage_ && storage_->stage.load()==Stage::Recording)storage_->stage.store(Stage::Failed,std::memory_order_release);
    failure=Failure::Exception;return false;}
bool Overlay::GetView(UINT index,PrivateView& output)const {
    output={};if(!storage_ || storage_->stage.load(std::memory_order_acquire)!=Stage::Recorded || index>=storage_->plan->versions.size())return false;
    const auto& version=storage_->plan->versions[index];output.scope=version.scope;output.resource=storage_->resource;output.owner=storage_;
    output.pipeline=version.pipeline;output.shader=version.shader;output.original=version.original;output.vertex=View(version,storage_->resource.Get());
    output.draw=version.draw;output.mapping=version.mapping;output.version=index;output.copyList=storage_->destination.list;output.copyGeneration=storage_->destination.generation;return true;
}
UINT Overlay::VersionCount()const noexcept{return storage_?UINT(storage_->plan->versions.size()):0;}
uint64_t Overlay::RequiredBytes()const noexcept{return storage_?storage_->plan->width:0;}
uint64_t Overlay::CopiedBytes()const noexcept{return storage_?storage_->plan->copied:0;}
bool Overlay::ValidateDestinationSubmission(const DestinationRecording& destination,ID3D12CommandQueue* queue)const {
    if(!storage_ || storage_->stage.load(std::memory_order_acquire)!=Stage::Recorded)return false;const auto& actual=storage_->destination;
    return destination.certifiedCurrentRecording && destination.list==actual.list && destination.generation==actual.generation && destination.allocator==actual.allocator &&
        destination.queue==actual.queue && queue==actual.queue.Get() && SameDevice(storage_->device.Get(),queue);
}
bool ValidatePrivateView(const PrivateView& view){
    Microsoft::WRL::ComPtr<Overlay::Storage> storage;
    if(!view.owner || FAILED(view.owner->QueryInterface(ownerType,reinterpret_cast<void**>(storage.GetAddressOf()))) ||
        storage->stage.load(std::memory_order_acquire)!=Stage::Recorded || view.version>=storage->plan->versions.size() || view.resource!=storage->resource)return false;
    const auto& version=storage->plan->versions[view.version];const auto expected=View(version,storage->resource.Get());
    return Qualified(version) && view.scope==version.scope && view.pipeline==version.pipeline && view.shader==version.shader &&
        SameView(view.vertex,expected) && SameView(view.original,version.original) && SameDraw(view.draw,version.draw) && view.mapping==version.mapping &&
        view.copyList==storage->destination.list && view.copyGeneration==storage->destination.generation;
}
bool ValidatePrivateViewForRecording(const PrivateView& view,ID3D12GraphicsCommandList* list,const DestinationRecording& destination){
    Microsoft::WRL::ComPtr<Overlay::Storage> storage;
    if(!ValidatePrivateView(view) || !list || view.copyList!=reinterpret_cast<uintptr_t>(list) || destination.list!=view.copyList ||
        destination.generation!=view.copyGeneration || !destination.certifiedCurrentRecording ||
        FAILED(view.owner->QueryInterface(ownerType,reinterpret_cast<void**>(storage.GetAddressOf()))))return false;
    return destination.allocator==storage->destination.allocator && destination.queue==storage->destination.queue &&
        gpu::HasCommandResource(list,storage.Get()) && SameDevice(storage->device.Get(),list);
}
bool ValidatePrivateViewAfterCompletion(const PrivateView& view,const DestinationCompletion& completion){
    Microsoft::WRL::ComPtr<Overlay::Storage> storage;
    if(!ValidatePrivateView(view) || !completion.submissionCertified || !completion.fence || !completion.value ||
        FAILED(view.owner->QueryInterface(ownerType,reinterpret_cast<void**>(storage.GetAddressOf()))))return false;
    const auto& actual=storage->destination;const auto& recording=completion.recording;
    if(!recording.certifiedCurrentRecording || recording.list!=actual.list || recording.generation!=actual.generation || recording.allocator!=actual.allocator ||
        recording.queue!=actual.queue || !SameDevice(storage->device.Get(),completion.fence.Get()))return false;
    const auto value=completion.fence->GetCompletedValue();return value!=UINT64_MAX && value>=completion.value;
}
namespace {
constexpr GUID nativeOwnerType{0xec3f3de4,0x7a7d,0x4eea,{0x92,0x26,0xe9,0x24,0x4c,0xa6,0xbb,0x23}};
bool SameRecording(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.list && a.list==b.list && a.generation && a.generation==b.generation &&
        a.context && a.context==b.context && a.registry && a.registry==b.registry;
}
}
struct GpuCaptureArena::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};std::atomic<Stage> stage{Stage::Created};
    Microsoft::WRL::ComPtr<ID3D12Device> device;Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    native_recording::Lease recording;UINT eye{};uint64_t capacity{},cursor{},width{},copied{};
    std::vector<detail::Version> versions;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** value)override{
        if(!value)return E_POINTER;*value=nullptr;if(id!=__uuidof(IUnknown) && id!=nativeOwnerType)return E_NOINTERFACE;*value=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count)delete this;return count;}
};
GpuCaptureArena::GpuCaptureArena()=default;GpuCaptureArena::~GpuCaptureArena()=default;
HRESULT GpuCaptureArena::Create(ID3D12Device* device,const native_recording::Lease& recording,UINT eye,uint64_t capacity)try{
    if(storage_ || !device || device->GetNodeCount()!=1 || !SameRecording(recording,recording) || eye>1 || !capacity || capacity>MaxBytes/2)return E_INVALIDARG;
    Microsoft::WRL::ComPtr<Storage> next;next.Attach(new Storage);
    next->device=device;next->recording=recording;next->eye=eye;next->capacity=capacity;
    next->versions.reserve(MaxVersions);
    {probe::Scope internal;const auto hr=Buffer(device,capacity,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_STATE_COPY_DEST,next->resource.GetAddressOf());if(FAILED(hr))return hr;}
    storage_=std::move(next);return S_OK;
}catch(...){return E_OUTOFMEMORY;}
bool GpuCaptureArena::Capture(ID3D12GraphicsCommandList* list,const Input& input,RecordNativeCopy record,void* context,PrivateView& output)try{
    output={};if(!storage_ || !list || !record)return false;auto& s=*storage_.Get();
    if(s.stage!=Stage::Created || s.versions.size()>=MaxVersions || input.bytes || !detail::ValidScope(input.scope) ||
        input.scope.list!=s.recording.list || input.scope.generation!=s.recording.generation || input.scope.eye!=s.eye ||
        reinterpret_cast<uintptr_t>(list)!=s.recording.list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(s.device.Get(),list) ||
        !input.pipeline || !input.shader || !input.shader->id || !input.draw.IndexCountPerInstance || !SameDevice(s.device.Get(),input.pipeline.Get()) ||
        std::any_of(s.versions.begin(),s.versions.end(),[&](const auto& version){return version.scope.occurrence==input.scope.occurrence;}))return false;
    detail::Version version;version.scope=input.scope;version.pipeline=input.pipeline;version.shader=input.shader;version.shaderId=input.shader->id;
    version.layout=input.shader->input;version.original=input.original;version.draw=input.draw;
    if(!Qualified(version) || !detail::Reserve(s.cursor,input.original,input.draw.StartInstanceLocation,input.draw.InstanceCount,s.capacity,version.mapping) ||
        !gpu::KeepCommandResources(list,{&s}))return false;
    // No allocation is allowed after the source callback begins recording.
    const auto index=UINT(s.versions.size());s.versions.push_back(std::move(version));auto& saved=s.versions.back();
    s.stage=Stage::Recording;
    if(!record(context,list,input,s.resource.Get(),saved.mapping.targetOffset)){s.stage=Stage::Failed;return false;}
    s.cursor=saved.mapping.nextFree;s.width=std::max(s.width,saved.mapping.viewEnd);s.copied+=saved.mapping.copyBytes;s.stage=Stage::Created;
    output.scope=saved.scope;output.resource=s.resource;output.owner=storage_;output.pipeline=saved.pipeline;output.shader=saved.shader;
    output.vertex=View(saved,s.resource.Get());output.original=saved.original;output.draw=saved.draw;output.mapping=saved.mapping;
    output.copyList=s.recording.list;output.copyGeneration=s.recording.generation;output.version=index;return true;
}catch(...){if(storage_)storage_->stage=Stage::Failed;output={};return false;}
bool GpuCaptureArena::Finalize(ID3D12GraphicsCommandList* list){
    if(!storage_ || !list)return false;auto& s=*storage_.Get();
    if(s.stage!=Stage::Created || s.versions.empty() || reinterpret_cast<uintptr_t>(list)!=s.recording.list ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(s.device.Get(),list) || !gpu::KeepCommandResources(list,{&s}))return false;
    probe::Scope internal;D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={s.resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_GENERIC_READ};
    list->ResourceBarrier(1,&barrier);s.stage=Stage::Recorded;return true;
}
UINT GpuCaptureArena::VersionCount()const{return storage_?UINT(storage_->versions.size()):0;}
uint64_t GpuCaptureArena::RequiredBytes()const{return storage_?storage_->width:0;}
uint64_t GpuCaptureArena::CopiedBytes()const{return storage_?storage_->copied:0;}
ID3D12Resource* GpuCaptureArena::Resource()const{return storage_?storage_->resource.Get():nullptr;}
bool ValidateNativeView(const PrivateView& view){
    Microsoft::WRL::ComPtr<GpuCaptureArena::Storage> storage;
    if(!view.owner || FAILED(view.owner->QueryInterface(nativeOwnerType,reinterpret_cast<void**>(storage.GetAddressOf()))) ||
        storage->stage!=Stage::Recorded || view.version>=storage->versions.size() || view.resource!=storage->resource)return false;
    const auto& v=storage->versions[view.version];const auto expected=View(v,storage->resource.Get());
    return Qualified(v) && view.scope==v.scope && view.pipeline==v.pipeline && view.shader==v.shader && SameView(view.vertex,expected) &&
        SameView(view.original,v.original) && SameDraw(view.draw,v.draw) && view.mapping==v.mapping &&
        view.copyList==storage->recording.list && view.copyGeneration==storage->recording.generation;
}
bool ValidateNativeViewForRecording(const PrivateView& view,ID3D12GraphicsCommandList* list,const native_recording::Lease& recording){
    Microsoft::WRL::ComPtr<GpuCaptureArena::Storage> storage;
    return ValidateNativeView(view) && list && view.copyList==reinterpret_cast<uintptr_t>(list) &&
        SUCCEEDED(view.owner->QueryInterface(nativeOwnerType,reinterpret_cast<void**>(storage.GetAddressOf()))) &&
        SameRecording(recording,storage->recording) && gpu::HasCommandResource(list,storage.Get()) && SameDevice(storage->device.Get(),list);
}
bool ValidateNativeViewAfterSubmission(const PrivateView& view,const NativeCompletion& completion){
    Microsoft::WRL::ComPtr<GpuCaptureArena::Storage> storage;
    return ValidateNativeView(view) && completion.submissionCertified && completion.value && completion.value!=UINT64_MAX && completion.queue && completion.fence &&
        SUCCEEDED(view.owner->QueryInterface(nativeOwnerType,reinterpret_cast<void**>(storage.GetAddressOf()))) &&
        SameRecording(completion.recording,storage->recording) && SameDevice(storage->device.Get(),completion.fence.Get()) &&
        SameDevice(storage->device.Get(),completion.queue.Get()) && completion.queue->GetDesc().Type==D3D12_COMMAND_LIST_TYPE_DIRECT;
}
bool ValidateNativeViewAfterCompletion(const PrivateView& view,const NativeCompletion& completion){
    if(!ValidateNativeViewAfterSubmission(view,completion))return false;
    const auto completed=completion.fence->GetCompletedValue();return completed!=UINT64_MAX && completed>=completion.value;
}
#endif
}
