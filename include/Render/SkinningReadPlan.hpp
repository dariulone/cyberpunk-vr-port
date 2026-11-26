#pragma once
#include <d3d12.h>
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <span>
#include <string_view>
#include <vector>

namespace cvr::stereo::skinning {
inline constexpr std::string_view DepthShader="906dcb2a6f034360c3b1d00067dbf12b9f6ee79a9789e73fae258688dee00d0f";
inline constexpr std::string_view AlphaShader="02f81c75e38964877d832f7378a39251132ab93580dfe89017e65d01fc22bc4c";
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/skinning_read_plan.rs (tools/single_pass_tests/skinning_read_plan_test runs its
// cases against them in its Rust variant).
struct BoneRange;struct RustBoneRanges;
extern "C" {
uint32_t cvr_sk_depth_contract(const char*,size_t,const D3D12_INPUT_LAYOUT_DESC*);
bool cvr_sk_constants(uint32_t,const uint8_t*,const uint8_t*);
RustBoneRanges* cvr_sk_bone_ranges(uint32_t,const uint8_t*,size_t,const D3D12_SHADER_RESOURCE_VIEW_DESC*,uint64_t);
const BoneRange* cvr_sk_ranges(const RustBoneRanges*,size_t*);
void cvr_sk_ranges_free(RustBoneRanges*);
}
#endif
// Contracts 1 (depth) and 2 (alpha depth) identify exact native VS/input pairs.
// Contract 2 also consumes UV/normal/tangent inputs; its PS is checked separately.
#if CVR_RUST_VRIK
inline uint32_t DepthContract(std::string_view shader,const D3D12_INPUT_LAYOUT_DESC& layout) {
    return cvr_sk_depth_contract(shader.data(),shader.size(),&layout);
}
#else
inline uint32_t DepthContract(std::string_view shader,const D3D12_INPUT_LAYOUT_DESC& layout) {
    const uint32_t contract=shader==DepthShader?1u:shader==AlphaShader?2u:0u;
    if(!contract || !layout.pInputElementDescs || !layout.NumElements || layout.NumElements>32)return 0;
    struct Input {std::string_view name;UINT index,slot,offset;DXGI_FORMAT format;bool instance,alphaOnly=false;};
    constexpr Input inputs[]{
        {"POSITION",0,0,0,DXGI_FORMAT_R16G16B16A16_SNORM,false},
        {"BLENDINDICES",0,0,8,DXGI_FORMAT_R8G8B8A8_UINT,false},
        {"BLENDINDICES",1,0,12,DXGI_FORMAT_R8G8B8A8_UINT,false},
        {"BLENDWEIGHT",0,0,16,DXGI_FORMAT_R8G8B8A8_UNORM,false},
        {"BLENDWEIGHT",1,0,20,DXGI_FORMAT_R8G8B8A8_UNORM,false},
        {"EXTRA_DATA",0,0,24,DXGI_FORMAT_R16G16B16A16_FLOAT,false},
        {"INSTANCE_TRANSFORM",0,7,0,DXGI_FORMAT_R32G32B32A32_FLOAT,true},
        {"INSTANCE_TRANSFORM",1,7,16,DXGI_FORMAT_R32G32B32A32_FLOAT,true},
        {"INSTANCE_TRANSFORM",2,7,32,DXGI_FORMAT_R32G32B32A32_FLOAT,true},
        {"INSTANCE_SKINNING_DATA",0,7,48,DXGI_FORMAT_R32G32B32A32_UINT,true},
        {"TEXCOORD",0,1,0,DXGI_FORMAT_R16G16_FLOAT,false,true},
        {"NORMAL",0,2,0,DXGI_FORMAT_R10G10B10A2_UNORM,false,true},
        {"TANGENT",0,2,4,DXGI_FORMAT_R10G10B10A2_UNORM,false,true},
    };
    for(const auto& expected:inputs){
        if(expected.alphaOnly && contract!=2)continue;
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
    return contract;
}
#endif
// Constant words read by both contracts. A zero cull count bypasses b8, but the
// modifier at byte 24 still selects pre/post-cull placement.
#if CVR_RUST_VRIK
inline bool ZeroCull(const std::array<uint8_t,448>& modifiers){return cvr_sk_constants(0,modifiers.data(),nullptr);}
inline bool SameMaterial(const std::array<uint8_t,112>& first,const std::array<uint8_t,112>& second){
    return cvr_sk_constants(1,first.data(),second.data());
}
inline bool SameModifiers(const std::array<uint8_t,448>& first,const std::array<uint8_t,448>& second){
    return cvr_sk_constants(2,first.data(),second.data());
}
#else
inline bool ZeroCull(const std::array<uint8_t,448>& modifiers){
    uint32_t count{};std::memcpy(&count,modifiers.data()+16,4);return (count&0x7fffffff)==0;
}
inline bool SameMaterial(const std::array<uint8_t,112>& first,const std::array<uint8_t,112>& second){
    for(const auto offset:{64u,68u,72u,80u,84u,88u})if(std::memcmp(first.data()+offset,second.data()+offset,4))return false;
    return true;
}
inline bool SameModifiers(const std::array<uint8_t,448>& first,const std::array<uint8_t,448>& second){
    if(!ZeroCull(first) || !ZeroCull(second))return false;
    for(const auto offset:{16u,24u,32u})if(std::memcmp(first.data()+offset,second.data()+offset,4))return false;
    return true;
}
#endif
struct BoneRange {uint64_t offset{},bytes{};bool operator==(const BoneRange&)const=default;};
// Covers all addresses base + index*stride + {0,16,32} used by both contracts.
// The caller supplies complete 64-byte records from the actual instance binding.
// Equality of these ranges is still a separate CPU/GPU content proof.
#if CVR_RUST_VRIK
inline bool BoneRanges(uint32_t contract,std::span<const uint8_t> instances,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                       uint64_t resourceBytes,std::vector<BoneRange>& output) {
    static_assert(sizeof(BoneRange)==16,"mirrored by skinning_read_plan.rs");
    output.clear();
    const auto ranges=cvr_sk_bone_ranges(contract,instances.data(),instances.size(),&view,resourceBytes);
    if(!ranges)return false;
    struct Release{RustBoneRanges* ranges;~Release(){cvr_sk_ranges_free(ranges);}}release{ranges};
    size_t count{};const auto data=cvr_sk_ranges(ranges,&count);output.assign(data,data+count);return true;
}
#else
inline bool BoneRanges(uint32_t contract,std::span<const uint8_t> instances,const D3D12_SHADER_RESOURCE_VIEW_DESC& view,
                       uint64_t resourceBytes,std::vector<BoneRange>& output) {
    output.clear();
    if((contract!=1 && contract!=2) || instances.empty() || instances.size()%64 || instances.size()>64*65536 ||
        view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER || view.Format!=DXGI_FORMAT_R32_TYPELESS ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || view.Buffer.Flags!=D3D12_BUFFER_SRV_FLAG_RAW ||
        view.Buffer.StructureByteStride || !view.Buffer.NumElements || view.Buffer.FirstElement>UINT64_MAX/4 || view.Buffer.FirstElement%4)return false;
    const auto begin=view.Buffer.FirstElement*4,bytes=uint64_t(view.Buffer.NumElements)*4;
    if(begin>resourceBytes || bytes>resourceBytes-begin)return false;
    std::vector<BoneRange> ranges;ranges.reserve(instances.size()/64);
    for(size_t at=0;at<instances.size();at+=64){
        uint32_t base{},stride{};std::memcpy(&base,instances.data()+at+48,4);std::memcpy(&stride,instances.data()+at+52,4);
        const uint64_t size=uint64_t(stride)*255+48;
        if(base%4 || stride%4 || uint64_t(base)+size>uint64_t(UINT32_MAX)+1 || base>bytes || size>bytes-base)return false;
        ranges.push_back({begin+base,size});
    }
    std::sort(ranges.begin(),ranges.end(),[](const auto& a,const auto& b){return a.offset<b.offset;});
    for(const auto range:ranges){
        if(!output.empty() && range.offset<=output.back().offset+output.back().bytes){
            auto& last=output.back();last.bytes=std::max(last.bytes,range.offset+range.bytes-last.offset);
        }else output.push_back(range);
    }
    return true;
}
#endif
}
