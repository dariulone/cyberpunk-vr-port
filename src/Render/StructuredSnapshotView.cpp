#include "Render/StructuredSnapshotView.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <algorithm>
#include <atomic>
#include <cstring>
#include <set>
#include <map>
#include <tuple>

namespace cvr::stereo::structured_snapshot_view {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
// The plan's versions, the adoption, the overlay's buffer and copies, the views: rust/crates/cvr-render/src/
// structured_view_overlay.rs. The read plan's check, the overlay's COM owner, stage and destination, the
// command-resource retention and the internal-commands scope stay here.
struct RustSovInput {
    Scope scope,b4Scope,completionScope;
    ID3D12Resource* dense;IUnknown* denseOwner;uint64_t denseSource,densePrivate,denseCopied;
    IUnknown* copyOwner;uint64_t copyList,copyGeneration,copyOccurrence,copySource,copyPrivate,copyCopied;
    ID3D12CommandQueue* queue;ID3D12Fence* fence;uint64_t value,firstElement,resourceWidth;uint32_t numElements,stride;
    const uint8_t* b4;size_t b4Bytes;const uint8_t* planB4;bool certified,finalized,b4Verified,bounded,planView;
};
struct RustSovRead {uint64_t resourceOffset;uint32_t byteExtent,firstRecord,recordSpan;const uint32_t* records;size_t count;};
struct RustSovDestination {uint64_t list,generation;ID3D12CommandAllocator* allocator;ID3D12CommandQueue* queue;bool certified;};
static_assert(sizeof(RustSovInput)==296 && sizeof(RustSovRead)==40 && sizeof(RustSovDestination)==40 && sizeof(PrivateView)==520 &&
    offsetof(PrivateView,srv)==56 && offsetof(PrivateView,records)==160 && offsetof(PrivateView,privateB4Bytes)==288,"mirrored by structured_view_overlay.rs");
extern "C" {
void* cvr_sov_plan_new();
void cvr_sov_plan_free(void*);
uint32_t cvr_sov_admit(const void*,const RustSovInput*);
uint32_t cvr_sov_push(void*,const RustSovInput*,const RustSovRead*,uint64_t);
uint64_t cvr_sov_counts(const void*,uint32_t);
}
namespace detail {
struct PlanData {
    void* rust=cvr_sov_plan_new();
    PlanData()=default;PlanData(const PlanData&)=delete;PlanData& operator=(const PlanData&)=delete;
    ~PlanData(){cvr_sov_plan_free(rust);}
};
}
namespace {
bool SameReadPlan(const multilayer_selectors::ReadPlan& a,const multilayer_selectors::ReadPlan& b){
    return a.family==b.family && a.b4Key==b.b4Key && a.b4Bytes==b.b4Bytes && a.view==b.view && a.baseRecord==b.baseRecord && a.mask==b.mask &&
        a.firstRecord==b.firstRecord && a.recordSpan==b.recordSpan && a.records==b.records && a.resourceOffset==b.resourceOffset && a.byteExtent==b.byteExtent;
}
RustSovInput Facts(const Input& input){const auto& view=input.originalView;const auto& dense=input.dense;const auto& copy=input.copy;const auto& done=input.completion;
    return {input.scope,input.privateB4Scope,done.scope,dense.resource.Get(),dense.owner.Get(),dense.sourceOffset,dense.privateOffset,dense.copiedBytes,
        copy.owner.Get(),copy.list,copy.generation,copy.occurrence,copy.sourceOffset,copy.privateOffset,copy.copiedBytes,done.queue.Get(),done.fence.Get(),
        done.value,view.firstElement,view.resourceWidth,view.numElements,view.structureByteStride,input.privateB4Bytes.data(),input.privateB4Bytes.size(),
        input.readPlan.b4Bytes.data(),done.submissionCertified,done.finalized,input.privateB4GpuMatchVerified,input.allRecordPathsBounded,input.readPlan.view==view};
}
}
UINT Plan::VersionCount()const noexcept{return data_?UINT(cvr_sov_counts(data_->rust,0)):0;}
uint64_t Plan::RequiredBytes()const noexcept{return data_?cvr_sov_counts(data_->rust,1):0;}
uint64_t Plan::CopiedBytes()const noexcept{return data_?cvr_sov_counts(data_->rust,2):0;}
bool BuildPlan(std::span<const Input> inputs,Plan& output,Failure& failure,uint64_t budget)try{
    output={};failure=Failure::Arguments;if(inputs.empty() || inputs.size()>MaxVersions || !budget || budget>MaxBytes)return false;
    auto pending=std::make_shared<detail::PlanData>();
    for(const auto& input:inputs){const auto facts=Facts(input);
        failure=Failure(cvr_sov_admit(pending->rust,&facts));if(failure!=Failure::None)return false;
        multilayer_selectors::ReadPlan checked;std::string error;failure=Failure::ReadPlan;
        if(!input.readPlan.conservativeRecordSuperset || !input.readPlan.originalSiteCoverageComplete ||
            !multilayer_selectors::PlanRead(input.readPlan.family,{input.readPlan.b4Key,input.privateB4Bytes},input.originalView,checked,error) ||
            !SameReadPlan(input.readPlan,checked) || checked.records.empty() || checked.records.size()>multilayer_selectors::MaxRecords)return false;
        const RustSovRead read{checked.resourceOffset,checked.byteExtent,checked.firstRecord,checked.recordSpan,checked.records.data(),checked.records.size()};
        failure=Failure(cvr_sov_push(pending->rust,&facts,&read,budget));if(failure!=Failure::None)return false;
    }
    output.data_=std::move(pending);failure=Failure::None;return true;
}catch(...){output={};failure=Failure::Exception;return false;}
#endif
#else
namespace detail {
struct Version {
    Scope scope;Mapping mapping;
    structured_buffer_snapshot::PrivateRange dense;
    structured_buffer_snapshot::PreparedCopy copy;
    SourceCompletion completion;
    multilayer_selectors::ReadPlan readPlan;
    std::array<uint8_t,multilayer_selectors::B4Bytes> b4{};
};
struct PlanData {std::vector<Version> versions;uint64_t width{},copied{};};
}
namespace {
constexpr GUID ownerType{0x2ae6e4c2,0xf181,0x4921,{0xb0,0xa4,0x69,0x90,0x74,0x20,0x7a,0x34}};
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){
    Microsoft::WRL::ComPtr<ID3D12Device> actual;return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;
}
bool DenseRange(const detail::Version& version){
    const auto& value=version.dense;const auto& copy=version.copy;
    if(!value.resource || !value.owner || !copy.owner || copy.list!=version.scope.list || copy.generation!=version.scope.generation ||
        copy.occurrence!=version.scope.occurrence || copy.sourceOffset!=value.sourceOffset || copy.privateOffset!=value.privateOffset ||
        copy.copiedBytes!=value.copiedBytes || value.sourceOffset!=version.readPlan.resourceOffset ||
        value.copiedBytes!=version.readPlan.byteExtent || !value.copiedBytes || value.copiedBytes>4096 || value.privateOffset%Stride)return false;
    const auto desc=value.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && !desc.Flags && value.privateOffset<=desc.Width && value.copiedBytes<=desc.Width-value.privateOffset &&
        SUCCEEDED(value.resource->GetHeapProperties(&heap,&flags)) && heap.Type==D3D12_HEAP_TYPE_DEFAULT;
}
bool SameReadPlan(const multilayer_selectors::ReadPlan& a,const multilayer_selectors::ReadPlan& b){
    return a.family==b.family && a.b4Key==b.b4Key && a.b4Bytes==b.b4Bytes && a.view==b.view && a.baseRecord==b.baseRecord && a.mask==b.mask &&
        a.firstRecord==b.firstRecord && a.recordSpan==b.recordSpan && a.records==b.records && a.resourceOffset==b.resourceOffset && a.byteExtent==b.byteExtent;
}
D3D12_SHADER_RESOURCE_VIEW_DESC View(const detail::Version& version){
    D3D12_SHADER_RESOURCE_VIEW_DESC result{};result.Format=DXGI_FORMAT_UNKNOWN;result.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;
    result.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;result.Buffer.FirstElement=version.mapping.privateFirstElement;
    result.Buffer.NumElements=UINT(Elements);result.Buffer.StructureByteStride=UINT(Stride);return result;
}
bool Completion(const detail::Version& version,ID3D12Device* device,bool requireDone){
    const auto& completion=version.completion;
    if(completion.scope!=version.scope || !completion.finalized || !completion.submissionCertified || !completion.value || !completion.fence || !completion.queue ||
        !SameDevice(device,completion.fence.Get()) || !SameDevice(device,completion.queue.Get()) || completion.queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    if(!requireDone)return true;const auto done=completion.fence->GetCompletedValue();return done!=UINT64_MAX && done>=completion.value;
}
enum class Stage:uint32_t {Created,Recording,Recorded,Failed};
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
        if(!detail::ValidScope(input.scope) || input.privateB4Scope!=input.scope || input.completion.scope!=input.scope ||
            !seen.emplace(input.scope.root,input.scope.list,input.scope.generation,input.scope.occurrence,input.scope.eye).second)return false;
        failure=Failure::View;const auto& view=input.originalView;
        if(view.firstElement || view.numElements!=Elements || view.structureByteStride!=Stride || view.resourceWidth!=DomainBytes ||
            input.readPlan.view!=view)return false;
        failure=Failure::B4;
        if(!input.privateB4GpuMatchVerified || !input.allRecordPathsBounded || input.privateB4Bytes.size()!=multilayer_selectors::B4Bytes ||
            !input.privateB4Bytes.data() || !std::equal(input.privateB4Bytes.begin(),input.privateB4Bytes.end(),input.readPlan.b4Bytes.begin()))return false;
        multilayer_selectors::ReadPlan checked;std::string error;failure=Failure::ReadPlan;
        if(!input.readPlan.conservativeRecordSuperset || !input.readPlan.originalSiteCoverageComplete ||
            !multilayer_selectors::PlanRead(input.readPlan.family,{input.readPlan.b4Key,input.privateB4Bytes},view,checked,error) ||
            !SameReadPlan(input.readPlan,checked) || checked.records.empty() || checked.records.size()>multilayer_selectors::MaxRecords)return false;
        detail::Version version;version.scope=input.scope;version.dense=input.dense;version.copy=input.copy;version.completion=input.completion;
        version.readPlan=std::move(checked);std::copy(input.privateB4Bytes.begin(),input.privateB4Bytes.end(),version.b4.begin());
        failure=Failure::Owners;if(!DenseRange(version))return false;
        Microsoft::WRL::ComPtr<ID3D12Device> device;failure=Failure::Device;
        if(FAILED(version.dense.resource->GetDevice(IID_PPV_ARGS(&device))))return false;
        failure=Failure::Completion;if(!Completion(version,device.Get(),false))return false;
        failure=Failure::Capacity;
        if(!detail::Reserve(cursor,version.readPlan.firstRecord,version.readPlan.byteExtent,budget,version.mapping))return false;
        cursor=version.mapping.nextFree;pending->width=std::max(pending->width,version.mapping.viewEnd);
        pending->copied+=version.mapping.copyBytes;pending->versions.push_back(std::move(version));
    }
    output.data_=std::move(pending);failure=Failure::None;return true;
}catch(...){output={};failure=Failure::Exception;return false;}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
// The overlay's owner: rust/crates/cvr-render/src/structured_view_owner.rs (a COM object: its stage, device, buffer,
// plan and destination); these forwards keep the header's API.
extern "C" {
HRESULT cvr_sov2_create(ID3D12Device*,const void*,void**);
uint32_t cvr_sov2_adopt(ID3D12Device*,const void*,void**);
uint32_t cvr_sov2_record(void*,ID3D12GraphicsCommandList*,const DestinationRecording*);
bool cvr_sov2_view(void*,UINT,PrivateView*,ID3D12Resource**);
uint64_t cvr_sov2_count(void*,uint32_t);
bool cvr_sov2_submitted(void*,const DestinationRecording*,ID3D12CommandQueue*);
bool cvr_sov2_validate(const PrivateView*);
bool cvr_sov2_validate_recording(const PrivateView*,ID3D12GraphicsCommandList*,const DestinationRecording*);
}
static_assert(sizeof(DestinationRecording)==40,"mirrored by structured_view_overlay.rs");
struct Overlay::Storage:IUnknown {};   // the Rust owner's IUnknown face
Overlay::Overlay()=default;Overlay::~Overlay()=default;
bool Overlay::AdoptSources(ID3D12Device* device,std::span<const Input> inputs,Failure& failure)try{
    failure=Failure::Arguments;if(storage_ || !device)return false;
    Plan checked;if(!BuildPlan(inputs,checked,failure))return false;
    void* out{};failure=Failure(cvr_sov2_adopt(device,checked.data_->rust,&out));if(failure!=Failure::None)return false;
    storage_.Attach(static_cast<Storage*>(out));return true;
}catch(...){failure=Failure::Exception;return false;}
HRESULT Overlay::Create(ID3D12Device* device,const Plan& plan){
    if(storage_ || !device || !plan.data_)return E_INVALIDARG;
    void* out{};const auto result=cvr_sov2_create(device,plan.data_->rust,&out);
    if(SUCCEEDED(result))storage_.Attach(static_cast<Storage*>(out));return result;
}
bool Overlay::Record(ID3D12GraphicsCommandList* list,const DestinationRecording& destination,Failure& failure){
    failure=Failure::Arguments;if(!storage_ || !list)return false;
    failure=Failure(cvr_sov2_record(storage_.Get(),list,&destination));return failure==Failure::None;
}
bool Overlay::GetView(UINT index,PrivateView& output)const {
    output={};ID3D12Resource* resource{};if(!storage_ || !cvr_sov2_view(storage_.Get(),index,&output,&resource))return false;
    output.resource=resource;output.owner=storage_;return true;
}
uint64_t Overlay::RequiredBytes()const noexcept{return storage_?cvr_sov2_count(storage_.Get(),1):0;}
uint64_t Overlay::CopiedBytes()const noexcept{return storage_?cvr_sov2_count(storage_.Get(),2):0;}
bool Overlay::ValidateDestinationSubmission(const DestinationRecording& destination,ID3D12CommandQueue* queue)const {
    return storage_ && cvr_sov2_submitted(storage_.Get(),&destination,queue);
}
bool ValidatePrivateView(const PrivateView& view){return cvr_sov2_validate(&view);}
bool ValidatePrivateViewForRecording(const PrivateView& view,ID3D12GraphicsCommandList* list,const DestinationRecording& destination){
    return cvr_sov2_validate_recording(&view,list,&destination);
}
#endif
#else
struct Overlay::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};std::atomic<Stage> stage{Stage::Created};
    bool adopted{};
    Microsoft::WRL::ComPtr<ID3D12Device> device;Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    std::shared_ptr<const detail::PlanData> plan;DestinationRecording destination;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** value)override{if(!value)return E_POINTER;*value=nullptr;
        if(id!=__uuidof(IUnknown) && id!=ownerType)return E_NOINTERFACE;*value=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count)delete this;return count;}
};
Overlay::Overlay()=default;Overlay::~Overlay()=default;
bool Overlay::AdoptSources(ID3D12Device* device,std::span<const Input> inputs,Failure& failure)try{
    failure=Failure::Arguments;if(storage_ || !device)return false;
    Plan checked;if(!BuildPlan(inputs,checked,failure))return false;
    auto plan=std::make_shared<detail::PlanData>(*checked.data_);plan->width=0;
    struct Backing {Scope scope;uint64_t end{},width{};};std::map<ID3D12Resource*,Backing> backings;
    for(auto& version:plan->versions){
        failure=Failure::Completion;if(!Completion(version,device,true))return false;
        failure=Failure::Device;if(!SameDevice(device,version.dense.resource.Get()))return false;
        auto& backing=backings[version.dense.resource.Get()];const auto& dense=version.dense;
        failure=Failure::Scope;
        if(backing.scope.list && (backing.scope.list!=version.scope.list || backing.scope.generation!=version.scope.generation ||
            backing.scope.eye!=version.scope.eye))return false;
        backing.scope=version.scope;failure=Failure::Range;
        if(dense.privateOffset<dense.sourceOffset || dense.privateOffset<backing.end)return false;
        const auto first=dense.privateOffset-dense.sourceOffset,width=dense.resource->GetDesc().Width;
        if(first%Stride || first>width || DomainBytes>width-first || width>MaxBytes)return false;
        version.mapping={dense.privateOffset,first/Stride,dense.copiedBytes,dense.privateOffset+dense.copiedBytes,first+DomainBytes};
        backing.end=version.mapping.nextFree;backing.width=std::max(backing.width,version.mapping.viewEnd);
    }
    for(const auto& [resource,backing]:backings)plan->width+=backing.width;
    Microsoft::WRL::ComPtr<Storage> next;next.Attach(new Storage);next->device=device;next->plan=std::move(plan);next->adopted=true;
    next->stage.store(Stage::Recorded,std::memory_order_release);storage_=std::move(next);failure=Failure::None;return true;
}catch(...){failure=Failure::Exception;return false;}
HRESULT Overlay::Create(ID3D12Device* device,const Plan& plan)try{
    if(storage_ || !device || !plan.data_ || plan.data_->versions.empty() || !plan.data_->width || plan.data_->width>MaxBytes || device->GetNodeCount()!=1)return E_INVALIDARG;
    for(const auto& version:plan.data_->versions)if(!DenseRange(version) || !SameDevice(device,version.dense.resource.Get()) || !Completion(version,device,false))return E_INVALIDARG;
    Microsoft::WRL::ComPtr<Storage> pending;pending.Attach(new Storage);pending->device=device;pending->plan=plan.data_;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=plan.data_->width;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;heap.CreationNodeMask=heap.VisibleNodeMask=1;
    HRESULT result;{probe::Scope internal;result=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&pending->resource));}
    if(FAILED(result))return result;storage_=std::move(pending);return S_OK;
}catch(...){return E_OUTOFMEMORY;}
bool Overlay::Record(ID3D12GraphicsCommandList* list,const DestinationRecording& destination,Failure& failure)try{
    failure=Failure::Arguments;if(!storage_ || !list)return false;
    failure=Failure::Recording;
    if(!destination.certifiedCurrentRecording || !destination.generation || destination.list!=reinterpret_cast<uintptr_t>(list) || !destination.allocator || !destination.queue ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || destination.queue->GetDesc().Type!=D3D12_COMMAND_LIST_TYPE_DIRECT ||
        !SameDevice(storage_->device.Get(),list) || !SameDevice(storage_->device.Get(),destination.allocator.Get()) || !SameDevice(storage_->device.Get(),destination.queue.Get()))return false;
    failure=Failure::AlreadyRecorded;if(storage_->stage.load(std::memory_order_acquire)!=Stage::Created)return false;
    for(const auto& version:storage_->plan->versions){failure=Failure::Changed;
        if(version.dense.resource==storage_->resource || !DenseRange(version))return false;
        failure=Failure::Completion;if(!Completion(version,storage_->device.Get(),true))return false;
    }
    failure=Failure::Retention;if(!gpu::KeepCommandResources(list,{storage_.Get(),destination.allocator.Get(),destination.queue.Get()}))return false;
    auto expected=Stage::Created;failure=Failure::AlreadyRecorded;
    if(!storage_->stage.compare_exchange_strong(expected,Stage::Recording,std::memory_order_acq_rel))return false;
    storage_->destination=destination;
    {probe::Scope internal;for(const auto& version:storage_->plan->versions)list->CopyBufferRegion(storage_->resource.Get(),version.mapping.targetOffset,
        version.dense.resource.Get(),version.dense.privateOffset,version.mapping.copyBytes);
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={storage_->resource.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_COPY_SOURCE)};list->ResourceBarrier(1,&barrier);}
    storage_->stage.store(Stage::Recorded,std::memory_order_release);failure=Failure::None;return true;
}catch(...){if(storage_ && storage_->stage.load()==Stage::Recording)storage_->stage.store(Stage::Failed,std::memory_order_release);
    failure=Failure::Exception;return false;}
