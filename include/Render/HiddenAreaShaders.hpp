#pragma once
// Hidden-area shader patches (docs/hidden-area-mask-20261009.md).
//
// Patched shaders live in `hidden_area\<FNV-1a of the original>.dxil` beside the plugin, built by
// tools/hidden_area_tests/ham_shaders.py. Each carries four placeholder constants that describe the
// visible ellipse; a pipeline created with such a shader gets twins built from the same description
// with the placeholders set for the current mask, and while the mask is on the twin is bound instead.
//
// Development census: with `cvr_shader_census.txt` next to the game executable at startup, every
// compute, pixel and vertex shader the engine creates a pipeline with is kept, so a census can name and
// dump the ones each render-graph node runs. CyberpunkVR_ShaderCensus 1 = full-screen draws and
// dispatches (their compute or pixel shader), 2 = instanced draws, indexed or not (their vertex shader).
#include <d3d12.h>
#include <cstdint>
#include <functional>

namespace cvr::hidden_area::shaders {
enum class Kind : uint32_t { Dispatch=1, ExecuteIndirect=2, Draw=3, DrawIndexed=4, DispatchRays=5 };
// Builds a pipeline from the engine's description with `code` in place of the patched stage.
using Creator=std::function<ID3D12PipelineState*(const D3D12_SHADER_BYTECODE& code)>;
#if CVR_RUST_VRIK
// The twins and the census: rust/crates/cvr-render/src/shader_patches.rs (the comments below, in the C++ build's
// declarations, describe each call).
extern "C" {
bool cvr_shader_patches_has_patch(const void* code,size_t size);
bool cvr_shader_patches_library_twin(const void* code,size_t size,const void** out,size_t* outSize);
void cvr_shader_patches_library_twin_created(bool ok,uint32_t libraries);
bool cvr_shader_patches_bound_code(const void* code,size_t size,const void** out,size_t* outSize);
bool cvr_shader_patches_fixed_vertex_twin(const void* code,size_t size,const void** out,size_t* outSize);
void cvr_shader_patches_register(ID3D12PipelineState* original,const void* code,size_t size,void* creator,
                                 ID3D12PipelineState* (*call)(void* creator,const uint8_t* code,size_t size),
                                 void (*free)(void* creator));
void cvr_shader_patches_publish(int side,const float* scales);
ID3D12PipelineState* cvr_shader_patches_bind(ID3D12PipelineState* pso,int side);
void cvr_shader_patches_pipeline_created(ID3D12PipelineState* pso,const void* cs,size_t csSize,const void* ps,size_t psSize,
                                         const void* vs,size_t vsSize);
void cvr_shader_patches_command(uint32_t node,int side,ID3D12PipelineState* pso,uint32_t kind,uint32_t x,uint32_t y,uint32_t z);
}
inline void PipelineCreated(ID3D12PipelineState* pso,const D3D12_SHADER_BYTECODE& cs,const D3D12_SHADER_BYTECODE& ps,
                            const D3D12_SHADER_BYTECODE& vs={}) {
    cvr_shader_patches_pipeline_created(pso,cs.pShaderBytecode,cs.BytecodeLength,ps.pShaderBytecode,ps.BytecodeLength,
                                        vs.pShaderBytecode,vs.BytecodeLength);
}
inline void Command(uint32_t node,int side,ID3D12PipelineState* pso,Kind kind,uint32_t x,uint32_t y,uint32_t z) {
    cvr_shader_patches_command(node,side,pso,uint32_t(kind),x,y,z);
}
inline bool HasPatch(const D3D12_SHADER_BYTECODE& code){return cvr_shader_patches_has_patch(code.pShaderBytecode,code.BytecodeLength);}
inline D3D12_SHADER_BYTECODE FixedVertexTwin(const D3D12_SHADER_BYTECODE& code) {
    D3D12_SHADER_BYTECODE out{};
    return cvr_shader_patches_fixed_vertex_twin(code.pShaderBytecode,code.BytecodeLength,&out.pShaderBytecode,&out.BytecodeLength)
               ? out : D3D12_SHADER_BYTECODE{};
}
inline D3D12_SHADER_BYTECODE BoundCode(const D3D12_SHADER_BYTECODE& code) {
    D3D12_SHADER_BYTECODE out{};
    return cvr_shader_patches_bound_code(code.pShaderBytecode,code.BytecodeLength,&out.pShaderBytecode,&out.BytecodeLength) ? out : code;
}
inline D3D12_SHADER_BYTECODE LibraryTwin(const D3D12_SHADER_BYTECODE& code) {
    D3D12_SHADER_BYTECODE out{};
    return cvr_shader_patches_library_twin(code.pShaderBytecode,code.BytecodeLength,&out.pShaderBytecode,&out.BytecodeLength)
               ? out : D3D12_SHADER_BYTECODE{};
}
inline void LibraryTwinCreated(bool ok,uint32_t libraries){cvr_shader_patches_library_twin_created(ok,libraries);}
// Rust owns the creator from here on (it frees it through the second callback, also when nothing is registered).
inline void Register(ID3D12PipelineState* original,const D3D12_SHADER_BYTECODE& code,Creator create) {
    if(!create)return;
    cvr_shader_patches_register(original,code.pShaderBytecode,code.BytecodeLength,new Creator(std::move(create)),
        [](void* c,const uint8_t* bytes,size_t size)->ID3D12PipelineState*{return (*static_cast<Creator*>(c))({bytes,size});},
        [](void* c){delete static_cast<Creator*>(c);});
}
inline void Publish(int side,const float scales[4]){cvr_shader_patches_publish(side,scales);}
inline ID3D12PipelineState* Bind(ID3D12PipelineState* pso,int side){return cvr_shader_patches_bind(pso,side);}
#else// After the engine's pipeline was created; any bytecode may be empty.
void PipelineCreated(ID3D12PipelineState*,const D3D12_SHADER_BYTECODE& cs,const D3D12_SHADER_BYTECODE& ps,
                     const D3D12_SHADER_BYTECODE& vs={});
// Every dispatch, indirect execution and draw: x/y/z are the group counts, the maximum command count,
// or vertex (index) / instance counts; for rays the width, height and depth, and no shader.
void Command(uint32_t node,int side,ID3D12PipelineState*,Kind,uint32_t x,uint32_t y,uint32_t z);

bool HasPatch(const D3D12_SHADER_BYTECODE&);
// The fixed twin of a vertex shader whose group is on, or empty: a pipeline twinned for its pixel shader keeps
// its vertex twin (particle facing) through this.
D3D12_SHADER_BYTECODE FixedVertexTwin(const D3D12_SHADER_BYTECODE&);
// Under engine stereo: the bytecode Bind() puts in place of `code` while the twins' groups stay as they are now
// (its alternate, fixed twin or fallback), or `code` itself. Single-pass twins are built from it.
D3D12_SHADER_BYTECODE BoundCode(const D3D12_SHADER_BYTECODE& code);
// The stereo_rt\ twin of a state object's DXIL library while that group is on, or empty; and the outcome of
// creating a state object with `libraries` of them.
D3D12_SHADER_BYTECODE LibraryTwin(const D3D12_SHADER_BYTECODE&);
void LibraryTwinCreated(bool ok,uint32_t libraries);
void Register(ID3D12PipelineState* original,const D3D12_SHADER_BYTECODE&,Creator);
// The visible ellipse of view `side` as a*x*x + b*y*y <= 1 in NDC (y down): x<0, x>=0, y<0, y>=0.
void Publish(int side,const float scales[4]);
// The pipeline to bind for `pso` in view `side`.
ID3D12PipelineState* Bind(ID3D12PipelineState* pso,int side);
#endif
}
