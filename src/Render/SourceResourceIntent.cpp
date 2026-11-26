#include "Render/SourceResourceIntent.hpp"
#include <windows.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstring>
#include <limits>
#include <mutex>
#include <unordered_map>
#include <utility>
#include <vector>

namespace cvr::stereo::source_resource_intent {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The cache: rust/crates/cvr-render/src/source_intent.rs (tools/single_pass_tests/source_resource_intent_test and
// structured_intent_cache_test run their cases against it in their Rust variants). A Candidate's payload is a Rust
// payload handle with its immutable bytes and labels.
using Microsoft::WRL::ComPtr;
struct RustPayloadView {const uint8_t* bytes;size_t length;uint64_t producerList,sequence,sourceResource,destinationOffset,sourceOffset,copiedBytes;};
struct RustCapture {const void* payload;RustPayloadView view;size_t dataOffset,viewOffset;bool composed;};
static_assert(sizeof(RustPayloadView)==64 && sizeof(RustCapture)==96 && sizeof(Statistics)==96,"mirrored by source_intent.rs");
extern "C" {
void cvr_sri_invalidate(ID3D12Resource*);
void cvr_sri_invalidate_range(ID3D12Resource*,uint64_t,uint64_t);
bool cvr_sri_record_intended_copy(ID3D12GraphicsCommandList*,ID3D12Resource*,uint64_t,uint64_t,const uint8_t*,size_t);
bool cvr_sri_mark_structured_view(ID3D12Resource*,const D3D12_SHADER_RESOURCE_VIEW_DESC*,const std::atomic<uint32_t>*);
bool cvr_sri_observe_copy(ID3D12GraphicsCommandList*,ID3D12Resource*,uint64_t,ID3D12Resource*,uint64_t,uint64_t,const std::atomic<uint32_t>*);
bool cvr_sri_query_diagnostic(ID3D12Resource*,bool,void*);
bool cvr_sri_query_coverage(uint32_t,ID3D12Resource*,uint64_t,size_t,void*);
bool cvr_sri_capture(bool,ID3D12Resource*,uint64_t,size_t,RustCapture*,void*,void(*)(void*,const PieceReceipt*,size_t));
void cvr_sri_payload_release(const void*);
void cvr_sri_clear();
void cvr_sri_statistics(Statistics*);
}
namespace detail {
struct Payload {
    const void* rust{};RustPayloadView view{};
    explicit Payload(const RustPayloadView& value):view(value){}
    Payload(const Payload&)=delete;Payload& operator=(const Payload&)=delete;
    ~Payload(){cvr_sri_payload_release(rust);}
};
}
namespace {
void AppendPieces(void* pieces,const PieceReceipt* values,size_t count){static_cast<std::vector<PieceReceipt>*>(pieces)->assign(values,values+count);}
// The payload handle is owned before anything can throw: a failed allocation releases it.
struct Handle {const void* payload;~Handle(){cvr_sri_payload_release(payload);}};
}
std::span<const uint8_t> Candidate::Bytes()const noexcept{
    return data_?std::span<const uint8_t>(data_->view.bytes,data_->view.length).subspan(dataOffset_,viewBytes_):std::span<const uint8_t>{};
}
uint64_t Candidate::ProducerList()const noexcept{return data_?data_->view.producerList:0;}
uint64_t Candidate::CopySequence()const noexcept{return data_?data_->view.sequence:0;}
uint64_t Candidate::DestinationOffset()const noexcept{return data_?data_->view.destinationOffset:0;}
uint64_t Candidate::SourceOffset()const noexcept{return data_?data_->view.sourceOffset:0;}
uint64_t Candidate::ViewOffset()const noexcept{return viewOffset_;}
size_t Candidate::CopiedBytes()const noexcept{return data_?size_t(data_->view.copiedBytes):0;}
bool Candidate::IsComposed()const noexcept{return composed_;}
std::span<const PieceReceipt> Candidate::Pieces()const noexcept{return pieces_;}
ID3D12Resource* Candidate::Destination()const noexcept{return destination_.Get();}
Candidate::operator bool()const noexcept{return data_ && destination_ && viewBytes_;}
void Invalidate(ID3D12Resource* resource){cvr_sri_invalidate(resource);}
void Invalidate(ID3D12Resource* resource,uint64_t offset,uint64_t bytes){cvr_sri_invalidate_range(resource,offset,bytes);}
bool RecordIntendedCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint64_t offset,uint64_t sourceOffset,std::span<const uint8_t> bytes){
    return cvr_sri_record_intended_copy(list,resource,offset,sourceOffset,bytes.data(),bytes.size());
}
bool MarkStructuredView(ID3D12Resource* resource,const D3D12_SHADER_RESOURCE_VIEW_DESC* view,const std::atomic<uint32_t>* enabled){
    return cvr_sri_mark_structured_view(resource,view,enabled);
}
bool ObserveCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,uint64_t bytes,const std::atomic<uint32_t>* enabled){
    return cvr_sri_observe_copy(list,resource,offset,source,sourceOffset,bytes,enabled);
}
bool QueryCopyDiagnostic(const ComPtr<ID3D12Resource>& resource,CopyDiagnostic& output)noexcept{
    output={};CopyDiagnostic value{};if(!cvr_sri_query_diagnostic(resource.Get(),false,&value))return false;output=value;return true;
}
bool QueryMarkDiagnostic(const ComPtr<ID3D12Resource>& resource,MarkDiagnostic& output)noexcept{
    output={};MarkDiagnostic value{};if(!cvr_sri_query_diagnostic(resource.Get(),true,&value))return false;output=value;return true;
}
bool QueryGenericCoverage(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,GenericCoverageDiagnostic& output)noexcept{
    output={};return cvr_sri_query_coverage(0,resource.Get(),offset,bytes,&output);
}
bool QueryCacheEvents(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,CacheEventDiagnostic& output)noexcept{
    output={};return cvr_sri_query_coverage(1,resource.Get(),offset,bytes,&output);
}
bool QueryStructuredCoverage(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,StructuredCoverageDiagnostic& output)noexcept{
    output={};return cvr_sri_query_coverage(2,resource.Get(),offset,bytes,&output);
}
bool CaptureAtDraw(const ComPtr<ID3D12Resource>& resource,uint64_t address,size_t bytes,Candidate& output){
    output={};try{Candidate captured;RustCapture value{};
        if(!cvr_sri_capture(false,resource.Get(),address,bytes,&value,&captured.pieces_,&AppendPieces))return false;
        Handle handle{value.payload};auto payload=std::make_shared<detail::Payload>(value.view);payload->rust=std::exchange(handle.payload,nullptr);
        captured.data_=std::move(payload);captured.destination_=resource;captured.dataOffset_=value.dataOffset;captured.viewOffset_=value.viewOffset;
        captured.viewBytes_=bytes;captured.composed_=value.composed;output=std::move(captured);return true;
    }catch(...){output={};return false;}
}
bool CaptureStructuredAtDraw(const ComPtr<ID3D12Resource>& resource,uint64_t address,size_t bytes,Candidate& output){
    output={};try{Candidate captured;RustCapture value{};
        if(!cvr_sri_capture(true,resource.Get(),address,bytes,&value,&captured.pieces_,&AppendPieces))return false;
        Handle handle{value.payload};auto payload=std::make_shared<detail::Payload>(value.view);payload->rust=std::exchange(handle.payload,nullptr);
        captured.data_=std::move(payload);captured.destination_=resource;captured.dataOffset_=value.dataOffset;captured.viewOffset_=value.viewOffset;
        captured.viewBytes_=bytes;captured.composed_=value.composed;output=std::move(captured);return true;
    }catch(...){output={};return false;}
}
void Clear(){cvr_sri_clear();}
Statistics GetStatistics(){Statistics value{};cvr_sri_statistics(&value);return value;}
uint64_t CurrentBytes(){return GetStatistics().currentBytes;}
uint64_t RangeCount(){return GetStatistics().rangeCount;}
#endif
#else
using Microsoft::WRL::ComPtr;
namespace detail {
struct ResourceBudget {std::atomic<size_t> bytes{};};
struct Payload {
    std::vector<uint8_t> bytes;
    std::shared_ptr<ResourceBudget> budget;
    uint64_t producerList{},sequence{},sourceResource{},destinationOffset{},sourceOffset{},copiedBytes{},gpuBase{},resourceWidth{};
    ~Payload();
};
}
namespace {
constexpr GUID ownerKey{0x680b32c4,0x81fa,0x4c6d,{0xaf,0x61,0x22,0x5a,0xc3,0x78,0x14,0x81}};
constexpr GUID ownerType{0x680b32c4,0x81fa,0x4c6d,{0xaf,0x61,0x22,0x5a,0xc3,0x78,0x14,0x82}};
constexpr GUID structuredKey{0x680b32c4,0x81fa,0x4c6d,{0xaf,0x61,0x22,0x5a,0xc3,0x78,0x14,0x83}};
constexpr GUID copyDiagnosticKey{0x680b32c4,0x81fa,0x4c6d,{0xaf,0x61,0x22,0x5a,0xc3,0x78,0x14,0x84}};
constexpr GUID markDiagnosticKey{0x680b32c4,0x81fa,0x4c6d,{0xaf,0x61,0x22,0x5a,0xc3,0x78,0x14,0x85}};
constexpr uint64_t CopyDiagnosticVersion=0x4350494e54303031ull,MarkDiagnosticVersion=0x53564d41524b3031ull;
constexpr uint64_t StructuredCoverageVersion=0x5356434f56303031ull;
constexpr uint64_t GenericCoverageVersion=0x47434f5630303031ull;
constexpr uint64_t CacheEventsVersion=0x4345565430303031ull;
struct StructuredHint {
    uint64_t signature{},resource{},gpu{},width{},generation{},offset{},bytes{};
    bool operator==(const StructuredHint&)const=default;
};
constexpr uint64_t StructuredSignature=0x5352563132383031ull;
struct Owner;
struct Publication;
struct StructuredRange {
    std::shared_ptr<const detail::Payload> data;
    uint64_t offset{};
    size_t dataOffset{},bytes{};
};
struct EventHistory {std::array<CacheEventReceipt,MaxCacheEventsPerOwner> events{};size_t next{},count{};};
struct Global {
    std::mutex mutex,creationMutex;
    std::atomic<Owner*> retired{};
    std::atomic<size_t> payloadBytes{},payloadCount{};
    std::vector<Owner*> owners;
    uint64_t currentBytes{},ranges{},sequence{},touch{},evictions{},captures{},rejected{},diagnosticSequence{},generation=1;
    uint64_t structuredBytes{},structuredRanges{};
    uint64_t eventSequence{};
};
Global& State(){static auto* state=new Global;return *state;}
template<class T>uint64_t Checksum(const T& value){const auto* bytes=reinterpret_cast<const uint8_t*>(&value);uint64_t hash=14695981039346656037ull;
    for(size_t n=0;n<offsetof(T,checksum);++n)hash=(hash^bytes[n])*1099511628211ull;return hash;}
template<class T>bool ReadDiagnostic(ID3D12Resource* resource,const GUID& key,uint64_t version,T& output){
    output={};if(!resource)return false;T value{};UINT bytes=sizeof(value);
    if(FAILED(resource->GetPrivateData(key,&bytes,&value)) || bytes!=sizeof(value) || value.version!=version ||
        value.resource!=reinterpret_cast<uintptr_t>(resource) || !value.sequence || value.checksum!=Checksum(value))return false;
    output=value;return true;
}
template<class T>void WriteDiagnostic(ID3D12Resource* resource,const GUID& key,T value)noexcept{
    if(!resource)return;try{auto& state=State();std::lock_guard lock(state.mutex);T previous{};
        if(ReadDiagnostic(resource,key,value.version,previous) && previous.sequence>value.sequence)return;
        value.checksum=Checksum(value);resource->SetPrivateData(key,sizeof(value),&value);
    }catch(...){}
}
uint64_t DiagnosticSequence(){auto& state=State();std::lock_guard lock(state.mutex);
    return state.diagnosticSequence==UINT64_MAX?0:++state.diagnosticSequence;}
struct Owner final:IUnknown {
    std::atomic<ULONG> references{1};Owner* next{};
    std::shared_ptr<detail::ResourceBudget> budget=std::make_shared<detail::ResourceBudget>();
    std::vector<std::shared_ptr<const detail::Payload>> ranges;
    size_t smallRanges{};
    std::vector<StructuredRange> structured;
    StructuredHint structuredHint;
    uint64_t resource{},resourceWidth{},mutation{},touch{},structuredBytes{};bool registered{};
    uint64_t fullInvalidations{},rangeInvalidations{},genericEvictions{},lastInvalidationOffset{},lastInvalidationBytes{};
    uint64_t lastOperation{},lastOperationSequence{},lastOperationSource{},lastOperationProducerList{},lastOperationGeneration{};
    std::unique_ptr<EventHistory> events;
    std::vector<Publication*> publications;
    uint64_t droppedEvents{};
    explicit Owner(ID3D12Resource* destination):resource(reinterpret_cast<uintptr_t>(destination)),resourceWidth(destination->GetDesc().Width){}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** output)override{
        if(!output)return E_POINTER;*output=nullptr;if(id!=__uuidof(IUnknown) && id!=ownerType)return E_NOINTERFACE;
        *output=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++references;}
    ULONG STDMETHODCALLTYPE Release()override{
        const auto count=--references;if(!count){auto& state=State();next=state.retired.load(std::memory_order_relaxed);
            // D3D12 may own its object lock here; never acquire our cache lock.
            while(!state.retired.compare_exchange_weak(next,this,std::memory_order_release,std::memory_order_relaxed)){}
        }return count;
    }
};
struct Publication {
    Owner* owner{};uint64_t offset{},bytes{};bool valid{};
    Publication()=default;Publication(const Publication&)=delete;Publication& operator=(const Publication&)=delete;
    // All access is under State().mutex; the observing call holds the Owner.
    bool Begin(Owner& value,uint64_t start,uint64_t length){
        if(value.publications.size()>=64)return false;
        value.publications.push_back(this);owner=&value;offset=start;bytes=length;valid=true;return true;
    }
    ~Publication(){if(owner){auto& state=State();std::lock_guard lock(state.mutex);std::erase(owner->publications,this);}}
};
void Remove(Owner& owner,size_t index){auto& state=State();const auto bytes=owner.ranges[index]->bytes.size();
    if(bytes<=MaxSmallCopyBytes)--owner.smallRanges;state.currentBytes-=bytes;--state.ranges;
    owner.ranges.erase(owner.ranges.begin()+index);}
