#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>

namespace cvr::stereo::instance_ring {
struct Code {uint32_t rva{},bytes{};uint64_t fnv{};};
// 2.5: the ring constructor/destructor only destroy their callbacks through sub_142004BA0 (same ring fields
// and constants); the stamp pair takes the registry spinlock (+0x13BA9C0 -> +0x140E9D0) through a guard object
// and reads the 24-byte slot records at +0x1408950 (2.31 +0x13B4940), version at +0x10. Whole functions.
inline constexpr std::array<Code,8> NativeCode{{
    {0x80D5A0,0x162,0xE8E01449FD6BCC96ull},
    {0x80D704,0x7D,0xF852EC5B6174D66Eull},
    {0x337144,0xDB,0x5AE0D2CD64C9FC88ull},
    {0x939ACC,0x51,0x4251259E7ABD2CB6ull},
    {0x4F3498,0x64,0xCBEF2DC8DE9C716Full},
    {0x4F34FC,0xDC,0x5DD2A8E324AC5BFAull},
    {0x3E6A48,0x77,0xFB1205A8FC3A5C47ull},
    {0x3E6504,0xA9,0x351642C3D6C0138Cull}
}};
inline constexpr uint32_t NormalWriterReturn=0x335991,SkinWriterReturn=0x333CDF;

// Numeric observations never acquire a native pointer, pin an allocation, or
// establish producer publication, no-overwrite, submission or GPU copy permission.
// head/tail/capacity are INSTANCE counts, not bytes. Native ring offsets are:
// handle=0,mapped=8,capacity=0x10,stride=0x14,retiredTail=0x38,head=0x40.
struct RingObservation {
    uint64_t ring{},generation{},resource{},gpu{},mapped{},registry{};
    uint64_t allocation{},allocator{},heap{},width{},capacity{},head{},tail{};
    uint32_t handle{},stride{},kind{};
    bool operator==(const RingObservation&)const=default;
};
struct Scope {
    uint64_t list{},generation{},occurrence{},thread{},context{},registry{},stamp{};
    bool operator==(const Scope&)const=default;
};
struct WriterTicket {
    uint64_t id{},observerEpoch{},ring{},ringGeneration{};
    Scope scope;
    bool operator==(const WriterTicket&)const=default;
};
struct CompletedWriter {
    WriterTicket writer;
    uint64_t absoluteBegin{};
    uint32_t firstInstance{},instances{};
};
// Exactly one native 16-byte {submission stamp, absolute head} publication.
// These labels are not a submission/completion certificate.
struct Publication {Scope scope;uint64_t head{};bool operator==(const Publication&)const=default;};
struct ReadRequest {
    WriterTicket writer;
    // Separate source labels. Their association with the exact actual IA
    // resource/range and recording is a native caller proof, not this ledger.
    Scope source;
    uint64_t absoluteBegin{};
    uint32_t firstInstance{},instances{};
    bool operator==(const ReadRequest&)const=default;
};
struct Range {uint64_t absoluteEnd{},byteOffset{},bytes{};};
bool InstanceRange(const RingObservation&,uint64_t absoluteBegin,uint32_t firstInstance,uint32_t instances,Range&)noexcept;

enum class Status:uint32_t {
    Observed,Completed,Published,Recreated,UnknownRing,InvalidObservation,Changed,
    InvalidScope,UnknownWriter,Pending,RangeMismatch,Overlap,Retired,UnknownWrite,Capacity,Exception
};
struct Limits {
    size_t rings=64,pendingWriters=256,ranges=4096,publications=4096;
    size_t writerBytes=65536,liveCpuBytes=16*1024*1024;
};
namespace detail {struct Payload;}
class CpuCandidate {
public:
    explicit operator bool()const noexcept;
    // Remains the immutable observed CPU version after reuse/recreation. Call
    // Query again to test current provenance; held bytes grant no native access.
    std::span<const uint8_t> Bytes()const noexcept;
    const CompletedWriter& Producer()const noexcept;
    const ReadRequest& Source()const noexcept{return source_;}
    const Publication& Published()const noexcept{return publication_;}
#if CVR_RUST_VRIK
    // The bytes are rust/crates/cvr-render/src/instance_ring.rs's immutable payload, one reference held here.
    CpuCandidate()=default;
    CpuCandidate(const CpuCandidate&)noexcept;
    CpuCandidate& operator=(const CpuCandidate&)noexcept;
    CpuCandidate(CpuCandidate&&)noexcept;
    CpuCandidate& operator=(CpuCandidate&&)noexcept;
    ~CpuCandidate();
    // A query the Rust instance observer made (rust/crates/cvr-render/src/instance_observer_native.rs): its payload
    // reference handed over with the producer, the request, the publication and the subrange.
    static CpuCandidate Adopt(const void* payload,const CompletedWriter& producer,const ReadRequest& source,
        const Publication& publication,size_t offset,size_t bytes)noexcept;
private:
    const void* data_{};
    CompletedWriter producer_;
#else
private:
    std::shared_ptr<const detail::Payload> data_;
#endif
    ReadRequest source_;
    Publication publication_;
    size_t offset_{},bytes_{};
    friend class Ledger;
};
// Caller serializes ledger operations. Complete accepts only bytes sampled from
// the actual mapped destination AFTER the genuine native writer returned. It
// copies them, never reads native memory, and cannot certify that sampling fact.
// Genuine backing ownership, producer/source joins and the original GPU read
// epoch remain external caller proofs. No operation grants copy permission.
class Ledger {
public:
    explicit Ledger(Limits={});
    ~Ledger();
    Ledger(const Ledger&)=delete;Ledger& operator=(const Ledger&)=delete;
    Status Observe(const RingObservation&)noexcept;
    Status Begin(const RingObservation&,const Scope&,WriterTicket&)noexcept;
    Status QualifyWrite(const RingObservation&,const WriterTicket&,uint64_t absoluteBegin,uint32_t firstInstance,uint32_t instances)noexcept;
    Status ValidateWrite(const WriterTicket&)const noexcept;
    // Only after this exact native writer has returned. Its known range was
    // invalidated before writing; dropping its sample preserves other writers.
    Status DiscardCompleted(const WriterTicket&)noexcept;
    // A guarded native cached-write branch supplies a physical range even if
    // its earlier absolute reservation was not observed. It invalidates only.
    Status InvalidatePhysical(const RingObservation&,uint32_t firstInstance,uint32_t instances)noexcept;
    Status FinishUnobserved(const RingObservation&,const WriterTicket&,uint32_t firstInstance,uint32_t instances)noexcept;
    Status Complete(const RingObservation&,const CompletedWriter&,std::span<const uint8_t> observedMappedBytes)noexcept;
    Status Publish(const RingObservation&,const Publication&)noexcept;
    // Clears local CPU provenance after a genuine NEW native reservation. It
    // keeps the same native lifecycle generation; numeric Observe cannot do it.
    Status RestartObservation(const RingObservation&)noexcept;
    // An actual reservation/reuse observation invalidates intersecting prior
    // versions. The absolute range must come from the native reservation, not
    // an inference from a later head or a matching physical address.
    Status Reuse(const RingObservation&,uint64_t absoluteBegin,uint32_t instances,const WriterTicket* activeWriter=nullptr)noexcept;
    void Unknown(uint64_t ring)noexcept;
    void Destroy(uint64_t ring)noexcept;
    void Reset()noexcept;
    Status Query(const RingObservation&,const ReadRequest&,CpuCandidate&)noexcept;
    // Tracks cache membership only, not permission to read native memory.
    // Expires on eviction/retirement even while immutable CPU bytes are held.
    std::weak_ptr<const WriterTicket> CompletedPresence(const WriterTicket&)const noexcept;
    size_t LiveCpuBytes()const noexcept;
private:
    struct Data;std::unique_ptr<Data> data_;
};
}
