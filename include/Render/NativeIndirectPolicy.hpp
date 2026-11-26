#pragma once
#include <d3d12.h>
#include <cstdint>

#if CVR_RUST_VRIK
namespace cvr::stereo::indirect_policy::detail {
struct Receipt {uint32_t version{},kind{},stride{},nodeMask{};};
static_assert(sizeof(Receipt)==16);
}
// The policy: rust/crates/cvr-render/src/indirect_policy.rs.
extern "C" {
bool cvr_indirect_valid_stride(UINT stride,UINT kind);
uint32_t cvr_indirect_classify(const D3D12_COMMAND_SIGNATURE_DESC*);
UINT cvr_indirect_receipt_kind(const cvr::stereo::indirect_policy::detail::Receipt*);
UINT cvr_indirect_receipt_pure_stride(const cvr::stereo::indirect_policy::detail::Receipt*);
UINT cvr_indirect_receipt_indexed_stride(const cvr::stereo::indirect_policy::detail::Receipt*);
void cvr_indirect_created(ID3D12CommandSignature*,const D3D12_COMMAND_SIGNATURE_DESC*);
void cvr_indirect_describe(ID3D12CommandSignature*,cvr::stereo::indirect_policy::detail::Receipt*);
}
namespace cvr::stereo::indirect_policy {
namespace detail {
inline bool ValidStride(UINT stride,UINT kind=1){return cvr_indirect_valid_stride(stride,kind);}
inline uint32_t Classify(const D3D12_COMMAND_SIGNATURE_DESC& desc){return cvr_indirect_classify(&desc);}
inline UINT DrawKind(const Receipt& receipt){return cvr_indirect_receipt_kind(&receipt);}
inline UINT PureDrawStride(const Receipt& receipt){return cvr_indirect_receipt_pure_stride(&receipt);}
inline UINT DrawIndexedStride(const Receipt& receipt){return cvr_indirect_receipt_indexed_stride(&receipt);}
}
inline void Created(ID3D12CommandSignature* signature,const D3D12_COMMAND_SIGNATURE_DESC& desc){cvr_indirect_created(signature,&desc);}
inline detail::Receipt Describe(ID3D12CommandSignature* signature){detail::Receipt receipt{};cvr_indirect_describe(signature,&receipt);return receipt;}
inline UINT DrawIndexedStride(ID3D12CommandSignature* signature){return detail::DrawIndexedStride(Describe(signature));}
inline UINT PureDrawStride(ID3D12CommandSignature* signature){return detail::PureDrawStride(Describe(signature));}
inline UINT DrawKind(ID3D12CommandSignature* signature){return detail::DrawKind(Describe(signature));}
inline bool DrawIndexedOnly(ID3D12CommandSignature* signature){return DrawIndexedStride(signature)!=0;}
}
#else
namespace cvr::stereo::indirect_policy {
namespace detail {
inline bool ValidStride(UINT stride,UINT kind=1){
    static_assert(sizeof(D3D12_DRAW_INDEXED_ARGUMENTS)==20);
    static_assert(sizeof(D3D12_DRAW_ARGUMENTS)==16);
    const UINT minimum=kind==1?sizeof(D3D12_DRAW_INDEXED_ARGUMENTS):kind==2?sizeof(D3D12_DRAW_ARGUMENTS):0;
    return minimum && stride>=minimum && stride<=1024 && !(stride%sizeof(UINT));
}
inline uint32_t Classify(const D3D12_COMMAND_SIGNATURE_DESC& desc){
    if(desc.NumArgumentDescs!=1 || !desc.pArgumentDescs || desc.NodeMask>1)return 0;
    const auto type=desc.pArgumentDescs[0].Type;
    const UINT kind=type==D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED?1u:type==D3D12_INDIRECT_ARGUMENT_TYPE_DRAW?2u:0u;
    return ValidStride(desc.ByteStride,kind)?kind:0;
}
struct Receipt {uint32_t version{},kind{},stride{},nodeMask{};};
static_assert(sizeof(Receipt)==16);
inline UINT DrawKind(const Receipt& receipt){
    return receipt.version==1 && receipt.nodeMask<=1 && ValidStride(receipt.stride,receipt.kind)?receipt.kind:0;
}
inline UINT PureDrawStride(const Receipt& receipt){return DrawKind(receipt)?receipt.stride:0;}
inline UINT DrawIndexedStride(const Receipt& receipt){
    return receipt.kind==1?PureDrawStride(receipt):0;
}
}
// Call only after successful native command-signature creation. The signature's
// private data owns this immutable classification; no raw-pointer registry exists.
void Created(ID3D12CommandSignature*,const D3D12_COMMAND_SIGNATURE_DESC&);
bool DrawIndexedOnly(ID3D12CommandSignature*);
// Zero means unknown or rejected. Runtime argument-buffer bounds must use this
// actual stride, which may include padding beyond the 20-byte indexed arguments.
UINT DrawIndexedStride(ID3D12CommandSignature*);
// Both pure draw kinds preserve root/IA state. Kind 1 is DRAW_INDEXED; kind 2 is
// DRAW. All other signatures, unobserved signatures, and malformed receipts are 0.
UINT PureDrawStride(ID3D12CommandSignature*);
UINT DrawKind(ID3D12CommandSignature*);
// Diagnostic metadata, including the original stride for rejected signatures.
// This raw receipt is not authorization; use the validated accessors above.
detail::Receipt Describe(ID3D12CommandSignature*);
}
#endif