void RemoveStructured(Owner& owner,size_t index){auto& state=State();state.structuredBytes-=owner.structured[index].bytes;--state.structuredRanges;
    owner.structuredBytes-=owner.structured[index].bytes;
    owner.structured.erase(owner.structured.begin()+index);}
void EmptyStructured(Owner& owner){auto& state=State();state.structuredBytes-=owner.structuredBytes;state.structuredRanges-=owner.structured.size();
    owner.structuredBytes=0;std::vector<StructuredRange>().swap(owner.structured);owner.structuredHint={};}
void Increment(uint64_t& value){if(value!=UINT64_MAX)++value;}
void Event(Owner& owner,CacheEventReason reason,uint64_t offset,uint64_t bytes,uint64_t copySequence=0,uint64_t list=0,uint64_t source=0)noexcept{
    try{auto& state=State();if(!owner.registered || state.eventSequence==UINT64_MAX){Increment(owner.droppedEvents);return;}
        if(!owner.events)owner.events=std::make_unique<EventHistory>();auto& history=*owner.events;
        if(history.count==history.events.size())Increment(owner.droppedEvents);else ++history.count;
        history.events[history.next]={state.generation,source,list,offset,bytes,copySequence,uint64_t(reason),++state.eventSequence};
        history.next=(history.next+1)%history.events.size();
    }catch(...){Increment(owner.droppedEvents);}
}
void ReleaseEvents(Owner& owner)noexcept {if(!owner.events)return;
    for(size_t n=0;n<owner.events->count;++n)Increment(owner.droppedEvents);owner.events.reset();}
