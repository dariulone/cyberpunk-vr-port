#pragma once
#include "Render/NativeInstanceRing.hpp"
#include "Render/NativeGeometryLeases.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/NativeShaderSnapshot.hpp"
#include "Render/NativeUploadReservation.hpp"
#include "Render/InstanceSnapshotView.hpp"
#include <optional>

namespace cvr::stereo::instance_observer {
using NativeWriter=uint32_t(__fastcall*)(void*,const void*,uint32_t,uint32_t*);
using NativeReserve=uint32_t(__fastcall*)(void*,uint32_t);
using NativePublish=void(__fastcall*)(void*);
inline constexpr uint32_t WriterRva=0x337144,ReserveRva=0x939ACC,PublishRva=0x4F34FC;
inline constexpr size_t MaxCursors=4096,MaxWriters=4096;

enum class Status:uint64_t {
    Disabled,Observed,Code,Caller,Contention,Scope,Backing,Changed,Capacity,
    Reservation,Cache,Returned,Sample,NoProducer,Range,Pending,Parent,Layout,Exception
};
enum QueryFlag:uint64_t {OwnedCpu=1,BackingPinned=2,CurrentIa=4,CurrentRecording=8,CurrentParent=16,PublicationObserved=32};
enum class QueryField:size_t {
    Status,Flags,ObserverEpoch,SourceList,SourceGeneration,SourceOccurrence,SourceEye,Ring,RingGeneration,
    Resource,Handle,Kind,Stride,Width,ViewOffset,FirstInstance,Instances,AbsoluteBegin,ReservationSequence,
    ReservationBegin,ReservationEnd,ReservationHeadBefore,RetiredTail,PublishedStamp,PublishedHead,CpuBytes,
    ProducerList,ProducerGeneration,ProducerOccurrence,ProducerThread,ProducerStamp,LiveCpuBytes
};
struct QueryObservation {std::array<uint64_t,32> values{};};
static_assert(sizeof(QueryObservation)==32*sizeof(uint64_t));
namespace detail {
struct Cursor {uint32_t first{},end{};bool operator==(const Cursor&)const=default;};
struct ReservationRange {
    uint64_t sequence{},headBefore{},absoluteBegin{},absoluteEnd{},capacity{};
    uint32_t first{},instances{};
    bool operator==(const ReservationRange&)const=default;
};
// Only runtime's nonblocking gate around the genuine guarded native allocator
// makes headBefore exclusive. Later heads alone cannot create this receipt.
#if !CVR_RUST_VRIK
bool ReservationResult(uint64_t headBefore,uint32_t capacity,uint32_t instances,uint32_t returnedFirst,
    uint64_t actualHeadAfter,ReservationRange&)noexcept;
#endif
struct CachedCursor {
    uint64_t address{},ring{},ringGeneration{},thread{};
    ReservationRange reservation;
    Cursor next;
};
struct WriterRange {uint64_t absoluteBegin{};ReservationRange reservation;std::optional<CachedCursor> cache;};
#if !CVR_RUST_VRIK
bool ResolveWriter(uint64_t cursorAddress,uint64_t ring,uint64_t ringGeneration,uint64_t thread,uint32_t capacity,
    Cursor before,Cursor after,uint32_t returnedFirst,uint32_t instances,const std::optional<ReservationRange>& actualReservation,
    const std::optional<CachedCursor>& previous,WriterRange&)noexcept;
bool InstanceLayout(std::span<const shader_snapshot::Input>,uint32_t stride)noexcept;
#endif
struct CallData;
#if CVR_RUST_VRIK
// The plan: rust/crates/cvr-render/src/instance_observer_plan.rs (optional values handed over as pointers).
struct RustWriterRange {uint64_t absoluteBegin{};ReservationRange reservation;uint32_t hasCache{};CachedCursor cache;};
struct RustLayoutInput {UINT format,slot,offset,classification,step;};
static_assert(sizeof(ReservationRange)==48 && sizeof(CachedCursor)==88 && sizeof(RustWriterRange)==152,
    "mirrored by rust/crates/cvr-render/src/instance_observer_plan.rs");
extern "C" {
bool cvr_instance_reservation_result(uint64_t,uint32_t,uint32_t,uint32_t,uint64_t,ReservationRange*);
bool cvr_instance_resolve_writer(uint64_t,uint64_t,uint64_t,uint64_t,uint32_t,Cursor,Cursor,uint32_t,uint32_t,
    const ReservationRange*,const CachedCursor*,RustWriterRange*);
bool cvr_instance_layout(const RustLayoutInput*,size_t,uint32_t);
}
inline bool ReservationResult(uint64_t headBefore,uint32_t capacity,uint32_t instances,uint32_t returnedFirst,
    uint64_t actualHeadAfter,ReservationRange& output)noexcept{
    return cvr_instance_reservation_result(headBefore,capacity,instances,returnedFirst,actualHeadAfter,&output);
}
inline bool ResolveWriter(uint64_t cursorAddress,uint64_t ring,uint64_t ringGeneration,uint64_t thread,uint32_t capacity,
    Cursor before,Cursor after,uint32_t returnedFirst,uint32_t instances,const std::optional<ReservationRange>& actualReservation,
    const std::optional<CachedCursor>& previous,WriterRange& output)noexcept{
    RustWriterRange row{};
    const bool resolved=cvr_instance_resolve_writer(cursorAddress,ring,ringGeneration,thread,capacity,before,after,returnedFirst,instances,
        actualReservation?&*actualReservation:nullptr,previous?&*previous:nullptr,&row);
    output={};output.absoluteBegin=row.absoluteBegin;output.reservation=row.reservation;
    if(row.hasCache)output.cache=row.cache;
    return resolved;
}
inline bool InstanceLayout(std::span<const shader_snapshot::Input> inputs,uint32_t stride)noexcept{
    if(inputs.size()>32)return false;
    std::array<RustLayoutInput,32> rows{};
    for(size_t i=0;i<inputs.size();++i)rows[i]={inputs[i].format,inputs[i].slot,inputs[i].offset,inputs[i].classification,inputs[i].step};
    return cvr_instance_layout(rows.data(),inputs.size(),stride);
}
#endif
}
// Capture exact originals BEFORE Root installs any of the three entry detours.
// Captured guards remain valid when those entries subsequently use trampolines.
void PrepareCodeGuards(uintptr_t gameBase)noexcept;
bool Compatible()noexcept;
bool PrepareWitness(uintptr_t gameBase)noexcept;
// false freezes/checks/resumes the other process threads itself. true is only
// for a caller that already holds verified process-thread quiescence.
bool InstallWitness(uintptr_t gameBase,bool threadsQuiescent=false)noexcept;
bool WitnessInstalled()noexcept;
// Root reports actual EngineDetour installation: writer=1,reserve=2,publish=4.
// Partial installation never enables a producer observation.
void SetInstalled(uint32_t mask)noexcept;
class Call {
public:
    Call()=default;~Call();Call(Call&&)noexcept;Call& operator=(Call&&)noexcept;
    Call(const Call&)=delete;Call& operator=(const Call&)=delete;
    explicit operator bool()const noexcept;
private:
    std::shared_ptr<detail::CallData> data_;
    friend Call Enter(void*,const void*,uint32_t,uint32_t*,uintptr_t)noexcept;
    friend Call Reservation(void*,uint32_t,uintptr_t)noexcept;
    friend Call Publication(void*,uintptr_t)noexcept;
    friend void Returned(Call&,uint32_t)noexcept;
    friend void Returned(Call&)noexcept;
};
// Root ALWAYS forwards original arguments/results, including disabled/rejected
// observation. These scopes never wait for the observer gate. Contention makes
// proof loss immediately visible to every future return/query before forwarding.
// Switched off and nothing ever observed: Enter, Publication and Returned would all do nothing,
// so the detours forward straight to the original (they run for every instance-data write).
bool Dormant()noexcept;
Call Enter(void* ring,const void* source,uint32_t instances,uint32_t* cursor,uintptr_t nativeCaller)noexcept;
Call Reservation(void* ring,uint32_t instances,uintptr_t nativeCaller)noexcept;
Call Publication(void* ring,uintptr_t nativeCaller)noexcept;
void Returned(Call&,uint32_t nativeResult)noexcept;
void Returned(Call&)noexcept;

struct SourceCandidate {
    instance_ring::CpuCandidate cpu;
    Microsoft::WRL::ComPtr<IUnknown> owner;
    QueryObservation observation;
};
// Actual original direct draw only. Rejoins currently bound VB7, native record,
// PSO instance layout, recording and parent against the held producer backing.
// An owned CPU candidate is provisional. No future CPU no-overwrite, original
// submission protection, GPU-byte equality or copy permission is certified.
bool QueryAtSource(ID3D12GraphicsCommandList*,const geometry::DrawResources&,bool indexed,
    const native_recording::Lease&,const preparation::GraphParentRecord&,uint64_t occurrence,SourceCandidate&)noexcept;
// Before the ORIGINAL native submission only; validates held backing, the
// current observed CPU version and captured native submission identity. It
// cannot establish future no-overwrite or authorize an inserted GPU copy.
bool ValidateSource(const SourceCandidate&,ID3D12CommandQueue*)noexcept;
// Diagnostic read immediately before the captured ORIGINAL direct draw.
// The caller owns a zero-initialized predicate and enables it only after
// ValidateSource at that original submission. No future replay permission.
bool RecordGpuProbe(ID3D12GraphicsCommandList*,const SourceCandidate&,ID3D12Resource* readback,uint64_t offset,
    ID3D12Resource* predicate,uint64_t predicateOffset)noexcept;
// Same original-read admission, additionally writes an owning arena's fresh
// DEFAULT/COPY_DEST range. Metadata must match the actual held source draw.
// Readback remains diagnostic; neither copy requires CPU-byte readback first.
bool RecordGpuCapture(ID3D12GraphicsCommandList*,const SourceCandidate&,const instance_snapshot_view::Input&,
    ID3D12Resource* destination,uint64_t destinationOffset,ID3D12Resource* readback,uint64_t readbackOffset,
    ID3D12Resource* predicate,uint64_t predicateOffset)noexcept;
QueryObservation LastQuery()noexcept;
Status LastStatus()noexcept;
// Disable first. Clear fails closed while any real callback owns the gate.
void Clear()noexcept;
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverEnable;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverInstalled;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverResetRequest;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_NativeInstanceObserverFirstLoss;
void CvrNativeInstanceReservationObserve(const cvr::stereo::upload_reservation::ThunkFrame*)noexcept;
extern uint64_t CvrNativeInstanceReservationXstateMask;
extern uint32_t CvrNativeInstanceReservationXstateMode,CvrNativeInstanceReservationXstateReady;
void CvrNativeInstanceReservationThunk();
}
