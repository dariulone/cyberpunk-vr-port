#pragma once
#include <cstddef>
#include <cstdint>
#include <map>
#include <vector>

namespace cvr::stereo::definition_generations {
inline constexpr size_t MaxConsumers=256,MaxGenerations=4096,MaxOwners=4096;
enum class Failure:uint32_t {
    None,Arguments,Sequence,Mutation,Generation,Epoch,Duplicate,Consumer,
    Capacity,SourceClosed,TerminalProof,Unsupported,Detach,PriorCoverage,
    Incomplete,Retirement,Allocation,NoSourceProof,Unsealed
};
enum class Admission:uint32_t {Source,Execution};
struct HeaderObservation {
    uint64_t ownerHeader{},bornMutationEpoch{},sequenceBefore{},sequenceAfter{};
    bool mutationInFlight{};
};
struct AggregateTerminalProof {
    uint64_t consumerId{},requestEpoch{},pairGeneration{};
    bool originalReturned{},aggregateComplete{};
};
struct Generation {
    uint64_t id{},ownerHeader{},bornMutationEpoch{},closedMutationEpoch{},closedAtConsumer{};
    uint64_t boundConsumers{},completedConsumers{},unsupportedConsumers{},pendingConsumers{};
    bool allPriorConsumersAccounted{},sourceClosed{},detached{},unsupported{},retiring{};
    Failure unsupportedReason=Failure::None;
};

// Caller holds its short synchronization gate and supplies audited mutation and
// aggregate terminal evidence. No method reads/writes native memory or owns a
// pin/allocation. Header addresses are observations; only monotonic IDs bind use.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/definition_generations.rs through definition_generations_abi.rs
// (tools/single_pass_tests/definition_generations_test runs its cases against them in its Rust variant). The ledger
// lives in Rust, a generation boxed there (GetGeneration's pointer stays valid while it is tracked); it does not fail
// on allocation, where the C++ map insertions reported Allocation.
namespace detail {
extern "C" {
void* cvr_dg_ledger_new();
void cvr_dg_ledger_free(void*);
uint64_t cvr_dg_birth(void*,const HeaderObservation*,bool);
uint64_t cvr_dg_find(const void*,uint64_t,uint64_t);
const Generation* cvr_dg_get(const void*,uint64_t);
bool cvr_dg_consumer(void*,uint32_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint32_t);
bool cvr_dg_complete(void*,const AggregateTerminalProof*);
bool cvr_dg_generation(void*,uint32_t,uint64_t,uint64_t,uint64_t,uint32_t);
uint64_t cvr_dg_count(const void*,uint32_t);
}
}
static_assert(sizeof(HeaderObservation)==40 && sizeof(AggregateTerminalProof)==32 && sizeof(Generation)==88 &&
    offsetof(Generation,unsupportedReason)==80,"mirrored by native/definition_generations.rs");
class Ledger {
public:
    Ledger():ledger_(detail::cvr_dg_ledger_new()){}
    ~Ledger(){detail::cvr_dg_ledger_free(ledger_);}
    Ledger(const Ledger&)=delete;
    Ledger& operator=(const Ledger&)=delete;
    Ledger(Ledger&&)=delete;
    Ledger& operator=(Ledger&&)=delete;