void Operation(Owner& owner,GenericOperation operation,uint64_t offset,uint64_t bytes,uint64_t list=0,uint64_t source=0,uint64_t sequence=0){
    auto& state=State();owner.lastOperation=uint64_t(operation);owner.lastInvalidationOffset=offset;owner.lastInvalidationBytes=bytes;
    if(!sequence && state.diagnosticSequence!=UINT64_MAX)sequence=++state.diagnosticSequence;
    owner.lastOperationSequence=sequence;owner.lastOperationSource=source;owner.lastOperationProducerList=list;owner.lastOperationGeneration=state.generation;
}
void Empty(Owner& owner,GenericOperation operation=GenericOperation::ResourceDestroyed,bool diagnose=true){
    for(auto* pending:owner.publications)pending->valid=false;
    Increment(owner.fullInvalidations);if(diagnose)Operation(owner,operation,0,owner.resourceWidth);
    const auto reason=operation==GenericOperation::Clear?CacheEventReason::Clear:operation==GenericOperation::OwnerEviction?
        CacheEventReason::OwnerCapacityEviction:operation==GenericOperation::ResourceDestroyed?CacheEventReason::ResourceDestroyed:CacheEventReason::ExplicitFullInvalidation;
    if(diagnose)Event(owner,reason,0,owner.resourceWidth);
    if(operation==GenericOperation::OwnerEviction)for(const auto& value:owner.ranges)
        Event(owner,reason,value->destinationOffset,value->bytes.size(),value->sequence,value->producerList,value->sourceResource);
    if(operation==GenericOperation::OwnerEviction)for(size_t n=0;n<owner.ranges.size();++n)Increment(owner.genericEvictions);
    while(!owner.ranges.empty())Remove(owner,owner.ranges.size()-1);EmptyStructured(owner);++owner.mutation;}
