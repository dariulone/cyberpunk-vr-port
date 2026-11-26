#pragma once
#include "Render/NativeTargetHandoff.hpp"
#include "Render/NativeSubmissionFrontier.hpp"
#include "Render/NativeQueueResidencyScope.hpp"
#include <span>
#include <vector>

namespace cvr::stereo::target_access {
inline constexpr uint32_t RecordingLimit=256,ResourceLimit=2048,AccessLimit=32768,MarkerLimit=8192;
enum class Failure:uint32_t {
    None,Arguments,Capacity,MissingReset,Closed,CloseFailed,Recording,Source,MissingExit,
    Frontier,Identity,Metadata,Read,Write,Transition,Alias,Shader,Unknown,Proof,
    State,Reset,Exception
};
enum class Kind:uint32_t {Read,Write,Transition,Alias,Uav,Shader,Unknown,Source,Exit,ProofBegin,ProofEnd};
namespace detail {
inline constexpr uint32_t NoResource=UINT32_MAX;
struct RecordingSlot {
    uint64_t list{},generation{},rows{},markers{};bool source{},closed{};
};
struct RecordingPlan {
    uint32_t index=UINT32_MAX;uint64_t generation{},rows{},markers{};Failure failure{};
};
// Reset discards the previous commands on this list. Other active recordings
// and every source generation must remain intact, including failed sources.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/target_access.rs (tools/single_pass_tests/target_access_test runs its cases
// against them in its Rust variant). The records are mirrored there.
extern "C" bool cvr_ta_plan_recording(const RecordingSlot*,size_t,uint64_t,uint64_t,uint64_t,uint64_t,RecordingPlan*);
inline bool PlanRecording(std::span<const RecordingSlot> slots,uint64_t list,uint64_t generation,
                          uint64_t rows,uint64_t markers,RecordingPlan& output){
    return cvr_ta_plan_recording(slots.data(),slots.size(),list,generation,rows,markers,&output);
}
#else
inline bool PlanRecording(std::span<const RecordingSlot> slots,uint64_t list,uint64_t generation,
                          uint64_t rows,uint64_t markers,RecordingPlan& output){
    output={};
    const auto reject=[&](Failure reason){output.failure=reason;return false;};
    if(!list || slots.size()>RecordingLimit || markers>rows)return reject(Failure::Recording);
    if(generation==UINT64_MAX)return reject(Failure::Capacity);
    uint32_t latest=UINT32_MAX;
    for(uint32_t i=0;i<slots.size();++i){
        if(!slots[i].list || !slots[i].generation || slots[i].generation>generation || slots[i].markers>slots[i].rows)
            return reject(Failure::Recording);
        if(slots[i].list==list && (latest==UINT32_MAX || slots[i].generation>slots[latest].generation))latest=i;
    }
    uint32_t index=latest!=UINT32_MAX && !slots[latest].source?latest:UINT32_MAX;
    if(index==UINT32_MAX){
        if(slots.size()<RecordingLimit)index=uint32_t(slots.size());
        else for(uint32_t i=0;i<slots.size();++i)if(!slots[i].source && slots[i].closed &&
            (index==UINT32_MAX || slots[i].generation<slots[index].generation))index=i;
    }
    if(index==UINT32_MAX)return reject(Failure::Capacity);
    if(index<slots.size()){
        if(slots[index].source || slots[index].rows>rows || slots[index].markers>markers)return reject(Failure::Recording);
        rows-=slots[index].rows;markers-=slots[index].markers;
    }
    output={index,generation+1,rows,markers,Failure::None};return true;
}
#endif
struct PressurePlan {uint32_t index=UINT32_MAX;uint64_t rows{},markers{};Failure failure{};};
#if CVR_RUST_VRIK
extern "C" bool cvr_ta_plan_pressure(const RecordingSlot*,size_t,uint32_t,uint64_t,uint64_t,bool,PressurePlan*);
inline bool PlanPressure(std::span<const RecordingSlot> slots,uint32_t current,uint64_t rows,uint64_t markers,
                         bool appendMarker,PressurePlan& output){
    return cvr_ta_plan_pressure(slots.data(),slots.size(),current,rows,markers,appendMarker,&output);
}
#else
inline bool PlanPressure(std::span<const RecordingSlot> slots,uint32_t current,uint64_t rows,uint64_t markers,
                         bool appendMarker,PressurePlan& output){
    output={UINT32_MAX,rows,markers,Failure::None};
    const auto reject=[&](Failure why){output.failure=why;return false;};
    if(slots.size()>RecordingLimit || current>=slots.size() || markers>rows || rows>AccessLimit || markers>MarkerLimit)
        return reject(Failure::Recording);
    const bool rowPressure=rows==AccessLimit,markerPressure=appendMarker && markers==MarkerLimit;
    if(!rowPressure && !markerPressure)return true;
    uint32_t candidate=UINT32_MAX;
    for(uint32_t i=0;i<slots.size();++i){const auto& slot=slots[i];
        if(i==current || slot.source || !slot.closed || !slot.rows || (!rowPressure && !slot.markers))continue;
        if(!slot.generation || slot.markers>slot.rows || slot.rows>rows || slot.markers>markers)return reject(Failure::Recording);
        if(candidate==UINT32_MAX || slot.generation<slots[candidate].generation)candidate=i;
    }
    if(candidate==UINT32_MAX)return reject(Failure::Capacity);
    output.index=candidate;output.rows-=slots[candidate].rows;output.markers-=slots[candidate].markers;
    if(output.markers>output.rows)return reject(Failure::Recording);
    return true;
}
#endif
struct Resource {uint64_t pointer{};texture_read::detail::Allocation allocation;uint32_t flags{};};
struct ReceiptRecord {
    uint64_t list{},generation{},nativeGeneration{},token{},context{},registry{},rows{},markers{};
    uint32_t frame{},side{},sourceRow{},exitRow{},proof{};HRESULT closeResult=E_PENDING;Failure failure{};
    bool whole{},closed{},omKnown{};std::array<uint32_t,9> om{};
    bool operator==(const ReceiptRecord&)const=default;
};
struct ReceiptKey {
    uint64_t epoch{},metadataVersion{},queue{},thread{},scope{};uint32_t frame{};
    std::array<ReceiptRecord,2> sources;
    bool operator==(const ReceiptKey&)const=default;
};
struct ReceiptStatus {ReceiptKey key;bool observing{},finished{},unavailable{};Failure failure{};};
#if CVR_RUST_VRIK
// The invocation crosses as its identity and its lists borrowed for the call.
struct RawInvocation {uint64_t thread{},queue{},listsAddress{};const uint64_t* lists{};size_t count{};};
extern "C" {
bool cvr_ta_receipt_matches(const ReceiptKey*,const ReceiptStatus*);
bool cvr_ta_receipt_boundary(const submission_frontier::Boundary*,const RawInvocation*,const std::array<uint64_t,2>*);
}
inline bool ReceiptMatches(const ReceiptKey& saved,const ReceiptStatus& current){return cvr_ta_receipt_matches(&saved,&current);}
inline bool ReceiptBoundary(const submission_frontier::Boundary& boundary,const queue_residency::Invocation& invocation,
                            const std::array<uint64_t,2>& sources){
    const RawInvocation raw{invocation.thread,invocation.queue,invocation.listsAddress,invocation.lists.data(),invocation.lists.size()};
    return cvr_ta_receipt_boundary(&boundary,&raw,&sources);
}
#else
inline bool ReceiptMatches(const ReceiptKey& saved,const ReceiptStatus& current){
    if(!current.observing || current.finished || current.unavailable || current.failure!=Failure::None ||
        !saved.epoch || !saved.queue || !saved.thread || !saved.scope || !saved.frame || saved!=current.key)return false;
    for(uint32_t side=0;side<2;++side){const auto& source=saved.sources[side];
        if(!source.list || !source.generation || !source.nativeGeneration || !source.token || !source.context || !source.registry ||
            !source.whole || !source.closed || FAILED(source.closeResult) || source.failure!=Failure::None || source.proof ||
            source.frame!=saved.frame || source.side!=side || !source.rows || source.rows>AccessLimit || source.markers>MarkerLimit ||
            source.markers>source.rows || source.sourceRow>=source.exitRow || source.exitRow>=source.rows)return false;
    }
    return saved.sources[0].list!=saved.sources[1].list && saved.sources[0].token!=saved.sources[1].token;
}
inline bool ReceiptBoundary(const submission_frontier::Boundary& boundary,const queue_residency::Invocation& invocation,
                            const std::array<uint64_t,2>& sources){
    if(!boundary.probeEpoch || !boundary.batch || !boundary.invocation || !boundary.consumer || !boundary.graphEpoch ||
        !boundary.pairGeneration || !boundary.frameOwner || !boundary.fingerprint || !boundary.thread || !boundary.queue || !boundary.lists ||
        boundary.thread!=invocation.thread || boundary.queue!=invocation.queue || boundary.lists!=invocation.listsAddress ||
        boundary.count!=invocation.lists.size() || boundary.count<3 || boundary.count>submission_frontier::detail::MaxSubmitLists ||
        !sources[0] || !sources[1] || sources[0]==sources[1])return false;
    const auto first=std::min(boundary.sourceIndices[0],boundary.sourceIndices[1]),last=std::max(boundary.sourceIndices[0],boundary.sourceIndices[1]);
    if(last>=boundary.count || first+1!=last || boundary.afterIndex!=last+1 || boundary.afterIndex>=boundary.count)return false;
    for(uint32_t side=0;side<2;++side)if(invocation.lists[boundary.sourceIndices[side]]!=sources[side])return false;
    for(size_t i=0;i<invocation.lists.size();++i){
        if(!invocation.lists[i])return false;
        for(size_t j=0;j<i;++j)if(invocation.lists[i]==invocation.lists[j])return false;
    }
    return true;
}
#endif
enum class CreationMatch:uint32_t {Conflict,Same,Upgrade};
// Only an authenticated breadcrumb lease may supply the observed metadata.
// API placement is immutable while this exact resource is retained.
#if CVR_RUST_VRIK
extern "C" uint32_t cvr_ta_compare_creation(const Resource*,const Resource*);
inline CreationMatch CompareAuthenticatedCreation(const Resource& cached,const Resource& authenticated){
    return CreationMatch(cvr_ta_compare_creation(&cached,&authenticated));
}
#else
inline CreationMatch CompareAuthenticatedCreation(const Resource& cached,const Resource& authenticated){
    if(!cached.pointer || cached.pointer!=authenticated.pointer || cached.flags!=authenticated.flags ||
        !texture_read::detail::KnownAllocation(authenticated.allocation))return CreationMatch::Conflict;
    if(!cached.allocation.kind)return CreationMatch::Upgrade;
    const auto& a=cached.allocation;const auto& b=authenticated.allocation;
    return texture_read::detail::KnownAllocation(a) && a.kind==b.kind && a.heap==b.heap && a.offset==b.offset && a.bytes==b.bytes?
        CreationMatch::Same:CreationMatch::Conflict;
}
#endif
struct Event {
    Kind kind{};uint32_t resource=NoResource,other=NoResource,subresource=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    uint32_t before{},after{},flags{},proof{},tag{};
};
struct Watch {Resource resource;uint32_t count{};std::array<uint32_t,2> states{};};
struct Check {
    uint64_t rows{},reads{},writes{},transitions{},aliases{},shaders{},excludedShaders{},proofReads{},proofScopes{};
    Failure failure{};uint32_t failureRow=UINT32_MAX,resource=NoResource,phase{};
    std::array<std::array<uint32_t,2>,4> finalStates{};
    bool Reject(Failure why,uint32_t row,uint32_t target=NoResource){
        if(failure==Failure::None){failure=why;failureRow=row;resource=target;}return false;
    }
};
#if CVR_RUST_VRIK
extern "C" bool cvr_ta_check_window(const Event*,size_t,const Resource*,size_t,const std::array<Watch,4>*,bool,Check*);
inline bool CheckWindow(std::span<const Event> events,std::span<const Resource> resources,
                        const std::array<Watch,4>& watched,bool ownSource,Check& out){
    return cvr_ta_check_window(events.data(),events.size(),resources.data(),resources.size(),&watched,ownSource,&out);
}
static_assert(sizeof(RecordingSlot)==40 && sizeof(RecordingPlan)==40 && sizeof(PressurePlan)==32 && sizeof(Resource)==48 &&
    sizeof(ReceiptRecord)==136 && offsetof(ReceiptRecord,om)==96 && sizeof(ReceiptKey)==320 && sizeof(ReceiptStatus)==328 &&
    sizeof(submission_frontier::Boundary)==104 && sizeof(Event)==36 && sizeof(Watch)==64 && sizeof(Check)==120,
    "mirrored by target_access.rs");
#else
inline bool CheckWindow(std::span<const Event> events,std::span<const Resource> resources,
                        const std::array<Watch,4>& watched,bool ownSource,Check& out){
    out={};bool source=!ownSource,exit=!ownSource;uint32_t proof{};bool proofTouched{};
    std::array<bool,4> allowed{};std::array<std::array<uint32_t,2>,4> proofStates{};
    for(uint32_t t=0;t<watched.size();++t){const auto& watch=watched[t];
        if(!watch.resource.pointer || !texture_read::detail::KnownAllocation(watch.resource.allocation) ||
           !watch.count || watch.count>2 || (watch.resource.flags&D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS))
            return out.Reject(Failure::Metadata,0,t);
        for(uint32_t p=0;p<watch.count;++p){
            if(!target_handoff::detail::StateKnown(watch.states[p]))return out.Reject(Failure::State,0,t);
            out.finalStates[t][p]=watch.states[p];
        }
    }
    const auto resource=[&](uint32_t index)->const Resource*{return index<resources.size()?&resources[index]:nullptr;};
    const auto overlaps=[&](uint32_t t,const Resource* value){
        return value && (value->pointer==watched[t].resource.pointer ||
            texture_read::detail::CouldAlias(watched[t].resource.allocation,value->allocation));
    };
    for(uint32_t row=0;row<events.size();++row){const auto& event=events[row];
        if(event.kind==Kind::Source){
            if(!ownSource)continue;
            if(source)return out.Reject(Failure::Source,row);source=true;continue;
        }
        if(!source)continue;
        if(event.kind==Kind::Exit){
            if(!ownSource)continue;
            if(exit || proof)return out.Reject(Failure::MissingExit,row);exit=true;continue;
        }
        const bool checking=exit;
        if(checking)++out.rows;
        const auto* a=resource(event.resource);const auto* b=resource(event.other);
        if((event.resource!=NoResource && !a) || (event.other!=NoResource && !b))return out.Reject(Failure::Metadata,row);
        if(event.kind==Kind::ProofBegin){
            if(!checking)continue;
            if(!event.proof || !a || (proof && (proof!=event.proof || proofTouched)))return out.Reject(Failure::Proof,row);
            if(!proof){proof=event.proof;proofStates=out.finalStates;allowed={};++out.proofScopes;}
            for(uint32_t t=0;t<watched.size();++t)if(overlaps(t,a)){
                if(a->pointer!=watched[t].resource.pointer)return out.Reject(Failure::Alias,row,t);
                allowed[t]=true;
            }
            continue;
        }
        if(event.kind==Kind::ProofEnd){
            if(!checking)continue;
            if(!proof || event.proof!=proof || !event.flags || out.finalStates!=proofStates)return out.Reject(Failure::Proof,row);
            proof=0;proofTouched=false;continue;
        }
        if(proof)proofTouched=true;
        if(event.kind==Kind::Unknown){if(checking)return out.Reject(proof?Failure::Proof:Failure::Unknown,row);continue;}
        if(event.kind==Kind::Shader){
            if(!checking)continue;++out.shaders;
            if(proof)return out.Reject(Failure::Proof,row);
            for(uint32_t t=0;t<watched.size();++t)for(uint32_t p=0;p<watched[t].count;++p){
                const auto state=out.finalStates[t][p];
                if(state!=D3D12_RESOURCE_STATE_RENDER_TARGET && state!=D3D12_RESOURCE_STATE_DEPTH_WRITE)
                    return out.Reject(Failure::Shader,row,t);
            }
            ++out.excludedShaders;continue;
        }
        if(event.kind==Kind::Alias){
            if(checking)++out.aliases;
            if(proof)return out.Reject(Failure::Proof,row);
            for(uint32_t t=0;t<watched.size();++t){
                if((!a && !b && watched[t].resource.allocation.kind!=1) || overlaps(t,a) || overlaps(t,b))
                    return out.Reject(Failure::Alias,row,t);
            }
            continue;
        }
        if(event.kind==Kind::Uav){
            if(proof)return out.Reject(Failure::Proof,row);
            if(checking && (!a || std::any_of(watched.begin(),watched.end(),[&](const Watch& watch){
                return a->pointer==watch.resource.pointer || texture_read::detail::CouldAlias(watch.resource.allocation,a->allocation);
            })))return out.Reject(proof?Failure::Proof:Failure::Unknown,row);
            continue;
        }
        if(!a)return out.Reject(Failure::Metadata,row);
        if(checking){
            if(event.kind==Kind::Read)++out.reads;
            else if(event.kind==Kind::Write)++out.writes;
            else if(event.kind==Kind::Transition)++out.transitions;
        }
        for(uint32_t t=0;t<watched.size();++t){if(!overlaps(t,a))continue;
            if(a->pointer!=watched[t].resource.pointer)return out.Reject(Failure::Alias,row,t);
            if(event.kind==Kind::Transition){
                if(checking && !ownSource && !proof)return out.Reject(Failure::Transition,row,t);
                if(event.flags || !target_handoff::detail::StateKnown(event.before) || !target_handoff::detail::StateKnown(event.after) ||
                   (event.subresource!=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES && event.subresource>=watched[t].count))
                    return out.Reject(Failure::State,row,t);
                if(proof && (!allowed[t] || (event.before!=D3D12_RESOURCE_STATE_COPY_SOURCE && event.after!=D3D12_RESOURCE_STATE_COPY_SOURCE)))
                    return out.Reject(Failure::Proof,row,t);
                const auto first=event.subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?0:event.subresource;
                const auto end=event.subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?watched[t].count:event.subresource+1;
                for(uint32_t p=first;p<end;++p){
                    if(out.finalStates[t][p]!=event.before)return out.Reject(Failure::State,row,t);
                    out.finalStates[t][p]=event.after;
                }
            }else if(checking && event.kind==Kind::Write)return out.Reject(proof?Failure::Proof:Failure::Write,row,t);
            else if(checking && event.kind==Kind::Read){
                if(!proof || !allowed[t])return out.Reject(Failure::Read,row,t);
                if(event.subresource!=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES && event.subresource>=watched[t].count)
                    return out.Reject(Failure::State,row,t);
                const auto first=event.subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?0:event.subresource;
                const auto end=event.subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?watched[t].count:event.subresource+1;
                for(uint32_t p=first;p<end;++p)if(out.finalStates[t][p]!=D3D12_RESOURCE_STATE_COPY_SOURCE)
                    return out.Reject(Failure::State,row,t);
                ++out.proofReads;
            }
        }
    }
    if(!source)return out.Reject(Failure::Source,uint32_t(events.size()));
    if(!exit)return out.Reject(Failure::MissingExit,uint32_t(events.size()));
    if(proof)return out.Reject(Failure::Proof,uint32_t(events.size()));
    return true;
}
#endif
}
// Arm before the source recordings' successful creation/Reset. Events before a
// Reset cannot establish whole-recording coverage. Finish preserves the report.
void Arm();
bool Observing()noexcept;
// A missed command-list hook permanently invalidates observation coverage for
// this process. Clear/Arm cannot repair an already unobserved API surface.
void ObserverUnavailable();
void Finish();
void Clear();
void Reset(ID3D12GraphicsCommandList*);
void Close(ID3D12GraphicsCommandList*,HRESULT);
void Read(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t kind=0,UINT subresource=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
          uint64_t offset=0,uint64_t bytes=UINT64_MAX);
void Written(ID3D12GraphicsCommandList*,ID3D12Resource*,uint32_t kind=0,UINT subresource=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
             uint64_t offset=0,uint64_t bytes=UINT64_MAX);
// Resolves a native diagnostic breadcrumb outside the journal mutex, then
// rechecks the exact journal epoch/generation. Failure records Unknown tag13.
bool ImmediateWrite(ID3D12GraphicsCommandList2*,UINT,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER*,
                    const D3D12_WRITEBUFFERIMMEDIATE_MODE*,uintptr_t caller);
void Barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
// Actual operands, including private/internal commands. Draw shader markers
// include the current OM writes; dispatch markers do not assume any OM writes.
void Targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*);
void Shader(ID3D12GraphicsCommandList*,bool graphics);
void ClearView(ID3D12GraphicsCommandList*,D3D12_CPU_DESCRIPTOR_HANDLE,bool depth);
void Unknown(ID3D12GraphicsCommandList*,uint32_t kind);
bool MarkSource(ID3D12GraphicsCommandList*,const native_recording::Lease&,uint32_t frame,uint32_t side);
void MarkExit(ID3D12GraphicsCommandList*);
// Only retained Proof may use this scope. Complete is required; destruction
// without completion invalidates it. Source reads and balanced transitions are
// allowed, never native writes, aliases, shaders or opaque commands.
class DiagnosticProofScope {
public:
    DiagnosticProofScope(ID3D12GraphicsCommandList*,std::span<const Microsoft::WRL::ComPtr<ID3D12Resource>>);
    ~DiagnosticProofScope();
    DiagnosticProofScope(const DiagnosticProofScope&)=delete;
    DiagnosticProofScope& operator=(const DiagnosticProofScope&)=delete;
    void Complete();
private:
    ID3D12GraphicsCommandList* list_{};uint64_t generation_{};uint32_t proof_{};bool done_{};
};
// Observational noninterference only. SnapshotSources and FindBoundary must have
// succeeded on this still-pending native submission. No allocation ownership,
// residency, native output copy or original draw suppression is authorized.
bool Query(ID3D12CommandQueue*,const std::array<target_handoff::SourceSnapshot,2>&,
           const submission_frontier::Boundary&);