    uint64_t BirthGeneration(const HeaderObservation& observation,bool allPriorConsumersAccounted=false){
        return detail::cvr_dg_birth(ledger_,&observation,allPriorConsumersAccounted);
    }
    uint64_t FindGeneration(uint64_t ownerHeader,uint64_t bornMutationEpoch)const{return detail::cvr_dg_find(ledger_,ownerHeader,bornMutationEpoch);}
    const Generation* GetGeneration(uint64_t generationId)const{return detail::cvr_dg_get(ledger_,generationId);}
    // Register before the first possible native cache/source access, not merely
    // at a later callback constructor. This class cannot prove that frontier.
    bool RegisterConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration){
        return ConsumerOp(0,consumerId,requestEpoch,pairGeneration);
    }
    bool RegisterProvisionalConsumer(uint64_t consumerId){return ConsumerOp(1,consumerId);}
    // The native root can advance request/pair identity before construction.
    // Its admission ID stays fixed; identity becomes immutable at this seal.
    bool SealProvisionalConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration){
        return ConsumerOp(2,consumerId,requestEpoch,pairGeneration);
    }
    bool HasConsumer(uint64_t consumerId)const{return ConsumerQuery(3,consumerId,0);}
    bool ConsumerBound(uint64_t consumerId,uint64_t generationId)const{return ConsumerQuery(4,consumerId,generationId);}
    bool BindConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration,
                      uint64_t generationId,uint64_t bornMutationEpoch,Admission admission=Admission::Source){
        return ConsumerOp(5,consumerId,requestEpoch,pairGeneration,generationId,bornMutationEpoch,uint32_t(admission));
    }
    bool CompleteConsumer(const AggregateTerminalProof& proof){return detail::cvr_dg_complete(ledger_,&proof);}
    bool UnsupportedConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration,
                             Failure reason=Failure::Unsupported){
        return ConsumerOp(6,consumerId,requestEpoch,pairGeneration,0,0,uint32_t(reason));
    }
    // A guarded native early exit can prove no cache/source access occurred.
    // Cancellation without that proof is not terminal or prior-use accounting.
    bool CancelUnboundConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration,bool nativeNoSourceProved=false){
        return ConsumerOp(7,consumerId,requestEpoch,pairGeneration,0,0,nativeNoSourceProved);
    }
    bool UnsupportedGeneration(uint64_t generationId,uint64_t bornMutationEpoch,Failure reason=Failure::Unsupported){
        return detail::cvr_dg_generation(ledger_,0,generationId,bornMutationEpoch,0,uint32_t(reason));
    }
    bool CloseSource(uint64_t generationId,uint64_t bornMutationEpoch,uint64_t closingMutationEpoch){
        return detail::cvr_dg_generation(ledger_,1,generationId,bornMutationEpoch,closingMutationEpoch,0);
    }
    bool VerifyPriorConsumers(uint64_t generationId,uint64_t bornMutationEpoch,bool allPriorConsumersAccounted){
        return detail::cvr_dg_generation(ledger_,2,generationId,bornMutationEpoch,0,allPriorConsumersAccounted);
    }
    bool VerifyDetach(uint64_t generationId,uint64_t bornMutationEpoch,bool originalOwnerDetached){
        return detail::cvr_dg_generation(ledger_,3,generationId,bornMutationEpoch,0,originalOwnerDetached);
    }
    // Moving a source-closed owner protects still-pending users. Destruction has
    // the additional terminal gates in ReadyToRetire; a mutex is not exclusivity.
    bool CanAdopt(uint64_t generationId,bool nativeExclusiveProved=false)const{return GenerationQuery(4,generationId,nativeExclusiveProved);}
    bool OwnershipEligible(uint64_t generationId)const{return GenerationQuery(5,generationId,false);}
    bool ReadyToRetire(uint64_t generationId)const{return GenerationQuery(6,generationId,false);}
    bool BeginRetire(uint64_t generationId,uint64_t bornMutationEpoch){return detail::cvr_dg_generation(ledger_,7,generationId,bornMutationEpoch,0,0);}
    bool FinishRetire(uint64_t generationId,uint64_t bornMutationEpoch){return detail::cvr_dg_generation(ledger_,8,generationId,bornMutationEpoch,0,0);}
    // Discard only observer metadata. Detached ownership and unresolved users
    // must survive; this neither retires nor destroys any native definitions.
    bool ForgetClosedObservation(uint64_t generationId,uint64_t bornMutationEpoch){
        return detail::cvr_dg_generation(ledger_,9,generationId,bornMutationEpoch,0,0);
    }
    size_t ActiveConsumers()const{return size_t(detail::cvr_dg_count(ledger_,0));}
    size_t ActiveGenerations()const{return size_t(detail::cvr_dg_count(ledger_,1));}
    size_t TrackedOwners()const{return size_t(detail::cvr_dg_count(ledger_,2));}
    uint64_t LastGenerationId()const{return detail::cvr_dg_count(ledger_,3);}
    uint64_t LastConsumerId()const{return detail::cvr_dg_count(ledger_,4);}
    uint64_t CompletedConsumers()const{return detail::cvr_dg_count(ledger_,5);}
    uint64_t UnsupportedConsumers()const{return detail::cvr_dg_count(ledger_,6);}
    uint64_t CancelledConsumers()const{return detail::cvr_dg_count(ledger_,7);}
    uint64_t RetiredGenerations()const{return detail::cvr_dg_count(ledger_,8);}
    uint64_t ForgottenObservations()const{return detail::cvr_dg_count(ledger_,9);}
    Failure Rejection()const{return Failure(detail::cvr_dg_count(ledger_,10));}
