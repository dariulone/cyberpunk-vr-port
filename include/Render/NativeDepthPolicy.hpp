#pragma once
#include <d3d12.h>

#if CVR_RUST_VRIK
// The policy: rust/crates/cvr-render/src/depth_policy.rs.
extern "C" {
bool cvr_depth_policy_explicit_shader_resources(const void* code, size_t length);
bool cvr_depth_policy_shader_read_only(const void* code, size_t length);
bool cvr_depth_policy_shader_single_layer_read_only(const void* code, size_t length, bool vertex);
}
namespace cvr::stereo {
inline bool ShaderReadOnly(D3D12_SHADER_BYTECODE c){return cvr_depth_policy_shader_read_only(c.pShaderBytecode,c.BytecodeLength);}
inline bool ExplicitShaderResources(D3D12_SHADER_BYTECODE c){return cvr_depth_policy_explicit_shader_resources(c.pShaderBytecode,c.BytecodeLength);}
inline bool ShaderSingleLayerReadOnly(D3D12_SHADER_BYTECODE c,bool vertex){
    return cvr_depth_policy_shader_single_layer_read_only(c.pShaderBytecode,c.BytecodeLength,vertex);
}
}
#else
namespace cvr::stereo {
// Authorizes an additional depth-reference draw. Empty stages are valid;
// unsupported bytecode and any UAV binding fail closed.
bool ShaderReadOnly(D3D12_SHADER_BYTECODE);
bool ExplicitShaderResources(D3D12_SHADER_BYTECODE);
// Native VS/PS routing into one array layer. Rejects writes, ViewID and vertex
// outputs that select an array layer or viewport. Only the PS may be empty.
bool ShaderSingleLayerReadOnly(D3D12_SHADER_BYTECODE,bool vertex);
}
#endif
