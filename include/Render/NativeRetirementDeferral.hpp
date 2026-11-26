#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>

namespace cvr::stereo::retirement_deferral {
inline constexpr size_t MaxOwners=128,MaxStates=256,MaxExtraReaders=256;
inline constexpr uint32_t MaxDefinitionSlots=8192;
struct NativeHeader {
    uint64_t data{};
    uint32_t capacity{},count{};
    bool operator==(const NativeHeader&)const=default;
};
static_assert(sizeof(NativeHeader)==16 && offsetof(NativeHeader,capacity)==8 && offsetof(NativeHeader,count)==12);
struct HeaderIdentity {
    uint64_t ownerHeader{},generation{},bornEpoch{};
    NativeHeader savedNative16B,emptyAllocator16B;
    bool operator==(const HeaderIdentity&)const=default;
};
struct AllocatorIdentity {
    uint64_t address{},generation{};
    bool operator==(const AllocatorIdentity&)const=default;
};
struct AllocatorProof {
    AllocatorIdentity identity;
    // Fresh typed guarded Reset/Destroy scope evidence, including matching
    // object/control/string allocator globals. A pointer read is not liveness.
    bool infrastructureMatches{},alive{};
};
struct ReservationAllocatorObservation {
    AllocatorIdentity identity;
    bool infrastructureMatches{};
};
struct ReservationFacts {
    bool retirementPathAudited{},layoutAudited{},destructorTreeSafe{};
};
enum class IntentKind:uint32_t {None,Reset,Destroy};
struct NativeIntentFacts {
    bool exactNativeRetirementPoint{},layoutAudited{},uniqueWriter{},destructorTreeSafe{};
};
struct HeaderTransferProof {
    HeaderIdentity sourceBefore;
    NativeHeader savedCopy16B,sourceAfter16B;
    bool successfulExactTransfer{};
};
struct ExtraReaderIdentity {
    uint64_t leaseId{},consumerId{},epoch{},pairGeneration{},sourceGraph{},executionGraph{};
    bool operator==(const ExtraReaderIdentity&)const=default;
};
struct OwnedGraphCompletionProof {
    ExtraReaderIdentity reader;
    bool ownedIdentityVerified{},producerClosed{},originalProducerReturned{};
    bool declarationQueued{},originalDeclarationReturned{},declarationComplete{};
    bool executionQueued{},originalExecutionReturned{},executionAggregateComplete{};
};
struct UnescapedCancellationProof {
    ExtraReaderIdentity reader;
    bool nativeOriginalNeverInvoked{},verifiedOriginalSlotRestored{},noNativeEscape{};
};
enum class Phase:uint32_t {Reserved,AcquisitionsClosed,IntentClosed,Detached,Destroying,Destroyed,Quarantined};
enum class Failure:uint32_t {
    None,Arguments,ReservationFacts,Capacity,Allocation,GenerationReplay,UnresolvedSource,StorageOverlap,
    Closed,ReaderSequence,ReaderIdentity,ReaderMissing,Incomplete,UnsupportedCompletion,CancellationClosed,CancellationProof,
    HeaderChanged,IntentFacts,IntentKind,NestedIntent,NoIntent,TransferRepeated,TransferFailed,
    AllocatorChanged,Shutdown,DestroyRepeated,DestroyNotBegun,DestroyFailed
};

class Ledger;
// The caller serializes all methods under a short gate and supplies audited facts.
// Unknown vanilla users remain the native retirement point's responsibility.
// This class tracks only added CVR readers; it never waits or writes native memory.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/retirement_deferral.rs through retirement_deferral_abi.rs
// (tools/single_pass_tests/retirement_deferral_test runs its cases against them in its Rust variant). The ledger and
// its states live in Rust; a State here is the stable face of one reserved, boxed Rust state.
namespace detail {
extern "C" {
void* cvr_rd_ledger_new();
void cvr_rd_ledger_free(void*);
bool cvr_rd_ledger_reserve(void*,const HeaderIdentity*,const ReservationFacts*,const ReservationAllocatorObservation*,
    bool(*)(void*),void*,uint32_t*,void**);
bool cvr_rd_ledger_find(const void*,uint64_t,uint64_t,uint64_t,uint32_t*);
bool cvr_rd_ledger_retire(void*,uint32_t,uint64_t,uint64_t,uint64_t);
uint64_t cvr_rd_ledger_op(void*,uint32_t);
uint64_t cvr_rd_state(void*,uint32_t,const void*,const void*,const void*,uint64_t);
void* cvr_rd_state_ptr(void*,uint32_t,const AllocatorProof*);
}
}
static_assert(sizeof(HeaderIdentity)==56 && sizeof(AllocatorIdentity)==16 && sizeof(AllocatorProof)==24 &&
    sizeof(ReservationAllocatorObservation)==24 && sizeof(ReservationFacts)==3 && sizeof(NativeIntentFacts)==4 &&
    sizeof(HeaderTransferProof)==96 && sizeof(ExtraReaderIdentity)==48 && sizeof(OwnedGraphCompletionProof)==64 &&
    sizeof(UnescapedCancellationProof)==56,"mirrored by native/retirement_deferral.rs");
class State {
public:
    State(const State&)=delete;
    State& operator=(const State&)=delete;
    State(State&&)=delete;
    State& operator=(State&&)=delete;
    const HeaderIdentity& FrozenHeader()const noexcept{return *static_cast<const HeaderIdentity*>(Address(0));}
    const AllocatorIdentity& FrozenAllocator()const noexcept{return *static_cast<const AllocatorIdentity*>(Address(1));}
    const NativeHeader& SavedHeader()const noexcept{return *static_cast<const NativeHeader*>(Address(2));}
    Phase CurrentPhase()const noexcept{return Phase(Get(0));}
    bool AcquisitionsOpen()const noexcept{return Get(1)!=0;}
    bool NativeIntentSeen()const noexcept{return Get(2)!=0;}
    IntentKind NativeIntentKind()const noexcept{return IntentKind(Get(3));}
    bool ExactHeaderDetached()const noexcept{return Get(4)!=0;}
    bool DestroyAttemptConsumed()const noexcept{return Get(5)!=0;}
    bool DestroyReturned()const noexcept{return Get(6)!=0;}
    bool Quarantined()const noexcept{return Get(7)!=0;}
    bool Selected()const noexcept{return Get(8)!=0;}
    uint64_t AdmittedReaders()const noexcept{return Get(9);}
    uint64_t CompletedReaders()const noexcept{return Get(10);}
    uint64_t CanceledReaders()const noexcept{return Get(11);}
    size_t PendingReaders()const noexcept{return size_t(Get(12));}
    Failure Rejection()const noexcept{return Failure(Get(13));}
    // Read-only exact lookup, not a new acquisition. Closing native intent (or
    // selection) does not invalidate a reader still protecting the saved owner.
    bool HasPendingExtraReader(const ExtraReaderIdentity& identity)const noexcept{return Get(16,&identity)!=0;}

