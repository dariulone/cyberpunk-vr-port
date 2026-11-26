#pragma once
#include "Render/NativeCameraFlushState.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include <memory>

namespace cvr::stereo::native_descriptors {
enum class Failure:UINT {None,Arguments,Code,Recording,State,Heap,Table,Capacity,Changed,Retention,Reservation,Exception};
struct Range {UINT first{},count{},before{},after{};D3D12_CPU_DESCRIPTOR_HANDLE cpu{};D3D12_GPU_DESCRIPTOR_HANDLE gpu{};};
namespace detail {
struct Ring {uint64_t address{},heap{},capacity{},head{},tail{},outstanding{};};
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/descriptor_arena.rs (tools/single_pass_tests/native_descriptor_arena_test runs
// its cases against them in its Rust variant).
extern "C" {
bool cvr_da_valid_ring(const Ring*,uint64_t,UINT);
bool cvr_da_ring_result(const Ring*,const Ring*,UINT,UINT,uint64_t*);
bool cvr_da_plan(const camera_upload::flush_state::RustSnapshotView*,UINT,UINT,UINT,UINT64,UINT64,Range*);
bool cvr_da_advanced(const camera_upload::flush_state::RustSnapshotView*,const camera_upload::flush_state::RustSnapshotView*,const Range*);
}
#endif
#if CVR_RUST_VRIK
inline bool ValidRing(const Ring& ring,uint64_t heap,UINT heapCount){
    static_assert(sizeof(Ring)==48 && sizeof(Range)==32,"mirrored by descriptor_arena.rs");
    return cvr_da_valid_ring(&ring,heap,heapCount);
}
inline bool RingResult(const Ring& before,const Ring& after,UINT first,UINT count,uint64_t& absolute){
    return cvr_da_ring_result(&before,&after,first,count,&absolute);
}
inline bool Plan(const camera_upload::flush_state::Snapshot& state,UINT count,UINT heapCount,UINT stride,
    UINT64 cpu,UINT64 gpu,Range& result){
    std::vector<camera_upload::flush_state::RustTableView> tables;
    const auto view=camera_upload::flush_state::detail::View(state,tables);
    return cvr_da_plan(&view,count,heapCount,stride,cpu,gpu,&result);
}
inline bool Advanced(const camera_upload::flush_state::Snapshot& before,const camera_upload::flush_state::Snapshot& after,const Range& range){
    std::vector<camera_upload::flush_state::RustTableView> beforeTables,afterTables;
    const auto a=camera_upload::flush_state::detail::View(before,beforeTables);
    const auto b=camera_upload::flush_state::detail::View(after,afterTables);
    return cvr_da_advanced(&a,&b,&range);
}
#else
inline bool ValidRing(const Ring& ring,uint64_t heap,UINT heapCount){
    return ring.address && !(ring.address&7) && ring.heap==heap && heap && ring.capacity && ring.capacity<=heapCount &&
        ring.head>=ring.tail && ring.head-ring.tail<=ring.capacity && ring.outstanding && ring.outstanding<=1048576;
}
inline bool RingResult(const Ring& before,const Ring& after,UINT first,UINT count,uint64_t& absolute){
    absolute=0;
    if(!count || count>8192 || !ValidRing(before,before.heap,1048576) || !ValidRing(after,before.heap,1048576) ||
        before.address!=after.address || before.capacity!=after.capacity || first>=before.capacity || count>before.capacity-first ||
        after.head<before.head || after.head-before.head>before.capacity || after.tail<before.tail || after.tail>before.head)return false;
    const auto cycle=before.head-before.head%before.capacity;
    if(first>UINT64_MAX-cycle)return false;auto start=cycle+first;
    if(start<before.head){if(before.capacity>UINT64_MAX-start)return false;start+=before.capacity;}
    if(start>after.head || count>after.head-start)return false;
    absolute=start;return true;
}
inline bool Plan(const camera_upload::flush_state::Snapshot& state,UINT count,UINT heapCount,UINT stride,
    UINT64 cpu,UINT64 gpu,Range& result){
    result={};
    if(!count || count>8192 || !state.heap || !state.context || !stride || stride!=state.stride || !cpu || !gpu ||
        (state.dirty&state.graphicsMask) || state.rebind || !state.capacity || state.cursor>state.capacity || count>state.capacity-state.cursor ||
        UINT64(state.base)+state.capacity>heapCount)return false;
    const auto first=UINT64(state.base)+state.cursor,bytes=(first+count)*stride;
    if(bytes>UINT64_MAX-cpu || bytes>UINT64_MAX-gpu)return false;
    result={UINT(first),count,state.cursor,state.cursor+count,{SIZE_T(cpu+first*stride)},{gpu+first*stride}};return true;
}
inline bool Advanced(const camera_upload::flush_state::Snapshot& before,const camera_upload::flush_state::Snapshot& after,const Range& range){
    auto expected=before;expected.cursor=range.after;
    return range.count && range.cpu.ptr && range.gpu.ptr && range.first==UINT64(before.base)+before.cursor &&
        range.before==before.cursor && UINT64(range.before)+range.count==range.after &&
        range.after<=before.capacity && after==expected;
}
#endif
}
struct Tables {
    Microsoft::WRL::ComPtr<IUnknown> owner;
    std::vector<std::pair<UINT,D3D12_GPU_DESCRIPTOR_HANDLE>> bindings;
    native_recording::Lease recording;
    Range range;
    uint64_t heap{},base{},capacity{},borrowedMask{};
    UINT descriptors{};
    // source,parameter,count,immutable CPU heap,GPU handle,first source,
    // first parameter,reused. First-source labels refer to actual allocations.
    std::vector<std::array<uint64_t,8>> entries;
    // source,registry,ring,heap,capacity,headBefore,tailBefore,headAfter,tailAfter,
    // first,count,absoluteBegin,outstandingBefore,outstandingAfter,noWait,contextUnchanged.
    std::array<uint64_t,16> ring{};
};
class Cache {
public:
    Cache();~Cache();
    Cache(const Cache&)=delete;Cache& operator=(const Cache&)=delete;
private:
    struct Storage;std::unique_ptr<Storage> storage_;
    friend bool CopyTables(ID3D12GraphicsCommandList*,const scene_state::Bindings&,
        std::span<const descriptors::OwnedSourceTable>,uint64_t,Cache&,Tables&);
};
// Actual source draw only. Uses the current native cursor or a nonblocking
// reservation from the SAME heap's native ring. Never rewinds cursors, waits for
// a fence or changes heaps. Root tables and dirty/cache state remain unchanged.
// Large/sampler tables retain their original handles.
// These are same-recording descriptors, not a later-frame material lease.
bool CopyTables(ID3D12GraphicsCommandList*,const scene_state::Bindings&,
    std::span<const descriptors::OwnedSourceTable>,uint64_t source,Cache&,Tables&);
bool Bind(ID3D12GraphicsCommandList*,const Tables&);
bool ValidateSubmission(const Tables&,ID3D12CommandQueue*);
Failure LastFailure();
}
