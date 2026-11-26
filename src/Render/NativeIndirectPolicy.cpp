#if !CVR_RUST_VRIK
// The C++ build's policy; the Rust build's is rust/crates/cvr-render/src/indirect_policy.rs (the header calls it).
#include "Render/NativeIndirectPolicy.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/RenderProbeScope.hpp"

namespace cvr::stereo::indirect_policy {
namespace {
constexpr GUID receiptKey{0x6e80df9a,0x3c21,0x4dc7,{0x9d,0x81,0x35,0x4f,0x90,0xaa,0x17,0x61}};
}
void Created(ID3D12CommandSignature* signature,const D3D12_COMMAND_SIGNATURE_DESC& desc){
    if(!signature || probe::internalCommands || !native_probe::ObserveBindings())return;
    const detail::Receipt receipt{1,detail::Classify(desc),desc.ByteStride,desc.NodeMask};
    signature->SetPrivateData(receiptKey,sizeof(receipt),&receipt);
}
detail::Receipt Describe(ID3D12CommandSignature* signature){
    if(!signature || !native_probe::ObserveBindings())return {};
    detail::Receipt receipt{};UINT bytes=sizeof(receipt);
    if(FAILED(signature->GetPrivateData(receiptKey,&bytes,&receipt)) || bytes!=sizeof(receipt))return {};
    return receipt;
}
UINT DrawIndexedStride(ID3D12CommandSignature* signature){return detail::DrawIndexedStride(Describe(signature));}
UINT PureDrawStride(ID3D12CommandSignature* signature){return detail::PureDrawStride(Describe(signature));}
UINT DrawKind(ID3D12CommandSignature* signature){return detail::DrawKind(Describe(signature));}
bool DrawIndexedOnly(ID3D12CommandSignature* signature){return DrawIndexedStride(signature)!=0;}
}
#endif