private:
    bool ConsumerOp(uint32_t which,uint64_t consumerId,uint64_t requestEpoch=0,uint64_t pairGeneration=0,
                    uint64_t a=0,uint64_t b=0,uint32_t value=0){
        return detail::cvr_dg_consumer(ledger_,which,consumerId,requestEpoch,pairGeneration,a,b,value);
    }
    bool ConsumerQuery(uint32_t which,uint64_t consumerId,uint64_t generationId)const{
        return detail::cvr_dg_consumer(ledger_,which,consumerId,0,0,generationId,0,0);
    }
    bool GenerationQuery(uint32_t which,uint64_t generationId,bool value)const{
        return detail::cvr_dg_generation(ledger_,which,generationId,0,0,value);
    }
    void* ledger_;
};
#else
class Ledger {
public:
    Ledger()=default;
    Ledger(const Ledger&)=delete;
    Ledger& operator=(const Ledger&)=delete;
    Ledger(Ledger&&)=delete;
    Ledger& operator=(Ledger&&)=delete;

    uint64_t BirthGeneration(const HeaderObservation& observation,bool allPriorConsumersAccounted=false)try{
        failure_=Failure::None;
        if(!observation.ownerHeader || (observation.ownerHeader&7) || !observation.bornMutationEpoch)
            return RejectId(Failure::Arguments);
        if(!observation.sequenceBefore || observation.sequenceBefore!=observation.sequenceAfter)
            return RejectId(Failure::Sequence);
        if(observation.mutationInFlight)return RejectId(Failure::Mutation);
        const auto owner=owners_.find(observation.ownerHeader);
        if(owner!=owners_.end()){
            if(observation.bornMutationEpoch<=owner->second.lastBornEpoch)return RejectId(Failure::Duplicate);
            if(owner->second.activeGeneration){const auto previous=generations_.find(owner->second.activeGeneration);
                if(previous!=generations_.end())MakeUnsupported(previous->second,Failure::Mutation);
                return RejectId(Failure::Mutation);}
            if(owner->second.lastClosedEpoch && observation.bornMutationEpoch<owner->second.lastClosedEpoch)
                return RejectId(Failure::Epoch);
        }
        if(generations_.size()>=MaxGenerations || (owner==owners_.end() && owners_.size()>=MaxOwners) || nextGeneration_==UINT64_MAX)
            return RejectId(Failure::Capacity);
        Generation value;
        value.id=nextGeneration_+1;value.ownerHeader=observation.ownerHeader;value.bornMutationEpoch=observation.bornMutationEpoch;
        value.allPriorConsumersAccounted=allPriorConsumersAccounted;
        const auto inserted=generations_.emplace(value.id,value);
        try{
            if(owner==owners_.end())owners_.emplace(observation.ownerHeader,Owner{observation.bornMutationEpoch,0,value.id});
            else{owner->second.lastBornEpoch=observation.bornMutationEpoch;owner->second.activeGeneration=value.id;}
        }catch(...){generations_.erase(inserted.first);throw;}
        nextGeneration_=value.id;return value.id;
    }catch(...){return RejectId(Failure::Allocation);}

