#pragma once
#include "Render/BorrowedSceneSections.hpp"
#include "Render/BothEyesSceneExpansion.hpp"
#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <map>
#include <memory>
#include <span>

namespace cvr::stereo::owned_graph_execution {
inline constexpr size_t MaxConsumers=256,MaxOwnerBindings=128;
struct Identity {
    uint64_t consumerId{},epoch{},pairGeneration{},sourceGraph{},executionGraph{},frameOwner{},prerequisiteGroup{};
    bool operator==(const Identity&)const=default;
};
struct OwnerGenerationBinding {
    // protectionReceipt identifies caller-proved reader protection or ownership.
    // Its presence is a binding fact; this helper does not acquire that protection.
    uint64_t ownerHeader{},generationId{},bornMutationEpoch{},protectionReceipt{};
    bool operator==(const OwnerGenerationBinding&)const=default;
};
struct AdmissionFacts {
    bool exactReferenceCompiled{},sourceMappingAudited{},definitionProtectionAudited{};
};
// Caller-computed audited facts, not native lifetime or execution proof by
// themselves. Expanded admission also verifies its typed source/view/plan joins.
struct ExpansionAdmissionFacts {
    bool sourceMappingAudited{},definitionProtectionAudited{},nativeCodeGuarded{},nativeContextOrderCompiled{};
};
enum class Phase:uint32_t {Prepared,Published,RollbackRequired,Escaped,Cancelled,Quarantined};
enum class Path:uint32_t {Declaration,Execution};
enum class Failure:uint32_t {
    None,Arguments,Sequence,Capacity,Allocation,Admission,Bindings,Identity,ProducerScope,
    Order,Publication,Rollback,Construct,TerminalGroup,Duplicate,Incomplete,MissingProof
};

class Ledger;
// The caller serializes every method and supplies audited facts. This object
// neither owns a native lease nor reads, writes, waits, or proves native ownership.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/owned_graph_execution.rs (tools/single_pass_tests/owned_graph_execution_state_test
// runs its cases against them in its Rust variant). The members stay here (the ledger's map owns them), mirrored there.
namespace detail {
extern "C" {
uint64_t cvr_oge_state(void*,uint32_t,const Identity*,uint32_t,uint64_t,bool,bool);
bool cvr_oge_begin_admission(const Identity*,uint64_t,uint64_t,Failure*,uint64_t*);
bool cvr_oge_verified_bindings(const OwnerGenerationBinding*,size_t);
}
}
class State {
public:
    State(const State&)=delete;
    State& operator=(const State&)=delete;
    State(State&&)=delete;
    State& operator=(State&&)=delete;

    const Identity& FrozenIdentity()const noexcept{return identity_;}
    std::span<const OwnerGenerationBinding> OwnerBindings()const noexcept{return {bindings_.data(),bindingCount_};}
    Phase CurrentPhase()const noexcept{return phase_;}
    Failure Rejection()const noexcept{return failure_;}
    bool Selected()const noexcept{return selected_;}
    bool Escaped()const noexcept{return escaped_;}
    bool ProducerOpen()const noexcept{return producerOpen_;}
    bool ConstructVerified()const noexcept{return constructVerified_;}
    bool Quarantined()const noexcept{return phase_==Phase::Quarantined;}

    // Only the same producer may access the slot before invoking originalConstruct.
    // Clear the address before that call; callbacks and polling get zero thereafter.
    uint64_t PreconstructGraphSlot(uint64_t producerScope)const noexcept{return Get(0,nullptr,0,producerScope);}
    bool CanRollback(const Identity& proof,uint64_t producerScope)const noexcept{return Get(1,&proof,0,producerScope)!=0;}

