#pragma once
#include <d3d12.h>

namespace cvr::markers {
#if CVR_RUST_VRIK
}
// The recognition: rust/crates/cvr-render/src/world_marker_shader.rs.
extern "C" {
void cvr_world_marker_vertex_shader_code(bool procedural, const void** code, size_t* size);
bool cvr_world_marker_vertex_shader_replacement(const void* code, size_t size, const void** out, size_t* outSize);
void cvr_world_marker_vertex_shader_result(const void* code, size_t size, bool success);
bool cvr_world_marker_vertex_shaders_ready();
}
namespace cvr::markers {
inline bool VertexShaderReplacement(const D3D12_SHADER_BYTECODE& original, D3D12_SHADER_BYTECODE& replacement) {
    return cvr_world_marker_vertex_shader_replacement(original.pShaderBytecode, original.BytecodeLength,
                                                      &replacement.pShaderBytecode, &replacement.BytecodeLength);
}
inline void VertexShaderResult(const D3D12_SHADER_BYTECODE& original, bool success) {
    cvr_world_marker_vertex_shader_result(original.pShaderBytecode, original.BytecodeLength, success);
}
inline bool VertexShadersReady() { return cvr_world_marker_vertex_shaders_ready(); }
inline D3D12_SHADER_BYTECODE VertexShaderCode(bool procedural) {
    D3D12_SHADER_BYTECODE code{};
    cvr_world_marker_vertex_shader_code(procedural, &code.pShaderBytecode, &code.BytecodeLength);
    return code;
}
#else
// The two captured flat ink VS variants, with their existing bindings/signatures.
bool VertexShaderReplacement(const D3D12_SHADER_BYTECODE& original, D3D12_SHADER_BYTECODE& replacement);
void VertexShaderResult(const D3D12_SHADER_BYTECODE& original, bool success);
bool VertexShadersReady();
D3D12_SHADER_BYTECODE VertexShaderCode(bool procedural);
#endif
}
