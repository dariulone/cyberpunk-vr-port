#pragma once
#include "Render/MultilayerSelectorPlan.hpp"
#include "Render/NativeStructuredBufferSnapshot.hpp"
#include <memory>
#include <span>

namespace cvr::stereo::structured_snapshot_view {
inline constexpr uint64_t Stride=128,Elements=8192,DomainBytes=Elements*Stride,MaxBytes=8*1024*1024;
inline constexpr UINT MaxVersions=2048;
enum class Failure:uint32_t {None,Arguments,Scope,View,B4,ReadPlan,Range,Owners,Completion,Capacity,Device,Recording,AlreadyRecorded,Retention,Changed,Exception};
struct Scope {
    uint64_t root{},list{},generation{},occurrence{};UINT eye=UINT_MAX;
    bool operator==(const Scope&)const=default;
};
struct SourceCompletion {
    Scope scope;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    // Caller-provided contracts for the actual source submission and
    // SnapshotArena::Finalize. Not inferred from fence/owner/shape labels.
    bool submissionCertified{},finalized{};
};
struct Input {
    Scope scope,privateB4Scope;
    structured_buffer_snapshot::PrivateRange dense;
    structured_buffer_snapshot::PreparedCopy copy;
    multilayer_selectors::StructuredView originalView;
    multilayer_selectors::ReadPlan readPlan;
    std::span<const uint8_t> privateB4Bytes;
    SourceCompletion completion;
    // Caller-provided contracts: these bytes matched this occurrence's actual
    // GPU b4 and all original t5 index paths belong to the bounded exact family.
    bool privateB4GpuMatchVerified{},allRecordPathsBounded{};
};
struct Mapping {
    uint64_t targetOffset{},privateFirstElement{},copyBytes{},nextFree{},viewEnd{};
    bool operator==(const Mapping&)const=default;
};
namespace detail {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/structured_snapshot.rs (tools/single_pass_tests/structured_snapshot_view_math
// runs its cases against them in its Rust variant).
static_assert(sizeof(Scope)==40 && sizeof(Mapping)==40,"mirrored by structured_snapshot.rs");
extern "C" {
bool cvr_ssv_check(uint32_t,const D3D12_SHADER_RESOURCE_VIEW_DESC*,const D3D12_SHADER_RESOURCE_VIEW_DESC*,const Scope*);
bool cvr_ssv_reserve(uint64_t,uint32_t,uint32_t,uint64_t,Mapping*);
}
inline bool SameBufferView(const D3D12_SHADER_RESOURCE_VIEW_DESC& a,const D3D12_SHADER_RESOURCE_VIEW_DESC& b)noexcept{
    return cvr_ssv_check(0,&a,&b,nullptr);
}
inline bool ValidScope(const Scope& scope)noexcept{return cvr_ssv_check(1,nullptr,nullptr,&scope);}
inline bool Reserve(uint64_t cursor,uint32_t firstRecord,uint32_t bytes,uint64_t budget,Mapping& output)noexcept {
    return cvr_ssv_reserve(cursor,firstRecord,bytes,budget,&output);
}
#else
inline bool SameBufferView(const D3D12_SHADER_RESOURCE_VIEW_DESC& a,const D3D12_SHADER_RESOURCE_VIEW_DESC& b)noexcept{
    return a.ViewDimension==D3D12_SRV_DIMENSION_BUFFER && b.ViewDimension==a.ViewDimension && a.Format==b.Format &&
        a.Shader4ComponentMapping==b.Shader4ComponentMapping && a.Buffer.FirstElement==b.Buffer.FirstElement &&
        a.Buffer.NumElements==b.Buffer.NumElements && a.Buffer.StructureByteStride==b.Buffer.StructureByteStride && a.Buffer.Flags==b.Buffer.Flags;
}
inline bool ValidScope(const Scope& scope)noexcept{return scope.root && scope.list && scope.generation && scope.occurrence && scope.eye<2;}
inline bool Reserve(uint64_t cursor,uint32_t firstRecord,uint32_t bytes,uint64_t budget,Mapping& output)noexcept {
    output={};
    uint64_t target{},end{},viewEnd{};
    if(firstRecord>=Elements || !structured_buffer_snapshot::detail::ReserveView(cursor,budget,uint64_t(firstRecord)*Stride,bytes,target,end,viewEnd))return false;
    output={target,(target-uint64_t(firstRecord)*Stride)/Stride,bytes,end,viewEnd};return true;
}
#endif
struct PlanData;
}
class Plan {
public:
    UINT VersionCount()const noexcept;
    uint64_t RequiredBytes()const noexcept;
    uint64_t CopiedBytes()const noexcept;
    const Mapping* VersionMapping(UINT)const noexcept;
    const Scope* VersionScope(UINT)const noexcept;
private:
    std::shared_ptr<const detail::PlanData> data_;
    friend bool BuildPlan(std::span<const Input>,Plan&,Failure&,uint64_t);
    friend class Overlay;
};
// No original resource access, shader/b4 rewrite, mapping or GPU copy. Only
// nonempty owned dense snapshots are admitted. Output is atomic on failure.
bool BuildPlan(std::span<const Input>,Plan&,Failure&,uint64_t budget=MaxBytes);
struct DestinationRecording {
    uint64_t list{},generation{};
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    // Caller-provided contract for this exact current DIRECT list/allocator
    // generation, queue association and actual CommandResources submission.
    bool certifiedCurrentRecording{};
};
struct PrivateView {
    Scope scope;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    Microsoft::WRL::ComPtr<IUnknown> owner;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    uint64_t sourceOffset{},denseOffset{},targetOffset{},copiedBytes{},copyList{},copyGeneration{};
    UINT version{},firstRecord{},recordSpan{},recordCount{};
    std::array<uint32_t,multilayer_selectors::MaxRecords> records{};
    std::array<uint8_t,multilayer_selectors::B4Bytes> privateB4Bytes{};
};
class Overlay {
public:
    Overlay();~Overlay();
    Overlay(const Overlay&)=delete;Overlay& operator=(const Overlay&)=delete;
    HRESULT Create(ID3D12Device*,const Plan&);
    // Reuses original-position GPU copies already finalized and completed on
    // their native source lists. No new allocation, GPU copy, list or fence.
    // Caller must verify RecordedView against each exact source copy first.
    bool AdoptSources(ID3D12Device*,std::span<const Input>,Failure&);
    // Once, after every actual source fence has completed. Retains genuine
    // owners before copying only the selected spans from finalized private
    // COPY_SOURCE buffers. Target finishes PS_RESOURCE | COPY_SOURCE.
    bool Record(ID3D12GraphicsCommandList*,const DestinationRecording&,Failure&);
    bool GetView(UINT,PrivateView&)const;
    UINT VersionCount()const noexcept;
    uint64_t RequiredBytes()const noexcept;
    uint64_t CopiedBytes()const noexcept;
    bool ValidateDestinationSubmission(const DestinationRecording&,ID3D12CommandQueue*)const;
private:
    struct Storage;Microsoft::WRL::ComPtr<Storage> storage_;
    friend bool ValidatePrivateView(const PrivateView&);
    friend bool ValidatePrivateViewForRecording(const PrivateView&,ID3D12GraphicsCommandList*,const DestinationRecording&);
};
// Genuine overlay owner, recorded stage and immutable descriptor/range/b4/
// record association. Native ownership is the input caller's contract, not an
// owner-shape test or GPU-consumer proof. No source GPU-address nomination.
bool ValidatePrivateView(const PrivateView&);
// First consumer must follow the overlay copies on this exact current recording.
// A foreign/deferred consumer needs its own explicit completed destination-fence
// ordering contract; shape validation alone does not supply that contract.
bool ValidatePrivateViewForRecording(const PrivateView&,ID3D12GraphicsCommandList*,const DestinationRecording&);
}
