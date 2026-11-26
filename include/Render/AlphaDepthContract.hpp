#pragma once
#include "Render/SkinningReadPlan.hpp"
#include <d3d12.h>
#include <cstdint>
#include <string_view>

namespace cvr::stereo::alpha_depth {
inline constexpr std::string_view VertexShader="7fa0e076ff234929afda3679349d06353d4936245785b547b339432e3b1fd9ae";
inline constexpr std::string_view PixelShader="dc69ea3ee2d8d906ebcbba4bd8f8d750add2b80946b5de9c9d94360cb18339e5";
inline constexpr std::string_view SkinnedVertexShader=skinning::AlphaShader;
inline constexpr uint32_t InstanceStride(uint32_t contract){return contract==1?48u:contract==2?64u:0u;}
// Contract 1 identifies the exact native shader pair and consumed IA inputs.
// Unused native NORMAL/TANGENT/COLOR/TEXCOORD1 attributes are not read proofs.
// Contract 2 composes the skinned VS contract with this same exact alpha PS.
// This does not admit rendering or certify resource contents, stride or bounds.
#if CVR_RUST_VRIK
// The rule: rust/crates/cvr-render/src/skinning_read_plan.rs (tools/single_pass_tests/alpha_depth_contract_test runs its
// cases against it in its Rust variant).
extern "C" uint32_t cvr_sk_alpha_contract(const char*,size_t,const char*,size_t,const D3D12_INPUT_LAYOUT_DESC*);
inline uint32_t Contract(std::string_view vertex,std::string_view pixel,const D3D12_INPUT_LAYOUT_DESC& layout){
    return cvr_sk_alpha_contract(vertex.data(),vertex.size(),pixel.data(),pixel.size(),&layout);
}
#else
inline uint32_t Contract(std::string_view vertex,std::string_view pixel,const D3D12_INPUT_LAYOUT_DESC& layout){
    if(pixel!=PixelShader)return 0;
    if(vertex==SkinnedVertexShader)return skinning::DepthContract(vertex,layout)==2?2u:0u;
    if(vertex!=VertexShader || !layout.pInputElementDescs ||
        !layout.NumElements || layout.NumElements>32)return 0;
    struct Input {std::string_view name;UINT index,slot,offset;DXGI_FORMAT format;bool instance;};
    constexpr Input inputs[]{
        {"POSITION",0,0,0,DXGI_FORMAT_R16G16B16A16_SNORM,false},
        {"TEXCOORD",0,1,0,DXGI_FORMAT_R16G16_FLOAT,false},
        {"INSTANCE_TRANSFORM",0,7,0,DXGI_FORMAT_R32G32B32A32_FLOAT,true},
        {"INSTANCE_TRANSFORM",1,7,16,DXGI_FORMAT_R32G32B32A32_FLOAT,true},
        {"INSTANCE_TRANSFORM",2,7,32,DXGI_FORMAT_R32G32B32A32_FLOAT,true},
    };
    for(const auto& expected:inputs){
        UINT found{};
        for(UINT i=0;i<layout.NumElements;++i){const auto& actual=layout.pInputElementDescs[i];
            if(!actual.SemanticName)return 0;
            if(expected.name!=actual.SemanticName || expected.index!=actual.SemanticIndex)continue;
            if(++found!=1 || actual.InputSlot!=expected.slot || actual.AlignedByteOffset!=expected.offset || actual.Format!=expected.format ||
                actual.InputSlotClass!=(expected.instance?D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA:D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA) ||
                actual.InstanceDataStepRate!=UINT(expected.instance))return 0;
        }
        if(found!=1)return 0;
    }
    return 1;
}
#endif
}