    // Record every publication attempt immediately, including uncertain writes.
    // A failed readback still requires verified rollback before fallback or release.
    bool RecordPublication(const Identity& proof,uint64_t producerScope,bool ownedSlotVerified)noexcept{
        return Call(10,&proof,0,producerScope,ownedSlotVerified);
    }
    bool RecordRollback(const Identity& proof,uint64_t producerScope,bool originalSlotVerified)noexcept{
        return Call(11,&proof,0,producerScope,originalSlotVerified);
    }
    // Call immediately before originalConstruct. Partial construction or an
    // exception cannot undo escape, even if post-call payload verification fails.
    bool MarkConstructInvoked(const Identity& proof,uint64_t producerScope)noexcept{return Call(12,&proof,0,producerScope);}
    bool RecordConstruct(const Identity& proof,bool payloadMatched,bool prerequisiteConsumed)noexcept{
        return Call(13,&proof,0,0,payloadMatched,prerequisiteConsumed);
    }

    // One event per aggregate path, published before its callbacks can enter.
    // The pinned group must cover every native descendant of this path.
    bool RecordQueued(Path path,const Identity& proof,uint64_t terminalGroup,bool terminalPinned)noexcept{
        return Call(14,&proof,uint32_t(path),terminalGroup,terminalPinned);
    }
    bool RecordOriginalReturned(Path path,const Identity& proof,uint64_t terminalGroup)noexcept{
        return Call(15,&proof,uint32_t(path),terminalGroup);
    }
    // A zero pending observation is insufficient before enqueue, producer close,
    // and original return. Pending work is a normal incomplete probe, not failure.
    bool ObserveCompletion(Path path,const Identity& proof,uint64_t terminalGroup,bool pinnedGroupComplete)noexcept{
        return Call(16,&proof,uint32_t(path),terminalGroup,pinnedGroupComplete);
    }
    bool RecordProducerClosed(const Identity& proof,uint64_t producerScope)noexcept{return Call(17,&proof,0,producerScope);}
    void CancelSelection()noexcept{Call(18);}
    void ReportFailure(Failure reason)noexcept{Call(19,nullptr,uint32_t(reason));}
    bool Queued(Path path)const noexcept{return Get(2,nullptr,uint32_t(path))!=0;}
    bool OriginalReturned(Path path)const noexcept{return Get(3,nullptr,uint32_t(path))!=0;}
    bool Complete(Path path)const noexcept{return Get(4,nullptr,uint32_t(path))!=0;}
    uint64_t TerminalGroup(Path path)const noexcept{return Get(5,nullptr,uint32_t(path));}
    bool CanReleaseOwnedStorage()const noexcept{return Get(6)!=0;}
private:
    friend class Ledger;
    struct Reader {uint64_t group{};bool queued{},originalReturned{},complete{};};
    State(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,uint64_t scope,uint64_t slot)noexcept:
        identity_(identity),bindingCount_(bindings.size()),producerScope_(scope),graphSlot_(slot){
        std::copy(bindings.begin(),bindings.end(),bindings_.begin());
    }
    // Used by the typed expansion admission, which stays here.
    static bool Pointer(uint64_t address,uint64_t bytes)noexcept{return address && !(address&7) && address<=UINT64_MAX-bytes;}
    uint64_t Get(uint32_t which,const Identity* proof=nullptr,uint32_t path=0,uint64_t scope=0)const noexcept{
        return detail::cvr_oge_state(const_cast<State*>(this),which,proof,path,scope,false,false);
    }
    bool Call(uint32_t which,const Identity* proof=nullptr,uint32_t path=0,uint64_t scope=0,bool flag=false,bool flag2=false)noexcept{
        return detail::cvr_oge_state(this,which,proof,path,scope,flag,flag2)!=0;
    }
    const Identity identity_;
    std::array<OwnerGenerationBinding,MaxOwnerBindings> bindings_{};
    size_t bindingCount_{};
    uint64_t producerScope_{},graphSlot_{};
    std::array<Reader,2> paths_{};
    Phase phase_=Phase::Prepared;
    Failure failure_=Failure::None;
    bool selected_=true,escaped_{},producerOpen_=true,constructVerified_{};
};
static_assert(sizeof(Identity)==56 && sizeof(OwnerGenerationBinding)==32 && sizeof(State)==4224,"mirrored by owned_graph_execution.rs");
#else
class State {
public:
    State(const State&)=delete;
    State& operator=(const State&)=delete;
    State(State&&)=delete;
    State& operator=(State&&)=delete;

