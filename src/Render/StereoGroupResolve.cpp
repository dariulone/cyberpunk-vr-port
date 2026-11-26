#if !CVR_RUST_VRIK
// The C++ build's resolve; the Rust build's is rust/crates/cvr-gpu/src/group_resolve.rs (the header calls it).
#include "Render/StereoGroupResolve.hpp"
#include "Render/CommandResources.hpp"
#include <d3dcompiler.h>
#include <cstring>
using Microsoft::WRL::ComPtr;
namespace cvr::stereo {
bool StereoGroupResolve::Supports(const D3D12_BLEND_DESC& blend,const D3D12_DEPTH_STENCIL_DESC& depth) {
    if(blend.AlphaToCoverageEnable || !depth.DepthEnable || depth.DepthWriteMask!=D3D12_DEPTH_WRITE_MASK_ALL ||
       depth.DepthFunc!=D3D12_COMPARISON_FUNC_GREATER_EQUAL || !depth.StencilEnable || depth.StencilWriteMask!=255)return false;
    for(UINT i=0;i<(blend.IndependentBlendEnable?3u:1u);++i) {
        const auto& b=blend.RenderTarget[i];if(b.BlendEnable || b.LogicOpEnable || b.RenderTargetWriteMask!=15)return false;
    }
    for(const auto& f:{depth.FrontFace,depth.BackFace})if(f.StencilFunc!=D3D12_COMPARISON_FUNC_ALWAYS ||
        f.StencilFailOp!=D3D12_STENCIL_OP_KEEP || f.StencilDepthFailOp!=D3D12_STENCIL_OP_KEEP || f.StencilPassOp!=D3D12_STENCIL_OP_REPLACE)return false;
    return true;
}
HRESULT StereoGroupResolve::Initialize(ID3D12Device* device,const std::array<ID3D12Resource*,4>& sources,bool depthOnly) {
    if(!device || pipeline_)return E_INVALIDARG;
    depthOnly_=depthOnly;
    const DXGI_FORMAT formats[]{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R32G8X24_TYPELESS};
    D3D12_RESOURCE_DESC first{};
    for(UINT i=depthOnly?3:0;i<4;++i) {
        if(!sources[i])return E_INVALIDARG;const auto d=sources[i]->GetDesc();if(i==(depthOnly?3u:0u))first=d;
        if(d.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || d.DepthOrArraySize!=2 || d.MipLevels!=1 ||
           d.SampleDesc.Count!=1 || d.Width!=first.Width || d.Height!=first.Height || d.Format!=formats[i] ||
           (d.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE))return E_INVALIDARG;
    }
    static constexpr char code[]=R"(
Texture2DArray<float4> C0:register(t0), C1:register(t1), C2:register(t2);
Texture2DArray<float> D:register(t3);
Texture2DArray<uint2> S:register(t4);
cbuffer Params:register(b0) { uint Eye; };
float4 VS(uint id:SV_VertexID):SV_Position {
    return float4(id==2?3:-1,id==1?3:-1,0,1);
}
struct Out {float4 c0:SV_Target0;float4 c1:SV_Target1;float4 c2:SV_Target2;float depth:SV_Depth;};
Out PS(float4 p:SV_Position) {
    int4 q=int4(int2(p.xy),Eye,0);
    // 255 is reserved as untouched coverage; native reference 255 is rejected.
    if(S.Load(q).y==255)discard;
    Out o;o.c0=C0.Load(q);o.c1=C1.Load(q);o.c2=C2.Load(q);o.depth=D.Load(q);return o;
}
float DepthPS(float4 p:SV_Position):SV_Depth {
    int4 q=int4(int2(p.xy),Eye,0);if(S.Load(q).y==255)discard;return D.Load(q);
})";
    ComPtr<ID3DBlob> vs,ps,error;auto hr=D3DCompile(code,std::strlen(code),nullptr,nullptr,nullptr,"VS","vs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&vs,&error);if(FAILED(hr))return hr;
    hr=D3DCompile(code,std::strlen(code),nullptr,nullptr,nullptr,depthOnly?"DepthPS":"PS","ps_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&ps,&error);if(FAILED(hr))return hr;
    D3D12_DESCRIPTOR_RANGE range{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,5,0,0,0};D3D12_ROOT_PARAMETER params[2]{};
    params[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[0].DescriptorTable={1,&range};params[0].ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
    params[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;params[1].Constants={0,0,1};params[1].ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
    const D3D12_ROOT_SIGNATURE_DESC rd{2,params,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};ComPtr<ID3DBlob> blob;
    hr=D3D12SerializeRootSignature(&rd,D3D_ROOT_SIGNATURE_VERSION_1,&blob,&error);if(FAILED(hr))return hr;
    hr=device->CreateRootSignature(0,blob->GetBufferPointer(),blob->GetBufferSize(),IID_PPV_ARGS(&root_));if(FAILED(hr))return hr;
    D3D12_GRAPHICS_PIPELINE_STATE_DESC pd{};pd.pRootSignature=root_.Get();pd.VS={vs->GetBufferPointer(),vs->GetBufferSize()};pd.PS={ps->GetBufferPointer(),ps->GetBufferSize()};
    pd.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;pd.NumRenderTargets=depthOnly?0:3;for(UINT i=0;i<pd.NumRenderTargets;++i)pd.RTVFormats[i]=formats[i];
    pd.DSVFormat=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;pd.SampleDesc.Count=1;pd.SampleMask=UINT_MAX;
    pd.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;pd.RasterizerState.CullMode=D3D12_CULL_MODE_NONE;pd.RasterizerState.DepthClipEnable=TRUE;
    for(auto& b:pd.BlendState.RenderTarget){b.SrcBlend=b.SrcBlendAlpha=D3D12_BLEND_ONE;b.DestBlend=b.DestBlendAlpha=D3D12_BLEND_ZERO;
        b.BlendOp=b.BlendOpAlpha=D3D12_BLEND_OP_ADD;b.LogicOp=D3D12_LOGIC_OP_NOOP;b.RenderTargetWriteMask=15;}
    pd.DepthStencilState.DepthEnable=TRUE;pd.DepthStencilState.DepthWriteMask=D3D12_DEPTH_WRITE_MASK_ALL;pd.DepthStencilState.DepthFunc=D3D12_COMPARISON_FUNC_GREATER_EQUAL;
    pd.DepthStencilState.StencilEnable=TRUE;pd.DepthStencilState.StencilReadMask=pd.DepthStencilState.StencilWriteMask=255;
    pd.DepthStencilState.FrontFace=pd.DepthStencilState.BackFace={D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_REPLACE,D3D12_COMPARISON_FUNC_ALWAYS};
    hr=device->CreateGraphicsPipelineState(&pd,IID_PPV_ARGS(&pipeline_));if(FAILED(hr))return hr;
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,5,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
    hr=device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&heap_));if(FAILED(hr))return hr;
    auto handle=heap_->GetCPUDescriptorHandleForHeapStart();const auto stride=device->GetDescriptorHandleIncrementSize(hd.Type);
    for(UINT i=0;i<5;++i) {
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=i<3?formats[i]:i==3?DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS:DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
        view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2DARRAY;view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        view.Texture2DArray.MipLevels=1;view.Texture2DArray.ArraySize=2;view.Texture2DArray.PlaneSlice=i==4?1:0;
        device->CreateShaderResourceView(sources[i<4?i:3],&view,handle);handle.ptr+=stride;
    }
    sources_=sources;return S_OK;
}
bool StereoGroupResolve::Retain(ID3D12GraphicsCommandList* list) const {
    return pipeline_ && cvr::gpu::KeepCommandResources(list,{root_.Get(),pipeline_.Get(),heap_.Get(),sources_[3]}) &&
        (depthOnly_ || cvr::gpu::KeepCommandResources(list,{sources_[0],sources_[1],sources_[2]}));
}
void StereoGroupResolve::Record(ID3D12GraphicsCommandList* list,UINT eye,UINT stencil,
    const D3D12_VIEWPORT& vp,const D3D12_RECT& scissor,const std::array<D3D12_CPU_DESCRIPTOR_HANDLE,3>& targets,D3D12_CPU_DESCRIPTOR_HANDLE depth) const {
    D3D12_RESOURCE_BARRIER barriers[4]{};
    const UINT first=depthOnly_?3:0;
    for(UINT i=first;i<4;++i){auto& b=barriers[i];b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={sources_[i],D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,i==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE};}
    list->ResourceBarrier(4-first,barriers+first);list->SetPipelineState(pipeline_.Get());list->SetGraphicsRootSignature(root_.Get());auto* heap=heap_.Get();list->SetDescriptorHeaps(1,&heap);
    list->SetGraphicsRootDescriptorTable(0,heap_->GetGPUDescriptorHandleForHeapStart());list->SetGraphicsRoot32BitConstant(1,eye,0);
    list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);list->RSSetViewports(1,&vp);list->RSSetScissorRects(1,&scissor);
    list->OMSetRenderTargets(depthOnly_?0:3,depthOnly_?nullptr:targets.data(),FALSE,&depth);list->OMSetStencilRef(stencil);list->DrawInstanced(3,1,0,0);
    for(UINT i=first;i<4;++i)std::swap(barriers[i].Transition.StateBefore,barriers[i].Transition.StateAfter);list->ResourceBarrier(4-first,barriers+first);
}
}
#endif
