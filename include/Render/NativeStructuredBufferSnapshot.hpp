#pragma once
#include <cstddef>
#include "Render/NativeSourceSrvBinding.hpp"
#include "Render/NativeIndirectCopy.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/NativeCopyWindows.hpp"
#include <atomic>
#include <algorithm>

namespace cvr::stereo::structured_buffer_snapshot {
enum class Layout:uint32_t {Dense,OriginalIndices};
enum class Extent:uint32_t {SelectedRange,FullOriginalView};
enum class Failure:uint32_t {None,Disabled,Arguments,Scope,Code,Parent,Source,Creation,Backing,State,Range,Capacity,
    Retention,Changed,Registration,RecoveryVerified,RecoveryFatal,AlreadyRecorded,Exception};
struct Request {
    source_srv::SourceSrvBacking source;UINT parameter{},offset{};
    uint64_t sourceOffset{};UINT bytes{};
    Extent extent=Extent::SelectedRange;
};
// Supplied by the trusted original source callback. Certifies the already valid
// ORIGINAL DEFAULT GPU read interval, with its unchanged native queue edges.
// A resource lease, initial state or read-only metadata cannot create this proof.
struct ReadEpochCertificate {uint64_t list{},generation{},occurrence{};bool certified{};};
struct PreparedCopy {
    Microsoft::WRL::ComPtr<IUnknown> owner;
    uint64_t list{},generation{},occurrence{},sourceOffset{},privateOffset{},copiedBytes{};
    // copiedBytes is the available private span; gpuCopiedBytes is the actual
    // transfer for this occurrence. Reuse still has its own native receipt.
    bool reused{};uint64_t gpuCopiedBytes{};
};
struct PrivateRange {
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    Microsoft::WRL::ComPtr<IUnknown> owner;
    uint64_t sourceOffset{},privateOffset{},copiedBytes{};
};
namespace detail {
inline constexpr uint64_t SourceBytes=1024*1024,Stride=128,MaxCopyBytes=4096,MaxArenaBytes=8*1024*1024,Alignment=16;
inline constexpr uint32_t AllowedHeapFlags=D3D12_HEAP_FLAG_DENY_RT_DS_TEXTURES|D3D12_HEAP_FLAG_DENY_NON_RT_DS_TEXTURES;
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/structured_snapshot.rs (tools/single_pass_tests/
// structured_buffer_snapshot_test runs its cases against them in its Rust variant).
struct Admission;struct FullVersion;
extern "C" {
bool cvr_sb_no_protected_session(HRESULT,bool);
bool cvr_sb_read_mask(const geometry::ReadState*,uint32_t,bool);
bool cvr_sb_recovery_mask(uint32_t,const geometry::ReadState*,uint32_t,uint32_t*);
bool cvr_sb_recovery_identity(const geometry::NativeBuffer*,const std::array<uint8_t,0xB0>*,const source_cbv_details::Context*,uint64_t,
    const geometry::NativeBuffer*,const std::array<uint8_t,0xB0>*,const source_cbv_details::Context*,uint64_t);
bool cvr_sb_copy_extent(uint32_t,uint32_t,uint64_t,uint64_t);
bool cvr_sb_range(const Admission*);
uint32_t cvr_sb_admit(const Admission*);
bool cvr_sb_reserve(uint32_t,uint32_t,uint32_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t*,uint64_t*,uint64_t*);
bool cvr_sb_reuse_matches(const FullVersion*,const copy_windows::Span*,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t);
bool cvr_sb_epoch(const ReadEpochCertificate*,uint64_t,uint64_t,uint64_t);
}
#endif
#if CVR_RUST_VRIK
inline bool NoProtectedSession(HRESULT result,bool present)noexcept{return cvr_sb_no_protected_session(result,present);}
#else
inline bool NoProtectedSession(HRESULT result,bool present)noexcept {
    return !present && (result==DXGI_ERROR_NOT_FOUND || result==E_NOINTERFACE || SUCCEEDED(result));
}
#endif
#if CVR_RUST_VRIK
inline bool ReadMask(geometry::ReadState state,uint32_t kind,bool ready=false)noexcept{return cvr_sb_read_mask(&state,kind,ready);}
#else
inline bool ReadMask(geometry::ReadState state,uint32_t kind,bool ready=false)noexcept{
    return (kind==0 || kind==3) && state.kind==kind && state.status==2 &&
        (state.value&D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) && !(state.value&~UINT(D3D12_RESOURCE_STATE_GENERIC_READ)) &&
        (!ready || (state.value&D3D12_RESOURCE_STATE_COPY_SOURCE));
}
#endif
#if CVR_RUST_VRIK
inline bool RecoveryMask(uint32_t previous,geometry::ReadState current,uint32_t kind,uint32_t& required)noexcept{
    return cvr_sb_recovery_mask(previous,&current,kind,&required);
}
#else
inline bool RecoveryMask(uint32_t previous,geometry::ReadState current,uint32_t kind,uint32_t& required)noexcept{
    required=0;if(!ReadMask({previous,2,kind},kind) || current.status!=2 || current.kind!=kind ||
        (current.value&~UINT(D3D12_RESOURCE_STATE_GENERIC_READ)))return false;
    required=previous|current.value;return true;
}
#endif
// Recovery may flush a genuine pending barrier only after the exact saved
// context and B0 identity still match. Pending is not a readable state proof.
#if CVR_RUST_VRIK
inline bool RecoveryIdentity(const geometry::NativeBuffer& expected,const std::array<uint8_t,0xB0>& savedRecord,
    const source_cbv_details::Context& savedContext,uint64_t savedAddress,const geometry::NativeBuffer& current,
    const std::array<uint8_t,0xB0>& currentRecord,const source_cbv_details::Context& currentContext,uint64_t currentAddress)noexcept{
    return cvr_sb_recovery_identity(&expected,&savedRecord,&savedContext,savedAddress,&current,&currentRecord,&currentContext,currentAddress);
}
#else
inline bool RecoveryIdentity(const geometry::NativeBuffer& expected,const std::array<uint8_t,0xB0>& savedRecord,
    const source_cbv_details::Context& savedContext,uint64_t savedAddress,const geometry::NativeBuffer& current,
    const std::array<uint8_t,0xB0>& currentRecord,const source_cbv_details::Context& currentContext,uint64_t currentAddress)noexcept{
    using Observation=source_cbv_details::Observation;
    return savedContext.observation==Observation::Known && currentContext.observation==Observation::Known &&
        savedContext.address && savedContext.list && savedContext.state && !savedContext.pending && !savedContext.submitted &&
        (savedContext.kind==1 || savedContext.kind==2) && currentContext.address==savedContext.address &&
        currentContext.list==savedContext.list && currentContext.state==savedContext.state && currentContext.kind==savedContext.kind &&
        !currentContext.submitted && expected.context==savedContext.address && current.context==expected.context &&
        expected.resource && current.resource==expected.resource && expected.gpu && current.gpu==expected.gpu &&
        expected.bytes && current.bytes==expected.bytes && expected.registry && current.registry==expected.registry &&
        expected.handle && current.handle==expected.handle && (expected.kind==0 || expected.kind==3) && current.kind==expected.kind &&
        savedAddress && currentAddress==savedAddress && savedRecord==currentRecord;
}
#endif
struct Admission {
    uint64_t resource{},gpu{},nativeResource{},nativeGpu{},nativeBytes{},viewOffset{},viewBytes{},sourceOffset{},bytes{};
    D3D12_RESOURCE_DESC desc{};D3D12_HEAP_PROPERTIES heap{};uint32_t heapFlags{},creationKind{};
    uint64_t creationHeap{},creationOffset{},creationBytes{};D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    geometry::ReadState state;uint32_t nativeKind{},handle{},stage{},shaderRegister{},parameter{},offset{};
    bool sourceCurrent{},creationKnown{},unprotected{},sameDevice{},recordingCurrent{};
    Extent extent=Extent::SelectedRange;Layout layout=Layout::Dense;
};
#if CVR_RUST_VRIK
inline bool CopyExtent(Extent extent,Layout layout,uint64_t offset,uint64_t bytes)noexcept{
    static_assert(sizeof(Admission)==280 && offsetof(Admission,desc)==72 && offsetof(Admission,view)==184 &&
        offsetof(Admission,state)==224 && offsetof(Admission,sourceCurrent)==260,"mirrored by structured_snapshot.rs");
    return cvr_sb_copy_extent(uint32_t(extent),uint32_t(layout),offset,bytes);
}
#else
inline bool CopyExtent(Extent extent,Layout layout,uint64_t offset,uint64_t bytes)noexcept{
    if(extent==Extent::SelectedRange)return bytes && bytes<=MaxCopyBytes;
    return extent==Extent::FullOriginalView && layout==Layout::OriginalIndices && !offset && bytes==SourceBytes;
}
#endif
#if CVR_RUST_VRIK
inline bool Range(const Admission& a)noexcept{return cvr_sb_range(&a);}
#else
inline bool Range(const Admission& a)noexcept{
    if(!a.resource || !a.gpu || a.gpu>UINT64_MAX-a.desc.Width || !CopyExtent(a.extent,a.layout,a.sourceOffset,a.bytes) ||
        a.bytes%Stride || a.sourceOffset%Stride || a.sourceOffset>a.desc.Width || a.bytes>a.desc.Width-a.sourceOffset ||
        a.viewOffset>a.desc.Width || a.viewBytes>a.desc.Width-a.viewOffset || a.sourceOffset<a.viewOffset ||
        a.sourceOffset-a.viewOffset>a.viewBytes || a.bytes>a.viewBytes-(a.sourceOffset-a.viewOffset))return false;
    return a.nativeResource==a.resource && a.nativeGpu>=a.gpu && a.nativeGpu-a.gpu<=a.desc.Width &&
        a.nativeBytes && a.nativeBytes<=a.desc.Width-(a.nativeGpu-a.gpu) && a.gpu+a.sourceOffset>=a.nativeGpu &&
        a.gpu+a.sourceOffset-a.nativeGpu<=a.nativeBytes && a.bytes<=a.nativeBytes-(a.gpu+a.sourceOffset-a.nativeGpu);
}
#endif
#if CVR_RUST_VRIK
inline Failure Admit(const Admission& a)noexcept{return Failure(cvr_sb_admit(&a));}
#else
inline Failure Admit(const Admission& a)noexcept{
    if(!a.sourceCurrent || !a.sameDevice || !a.recordingCurrent || !a.handle || a.handle>source_srv::MaxHandles ||
        a.stage!=source_srv::StageIndex || a.shaderRegister!=source_srv::ShaderRegister ||
        a.parameter!=source_srv::RootParameter || a.offset!=source_srv::TableOffset)return Failure::Source;
    if(!a.creationKnown || a.creationKind!=1 || a.creationHeap || a.creationOffset || a.creationBytes || !a.unprotected)return Failure::Creation;
    if(a.desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || a.desc.Width!=SourceBytes || a.desc.Height!=1 ||
        a.desc.DepthOrArraySize!=1 || a.desc.MipLevels!=1 || a.desc.Format!=DXGI_FORMAT_UNKNOWN || a.desc.SampleDesc.Count!=1 ||
        a.desc.SampleDesc.Quality || a.desc.Layout!=D3D12_TEXTURE_LAYOUT_ROW_MAJOR || a.desc.Flags ||
        (a.desc.Alignment && a.desc.Alignment!=D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT) || a.heap.Type!=D3D12_HEAP_TYPE_DEFAULT ||
        a.nativeGpu!=a.gpu || a.nativeBytes!=a.desc.Width ||
        a.heap.CPUPageProperty!=D3D12_CPU_PAGE_PROPERTY_UNKNOWN || a.heap.MemoryPoolPreference!=D3D12_MEMORY_POOL_UNKNOWN ||
        a.heap.CreationNodeMask>1 || a.heap.VisibleNodeMask>1 || (a.heapFlags&~AllowedHeapFlags))return Failure::Backing;
    if(a.view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER || a.view.Format!=DXGI_FORMAT_UNKNOWN || a.view.Buffer.Flags ||
        a.view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || a.view.Buffer.StructureByteStride!=Stride ||
        !a.view.Buffer.NumElements || a.view.Buffer.FirstElement>UINT64_MAX/Stride || a.viewOffset!=a.view.Buffer.FirstElement*Stride ||
        a.viewBytes!=uint64_t(a.view.Buffer.NumElements)*Stride || !Range(a))return Failure::Range;
    if(a.extent==Extent::FullOriginalView && (a.view.Buffer.FirstElement || a.view.Buffer.NumElements!=SourceBytes/Stride ||
        a.viewOffset || a.viewBytes!=SourceBytes))return Failure::Range;
    return ReadMask(a.state,a.nativeKind)?Failure::None:Failure::State;
}
#endif
#if CVR_RUST_VRIK
inline bool Reserve(uint64_t cursor,uint64_t capacity,uint64_t bytes,uint64_t& offset,uint64_t& end)noexcept{
    return cvr_sb_reserve(0,0,0,cursor,capacity,0,bytes,&offset,&end,nullptr);
}
#else
inline bool Reserve(uint64_t cursor,uint64_t capacity,uint64_t bytes,uint64_t& offset,uint64_t& end)noexcept{
    offset=end=0;if(!capacity || capacity>MaxArenaBytes || capacity%Alignment || cursor>capacity || cursor%Alignment ||
        !bytes || bytes>MaxCopyBytes || bytes%Stride || bytes>capacity-cursor)return false;
    offset=cursor;end=cursor+bytes;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool ReserveView(uint64_t cursor,uint64_t capacity,uint64_t sourceOffset,uint64_t bytes,
    uint64_t& offset,uint64_t& end,uint64_t& viewEnd)noexcept{
    return cvr_sb_reserve(1,0,0,cursor,capacity,sourceOffset,bytes,&offset,&end,&viewEnd);
}
#else
inline bool ReserveView(uint64_t cursor,uint64_t capacity,uint64_t sourceOffset,uint64_t bytes,
    uint64_t& offset,uint64_t& end,uint64_t& viewEnd)noexcept{
    offset=end=viewEnd=0;
    if(!capacity || capacity>MaxArenaBytes || cursor%Stride || sourceOffset%Stride || sourceOffset>=SourceBytes ||
        !bytes || bytes>MaxCopyBytes || bytes%Stride || bytes>SourceBytes-sourceOffset)return false;
    const auto target=(std::max)(cursor,sourceOffset),first=target-sourceOffset;
    if(first>capacity || SourceBytes>capacity-first)return false;
    offset=target;end=target+bytes;viewEnd=first+SourceBytes;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool ReserveExtent(Extent extent,Layout layout,uint64_t cursor,uint64_t capacity,uint64_t sourceOffset,uint64_t bytes,
                          uint64_t& offset,uint64_t& end,uint64_t& viewEnd)noexcept{
    return cvr_sb_reserve(2,uint32_t(extent),uint32_t(layout),cursor,capacity,sourceOffset,bytes,&offset,&end,&viewEnd);
}
#else
inline bool ReserveExtent(Extent extent,Layout layout,uint64_t cursor,uint64_t capacity,uint64_t sourceOffset,uint64_t bytes,
                          uint64_t& offset,uint64_t& end,uint64_t& viewEnd)noexcept{
    offset=end=viewEnd=0;if(!CopyExtent(extent,layout,sourceOffset,bytes))return false;
    if(extent==Extent::SelectedRange)return layout==Layout::OriginalIndices?
        ReserveView(cursor,capacity,sourceOffset,bytes,offset,end,viewEnd):
        layout==Layout::Dense && Reserve(cursor,capacity,bytes,offset,end);
    if(!capacity || capacity>MaxArenaBytes || capacity%Stride || cursor%Stride || cursor>capacity || bytes>capacity-cursor)return false;
    offset=cursor;end=viewEnd=cursor+bytes;return true;
}
#endif
// Internal copy records are populated only after an authorized GPU copy. A
// Window span is a no-write cache hint, never native source/read authority.
struct FullVersion {
    uint64_t list{},generation{},resource{},gpu{},destination{},occurrence{};
    bool recorded{};
};
#if CVR_RUST_VRIK
inline bool ReuseMatches(const FullVersion& prior,const copy_windows::Span& span,uint64_t list,uint64_t generation,
                         uint64_t resource,uint64_t gpu,uint64_t occurrence,uint64_t cursor,uint64_t capacity)noexcept{
    static_assert(sizeof(FullVersion)==56 && sizeof(copy_windows::Span)==56,"mirrored by structured_snapshot.rs");
    return cvr_sb_reuse_matches(&prior,&span,list,generation,resource,gpu,occurrence,cursor,capacity);
}
#else
inline bool ReuseMatches(const FullVersion& prior,const copy_windows::Span& span,uint64_t list,uint64_t generation,
                         uint64_t resource,uint64_t gpu,uint64_t occurrence,uint64_t cursor,uint64_t capacity)noexcept{
    return prior.recorded && list && generation && resource && gpu && occurrence &&
        prior.list==list && prior.generation==generation && prior.resource==resource && prior.gpu==gpu &&
        prior.occurrence && prior.occurrence<occurrence && prior.destination%Stride==0 &&
        capacity && capacity<=MaxArenaBytes && capacity%Stride==0 && cursor%Stride==0 && cursor<=capacity &&
        prior.destination<=cursor && SourceBytes<=cursor-prior.destination &&
        span.resource==resource && span.gpu==gpu && span.backing==SourceBytes && !span.offset && span.bytes==SourceBytes &&
        span.destination==prior.destination && span.occurrence==prior.occurrence;
}
#endif
#if CVR_RUST_VRIK
inline bool Epoch(const ReadEpochCertificate& epoch,uint64_t list,uint64_t generation,uint64_t occurrence)noexcept{
    static_assert(sizeof(ReadEpochCertificate)==32,"mirrored by structured_snapshot.rs");
    return cvr_sb_epoch(&epoch,list,generation,occurrence);
}
#else
inline bool Epoch(const ReadEpochCertificate& epoch,uint64_t list,uint64_t generation,uint64_t occurrence)noexcept{
    return epoch.certified && list && generation && occurrence && epoch.list==list && epoch.generation==generation && epoch.occurrence==occurrence;
}
#endif
}
class ActualCallScope {
public:
    ActualCallScope(ID3D12GraphicsCommandList*,uint64_t generation,uint64_t occurrence)noexcept;
    ~ActualCallScope();
    ActualCallScope(const ActualCallScope&)=delete;ActualCallScope& operator=(const ActualCallScope&)=delete;
    bool Active()const noexcept;
private:indirect_copy::detail::ScopeStamp stamp_;
};
class SnapshotArena {
public:
    SnapshotArena();~SnapshotArena();
    SnapshotArena(const SnapshotArena&)=delete;SnapshotArena& operator=(const SnapshotArena&)=delete;
    HRESULT Create(ID3D12Device*,ID3D12GraphicsCommandList*,uint64_t generation,uint64_t capacityBytes,Layout=Layout::Dense);
    // Once at native Seal. OriginalIndices also finishes shader-readable;
    // selected spans preserve original indices without a later overlay copy.
    bool Finalize(ID3D12GraphicsCommandList*);
    void Abort()noexcept;
    bool Finalized()const;uint64_t Bytes()const;
    Microsoft::WRL::ComPtr<ID3D12Resource> Resource()const;
private:
    struct Storage;Microsoft::WRL::ComPtr<Storage> storage_;
    friend bool PrepareAtActualCall(ID3D12GraphicsCommandList*,uint64_t,const Request&,const preparation::GraphParentRecord&,
        const ReadEpochCertificate&,SnapshotArena&,PreparedCopy&);
};
bool Compatible();
// Dedicated committed source only. Prepares complete ownership and the bounded
// COM-relative span before native registration or any inserted GPU command.
// FullOriginalView is an explicit 1 MiB OriginalIndices-only mode. A recorded
// full view can be reused inside this arena's same recording no-write window;
// every occurrence still needs fresh native source, state, parent and epoch
// authorization. Eight full versions fit the unchanged 8 MiB arena limit.
bool PrepareAtActualCall(ID3D12GraphicsCommandList*,uint64_t root,const Request&,const preparation::GraphParentRecord&,
    const ReadEpochCertificate&,SnapshotArena&,PreparedCopy&);
bool ValidateAtActualCall(ID3D12GraphicsCommandList*,const PreparedCopy&);
bool RecordAtActualCall(ID3D12GraphicsCommandList*,const PreparedCopy&,PrivateRange&);
// Exposes a recorded original-index view, not permission to consume it. The
// consumer still requires finalized source ordering and verified index bounds.
bool RecordedView(const PreparedCopy&,const PrivateRange&,D3D12_SHADER_RESOURCE_VIEW_DESC&);
bool ValidateSourceSubmission(const PreparedCopy&,ID3D12CommandQueue*);
Failure LastFailure();
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_StructuredBufferSnapshotsEnable;
__declspec(dllexport) extern uint64_t CyberpunkVR_StructuredBufferFinalizeDiagnostic[8];
}