bool Overlay::GetView(UINT index,PrivateView& output)const {
    output={};if(!storage_ || storage_->stage.load(std::memory_order_acquire)!=Stage::Recorded || index>=storage_->plan->versions.size())return false;
    const auto& version=storage_->plan->versions[index];if(version.readPlan.records.size()>multilayer_selectors::MaxRecords)return false;
    output.scope=version.scope;output.resource=storage_->adopted?version.dense.resource:storage_->resource;output.owner=storage_;
    output.srv=View(version);output.sourceOffset=version.dense.sourceOffset;output.denseOffset=version.dense.privateOffset;
    output.targetOffset=version.mapping.targetOffset;output.copiedBytes=version.mapping.copyBytes;output.version=index;
    output.copyList=storage_->adopted?version.scope.list:storage_->destination.list;
    output.copyGeneration=storage_->adopted?version.scope.generation:storage_->destination.generation;
    output.firstRecord=version.readPlan.firstRecord;output.recordSpan=version.readPlan.recordSpan;output.recordCount=UINT(version.readPlan.records.size());
    std::copy(version.readPlan.records.begin(),version.readPlan.records.end(),output.records.begin());output.privateB4Bytes=version.b4;return true;
}
UINT Overlay::VersionCount()const noexcept{return storage_?UINT(storage_->plan->versions.size()):0;}
uint64_t Overlay::RequiredBytes()const noexcept{return storage_?storage_->plan->width:0;}
uint64_t Overlay::CopiedBytes()const noexcept{return storage_?storage_->plan->copied:0;}
bool Overlay::ValidateDestinationSubmission(const DestinationRecording& destination,ID3D12CommandQueue* queue)const {
    if(!storage_ || storage_->adopted || storage_->stage.load(std::memory_order_acquire)!=Stage::Recorded)return false;const auto& actual=storage_->destination;
    return destination.certifiedCurrentRecording && destination.list==actual.list && destination.generation==actual.generation && destination.allocator==actual.allocator &&
        destination.queue==actual.queue && queue==actual.queue.Get() && SameDevice(storage_->device.Get(),queue);
}
bool ValidatePrivateView(const PrivateView& view){
    Microsoft::WRL::ComPtr<Overlay::Storage> storage;
    if(!view.owner || FAILED(view.owner->QueryInterface(ownerType,reinterpret_cast<void**>(storage.GetAddressOf()))) ||
        storage->stage.load(std::memory_order_acquire)!=Stage::Recorded || view.version>=storage->plan->versions.size())return false;
    const auto& version=storage->plan->versions[view.version];const auto expected=View(version);
    if(view.resource!=(storage->adopted?version.dense.resource:storage->resource))return false;
    if(version.readPlan.records.size()>multilayer_selectors::MaxRecords)return false;
    std::array<uint32_t,multilayer_selectors::MaxRecords> records{};std::copy(version.readPlan.records.begin(),version.readPlan.records.end(),records.begin());
    return view.scope==version.scope && detail::SameBufferView(view.srv,expected) && view.sourceOffset==version.dense.sourceOffset &&
        view.denseOffset==version.dense.privateOffset && view.targetOffset==version.mapping.targetOffset && view.copiedBytes==version.mapping.copyBytes &&
        view.firstRecord==version.readPlan.firstRecord && view.recordSpan==version.readPlan.recordSpan && view.recordCount==version.readPlan.records.size() &&
        view.records==records && view.privateB4Bytes==version.b4 && view.copyList==(storage->adopted?version.scope.list:storage->destination.list) &&
        view.copyGeneration==(storage->adopted?version.scope.generation:storage->destination.generation);
}
bool ValidatePrivateViewForRecording(const PrivateView& view,ID3D12GraphicsCommandList* list,const DestinationRecording& destination){
    Microsoft::WRL::ComPtr<Overlay::Storage> storage;
    if(!ValidatePrivateView(view) || !list || view.copyList!=reinterpret_cast<uintptr_t>(list) || destination.list!=view.copyList ||
        destination.generation!=view.copyGeneration || !destination.certifiedCurrentRecording ||
        FAILED(view.owner->QueryInterface(ownerType,reinterpret_cast<void**>(storage.GetAddressOf()))))return false;
    return !storage->adopted && destination.allocator==storage->destination.allocator && destination.queue==storage->destination.queue &&
        gpu::HasCommandResource(list,storage.Get()) && SameDevice(storage->device.Get(),list);
}
#endif
}
