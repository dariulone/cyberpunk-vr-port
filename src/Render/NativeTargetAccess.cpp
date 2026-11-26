#include "Render/NativeTargetAccess.hpp"
#include "Render/NativeBreadcrumbWrite.hpp"
#include "Render/NativeSceneAccess.hpp"
#include <algorithm>
#include <mutex>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do

// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
// The query's checks: rust/crates/cvr-render/src/target_access_query.rs (target_access.rs's mirrors; a source's final
// states read at the target stride).
// The journal's events: rust/crates/cvr-render/src/target_access_events.rs (target_access.rs's Event mirrored; holding
// and appending stay here).
// The observer's counters: rust/crates/cvr-render/src/target_access_observer.rs (the names below are references to them,
// guarded by the observer lock; the two switches are atomics).
struct CvrTaCounters {
    std::atomic<bool> observing,unavailable;bool finished;cvr::stereo::target_access::Failure failure;uint32_t nextProof;
    uint64_t epoch,generation,rows,markers,resets,closes,sourceCount,exits,queries,qualified,finishedRecordings,finishedResources,
        metadataVersion;
};
static_assert(sizeof(CvrTaCounters)==120 && offsetof(CvrTaCounters,epoch)==16 && offsetof(CvrTaCounters,metadataVersion)==112,
    "mirrored by Counters in rust/crates/cvr-render/src/target_access_observer.rs");
extern "C" CvrTaCounters* cvr_ta_counters();
extern "C" void cvr_ta_publish_stats();
// THE JOURNAL IS RUST'S (rust/crates/cvr-render/src/target_access_observer.rs): the recordings, the held resources, the
// counters, the observer lock and the detours' events. What needs the C++ half stays here, under that lock -- the
// breadcrumb resolver of the immediate writes, the writable identities, the residency scope and the receipt's data.
extern "C" void cvr_ta_lock(bool);
extern "C" void cvr_ta_receipt_status(uint64_t queue,uint64_t thread,uint64_t scope,uint32_t frame,uint64_t list0,uint64_t list1,
                                      void* out);
extern "C" void cvr_ta2_clear();
extern "C" void cvr_ta2_arm();
extern "C" void cvr_ta2_finish();
extern "C" bool cvr_ta_mark_source(uint64_t list,uint64_t token,uint64_t generation,uint64_t context,uint64_t leaseList,
                                   uint64_t registry,uint32_t frame,uint32_t side);
struct CvrTaQueryTarget {
    cvr::stereo::target_access::detail::Resource observed;uint32_t planes,expected;uint32_t finals[2];
    uint64_t token,generation,context,list,registry;
};
struct CvrTaQuerySource {uint64_t token,generation,context,list,registry;uint32_t frame,side;CvrTaQueryTarget targets[4];};
extern "C" bool cvr_ta_query(ID3D12CommandQueue* queue,const CvrTaQuerySource* sources,const void* boundary,
                             bool (*validateIdentity)(void* ctx,uint32_t side,uint32_t target),void* ctx);
namespace cvr::stereo::target_access {
struct Receipt::Data {
    std::array<target_handoff::SourceSnapshot,2> sources;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    submission_frontier::Boundary boundary;
    queue_residency::Scope scope;queue_residency::Invocation invocation;
    detail::ReceiptKey key;
    bool Live(ID3D12CommandQueue*,uint32_t)const;
};
namespace {
using Microsoft::WRL::ComPtr;
struct RustMutex {void lock(){cvr_ta_lock(true);}void unlock(){cvr_ta_lock(false);}} mutex;
std::atomic<bool>& observing=cvr_ta_counters()->observing;std::atomic<bool>& unavailable=cvr_ta_counters()->unavailable;
struct Publish {
    Publish(){CyberpunkVR_TargetAccessSeq.fetch_add(1,std::memory_order_acq_rel);}
    ~Publish(){cvr_ta_publish_stats();CyberpunkVR_TargetAccessSeq.fetch_add(1,std::memory_order_release);}
};
uint64_t Address(const void* value){return reinterpret_cast<uintptr_t>(value);}
bool SameLease(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.generation==b.generation && a.context==b.context && a.list==b.list && a.registry==b.registry;
}
bool SameSources(const std::array<target_handoff::SourceSnapshot,2>& a,const std::array<target_handoff::SourceSnapshot,2>& b){
    for(uint32_t side=0;side<2;++side){
        if(!SameLease(a[side].recording,b[side].recording) || a[side].frame!=b[side].frame || a[side].side!=b[side].side)return false;
        for(uint32_t i=0;i<4;++i){const auto& left=a[side].targets[i];const auto& right=b[side].targets[i];
            if(!texture_read::detail::SameWritableIdentityMetadata(left.identity,right.identity) ||
                left.slot!=right.slot || left.planes!=right.planes || left.finalStates!=right.finalStates ||
                std::memcmp(&left.view,&right.view,sizeof(left.view)))return false;
        }
    }
    return true;
}
std::array<uint64_t,2> SourceLists(const std::array<target_handoff::SourceSnapshot,2>& sources){
    return {sources[0].recording.list,sources[1].recording.list};
}
bool SameInvocation(const queue_residency::Invocation& a,const queue_residency::Invocation& b){
    return a.thread==b.thread && a.queue==b.queue && a.listsAddress==b.listsAddress && a.lists==b.lists;
}
// The journal mutex is held by the caller. No pending-token validation is used:
// submitted prefix tokens may be consumed while these exact closed rows remain.
// The journal mutex is held by the caller.
detail::ReceiptStatus ReceiptStatusLocked(ID3D12CommandQueue* queue,uint32_t frame,const queue_residency::Scope& scope,
                                        const std::array<target_handoff::SourceSnapshot,2>& sources){
    detail::ReceiptStatus status;
    cvr_ta_receipt_status(uint64_t(queue),GetCurrentThreadId(),uint64_t(scope.proof.get()),frame,sources[0].recording.list,
                          sources[1].recording.list,&status);
    return status;
}
detail::Resource Observed(const texture_read::WritableTargetIdentity& identity){
    const auto& value=identity.creation;
    return {uint64_t(value.resource.Get()),{uint64_t(value.heap.Get()),value.heapOffset,value.allocationBytes,value.kind},uint32_t(identity.resourceDesc.Flags)};
}
}
bool Receipt::Data::Live(ID3D12CommandQueue* actualQueue,uint32_t frame)const{
    if(!actualQueue || actualQueue!=queue.Get() || !frame || frame!=key.frame || GetCurrentThreadId()!=key.thread)return false;
    {
        std::lock_guard lock(mutex);
        if(!detail::ReceiptMatches(key,ReceiptStatusLocked(actualQueue,frame,scope,sources)))return false;
    }
    queue_residency::Invocation current;
    if(!queue_residency::DescribeInvocation(scope,current) || !SameInvocation(invocation,current) ||
        !detail::ReceiptBoundary(boundary,current,SourceLists(sources)))return false;
    for(const auto& source:sources)for(const auto& target:source.targets)
        if(!texture_read::ValidateWritableIdentity(target.identity))return false;
    if(!queue_residency::Validate(scope))return false;
    std::lock_guard lock(mutex);
    return detail::ReceiptMatches(key,ReceiptStatusLocked(actualQueue,frame,scope,sources));
}
bool Observing()noexcept{return observing.load(std::memory_order_acquire);}
void Clear(){cvr_ta2_clear();}   // target_access_observer.rs' events
void Arm(){cvr_ta2_arm();}
void Finish(){cvr_ta2_finish();}
// ImmediateWrite: target_access_observer.rs' events::immediate_write (the list detour calls it).
extern "C" {
void cvr_ta2_observer_unavailable();
void cvr_ta2_reset(ID3D12GraphicsCommandList*);
void cvr_ta2_close(ID3D12GraphicsCommandList*,HRESULT);
void cvr_ta2_read(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t,UINT,uint64_t,uint64_t);
void cvr_ta2_written(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t,UINT,uint64_t,uint64_t);
void cvr_ta2_barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void cvr_ta2_targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,bool,const D3D12_CPU_DESCRIPTOR_HANDLE*);
void cvr_ta2_shader(ID3D12GraphicsCommandList*,bool);
void cvr_ta2_clear_view(ID3D12GraphicsCommandList*,size_t,bool);
void cvr_ta2_unknown(ID3D12GraphicsCommandList*,uint32_t);
}
void ObserverUnavailable(){cvr_ta2_observer_unavailable();}
void Reset(ID3D12GraphicsCommandList* list){cvr_ta2_reset(list);}
void Close(ID3D12GraphicsCommandList* list,HRESULT result){cvr_ta2_close(list,result);}
void Read(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint32_t kind,UINT subresource,uint64_t offset,uint64_t bytes){
    cvr_ta2_read(list,resource,kind,subresource,offset,bytes);}
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint32_t kind,UINT subresource,uint64_t offset,uint64_t bytes){
    cvr_ta2_written(list,resource,kind,subresource,offset,bytes);}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){cvr_ta2_barriers(list,count,barriers);}