void Drain(){auto& state=State();auto* owner=state.retired.exchange(nullptr,std::memory_order_acquire);
    while(owner){auto* next=owner->next;Empty(*owner);
        if(owner->registered){std::erase(state.owners,owner);owner->registered=false;}ReleaseEvents(*owner);delete owner;owner=next;}
}
void Register(Owner& owner){auto& state=State();owner.touch=++state.touch;if(owner.registered)return;
    if(state.owners.size()==MaxOwners){auto* oldest=*std::min_element(state.owners.begin(),state.owners.end(),[](const auto* a,const auto* b){return a->touch<b->touch;});
        Empty(*oldest,GenericOperation::OwnerEviction);oldest->registered=false;ReleaseEvents(*oldest);std::erase(state.owners,oldest);++state.evictions;}
    state.owners.push_back(&owner);owner.registered=true;
}
ComPtr<Owner> Existing(ID3D12Resource* resource){
    ComPtr<Owner> result;if(!resource)return result;IUnknown* raw{};UINT bytes=sizeof(raw);
    if(FAILED(resource->GetPrivateData(ownerKey,&bytes,&raw)) || bytes!=sizeof(raw) || !raw)return result;
    ComPtr<IUnknown> held;held.Attach(raw);held->QueryInterface(ownerType,reinterpret_cast<void**>(result.GetAddressOf()));return result;
}
ComPtr<Owner> Cache(ID3D12Resource* resource,bool create){
    auto current=Existing(resource);if(current || !create)return current;
    auto& state=State();std::lock_guard serial(state.creationMutex);current=Existing(resource);if(current)return current;
    static const bool pinned=[] {HMODULE module{};return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&State),&module)!=FALSE;}();
    if(!pinned)return {};
    ComPtr<Owner> created;created.Attach(new Owner(resource));
    if(FAILED(resource->SetPrivateDataInterface(ownerKey,created.Get())))return {};return created;
}
bool Span(uint64_t offset,uint64_t bytes,uint64_t width){return bytes && offset<=width && bytes<=width-offset;}
bool Destination(ID3D12Resource* resource,uint64_t offset,size_t bytes,uint64_t& gpu,uint64_t& width,size_t limit=MaxCopyBytes){
    gpu=width=0;if(!resource || !bytes || bytes>limit)return false;
    const auto desc=resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Flags || !Span(offset,bytes,desc.Width) ||
        FAILED(resource->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return false;
    gpu=resource->GetGPUVirtualAddress();width=desc.Width;return gpu && gpu<=UINT64_MAX-width;
}
bool RawHint(ID3D12Resource* resource,StructuredHint& output){
    output={};if(!resource)return false;StructuredHint current;UINT size=sizeof(current);
    if(FAILED(resource->GetPrivateData(structuredKey,&size,&current)) || size!=sizeof(current) || current.signature!=StructuredSignature ||
        current.resource!=reinterpret_cast<uintptr_t>(resource))return false;output=current;return true;
}
bool Hint(ID3D12Resource* resource,uint64_t generation,uint64_t offset,uint64_t bytes,uint64_t gpu,uint64_t width,StructuredHint& output){
    StructuredHint current;output={};
    if(!RawHint(resource,current) || current.generation!=generation || current.gpu!=gpu || current.width!=width ||
        !current.bytes || current.bytes>MaxResourceBytes || !Span(current.offset,current.bytes,width) || offset<current.offset ||
        !Span(offset-current.offset,bytes,current.bytes))return false;
    output=current;return true;
}
bool Overlap(const detail::Payload& value,uint64_t offset,uint64_t bytes){return value.destinationOffset<offset+bytes && offset<value.destinationOffset+value.bytes.size();}
void EraseOverlap(Owner& owner,uint64_t offset,uint64_t bytes,uint64_t list=0,uint64_t source=0){
    if(!bytes || offset>UINT64_MAX-bytes){for(const auto& value:owner.ranges)
            Event(owner,CacheEventReason::OverwrittenPayload,value->destinationOffset,value->bytes.size(),value->sequence,list,source);
        Empty(owner,GenericOperation::None,false);return;}
    for(auto* pending:owner.publications)if(pending->offset<offset+bytes && offset<pending->offset+pending->bytes)pending->valid=false;
    for(size_t index=owner.ranges.size();index--;)if(Overlap(*owner.ranges[index],offset,bytes)){
        const auto& value=*owner.ranges[index];Event(owner,CacheEventReason::OverwrittenPayload,value.destinationOffset,value.bytes.size(),value.sequence,list,source);
        Remove(owner,index);}
    ++owner.mutation;
}
void EraseStructuredOverlap(Owner& owner,uint64_t offset,uint64_t bytes){
    if(!bytes || offset>UINT64_MAX-bytes){EmptyStructured(owner);return;}
    try{std::vector<StructuredRange> retained;retained.reserve(owner.structured.size()+1);uint64_t retainedBytes{};
        for(const auto& range:owner.structured){const auto end=range.offset+range.bytes;
            if(end<=offset || range.offset>=offset+bytes){retained.push_back(range);retainedBytes+=range.bytes;continue;}
            if(range.offset<offset){auto left=range;left.bytes=size_t(offset-range.offset);retainedBytes+=left.bytes;retained.push_back(std::move(left));}
            if(end>offset+bytes){auto right=range;const auto skip=offset+bytes-range.offset;right.offset+=skip;
                right.dataOffset+=size_t(skip);right.bytes-=size_t(skip);retainedBytes+=right.bytes;retained.push_back(std::move(right));}}
        auto& state=State();while(retained.size()>MaxStructuredRangesPerResource){retainedBytes-=retained.front().bytes;retained.erase(retained.begin());++state.evictions;}
        state.structuredBytes-=owner.structuredBytes;
        state.structuredRanges-=owner.structured.size();owner.structured.swap(retained);
        owner.structuredBytes=retainedBytes;
        state.structuredBytes+=retainedBytes;state.structuredRanges+=owner.structured.size();
    }catch(...){EmptyStructured(owner);throw;}
}
bool InvalidateRangeEvent(ID3D12Resource* resource,uint64_t offset,uint64_t bytes,GenericOperation operation,
    uint64_t list=0,uint64_t source=0,uint64_t sequence=0)noexcept{
    try{auto owner=Cache(resource,false);if(!owner)return false;auto& state=State();std::lock_guard lock(state.mutex);Drain();
        Increment(owner->rangeInvalidations);Operation(*owner.Get(),operation,offset,bytes,list,source,sequence);
        const bool full=!bytes || offset>UINT64_MAX-bytes;
        Event(*owner.Get(),operation==GenericOperation::ObserveCopy?CacheEventReason::ObserveCopyInvalidation:CacheEventReason::ExplicitRangeInvalidation,
            full?0:offset,full?owner->resourceWidth:bytes,0,list,source);
        EraseOverlap(*owner.Get(),offset,bytes,list,source);EraseStructuredOverlap(*owner.Get(),offset,bytes);return true;
    }catch(...){return false;}
}
void RejectedEvent(ID3D12Resource* resource,uint64_t offset,uint64_t bytes,uint64_t list,uint64_t source,DiagnosticOutcome outcome)noexcept{
    try{auto owner=Cache(resource,false);if(!owner)return;auto& state=State();std::lock_guard lock(state.mutex);Drain();
        Event(*owner.Get(),CacheEventReason(uint64_t(CacheEventReason::RejectedBase)+uint64_t(outcome)),offset,bytes,0,list,source);
    }catch(...){}
}
void Account(detail::Payload& value,const std::shared_ptr<detail::ResourceBudget>& budget){auto& state=State();value.budget=budget;
    state.payloadBytes.fetch_add(value.bytes.size(),std::memory_order_relaxed);state.payloadCount.fetch_add(1,std::memory_order_relaxed);
    budget->bytes.fetch_add(value.bytes.size(),std::memory_order_relaxed);}
// Repack only backing data with no captured/generic owner. Original copy labels
// survive repacking; each range still identifies its exact original extent.
void CompactStructured(Owner& owner){
    struct Group {size_t bytes{};std::vector<size_t> ranges;};std::unordered_map<const detail::Payload*,Group> groups;
    for(size_t index=0;index<owner.structured.size();++index){const auto& range=owner.structured[index];auto& group=groups[range.data.get()];
        group.bytes+=range.bytes;group.ranges.push_back(index);}
    for(const auto& [old,group]:groups){
        if(group.bytes==old->bytes.size() || owner.structured[group.ranges.front()].data.use_count()!=long(group.ranges.size()))continue;
        auto packed=std::make_shared<detail::Payload>();packed->bytes.reserve(group.bytes);
        packed->producerList=old->producerList;packed->sequence=old->sequence;packed->sourceResource=old->sourceResource;
        packed->destinationOffset=old->destinationOffset;packed->sourceOffset=old->sourceOffset;packed->copiedBytes=old->copiedBytes;
        packed->gpuBase=old->gpuBase;packed->resourceWidth=old->resourceWidth;
        for(const auto index:group.ranges){const auto& range=owner.structured[index];
            packed->bytes.insert(packed->bytes.end(),old->bytes.begin()+range.dataOffset,old->bytes.begin()+range.dataOffset+range.bytes);
        }
        size_t at{};for(const auto index:group.ranges){auto& range=owner.structured[index];const auto count=range.bytes;range.data=packed;range.dataOffset=at;at+=count;}
        Account(*packed,owner.budget);
    }
}
enum class RangeClass {Any,Small,Ordinary};
bool Evict(Owner* only=nullptr,bool genericOnly=false,CacheEventReason reason=CacheEventReason::GenericRangeEviction,
    RangeClass rangeClass=RangeClass::Any){auto& state=State();Owner* selected{};size_t at{};uint64_t oldest=UINT64_MAX;
    const detail::Payload* payload{};
    const auto select=[&](bool withoutStructured,RangeClass candidates){
        for(auto* owner:state.owners){if((only && owner!=only) || (withoutStructured && !owner->structured.empty()))continue;
            for(size_t index=0;index<owner->ranges.size();++index){const auto& value=*owner->ranges[index];
                if(candidates==RangeClass::Small && value.bytes.size()>MaxSmallCopyBytes)continue;
                if(candidates==RangeClass::Ordinary && value.bytes.size()<=MaxSmallCopyBytes)continue;
                if(value.sequence<oldest){selected=owner;at=index;
                payload=owner->ranges[index].get();oldest=payload->sequence;}
            }
            if(!genericOnly)for(const auto& range:owner->structured)if(range.data->sequence<oldest){selected=owner;payload=range.data.get();oldest=payload->sequence;}}
    };
    // Large transient copies must not flush many untouched small material
    // records just because those original uploads happened earlier.
    if(!only && rangeClass==RangeClass::Any)select(true,RangeClass::Ordinary);
    if(!selected)select(!only,rangeClass);
    if(!selected && !only)select(false,rangeClass);
    if(!selected)return false;
    const auto prior=selected->ranges.size();const auto offset=payload->destinationOffset;const auto bytes=payload->bytes.size();
    const auto source=payload->sourceResource,list=payload->producerList;
    if(genericOnly){Event(*selected,reason,offset,bytes,payload->sequence,list,source);Remove(*selected,at);}
    else{std::erase_if(selected->ranges,[&](const auto& value){if(value.get()!=payload)return false;
            Event(*selected,reason,value->destinationOffset,value->bytes.size(),value->sequence,value->producerList,value->sourceResource);
            if(value->bytes.size()<=MaxSmallCopyBytes)--selected->smallRanges;
            state.currentBytes-=value->bytes.size();--state.ranges;return true;});
        std::erase_if(selected->structured,[&](const auto& range){if(range.data.get()!=payload)return false;
            Event(*selected,reason,range.offset,range.bytes,range.data->sequence,range.data->producerList,range.data->sourceResource);
            state.structuredBytes-=range.bytes;selected->structuredBytes-=range.bytes;--state.structuredRanges;return true;});
        if(selected->structured.empty())std::vector<StructuredRange>().swap(selected->structured);}
    const auto removed=prior-selected->ranges.size();for(size_t n=0;n<removed;++n)Increment(selected->genericEvictions);
    if(removed)Operation(*selected,GenericOperation::GenericEviction,offset,bytes,list,source);
    ++selected->mutation;++state.evictions;return true;
}
bool Space(Owner& owner,size_t bytes){auto& state=State();
    const bool isSmall=bytes<=MaxSmallCopyBytes;
    while(isSmall?owner.smallRanges>=MaxSmallRangesPerResource:owner.ranges.size()-owner.smallRanges>=MaxRangesPerResource)
        if(!Evict(&owner,true,CacheEventReason::GenericRangeEviction,isSmall?RangeClass::Small:RangeClass::Ordinary))return false;
    if(owner.budget->bytes.load(std::memory_order_relaxed)>MaxResourceBytes-bytes)CompactStructured(owner);
    while(owner.budget->bytes.load(std::memory_order_relaxed)>MaxResourceBytes-bytes)
        if(!Evict(&owner,false,CacheEventReason::ResourceBytesEviction))return false;
    while(state.payloadBytes.load(std::memory_order_relaxed)>MaxGlobalBytes-bytes || state.payloadCount.load(std::memory_order_relaxed)>=MaxPayloads)
        if(!Evict(nullptr,false,state.payloadBytes.load(std::memory_order_relaxed)>MaxGlobalBytes-bytes?
            CacheEventReason::GlobalBytesEviction:CacheEventReason::GlobalPayloadEviction))return false;
    return true;
}
bool Record(Owner& owner,ID3D12GraphicsCommandList* list,uint64_t offset,uint64_t sourceOffset,std::span<const uint8_t> bytes,
    uint64_t gpu,uint64_t width,uint64_t generation,const Publication* publication=nullptr,const std::atomic<uint32_t>* enabled=nullptr,
    ID3D12Resource* destination=nullptr,const StructuredHint* expectedHint=nullptr,DiagnosticOutcome* outcome=nullptr,
    ID3D12Resource* observedSource=nullptr){
    auto& state=State();std::lock_guard lock(state.mutex);Drain();
    const auto reject=[&](DiagnosticOutcome reason){if(outcome)*outcome=reason;++state.rejected;return false;};
    if(state.generation!=generation)return reject(DiagnosticOutcome::GenerationChanged);
    if(enabled && !enabled->load(std::memory_order_acquire))return reject(DiagnosticOutcome::Disabled);Register(owner);
    if(publication && (publication->owner!=&owner || !publication->valid || publication->offset!=offset || publication->bytes!=bytes.size()))return reject(DiagnosticOutcome::Changed);
    if(expectedHint){StructuredHint current;if(!Hint(destination,generation,offset,bytes.size(),gpu,width,current) || current!=*expectedHint)return reject(DiagnosticOutcome::NoHint);}
    if(!observedSource){Increment(owner.rangeInvalidations);Operation(owner,GenericOperation::RecordIntendedCopy,offset,bytes.size(),reinterpret_cast<uintptr_t>(list));
        Event(owner,CacheEventReason::DirectCopyInvalidation,offset,bytes.size(),0,reinterpret_cast<uintptr_t>(list));}
    EraseOverlap(owner,offset,bytes.size(),reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(observedSource));
    if(!observedSource)EraseStructuredOverlap(owner,offset,bytes.size());
    if(state.sequence==UINT64_MAX)return reject(DiagnosticOutcome::Capacity);
    if(!Space(owner,bytes.size()))return reject(DiagnosticOutcome::Budget);
    const bool structured=observedSource && expectedHint;
    if(structured && owner.structuredHint!=*expectedHint){EmptyStructured(owner);owner.structuredHint=*expectedHint;}
    if(structured){while(owner.structured.size()>=MaxStructuredRangesPerResource){RemoveStructured(owner,0);++state.evictions;}
        owner.structured.reserve(owner.structured.size()+1);}
    if(owner.ranges.size()==owner.ranges.capacity())owner.ranges.reserve((std::min)(MaxGenericRangesPerResource,
        (std::max)(size_t(16),owner.ranges.capacity()*2)));
    auto value=std::make_shared<detail::Payload>();value->bytes.assign(bytes.begin(),bytes.end());
    value->producerList=reinterpret_cast<uintptr_t>(list);value->sequence=++state.sequence;value->destinationOffset=offset;
    value->sourceResource=reinterpret_cast<uintptr_t>(observedSource);value->sourceOffset=sourceOffset;value->copiedBytes=bytes.size();value->gpuBase=gpu;value->resourceWidth=width;
    Account(*value,owner.budget);
    if(structured){const auto at=std::lower_bound(owner.structured.begin(),owner.structured.end(),offset,[](const auto& range,uint64_t key){return range.offset<key;});
        owner.structured.insert(at,{value,offset,0,bytes.size()});owner.structuredBytes+=bytes.size();state.structuredBytes+=bytes.size();++state.structuredRanges;}
    owner.ranges.push_back(std::move(value));if(bytes.size()<=MaxSmallCopyBytes)++owner.smallRanges;
    state.currentBytes+=bytes.size();++state.ranges;
    Event(owner,CacheEventReason::PublishedCopy,offset,bytes.size(),state.sequence,reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(observedSource));
    if(outcome)*outcome=DiagnosticOutcome::Success;return true;
}
bool Recording(ID3D12GraphicsCommandList* list){return list && (list->GetType()==D3D12_COMMAND_LIST_TYPE_DIRECT || list->GetType()==D3D12_COMMAND_LIST_TYPE_COPY);}
bool SameDevice(ID3D12DeviceChild* a,ID3D12DeviceChild* b){ComPtr<ID3D12Device> first,second;
    return a && b && SUCCEEDED(a->GetDevice(IID_PPV_ARGS(&first))) && SUCCEEDED(b->GetDevice(IID_PPV_ARGS(&second))) && first==second;}
bool Sample(ID3D12Resource* source,uint64_t offset,std::span<uint8_t> bytes){
    if(!source)return false;const auto desc=source->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !Span(offset,bytes.size(),desc.Width) || offset>SIZE_MAX-bytes.size() ||
        FAILED(source->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_UPLOAD)return false;
    void* mapped{};const D3D12_RANGE requested{SIZE_T(offset),SIZE_T(offset+bytes.size())};if(FAILED(source->Map(0,&requested,&mapped)))return false;
    SIZE_T read{};const auto address=reinterpret_cast<uintptr_t>(mapped);
    const bool okay=mapped && address<=UINTPTR_MAX-offset && address+offset<=UINTPTR_MAX-bytes.size() &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address+offset),bytes.data(),bytes.size(),&read) && read==bytes.size();
    const D3D12_RANGE none{};source->Unmap(0,&none);return okay;
}
void Rejected(){auto& state=State();std::lock_guard lock(state.mutex);Drain();++state.rejected;}
uint64_t Generation(){auto& state=State();std::lock_guard lock(state.mutex);return state.generation;}
}
detail::Payload::~Payload(){if(!budget)return;auto& state=State();state.payloadBytes.fetch_sub(bytes.size(),std::memory_order_relaxed);
    state.payloadCount.fetch_sub(1,std::memory_order_relaxed);budget->bytes.fetch_sub(bytes.size(),std::memory_order_relaxed);}
