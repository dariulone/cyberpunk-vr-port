#pragma once
#include "Render/NativeShaderSnapshot.hpp"
#include "Render/NativeCommandRecording.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <memory>
#include <span>

namespace cvr::stereo::instance_snapshot_view {
inline constexpr UINT Slot=7,MaxVersions=2048;
inline constexpr uint64_t MaxBytes=64ull*1024*1024;
enum class Failure:uint32_t {None,Arguments,Scope,Layout,View,Range,Bytes,Capacity,Device,Recording,AlreadyRecorded,Retention,Changed,Exception};
struct Scope {
    uint64_t root{},list{},generation{},occurrence{};UINT eye=UINT_MAX;
    bool operator==(const Scope&)const=default;
};
struct Mapping {
    uint64_t sourceOffset{},targetOffset{},viewOffset{},copyBytes{},nextFree{},viewEnd{};
    bool operator==(const Mapping&)const=default;
};
namespace detail {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/instance_snapshot_view.rs (tools/single_pass_tests/instance_snapshot_view_test
// runs its cases against them in its Rust variant).
static_assert(sizeof(Scope)==40 && sizeof(Mapping)==48 && sizeof(D3D12_VERTEX_BUFFER_VIEW)==16,"mirrored by instance_snapshot_view.rs");
extern "C" {
bool cvr_isv_check(uint32_t,const void*);
bool cvr_isv_reserve(uint64_t,const D3D12_VERTEX_BUFFER_VIEW*,UINT,UINT,uint64_t,Mapping*);
}
inline bool ValidScope(const Scope& scope)noexcept{return cvr_isv_check(0,&scope);}
inline bool ViewShape(const D3D12_VERTEX_BUFFER_VIEW& view)noexcept{return cvr_isv_check(1,&view);}
inline bool Reserve(uint64_t cursor,const D3D12_VERTEX_BUFFER_VIEW& view,UINT firstInstance,UINT count,
    uint64_t budget,Mapping& output)noexcept {
    return cvr_isv_reserve(cursor,&view,firstInstance,count,budget,&output);
}
#else
inline bool ValidScope(const Scope& scope)noexcept {
    return scope.root && scope.list && scope.generation && scope.occurrence && scope.eye<2;
}
inline bool ViewShape(const D3D12_VERTEX_BUFFER_VIEW& view)noexcept {
    return view.BufferLocation && view.BufferLocation<=UINT64_MAX-view.SizeInBytes && !(view.BufferLocation%4) &&
        ((view.StrideInBytes==48 && view.SizeInBytes==6*1024*1024) ||
         (view.StrideInBytes==64 && view.SizeInBytes==2*1024*1024));
}
inline bool Reserve(uint64_t cursor,const D3D12_VERTEX_BUFFER_VIEW& view,UINT firstInstance,UINT count,
    uint64_t budget,Mapping& output)noexcept {
    output={};
    if(!ViewShape(view) || !count || !budget || budget>MaxBytes || cursor>budget)return false;
    const uint64_t logical=uint64_t(firstInstance)*view.StrideInBytes,bytes=uint64_t(count)*view.StrideInBytes;
    if(logical>view.SizeInBytes || bytes>view.SizeInBytes-logical)return false;
    const uint64_t remainder=cursor%view.StrideInBytes,padding=remainder?view.StrideInBytes-remainder:0;
    if(padding>budget-cursor)return false;
    const auto aligned=cursor+padding,target=aligned>logical?aligned:logical,base=target-logical;
    if(base>budget || view.SizeInBytes>budget-base || target>budget || bytes>budget-target)return false;
    output={logical,target,base,bytes,target+bytes,base+view.SizeInBytes};return true;
}
#endif
// Input-layout arithmetic only. Source PSO association is checked separately
// through shader_snapshot::Capture, not through these element fields alone.
bool Layout(std::span<const shader_snapshot::Input>,UINT stride)noexcept;
struct PlanData;
}
struct Input {
    Scope scope;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> pipeline;
    std::shared_ptr<const shader_snapshot::Snapshot> shader;
    D3D12_VERTEX_BUFFER_VIEW original{};
    D3D12_DRAW_INDEXED_ARGUMENTS draw{};
    // Owning CPU observation, exactly the contiguous instance records selected
    // by this draw. BuildPlan deep-copies it; no original GPU source is accessed.
    std::shared_ptr<const std::vector<uint8_t>> bytes;
};
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
// This is not native admission. Native callers must independently join the
// actual producer, source/recording/draw/layout, GPU-byte match and protected
// source epoch. Neither a CPU sample nor an owning view grants native permission.
bool BuildPlan(std::span<const Input>,Plan&,Failure&,uint64_t budget=MaxBytes);
struct DestinationRecording {
    uint64_t list{},generation{};
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> allocator;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    bool certifiedCurrentRecording{};
};
struct DestinationCompletion {
    DestinationRecording recording;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    // Trusted caller's exact submission and successful fence signal contract.
    // A completed but unrelated fence is not inferred to order the copies.
    bool submissionCertified{};
};
struct PrivateView {
    Scope scope;
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    Microsoft::WRL::ComPtr<IUnknown> owner;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> pipeline;
    std::shared_ptr<const shader_snapshot::Snapshot> shader;
    D3D12_VERTEX_BUFFER_VIEW vertex{},original{};
    D3D12_DRAW_INDEXED_ARGUMENTS draw{};
    Mapping mapping;
    uint64_t copyList{},copyGeneration{};UINT version{};
};
class Overlay {
public:
    Overlay();~Overlay();
    Overlay(const Overlay&)=delete;Overlay& operator=(const Overlay&)=delete;
    HRESULT Create(ID3D12Device*,const Plan&);
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
    friend bool ValidatePrivateViewAfterCompletion(const PrivateView&,const DestinationCompletion&);
};
bool ValidatePrivateView(const PrivateView&);
// First consumer follows copies on the same certified private recording. A
// foreign/later consumer requires an explicit completed destination submission.
bool ValidatePrivateViewForRecording(const PrivateView&,ID3D12GraphicsCommandList*,const DestinationRecording&);
bool ValidatePrivateViewAfterCompletion(const PrivateView&,const DestinationCompletion&);

// Copies captured at the original native draw, not an upload of a CPU sample.
// The callback must validate the actual source/read occurrence and record its
// copy into the supplied fresh COPY_DEST range under an initially zero predicate.
// Its original pre-submit validator is solely responsible for enabling that copy.
using RecordNativeCopy=bool(*)(void*,ID3D12GraphicsCommandList*,const Input&,ID3D12Resource*,uint64_t);
struct NativeCompletion {
    native_recording::Lease recording;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> queue;
    Microsoft::WRL::ComPtr<ID3D12Fence> fence;
    uint64_t value{};
    // Trusted caller certifies this exact source submission, queue and successful
    // fence signal. The stored native recording alone cannot prove those facts.
    bool submissionCertified{};
};
class GpuCaptureArena {
public:
    GpuCaptureArena();~GpuCaptureArena();
    GpuCaptureArena(const GpuCaptureArena&)=delete;GpuCaptureArena& operator=(const GpuCaptureArena&)=delete;
    HRESULT Create(ID3D12Device*,const native_recording::Lease&,UINT eye,uint64_t capacity=MaxBytes/2);
    bool Capture(ID3D12GraphicsCommandList*,const Input&,RecordNativeCopy,void*,PrivateView&);
    bool Finalize(ID3D12GraphicsCommandList*);
    UINT VersionCount()const;
    uint64_t RequiredBytes()const;
    uint64_t CopiedBytes()const;
    ID3D12Resource* Resource()const;
private:
    struct Storage;Microsoft::WRL::ComPtr<Storage> storage_;
    friend bool ValidateNativeView(const PrivateView&);
    friend bool ValidateNativeViewForRecording(const PrivateView&,ID3D12GraphicsCommandList*,const native_recording::Lease&);
    friend bool ValidateNativeViewAfterSubmission(const PrivateView&,const NativeCompletion&);
    friend bool ValidateNativeViewAfterCompletion(const PrivateView&,const NativeCompletion&);
};
// These checks prove private-copy ownership and coordinates under their stated
// recording, submission or completion contract.
// They do not prove that the original per-copy permission was enabled, or grant
// material readiness, a cross-list dependency, or suppression of native draws.
bool ValidateNativeView(const PrivateView&);
bool ValidateNativeViewForRecording(const PrivateView&,ID3D12GraphicsCommandList*,const native_recording::Lease&);
// Accepts an authenticated finalized copy after its caller-certified submission,
// without requiring GPU completion or a still-current native recording. The
// caller must prove actual submission and order its consumer on that same queue.
bool ValidateNativeViewAfterSubmission(const PrivateView&,const NativeCompletion&);
bool ValidateNativeViewAfterCompletion(const PrivateView&,const NativeCompletion&);
}