    const Identity& FrozenIdentity()const noexcept{return identity_;}
    std::span<const OwnerGenerationBinding> OwnerBindings()const noexcept{return {bindings_.data(),bindingCount_};}
    Phase CurrentPhase()const noexcept{return phase_;}
    Failure Rejection()const noexcept{return failure_;}
    bool Selected()const noexcept{return selected_;}
    bool Escaped()const noexcept{return escaped_;}
    bool ProducerOpen()const noexcept{return producerOpen_;}
    bool ConstructVerified()const noexcept{return constructVerified_;}
    bool Quarantined()const noexcept{return phase_==Phase::Quarantined;}

    // Only the same producer may access the slot before invoking originalConstruct.
    // Clear the address before that call; callbacks and polling get zero thereafter.
    uint64_t PreconstructGraphSlot(uint64_t producerScope)const noexcept{
        return producerOpen_ && !escaped_ && producerScope==producerScope_ &&
            (phase_==Phase::Prepared || phase_==Phase::Published || phase_==Phase::RollbackRequired)?graphSlot_:0;
    }
    bool CanRollback(const Identity& proof,uint64_t producerScope)const noexcept{
        return proof==identity_ && PreconstructGraphSlot(producerScope) &&
            (phase_==Phase::Published || phase_==Phase::RollbackRequired);
    }

    // Record every publication attempt immediately, including uncertain writes.
    // A failed readback still requires verified rollback before fallback or release.
    bool RecordPublication(const Identity& proof,uint64_t producerScope,bool ownedSlotVerified)noexcept{
        if(phase_!=Phase::Prepared)return Quarantine(Failure::Order);
        phase_=Phase::RollbackRequired;
        if(!CheckIdentity(proof))return false;
        if(!CheckScope(producerScope))return false;
        if(!ownedSlotVerified)return Reject(Failure::Publication);
        phase_=Phase::Published;failure_=Failure::None;return true;
    }
    bool RecordRollback(const Identity& proof,uint64_t producerScope,bool originalSlotVerified)noexcept{
        if(!CheckIdentity(proof))return false;
        if(!CanRollback(proof,producerScope))return Reject(escaped_?Failure::Order:Failure::ProducerScope);
        if(!originalSlotVerified)return Quarantine(Failure::Rollback);
        phase_=Phase::Cancelled;graphSlot_=0;selected_=false;failure_=Failure::None;return true;
    }
    // Call immediately before originalConstruct. Partial construction or an
    // exception cannot undo escape, even if post-call payload verification fails.
    bool MarkConstructInvoked(const Identity& proof,uint64_t producerScope)noexcept{
        if(!CheckIdentity(proof))return false;
        if(!CheckScope(producerScope))return false;
        if(phase_!=Phase::Published)return Reject(Failure::Order);
        escaped_=true;phase_=Phase::Escaped;graphSlot_=0;failure_=Failure::None;return true;
    }
    bool RecordConstruct(const Identity& proof,bool payloadMatched,bool prerequisiteConsumed)noexcept{
        if(!CheckIdentity(proof))return false;
        if(phase_!=Phase::Escaped)return Reject(Failure::Order);
        if(constructVerified_)return Reject(Failure::Duplicate);
        if(!payloadMatched || !prerequisiteConsumed)return Reject(Failure::Construct);
        constructVerified_=true;failure_=Failure::None;return true;
    }