std::span<const uint8_t> Candidate::Bytes()const noexcept{return data_?std::span<const uint8_t>(data_->bytes).subspan(dataOffset_,viewBytes_):std::span<const uint8_t>{};}
uint64_t Candidate::ProducerList()const noexcept{return data_?data_->producerList:0;}
uint64_t Candidate::CopySequence()const noexcept{return data_?data_->sequence:0;}
uint64_t Candidate::DestinationOffset()const noexcept{return data_?data_->destinationOffset:0;}
uint64_t Candidate::SourceOffset()const noexcept{return data_?data_->sourceOffset:0;}
uint64_t Candidate::ViewOffset()const noexcept{return viewOffset_;}
size_t Candidate::CopiedBytes()const noexcept{return data_?size_t(data_->copiedBytes):0;}
bool Candidate::IsComposed()const noexcept{return composed_;}
std::span<const PieceReceipt> Candidate::Pieces()const noexcept{return pieces_;}
ID3D12Resource* Candidate::Destination()const noexcept{return destination_.Get();}
Candidate::operator bool()const noexcept{return data_ && destination_ && viewBytes_;}
void Invalidate(ID3D12Resource* resource){try{auto owner=Cache(resource,false);if(!owner)return;auto& state=State();std::lock_guard lock(state.mutex);Drain();
    Empty(*owner.Get(),GenericOperation::Invalidate);}catch(...){} }
