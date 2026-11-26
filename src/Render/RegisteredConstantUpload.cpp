#include "Render/RegisteredConstantUpload.hpp"
#include "Render/ConstantUploadRegistry.hpp"
#include "Render/NativeConstantCapture.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/ConstantTrace.hpp"
#include "Render/SourceResourceIntent.hpp"
#include <map>
#include <mutex>
#include <cstring>
#include <cstddef>
#include <algorithm>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The exported state is Rust's (rust/crates/cvr-render/src/native_state_exports.rs), declared in the header.
// The creation receipts (rust/crates/cvr-render/src/creation_receipts.rs) and the copy ledger
// (rust/crates/cvr-render/src/registered_copies.rs); tools/single_pass_tests/resource_creation_info_test and
// constant_capture_test run their cases against them in their Rust variants.
extern "C" {
void cvr_crc_created(ID3D12Resource*,bool,ID3D12Heap*,uint64_t,uint64_t,bool,uint32_t);
bool cvr_crc_query(ID3D12Resource*,uint32_t*,bool*,uint32_t*,void**,uint64_t*,uint64_t*);
extern std::atomic<bool> cvr_rc_active;
void cvr_rc_trace(uint64_t,const uint64_t*);
void cvr_rc_resolve(const void*,uint64_t,void*,void*,uint64_t,uint8_t*,size_t,void*);
bool cvr_rc_capture_intent(uint64_t,void*,void*,uint64_t,uint8_t*,size_t,uint64_t*,uint64_t*,void*);
void cvr_rc_freeze();
void cvr_rc_refresh_fences();
void cvr_rc_clear(const void*,bool);
// The entry points (registered_copies.rs' entries): the host this file lends them and the intent switch.
void cvr_rc2_attach(const void*);
bool cvr_rc2_intent_configured();
bool cvr_rc2_intent_observed_take();
void cvr_rc2_clear_registered(bool);
}
namespace cvr::stereo::constants {
using Microsoft::WRL::ComPtr;
namespace {
bool IntentConfigured(){return cvr_rc2_intent_configured();}
// The exported counters the ledger updates, lent per call (registered_copies.rs Counters).
struct RustCounters {
    std::atomic<uint32_t>* count,*rejected,*changed,*unscoped,*pendingReads,*orderRejected;std::atomic<uint64_t>* reads;
    std::atomic<uint32_t>* timelineResults,*timelineStats,*signals,*waits,*cpuSignals,*traceSeq,*traceCount,*traceOverflow;
    uint64_t* traceData;std::atomic<uint32_t>* registeredTypes;
};
static_assert(sizeof(RustCounters)==136 && sizeof(ConstantWriteTimeline::Result)==32 && sizeof(ConstantWriteTimeline::Position)==16,
    "mirrored by registered_copies.rs");
const RustCounters counters{&CyberpunkVR_RegisteredCopyCount,&CyberpunkVR_RegisteredCopyRejected,&CyberpunkVR_RegisteredCopyChanged,
    &CyberpunkVR_RegisteredCopyUnscoped,&CyberpunkVR_RegisteredCopyPendingReads,&CyberpunkVR_RegisteredCopyOrderRejected,
    &CyberpunkVR_RegisteredCopyReads,CyberpunkVR_RegisteredCopyTimelineResults,CyberpunkVR_RegisteredCopyTimelineStats,
    &CyberpunkVR_RegisteredCopySignals,&CyberpunkVR_RegisteredCopyWaits,&CyberpunkVR_RegisteredCopyCpuSignals,
    &CyberpunkVR_RegisteredCopyTraceSeq,&CyberpunkVR_RegisteredCopyTraceCount,&CyberpunkVR_RegisteredCopyTraceOverflow,
    &CyberpunkVR_RegisteredCopyTraceData,CyberpunkVR_ConstantRegisteredTypes};
// Queue Signal can run while the command resources hold their own mutex: the ledger calls this before taking its own.
bool Keep(void* list,void* source,void* destination)noexcept{
    try{return gpu::KeepCommandResources(static_cast<ID3D12GraphicsCommandList*>(list),
        {static_cast<ID3D12Resource*>(source),static_cast<ID3D12Resource*>(destination)});}catch(...){return false;}
}
// What the entries take from the program: the counters and switches, the command resources' keep, the owned uploads'
// read, the registered uploads' source and the source intent (registered_copies.rs' Host).
struct RustEntriesHost {
    const RustCounters* counters;const std::atomic<uint32_t>* captureEnable;const std::atomic<uint32_t>* intentEnable;
    bool(*keep)(void*,void*,void*)noexcept;bool(*readUpload)(uint64_t,void*,size_t);ID3D12Resource*(*registeredSource)(uint32_t,uint32_t);
    bool(*intentCopy)(ID3D12GraphicsCommandList*,ID3D12Resource*,uint64_t,ID3D12Resource*,uint64_t,uint64_t,const std::atomic<uint32_t>*);
    void(*intentInvalidate)(ID3D12Resource*);void(*intentClear)();
};
const RustEntriesHost entriesHost{&counters,&CyberpunkVR_ConstantCaptureEnable,&CyberpunkVR_RegisteredIntentEnable,&Keep,
    [](uint64_t address,void* output,size_t bytes){return ReadUpload(address,output,bytes);},
    [](uint32_t handle,uint32_t size){return RegisteredSource(handle,size);},
    [](ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,uint64_t bytes,
        const std::atomic<uint32_t>* enabled){return source_resource_intent::ObserveCopy(list,resource,offset,source,sourceOffset,bytes,enabled);},
    [](ID3D12Resource* resource){source_resource_intent::Invalidate(resource);},[]{source_resource_intent::Clear();}};
[[maybe_unused]] const bool entriesAttached=(cvr_rc2_attach(&entriesHost),true);
}
void RegisteredResourceCreated(ID3D12Resource* resource,bool placed,ID3D12Heap* heap,uint64_t offset,uint64_t allocationBytes,
                               std::optional<D3D12_RESOURCE_STATES> initialState) {
    if(probe::internalCommands || !native_probe::ObserveBindings() || !families::Enabled() || !resource)return;
    cvr_crc_created(resource,placed,heap,offset,allocationBytes,initialState.has_value(),initialState?UINT(*initialState):0);
}
bool QueryResourceCreation(const ComPtr<ID3D12Resource>& heldResource,ResourceCreationInfo& output) {
    // The held resource may be the output's own (an aliased query): keep it before clearing the output.
    ComPtr<ID3D12Resource> resource=heldResource;output={};
    uint32_t kind{},state{};bool known{};void* heap{};uint64_t offset{},bytes{};
    if(!cvr_crc_query(resource.Get(),&kind,&known,&state,&heap,&offset,&bytes))return false;
    ResourceCreationInfo result;result.kind=kind;result.initialStateKnown=known;
    if(known)result.initialState=D3D12_RESOURCE_STATES(state);
    result.resource=resource;result.heap.Attach(static_cast<ID3D12Heap*>(heap));result.heapOffset=offset;result.allocationBytes=bytes;
    output=std::move(result);return true;
}
void RegisteredReadDraw(uint64_t position,uint32_t sequence,uint64_t family,uint32_t side,uint32_t frame) {
    const uint64_t args[5]{position,sequence,family,side,frame};cvr_rc_trace(uint64_t(TraceOp::Draw),args);
}
void RegisteredReadBinding(uint64_t position,uint32_t visibility,uint32_t shaderRegister,uint64_t address,uint32_t bytes) {
    const uint64_t args[5]{position,visibility,shaderRegister,address,bytes};cvr_rc_trace(uint64_t(TraceOp::Binding),args);
}
ConstantWriteTimeline::Result ResolveRegisteredResourceRead(uint64_t position,ID3D12CommandList* list,
    const ComPtr<ID3D12Resource>& expectedDestination,uint64_t address,void* output,size_t bytes){
    if(!expectedDestination)return {ConstantWriteTimeline::Status::UnknownWrite};
    ConstantWriteTimeline::Result result;
    cvr_rc_resolve(&counters,position,list,expectedDestination.Get(),address,static_cast<uint8_t*>(output),bytes,&result);return result;
}
struct RegisteredResourceIntent::Data {
    ConstantWriteTimeline::Position producer;ComPtr<ID3D12CommandList> consumer;ComPtr<ID3D12Resource> destination;
    uint64_t position{},payload{},offset{},address{};std::vector<uint8_t> bytes;
};
uint64_t RegisteredResourceIntent::Payload()const noexcept{return data_?data_->payload:0;}
uint64_t RegisteredResourceIntent::Offset()const noexcept{return data_?data_->offset:0;}
uint64_t RegisteredResourceIntent::ReadPosition()const noexcept{return data_?data_->position:0;}
ConstantWriteTimeline::Position RegisteredResourceIntent::ProducerPosition()const noexcept{return data_?data_->producer:ConstantWriteTimeline::Position{};}
std::span<const uint8_t> RegisteredResourceIntent::Bytes()const noexcept{return data_?std::span<const uint8_t>(data_->bytes):std::span<const uint8_t>{};}
RegisteredResourceIntent::operator bool()const noexcept{return bool(data_);}
bool CaptureRegisteredResourceIntent(uint64_t position,ID3D12CommandList* list,
    const ComPtr<ID3D12Resource>& destination,uint64_t address,size_t bytes,RegisteredResourceIntent& output)try{
    if(output || !position || !list || !destination || !address || !bytes || bytes>65536 || address>UINT64_MAX-bytes)return false;
    auto data=std::make_shared<RegisteredResourceIntent::Data>();data->bytes.resize(bytes);
    if(!cvr_rc_capture_intent(position,list,destination.Get(),address,data->bytes.data(),bytes,&data->payload,&data->offset,&data->producer))return false;
    data->consumer=list;data->destination=destination;data->position=position;data->address=address;output.data_=std::move(data);return true;
}catch(...){return false;}
bool ValidateRegisteredResourceIntent(const RegisteredResourceIntent& intent,ConstantWriteTimeline::Result* resolution)try{
    if(resolution)*resolution={};if(!intent.data_)return false;const auto& data=*intent.data_;
    std::vector<uint8_t> actual(data.bytes.size());const auto result=ResolveRegisteredResourceRead(data.position,data.consumer.Get(),
        data.destination,data.address,actual.data(),actual.size());if(resolution)*resolution=result;
    return result.status==ConstantWriteTimeline::Status::Ready && result.payload==data.payload && result.offset==data.offset && actual==data.bytes;
}catch(...){return false;}
void FreezeRegistered(){cvr_rc_freeze();}
bool RegisteredIntentTracking(){return IntentConfigured();}
}
namespace cvr::stereo::constants {
void PollRegisteredIntent(){
    static bool enabled{};const bool requested=IntentConfigured();
    const bool cleanup=!requested && cvr_rc2_intent_observed_take();
    if(cleanup){source_resource_intent::Clear();if(!CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed))ClearRegistered();}
    if(enabled || requested || cleanup){const auto stats=source_resource_intent::GetStatistics();
        const uint64_t values[]{stats.currentBytes,stats.rangeCount,stats.livePayloadBytes,stats.livePayloads,stats.ownerCount,
            stats.copySequence,stats.evictions,stats.captures,stats.rejected,uint64_t(requested)};
        std::copy(std::begin(values),std::end(values),CyberpunkVR_RegisteredIntentCacheStats);
        CyberpunkVR_RegisteredStructuredIntentStats[0]=stats.structuredBytes;CyberpunkVR_RegisteredStructuredIntentStats[1]=stats.structuredRanges;
    }
    enabled=requested;
}
void RefreshRegisteredFences(){cvr_rc_refresh_fences();}
void ClearRegistered(bool publish){cvr_rc2_clear_registered(publish);}   // over the entries' counters
}
extern "C" {
bool cvr_rc2_scope_enter(ID3D12Resource*,uint32_t,const void*);
bool cvr_rc2_scope_enter_handle(uint32_t,uint32_t,const void*);
void cvr_rc2_scope_leave();
void cvr_rc2_observe_copy(ID3D12GraphicsCommandList*,ID3D12Resource*,uint64_t,ID3D12Resource*,uint64_t,uint64_t);
void cvr_rc2_observe_resource_copy(ID3D12GraphicsCommandList*,ID3D12Resource*,ID3D12Resource*);
void cvr_rc2_invalidate(ID3D12GraphicsCommandList*,ID3D12Resource*);
void cvr_rc2_barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void cvr_rc2_reset(ID3D12GraphicsCommandList*);
void cvr_rc2_submit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
bool cvr_rc2_read(ID3D12CommandList*,uint64_t,void*,size_t,uint64_t*);
bool cvr_rc2_validate(uint64_t,ID3D12CommandList*,uint64_t,const void*,size_t);
uint64_t cvr_rc2_mark_read(ID3D12CommandList*);
cvr::stereo::constants::ConstantWriteTimeline::Result cvr_rc2_resolve(uint64_t,ID3D12CommandList*,uint64_t,void*,size_t);
bool cvr_rc2_tracking();
void cvr_rc2_register_fence(ID3D12Fence*);
void cvr_rc2_signal(ID3D12CommandQueue*,ID3D12Fence*,uint64_t);
void cvr_rc2_wait(ID3D12CommandQueue*,ID3D12Fence*,uint64_t);
void cvr_rc2_cpu_signal(ID3D12Fence*,uint64_t,uint64_t,uint32_t);
}
namespace cvr::stereo::constants {
struct RegisteredUploadScope::Data {};
RegisteredUploadScope::RegisteredUploadScope(uint32_t handle,uint32_t bytes,const void* source){
    if(cvr_rc2_scope_enter_handle(handle,bytes,source))data_=std::make_unique<Data>();}
RegisteredUploadScope::RegisteredUploadScope(ID3D12Resource* destination,uint32_t size,const void* source){
    if(cvr_rc2_scope_enter(destination,size,source))data_=std::make_unique<Data>();}
RegisteredUploadScope::~RegisteredUploadScope(){if(data_)cvr_rc2_scope_leave();}
void ObserveRegisteredCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,uint64_t size){
    cvr_rc2_observe_copy(list,destination,offset,source,sourceOffset,size);}
void ObserveRegisteredResourceCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,ID3D12Resource* source){
    cvr_rc2_observe_resource_copy(list,destination,source);}
void InvalidateRegistered(ID3D12GraphicsCommandList* list,ID3D12Resource* destination){cvr_rc2_invalidate(list,destination);}
void RegisteredBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){cvr_rc2_barriers(list,count,barriers);}
void ResetRegistered(ID3D12GraphicsCommandList* list){cvr_rc2_reset(list);}
void SubmitRegistered(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){cvr_rc2_submit(queue,count,lists);}
bool ReadRegistered(ID3D12CommandList* list,uint64_t address,void* output,size_t bytes,uint64_t* receipt){
    return cvr_rc2_read(list,address,output,bytes,receipt);}
bool ValidateRegistered(uint64_t receipt,ID3D12CommandList* list,uint64_t address,const void* expectedBytes,size_t bytes){
    return cvr_rc2_validate(receipt,list,address,expectedBytes,bytes);}
uint64_t MarkRegisteredRead(ID3D12CommandList* list){return cvr_rc2_mark_read(list);}
ConstantWriteTimeline::Result ResolveRegisteredRead(uint64_t position,ID3D12CommandList* list,uint64_t address,void* output,size_t bytes){
    return cvr_rc2_resolve(position,list,address,output,bytes);}