    bool AdmitExtraReader(const ExtraReaderIdentity& reader)noexcept{return Call(20,&reader)!=0;}
    // Incomplete observations retain the reader. Only actual producer, declaration
    // and execution proofs release its slot; cancellation is not such a proof.
    bool CompleteExtraReader(const OwnedGraphCompletionProof& proof)noexcept{return Call(21,&proof)!=0;}
    bool MarkExtraReaderEscaped(const ExtraReaderIdentity& identity)noexcept{return Call(22,&identity)!=0;}
    bool CancelUnescapedExtraReader(const UnescapedCancellationProof& proof)noexcept{return Call(23,&proof)!=0;}
    void UnsupportedExtraCompletion(uint64_t leaseId)noexcept{Call(24,nullptr,nullptr,nullptr,leaseId);}
    // Closing and sealing the exact native intent is one transition under the
    // caller's gate. It closes only CVR acquisitions, never claims prior coverage.
    bool ObserveNativeIntent(IntentKind kind,const HeaderIdentity& source,NativeIntentFacts facts,
                             const AllocatorProof& allocator)noexcept{
        return Call(25,&source,&facts,&allocator,uint64_t(kind))!=0;
    }
    // A quarantined but exact source can still be detached to preserve its readers.
    // Successful transfer never clears quarantine or authorizes its destruction.
    bool RecordHeaderTransfer(const HeaderTransferProof& proof)noexcept{return Call(26,&proof)!=0;}
    bool ObserveAllocator(const AllocatorProof& proof)noexcept{return Call(27,&proof)!=0;}
    bool ReadyToDestroy(const AllocatorProof& proof)const noexcept{return Get(17,&proof)!=0;}
    // The returned 16-byte storage is stable (inside the boxed Rust state). Consume
    // the attempt BEFORE invoking original986E74(pointer) outside the short CVR gate
    // but within that typed native scope; never retry after an exception.
    NativeHeader* BeginDestroy(const AllocatorProof& proof)noexcept{
        return static_cast<NativeHeader*>(detail::cvr_rd_state_ptr(state_,3,&proof));
    }
    bool FinishDestroy(bool originalReturnedNormally)noexcept{return Call(28,nullptr,nullptr,nullptr,originalReturnedNormally)!=0;}
    void CancelSelection()noexcept{Call(29);}
    void Shutdown()noexcept{Call(30);}
    bool CanRetireMetadata()const noexcept{return Get(14)!=0;}
    bool CanAbandonUnusedReservation()const noexcept{return Get(15)!=0;}
private:
    friend class Ledger;
    State()noexcept=default;
    uint64_t Get(uint32_t which,const void* a=nullptr)const noexcept{return detail::cvr_rd_state(state_,which,a,nullptr,nullptr,0);}
    uint64_t Call(uint32_t which,const void* a=nullptr,const void* b=nullptr,const void* c=nullptr,uint64_t value=0)noexcept{
        return detail::cvr_rd_state(state_,which,a,b,c,value);
    }
    const void* Address(uint32_t which)const noexcept{return detail::cvr_rd_state_ptr(state_,which,nullptr);}
    void* state_{};
};
#else
class State {
public:
    State(const State&)=delete;
    State& operator=(const State&)=delete;
    State(State&&)=delete;
    State& operator=(State&&)=delete;
    const HeaderIdentity& FrozenHeader()const noexcept{return identity_;}
    const AllocatorIdentity& FrozenAllocator()const noexcept{return allocator_;}
    const NativeHeader& SavedHeader()const noexcept{return destructionHeader_;}
    Phase CurrentPhase()const noexcept{
        if(quarantined_)return Phase::Quarantined;
        if(destroyed_)return Phase::Destroyed;
        if(destroyAttempted_)return Phase::Destroying;
        if(detached_)return Phase::Detached;
        if(intentSeen_)return Phase::IntentClosed;
        return acquisitionsClosed_?Phase::AcquisitionsClosed:Phase::Reserved;
    }
    bool AcquisitionsOpen()const noexcept{return !acquisitionsClosed_ && !quarantined_;}
    bool NativeIntentSeen()const noexcept{return intentSeen_;}
    IntentKind NativeIntentKind()const noexcept{return kind_;}
    bool ExactHeaderDetached()const noexcept{return detached_;}
    bool DestroyAttemptConsumed()const noexcept{return destroyAttempted_;}
    bool DestroyReturned()const noexcept{return destroyed_;}
    bool Quarantined()const noexcept{return quarantined_;}
    bool Selected()const noexcept{return selected_;}
    uint64_t AdmittedReaders()const noexcept{return admittedReaders_;}
    uint64_t CompletedReaders()const noexcept{return completedReaders_;}
    uint64_t CanceledReaders()const noexcept{return canceledReaders_;}
    size_t PendingReaders()const noexcept{return pendingReaders_;}
    Failure Rejection()const noexcept{return failure_;}
    // Read-only exact lookup, not a new acquisition. Closing native intent (or
    // selection) does not invalidate a reader still protecting the saved owner.
    bool HasPendingExtraReader(const ExtraReaderIdentity& identity)const noexcept{
        if(quarantined_ || shutdown_ || destroyAttempted_ || destroyed_)return false;
        for(const auto& reader:readers_)if(reader.active && reader.identity==identity)return true;
        return false;
    }