void Targets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* colors,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth){
    cvr_ta2_targets(list,count,colors,contiguous!=FALSE,depth);}
void Shader(ID3D12GraphicsCommandList* list,bool graphics){cvr_ta2_shader(list,graphics);}
void ClearView(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE handle,bool depth){cvr_ta2_clear_view(list,handle.ptr,depth);}
void Unknown(ID3D12GraphicsCommandList* list,uint32_t kind){cvr_ta2_unknown(list,kind);}
bool MarkSource(ID3D12GraphicsCommandList* list,const native_recording::Lease& lease,uint32_t frame,uint32_t side){
    if(!Observing())return false;std::lock_guard lock(mutex);if(!Observing())return false;Publish publish;
    return cvr_ta_mark_source(Address(list),Address(lease.token.Get()),lease.generation,lease.context,lease.list,lease.registry,frame,side);
}
// MarkExit and the diagnostic proof scope are rust/crates/cvr-render/src/target_access_observer.rs' events (the seal's).
bool Query(ID3D12CommandQueue* queue,const std::array<target_handoff::SourceSnapshot,2>& sources,const submission_frontier::Boundary& boundary){
    if(!Observing() && !unavailable.load(std::memory_order_acquire))return false;
    std::lock_guard lock(mutex);
    if(!Observing() && !unavailable.load(std::memory_order_relaxed))return false;Publish publish;
    static_assert(sizeof(submission_frontier::Boundary)==104,"mirrored by target_access.rs");
    static_assert(sizeof(CvrTaQueryTarget)==104 && sizeof(CvrTaQuerySource)==464,"mirrored by QuerySource in target_access_observer.rs");
    CvrTaQuerySource flat[2]{};
    for(uint32_t side=0;side<2;++side){const auto& source=sources[side];auto& out=flat[side];const auto& lease=source.recording;
        out.token=Address(lease.token.Get());out.generation=lease.generation;out.context=lease.context;out.list=lease.list;
        out.registry=lease.registry;out.frame=source.frame;out.side=source.side;
        for(uint32_t t=0;t<4;++t){const auto& target=source.targets[t];auto& value=out.targets[t];const auto& own=target.identity.recording;
            value.observed=Observed(target.identity);value.planes=target.planes;value.expected=uint32_t(target.identity.expectedWritableState);
            value.finals[0]=uint32_t(target.finalStates[0]);value.finals[1]=uint32_t(target.finalStates[1]);
            value.token=Address(own.token.Get());value.generation=own.generation;value.context=own.context;value.list=own.list;
            value.registry=own.registry;
        }
    }
    struct Context {ID3D12CommandQueue* queue;const std::array<target_handoff::SourceSnapshot,2>* sources;} context{queue,&sources};
    return cvr_ta_query(queue,flat,&boundary,[](void* value,uint32_t side,uint32_t target){
        const auto* c=static_cast<const Context*>(value);
        return texture_read::ValidateWritableIdentity((*c->sources)[side].targets[target].identity,c->queue);
    },&context);
}
bool Query(ID3D12CommandQueue* queue,const std::array<target_handoff::SourceSnapshot,2>& supplied,
           const submission_frontier::Boundary& boundary,const queue_residency::Scope& scope,Receipt& output)try{
    output={};const auto frame=supplied[0].frame;
    if(!queue || !scope || !frame || supplied[1].frame!=frame)return false;
    queue_residency::Invocation invocation;
    if(!queue_residency::DescribeInvocation(scope,invocation) || invocation.queue!=uint64_t(queue) ||
        invocation.thread!=GetCurrentThreadId() || !detail::ReceiptBoundary(boundary,invocation,SourceLists(supplied)))return false;
    std::array<target_handoff::SourceSnapshot,2> authenticated;
    if(!target_handoff::SnapshotSources(queue,frame,authenticated) || !SameSources(supplied,authenticated))return false;
    for(const auto& source:authenticated)if(!queue_residency::ContainsRecording(scope,source.recording))return false;
    detail::ReceiptKey before;
    {
        std::lock_guard lock(mutex);const auto state=ReceiptStatusLocked(queue,frame,scope,authenticated);
        if(!detail::ReceiptMatches(state.key,state))return false;before=state.key;
    }
    if(!Query(queue,authenticated,boundary))return false;
    queue_residency::Invocation current;
    if(!queue_residency::DescribeInvocation(scope,current) || !SameInvocation(invocation,current))return false;
    std::array<target_handoff::SourceSnapshot,2> confirmed;
    if(!target_handoff::SnapshotSources(queue,frame,confirmed) || !SameSources(authenticated,confirmed))return false;
    for(const auto& source:confirmed)if(!queue_residency::ContainsRecording(scope,source.recording))return false;
    auto proof=std::make_shared<Receipt::Data>();proof->sources=std::move(confirmed);proof->queue=queue;
    proof->boundary=boundary;proof->scope=scope;proof->invocation=std::move(invocation);proof->key=before;
    {
        std::lock_guard lock(mutex);
        if(!detail::ReceiptMatches(before,ReceiptStatusLocked(queue,frame,scope,proof->sources)))return false;
    }
    if(!proof->Live(queue,frame))return false;
    output.data_=std::move(proof);return true;
}catch(...){output={};return false;}
bool Validate(const Receipt& receipt,ID3D12CommandQueue* queue,uint32_t frame)try{
    return receipt.data_ && receipt.data_->Live(queue,frame);
}catch(...){return false;}
bool Describe(const Receipt& receipt,ID3D12CommandQueue* queue,uint32_t frame,std::array<target_handoff::SourceSnapshot,2>& output)try{
    output={};if(!Validate(receipt,queue,frame))return false;
    auto next=receipt.data_->sources;
    if(!Validate(receipt,queue,frame))return false;
    output=std::move(next);return true;
}catch(...){output={};return false;}
}
#endif
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_TargetAccessSeq{0};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessSources[32]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessChecks[32]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessFailureEvent[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_TargetAccessBreadcrumbStats[8]{};
}
// The query's checks: rust/crates/cvr-render/src/target_access_query.rs (target_access.rs's mirrors; a source's final
// states read at the target stride).
extern "C" bool cvr_taq_frontier(const void*,uint64_t);
extern "C" bool cvr_taq_final_states(void*,const void*,const uint8_t*,size_t,uint32_t,bool);
extern "C" void cvr_taq_seed(void*,const uint8_t*,size_t);
extern "C" void cvr_taq_merge(void*,const void*,bool);
extern "C" void cvr_taq_row(uint32_t,const void*,bool,uint64_t*);
namespace cvr::stereo::target_access {
struct Receipt::Data {
    std::array<target_handoff::SourceSnapshot,2> sources;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    submission_frontier::Boundary boundary;
    queue_residency::Scope scope;queue_residency::Invocation invocation;
    detail::ReceiptKey key;
    bool Live(ID3D12CommandQueue*,uint32_t)const;
};
namespace {
using Microsoft::WRL::ComPtr;
struct HeldResource {ComPtr<ID3D12Resource> resource;constants::ResourceCreationInfo creation;detail::Resource observed;};
struct Recording {
    ComPtr<ID3D12GraphicsCommandList> list;uint64_t generation{};
    bool whole{},closed{},omKnown=true;HRESULT closeResult=E_PENDING;Failure failure{};
    std::vector<detail::Event> events;uint64_t markers{};std::array<uint32_t,9> om{};
    native_recording::Lease source;uint32_t frame{},side=UINT32_MAX,sourceRow=UINT32_MAX,exitRow=UINT32_MAX;
    uint32_t proof{};
    Recording(){om.fill(detail::NoResource);}
};
std::mutex mutex;std::atomic<bool> observing{false},unavailable{false};
std::vector<Recording> recordings;std::vector<HeldResource> resources;
uint64_t epoch{},generation{},rows{},markers{},resets{},closes{},sourceCount{},exits{},queries{},qualified{};
uint64_t finishedRecordings{},finishedResources{};
uint64_t metadataVersion{};
uint32_t nextProof{};Failure failure{};bool finished{};
struct Publish {
    Publish(){CyberpunkVR_TargetAccessSeq.fetch_add(1,std::memory_order_acq_rel);}
    ~Publish(){
        const uint64_t values[16]{3,observing.load(std::memory_order_relaxed),finished,epoch,
            finished?finishedRecordings:recordings.size(),finished?finishedResources:resources.size(),
            rows,markers,resets,closes,sourceCount,exits,queries,qualified,uint32_t(failure),0};
        std::copy(std::begin(values),std::end(values),CyberpunkVR_TargetAccessStats);
        CyberpunkVR_TargetAccessSeq.fetch_add(1,std::memory_order_release);
    }
};
void Fail(Failure value){if(failure==Failure::None)failure=value;}
void Reject(Recording& recording,Failure value){if(recording.failure==Failure::None)recording.failure=value;}
Recording* Find(ID3D12GraphicsCommandList* list){
    Recording* latest{};
    for(auto& record:recordings)if(record.list.Get()==list && (!latest || record.generation>latest->generation))latest=&record;
    return latest;
}
Recording* StartRecording(ID3D12GraphicsCommandList* list,bool whole){
    std::array<detail::RecordingSlot,RecordingLimit> slots{};
    for(uint32_t i=0;i<recordings.size();++i){const auto& record=recordings[i];
        slots[i]={uint64_t(record.list.Get()),record.generation,record.events.size(),record.markers,record.source.token.Get()!=nullptr,record.closed};
    }
    detail::RecordingPlan plan;
    if(!detail::PlanRecording(std::span(slots).first(recordings.size()),uint64_t(list),generation,rows,markers,plan)){
        Fail(plan.failure);return nullptr;
    }
    Recording next;next.list=list;next.generation=plan.generation;next.whole=whole;
    if(plan.index==recordings.size())recordings.push_back(std::move(next));
    else recordings[plan.index]=std::move(next);
    generation=plan.generation;rows=plan.rows;markers=plan.markers;
    return &recordings[plan.index];
}
Recording* Get(ID3D12GraphicsCommandList* list){
    if(!list){Fail(Failure::Arguments);return nullptr;}
    if(auto* found=Find(list))return found;
    return StartRecording(list,false);
}
uint32_t Hold(ID3D12Resource* resource){
    if(!resource)return detail::NoResource;
    for(uint32_t i=0;i<resources.size();++i)if(resources[i].resource.Get()==resource)return i;
    if(resources.size()==ResourceLimit){Fail(Failure::Capacity);return detail::NoResource;}
    HeldResource next;next.resource=resource;
    if(!constants::QueryResourceCreation(next.resource,next.creation))texture_read::QueryObservedAllocation(next.resource,next.creation);
    next.observed={uint64_t(resource),{uint64_t(next.creation.heap.Get()),next.creation.heapOffset,next.creation.allocationBytes,next.creation.kind},
        uint32_t(resource->GetDesc().Flags)};
    resources.push_back(std::move(next));return uint32_t(resources.size()-1);
}
uint32_t HoldBreadcrumb(const breadcrumb_write::Snapshot& snapshot){
    const auto& creation=snapshot.creation;
    const detail::Resource observed{uint64_t(creation.resource.Get()),
        {uint64_t(creation.heap.Get()),creation.heapOffset,creation.allocationBytes,creation.kind},uint32_t(snapshot.resourceDesc.Flags)};
    if(!creation.resource || !texture_read::detail::KnownAllocation(observed.allocation)){
        ++CyberpunkVR_TargetAccessBreadcrumbStats[4];return detail::NoResource;
    }
    for(uint32_t i=0;i<resources.size();++i){auto& cached=resources[i];if(cached.resource!=creation.resource)continue;
        const auto match=detail::CompareAuthenticatedCreation(cached.observed,observed);
        if(match==detail::CreationMatch::Conflict){++CyberpunkVR_TargetAccessBreadcrumbStats[4];return detail::NoResource;}
        if(match==detail::CreationMatch::Upgrade){cached.creation=creation;cached.observed=observed;++metadataVersion;}
        return i;
    }
    if(resources.size()==ResourceLimit){Fail(Failure::Capacity);return detail::NoResource;}
    HeldResource next;next.resource=creation.resource;next.creation=creation;next.observed=observed;
    resources.push_back(std::move(next));return uint32_t(resources.size()-1);
}
bool IsMarker(Kind kind){return kind==Kind::Shader || kind==Kind::Unknown || kind==Kind::Source || kind==Kind::Exit ||
    kind==Kind::ProofBegin || kind==Kind::ProofEnd;}
void Append(Recording& recording,detail::Event event){
    if(recording.closed){Reject(recording,Failure::Closed);return;}
    while(rows==AccessLimit || (IsMarker(event.kind) && markers==MarkerLimit)){
        std::array<detail::RecordingSlot,RecordingLimit> slots{};uint32_t current=UINT32_MAX;
        for(uint32_t i=0;i<recordings.size();++i){const auto& value=recordings[i];
            slots[i]={uint64_t(value.list.Get()),value.generation,value.events.size(),value.markers,value.source.token.Get()!=nullptr,value.closed};
            if(&value==&recording)current=i;
        }
        detail::PressurePlan plan;
        if(!detail::PlanPressure(std::span(slots).first(recordings.size()),current,rows,markers,IsMarker(event.kind),plan)){
            Fail(plan.failure);return;
        }
        if(plan.index==UINT32_MAX){Fail(Failure::Recording);return;}
        auto& discarded=recordings[plan.index];std::vector<detail::Event>{}.swap(discarded.events);discarded.markers=0;discarded.whole=false;
        rows=plan.rows;markers=plan.markers;
    }
    recording.events.push_back(event);++rows;if(IsMarker(event.kind)){++markers;++recording.markers;}
}
void Observe(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,Kind kind,uint32_t tag,UINT subresource){
    if(!observing.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(!observing.load(std::memory_order_relaxed))return;Publish publish;
    try{if(auto* record=Get(list)){
        detail::Event event;event.kind=kind;event.resource=Hold(resource);event.tag=tag;event.subresource=subresource;
        if(!resource)Reject(*record,Failure::Arguments);Append(*record,event);
    }}catch(...){Fail(Failure::Exception);}
}
void ObserveUnknown(ID3D12GraphicsCommandList* list,uint32_t kind){
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{if(auto* record=Get(list)){detail::Event event;event.kind=Kind::Unknown;event.tag=kind;Append(*record,event);}}
    catch(...){Fail(Failure::Exception);}
}
void EndProof(ID3D12GraphicsCommandList* list,uint64_t serial,uint32_t proof,bool complete){
    if(!proof)return;
    std::lock_guard lock(mutex);if(!observing.load(std::memory_order_relaxed))return;Publish publish;
    try{auto* record=Find(list);
        if(!record || record->generation!=serial || record->proof!=proof){Fail(Failure::Proof);return;}
        detail::Event event;event.kind=Kind::ProofEnd;event.proof=proof;event.flags=complete;
        Append(*record,event);record->proof=0;if(!complete)Reject(*record,Failure::Proof);
    }catch(...){Fail(Failure::Exception);}
}
bool SameLease(const native_recording::Lease& a,const native_recording::Lease& b){
    return a.token && a.token==b.token && a.generation==b.generation && a.context==b.context && a.list==b.list && a.registry==b.registry;
}
bool SameSources(const std::array<target_handoff::SourceSnapshot,2>& a,const std::array<target_handoff::SourceSnapshot,2>& b){
    for(uint32_t side=0;side<2;++side){
        if(!SameLease(a[side].recording,b[side].recording) || a[side].frame!=b[side].frame || a[side].side!=b[side].side)return false;
        for(uint32_t i=0;i<4;++i){const auto& left=a[side].targets[i];const auto& right=b[side].targets[i];
            if(!texture_read::detail::SameWritableIdentityMetadata(left.identity,right.identity) ||
                left.slot!=right.slot || left.planes!=right.planes || left.finalStates!=right.finalStates ||
                std::memcmp(&left.view,&right.view,sizeof(left.view)))return false;
        }
    }
    return true;
}
std::array<uint64_t,2> SourceLists(const std::array<target_handoff::SourceSnapshot,2>& sources){
    return {sources[0].recording.list,sources[1].recording.list};
}
bool SameInvocation(const queue_residency::Invocation& a,const queue_residency::Invocation& b){
    return a.thread==b.thread && a.queue==b.queue && a.listsAddress==b.listsAddress && a.lists==b.lists;
}
// The journal mutex is held by the caller. No pending-token validation is used:
// submitted prefix tokens may be consumed while these exact closed rows remain.
detail::ReceiptStatus ReceiptStatusLocked(ID3D12CommandQueue* queue,uint32_t frame,const queue_residency::Scope& scope,
                                        const std::array<target_handoff::SourceSnapshot,2>& sources){
    detail::ReceiptStatus status;status.observing=observing.load(std::memory_order_relaxed);status.finished=finished;
    status.unavailable=unavailable.load(std::memory_order_relaxed);status.failure=failure;
    status.key.epoch=epoch;status.key.metadataVersion=metadataVersion;status.key.queue=uint64_t(queue);
    status.key.thread=GetCurrentThreadId();status.key.scope=uint64_t(scope.proof.get());status.key.frame=frame;
    for(uint32_t side=0;side<2;++side){auto* record=Find(reinterpret_cast<ID3D12GraphicsCommandList*>(sources[side].recording.list));
        if(!record)continue;auto& saved=status.key.sources[side];
        saved.list=uint64_t(record->list.Get());saved.generation=record->generation;saved.nativeGeneration=record->source.generation;
        saved.token=uint64_t(record->source.token.Get());saved.context=record->source.context;saved.registry=record->source.registry;
        saved.rows=record->events.size();saved.markers=record->markers;saved.frame=record->frame;saved.side=record->side;
        saved.sourceRow=record->sourceRow;saved.exitRow=record->exitRow;saved.proof=record->proof;saved.closeResult=record->closeResult;
        saved.failure=record->failure;saved.whole=record->whole;saved.closed=record->closed;saved.omKnown=record->omKnown;saved.om=record->om;
    }
    return status;
}
detail::Resource Observed(const texture_read::WritableTargetIdentity& identity){
    const auto& value=identity.creation;
    return {uint64_t(value.resource.Get()),{uint64_t(value.heap.Get()),value.heapOffset,value.allocationBytes,value.kind},uint32_t(identity.resourceDesc.Flags)};
}
void ReportCheck(uint32_t side,const detail::Check& value,bool success){
    const uint64_t data[16]{side,value.phase,value.rows,value.reads,value.writes,value.transitions,value.aliases,value.shaders,
        value.excludedShaders,value.proofReads,value.proofScopes,uint32_t(value.failure),value.failureRow,value.resource,success,0};
    std::copy(std::begin(data),std::end(data),CyberpunkVR_TargetAccessChecks+side*16);
}
void ReportFailureEvent(const Recording& recording,const detail::Check& check){
    if(CyberpunkVR_TargetAccessFailureEvent[1])return;
    uint64_t data[8]{recording.side,uint64_t(recording.list.Get()),recording.generation,check.failureRow,UINT64_MAX,0,0,0};
    if(check.failureRow<recording.events.size()){
        const auto& event=recording.events[check.failureRow];data[4]=uint32_t(event.kind);data[5]=event.tag;
        if(event.resource<resources.size())data[6]=resources[event.resource].observed.pointer;
        if(event.other<resources.size())data[7]=resources[event.other].observed.pointer;
    }
    std::copy(std::begin(data),std::end(data),CyberpunkVR_TargetAccessFailureEvent);
}
void AddCounts(detail::Check& a,const detail::Check& b){
    a.rows+=b.rows;a.reads+=b.reads;a.writes+=b.writes;a.transitions+=b.transitions;a.aliases+=b.aliases;
    a.shaders+=b.shaders;a.excludedShaders+=b.excludedShaders;a.proofReads+=b.proofReads;a.proofScopes+=b.proofScopes;
}
}
bool Receipt::Data::Live(ID3D12CommandQueue* actualQueue,uint32_t frame)const{
    if(!actualQueue || actualQueue!=queue.Get() || !frame || frame!=key.frame || GetCurrentThreadId()!=key.thread)return false;
    {
        std::lock_guard lock(mutex);
        if(!detail::ReceiptMatches(key,ReceiptStatusLocked(actualQueue,frame,scope,sources)))return false;
    }
    queue_residency::Invocation current;
    if(!queue_residency::DescribeInvocation(scope,current) || !SameInvocation(invocation,current) ||
        !detail::ReceiptBoundary(boundary,current,SourceLists(sources)))return false;
    for(const auto& source:sources)for(const auto& target:source.targets)
        if(!texture_read::ValidateWritableIdentity(target.identity))return false;
    if(!queue_residency::Validate(scope))return false;
    std::lock_guard lock(mutex);
    return detail::ReceiptMatches(key,ReceiptStatusLocked(actualQueue,frame,scope,sources));
}
bool Observing()noexcept{return observing.load(std::memory_order_acquire);}
void ObserverUnavailable(){
    scene_access_trace::ObserverUnavailable();
    if(unavailable.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(unavailable.load(std::memory_order_relaxed))return;Publish publish;
    unavailable.store(true,std::memory_order_release);failure=Failure::Unknown;
}
void Clear(){
    std::lock_guard lock(mutex);Publish publish;observing.store(false,std::memory_order_release);
    recordings.clear();resources.clear();rows=markers=resets=closes=sourceCount=exits=queries=qualified=0;
    failure=unavailable.load(std::memory_order_relaxed)?Failure::Unknown:Failure::None;
    finished=false;finishedRecordings=finishedResources=0;metadataVersion=0;nextProof=0;
    std::fill_n(CyberpunkVR_TargetAccessSources,32,0);std::fill_n(CyberpunkVR_TargetAccessChecks,32,0);
    std::fill_n(CyberpunkVR_TargetAccessFailureEvent,8,0);
    std::fill_n(CyberpunkVR_TargetAccessBreadcrumbStats,8,0);
}
void Arm(){
    std::lock_guard lock(mutex);Publish publish;
    if(unavailable.load(std::memory_order_relaxed)){failure=Failure::Unknown;return;}
    if(observing.load(std::memory_order_relaxed) || finished || !recordings.empty() || epoch==UINT64_MAX){Fail(Failure::Arguments);return;}
    try{recordings.reserve(RecordingLimit);resources.reserve(ResourceLimit);++epoch;observing.store(true,std::memory_order_release);}
    catch(...){Fail(Failure::Exception);}
}
void Finish(){
    std::lock_guard lock(mutex);if(finished)return;Publish publish;
    observing.store(false,std::memory_order_release);
    finishedRecordings=recordings.size();finishedResources=resources.size();finished=true;
    recordings.clear();resources.clear();
}
void Reset(ID3D12GraphicsCommandList* list){
    scene_access_trace::Reset(list);
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{++resets;
        if(!list){Fail(Failure::Arguments);return;}
        if(auto* old=Find(list);old && old->source.token)Reject(*old,Failure::Reset);
        StartRecording(list,true);
    }catch(...){Fail(Failure::Exception);}
}
void Close(ID3D12GraphicsCommandList* list,HRESULT result){
    scene_access_trace::Close(list,result);
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{if(auto* record=Get(list)){++closes;
        if(record->closed)Reject(*record,Failure::Closed);
        record->closed=true;record->closeResult=result;
        if(FAILED(result))Reject(*record,Failure::CloseFailed);
        if(record->proof)Reject(*record,Failure::Proof);
    }}catch(...){Fail(Failure::Exception);}
}
void Read(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint32_t kind,UINT subresource,uint64_t offset,uint64_t bytes){
    scene_access_trace::Operand(list,resource,false,kind,subresource,offset,bytes);Observe(list,resource,Kind::Read,kind,subresource);
}
void Written(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,uint32_t kind,UINT subresource,uint64_t offset,uint64_t bytes){
    scene_access_trace::Operand(list,resource,true,kind,subresource,offset,bytes);Observe(list,resource,Kind::Write,kind,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);
}
bool ImmediateWrite(ID3D12GraphicsCommandList2* list,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* parameters,
                    const D3D12_WRITEBUFFERIMMEDIATE_MODE* modes,uintptr_t caller){
    scene_access_trace::ImmediateWrite(list,count,parameters,modes,caller);
    if(!count || !Observing())return false;
    uint64_t capturedEpoch{},capturedGeneration{};
    {
        std::lock_guard lock(mutex);if(!Observing())return false;Publish publish;
        try{auto* record=Get(list);if(!record)return false;
            ++CyberpunkVR_TargetAccessBreadcrumbStats[0];capturedEpoch=epoch;capturedGeneration=record->generation;
        }catch(...){Fail(Failure::Exception);return false;}
    }
    breadcrumb_write::Lease lease;breadcrumb_write::Snapshot snapshot;bool resolved{};uint32_t resolverFailure{};
    try{
        resolved=breadcrumb_write::Capture(list,count,parameters,modes,caller,lease) && breadcrumb_write::Describe(lease,snapshot);
        if(!resolved)resolverFailure=breadcrumb_write::LastFailure();
    }catch(...){resolverFailure=UINT32_MAX;}
    std::lock_guard lock(mutex);
    if(!Observing() || epoch!=capturedEpoch)return false;Publish publish;
    try{
        auto* record=Find(list);
        if(!record || record->generation!=capturedGeneration){++CyberpunkVR_TargetAccessBreadcrumbStats[3];Fail(Failure::Recording);return false;}
        CyberpunkVR_TargetAccessBreadcrumbStats[5]=resolverFailure;
        CyberpunkVR_TargetAccessBreadcrumbStats[6]=snapshot.destination;
        CyberpunkVR_TargetAccessBreadcrumbStats[7]=uint64_t(snapshot.creation.resource.Get());
        detail::Event event;event.kind=Kind::Unknown;event.tag=13;
        if(resolved){event.resource=HoldBreadcrumb(snapshot);
            if(event.resource!=detail::NoResource){event.kind=Kind::Write;++CyberpunkVR_TargetAccessBreadcrumbStats[1];}
        }
        if(event.kind==Kind::Unknown)++CyberpunkVR_TargetAccessBreadcrumbStats[2];
        Append(*record,event);return event.kind==Kind::Write;
    }catch(...){Fail(Failure::Exception);return false;}
}
void Barriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers){
    scene_access_trace::Barriers(list,count,barriers);
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{auto* record=Get(list);if(!record)return;
        if(count && !barriers){Reject(*record,Failure::Arguments);return;}
        for(UINT i=0;i<count;++i){const auto& barrier=barriers[i];detail::Event event;event.flags=barrier.Flags;
            if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_TRANSITION){event.kind=Kind::Transition;event.resource=Hold(barrier.Transition.pResource);
                event.subresource=barrier.Transition.Subresource;event.before=barrier.Transition.StateBefore;event.after=barrier.Transition.StateAfter;
            }else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){event.kind=Kind::Alias;event.resource=Hold(barrier.Aliasing.pResourceBefore);event.other=Hold(barrier.Aliasing.pResourceAfter);
            }else if(barrier.Type==D3D12_RESOURCE_BARRIER_TYPE_UAV){event.kind=Kind::Uav;event.resource=Hold(barrier.UAV.pResource);
            }else event.kind=Kind::Unknown;
            Append(*record,event);
        }
    }catch(...){Fail(Failure::Exception);}
}
void Targets(ID3D12GraphicsCommandList* list,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* colors,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth){
    scene_access_trace::Targets(list,count,colors,contiguous,depth);
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{if(auto* record=Get(list)){
        std::array<ComPtr<ID3D12Resource>,9> held;record->om.fill(detail::NoResource);
        record->omKnown=scene_state::CaptureBoundResources(count,colors,contiguous,depth,held);
        if(record->omKnown)for(UINT i=0;i<held.size();++i)record->om[i]=Hold(held[i].Get());
    }}catch(...){Fail(Failure::Exception);}
}
void Shader(ID3D12GraphicsCommandList* list,bool graphics){
    scene_access_trace::Shader(list,graphics);
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{if(auto* record=Get(list)){
        detail::Event event;event.kind=Kind::Shader;Append(*record,event);
        if(graphics){
            if(!record->omKnown){event.kind=Kind::Unknown;Append(*record,event);}
            else for(const auto index:record->om)if(index!=detail::NoResource){event.kind=Kind::Write;event.resource=index;Append(*record,event);}
        }
    }}catch(...){Fail(Failure::Exception);}
}
void ClearView(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE handle,bool depth){
    scene_access_trace::ClearView(list,handle,depth);
    if(!Observing())return;
    std::array<ComPtr<ID3D12Resource>,9> held;
    if(!scene_state::CaptureBoundResources(depth?0:1,depth?nullptr:&handle,FALSE,depth?&handle:nullptr,held)){ObserveUnknown(list,0);return;}
    for(const auto& resource:held)if(resource)Observe(list,resource.Get(),Kind::Write,0,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);
}
void Unknown(ID3D12GraphicsCommandList* list,uint32_t kind){
    scene_access_trace::Unknown(list,kind);
    ObserveUnknown(list,kind);
}
bool MarkSource(ID3D12GraphicsCommandList* list,const native_recording::Lease& lease,uint32_t frame,uint32_t side){
    if(!Observing())return false;std::lock_guard lock(mutex);if(!Observing())return false;Publish publish;
    try{auto* record=Get(list);if(!record)return false;
        if(record->closed){Reject(*record,Failure::Closed);return false;}
        if(side>1 || !frame || !lease.token || lease.list!=uint64_t(list) || !lease.generation || record->source.token){Reject(*record,Failure::Source);return false;}
        for(const auto& old:recordings)if(old.source.token && old.side==side){Reject(*record,Failure::Source);return false;}
        record->source=lease;record->frame=frame;record->side=side;record->sourceRow=uint32_t(record->events.size());
        detail::Event event;event.kind=Kind::Source;Append(*record,event);++sourceCount;return record->failure==Failure::None;
    }catch(...){Fail(Failure::Exception);return false;}
}
void MarkExit(ID3D12GraphicsCommandList* list){
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{auto* record=Find(list);if(!record || !record->source.token)return;
        if(record->exitRow!=UINT32_MAX)return;
        record->exitRow=uint32_t(record->events.size());detail::Event event;event.kind=Kind::Exit;Append(*record,event);++exits;
    }catch(...){Fail(Failure::Exception);}
}
DiagnosticProofScope::DiagnosticProofScope(ID3D12GraphicsCommandList* list,std::span<const ComPtr<ID3D12Resource>> held):list_(list){
    if(!Observing())return;std::lock_guard lock(mutex);if(!Observing())return;Publish publish;
    try{auto* record=Find(list);if(!record)return;
        if(!record->source.token || record->exitRow==UINT32_MAX || record->proof || nextProof==UINT32_MAX){Reject(*record,Failure::Proof);return;}
        std::array<ID3D12Resource*,4> unique{};size_t count{};
        for(const auto& resource:held)if(resource){
            if(count==unique.size() || std::find(unique.begin(),unique.begin()+count,resource.Get())!=unique.begin()+count){Reject(*record,Failure::Proof);return;}
            unique[count++]=resource.Get();
        }
        if(count!=4){Reject(*record,Failure::Proof);return;}
        proof_=++nextProof;generation_=record->generation;record->proof=proof_;
        for(const auto resource:unique){detail::Event event;event.kind=Kind::ProofBegin;event.resource=Hold(resource);event.proof=proof_;Append(*record,event);}
    }catch(...){Fail(Failure::Exception);}
}
DiagnosticProofScope::~DiagnosticProofScope(){if(!done_)EndProof(list_,generation_,proof_,false);}
void DiagnosticProofScope::Complete(){if(!done_){EndProof(list_,generation_,proof_,true);done_=true;}}
bool Query(ID3D12CommandQueue* queue,const std::array<target_handoff::SourceSnapshot,2>& sources,const submission_frontier::Boundary& boundary){
    if(!Observing() && !unavailable.load(std::memory_order_acquire))return false;
    std::lock_guard lock(mutex);
    if(!Observing() && !unavailable.load(std::memory_order_relaxed))return false;Publish publish;++queries;
    std::fill_n(CyberpunkVR_TargetAccessSources,32,0);std::fill_n(CyberpunkVR_TargetAccessChecks,32,0);
    const auto refuse=[&](Failure why){Fail(why);return false;};
    try{
        std::array<Recording*,2> selected{};
        for(uint32_t side=0;side<2;++side){const auto& source=sources[side];
            auto* record=Find(reinterpret_cast<ID3D12GraphicsCommandList*>(source.recording.list));selected[side]=record;
            for(auto& candidate:recordings)if(SameLease(candidate.source,source.recording)){record=&candidate;break;}
            const uint64_t values[16]{source.frame,side,source.recording.list,record?record->generation:0,source.recording.generation,
                uint64_t(source.recording.token.Get()),record && record->whole,record && record->closed,uint32_t(record?record->closeResult:E_PENDING),
                0,record?record->events.size():0,record?record->sourceRow:UINT32_MAX,record?record->exitRow:UINT32_MAX,
                uint32_t(record?record->failure:Failure::MissingReset),boundary.sourceIndices[side],0};
            std::copy(std::begin(values),std::end(values),CyberpunkVR_TargetAccessSources+side*16);
        }
        if(unavailable.load(std::memory_order_relaxed)){failure=Failure::Unknown;return false;}
        if(failure!=Failure::None)return false;
        const auto low=std::min(boundary.sourceIndices[0],boundary.sourceIndices[1]),high=std::max(boundary.sourceIndices[0],boundary.sourceIndices[1]);
        if(!queue || boundary.queue!=uint64_t(queue) || !boundary.batch || !boundary.invocation || !boundary.graphEpoch ||
            high>=boundary.count || low+1!=high || boundary.afterIndex!=high+1 || boundary.afterIndex>=boundary.count)return refuse(Failure::Frontier);
        std::array<std::array<detail::Watch,4>,2> watched{};
        for(uint32_t side=0;side<2;++side){const auto& source=sources[side];
            auto* record=selected[side];
            if(!record)return refuse(Failure::MissingReset);
            if(!record->whole)return refuse(Failure::MissingReset);
            if(record->failure!=Failure::None)return refuse(record->failure);
            if(!record->closed || FAILED(record->closeResult))return refuse(Failure::CloseFailed);
            if(source.side!=side || !source.frame || source.frame!=sources[1-side].frame || source.frame!=record->frame || record->side!=side ||
                !SameLease(source.recording,record->source) || !native_recording::ValidateSubmission(record->source,queue))return refuse(Failure::Recording);
            if(record->sourceRow==UINT32_MAX || record->exitRow==UINT32_MAX || record->proof)return refuse(Failure::MissingExit);
            for(uint32_t t=0;t<4;++t){const auto& target=source.targets[t];auto& watch=watched[side][t];
                if(!texture_read::ValidateWritableIdentity(target.identity,queue) || !SameLease(source.recording,target.identity.recording) || !target.planes || target.planes>2)
                    return refuse(Failure::Identity);
                watch.resource=Observed(target.identity);watch.count=target.planes;
                for(uint32_t p=0;p<watch.count;++p)watch.states[p]=target.identity.expectedWritableState;
            }
        }
        if(selected[0]==selected[1])return refuse(Failure::Recording);
        std::vector<detail::Resource> metadata;metadata.reserve(resources.size());for(const auto& resource:resources)metadata.push_back(resource.observed);
        std::array<detail::Check,2> checks;
        for(uint32_t side=0;side<2;++side){auto& check=checks[side];
            bool success=detail::CheckWindow(selected[side]->events,metadata,watched[side],true,check);
            if(success)for(uint32_t t=0;t<4;++t)for(uint32_t p=0;p<watched[side][t].count;++p)
                if(check.finalStates[t][p]!=sources[side].targets[t].finalStates[p])success=check.Reject(Failure::State,uint32_t(selected[side]->events.size()),t);
            ReportCheck(side,check,success);if(!success){ReportFailureEvent(*selected[side],check);return refuse(check.failure);}
        }
        const uint32_t earlier=boundary.sourceIndices[0]<boundary.sourceIndices[1]?0:1,later=1-earlier;
        for(uint32_t t=0;t<4;++t)for(uint32_t p=0;p<watched[earlier][t].count;++p)
            watched[earlier][t].states[p]=sources[earlier].targets[t].finalStates[p];
        detail::Check cross;const bool success=detail::CheckWindow(selected[later]->events,metadata,watched[earlier],false,cross);
        AddCounts(checks[earlier],cross);checks[earlier].phase=1;
        if(!success){checks[earlier].failure=cross.failure;checks[earlier].failureRow=cross.failureRow;checks[earlier].resource=cross.resource;}
        ReportCheck(earlier,checks[earlier],success);if(!success){ReportFailureEvent(*selected[later],cross);return refuse(cross.failure);}
        for(uint32_t side=0;side<2;++side)CyberpunkVR_TargetAccessSources[side*16+15]=1;
        ++qualified;return true;
    }catch(...){return refuse(Failure::Exception);}
}
bool Query(ID3D12CommandQueue* queue,const std::array<target_handoff::SourceSnapshot,2>& supplied,
           const submission_frontier::Boundary& boundary,const queue_residency::Scope& scope,Receipt& output)try{
    output={};const auto frame=supplied[0].frame;
    if(!queue || !scope || !frame || supplied[1].frame!=frame)return false;
    queue_residency::Invocation invocation;
    if(!queue_residency::DescribeInvocation(scope,invocation) || invocation.queue!=uint64_t(queue) ||
        invocation.thread!=GetCurrentThreadId() || !detail::ReceiptBoundary(boundary,invocation,SourceLists(supplied)))return false;
    std::array<target_handoff::SourceSnapshot,2> authenticated;
    if(!target_handoff::SnapshotSources(queue,frame,authenticated) || !SameSources(supplied,authenticated))return false;
    for(const auto& source:authenticated)if(!queue_residency::ContainsRecording(scope,source.recording))return false;
    detail::ReceiptKey before;
    {
        std::lock_guard lock(mutex);const auto state=ReceiptStatusLocked(queue,frame,scope,authenticated);
        if(!detail::ReceiptMatches(state.key,state))return false;before=state.key;
    }
    if(!Query(queue,authenticated,boundary))return false;
    queue_residency::Invocation current;
    if(!queue_residency::DescribeInvocation(scope,current) || !SameInvocation(invocation,current))return false;
    std::array<target_handoff::SourceSnapshot,2> confirmed;
    if(!target_handoff::SnapshotSources(queue,frame,confirmed) || !SameSources(authenticated,confirmed))return false;
    for(const auto& source:confirmed)if(!queue_residency::ContainsRecording(scope,source.recording))return false;
    auto proof=std::make_shared<Receipt::Data>();proof->sources=std::move(confirmed);proof->queue=queue;
    proof->boundary=boundary;proof->scope=scope;proof->invocation=std::move(invocation);proof->key=before;
    {
        std::lock_guard lock(mutex);
        if(!detail::ReceiptMatches(before,ReceiptStatusLocked(queue,frame,scope,proof->sources)))return false;
    }
    if(!proof->Live(queue,frame))return false;
    output.data_=std::move(proof);return true;
}catch(...){output={};return false;}
bool Validate(const Receipt& receipt,ID3D12CommandQueue* queue,uint32_t frame)try{
    return receipt.data_ && receipt.data_->Live(queue,frame);
}catch(...){return false;}
bool Describe(const Receipt& receipt,ID3D12CommandQueue* queue,uint32_t frame,std::array<target_handoff::SourceSnapshot,2>& output)try{
    output={};if(!Validate(receipt,queue,frame))return false;
    auto next=receipt.data_->sources;
    if(!Validate(receipt,queue,frame))return false;
    output=std::move(next);return true;
}catch(...){output={};return false;}
}
#endif