    // One event per aggregate path, published before its callbacks can enter.
    // The pinned group must cover every native descendant of this path.
    bool RecordQueued(Path path,const Identity& proof,uint64_t terminalGroup,bool terminalPinned)noexcept{
        if(!CheckIdentity(proof))return false;
        if(!ValidPath(path))return Reject(Failure::Arguments);
        if(phase_!=Phase::Escaped || !constructVerified_ || !producerOpen_)return Reject(Failure::Order);
        auto& reader=paths_[Index(path)];
        if(reader.queued)return Reject(Failure::Duplicate);
        if(!Pointer(terminalGroup,0x20) || terminalGroup==identity_.prerequisiteGroup ||
            terminalGroup==identity_.frameOwner || !terminalPinned)return Reject(Failure::TerminalGroup);
        reader.group=terminalGroup;reader.queued=true;failure_=Failure::None;return true;
    }
    bool RecordOriginalReturned(Path path,const Identity& proof,uint64_t terminalGroup)noexcept{
        if(!CheckPath(path,proof,terminalGroup))return false;
        auto& reader=paths_[Index(path)];
        if(reader.originalReturned)return Reject(Failure::Duplicate);
        reader.originalReturned=true;failure_=Failure::None;return true;
    }
    // A zero pending observation is insufficient before enqueue, producer close,
    // and original return. Pending work is a normal incomplete probe, not failure.
    bool ObserveCompletion(Path path,const Identity& proof,uint64_t terminalGroup,bool pinnedGroupComplete)noexcept{
        if(!CheckIdentity(proof))return false;
        if(!ValidPath(path))return Reject(Failure::Arguments);
        if(phase_!=Phase::Escaped)return Reject(Failure::Order);
        auto& reader=paths_[Index(path)];
        if(reader.queued && reader.group!=terminalGroup)return Reject(Failure::TerminalGroup);
        if(!reader.queued || producerOpen_ || !reader.originalReturned){failure_=Failure::Incomplete;return false;}
        if(!pinnedGroupComplete){failure_=Failure::Incomplete;return false;}
        reader.complete=true;failure_=Failure::None;return true;
    }
    bool RecordProducerClosed(const Identity& proof,uint64_t producerScope)noexcept{
        if(!CheckIdentity(proof))return false;
        if(!CheckScope(producerScope))return false;
        producerOpen_=false;graphSlot_=0;
        if(phase_==Phase::Prepared){phase_=Phase::Cancelled;selected_=false;failure_=Failure::None;return true;}
        if(phase_==Phase::Cancelled)return true;
        if(phase_!=Phase::Escaped || !constructVerified_ || !paths_[0].queued || !paths_[1].queued)
            return Quarantine(Failure::MissingProof);
        failure_=Failure::None;return true;
    }
    void CancelSelection()noexcept{
        selected_=false;
        if(phase_==Phase::Prepared){phase_=Phase::Cancelled;graphSlot_=0;}
        else if(phase_==Phase::Published)phase_=Phase::RollbackRequired;
    }
    void ReportFailure(Failure reason)noexcept{Reject(reason==Failure::None?Failure::MissingProof:reason);}
    bool Queued(Path path)const noexcept{return ValidPath(path) && paths_[Index(path)].queued;}
    bool OriginalReturned(Path path)const noexcept{return ValidPath(path) && paths_[Index(path)].originalReturned;}
    bool Complete(Path path)const noexcept{return ValidPath(path) && paths_[Index(path)].complete;}
    uint64_t TerminalGroup(Path path)const noexcept{return ValidPath(path)?paths_[Index(path)].group:0;}
    bool CanReleaseOwnedStorage()const noexcept{
        if(phase_==Phase::Cancelled)return !escaped_;
        return phase_==Phase::Escaped && escaped_ && constructVerified_ && !producerOpen_ &&
            std::all_of(paths_.begin(),paths_.end(),[](const Reader& reader){return reader.queued && reader.originalReturned && reader.complete;});
    }
private:
    friend class Ledger;
    struct Reader {uint64_t group{};bool queued{},originalReturned{},complete{};};
    State(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,uint64_t scope,uint64_t slot)noexcept:
        identity_(identity),bindingCount_(bindings.size()),producerScope_(scope),graphSlot_(slot){
        std::copy(bindings.begin(),bindings.end(),bindings_.begin());
    }
    static bool Pointer(uint64_t address,uint64_t bytes)noexcept{return address && !(address&7) && address<=UINT64_MAX-bytes;}
    static bool ValidPath(Path path)noexcept{return path==Path::Declaration || path==Path::Execution;}
    static size_t Index(Path path)noexcept{return size_t(path);}
    bool CheckIdentity(const Identity& proof)noexcept{
        if(phase_==Phase::Quarantined)return false;
        return proof==identity_ || Reject(Failure::Identity);
    }
    bool CheckScope(uint64_t scope)noexcept{
        return (producerOpen_ && scope==producerScope_) || Reject(Failure::ProducerScope);
    }
    bool CheckPath(Path path,const Identity& proof,uint64_t group)noexcept{
        if(!CheckIdentity(proof))return false;
        if(!ValidPath(path))return Reject(Failure::Arguments);
        if(phase_!=Phase::Escaped || !paths_[Index(path)].queued)return Reject(Failure::Order);
        return paths_[Index(path)].group==group || Reject(Failure::TerminalGroup);
    }
    bool Quarantine(Failure reason)noexcept{
        if(phase_!=Phase::Quarantined)failure_=reason;
        phase_=Phase::Quarantined;graphSlot_=0;selected_=false;return false;
    }
    bool Reject(Failure reason)noexcept{
        if(phase_==Phase::Quarantined)return false;
        if(escaped_)return Quarantine(reason);
        failure_=reason;
        if(phase_==Phase::Published || phase_==Phase::RollbackRequired)phase_=Phase::RollbackRequired;
        else if(phase_==Phase::Prepared){phase_=Phase::Cancelled;graphSlot_=0;selected_=false;}
        return false;
    }
    const Identity identity_;
    std::array<OwnerGenerationBinding,MaxOwnerBindings> bindings_{};
    size_t bindingCount_{};
    uint64_t producerScope_{},graphSlot_{};
    std::array<Reader,2> paths_{};
    Phase phase_=Phase::Prepared;
    Failure failure_=Failure::None;
    bool selected_=true,escaped_{},producerOpen_=true,constructVerified_{};
};
#endif

// This bounded registry owns only state records. Destroying it is not authority
// to destroy graph storage or native leases; the runtime must honor each state.
class Ledger {
public:
    Ledger()=default;
    Ledger(const Ledger&)=delete;
    Ledger& operator=(const Ledger&)=delete;
    Ledger(Ledger&&)=delete;
    Ledger& operator=(Ledger&&)=delete;
    State* Admit(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,AdmissionFacts facts,
                 uint64_t producerScope,uint64_t graphSlot)noexcept try{
        if(!BeginAdmission(identity,producerScope,graphSlot))return nullptr;
        failure_=Failure::Admission;
        if(!facts.exactReferenceCompiled || !facts.sourceMappingAudited || !facts.definitionProtectionAudited)return nullptr;
        return AdmitVerified(identity,bindings,producerScope,graphSlot);
    }catch(...){failure_=Failure::Allocation;return nullptr;}
    State* AdmitExpanded(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,
                         const scene_expansion::Plan& plan,const borrowed_scene_sections::Sections& views,
                         const scene_sections::Snapshot& sections,const borrowed_scene_sections::CallerLifetime& lifetime,
                         const graph_snapshot::Snapshot& actualCompiled,uint64_t base,ExpansionAdmissionFacts facts,
                         uint64_t producerScope,uint64_t graphSlot)noexcept try{
        if(!BeginAdmission(identity,producerScope,graphSlot))return nullptr;
        failure_=Failure::Admission;
        if(!facts.sourceMappingAudited || !facts.definitionProtectionAudited || !facts.nativeCodeGuarded || !facts.nativeContextOrderCompiled ||
            !plan.Ready() || !views.Bound() || !views.Matches(sections,lifetime) ||
            !plan.MatchesSource(plan.Source(),base) || !plan.MatchesCompiled(actualCompiled) || plan.Definitions()!=views.Definitions())return nullptr;
        const auto& source=plan.Source();
        if(source.nodes.empty() || source.nodes.size()>=220 || actualCompiled.nodes.size()!=source.nodes.size()+1)return nullptr;
        const auto selected=std::find_if(source.nodes.begin(),source.nodes.end(),[&](const auto& node){return node.handle==plan.PrefixHandle();});
        if(selected==source.nodes.end() || *selected!=sections.RootIdentity() ||
            graph_snapshot::Field<uint64_t>(selected->bytes,16)!=lifetime.originalRoot)return nullptr;
        failure_=Failure::Identity;
        if(identity.consumerId!=lifetime.consumerId || identity.epoch!=lifetime.epoch || identity.pairGeneration!=lifetime.pairGeneration)return nullptr;
        failure_=Failure::Bindings;
        if(bindings.empty() || bindings.size()>MaxOwnerBindings)return nullptr;
        const OwnerGenerationBinding required{lifetime.ownerHeader,lifetime.ownerGeneration,lifetime.ownerBornEpoch,lifetime.protectionReceipt};
        size_t matches{};
        for(size_t i=0;i<bindings.size();++i){
            if(bindings[i]==required)++matches;
            for(size_t j=0;j<i;++j)if(bindings[j].protectionReceipt==bindings[i].protectionReceipt)return nullptr;
        }
        if(matches!=1)return nullptr;
        return AdmitVerified(identity,bindings,producerScope,graphSlot);
    }catch(...){failure_=Failure::Allocation;return nullptr;}
    State* AdmitExpanded(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,
                         const scene_expansion::BothEyesPlan& plan,const std::array<borrowed_scene_sections::Sections,2>& views,
                         const std::array<scene_sections::Snapshot,2>& sections,
                         const std::array<borrowed_scene_sections::CallerLifetime,2>& lifetimes,
                         const graph_snapshot::Snapshot& actualCompiled,uint64_t base,ExpansionAdmissionFacts facts,
                         uint64_t producerScope,uint64_t graphSlot)noexcept try{
        if(!BeginAdmission(identity,producerScope,graphSlot))return nullptr;
        failure_=Failure::Admission;
        if(!facts.sourceMappingAudited || !facts.definitionProtectionAudited || !facts.nativeCodeGuarded || !facts.nativeContextOrderCompiled ||
            !plan.Ready() || !plan.MatchesSource(plan.Source(),base) || !plan.MatchesCompiled(actualCompiled))return nullptr;
        const auto& source=plan.Source();
        if(source.nodes.size()<2 || source.nodes.size()>218 || actualCompiled.nodes.size()!=source.nodes.size()+2)return nullptr;
        std::array<uint64_t,4> clones{};
        for(size_t view=0;view<2;++view){
            if(!views[view].Bound() || !views[view].Matches(sections[view],lifetimes[view]) ||
                plan.Definitions(view)!=views[view].Definitions())return nullptr;
            // The second expansion changes adjacency in its intermediate source.
            // Join both captured roots to S0, never to that intermediate graph.
            const auto& selected=plan.SelectedSource(view);
            if(selected!=sections[view].RootIdentity() || graph_snapshot::Field<uint32_t>(selected.bytes,24)!=view ||
                graph_snapshot::Field<uint64_t>(selected.bytes,16)!=lifetimes[view].originalRoot)return nullptr;
            for(size_t part=0;part<2;++part){const auto object=plan.Definitions(view)[part].object;
                if(!State::Pointer(object,56))return nullptr;
                const auto index=2*view+part;
                for(size_t previous=0;previous<index;++previous)
                    if(object<clones[previous]+56 && clones[previous]<object+56)return nullptr;
                clones[index]=object;
            }
        }
        failure_=Failure::Identity;
        for(const auto& lifetime:lifetimes)
            if(identity.consumerId!=lifetime.consumerId || identity.epoch!=lifetime.epoch || identity.pairGeneration!=lifetime.pairGeneration)return nullptr;
        failure_=Failure::Bindings;
        if(bindings.empty() || bindings.size()>MaxOwnerBindings)return nullptr;
        for(size_t i=0;i<bindings.size();++i)
            for(size_t j=0;j<i;++j)if(bindings[j].protectionReceipt==bindings[i].protectionReceipt)return nullptr;
        for(const auto& lifetime:lifetimes){
            const OwnerGenerationBinding required{lifetime.ownerHeader,lifetime.ownerGeneration,lifetime.ownerBornEpoch,lifetime.protectionReceipt};
            if(std::count(bindings.begin(),bindings.end(),required)!=1)return nullptr;
        }
        return AdmitVerified(identity,bindings,producerScope,graphSlot);
    }catch(...){failure_=Failure::Allocation;return nullptr;}
    State* Find(uint64_t consumerId)noexcept{const auto found=states_.find(consumerId);return found==states_.end()?nullptr:found->second.get();}
    const State* Find(uint64_t consumerId)const noexcept{const auto found=states_.find(consumerId);return found==states_.end()?nullptr:found->second.get();}
    void CancelSelection()noexcept{if(auto* selected=Find(selectedId_))selected->CancelSelection();selectedId_=0;}
    bool Retire(uint64_t consumerId)noexcept{
        const auto* state=Find(consumerId);if(!state || !state->CanReleaseOwnedStorage()){failure_=Failure::Incomplete;return false;}
        states_.erase(consumerId);if(selectedId_==consumerId)selectedId_=0;failure_=Failure::None;return true;
    }
    size_t Size()const noexcept{return states_.size();}
    uint64_t LastConsumerId()const noexcept{return lastConsumerId_;}
    uint64_t SelectedId()const noexcept{return selectedId_;}
    Failure Rejection()const noexcept{return failure_;}
private:
#if CVR_RUST_VRIK
    // The consumer id is consumed even when the rest is refused (Rust's rule).
    bool BeginAdmission(const Identity& identity,uint64_t producerScope,uint64_t graphSlot)noexcept{
        return detail::cvr_oge_begin_admission(&identity,producerScope,graphSlot,&failure_,&lastConsumerId_);
    }
    // Only the corresponding reference or typed expansion proof reaches here.
    State* AdmitVerified(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,
                         uint64_t producerScope,uint64_t graphSlot){
        failure_=Failure::Bindings;
        if(!detail::cvr_oge_verified_bindings(bindings.data(),bindings.size()))return nullptr;
        failure_=Failure::Capacity;if(states_.size()>=MaxConsumers)return nullptr;
        auto state=std::unique_ptr<State>(new State(identity,bindings,producerScope,graphSlot));auto* value=state.get();
        states_.emplace(identity.consumerId,std::move(state));
        if(auto* previous=Find(selectedId_))previous->CancelSelection();
        selectedId_=identity.consumerId;failure_=Failure::None;return value;
    }
#else
    bool BeginAdmission(const Identity& identity,uint64_t producerScope,uint64_t graphSlot)noexcept{
        failure_=Failure::Sequence;
        if(!identity.consumerId || identity.consumerId<=lastConsumerId_)return false;
        lastConsumerId_=identity.consumerId;
        failure_=Failure::Arguments;
        return identity.epoch && identity.pairGeneration && producerScope && State::Pointer(graphSlot,8) &&
            State::Pointer(identity.sourceGraph,0x70) && State::Pointer(identity.executionGraph,0x70) &&
            identity.sourceGraph!=identity.executionGraph && State::Pointer(identity.frameOwner,12) &&
            State::Pointer(identity.prerequisiteGroup,0x20) && identity.frameOwner!=identity.prerequisiteGroup;
    }
    // Only the corresponding reference or typed expansion proof reaches here.
    State* AdmitVerified(const Identity& identity,std::span<const OwnerGenerationBinding> bindings,
                         uint64_t producerScope,uint64_t graphSlot){
        failure_=Failure::Bindings;
        if(bindings.size()>MaxOwnerBindings)return nullptr;
        for(size_t i=0;i<bindings.size();++i){const auto& value=bindings[i];
            if(!State::Pointer(value.ownerHeader,16) || !value.generationId || !value.bornMutationEpoch || !value.protectionReceipt)return nullptr;
            for(size_t j=0;j<i;++j)if(bindings[j].ownerHeader==value.ownerHeader || bindings[j].generationId==value.generationId)return nullptr;
        }
        failure_=Failure::Capacity;if(states_.size()>=MaxConsumers)return nullptr;
        auto state=std::unique_ptr<State>(new State(identity,bindings,producerScope,graphSlot));auto* value=state.get();
        states_.emplace(identity.consumerId,std::move(state));
        if(auto* previous=Find(selectedId_))previous->CancelSelection();
        selectedId_=identity.consumerId;failure_=Failure::None;return value;
    }
#endif
    std::map<uint64_t,std::unique_ptr<State>> states_;
    uint64_t lastConsumerId_{},selectedId_{};
    Failure failure_=Failure::None;
};
}
