#pragma once
#include <d3d12.h>
#include <cstring>
#include <cstdint>

namespace cvr::stereo {
// Batch splitting changes the draw-local instance ordinal. Require an explicit
// parsed input signature without SV_InstanceID, in addition to rebase support.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/instance_rebase.rs (tools/single_pass_tests/geometry_commands_test and
// instance_coverage_test run their cases against them in their Rust variants).
extern "C" {
bool cvr_ir_independent_ordinal(const D3D12_SHADER_BYTECODE*);
bool cvr_ir_supports_rebase(const D3D12_SHADER_BYTECODE*,const D3D12_INPUT_LAYOUT_DESC*);
}
inline bool HasIndependentInstanceOrdinal(D3D12_SHADER_BYTECODE shader){return cvr_ir_independent_ordinal(&shader);}
inline bool SupportsInstanceRebase(D3D12_SHADER_BYTECODE shader,const D3D12_INPUT_LAYOUT_DESC& layout) {
    return cvr_ir_supports_rebase(&shader,&layout);
}
#else
inline bool HasIndependentInstanceOrdinal(D3D12_SHADER_BYTECODE shader){
    if(!shader.pShaderBytecode || shader.BytecodeLength<32)return false;
    const auto* data=static_cast<const uint8_t*>(shader.pShaderBytecode);
    const auto u32=[&](size_t at){uint32_t v{};std::memcpy(&v,data+at,4);return v;};
    if(std::memcmp(data,"DXBC",4) || u32(24)!=shader.BytecodeLength)return false;
    const auto bytes=u32(24),chunks=u32(28);if(chunks>64 || chunks>(bytes-32)/4)return false;
    bool signature=false;
    for(uint32_t i=0;i<chunks;++i){
        const auto at=u32(32+i*4);if(at<32+chunks*4 || at>bytes-8)return false;
        const auto length=u32(at+4);if(length>bytes-at-8)return false;
        const bool modern=std::memcmp(data+at,"ISG1",4)==0,legacy=std::memcmp(data+at,"ISGN",4)==0;
        if(!modern && !legacy)continue;
        if(signature || length<8)return false;signature=true;
        const auto count=u32(at+8),stride=modern?32u:24u;
        if(count>32 || count>(length-8)/stride)return false;
        for(uint32_t j=0;j<count;++j){
            const auto entry=at+16+j*stride+(modern?4:0);
            const auto name=u32(entry);if(name>=length || !std::memchr(data+at+8+name,0,length-name))return false;
            if(u32(entry+8)==D3D_NAME_INSTANCE_ID)return false;
        }
    }
    return signature;
}
// SV_InstanceID excludes StartInstanceLocation. The newer SM6.8 base-instance
// semantic is deliberately excluded before compacting native IA streams.
inline bool SupportsInstanceRebase(D3D12_SHADER_BYTECODE shader,const D3D12_INPUT_LAYOUT_DESC& layout) {
    if(!shader.pShaderBytecode || shader.BytecodeLength<32 || layout.NumElements>32 || !layout.pInputElementDescs)return false;
    bool instance=false;
    for(UINT i=0;i<layout.NumElements;++i) {
        const auto& e=layout.pInputElementDescs[i];
        if(e.InputSlotClass==D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA) {
            if(e.InputSlot!=7 || e.InstanceDataStepRate!=1)return false;instance=true;
        }else if(e.InputSlot==7)return false;
    }
    if(!instance)return false;
    const auto* data=static_cast<const uint8_t*>(shader.pShaderBytecode);
    if(std::memcmp(data,"DXBC",4))return false;
    const auto u32=[&](size_t at){uint32_t value{};std::memcpy(&value,data+at,4);return value;};
    const auto bytes=u32(24),count=u32(28);
    if(bytes!=shader.BytecodeLength || count>(bytes-32)/4 || count>64)return false;
    bool vertex=false;
    for(UINT i=0;i<count;++i) {
        const auto at=u32(32+4*i);if(at<32+4*count || at>bytes-8)return false;
        const auto size=u32(at+4);if(size>bytes-at-8)return false;
        if(!std::memcmp(data+at,"SFI0",4)) {
            uint64_t features{};if(size!=8)return false;std::memcpy(&features,data+at+8,8);
            if(features&(uint64_t(1)<<32))return false;
        }
        if(!std::memcmp(data+at,"DXIL",4)) {
            if(vertex || size<4)return false;const auto version=u32(at+8);
            if((version>>16)!=1 || (version&0xFFFF)<0x60 || (version&0xFFFF)>0x67)return false;vertex=true;
        }
    }
    return vertex;
}
#endif
}
