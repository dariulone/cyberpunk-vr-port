#pragma once
#include "Render/NativeMaterialBinding.hpp"

namespace cvr::stereo::material_read {
namespace detail {
inline constexpr uint32_t ShaderRead=D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER;
inline constexpr uint32_t CopyRead=D3D12_RESOURCE_STATE_COPY_SOURCE;
inline constexpr uint32_t AllowedReads=ShaderRead|D3D12_RESOURCE_STATE_INDEX_BUFFER|
    D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|
    D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT|CopyRead;
#if CVR_RUST_VRIK
// The rules below: rust/crates/cvr-render/src/material.rs (tools/single_pass_tests/material_read_test runs its cases
// against them in its Rust variant).
extern "C" {
bool cvr_mat_tracked_class(uint32_t);
bool cvr_mat_copy_read_mask(const geometry::ReadState*,uint32_t,uint32_t,uint32_t*);
bool cvr_mat_has_read_state(const geometry::ReadState*,uint32_t,uint32_t,uint32_t);
bool cvr_mat_recovery_read_mask(const geometry::ReadState*,uint32_t,uint32_t,uint32_t*);
bool cvr_mat_native_identity(const material_binding::Snapshot*,const geometry::NativeBuffer*);
bool cvr_mat_submission_heap(uint32_t,uint32_t);
bool cvr_mat_needs_buffer_receipt(uint32_t,uint32_t);
bool cvr_mat_submission_record(geometry::MemoryReader,void*,uint64_t,const material_binding::Snapshot*,bool);
bool cvr_mat_source_range(uint64_t,uint64_t,uint64_t,uint64_t);
bool cvr_mat_destination_range(uint64_t,uint64_t);
}
#endif
#if CVR_RUST_VRIK
inline bool TrackedClass(uint32_t kind){return cvr_mat_tracked_class(kind);}
#else
inline bool TrackedClass(uint32_t kind){return kind<16 && kind!=1 && kind!=2 && kind!=4 && kind!=5;}
#endif
#if CVR_RUST_VRIK
inline bool CopyReadMask(geometry::ReadState state,uint32_t heap,uint32_t kind,uint32_t& required){
    return cvr_mat_copy_read_mask(&state,heap,kind,&required);
}
#else
inline bool CopyReadMask(geometry::ReadState state,uint32_t heap,uint32_t kind,uint32_t& required){
    required=0;
    if(state.kind!=kind || kind>=16 || kind==4)return false;
    if(heap==D3D12_HEAP_TYPE_UPLOAD){
        if(state.status!=1 || state.value!=D3D12_RESOURCE_STATE_GENERIC_READ)return false;
    }else if(heap!=D3D12_HEAP_TYPE_DEFAULT || !TrackedClass(kind) || state.status!=2 ||
        !(state.value&ShaderRead) || (state.value&~AllowedReads))return false;
    required=state.value|ShaderRead|CopyRead;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool HasReadState(geometry::ReadState state,uint32_t heap,uint32_t kind,uint32_t required){
    return cvr_mat_has_read_state(&state,heap,kind,required);
}
#else
inline bool HasReadState(geometry::ReadState state,uint32_t heap,uint32_t kind,uint32_t required){
    uint32_t unused{};
    return required && !(required&~AllowedReads) && (required&ShaderRead) &&
        CopyReadMask(state,heap,kind,unused) && (state.value&required)==required;
}
#endif
// Recovery is only for an attempted native mutation. It needs a known tracked
// read state and retains every read permission from before and after the attempt.
#if CVR_RUST_VRIK
inline bool RecoveryReadMask(geometry::ReadState state,uint32_t kind,uint32_t previous,uint32_t& required){
    return cvr_mat_recovery_read_mask(&state,kind,previous,&required);
}
#else
inline bool RecoveryReadMask(geometry::ReadState state,uint32_t kind,uint32_t previous,uint32_t& required){
    required=0;
    if(!TrackedClass(kind) || state.kind!=kind || state.status!=2 || (state.value&~AllowedReads) ||
        !(previous&ShaderRead) || (previous&~AllowedReads))return false;
    required=state.value|previous|ShaderRead;return true;
}
#endif
#if CVR_RUST_VRIK
inline bool NativeIdentity(const material_binding::Snapshot& saved,const geometry::NativeBuffer& current){
    return cvr_mat_native_identity(&saved,&current);
}
#else
inline bool NativeIdentity(const material_binding::Snapshot& saved,const geometry::NativeBuffer& current){
    const auto& r=saved.record;
    return saved.handle && saved.handle<=0x100000 && saved.registry && saved.context && r.kind!=4 && r.kind<16 &&
        r.resource && r.gpu && r.bytes>=material_binding::LogicalBytes &&
        current.resource==r.resource && current.gpu==r.gpu && current.bytes==r.bytes &&
        current.registry==saved.registry && current.context==saved.context.context &&
        current.handle==saved.handle && current.kind==r.kind;
}
#endif
#if CVR_RUST_VRIK
inline bool SubmissionHeap(uint32_t heap,uint32_t kind){return cvr_mat_submission_heap(heap,kind);}
#else
inline bool SubmissionHeap(uint32_t heap,uint32_t kind){
    return kind<16 && kind!=4 && (heap==D3D12_HEAP_TYPE_UPLOAD ||
        (heap==D3D12_HEAP_TYPE_DEFAULT && TrackedClass(kind)));
}
#endif
#if CVR_RUST_VRIK
inline bool NeedsBufferReceipt(uint32_t heap,uint32_t kind){return cvr_mat_needs_buffer_receipt(heap,kind);}
#else
inline bool NeedsBufferReceipt(uint32_t heap,uint32_t kind){
    return heap==D3D12_HEAP_TYPE_DEFAULT && kind==1;
}
#endif
#if CVR_RUST_VRIK
inline bool SubmissionRecordOnce(geometry::MemoryReader reader,void* user,uint64_t game,
                                 const material_binding::Snapshot& saved){
    return cvr_mat_submission_record(reader,user,game,&saved,true);
}
#else
inline bool SubmissionRecordOnce(geometry::MemoryReader reader,void* user,uint64_t game,
                                 const material_binding::Snapshot& saved){
    const auto& r=saved.record;
    if(!reader || !game || game>UINT64_MAX-0x3BCDCB0 || !saved.registry ||
        !saved.handle || saved.handle>0x100000 || r.kind>=16 || r.kind==4 ||
        !r.resource || !r.gpu || (r.gpu&255) || r.bytes<material_binding::LogicalBytes ||
        r.gpu>UINT64_MAX-r.bytes || !r.descriptor || r.descriptor!=saved.slot.descriptor)return false;
    uint64_t registry{};
    if(!reader(user,game+0x3BCDCA8,&registry,8) || registry!=saved.registry)return false;
    const auto offset=0x5C0AF8+uint64_t(saved.handle-1)*0xB0;
    if(registry>UINT64_MAX-offset-0x50 || saved.recordAddress!=registry+offset)return false;
    std::array<uint8_t,0x50> bytes{};
    if(!reader(user,registry+offset,bytes.data(),bytes.size()) || (bytes[6]&15)!=6)return false;
    material_binding::Record current;
    std::memcpy(&current.bytes,bytes.data(),4);current.kind=bytes[6]>>4;
    std::memcpy(&current.gpu,bytes.data()+8,8);std::memcpy(&current.resource,bytes.data()+16,8);
    std::memcpy(&current.descriptor,bytes.data()+0x48,8);
    return current==r;
}
#endif
// Only the global registry and saved handle are read. Recording contexts and
// their CPU binding slots may already have advanced on the submission thread.
#if CVR_RUST_VRIK
inline bool SubmissionRecord(geometry::MemoryReader reader,void* user,uint64_t game,
                             const material_binding::Snapshot& saved){
    return cvr_mat_submission_record(reader,user,game,&saved,false);
}
#else
inline bool SubmissionRecord(geometry::MemoryReader reader,void* user,uint64_t game,
                             const material_binding::Snapshot& saved){
    return SubmissionRecordOnce(reader,user,game,saved) && SubmissionRecordOnce(reader,user,game,saved);
}
#endif
#if CVR_RUST_VRIK
inline bool SourceRange(uint64_t address,uint64_t gpu,uint64_t bytes,uint64_t offset){return cvr_mat_source_range(address,gpu,bytes,offset);}
#else
inline bool SourceRange(uint64_t address,uint64_t gpu,uint64_t bytes,uint64_t offset){
    return address && gpu && gpu<=UINT64_MAX-bytes && address>=gpu && address-gpu==offset &&
        offset<=bytes && material_binding::LogicalBytes<=bytes-offset;
}
#endif
#if CVR_RUST_VRIK
inline bool DestinationRange(uint64_t bytes,uint64_t offset){return cvr_mat_destination_range(bytes,offset);}
#else
inline bool DestinationRange(uint64_t bytes,uint64_t offset){
    return offset<=bytes && material_binding::LogicalBytes<=bytes-offset;
}
#endif
}
bool Compatible();
// Recording-time identity/descriptor validation only, on the native recording
// thread. It does not certify contents, subsequent writes, aliases or submission.
bool Validate(ID3D12GraphicsCommandList*,const material_binding::Lease&);
// Submission-time retained source, fresh native record and saved descriptor
// identity checks. No render-worker context, CPU slot lookup or raw reacquisition.
// The caller still owns copy scheduling, read-state proof and fence completion.
bool ValidateSubmission(const material_binding::Lease&,ID3D12CommandQueue* queue=nullptr);
// Owned DEFAULT snapshot destination must be in COPY_DEST. Source identity is
// global/retained; the current native context supplies this recording's proof.
// No historical descriptor slot or render-worker binding is reused.
bool CopySnapshot(ID3D12GraphicsCommandList*,const material_binding::Lease&,ID3D12Resource* destination,
                  uint64_t destinationOffset,buffer_read::Proof&);
bool ValidateSnapshotSubmission(const material_binding::Lease&,const buffer_read::Proof&,ID3D12CommandQueue*);
// Copies exactly 292 bytes into a caller-owned READBACK buffer (fixed COPY_DEST).
// The caller owns scheduling/fence completion and a pre-submit mutation predicate.
// DEFAULT source: verified tracked state, or a class-1 observed pool receipt
// stored in the lease. UPLOAD: verified GENERIC_READ. Class-1 submission needs
// the actual native DIRECT queue. Transient and GPU_UPLOAD remain unsupported.
bool Copy(ID3D12GraphicsCommandList*,material_binding::Lease&,ID3D12Resource* destination,uint64_t destinationOffset);
// 0 success; 1 arguments, 2 executable, 3 identity, 4 destination, 5 device,
// 6 retention, 7 changed before copy, 8 exception; 10+native status rejects a
// source before mutation. 40 means readability unverified after mutation (fatal
// to replay); 41 means native read recovery verified but copy rejected.
// 42 rejects an absent or changed class-1 creation/history/pool receipt.
uint32_t LastFailure();
}