    bool AdmitExtraReader(const ExtraReaderIdentity& reader)noexcept{
        if(!reader.leaseId || reader.leaseId<=lastReaderLease_)return Reject(Failure::ReaderSequence);
        lastReaderLease_=reader.leaseId;
        if(!AcquisitionsOpen())return Reject(Failure::Closed);
        if(!ReaderValid(reader))return Reject(Failure::ReaderIdentity);
        if(pendingReaders_>=MaxExtraReaders || admittedReaders_==UINT64_MAX)return Reject(Failure::Capacity);
        for(const auto& slot:readers_)if(slot.active && slot.identity.consumerId==reader.consumerId)
            return Reject(Failure::ReaderIdentity);
        for(auto& slot:readers_)if(!slot.active){slot.identity=reader;slot.active=true;
            ++pendingReaders_;++admittedReaders_;failure_=Failure::None;return true;}
        return Reject(Failure::Capacity);
    }
    // Incomplete observations retain the reader. Only actual producer, declaration
    // and execution proofs release its slot; cancellation is not such a proof.
    bool CompleteExtraReader(const OwnedGraphCompletionProof& proof)noexcept{
        auto* slot=FindReader(proof.reader.leaseId);
        if(!slot)return Reject(Failure::ReaderMissing);
        if(slot->identity!=proof.reader)return Quarantine(Failure::ReaderIdentity);
        if(!proof.ownedIdentityVerified || !proof.producerClosed || !proof.originalProducerReturned ||
            !proof.declarationQueued || !proof.originalDeclarationReturned || !proof.declarationComplete ||
            !proof.executionQueued || !proof.originalExecutionReturned || !proof.executionAggregateComplete)
            return Reject(Failure::Incomplete);
        *slot=Reader{};--pendingReaders_;++completedReaders_;
        if(!quarantined_)failure_=Failure::None;return true;
    }
    bool MarkExtraReaderEscaped(const ExtraReaderIdentity& identity)noexcept{
        auto* reader=FindReader(identity.leaseId);if(!reader)return Reject(Failure::ReaderMissing);
        if(reader->identity!=identity)return Quarantine(Failure::ReaderIdentity);
        reader->nativeEscapeKnown=true;return true;
    }
    bool CancelUnescapedExtraReader(const UnescapedCancellationProof& proof)noexcept{
        auto* reader=FindReader(proof.reader.leaseId);if(!reader)return Reject(Failure::ReaderMissing);
        if(reader->identity!=proof.reader)return Quarantine(Failure::ReaderIdentity);
        if(intentAttempted_ || transferAttempted_ || detached_ || destroyAttempted_ || shutdown_ || quarantined_)
            return Reject(Failure::CancellationClosed);
        if(reader->nativeEscapeKnown || !proof.nativeOriginalNeverInvoked || !proof.noNativeEscape){
            reader->nativeEscapeKnown=true;return Reject(Failure::CancellationProof);}
        if(!proof.verifiedOriginalSlotRestored)return Reject(Failure::CancellationProof);
        *reader=Reader{};--pendingReaders_;++canceledReaders_;failure_=Failure::None;return true;
    }
    void UnsupportedExtraCompletion(uint64_t leaseId)noexcept{
        Quarantine(FindReader(leaseId)?Failure::UnsupportedCompletion:Failure::ReaderMissing);
    }
    // Closing and sealing the exact native intent is one transition under the
    // caller's gate. It closes only CVR acquisitions, never claims prior coverage.
    bool ObserveNativeIntent(IntentKind kind,const HeaderIdentity& source,NativeIntentFacts facts,
                             const AllocatorProof& allocator)noexcept{
        acquisitionsClosed_=true;
        if(intentAttempted_)return Quarantine(Failure::NestedIntent);
        intentAttempted_=true;
        if(kind!=IntentKind::Reset && kind!=IntentKind::Destroy)return Quarantine(Failure::IntentKind);
        if(source!=identity_)return Quarantine(Failure::HeaderChanged);
        if(!facts.exactNativeRetirementPoint || !facts.layoutAudited || !facts.uniqueWriter || !facts.destructorTreeSafe)
            return Quarantine(Failure::IntentFacts);
        if(!AllocatorMatches(allocator))return Quarantine(Failure::AllocatorChanged);
        kind_=kind;intentSeen_=true;
        if(!quarantined_)failure_=Failure::None;return true;
    }
    // A quarantined but exact source can still be detached to preserve its readers.
    // Successful transfer never clears quarantine or authorizes its destruction.
    bool RecordHeaderTransfer(const HeaderTransferProof& proof)noexcept{
        if(!intentSeen_)return Quarantine(Failure::NoIntent);
        if(transferAttempted_)return Quarantine(Failure::TransferRepeated);
        transferAttempted_=true;
        if(!proof.successfulExactTransfer || proof.sourceBefore!=identity_ ||
            proof.savedCopy16B!=identity_.savedNative16B || proof.savedCopy16B!=destructionHeader_ ||
            proof.sourceAfter16B!=identity_.emptyAllocator16B)return Quarantine(Failure::TransferFailed);
        detached_=true;if(!quarantined_)failure_=Failure::None;return true;
    }
    bool ObserveAllocator(const AllocatorProof& proof)noexcept{
        return AllocatorMatches(proof) || Quarantine(Failure::AllocatorChanged);
    }
    bool ReadyToDestroy(const AllocatorProof& proof)const noexcept{
        return intentSeen_ && detached_ && !pendingReaders_ && !quarantined_ && !shutdown_ &&
            !destroyAttempted_ && destructionHeader_==identity_.savedNative16B && AllocatorMatches(proof);
    }
    // The returned 16-byte storage is stable. Consume the attempt BEFORE invoking
    // original986E74(pointer) outside the short CVR gate but within that typed
    // native scope. Keep its certificate valid through the call; scope closing
    // stops later attempts, never interrupts one already begun. An exception or
    // unsupported return must never be retried. This pointer owns no other RAII
    // object; native baseline intent continuation is always the caller's duty.
    NativeHeader* BeginDestroy(const AllocatorProof& proof)noexcept{
        if(destroyAttempted_){Quarantine(Failure::DestroyRepeated);return nullptr;}
        if(!AllocatorMatches(proof)){Quarantine(Failure::AllocatorChanged);return nullptr;}
        if(!ReadyToDestroy(proof)){Reject(Failure::Incomplete);return nullptr;}
        destroyAttempted_=true;failure_=Failure::None;return &destructionHeader_;
    }
    bool FinishDestroy(bool originalReturnedNormally)noexcept{
        if(!destroyAttempted_)return Quarantine(Failure::DestroyNotBegun);
        if(destroyFinished_)return Quarantine(Failure::DestroyRepeated);
        destroyFinished_=true;
        if(!originalReturnedNormally)return Quarantine(Failure::DestroyFailed);
        destroyed_=true;if(!quarantined_)failure_=Failure::None;return !quarantined_;
    }
    void CancelSelection()noexcept{selected_=false;acquisitionsClosed_=true;}
    void Shutdown()noexcept{shutdown_=true;acquisitionsClosed_=true;selected_=false;Quarantine(Failure::Shutdown);}
    bool CanRetireMetadata()const noexcept{
        return destroyed_ && detached_ && intentSeen_ && !pendingReaders_ && !quarantined_ && !shutdown_;
    }
    bool CanAbandonUnusedReservation()const noexcept{
        return !pendingReaders_ && !completedReaders_ && admittedReaders_==canceledReaders_ && !intentAttempted_ &&
            !transferAttempted_ && !detached_ && !destroyAttempted_ && !quarantined_ && !shutdown_;
    }
private:
    friend class Ledger;
    struct Reader {ExtraReaderIdentity identity;bool active{},nativeEscapeKnown{};};
    State(const HeaderIdentity& identity,AllocatorIdentity allocator,uint64_t previousReaderLease=0)noexcept:
        identity_(identity),allocator_(allocator),destructionHeader_(identity.savedNative16B),lastReaderLease_(previousReaderLease){}
    static bool Pointer(uint64_t value,uint64_t bytes)noexcept{return value && !(value&7) && value<=UINT64_MAX-bytes;}
    static bool Overlap(uint64_t a,uint64_t aBytes,uint64_t b,uint64_t bBytes)noexcept{return a<b+bBytes && b<a+aBytes;}
    static uint64_t AllocationBytes(const NativeHeader& header)noexcept{return header.capacity?uint64_t(header.capacity)*8+8:0;}
    bool AllocationStillRetained()const noexcept{return identity_.savedNative16B.capacity && (!destroyed_ || quarantined_);}
    static bool HeaderValid(const HeaderIdentity& identity)noexcept{
        const auto& saved=identity.savedNative16B;const auto& empty=identity.emptyAllocator16B;
        if(!Pointer(identity.ownerHeader,16) || !identity.generation || !identity.bornEpoch || !Pointer(empty.data,8) ||
            empty.capacity || empty.count || saved.capacity>MaxDefinitionSlots || saved.count>saved.capacity || !Pointer(saved.data,8) ||
            Overlap(identity.ownerHeader,16,empty.data,8))return false;
        if(!saved.capacity)return saved.data==empty.data;
        const auto bytes=AllocationBytes(saved);
        return Pointer(saved.data,bytes) && !Overlap(saved.data,bytes,empty.data,8) &&
            !Overlap(saved.data,bytes,identity.ownerHeader,16);
    }
    static bool ReaderValid(const ExtraReaderIdentity& reader)noexcept{
        return reader.consumerId && reader.epoch && reader.pairGeneration && Pointer(reader.sourceGraph,0x70) &&
            Pointer(reader.executionGraph,0x70) && reader.sourceGraph!=reader.executionGraph;
    }
    bool AllocatorMatches(const AllocatorProof& proof)const noexcept{
        return proof.alive && proof.infrastructureMatches && proof.identity==allocator_;
    }
    Reader* FindReader(uint64_t leaseId)noexcept{
        for(auto& reader:readers_)if(reader.active && reader.identity.leaseId==leaseId)return &reader;
        return nullptr;
    }
    bool Reject(Failure reason)noexcept{if(!quarantined_)failure_=reason;return false;}
    bool Quarantine(Failure reason)noexcept{
        if(!quarantined_)failure_=reason;
        quarantined_=true;acquisitionsClosed_=true;selected_=false;return false;
    }
    const HeaderIdentity identity_;
    const AllocatorIdentity allocator_;
    NativeHeader destructionHeader_;
    std::array<Reader,MaxExtraReaders> readers_{};
    uint64_t lastReaderLease_{},admittedReaders_{},completedReaders_{},canceledReaders_{};
    size_t pendingReaders_{};
    IntentKind kind_=IntentKind::None;
    Failure failure_=Failure::None;
    bool selected_=true,acquisitionsClosed_{},intentAttempted_{},intentSeen_{},transferAttempted_{},detached_{};
    bool destroyAttempted_{},destroyFinished_{},destroyed_{},quarantined_{},shutdown_{};
};
#endif

// Reserve the State and all its fixed reader slots before admitting any extra
// reader. Completed records are erased; only bounded per-owner ABA watermarks
// persist. The layout audit must establish data as the vector allocation base,
// capacity*8 entry bytes and its 8-byte allocator trailer; prefix layouts are not
// supported. The ledger must outlive its readers and any begun native destroy
// call. Shutdown preserves storage only while this ledger stays alive; it does
// not arrange module/process teardown retention. No native allocation is freed
// by the C++ State/Ledger destructors. This is not an adoption gate.
#if CVR_RUST_VRIK
class Ledger {
public:
    Ledger():ledger_(detail::cvr_rd_ledger_new()){}
    ~Ledger(){detail::cvr_rd_ledger_free(ledger_);}
    Ledger(const Ledger&)=delete;
    Ledger& operator=(const Ledger&)=delete;
    Ledger(Ledger&&)=delete;
    Ledger& operator=(Ledger&&)=delete;
    // The face is allocated where the C++ allocated its State: after every check,
    // before anything is committed. Its failure is Allocation, committing nothing.
    State* Reserve(const HeaderIdentity& identity,ReservationFacts facts,const ReservationAllocatorObservation& allocator)noexcept{
        std::unique_ptr<State> face;uint32_t slot{};void* state{};
        const auto allocate=[](void* context)->bool{
            try{static_cast<std::unique_ptr<State>*>(context)->reset(new State);return true;}catch(...){return false;}
        };
        if(!detail::cvr_rd_ledger_reserve(ledger_,&identity,&facts,&allocator,allocate,&face,&slot,&state) || slot>=MaxStates)return nullptr;
        face->state_=state;states_[slot]=std::move(face);return states_[slot].get();
    }
    State* Find(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        uint32_t slot{};
        return detail::cvr_rd_ledger_find(ledger_,ownerHeader,generation,bornEpoch,&slot) && slot<MaxStates?states_[slot].get():nullptr;
    }
    const State* Find(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)const noexcept{
        uint32_t slot{};
        return detail::cvr_rd_ledger_find(ledger_,ownerHeader,generation,bornEpoch,&slot) && slot<MaxStates?states_[slot].get():nullptr;
    }
    bool RetireMetadata(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        return Retire(0,ownerHeader,generation,bornEpoch);
    }
    // Metadata only: no native intent/transfer occurred, and every admitted
    // reader was explicitly canceled before escape. Preserve its exact retry
    // identity and nonce; a completed escaped reader is not an unused attempt.
    bool AbandonUnusedReservation(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        return Retire(1,ownerHeader,generation,bornEpoch);
    }
    void CancelSelection()noexcept{detail::cvr_rd_ledger_op(ledger_,0);}
    void Shutdown()noexcept{detail::cvr_rd_ledger_op(ledger_,1);}
    size_t Size()const noexcept{return size_t(detail::cvr_rd_ledger_op(ledger_,2));}
    size_t OwnerCount()const noexcept{return size_t(detail::cvr_rd_ledger_op(ledger_,3));}
    Failure Rejection()const noexcept{return Failure(detail::cvr_rd_ledger_op(ledger_,4));}
private:
    // A retired state's face goes with it; its Rust storage is dropped there.
    bool Retire(uint32_t which,uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        uint32_t slot{};const bool found=detail::cvr_rd_ledger_find(ledger_,ownerHeader,generation,bornEpoch,&slot);
        if(!detail::cvr_rd_ledger_retire(ledger_,which,ownerHeader,generation,bornEpoch))return false;
        if(found && slot<MaxStates)states_[slot].reset();return true;
    }
    void* ledger_;
    std::array<std::unique_ptr<State>,MaxStates> states_{};
};
#else
class Ledger {
public:
    Ledger()=default;
    Ledger(const Ledger&)=delete;
    Ledger& operator=(const Ledger&)=delete;
    Ledger(Ledger&&)=delete;
    Ledger& operator=(Ledger&&)=delete;
    State* Reserve(const HeaderIdentity& identity,ReservationFacts facts,const ReservationAllocatorObservation& allocator)noexcept try{
        if(shutdown_){failure_=Failure::Shutdown;return nullptr;}
        failure_=Failure::Arguments;
        if(!State::HeaderValid(identity) || !State::Pointer(allocator.identity.address,8) || !allocator.identity.generation ||
            allocator.identity.address!=identity.emptyAllocator16B.data)return nullptr;
        failure_=Failure::ReservationFacts;
        if(!facts.retirementPathAudited || !facts.layoutAudited || !facts.destructorTreeSafe ||
            !allocator.infrastructureMatches)return nullptr;
        Watermark* watermark{};Watermark* emptyOwner{};
        for(auto& value:owners_){if(value.identity.ownerHeader==identity.ownerHeader)watermark=&value;else if(!value.identity.ownerHeader && !emptyOwner)emptyOwner=&value;}
        failure_=Failure::GenerationReplay;
        const bool unusedRetry=watermark && watermark->unusedAbandoned && identity==watermark->identity && allocator.identity==watermark->allocator;
        if(watermark && !unusedRetry && (identity.generation<=watermark->identity.generation || identity.bornEpoch<=watermark->identity.bornEpoch))return nullptr;
        for(const auto& old:states_)if(old && old->identity_.ownerHeader==identity.ownerHeader && !old->detached_){
            failure_=Failure::UnresolvedSource;return nullptr;}
        for(auto& old:states_)if(old){
            const auto& saved=old->identity_.savedNative16B;
            const auto oldBytes=State::AllocationBytes(saved),newBytes=State::AllocationBytes(identity.savedNative16B);
            const bool oldAllocationConflict=old->AllocationStillRetained() &&
                ((newBytes && State::Overlap(saved.data,oldBytes,identity.savedNative16B.data,newBytes)) ||
                 State::Overlap(saved.data,oldBytes,identity.ownerHeader,16) ||
                 State::Overlap(saved.data,oldBytes,identity.emptyAllocator16B.data,8));
            const bool newAllocationConflict=newBytes &&
                ((!old->detached_ && State::Overlap(identity.savedNative16B.data,newBytes,old->identity_.ownerHeader,16)) ||
                 ((!old->destroyed_ || old->quarantined_) && State::Overlap(identity.savedNative16B.data,newBytes,old->identity_.emptyAllocator16B.data,8)));
            if(oldAllocationConflict || newAllocationConflict){
                old->Quarantine(Failure::StorageOverlap);failure_=Failure::StorageOverlap;return nullptr;}
        }
        failure_=Failure::Capacity;
        if(!watermark && !emptyOwner)return nullptr;
        std::unique_ptr<State>* emptyState{};for(auto& state:states_)if(!state){emptyState=&state;break;}
        if(!emptyState)return nullptr;
        auto state=std::unique_ptr<State>(new State(identity,allocator.identity,unusedRetry?watermark->lastReaderLease:0));auto* result=state.get();
        *emptyState=std::move(state);if(!watermark){watermark=emptyOwner;++ownerCount_;}
        *watermark={identity,allocator.identity,unusedRetry?watermark->lastReaderLease:0,false};++stateCount_;
        failure_=Failure::None;return result;
    }catch(...){failure_=Failure::Allocation;return nullptr;}
    State* Find(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        for(auto& state:states_)if(state && state->identity_.ownerHeader==ownerHeader &&
            state->identity_.generation==generation && state->identity_.bornEpoch==bornEpoch)return state.get();
        return nullptr;
    }
    const State* Find(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)const noexcept{
        for(const auto& state:states_)if(state && state->identity_.ownerHeader==ownerHeader &&
            state->identity_.generation==generation && state->identity_.bornEpoch==bornEpoch)return state.get();
        return nullptr;
    }
    bool RetireMetadata(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        for(auto& state:states_)if(state && state->identity_.ownerHeader==ownerHeader &&
            state->identity_.generation==generation && state->identity_.bornEpoch==bornEpoch){
            if(!state->CanRetireMetadata()){failure_=Failure::Incomplete;return false;}
            state.reset();--stateCount_;failure_=Failure::None;return true;}
        failure_=Failure::Arguments;return false;
    }
    // Metadata only: no native intent/transfer occurred, and every admitted
    // reader was explicitly canceled before escape. Preserve its exact retry
    // identity and nonce; a completed escaped reader is not an unused attempt.
    bool AbandonUnusedReservation(uint64_t ownerHeader,uint64_t generation,uint64_t bornEpoch)noexcept{
        for(auto& state:states_)if(state && state->identity_.ownerHeader==ownerHeader &&
            state->identity_.generation==generation && state->identity_.bornEpoch==bornEpoch){
            if(!state->CanAbandonUnusedReservation()){failure_=Failure::Incomplete;return false;}
            for(auto& watermark:owners_)if(watermark.identity==state->identity_){
                watermark.unusedAbandoned=true;watermark.lastReaderLease=state->lastReaderLease_;break;}
            state.reset();--stateCount_;failure_=Failure::None;return true;}
        failure_=Failure::Arguments;return false;
    }
    void CancelSelection()noexcept{for(auto& state:states_)if(state)state->CancelSelection();}
    void Shutdown()noexcept{shutdown_=true;for(auto& state:states_)if(state)state->Shutdown();}
    size_t Size()const noexcept{return stateCount_;}
    size_t OwnerCount()const noexcept{return ownerCount_;}
    Failure Rejection()const noexcept{return failure_;}
private:
    struct Watermark {HeaderIdentity identity;AllocatorIdentity allocator;uint64_t lastReaderLease{};bool unusedAbandoned{};};
    std::array<Watermark,MaxOwners> owners_{};
    std::array<std::unique_ptr<State>,MaxStates> states_{};
    size_t stateCount_{},ownerCount_{};
    bool shutdown_{};
    Failure failure_=Failure::None;
};
#endif
}
