#pragma once
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <type_traits>

namespace cvr::stereo::upload_reservation {
// 2.5: sub_140419DF4 inlines the full-ring wait 2.31 kept out of line (sub_141EF5D8A), so the guarded body is the
// whole function (312 bytes). The successful-CAS JNE sits at +0x10C and retries BACKWARDS to 0x419EEB (2.31: +0x76,
// forward to 0x7745F4); the five success-only bytes after it are unchanged.
inline constexpr uint32_t AllocatorRva=0x419DF4,AllocatorBytes=312,WitnessRva=0x419F00,RetryRva=0x419EEB;
inline constexpr uint64_t AllocatorFnv=0x6B65A1650C2F23E4ull;
inline constexpr uint8_t RetryDisplacement=uint8_t(int32_t(RetryRva)-int32_t(WitnessRva+7));
inline constexpr size_t MaxRows=4096,MaxRings=64;
inline constexpr size_t MaxXstateBytes=16384;
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/upload_reservation.rs (tools/single_pass_tests/upload_reservation_test
// runs its cases against them in its Rust variant); the ledger is an owned Rust handle.
struct Witness;struct Row;struct Reservation;struct Range;struct ExternalInstallPlan;struct ExternalInstallReceipt;
extern "C" {
bool cvr_ur_valid_xstate(uint64_t,uint64_t,uint64_t);
uint64_t cvr_ur_fnv(const uint8_t*,size_t);
uint64_t cvr_ur_external_plan_checksum(const ExternalInstallPlan*);
uint64_t cvr_ur_external_receipt_checksum(const ExternalInstallReceipt*);
bool cvr_ur_valid_external_plan(const ExternalInstallPlan*);
bool cvr_ur_matches_external_patched_body(const ExternalInstallPlan*,const std::array<uint8_t,AllocatorBytes>*);
bool cvr_ur_valid_external_receipt(const ExternalInstallPlan*,const ExternalInstallReceipt*);
bool cvr_ur_valid_witness(const Witness*);
void* cvr_ur_ledger_new(size_t,size_t,uint64_t);
void cvr_ur_ledger_free(void*);
void cvr_ur_ledger_reset(void*,uint64_t);
bool cvr_ur_ledger_begin_generation(void*,uint64_t,uint64_t,uint64_t);
void cvr_ur_ledger_end_generation(void*,uint64_t);
void cvr_ur_ledger_quarantine_all(void*);
uint64_t cvr_ur_ledger_observe(void*,const Witness*,uint64_t,Row*,bool);
uint64_t cvr_ur_ledger_find(void*,uint64_t,Reservation*);
uint64_t cvr_ur_ledger_validate_range(void*,const Reservation*,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,Range*);
size_t cvr_ur_ledger_count(void*);
const Row* cvr_ur_ledger_rows(void*);
bool cvr_ur_ledger_exhausted(void*);
}
#endif
#if CVR_RUST_VRIK
inline bool ValidXstate(uint64_t enabled,uint64_t supported,uint64_t bytes)noexcept{return cvr_ur_valid_xstate(enabled,supported,bytes);}
#else
inline bool ValidXstate(uint64_t enabled,uint64_t supported,uint64_t bytes)noexcept {
    return (enabled&3)==3 && !(enabled&~supported) && bytes>=576 && bytes<=MaxXstateBytes;
}
#endif
enum class Status:uint64_t {
    Ignored,Captured,UnknownGeneration,InvalidWitness,Capacity,Quarantined,
    UnknownReservation,GenerationMismatch,RangeMismatch,Disabled,GuardMismatch,
    InstallFailure,Contention
};
struct Witness {
    uint64_t ring{},oldHead{},absoluteBegin{},absoluteEnd{},bytes{},capacity{};
    uint64_t nativeStack{},nativeCaller{},flags{};
};
// A row is evidence of the successful native CAS only. It carries no writer,
// retirement, source-submission, no-overwrite, read, or copy permission.
struct Row {
    uint64_t sequence{},observerEpoch{},arenaGeneration{},ring{},oldHead{},absoluteBegin{},absoluteEnd{},bytes{};
    uint64_t capacity{},physicalOffset{},thread{},nativeCaller{},nativeStack{},flags{},status{},witnessRva{WitnessRva};
};
static_assert(sizeof(Row)==128);
struct Reservation {
    uint64_t sequence{},observerEpoch{},arenaGeneration{},ring{},absoluteBegin{},absoluteEnd{},capacity{};
    bool operator==(const Reservation&)const=default;
};
struct Range {uint64_t absoluteBegin{},absoluteEnd{},physicalOffset{},paddedBytes{},logicalBytes{};};
#if CVR_RUST_VRIK
inline uint64_t Fnv(const uint8_t* bytes,size_t count)noexcept{return cvr_ur_fnv(bytes,count);}
#else
inline uint64_t Fnv(const uint8_t* bytes,size_t count)noexcept {
    uint64_t hash=14695981039346656037ull;
    for(size_t i=0;i<count;++i)hash=(hash^bytes[i])*1099511628211ull;
    return hash;
}
#endif
inline constexpr uint64_t ExternalPlanVersion=0x55504C414E303031ull,ExternalReceiptVersion=0x5550524543303031ull;
inline constexpr std::array<uint8_t,7> OriginalWitnessBytes{0x75,0xE9,0x33,0xD2,0x48,0x8B,0xC1};
enum class ExternalInstallState:uint32_t {Idle,Prepared,Accepted,PrepareRejected,ReceiptRejected,UnsupportedRequest,LegacyOwned};
enum class ExternalInstallOutcome:uint64_t {None,CommittedDetached,RolledBackDetached,FailedNoWriteDetached,CommittedHeldEvent};
// Immutable once published. The external debugger commits only this prepared
// CALL/JNE patch while holding the real attach breakpoint event.
struct ExternalInstallPlan {
    uint64_t version{},nonce{},processId{},processCreationTime{},gameBase{},allocatorAddress{},siteAddress{},relayAddress{},thunkAddress{},modulePinAddress{};
    uint64_t allocatorBytes{},siteBytes{},relayBytes{},thunkBytes{},originalProtection{},guardFnv{},checksum{},reserved[3]{};
    std::array<uint8_t,AllocatorBytes> guard{};
    std::array<uint8_t,7> original{},patch{};
    std::array<uint8_t,14> relayCode{};
    std::array<uint8_t,4> reservedBytes{};
};
struct ExternalInstallReceipt {
    uint64_t version{},nonce{},planChecksum{},processId{},processCreationTime{},outcome{},debugEventCode{},debugEventThread{};
    uint64_t originalProtection{},finalProtection{},bytesWritten{},patchVerified{},cacheFlushed{},detachSucceeded{},win32Error{};
    uint64_t exceptionCode{},firstChance{},checksum{};
};
static_assert(std::is_standard_layout_v<ExternalInstallPlan> && std::is_trivially_copyable_v<ExternalInstallPlan> &&
    sizeof(ExternalInstallPlan)==504 && offsetof(ExternalInstallPlan,checksum)==128 && offsetof(ExternalInstallPlan,guard)==160 &&
    offsetof(ExternalInstallPlan,original)==472 && offsetof(ExternalInstallPlan,patch)==479 && offsetof(ExternalInstallPlan,relayCode)==486);
static_assert(std::is_standard_layout_v<ExternalInstallReceipt> && std::is_trivially_copyable_v<ExternalInstallReceipt> &&
    sizeof(ExternalInstallReceipt)==144 && offsetof(ExternalInstallReceipt,checksum)==136);
#if CVR_RUST_VRIK
inline uint64_t ExternalPlanChecksum(const ExternalInstallPlan& value)noexcept{return cvr_ur_external_plan_checksum(&value);}
#else
inline uint64_t ExternalPlanChecksum(const ExternalInstallPlan& value)noexcept {
    const auto* bytes=reinterpret_cast<const uint8_t*>(&value);uint64_t hash=14695981039346656037ull;
    for(size_t i=0;i<sizeof(value);++i){const auto byte=i>=offsetof(ExternalInstallPlan,checksum) &&
        i<offsetof(ExternalInstallPlan,checksum)+sizeof(value.checksum)?uint8_t(0):bytes[i];hash=(hash^byte)*1099511628211ull;}return hash;
}
#endif
#if CVR_RUST_VRIK
inline uint64_t ExternalReceiptChecksum(const ExternalInstallReceipt& value)noexcept{return cvr_ur_external_receipt_checksum(&value);}
#else
inline uint64_t ExternalReceiptChecksum(const ExternalInstallReceipt& value)noexcept {
    return Fnv(reinterpret_cast<const uint8_t*>(&value),offsetof(ExternalInstallReceipt,checksum));
}
#endif
#if CVR_RUST_VRIK
inline bool ValidExternalPlan(const ExternalInstallPlan& value)noexcept{return cvr_ur_valid_external_plan(&value);}
#else
inline bool ValidExternalPlan(const ExternalInstallPlan& value)noexcept {
    if(value.version!=ExternalPlanVersion || !value.nonce || !value.processId || value.processId>UINT32_MAX || !value.processCreationTime ||
        !value.gameBase || value.gameBase>UINT64_MAX-AllocatorRva-AllocatorBytes || value.allocatorAddress!=value.gameBase+AllocatorRva ||
        value.siteAddress!=value.gameBase+WitnessRva || !value.relayAddress || !value.thunkAddress || !value.modulePinAddress ||
        value.modulePinAddress>value.thunkAddress || !value.thunkBytes || value.thunkBytes>4096 || value.thunkAddress>UINT64_MAX-value.thunkBytes ||
        value.allocatorBytes!=AllocatorBytes || value.siteBytes!=7 || value.relayBytes!=14 || value.relayAddress>UINT64_MAX-value.relayBytes ||
        (value.originalProtection!=0x20 && value.originalProtection!=0x40 && value.originalProtection!=0x80) ||
        value.guardFnv!=AllocatorFnv || Fnv(value.guard.data(),value.guard.size())!=AllocatorFnv || value.original!=OriginalWitnessBytes ||
        value.checksum!=ExternalPlanChecksum(value))return false;
    for(const auto reserved:value.reserved)if(reserved)return false;
    for(const auto reserved:value.reservedBytes)if(reserved)return false;
    constexpr size_t witness=WitnessRva-AllocatorRva;
    for(size_t i=0;i<value.original.size();++i)if(value.guard[witness+i]!=value.original[i])return false;
    if(value.patch[0]!=0xE8 || value.patch[5]!=0x75 || value.patch[6]!=RetryDisplacement || value.relayCode[0]!=0xFF || value.relayCode[1]!=0x25)return false;
    for(size_t i=2;i<6;++i)if(value.relayCode[i])return false;
    uint64_t target{};for(size_t i=0;i<8;++i)target|=uint64_t(value.relayCode[6+i])<<(i*8);
    if(target!=value.thunkAddress)return false;
    uint32_t encoded{};for(size_t i=0;i<4;++i)encoded|=uint32_t(value.patch[1+i])<<(i*8);
    const int64_t displacement=encoded<=INT32_MAX?int64_t(encoded):int64_t(encoded)-4294967296ll;
    const auto returnAddress=value.siteAddress+5;
    return displacement>=0?(returnAddress<=UINT64_MAX-uint64_t(displacement) && returnAddress+uint64_t(displacement)==value.relayAddress):
        (returnAddress>=uint64_t(-displacement) && returnAddress-uint64_t(-displacement)==value.relayAddress);
}
#endif
#if CVR_RUST_VRIK
inline bool MatchesExternalPatchedBody(const ExternalInstallPlan& value,const std::array<uint8_t,AllocatorBytes>& current)noexcept{
    return cvr_ur_matches_external_patched_body(&value,&current);
}
#else
inline bool MatchesExternalPatchedBody(const ExternalInstallPlan& value,const std::array<uint8_t,AllocatorBytes>& current)noexcept {
    constexpr size_t witness=WitnessRva-AllocatorRva;
    for(size_t i=0;i<current.size();++i)if(current[i]!=(i>=witness && i<witness+value.patch.size()?value.patch[i-witness]:value.guard[i]))return false;
    return true;
}
#endif
#if CVR_RUST_VRIK
inline bool ValidExternalReceipt(const ExternalInstallPlan& plan,const ExternalInstallReceipt& receipt)noexcept{
    return cvr_ur_valid_external_receipt(&plan,&receipt);
}
#else
inline bool ValidExternalReceipt(const ExternalInstallPlan& plan,const ExternalInstallReceipt& receipt)noexcept {
    return ValidExternalPlan(plan) && receipt.version==ExternalReceiptVersion && receipt.nonce==plan.nonce && receipt.planChecksum==plan.checksum &&
        receipt.processId==plan.processId && receipt.processCreationTime==plan.processCreationTime &&
        receipt.outcome==uint64_t(ExternalInstallOutcome::CommittedDetached) && receipt.debugEventCode==1 && receipt.debugEventThread &&
        receipt.debugEventThread<=UINT32_MAX && receipt.exceptionCode==0x80000003 && receipt.firstChance==1 &&
        receipt.originalProtection==plan.originalProtection && receipt.finalProtection==plan.originalProtection && receipt.bytesWritten==7 &&
        receipt.patchVerified==1 && receipt.cacheFlushed==1 && receipt.detachSucceeded==1 && !receipt.win32Error &&
        receipt.checksum==ExternalReceiptChecksum(receipt);
}
#endif
#if CVR_RUST_VRIK
inline bool ValidWitness(const Witness& value)noexcept{
    static_assert(sizeof(Witness)==72 && sizeof(Reservation)==56 && sizeof(Range)==40,"mirrored by upload_reservation.rs");
    return cvr_ur_valid_witness(&value);
}
#else
inline bool ValidWitness(const Witness& value)noexcept {
    return (value.flags&0x40) && value.ring && value.ring<=UINT64_MAX-0x18 && value.capacity && value.bytes &&
        value.bytes<=value.capacity && value.oldHead<=value.absoluteBegin && value.absoluteBegin<=UINT64_MAX-value.bytes &&
        value.absoluteEnd==value.absoluteBegin+value.bytes && value.absoluteEnd-value.oldHead<=value.capacity &&
        value.absoluteBegin%value.capacity<=value.capacity-value.bytes;
}
#endif
#if CVR_RUST_VRIK
template<size_t Rows=MaxRows,size_t Rings=MaxRings>class Ledger {
    static_assert(Rows && Rings);
public:
    explicit Ledger(uint64_t observerEpoch=1)noexcept:handle_(cvr_ur_ledger_new(Rows,Rings,observerEpoch)){}
    ~Ledger(){cvr_ur_ledger_free(handle_);}
    Ledger(const Ledger&)=delete;Ledger& operator=(const Ledger&)=delete;
    void Reset(uint64_t observerEpoch)noexcept{cvr_ur_ledger_reset(handle_,observerEpoch);}
    bool BeginGeneration(uint64_t ring,uint64_t generation,uint64_t capacity)noexcept{
        return cvr_ur_ledger_begin_generation(handle_,ring,generation,capacity);
    }
    void EndGeneration(uint64_t ring)noexcept{cvr_ur_ledger_end_generation(handle_,ring);}
    void QuarantineAll()noexcept{cvr_ur_ledger_quarantine_all(handle_);}
    Status Observe(const Witness& value,uint64_t thread,Row& output,bool capacityStable=true)noexcept{
        return Status(cvr_ur_ledger_observe(handle_,&value,thread,&output,capacityStable));
    }
    Status Find(uint64_t sequence,Reservation& output)const noexcept{return Status(cvr_ur_ledger_find(handle_,sequence,&output));}
    Status ValidateRange(const Reservation& reservation,uint64_t localOffset,uint64_t physicalOffset,
        uint64_t paddedBytes,uint64_t logicalBytes,uint64_t backingBytes,Range& output)const noexcept{
        return Status(cvr_ur_ledger_validate_range(handle_,&reservation,localOffset,physicalOffset,paddedBytes,logicalBytes,backingBytes,&output));
    }
    size_t Count()const noexcept{return cvr_ur_ledger_count(handle_);}
    const Row& At(size_t i)const noexcept{return cvr_ur_ledger_rows(handle_)[i];}
    bool Exhausted()const noexcept{return cvr_ur_ledger_exhausted(handle_);}
private:
    void* handle_;
};
#else
template<size_t Rows=MaxRows,size_t Rings=MaxRings>class Ledger {
    static_assert(Rows && Rings);
    struct Ring {uint64_t address{},generation{},capacity{};bool quarantined{};};
public:
    explicit Ledger(uint64_t observerEpoch=1)noexcept:epoch_(observerEpoch?observerEpoch:1){}
    void Reset(uint64_t observerEpoch)noexcept {
        rows_.fill({});rings_.fill({});epoch_=observerEpoch?observerEpoch:1;
        sequence_=0;count_=ringCount_=0;exhausted_=false;
    }
    // Only an exact arena lifecycle observation may supply this identity.
    // Earlier rows remain unknown; physical offsets never establish generation.
    bool BeginGeneration(uint64_t ring,uint64_t generation,uint64_t capacity)noexcept {
        if(!ring || ring>UINT64_MAX-0x18 || !generation || !capacity || exhausted_)return false;
        auto* state=FindRing(ring);
        if(state && generation<=state->generation){state->quarantined=true;return false;}
        if(!state)state=AddRing(ring);
        if(!state)return false;
        *state={ring,generation,capacity,false};return true;
    }
    void EndGeneration(uint64_t ring)noexcept {if(auto* state=FindRing(ring))state->quarantined=true;}
    void QuarantineAll()noexcept {for(size_t i=0;i<ringCount_;++i)rings_[i].quarantined=true;exhausted_=true;}
    Status Observe(const Witness& value,uint64_t thread,Row& output,bool capacityStable=true)noexcept {
        output={};if(!(value.flags&0x40))return Status::Ignored;
        if(count_==Rows || sequence_==UINT64_MAX){QuarantineAll();return Status::Capacity;}
        auto* state=FindRing(value.ring);
        Status status=Status::UnknownGeneration;
        if(!capacityStable || !ValidWitness(value)){
            status=Status::InvalidWitness;
            if(state)state->quarantined=true;
        }else{
            if(!state)state=AddRing(value.ring);
            if(!state)status=Status::Capacity;
            else if(exhausted_ || state->quarantined)status=Status::Quarantined;
            else if(state->capacity && state->capacity!=value.capacity){state->quarantined=true;status=Status::Quarantined;}
            else{state->capacity=value.capacity;status=state->generation?Status::Captured:Status::UnknownGeneration;}
        }
        output={++sequence_,epoch_,state?state->generation:0,value.ring,value.oldHead,value.absoluteBegin,value.absoluteEnd,value.bytes,
            value.capacity,value.capacity?value.absoluteBegin%value.capacity:0,thread,value.nativeCaller,value.nativeStack,value.flags,uint64_t(status),WitnessRva};
        rows_[count_++]=output;return status;
    }
    Status Find(uint64_t sequence,Reservation& output)const noexcept {
        output={};const auto* row=FindRow(sequence);if(!row)return Status::UnknownReservation;
        if(row->status!=uint64_t(Status::Captured))return Status(row->status);
        const auto* state=FindRing(row->ring);
        if(exhausted_ || !state || state->quarantined)return Status::Quarantined;
        if(!row->arenaGeneration || row->arenaGeneration!=state->generation || row->capacity!=state->capacity)return Status::GenerationMismatch;
        output={row->sequence,row->observerEpoch,row->arenaGeneration,row->ring,row->absoluteBegin,row->absoluteEnd,row->capacity};
        return Status::Captured;
    }
    Status ValidateRange(const Reservation& reservation,uint64_t localOffset,uint64_t physicalOffset,
        uint64_t paddedBytes,uint64_t logicalBytes,uint64_t backingBytes,Range& output)const noexcept {
        output={};Reservation actual;const auto status=Find(reservation.sequence,actual);
        if(status!=Status::Captured)return status;
        if(actual!=reservation)return Status::GenerationMismatch;
        const auto span=actual.absoluteEnd-actual.absoluteBegin;
        if(!paddedBytes || !logicalBytes || logicalBytes>paddedBytes || localOffset>span || paddedBytes>span-localOffset ||
            actual.absoluteBegin>UINT64_MAX-localOffset)return Status::RangeMismatch;
        const auto begin=actual.absoluteBegin+localOffset;
        if(begin>UINT64_MAX-paddedBytes || physicalOffset!=begin%actual.capacity || physicalOffset>actual.capacity ||
            paddedBytes>actual.capacity-physicalOffset || physicalOffset>backingBytes || paddedBytes>backingBytes-physicalOffset)
            return Status::RangeMismatch;
        output={begin,begin+paddedBytes,physicalOffset,paddedBytes,logicalBytes};return Status::Captured;
    }
    size_t Count()const noexcept{return count_;}
    const Row& At(size_t i)const noexcept{return rows_[i];}
    bool Exhausted()const noexcept{return exhausted_;}
private:
    Ring* FindRing(uint64_t ring)noexcept {for(size_t i=0;i<ringCount_;++i)if(rings_[i].address==ring)return &rings_[i];return nullptr;}
    const Ring* FindRing(uint64_t ring)const noexcept {for(size_t i=0;i<ringCount_;++i)if(rings_[i].address==ring)return &rings_[i];return nullptr;}
    Ring* AddRing(uint64_t ring)noexcept {
        if(ringCount_==Rings){QuarantineAll();return nullptr;}
        auto* result=&rings_[ringCount_++];*result={ring,0,0,exhausted_};return result;
    }
    const Row* FindRow(uint64_t sequence)const noexcept {
        if(!sequence || sequence>count_)return nullptr;
        const auto& row=rows_[size_t(sequence-1)];return row.sequence==sequence?&row:nullptr;
    }
    std::array<Row,Rows> rows_{};std::array<Ring,Rings> rings_{};
    uint64_t epoch_{1},sequence_{};size_t count_{},ringCount_{};bool exhausted_{};
};
#endif
// The real CALL at the witness supplies a valid unwind return address.
// The installer selects full enabled XSAVE state, or legacy FXSAVE only when
// OSXSAVE is absent. The fixed area includes alignment slack and is never grown.
struct alignas(16) ThunkFrame {
    uint64_t rax{},rcx{},rdx{},rbx{},rbp{},rsi{},rdi{},r8{},r9{},r10{},r11{},r12{},r13{},r14{},r15{},flags{};
    uint64_t nativeStack{},nativeCaller{};
    uint64_t xstateAddress{},xstateMask{},capacity{},xstateMode{};
    alignas(16) std::array<uint8_t,MaxXstateBytes+63> xstate{};
};
static_assert(offsetof(ThunkFrame,flags)==0x78 && offsetof(ThunkFrame,nativeStack)==0x80 &&
    offsetof(ThunkFrame,xstateAddress)==0x90 && offsetof(ThunkFrame,xstate)==0xB0 && sizeof(ThunkFrame)==0x40F0);
#if CVR_RUST_VRIK
// The runtime: rust/crates/cvr-render/src/upload_reservation_runtime.rs.
extern "C" {
void cvr_ur_rt_prepare_code_guards(uintptr_t);
bool cvr_ur_rt_compatible();
bool cvr_ur_rt_install(uintptr_t,bool);
bool cvr_ur_rt_remove(bool);
bool cvr_ur_rt_installed();
void cvr_ur_rt_poll_installation(uintptr_t);
bool cvr_ur_rt_begin_generation(uint64_t,uint64_t,uint64_t);
void cvr_ur_rt_end_generation(uint64_t);
uint64_t cvr_ur_rt_find(uint64_t,Reservation*);
uint64_t cvr_ur_rt_validate_range(const Reservation*,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,Range*);
void cvr_ur_rt_clear();
}
inline void PrepareCodeGuards(uintptr_t gameBase)noexcept{cvr_ur_rt_prepare_code_guards(gameBase);}
inline bool Compatible()noexcept{return cvr_ur_rt_compatible();}
// The host must establish process-thread quiescence for the seven-byte patch.
// The false default prevents activation during ordinary concurrent execution.
inline bool Install(uintptr_t gameBase,bool threadsQuiescent=false)noexcept{return cvr_ur_rt_install(gameBase,threadsQuiescent);}
inline bool Remove(bool threadsQuiescent=false)noexcept{return cvr_ur_rt_remove(threadsQuiescent);}
inline bool Installed()noexcept{return cvr_ur_rt_installed();}
// No patch is written here. Explicit requests prepare one retained relay/plan,
// then accept the detached debugger receipt after exact current-code checks.
// External removal is intentionally unsupported; retain the relay until exit.
inline void PollInstallation(uintptr_t gameBase=0)noexcept{cvr_ur_rt_poll_installation(gameBase);}
inline bool BeginGeneration(uint64_t ring,uint64_t generation,uint64_t capacity)noexcept{
    return cvr_ur_rt_begin_generation(ring,generation,capacity);
}
inline void EndGeneration(uint64_t ring)noexcept{cvr_ur_rt_end_generation(ring);}
inline Status Find(uint64_t sequence,Reservation& output)noexcept{return Status(cvr_ur_rt_find(sequence,&output));}
inline Status ValidateRange(const Reservation& reservation,uint64_t localOffset,uint64_t physicalOffset,uint64_t paddedBytes,
    uint64_t logicalBytes,uint64_t backingBytes,Range& output)noexcept{
    return Status(cvr_ur_rt_validate_range(&reservation,localOffset,physicalOffset,paddedBytes,logicalBytes,backingBytes,&output));
}
inline void Clear()noexcept{cvr_ur_rt_clear();}
#else
void PrepareCodeGuards(uintptr_t gameBase)noexcept;
bool Compatible()noexcept;
// The host must establish process-thread quiescence for the seven-byte patch.
// The false default prevents activation during ordinary concurrent execution.
bool Install(uintptr_t gameBase,bool threadsQuiescent=false)noexcept;
bool Remove(bool threadsQuiescent=false)noexcept;
bool Installed()noexcept;
// No patch is written here. Explicit requests prepare one retained relay/plan,
// then accept the detached debugger receipt after exact current-code checks.
// External removal is intentionally unsupported; retain the relay until exit.
void PollInstallation(uintptr_t gameBase=0)noexcept;
bool BeginGeneration(uint64_t ring,uint64_t generation,uint64_t capacity)noexcept;
void EndGeneration(uint64_t ring)noexcept;
Status Find(uint64_t sequence,Reservation& output)noexcept;
Status ValidateRange(const Reservation&,uint64_t localOffset,uint64_t physicalOffset,uint64_t paddedBytes,
    uint64_t logicalBytes,uint64_t backingBytes,Range& output)noexcept;
void Clear()noexcept;
#endif
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadReservationEnable;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadReservationCount;
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_UploadReservationDropped;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadReservationState;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadReservationPrepareRequest,CyberpunkVR_UploadReservationAcceptRequest;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadReservationExternalPlanSeq,CyberpunkVR_UploadReservationExternalReceiptSeq;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadReservationExternalInstallState;
__declspec(dllexport) extern cvr::stereo::upload_reservation::ExternalInstallPlan CyberpunkVR_UploadReservationExternalPlan;
__declspec(dllexport) extern cvr::stereo::upload_reservation::ExternalInstallReceipt CyberpunkVR_UploadReservationExternalReceipt;
__declspec(dllexport) extern std::array<cvr::stereo::upload_reservation::Row,cvr::stereo::upload_reservation::MaxRows> CyberpunkVR_UploadReservationRows;
}