void Invalidate(ID3D12Resource* resource,uint64_t offset,uint64_t bytes){InvalidateRangeEvent(resource,offset,bytes,GenericOperation::InvalidateRange);}
bool RecordIntendedCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint64_t offset,uint64_t sourceOffset,std::span<const uint8_t> bytes){
    const auto generation=Generation();try{uint64_t gpu{},width{};if(!Recording(list) || !bytes.data() || sourceOffset>UINT64_MAX-bytes.size() ||
            !Destination(resource,offset,bytes.size(),gpu,width) || !SameDevice(list,resource)){
            Invalidate(resource,offset,bytes.size());Rejected();return false;}
        auto owner=Cache(resource,true);if(!owner){Invalidate(resource,offset,bytes.size());Rejected();return false;}
        return Record(*owner.Get(),list,offset,sourceOffset,bytes,gpu,width,generation);
    }catch(...){Invalidate(resource,offset,bytes.size());Rejected();return false;}
}
bool MarkStructuredView(ID3D12Resource* resource,const D3D12_SHADER_RESOURCE_VIEW_DESC* view,const std::atomic<uint32_t>* enabled){
    MarkDiagnostic diagnostic;diagnostic.version=MarkDiagnosticVersion;diagnostic.resource=reinterpret_cast<uintptr_t>(resource);
    diagnostic.sequence=DiagnosticSequence();diagnostic.generation=Generation();diagnostic.stage=uint64_t(DiagnosticStage::Begin);
    if(view){diagnostic.format=view->Format;diagnostic.dimension=view->ViewDimension;diagnostic.componentMapping=view->Shader4ComponentMapping;
        if(view->ViewDimension==D3D12_SRV_DIMENSION_BUFFER){diagnostic.firstElement=view->Buffer.FirstElement;diagnostic.numElements=view->Buffer.NumElements;
            diagnostic.stride=view->Buffer.StructureByteStride;diagnostic.flags=view->Buffer.Flags;}}
    const auto finish=[&](DiagnosticOutcome outcome,bool result=false){diagnostic.outcome=uint64_t(outcome);diagnostic.hintAccepted=result;
        WriteDiagnostic(resource,markDiagnosticKey,diagnostic);return result;};
    WriteDiagnostic(resource,markDiagnosticKey,diagnostic);
    if(enabled && !enabled->load(std::memory_order_acquire))return finish(DiagnosticOutcome::Disabled);
    const auto generation=diagnostic.generation;try{
        if(resource){const auto desc=resource->GetDesc();diagnostic.resourceWidth=desc.Width;diagnostic.resourceGpu=resource->GetGPUVirtualAddress();
            D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};if(SUCCEEDED(resource->GetHeapProperties(&heap,&flags))){
                diagnostic.heapType=heap.Type;diagnostic.cpuPage=heap.CPUPageProperty;diagnostic.heapFlags=flags;}}
        diagnostic.stage=uint64_t(DiagnosticStage::Hint);
        uint64_t gpu{},width{},offset{},bytes{};bool valid=resource && view && view->ViewDimension==D3D12_SRV_DIMENSION_BUFFER &&
            view->Format==DXGI_FORMAT_UNKNOWN && view->Shader4ComponentMapping==D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING &&
            view->Buffer.Flags==D3D12_BUFFER_SRV_FLAG_NONE && view->Buffer.StructureByteStride==128 && view->Buffer.NumElements &&
            view->Buffer.FirstElement<=UINT64_MAX/128;
        if(valid){offset=view->Buffer.FirstElement*128;bytes=uint64_t(view->Buffer.NumElements)*128;
            valid=bytes<=MaxResourceBytes && Destination(resource,offset,size_t(bytes),gpu,width,MaxResourceBytes);}
        diagnostic.viewOffset=offset;diagnostic.viewBytes=bytes;DiagnosticOutcome outcome=DiagnosticOutcome::InvalidView;
        bool accepted{};{auto& state=State();std::lock_guard lock(state.mutex);Drain();
            if(state.generation!=generation)outcome=DiagnosticOutcome::GenerationChanged;
            else if(enabled && !enabled->load(std::memory_order_acquire))outcome=DiagnosticOutcome::Disabled;
            else if(!valid){if(resource)resource->SetPrivateData(structuredKey,0,nullptr);auto owner=Existing(resource);if(owner){EmptyStructured(*owner.Get());++owner->mutation;}}
            else{const StructuredHint hint{StructuredSignature,reinterpret_cast<uintptr_t>(resource),gpu,width,generation,offset,bytes};
                accepted=SUCCEEDED(resource->SetPrivateData(structuredKey,sizeof(hint),&hint));outcome=accepted?DiagnosticOutcome::Success:DiagnosticOutcome::ApiFailed;
                auto owner=Existing(resource);if(owner && (!accepted || owner->structuredHint!=hint)){EmptyStructured(*owner.Get());++owner->mutation;}}}
        diagnostic.stage=uint64_t(DiagnosticStage::Complete);return finish(outcome,accepted);
    }catch(...){return finish(DiagnosticOutcome::ApiFailed);}
}
bool ObserveCopy(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,uint64_t bytes,const std::atomic<uint32_t>* enabled){
    CopyDiagnostic diagnostic;diagnostic.version=CopyDiagnosticVersion;diagnostic.resource=reinterpret_cast<uintptr_t>(resource);
    diagnostic.sequence=DiagnosticSequence();diagnostic.generation=Generation();diagnostic.producerList=reinterpret_cast<uintptr_t>(list);
    diagnostic.sourceResource=reinterpret_cast<uintptr_t>(source);diagnostic.destinationOffset=offset;diagnostic.sourceOffset=sourceOffset;diagnostic.bytes=bytes;
    bool attempted{};
    const auto finish=[&](DiagnosticOutcome outcome,bool result=false){diagnostic.outcome=uint64_t(outcome);diagnostic.publicationGeneration=Generation();
        if(attempted && outcome!=DiagnosticOutcome::Success)RejectedEvent(resource,offset,bytes,diagnostic.producerList,diagnostic.sourceResource,outcome);
        WriteDiagnostic(resource,copyDiagnosticKey,diagnostic);return result;};
    WriteDiagnostic(resource,copyDiagnosticKey,diagnostic);
    if(enabled && !enabled->load(std::memory_order_acquire))return finish(DiagnosticOutcome::Disabled);
    attempted=true;
    const auto generation=diagnostic.generation;
    const bool invalidated=InvalidateRangeEvent(resource,offset,bytes,GenericOperation::ObserveCopy,
        reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(source),diagnostic.sequence);
    try{const bool large=bytes>MaxCopyBytes;uint64_t gpu{},width{};StructuredHint hint,observed;
        if(resource){diagnostic.destinationWidth=resource->GetDesc().Width;diagnostic.destinationGpu=resource->GetGPUVirtualAddress();}
        if(RawHint(resource,observed)){diagnostic.hintPresent=1;diagnostic.hintGeneration=observed.generation;diagnostic.hintOffset=observed.offset;diagnostic.hintBytes=observed.bytes;}
        D3D12_HEAP_PROPERTIES sourceHeap{};D3D12_HEAP_FLAGS sourceFlags{};const bool heapKnown=source && SUCCEEDED(source->GetHeapProperties(&sourceHeap,&sourceFlags));
        if(heapKnown){diagnostic.sourceHeapType=sourceHeap.Type;diagnostic.sourceCpuPage=sourceHeap.CPUPageProperty;diagnostic.sourceHeapFlags=sourceFlags;}
        diagnostic.stage=uint64_t(DiagnosticStage::Destination);
        if(!Recording(list) || !bytes || bytes>MaxResourceBytes || !source || !SameDevice(list,resource) || !SameDevice(resource,source)){
            Rejected();return finish(DiagnosticOutcome::Arguments);}
        if(!Destination(resource,offset,size_t(bytes),gpu,width,large?MaxResourceBytes:MaxCopyBytes)){Rejected();return finish(DiagnosticOutcome::UnsupportedDestination);}
        diagnostic.hintFresh=Hint(resource,generation,offset,bytes,gpu,width,hint);
        diagnostic.stage=uint64_t(DiagnosticStage::Source);
        if(heapKnown && sourceHeap.Type==D3D12_HEAP_TYPE_DEFAULT){Rejected();return finish(DiagnosticOutcome::SourceDefault);}
        if(!heapKnown || sourceHeap.Type!=D3D12_HEAP_TYPE_UPLOAD){Rejected();return finish(DiagnosticOutcome::UnsupportedHeap);}
        if(Generation()!=generation){Rejected();return finish(DiagnosticOutcome::GenerationChanged);}
        if(enabled && !enabled->load(std::memory_order_acquire)){Rejected();return finish(DiagnosticOutcome::Disabled);}
        if(large){auto& state=State();std::lock_guard lock(state.mutex);Drain();
            diagnostic.hintFresh=state.generation==generation && Hint(resource,generation,offset,bytes,gpu,width,hint);}
        diagnostic.stage=uint64_t(DiagnosticStage::Hint);if(large && !diagnostic.hintFresh){Rejected();return finish(DiagnosticOutcome::NoHint);}
        auto owner=Cache(resource,true);if(!owner){Rejected();return finish(DiagnosticOutcome::ApiFailed);}
        Publication publication;DiagnosticOutcome ready=DiagnosticOutcome::Success;{auto& state=State();std::lock_guard lock(state.mutex);Drain();
            if(state.generation!=generation)ready=DiagnosticOutcome::GenerationChanged;
            else if(enabled && !enabled->load(std::memory_order_acquire))ready=DiagnosticOutcome::Disabled;
            else{Register(*owner.Get());if(!invalidated){Increment(owner->rangeInvalidations);Operation(*owner.Get(),GenericOperation::ObserveCopy,offset,bytes,
                    reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(source),diagnostic.sequence);
                    Event(*owner.Get(),CacheEventReason::ObserveCopyInvalidation,offset,bytes,0,diagnostic.producerList,diagnostic.sourceResource);}
                EraseOverlap(*owner.Get(),offset,bytes,diagnostic.producerList,diagnostic.sourceResource);
                if(!publication.Begin(*owner.Get(),offset,bytes))ready=DiagnosticOutcome::Capacity;}}
        if(ready!=DiagnosticOutcome::Success){Rejected();return finish(ready);}
        std::vector<uint8_t> first(static_cast<size_t>(bytes)),second(static_cast<size_t>(bytes));
        diagnostic.stage=uint64_t(DiagnosticStage::FirstSample);if(!Sample(source,sourceOffset,first)){Rejected();return finish(DiagnosticOutcome::MapFailed);}
        diagnostic.stage=uint64_t(DiagnosticStage::SecondSample);if(!Sample(source,sourceOffset,second)){Rejected();return finish(DiagnosticOutcome::MapFailed);}
        diagnostic.stage=uint64_t(DiagnosticStage::Compare);if(first!=second){Rejected();return finish(DiagnosticOutcome::Unstable);}
        diagnostic.stage=uint64_t(DiagnosticStage::Publication);DiagnosticOutcome outcome=DiagnosticOutcome::ApiFailed;
        const bool accepted=Record(*owner.Get(),list,offset,sourceOffset,first,gpu,width,generation,&publication,enabled,resource,
            diagnostic.hintFresh?&hint:nullptr,&outcome,source);
        if(accepted)diagnostic.stage=uint64_t(DiagnosticStage::Complete);return finish(outcome,accepted);
    }catch(...){Rejected();return finish(DiagnosticOutcome::ApiFailed);}
}
bool QueryCopyDiagnostic(const ComPtr<ID3D12Resource>& resource,CopyDiagnostic& output)noexcept{
    try{return ReadDiagnostic(resource.Get(),copyDiagnosticKey,CopyDiagnosticVersion,output);}catch(...){output={};return false;}}