    uint64_t FindGeneration(uint64_t ownerHeader,uint64_t bornMutationEpoch)const{
        const auto owner=owners_.find(ownerHeader);
        if(owner==owners_.end() || owner->second.lastBornEpoch!=bornMutationEpoch)return 0;
        return owner->second.activeGeneration;
    }
    const Generation* GetGeneration(uint64_t generationId)const{
        const auto found=generations_.find(generationId);return found==generations_.end()?nullptr:&found->second;
    }
    // Register before the first possible native cache/source access, not merely
    // at a later callback constructor. This class cannot prove that frontier.
    bool RegisterConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration)try{
        failure_=Failure::None;
        if(!consumerId || consumerId<=lastConsumer_)return Reject(Failure::Duplicate);
        // Consume the ID even if admission fails; it cannot identify a later root.
        lastConsumer_=consumerId;
        if(consumers_.size()>=MaxConsumers){RejectOpenGenerations(Failure::Capacity);return Reject(Failure::Capacity);}
        consumers_.emplace(consumerId,Consumer{requestEpoch,pairGeneration,{},false,false});return true;
    }catch(...){RejectOpenGenerations(Failure::Allocation);return Reject(Failure::Allocation);}
    bool RegisterProvisionalConsumer(uint64_t consumerId){
        if(!RegisterConsumer(consumerId,0,0))return false;
        consumers_.find(consumerId)->second.provisional=true;return true;
    }
    // The native root can advance request/pair identity before construction.
    // Its admission ID stays fixed; identity becomes immutable at this seal.
    bool SealProvisionalConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration){
        failure_=Failure::None;const auto consumer=consumers_.find(consumerId);
        if(consumer==consumers_.end())return Reject(Failure::Consumer);
        if(consumer->second.failed)return RejectConsumer(consumer->second,Failure::Unsupported);
        if(!consumer->second.provisional){
            if(Matches(consumer->second,requestEpoch,pairGeneration))return Reject(Failure::Duplicate);
            return RejectConsumer(consumer->second,Failure::Epoch);
        }
        if(!consumer->second.generations.empty())return RejectConsumer(consumer->second,Failure::Consumer);
        consumer->second.requestEpoch=requestEpoch;consumer->second.pairGeneration=pairGeneration;
        consumer->second.provisional=false;return true;
    }
    bool HasConsumer(uint64_t consumerId)const{return consumers_.contains(consumerId);}
    bool ConsumerBound(uint64_t consumerId,uint64_t generationId)const{
        const auto consumer=consumers_.find(consumerId);if(consumer==consumers_.end())return false;
        for(const auto id:consumer->second.generations)if(id==generationId)return true;
        return false;
    }
    bool BindConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration,
                      uint64_t generationId,uint64_t bornMutationEpoch,Admission admission=Admission::Source)try{
        const auto consumer=consumers_.find(consumerId);
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);
        if(!generation){if(consumer!=consumers_.end())return RejectConsumer(consumer->second,failure_);return false;}
        if(consumer==consumers_.end())return RejectGeneration(*generation,Failure::Consumer);
        if(consumer->second.provisional)return RejectBinding(*generation,consumer->second,Failure::Unsealed);
        if(!Matches(consumer->second,requestEpoch,pairGeneration)){
            MakeUnsupported(*generation,Failure::Epoch);return RejectConsumer(consumer->second,Failure::Epoch);}
        if(consumer->second.failed)return RejectGeneration(*generation,Failure::Unsupported);
        if(generation->retiring)return RejectBinding(*generation,consumer->second,Failure::Retirement);
        if(generation->sourceClosed && (admission!=Admission::Execution || consumerId>generation->closedAtConsumer))
            return RejectBinding(*generation,consumer->second,Failure::SourceClosed);
        if(admission!=Admission::Source && admission!=Admission::Execution)return RejectBinding(*generation,consumer->second,Failure::Arguments);
        if(ConsumerBound(consumerId,generationId))return Reject(Failure::Duplicate);
        if(consumer->second.generations.size()>=MaxGenerations || generation->boundConsumers==UINT64_MAX || generation->pendingConsumers==UINT64_MAX)
            return RejectBinding(*generation,consumer->second,Failure::Capacity);
        consumer->second.generations.push_back(generationId);
        ++generation->boundConsumers;++generation->pendingConsumers;return true;
    }catch(...){const auto found=generations_.find(generationId);
        const auto consumer=consumers_.find(consumerId);
        if(found!=generations_.end() && consumer!=consumers_.end())return RejectBinding(found->second,consumer->second,Failure::Allocation);
        if(found!=generations_.end())return RejectGeneration(found->second,Failure::Allocation);
        if(consumer!=consumers_.end())return RejectConsumer(consumer->second,Failure::Allocation);
        return Reject(Failure::Allocation);}

    bool CompleteConsumer(const AggregateTerminalProof& proof){
        failure_=Failure::None;const auto consumer=consumers_.find(proof.consumerId);
        if(consumer==consumers_.end())return Reject(Failure::Consumer);
        if(consumer->second.failed)return RejectConsumer(consumer->second,Failure::Unsupported);
        if(consumer->second.provisional)return RejectConsumer(consumer->second,Failure::Unsealed);
        if(!Matches(consumer->second,proof.requestEpoch,proof.pairGeneration))return RejectConsumer(consumer->second,Failure::Epoch);
        if(!proof.originalReturned || !proof.aggregateComplete)return RejectConsumer(consumer->second,Failure::TerminalProof);
        for(const auto id:consumer->second.generations){const auto found=generations_.find(id);
            if(found==generations_.end() || !found->second.pendingConsumers || found->second.completedConsumers==UINT64_MAX)
                return RejectConsumer(consumer->second,Failure::Incomplete);}
        for(const auto id:consumer->second.generations){auto& generation=generations_.at(id);
            --generation.pendingConsumers;++generation.completedConsumers;}
        consumers_.erase(consumer);++completedConsumers_;return true;
    }
    bool UnsupportedConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration,
                             Failure reason=Failure::Unsupported){
        failure_=Failure::None;const auto consumer=consumers_.find(consumerId);
        if(consumer==consumers_.end())return Reject(Failure::Consumer);
        if(!Matches(consumer->second,requestEpoch,pairGeneration))return RejectConsumer(consumer->second,Failure::Epoch);
        if(reason==Failure::None)reason=Failure::Unsupported;
        for(const auto id:consumer->second.generations){const auto found=generations_.find(id);if(found==generations_.end())continue;
            MakeUnsupported(found->second,reason);++found->second.unsupportedConsumers;}
        for(auto& [id,generation]:generations_)
            if(!generation.sourceClosed || consumerId<=generation.closedAtConsumer)MakeUnsupported(generation,reason);
        // Unresolved use stays pending on each generation after its observer row
        // leaves the bounded live table. Unsupported is never terminal completion.
        consumers_.erase(consumer);++unsupportedConsumers_;return true;
    }
    // A guarded native early exit can prove no cache/source access occurred.
    // Cancellation without that proof is not terminal or prior-use accounting.
    bool CancelUnboundConsumer(uint64_t consumerId,uint64_t requestEpoch,uint64_t pairGeneration,bool nativeNoSourceProved=false){
        failure_=Failure::None;const auto consumer=consumers_.find(consumerId);
        if(consumer==consumers_.end())return Reject(Failure::Consumer);
        if(consumer->second.failed)return RejectConsumer(consumer->second,Failure::Unsupported);
        if(!Matches(consumer->second,requestEpoch,pairGeneration))return RejectConsumer(consumer->second,Failure::Epoch);
        if(!consumer->second.generations.empty())return RejectConsumer(consumer->second,Failure::Consumer);
        if(!nativeNoSourceProved)return RejectConsumer(consumer->second,Failure::NoSourceProof);
        consumers_.erase(consumer);++cancelledConsumers_;return true;
    }
    bool UnsupportedGeneration(uint64_t generationId,uint64_t bornMutationEpoch,Failure reason=Failure::Unsupported){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        MakeUnsupported(*generation,reason==Failure::None?Failure::Unsupported:reason);return true;
    }
    bool CloseSource(uint64_t generationId,uint64_t bornMutationEpoch,uint64_t closingMutationEpoch){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        if(generation->sourceClosed)return Reject(Failure::Duplicate);
        if(generation->retiring || closingMutationEpoch<=bornMutationEpoch)return RejectGeneration(*generation,Failure::Mutation);
        generation->sourceClosed=true;generation->closedMutationEpoch=closingMutationEpoch;generation->closedAtConsumer=lastConsumer_;
        const auto owner=owners_.find(generation->ownerHeader);
        if(owner==owners_.end() || owner->second.activeGeneration!=generationId)return RejectGeneration(*generation,Failure::Generation);
        owner->second.activeGeneration=0;owner->second.lastClosedEpoch=closingMutationEpoch;return true;
    }
    bool VerifyPriorConsumers(uint64_t generationId,uint64_t bornMutationEpoch,bool allPriorConsumersAccounted){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        if(generation->retiring || !allPriorConsumersAccounted)return RejectGeneration(*generation,Failure::PriorCoverage);
        if(generation->allPriorConsumersAccounted)return Reject(Failure::Duplicate);
        generation->allPriorConsumersAccounted=true;return true;
    }
    bool VerifyDetach(uint64_t generationId,uint64_t bornMutationEpoch,bool originalOwnerDetached){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        if(generation->detached)return Reject(Failure::Duplicate);
        if(generation->retiring || !originalOwnerDetached)return RejectGeneration(*generation,Failure::Detach);
        generation->detached=true;return true;
    }
    // Moving a source-closed owner protects still-pending users. Destruction has
    // the additional terminal gates in ReadyToRetire; a mutex is not exclusivity.
    bool CanAdopt(uint64_t generationId,bool nativeExclusiveProved=false)const{
        const auto* generation=GetGeneration(generationId);
        return nativeExclusiveProved && generation && !generation->detached && CoverageEligible(*generation);
    }
    bool OwnershipEligible(uint64_t generationId)const{
        const auto* generation=GetGeneration(generationId);
        return generation && generation->detached && CoverageEligible(*generation);
    }
    bool ReadyToRetire(uint64_t generationId)const{
        const auto* generation=GetGeneration(generationId);
        if(!OwnershipEligible(generationId) || generation->pendingConsumers)return false;
        // A delayed root can discover another generation at actual execution.
        // Every pre-closure observer must therefore reach terminal resolution.
        return consumers_.empty() || consumers_.begin()->first>generation->closedAtConsumer;
    }
    bool BeginRetire(uint64_t generationId,uint64_t bornMutationEpoch){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        if(!ReadyToRetire(generationId))return Reject(Failure::Retirement);
        generation->retiring=true;return true;
    }
    bool FinishRetire(uint64_t generationId,uint64_t bornMutationEpoch){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        if(!generation->retiring || generation->unsupported)return Reject(Failure::Retirement);
        generations_.erase(generationId);++retiredGenerations_;return true;
    }
    // Discard only observer metadata. Detached ownership and unresolved users
    // must survive; this neither retires nor destroys any native definitions.
    bool ForgetClosedObservation(uint64_t generationId,uint64_t bornMutationEpoch){
        failure_=Failure::None;auto* generation=CheckGeneration(generationId,bornMutationEpoch);if(!generation)return false;
        if(!generation->sourceClosed || generation->detached || generation->retiring || generation->pendingConsumers ||
            (!consumers_.empty() && consumers_.begin()->first<=generation->closedAtConsumer))return Reject(Failure::Incomplete);
        generations_.erase(generationId);++forgottenObservations_;return true;
    }
    size_t ActiveConsumers()const{return consumers_.size();}
    size_t ActiveGenerations()const{return generations_.size();}
    size_t TrackedOwners()const{return owners_.size();}
    uint64_t LastGenerationId()const{return nextGeneration_;}
    uint64_t LastConsumerId()const{return lastConsumer_;}
    uint64_t CompletedConsumers()const{return completedConsumers_;}
    uint64_t UnsupportedConsumers()const{return unsupportedConsumers_;}
    uint64_t CancelledConsumers()const{return cancelledConsumers_;}
    uint64_t RetiredGenerations()const{return retiredGenerations_;}
    uint64_t ForgottenObservations()const{return forgottenObservations_;}
    Failure Rejection()const{return failure_;}