bool RegisteredTracking(){return cvr_rc2_tracking();}
void RegisterFence(ID3D12Fence* fence){cvr_rc2_register_fence(fence);}
void RegisteredSignal(ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t value){cvr_rc2_signal(queue,fence,value);}
void RegisteredWait(ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t value){cvr_rc2_wait(queue,fence,value);}
void RegisteredCpuSignal(ID3D12Fence* fence,uint64_t value,uint64_t caller,uint32_t nested){cvr_rc2_cpu_signal(fence,value,caller,nested);}
}
#endif
#else
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredIntentEnable{};
__declspec(dllexport) uint64_t CyberpunkVR_RegisteredIntentCacheStats[10]{};
__declspec(dllexport) uint64_t CyberpunkVR_RegisteredStructuredIntentStats[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyCount{},CyberpunkVR_RegisteredCopyRejected{},CyberpunkVR_RegisteredCopyChanged{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyUnscoped{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyPendingReads{},CyberpunkVR_RegisteredCopyOrderRejected{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_RegisteredCopyReads{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyTimelineResults[8]{},CyberpunkVR_RegisteredCopyTimelineStats[5]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopySignals{},CyberpunkVR_RegisteredCopyWaits{},CyberpunkVR_RegisteredCopyCpuSignals{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RegisteredCopyTraceSeq{},CyberpunkVR_RegisteredCopyTraceCount{},CyberpunkVR_RegisteredCopyTraceOverflow{};
__declspec(dllexport) uint64_t CyberpunkVR_RegisteredCopyTraceData{};
}
namespace cvr::stereo::constants {
using Microsoft::WRL::ComPtr;
namespace {
std::atomic<bool> intentObserved{};
bool IntentConfigured() {
    static const bool configured=[] {
        if(GetEnvironmentVariableW(L"CVR_STEREO_SOURCE_SELECTORS",nullptr,0) &&
            GetEnvironmentVariableW(L"CVR_STEREO_FAMILY_MANIFEST",nullptr,0))CyberpunkVR_RegisteredIntentEnable=1;
        return true;
    }();
    (void)configured;
    const bool requested=CyberpunkVR_RegisteredIntentEnable.load(std::memory_order_acquire)!=0;
    if(requested)intentObserved=true;
    return requested;
}
struct Expected {ID3D12Resource* destination{};std::vector<uint8_t> bytes;};
thread_local Expected* expected{};
struct Submission {ComPtr<ID3D12CommandQueue> queue;uint64_t batch{};UINT position{};};
struct Copy {
    ComPtr<ID3D12Resource> destination,source;
    ID3D12CommandList* list{};
    uint64_t id{},address{},sourceOffset{};
    std::vector<uint8_t> bytes;
    bool submitted{},invalid{},committed{};
    Submission submission;
    uint32_t write{};
    ConstantWriteTimeline::Position point;
    ConstantWriteTimeline::MemorySpan memory;
};
constexpr GUID placementKey{0x4d3815bb,0x3885,0x463d,{0x81,0xc3,0x48,0x32,0x2a,0xcb,0x62,0x51}};
constexpr GUID heapOwnerKey{0x4d3815bb,0x3885,0x463d,{0x81,0xc3,0x48,0x32,0x2a,0xcb,0x62,0x52}};
constexpr GUID creationKey{0x4d3815bb,0x3885,0x463d,{0x81,0xc3,0x48,0x32,0x2a,0xcb,0x62,0x53}};
struct Placement {ConstantWriteTimeline::MemorySpan memory;uint32_t kind{};};
struct CreationReceipt {
    uint64_t resource{},heap{},offset{},bytes{};
    uint32_t kind{},stateKnown{},state{},version{};
    uint64_t checksum{};
};
static_assert(sizeof(CreationReceipt)==56 && offsetof(CreationReceipt,checksum)==48);
constexpr uint32_t CreationVersion=0x43524931;
uint64_t CreationChecksum(const CreationReceipt& receipt) {
    uint64_t result=14695981039346656037ull;
    const auto* bytes=reinterpret_cast<const uint8_t*>(&receipt);
    for(size_t n=0;n<offsetof(CreationReceipt,checksum);++n){result^=bytes[n];result*=1099511628211ull;}
    return result;
}
bool ReadCreation(ID3D12Resource* resource,CreationReceipt& receipt) {
    UINT bytes=sizeof(receipt);receipt={};
    return resource && SUCCEEDED(resource->GetPrivateData(creationKey,&bytes,&receipt)) && bytes==sizeof(receipt) &&
        receipt.version==CreationVersion && receipt.checksum==CreationChecksum(receipt);
}
bool SameCreation(const CreationReceipt& a,const CreationReceipt& b) {
    return a.resource==b.resource && a.heap==b.heap && a.offset==b.offset && a.bytes==b.bytes &&
        a.kind==b.kind && a.stateKnown==b.stateKnown && a.state==b.state && a.version==b.version && a.checksum==b.checksum;
}
Placement PlacementOf(ID3D12Resource* resource) {
    Placement value{};UINT bytes=sizeof(value);
    if(!resource || FAILED(resource->GetPrivateData(placementKey,&bytes,&value)) || bytes!=sizeof(value))return {};
    return value;
}
std::map<uint64_t,std::shared_ptr<Copy>> copies,history;
std::map<ID3D12CommandList*,Submission> submissions;
std::mutex mutex;
std::atomic<bool> haveCopies{};
constexpr size_t Limit=16*1024*1024;
size_t ownedBytes{};
uint64_t nextReceipt{};
uint64_t nextBatch{};
ConstantWriteTimeline timeline;
struct ReadPosition {ID3D12CommandList* list{};ConstantWriteTimeline::Position point;};
std::map<uint64_t,ReadPosition> readPositions;
std::map<ID3D12Fence*,ComPtr<ID3D12Fence>> fences;
std::map<ID3D12CommandQueue*,ComPtr<ID3D12CommandQueue>> queueOwners;
uint64_t nextReadPosition{};
bool frozen{},timelineFailed{};
std::vector<TraceEvent> trace,publishedTrace;
bool traceOverflow{};
void Trace(TraceOp op,std::array<uint64_t,7> args={}) noexcept {
    if(trace.size()>=TraceCapacity){traceOverflow=true;return;}
    try {trace.push_back({op,args});}catch(...){traceOverflow=true;}
}
ConstantWriteTimeline::Position Mark(ID3D12CommandList* list) {
    const auto point=timeline.Mark(reinterpret_cast<uintptr_t>(list));
    Trace(TraceOp::Mark,{reinterpret_cast<uintptr_t>(list),point.recording,point.command});return point;
}
uint32_t RecordWrite(ID3D12CommandList* list,uint64_t address,uint64_t size,bool committed,ConstantWriteTimeline::MemorySpan memory={},
    ConstantWriteTimeline::Position* position=nullptr) {
    const auto point=Mark(list);const auto id=timeline.Write(point,address,size,committed);
    if(position)*position=point;
    Trace(TraceOp::Write,{id,point.recording,point.command,address,size,committed});
    timeline.Memory(id,memory);Trace(TraceOp::Memory,{id,memory.heap,memory.offset,memory.bytes});return id;
}
void Explain(uint32_t write,ConstantWriteTimeline::Detail detail) {
    timeline.Explanation(write,detail);Trace(TraceOp::Explanation,{write,uint64_t(detail)});
}
bool ReadSource(ID3D12Resource* source,uint64_t offset,void* data,size_t bytes) {
    const auto d=source->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(d.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || offset>d.Width || bytes>d.Width-offset ||
       FAILED(source->GetHeapProperties(&heap,&flags)) || (heap.Type!=D3D12_HEAP_TYPE_UPLOAD && heap.Type!=D3D12_HEAP_TYPE_GPU_UPLOAD))return false;
    void* mapped{};const D3D12_RANGE range{SIZE_T(offset),SIZE_T(offset+bytes)};if(FAILED(source->Map(0,&range,&mapped)))return false;
    SIZE_T read{};const bool ok=mapped && ReadProcessMemory(GetCurrentProcess(),static_cast<const uint8_t*>(mapped)+offset,data,bytes,&read) && read==bytes;
    const D3D12_RANGE none{};source->Unmap(0,&none);return ok;
}
void InvalidateRange(uint64_t address,uint64_t size) {
    if(!size)return;
    for(auto it=copies.begin();it!=copies.end();) {
        if(address<=UINT64_MAX-size && address<it->first+it->second->bytes.size() && it->first<address+size) {
            it=copies.erase(it);
        } else ++it;
    }
}
}
struct RegisteredUploadScope::Data {Expected current;Expected* previous{};};
RegisteredUploadScope::RegisteredUploadScope(uint32_t handle,uint32_t bytes,const void* source)
    :RegisteredUploadScope(CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed)?RegisteredSource(handle,bytes):nullptr,bytes,source){}
RegisteredUploadScope::RegisteredUploadScope(ID3D12Resource* destination,uint32_t size,const void* source) {
    if(!CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed) || probe::internalCommands || !destination || !source || !size || size>65536)return;
    try {
        auto pending=std::make_unique<Data>();pending->current.destination=destination;pending->current.bytes.resize(size);
        SIZE_T copied{};if(!ReadProcessMemory(GetCurrentProcess(),source,pending->current.bytes.data(),size,&copied) || copied!=size)return;
        pending->previous=expected;expected=&pending->current;data_=std::move(pending);
    }catch(...){++CyberpunkVR_ConstantRegisteredRejects;}
}
RegisteredUploadScope::~RegisteredUploadScope(){if(data_)expected=data_->previous;}
void RegisteredResourceCreated(ID3D12Resource* resource,bool placed,ID3D12Heap* heap,uint64_t offset,uint64_t allocationBytes,
                               std::optional<D3D12_RESOURCE_STATES> initialState) {
    if(probe::internalCommands || !native_probe::ObserveBindings() || !families::Enabled() || !resource)return;
    Placement value{};value.kind=placed?2:1;
    if(placed && heap && allocationBytes && allocationBytes!=UINT64_MAX && offset<=UINT64_MAX-allocationBytes &&
       SUCCEEDED(resource->SetPrivateDataInterface(heapOwnerKey,heap)))
        value.memory={reinterpret_cast<uintptr_t>(heap),offset,allocationBytes};
    resource->SetPrivateData(placementKey,sizeof(value),&value);
    CreationReceipt receipt;receipt.resource=reinterpret_cast<uintptr_t>(resource);receipt.kind=value.kind;
    receipt.heap=value.memory.heap;receipt.offset=value.memory.offset;receipt.bytes=value.memory.bytes;
    receipt.stateKnown=initialState.has_value();receipt.state=initialState?UINT(*initialState):0;receipt.version=CreationVersion;
    receipt.checksum=CreationChecksum(receipt);resource->SetPrivateData(creationKey,sizeof(receipt),&receipt);
}
bool QueryResourceCreation(const ComPtr<ID3D12Resource>& heldResource,ResourceCreationInfo& output) {
    ComPtr<ID3D12Resource> resource=heldResource;output={};CreationReceipt receipt;
    if(!ReadCreation(resource.Get(),receipt) || receipt.resource!=reinterpret_cast<uintptr_t>(resource.Get()) ||
        (receipt.kind!=1 && receipt.kind!=2) || receipt.stateKnown>1 || (!receipt.stateKnown && receipt.state))return false;
    const auto placement=PlacementOf(resource.Get());
    if(placement.kind!=receipt.kind || placement.memory.heap!=receipt.heap || placement.memory.offset!=receipt.offset || placement.memory.bytes!=receipt.bytes)return false;
    ComPtr<ID3D12Device> resourceDevice;
    if(FAILED(resource->GetDevice(IID_PPV_ARGS(&resourceDevice))) || !resourceDevice)return false;
    ResourceCreationInfo result;result.kind=receipt.kind;result.initialStateKnown=receipt.stateKnown!=0;
    if(result.initialStateKnown)result.initialState=D3D12_RESOURCE_STATES(receipt.state);
    result.resource=resource;
    if(receipt.kind==1){if(receipt.heap || receipt.offset || receipt.bytes)return false;}
    else{
        if(!receipt.heap || !receipt.bytes || receipt.bytes==UINT64_MAX || receipt.offset>UINT64_MAX-receipt.bytes)return false;
        IUnknown* raw{};UINT bytes=sizeof(raw);
        if(FAILED(resource->GetPrivateData(heapOwnerKey,&bytes,&raw)) || bytes!=sizeof(raw) || !raw)return false;
        // SetPrivateDataInterface retrieval already AddRefs the returned object.
        ComPtr<IUnknown> owner;owner.Attach(raw);
        if(FAILED(owner.As(&result.heap)) || reinterpret_cast<uintptr_t>(result.heap.Get())!=receipt.heap)return false;
        ComPtr<ID3D12Device> heapDevice;
        if(FAILED(result.heap->GetDevice(IID_PPV_ARGS(&heapDevice))) ||
            resourceDevice.Get()!=heapDevice.Get())return false;
        const auto heap=result.heap->GetDesc();const auto desc=resource->GetDesc();
        const auto allocation=resourceDevice->GetResourceAllocationInfo(0,1,&desc);
        if(!heap.SizeInBytes || receipt.offset>heap.SizeInBytes || receipt.bytes>heap.SizeInBytes-receipt.offset ||
            !allocation.SizeInBytes || allocation.SizeInBytes==UINT64_MAX || !allocation.Alignment ||
            receipt.offset%allocation.Alignment || receipt.bytes<allocation.SizeInBytes)return false;
        result.heapOffset=receipt.offset;result.allocationBytes=receipt.bytes;
    }
    CreationReceipt current;
    if(!ReadCreation(resource.Get(),current) || !SameCreation(receipt,current))return false;
    const auto currentPlacement=PlacementOf(resource.Get());
    if(currentPlacement.kind!=receipt.kind || currentPlacement.memory.heap!=receipt.heap || currentPlacement.memory.offset!=receipt.offset ||
        currentPlacement.memory.bytes!=receipt.bytes)return false;
    output=std::move(result);return true;
}
void ObserveRegisteredCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,uint64_t size) {
    if(probe::internalCommands || !destination || !source)return;
    const bool legacy=CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed)!=0;
    if(IntentConfigured()){
        source_resource_intent::ObserveCopy(list,destination,offset,source,sourceOffset,size,&CyberpunkVR_RegisteredIntentEnable);
        if(!legacy)return;
    }
    if(!haveCopies.load(std::memory_order_relaxed) && !legacy)return;
    try {
        const auto desc=destination->GetDesc();if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER)return;
        const auto base=destination->GetGPUVirtualAddress();if(!base || offset>desc.Width || size>desc.Width-offset || base>UINT64_MAX-desc.Width)return;
        // Queue Signal can run while CommandResources holds its own mutex.
        // Never acquire that mutex from inside this ledger's critical section.
        const bool retained=legacy && size && size<=65536 &&
            gpu::KeepCommandResources(list,{source,destination});
        std::lock_guard lock(mutex);InvalidateRange(base+offset,size);
        if(frozen)return;
        const auto placement=PlacementOf(destination);const bool committed=placement.kind==1;
        ConstantWriteTimeline::Position point;const auto write=RecordWrite(list,base+offset,size,committed,placement.memory,&point);
        haveCopies=true;
        if(!legacy || !size)return;
        if(size>65536){Explain(write,ConstantWriteTimeline::Detail::CopyTooLarge);return;}
        D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        if(FAILED(destination->GetHeapProperties(&heap,&flags)))return;
        if(UINT(heap.Type)<6)++CyberpunkVR_ConstantRegisteredTypes[UINT(heap.Type)];
        if(heap.Type!=D3D12_HEAP_TYPE_DEFAULT || (desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)) {
            Explain(write,ConstantWriteTimeline::Detail::UnsupportedDestination);return;
        }
        // Other native uploaders may bypass BufUpload. The actual copy API is
        // authoritative for its source, destination and suballocated range.
        // This ledger describes recorded writes, not completed GPU execution.
        const bool scoped=expected && expected->destination==destination && !offset && size==expected->bytes.size();
        std::vector<uint8_t> actual(size),repeated(size);
        if(!ReadSource(source,sourceOffset,actual.data(),actual.size())){Explain(write,ConstantWriteTimeline::Detail::UnmappedSource);return;}
        if(!ReadSource(source,sourceOffset,repeated.data(),repeated.size()) || actual!=repeated || (scoped && actual!=expected->bytes) ||
           actual.size()>Limit-ownedBytes || history.size()>=8192 || !retained){Explain(write,ConstantWriteTimeline::Detail::UnstableSource);++CyberpunkVR_RegisteredCopyRejected;return;}
        auto copy=std::make_shared<Copy>();copy->id=++nextReceipt;copy->address=base+offset;copy->destination=destination;copy->source=source;
        copy->committed=committed;copy->write=write;copy->point=point;copy->memory=placement.memory;
        copy->sourceOffset=sourceOffset;copy->list=list;copy->bytes=std::move(actual);
        ownedBytes+=copy->bytes.size();timeline.Payload(write,copy->id);Trace(TraceOp::Payload,{write,copy->id});history.emplace(copy->id,copy);copies.emplace(base+offset,std::move(copy));haveCopies.store(true);++CyberpunkVR_RegisteredCopyCount;
        if(!scoped)++CyberpunkVR_RegisteredCopyUnscoped;
    }catch(...){std::lock_guard lock(mutex);timelineFailed=true;++CyberpunkVR_RegisteredCopyRejected;}
}
void ObserveRegisteredResourceCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* destination,ID3D12Resource* source){
    if(probe::internalCommands || !destination)return;
    const bool intent=IntentConfigured();
    if(!intent && !CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed) && !haveCopies.load(std::memory_order_relaxed))return;
    try{if(source){const auto bytes=CopyResourceBufferBytes(destination->GetDesc(),source->GetDesc());
            if(bytes){ObserveRegisteredCopy(list,destination,0,source,0,bytes);return;}}
    }catch(...){}
    InvalidateRegistered(list,destination);
}
void InvalidateRegistered(ID3D12GraphicsCommandList* list,ID3D12Resource* destination) {
    if(probe::internalCommands || !destination)return;
    if(IntentConfigured())source_resource_intent::Invalidate(destination);
    if(!haveCopies.load(std::memory_order_relaxed))return;
    const auto desc=destination->GetDesc();if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER)return;
    try {
        std::lock_guard lock(mutex);const auto address=destination->GetGPUVirtualAddress();InvalidateRange(address,desc.Width);
        if(!frozen){const auto p=PlacementOf(destination);RecordWrite(list,address,desc.Width,p.kind==1,p.memory);}
    }catch(...){std::lock_guard lock(mutex);timelineFailed=true;}
}
void RegisteredBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers) {
    if(probe::internalCommands || !barriers)return;
    if(IntentConfigured())for(UINT n=0;n<count;++n)if(barriers[n].Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){
        const auto& alias=barriers[n].Aliasing;
        if(alias.pResourceBefore)source_resource_intent::Invalidate(alias.pResourceBefore);
        if(alias.pResourceAfter)source_resource_intent::Invalidate(alias.pResourceAfter);
        if(!alias.pResourceBefore && !alias.pResourceAfter)source_resource_intent::Clear();
    }
    if(!haveCopies.load(std::memory_order_relaxed))return;
    try {
    for(UINT i=0;i<count;++i)if(barriers[i].Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING) {
        const auto& b=barriers[i].Aliasing;
        const auto before=PlacementOf(b.pResourceBefore),after=PlacementOf(b.pResourceAfter);
        const auto known=[](const Placement& p){return p.kind==1 || (p.kind==2 && p.memory.heap && p.memory.bytes);};
        std::array<Placement,2> areas{before,after};UINT used=2;
        if(!b.pResourceBefore){areas[0]=after;used=1;}
        if(!b.pResourceAfter){areas[0]=before;used=1;}
        if((b.pResourceBefore && !known(before)) || (b.pResourceAfter && !known(after)) || (!b.pResourceBefore && !b.pResourceAfter)) {
            areas[0]={};used=1;
        }
        std::lock_guard lock(mutex);
        for(UINT n=0;n<used;++n) {
            if(areas[n].kind==1)continue;
            const auto memory=areas[n].memory;
            if(!frozen){const auto point=Mark(list);timeline.Alias(point,memory);
                Trace(TraceOp::Alias,{point.recording,point.command,memory.heap,memory.offset,memory.bytes});}
            std::erase_if(copies,[&](const auto& entry){return !entry.second->committed && ConstantWriteTimeline::CouldAlias(entry.second->memory,memory);});
        }
    }
    }catch(...){std::lock_guard lock(mutex);timelineFailed=true;}
}
void ResetRegistered(ID3D12GraphicsCommandList* list) {
    if(!haveCopies.load(std::memory_order_relaxed))return;std::lock_guard lock(mutex);
    submissions.erase(list);
    timeline.Reset(reinterpret_cast<uintptr_t>(list));Trace(TraceOp::Reset,{reinterpret_cast<uintptr_t>(list)});
    for(auto& [_,copy]:history)if(copy->list==list && !copy->submitted)copy->invalid=true;
}
void SubmitRegistered(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    if(!haveCopies.load(std::memory_order_relaxed) || !queue || !lists)return;std::lock_guard lock(mutex);
    try {
    const auto batch=++nextBatch;
    std::vector<uint64_t> ids;ids.reserve(count);for(UINT i=0;i<count;++i)ids.push_back(reinterpret_cast<uintptr_t>(lists[i]));
    timeline.Submit(reinterpret_cast<uintptr_t>(queue),batch,ids);
    Trace(TraceOp::SubmitBegin,{reinterpret_cast<uintptr_t>(queue),batch,count});
    for(auto id:ids)Trace(TraceOp::SubmitList,{id});Trace(TraceOp::SubmitEnd);
    for(UINT i=0;i<count;++i)submissions[lists[i]]={queue,batch,i};
    for(auto& [_,entry]:history) {
        auto& copy=*entry;bool submitting=false;for(UINT i=0;i<count;++i)submitting|=copy.list==lists[i];
        if(copy.submitted || copy.invalid || !submitting)continue;
        std::vector<uint8_t> current(copy.bytes.size());
        if(!ReadSource(copy.source.Get(),copy.sourceOffset,current.data(),current.size()) || current!=copy.bytes){
            ++CyberpunkVR_RegisteredCopyChanged;copy.invalid=true;timeline.Payload(copy.write,0);Trace(TraceOp::Payload,{copy.write,0});
        } else {copy.submitted=true;copy.submission=submissions.at(copy.list);}
    }
    }catch(...){for(auto& [_,copy]:history)copy->invalid=true;copies.clear();timelineFailed=true;++CyberpunkVR_RegisteredCopyRejected;}
}
bool ReadRegistered(ID3D12CommandList* list,uint64_t address,void* output,size_t bytes,uint64_t* receipt) {
    if(receipt)*receipt=0;
    if(ReadUpload(address,output,bytes))return true;
    if(!output || !bytes || bytes>65536)return false;std::lock_guard lock(mutex);
    auto found=copies.upper_bound(address);if(found==copies.begin())return false;--found;
    const auto& copy=*found->second;const auto offset=address-found->first;
    if(copy.invalid || (!copy.submitted && copy.list!=list && (!receipt || !list)) || offset>copy.bytes.size() || bytes>copy.bytes.size()-offset)return false;
    if(!copy.submitted && copy.list!=list)++CyberpunkVR_RegisteredCopyPendingReads;
    if(receipt)*receipt=copy.id;
    std::memcpy(output,copy.bytes.data()+offset,bytes);++CyberpunkVR_RegisteredCopyReads;return true;
}
bool ValidateRegistered(uint64_t receipt,ID3D12CommandList* list,uint64_t address,const void* expectedBytes,size_t bytes) {
    if(!expectedBytes || !bytes)return false;std::lock_guard lock(mutex);const auto found=history.find(receipt);
    if(found==history.end())return false;const auto& copy=*found->second;
    if(copy.invalid || !copy.submitted || address<copy.address || address-copy.address>copy.bytes.size() || bytes>copy.bytes.size()-(address-copy.address))return false;
    const auto consumer=submissions.find(list);
    if(consumer==submissions.end() || consumer->second.queue!=copy.submission.queue ||
       consumer->second.batch<copy.submission.batch ||
       (consumer->second.batch==copy.submission.batch && consumer->second.position<copy.submission.position)) {
        ++CyberpunkVR_RegisteredCopyOrderRejected;return false;
    }
    return !std::memcmp(expectedBytes,copy.bytes.data()+(address-copy.address),bytes);
}
uint64_t MarkRegisteredRead(ID3D12CommandList* list) {
    if(!list || !CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed))return 0;
    std::lock_guard lock(mutex);
    try {
        if(frozen || readPositions.size()>=native_probe::DrawCapacity)return 0;
        const auto point=Mark(list);if(!point.recording)return 0;
        const auto id=++nextReadPosition;readPositions.emplace(id,ReadPosition{list,point});
        Trace(TraceOp::ReadPosition,{id,reinterpret_cast<uintptr_t>(list),point.recording,point.command});haveCopies=true;return id;
    }catch(...){timelineFailed=true;return 0;}
}
void RegisteredReadDraw(uint64_t position,uint32_t sequence,uint64_t family,uint32_t side,uint32_t frame) {
    std::lock_guard lock(mutex);Trace(TraceOp::Draw,{position,sequence,family,side,frame});
}
void RegisteredReadBinding(uint64_t position,uint32_t visibility,uint32_t shaderRegister,uint64_t address,uint32_t bytes) {
    std::lock_guard lock(mutex);Trace(TraceOp::Binding,{position,visibility,shaderRegister,address,bytes});
}
namespace {
ConstantWriteTimeline::Result ResolvePositionPayload(uint64_t position,ID3D12CommandList* list,ID3D12Resource* destination,
    uint64_t address,void* output,size_t bytes) {
    using Status=ConstantWriteTimeline::Status;
    std::lock_guard lock(mutex);
    const auto stats=timeline.Stats();for(UINT i=0;i<stats.size();++i)CyberpunkVR_RegisteredCopyTimelineStats[i]=stats[i];
    const auto resolve=[&]() -> ConstantWriteTimeline::Result {
        if(timelineFailed)return {Status::Overflow};
        const auto read=readPositions.find(position);
        if(!frozen || read==readPositions.end() || read->second.list!=list || !output || !bytes)return {};
        const auto result=timeline.Resolve(read->second.point,address,bytes);
        if(result.status!=Status::Pending)Trace(TraceOp::Resolve,{position,address,bytes,uint64_t(result.status),uint64_t(result.detail),result.payload,result.offset});
        if(result.status!=Status::Ready)return result;
        const auto found=history.find(result.payload);
        if(found==history.end())return {Status::UnknownWrite};
        const auto& copy=*found->second;
        if(destination && copy.destination.Get()!=destination)return {Status::UnknownWrite};
        if(copy.invalid || !copy.submitted || result.offset>copy.bytes.size() || bytes>copy.bytes.size()-result.offset)return {Status::UnknownWrite,0,0,ConstantWriteTimeline::Detail::UnstableSource};
        std::memcpy(output,copy.bytes.data()+result.offset,bytes);return result;
    };
    const auto result=resolve();++CyberpunkVR_RegisteredCopyTimelineResults[UINT(result.status)];
    if(result.status!=Status::Pending)Trace(TraceOp::FinalResult,{position,address,bytes,uint64_t(result.status),uint64_t(result.detail),result.payload,result.offset});
    return result;
}
}
ConstantWriteTimeline::Result ResolveRegisteredRead(uint64_t position,ID3D12CommandList* list,uint64_t address,void* output,size_t bytes){
    return ResolvePositionPayload(position,list,nullptr,address,output,bytes);
}
ConstantWriteTimeline::Result ResolveRegisteredResourceRead(uint64_t position,ID3D12CommandList* list,
    const ComPtr<ID3D12Resource>& expectedDestination,uint64_t address,void* output,size_t bytes){
    if(!expectedDestination)return {ConstantWriteTimeline::Status::UnknownWrite};
    return ResolvePositionPayload(position,list,expectedDestination.Get(),address,output,bytes);
}
struct RegisteredResourceIntent::Data {
    std::shared_ptr<const Copy> producer;ComPtr<ID3D12CommandList> consumer;ComPtr<ID3D12Resource> destination;
    uint64_t position{},payload{},offset{},address{};std::vector<uint8_t> bytes;
};
uint64_t RegisteredResourceIntent::Payload()const noexcept{return data_?data_->payload:0;}
uint64_t RegisteredResourceIntent::Offset()const noexcept{return data_?data_->offset:0;}
uint64_t RegisteredResourceIntent::ReadPosition()const noexcept{return data_?data_->position:0;}
ConstantWriteTimeline::Position RegisteredResourceIntent::ProducerPosition()const noexcept{return data_?data_->producer->point:ConstantWriteTimeline::Position{};}
std::span<const uint8_t> RegisteredResourceIntent::Bytes()const noexcept{return data_?std::span<const uint8_t>(data_->bytes):std::span<const uint8_t>{};}
RegisteredResourceIntent::operator bool()const noexcept{return bool(data_);}
bool CaptureRegisteredResourceIntent(uint64_t position,ID3D12CommandList* list,
    const ComPtr<ID3D12Resource>& destination,uint64_t address,size_t bytes,RegisteredResourceIntent& output)try{
    if(output || !position || !list || !destination || !address || !bytes || bytes>65536 || address>UINT64_MAX-bytes)return false;
    const auto desc=destination->GetDesc();const auto gpu=destination->GetGPUVirtualAddress();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || address<gpu || address-gpu>desc.Width || bytes>desc.Width-(address-gpu))return false;
    std::lock_guard lock(mutex);const auto read=readPositions.find(position);
    if(frozen || timelineFailed || read==readPositions.end() || read->second.list!=list ||
        timeline.Resolve(read->second.point,address,bytes).status!=ConstantWriteTimeline::Status::Pending)return false;
    // Only command order within a recording is known before consumer submission.
    // Competing producer recordings require the final queue ledger, not CPU order.
    std::map<uint32_t,ConstantWriteTimeline::RelatedWrite> producers;
    for(const auto& write:timeline.Related(read->second.point,address,bytes)){
        if(write.point.recording==read->second.point.recording && write.point.command>=read->second.point.command)continue;
        const auto found=producers.find(write.point.recording);
        if(found==producers.end() || found->second.point.command<write.point.command)producers[write.point.recording]=write;
    }
    if(producers.size()!=1)return false;
    const auto& write=producers.begin()->second;const auto found=history.find(write.payload);
    if(found==history.end())return false;const auto& copy=*found->second;
    if(copy.invalid || !copy.id || copy.write!=write.id || copy.destination.Get()!=destination.Get() ||
        copy.point.recording!=write.point.recording || copy.point.command!=write.point.command || address<copy.address ||
        address-copy.address>copy.bytes.size() || bytes>copy.bytes.size()-(address-copy.address))return false;
    auto data=std::make_shared<RegisteredResourceIntent::Data>();data->producer=found->second;data->consumer=list;data->destination=destination;
    data->position=position;data->payload=copy.id;data->offset=address-copy.address;data->address=address;
    data->bytes.assign(copy.bytes.begin()+data->offset,copy.bytes.begin()+data->offset+bytes);output.data_=std::move(data);return true;
}catch(...){return false;}
bool ValidateRegisteredResourceIntent(const RegisteredResourceIntent& intent,ConstantWriteTimeline::Result* resolution)try{
    if(resolution)*resolution={};if(!intent.data_)return false;const auto& data=*intent.data_;
    std::vector<uint8_t> actual(data.bytes.size());const auto result=ResolveRegisteredResourceRead(data.position,data.consumer.Get(),
        data.destination,data.address,actual.data(),actual.size());if(resolution)*resolution=result;
    return result.status==ConstantWriteTimeline::Status::Ready && result.payload==data.payload && result.offset==data.offset && actual==data.bytes;
}catch(...){return false;}
void FreezeRegistered(){std::lock_guard lock(mutex);frozen=true;}
bool RegisteredTracking(){return haveCopies.load(std::memory_order_relaxed) || CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed);}
bool RegisteredIntentTracking(){return IntentConfigured();}
void PollRegisteredIntent(){
    static bool enabled{};const bool requested=IntentConfigured();
    const bool cleanup=!requested && intentObserved.exchange(false);
    if(cleanup){source_resource_intent::Clear();if(!CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed))ClearRegistered();}
    if(enabled || requested || cleanup){const auto stats=source_resource_intent::GetStatistics();
        const uint64_t values[]{stats.currentBytes,stats.rangeCount,stats.livePayloadBytes,stats.livePayloads,stats.ownerCount,
            stats.copySequence,stats.evictions,stats.captures,stats.rejected,uint64_t(requested)};
        std::copy(std::begin(values),std::end(values),CyberpunkVR_RegisteredIntentCacheStats);
        CyberpunkVR_RegisteredStructuredIntentStats[0]=stats.structuredBytes;CyberpunkVR_RegisteredStructuredIntentStats[1]=stats.structuredRanges;
    }
    enabled=requested;
}
void RegisterFence(ID3D12Fence* fence) {
    if(!fence || !RegisteredTracking())return;
    std::lock_guard lock(mutex);
    try {
        if(fences.contains(fence))return;
        if(fences.size()>=4096){timelineFailed=true;return;}
        const auto completed=fence->GetCompletedValue();if(completed==UINT64_MAX){timelineFailed=true;return;}
        fences.emplace(fence,fence);timeline.Fence(reinterpret_cast<uintptr_t>(fence),completed);Trace(TraceOp::Fence,{reinterpret_cast<uintptr_t>(fence),completed});
    }catch(...){timelineFailed=true;}
}
void RegisteredSignal(ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t value) {
    if(!RegisteredTracking())return;std::lock_guard lock(mutex);
    try {if(queueOwners.size()>=64 && !queueOwners.contains(queue)){timelineFailed=true;return;}
        queueOwners.try_emplace(queue,queue);timeline.Signal(reinterpret_cast<uintptr_t>(queue),reinterpret_cast<uintptr_t>(fence),value);
        Trace(TraceOp::Signal,{reinterpret_cast<uintptr_t>(queue),reinterpret_cast<uintptr_t>(fence),value});++CyberpunkVR_RegisteredCopySignals;}
    catch(...){timelineFailed=true;}
}
void RegisteredWait(ID3D12CommandQueue* queue,ID3D12Fence* fence,uint64_t value) {
    if(!RegisteredTracking())return;std::lock_guard lock(mutex);
    try {if(queueOwners.size()>=64 && !queueOwners.contains(queue)){timelineFailed=true;return;}
        queueOwners.try_emplace(queue,queue);timeline.Wait(reinterpret_cast<uintptr_t>(queue),reinterpret_cast<uintptr_t>(fence),value);
        Trace(TraceOp::Wait,{reinterpret_cast<uintptr_t>(queue),reinterpret_cast<uintptr_t>(fence),value});++CyberpunkVR_RegisteredCopyWaits;}
    catch(...){timelineFailed=true;}
}
void RegisteredCpuSignal(ID3D12Fence* fence,uint64_t value,uint64_t caller,uint32_t nested) {
    if(!RegisteredTracking())return;std::lock_guard lock(mutex);timeline.InvalidateFence(reinterpret_cast<uintptr_t>(fence));
    try {Trace(TraceOp::CpuSignal,{reinterpret_cast<uintptr_t>(fence),value,caller,reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)),nested});}
    catch(...){traceOverflow=true;}
    ++CyberpunkVR_RegisteredCopyCpuSignals;
}
void RefreshRegisteredFences() {
    std::lock_guard lock(mutex);
    for(const auto& [fence,_]:fences) {
        const auto done=fence->GetCompletedValue();
        if(done==UINT64_MAX){timelineFailed=true;return;}
        timeline.FenceProgress(reinterpret_cast<uintptr_t>(fence),done);Trace(TraceOp::Progress,{reinterpret_cast<uintptr_t>(fence),done});
    }
}
void ClearRegistered(bool publish){std::lock_guard lock(mutex);
    CyberpunkVR_RegisteredCopyTraceSeq.fetch_add(1);
    if(publish)publishedTrace=std::move(trace);else publishedTrace.clear();
    CyberpunkVR_RegisteredCopyTraceCount=UINT(publishedTrace.size());
    CyberpunkVR_RegisteredCopyTraceData=reinterpret_cast<uintptr_t>(publishedTrace.data());
    CyberpunkVR_RegisteredCopyTraceOverflow=traceOverflow;CyberpunkVR_RegisteredCopyTraceSeq.fetch_add(1);
    trace.clear();traceOverflow=false;copies.clear();history.clear();submissions.clear();readPositions.clear();
    fences.clear();queueOwners.clear();timeline.Clear();frozen=false;timelineFailed=false;ownedBytes=0;haveCopies=false;}
}
#endif