bool QueryMarkDiagnostic(const ComPtr<ID3D12Resource>& resource,MarkDiagnostic& output)noexcept{
    try{return ReadDiagnostic(resource.Get(),markDiagnosticKey,MarkDiagnosticVersion,output);}catch(...){output={};return false;}}
bool QueryGenericCoverage(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,GenericCoverageDiagnostic& output)noexcept{
    output={};try{if(!resource || !bytes || bytes>MaxCopyBytes)return false;
        const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width || !Span(offset,bytes,desc.Width))return false;
        auto owner=Cache(resource.Get(),false);auto& state=State();std::lock_guard lock(state.mutex);Drain();
        output.version=GenericCoverageVersion;output.resource=reinterpret_cast<uintptr_t>(resource.Get());output.generation=state.generation;
        output.queryOffset=offset;output.queryBytes=bytes;output.firstHoleOffset=offset;
        if(!owner || owner->resource!=reinterpret_cast<uintptr_t>(resource.Get()))return true;
        output.ownerPresent=1;output.rangeCount=owner->ranges.size();output.livePayloadBytes=owner->budget->bytes.load(std::memory_order_relaxed);
        output.fullInvalidations=owner->fullInvalidations;output.rangeInvalidations=owner->rangeInvalidations;output.genericEvictions=owner->genericEvictions;
        output.lastInvalidationOffset=owner->lastInvalidationOffset;output.lastInvalidationBytes=owner->lastInvalidationBytes;
        output.lastOperation=owner->lastOperation;output.lastOperationSequence=owner->lastOperationSequence;
        output.lastOperationSource=owner->lastOperationSource;output.lastOperationProducerList=owner->lastOperationProducerList;
        output.lastOperationGeneration=owner->lastOperationGeneration;
        if(owner->ranges.size()>MaxGenericRangesPerResource)return false;
        std::vector<std::pair<uint64_t,uint64_t>> spans;const auto end=offset+bytes;
        for(const auto& value:owner->ranges){if(!output.oldestSequence || value->sequence<output.oldestSequence)output.oldestSequence=value->sequence;
            output.newestSequence=(std::max)(output.newestSequence,value->sequence);
            if(value->gpuBase!=gpu || value->resourceWidth!=desc.Width)continue;
            if(offset>=value->destinationOffset && Span(offset-value->destinationOffset,bytes,value->bytes.size()))++output.singleCoverCount;
            const auto valueEnd=value->destinationOffset+value->bytes.size();
            if(valueEnd<=offset || value->destinationOffset>=end)continue;
            spans.emplace_back((std::max)(offset,value->destinationOffset),(std::min)(end,valueEnd));++output.overlapCount;
        }
        std::sort(spans.begin(),spans.end());uint64_t cursor=offset;bool hole{};
        for(const auto [begin,finish]:spans){
            if(!hole && begin>cursor){output.firstHoleOffset=cursor;hole=true;}
            const auto start=(std::max)(cursor,begin);if(finish>start)output.coveredBytes+=finish-start;
            cursor=(std::max)(cursor,finish);
        }
        if(!hole)output.firstHoleOffset=cursor==end?UINT64_MAX:cursor;return true;
    }catch(...){output={};return false;}
}
bool QueryCacheEvents(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,CacheEventDiagnostic& output)noexcept{
    output={};try{if(!resource || !bytes || bytes>MaxCopyBytes)return false;
        const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width || !Span(offset,bytes,desc.Width))return false;
        auto owner=Cache(resource.Get(),false);auto& state=State();std::lock_guard lock(state.mutex);Drain();
        output.version=CacheEventsVersion;output.resource=reinterpret_cast<uintptr_t>(resource.Get());output.generation=state.generation;
        output.queryOffset=offset;output.queryBytes=bytes;if(!owner || owner->resource!=reinterpret_cast<uintptr_t>(resource.Get()))return true;
        output.ownerPresent=1;output.droppedEvents=owner->droppedEvents;if(!owner->events)return true;
        const auto& history=*owner->events;output.storedEvents=history.count;
        for(size_t n=0;n<history.count;++n){const auto& event=history.events[n];
            const bool intersects=event.bytes && (event.offset>UINT64_MAX-event.bytes || (event.offset<offset+bytes && offset<event.offset+event.bytes));
            if(!intersects)continue;auto newer=[&](CacheEventReceipt& receipt){if(event.eventSequence>receipt.eventSequence)receipt=event;};
            const auto reason=CacheEventReason(event.reason);
            if(reason==CacheEventReason::PublishedCopy)newer(output.lastPublishedCopy);
            else{newer(output.lastInvalidated);
                if(reason==CacheEventReason::OwnerCapacityEviction || reason==CacheEventReason::GlobalBytesEviction || reason==CacheEventReason::GlobalPayloadEviction ||
                    reason==CacheEventReason::ResourceBytesEviction || reason==CacheEventReason::GenericRangeEviction)newer(output.lastEvicted);}
        }
        return true;
    }catch(...){output={};return false;}
}
bool QueryStructuredCoverage(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,StructuredCoverageDiagnostic& output)noexcept{
    output={};try{if(!resource || !bytes || bytes>MaxCopyBytes)return false;
        const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width || !Span(offset,bytes,desc.Width))return false;
        auto owner=Cache(resource.Get(),false);auto& state=State();std::lock_guard lock(state.mutex);Drain();
        output.version=StructuredCoverageVersion;output.resource=reinterpret_cast<uintptr_t>(resource.Get());output.generation=state.generation;
        output.queryOffset=offset;output.queryBytes=bytes;output.firstHoleOffset=offset;
        StructuredHint marker;if(RawHint(resource.Get(),marker)){
            output.markerGeneration=marker.generation;output.viewOffset=marker.offset;output.viewBytes=marker.bytes;
            StructuredHint fresh;output.markerFresh=Hint(resource.Get(),state.generation,marker.offset,marker.bytes,gpu,desc.Width,fresh);}
        if(!owner || owner->resource!=reinterpret_cast<uintptr_t>(resource.Get()))return true;
        output.ownerPresent=1;output.ownerHintGeneration=owner->structuredHint.generation;
        output.ownerHintMatchesMarker=output.markerFresh && owner->structuredHint==marker;
        output.structuredBytes=owner->structuredBytes;output.structuredRanges=owner->structured.size();
        const auto end=offset+bytes;auto at=std::lower_bound(owner->structured.begin(),owner->structured.end(),offset,
            [](const auto& range,uint64_t key){return range.offset<key;});
        if(at!=owner->structured.begin())--at;
        uint64_t cursor=offset;bool hole{};
        for(;at!=owner->structured.end() && at->offset<end;++at){const auto rangeEnd=at->offset+at->bytes;if(rangeEnd<=offset)continue;
            const auto begin=(std::max)(offset,at->offset),finish=(std::min)(end,rangeEnd);
            output.coveredBytes+=finish-begin;
            if(!hole && begin>cursor){output.firstHoleOffset=cursor;hole=true;}
            cursor=finish;
        }
        if(!hole)output.firstHoleOffset=cursor==end?UINT64_MAX:cursor;return true;
    }catch(...){output={};return false;}
}
bool CaptureAtDraw(const ComPtr<ID3D12Resource>& resource,uint64_t address,size_t bytes,Candidate& output){
    output={};try{if(!resource || !bytes || bytes>MaxCopyBytes)return false;
        const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width || address<gpu || !Span(address-gpu,bytes,desc.Width))return false;
        auto owner=Cache(resource.Get(),false);if(!owner)return false;auto& state=State();std::lock_guard lock(state.mutex);Drain();
        if(owner->resource!=reinterpret_cast<uintptr_t>(resource.Get()))return false;
        const uint64_t offset=address-gpu;
        for(const auto& value:owner->ranges)if(value->gpuBase==gpu && value->resourceWidth==desc.Width && offset>=value->destinationOffset &&
            Span(offset-value->destinationOffset,bytes,value->bytes.size())){
            output.data_=value;output.destination_=resource;output.dataOffset_=output.viewOffset_=size_t(offset-value->destinationOffset);output.viewBytes_=bytes;
            owner->touch=++state.touch;++state.captures;return true;}
        return false;
    }catch(...){output={};return false;}
}
bool CaptureStructuredAtDraw(const ComPtr<ID3D12Resource>& resource,uint64_t address,size_t bytes,Candidate& output){
    output={};try{if(!resource || !bytes || bytes>MaxCopyBytes)return false;
        const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width || address<gpu || !Span(address-gpu,bytes,desc.Width))return false;
        auto owner=Cache(resource.Get(),false);if(!owner)return false;auto& state=State();std::lock_guard lock(state.mutex);Drain();
        const uint64_t offset=address-gpu;StructuredHint hint;
        if(owner->resource!=reinterpret_cast<uintptr_t>(resource.Get()) || !Hint(resource.Get(),state.generation,offset,bytes,gpu,desc.Width,hint) ||
            owner->structuredHint!=hint)return false;
        uint64_t cursor=offset;const auto end=offset+bytes;
        std::vector<const StructuredRange*> selected;std::vector<PieceReceipt> pieces;
        for(const auto& range:owner->structured){if(range.offset+range.bytes<=cursor)continue;if(range.offset>cursor)break;
            const auto& value=*range.data;if(value.gpuBase!=gpu || value.resourceWidth!=desc.Width || !value.sourceResource)return false;
            const auto count=std::min<uint64_t>(range.offset+range.bytes,end)-cursor;
            PieceReceipt piece{value.producerList,value.sequence,value.sourceResource,value.destinationOffset,value.sourceOffset,value.copiedBytes,
                cursor,value.sourceOffset+(cursor-value.destinationOffset),count};
            if(!pieces.empty() && pieces.back().copySequence==piece.copySequence && pieces.back().pieceDestinationOffset+pieces.back().pieceBytes==cursor)
                pieces.back().pieceBytes+=count;
            else pieces.push_back(piece);
            selected.push_back(&range);cursor+=count;if(cursor==end)break;
        }
        if(cursor!=end || pieces.empty())return false;
        Candidate captured;captured.destination_=resource;captured.viewBytes_=bytes;captured.pieces_=std::move(pieces);
        if(captured.pieces_.size()==1){const auto& range=*selected.front();captured.data_=range.data;
            captured.dataOffset_=range.dataOffset+size_t(offset-range.offset);captured.viewOffset_=size_t(offset-range.data->destinationOffset);}
        else{if(owner->budget->bytes.load(std::memory_order_relaxed)>MaxResourceBytes-bytes ||
                state.payloadBytes.load(std::memory_order_relaxed)>MaxGlobalBytes-bytes)CompactStructured(*owner.Get());
            if(owner->budget->bytes.load(std::memory_order_relaxed)>MaxResourceBytes-bytes ||
                state.payloadBytes.load(std::memory_order_relaxed)>MaxGlobalBytes-bytes || state.payloadCount.load(std::memory_order_relaxed)>=MaxPayloads)return false;
            auto value=std::make_shared<detail::Payload>();value->bytes.reserve(bytes);cursor=offset;
            for(const auto* range:selected){const auto at=range->dataOffset+size_t(cursor-range->offset);
                const auto count=size_t(std::min<uint64_t>(range->offset+range->bytes,end)-cursor);
                value->bytes.insert(value->bytes.end(),range->data->bytes.begin()+at,range->data->bytes.begin()+at+count);cursor+=count;}
            Account(*value,owner->budget);captured.data_=std::move(value);captured.composed_=true;
        }
        output=std::move(captured);owner->touch=++state.touch;++state.captures;return true;
    }catch(...){output={};return false;}
}
void Clear(){auto& state=State();std::lock_guard lock(state.mutex);Drain();++state.generation;
    for(auto* owner:state.owners){Empty(*owner,GenericOperation::Clear);owner->registered=false;ReleaseEvents(*owner);}state.owners.clear();}
Statistics GetStatistics(){auto& state=State();std::lock_guard lock(state.mutex);Drain();return {state.currentBytes,state.ranges,
    state.payloadBytes.load(std::memory_order_relaxed),state.payloadCount.load(std::memory_order_relaxed),uint64_t(state.owners.size()),
    state.sequence,state.evictions,state.captures,state.rejected,state.generation,state.structuredBytes,state.structuredRanges};}
uint64_t CurrentBytes(){return GetStatistics().currentBytes;}
uint64_t RangeCount(){return GetStatistics().rangeCount;}
#endif
}