class Receipt {
    struct Data;std::shared_ptr<const Data> data_;
    friend bool Query(ID3D12CommandQueue*,const std::array<target_handoff::SourceSnapshot,2>&,
                      const submission_frontier::Boundary&,const queue_residency::Scope&,Receipt&);
    friend bool Validate(const Receipt&,ID3D12CommandQueue*,uint32_t);
    friend bool Describe(const Receipt&,ID3D12CommandQueue*,uint32_t,std::array<target_handoff::SourceSnapshot,2>&);
public:explicit operator bool()const{return bool(data_);}
};
// Authenticates the exact closed source journals and native batch while both
// source submission tokens are pending. The receipt may survive prefix submit,
// but only inside this same live synchronous scope and journal epoch.
bool Query(ID3D12CommandQueue*,const std::array<target_handoff::SourceSnapshot,2>&,
           const submission_frontier::Boundary&,const queue_residency::Scope&,Receipt&);
bool Validate(const Receipt&,ID3D12CommandQueue*,uint32_t frame);
bool Describe(const Receipt&,ID3D12CommandQueue*,uint32_t frame,std::array<target_handoff::SourceSnapshot,2>&);
// A receipt proves this access window only. Native allocation lifetime, queue
// residency, output-copy admission and single-use submission are separate.
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_TargetAccessSeq;
// Schema 3: armed, finished, epoch, recordings, resources, accessRows, markers,
// resets, closes, sources, exits, queries, qualified, failure, permission(0).
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetAccessStats[16];
// Two rows: frame,side,list,journalGeneration,nativeGeneration,token,wholeCoverage,
// closed,closeHRESULT,firstRow,rowCount,sourceRow,exitRow,failure,sourceIndex,qualified.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetAccessSources[32];
// Two rows: side,phase(0 own suffix/1 later whole),rows,reads,writes,transitions,
// aliases,shaders,excludedShaders,proofReads,proofScopes,failure,failureRow,
// failureTarget,qualified,permission(0). Earlier source aggregates both checks.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetAccessChecks[32];
// First failed event: actual native side,list,journalGeneration,row,Kind,tag,
// resource pointer,other pointer. For cross checks this is the later recording,
// not the earlier watched-target side. Kind UINT64_MAX means no in-range event.
// Numeric evidence survives Finish; Clear resets all eight values.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetAccessFailureEvent[8];
// Attempts,resolved,fallbackUnknown,staleRecording,metadataConflicts,
// lastResolverFailure,lastDestination,lastResource. Protected by TargetAccessSeq.
__declspec(dllexport) extern uint64_t CyberpunkVR_TargetAccessBreadcrumbStats[8];
}