private:
    struct Owner {uint64_t lastBornEpoch{},lastClosedEpoch{},activeGeneration{};};
    struct Consumer {uint64_t requestEpoch{},pairGeneration{};std::vector<uint64_t> generations;bool failed{},provisional{};};
    static bool Matches(const Consumer& consumer,uint64_t requestEpoch,uint64_t pairGeneration){
        return consumer.requestEpoch==requestEpoch && consumer.pairGeneration==pairGeneration;
    }
    static bool CoverageEligible(const Generation& generation){
        return !generation.unsupported && !generation.retiring && generation.sourceClosed && generation.allPriorConsumersAccounted;
    }
    static void MakeUnsupported(Generation& generation,Failure reason){
        generation.unsupported=true;if(generation.unsupportedReason==Failure::None)generation.unsupportedReason=reason;
    }
    bool Reject(Failure reason){failure_=reason;return false;}
    uint64_t RejectId(Failure reason){Reject(reason);return 0;}
    bool RejectGeneration(Generation& generation,Failure reason){MakeUnsupported(generation,reason);return Reject(reason);}
    bool RejectConsumer(Consumer& consumer,Failure reason){
        consumer.failed=true;
        for(const auto id:consumer.generations){const auto found=generations_.find(id);
            if(found!=generations_.end())MakeUnsupported(found->second,reason);}
        return Reject(reason);
    }
    bool RejectBinding(Generation& generation,Consumer& consumer,Failure reason){
        MakeUnsupported(generation,reason);return RejectConsumer(consumer,reason);
    }
    void RejectOpenGenerations(Failure reason){
        for(auto& [id,generation]:generations_)if(!generation.sourceClosed)MakeUnsupported(generation,reason);
    }
    Generation* CheckGeneration(uint64_t generationId,uint64_t bornMutationEpoch){
        const auto found=generations_.find(generationId);
        if(found==generations_.end()){Reject(Failure::Generation);return nullptr;}
        if(found->second.bornMutationEpoch!=bornMutationEpoch){RejectGeneration(found->second,Failure::Epoch);return nullptr;}
        return &found->second;
    }
    std::map<uint64_t,Owner> owners_;
    std::map<uint64_t,Generation> generations_;
    std::map<uint64_t,Consumer> consumers_;
    uint64_t nextGeneration_{},lastConsumer_{},completedConsumers_{},unsupportedConsumers_{},cancelledConsumers_{},retiredGenerations_{},forgottenObservations_{};
    Failure failure_=Failure::None;
};
#endif
}
