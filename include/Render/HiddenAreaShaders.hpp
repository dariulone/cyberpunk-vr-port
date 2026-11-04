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
// After the engine's pipeline was created; any bytecode may be empty.
void PipelineCreated(ID3D12PipelineState*,const D3D12_SHADER_BYTECODE& cs,const D3D12_SHADER_BYTECODE& ps,
                     const D3D12_SHADER_BYTECODE& vs={});
enum class Kind : uint32_t { Dispatch=1, ExecuteIndirect=2, Draw=3, DrawIndexed=4, DispatchRays=5 };
// Every dispatch, indirect execution and draw: x/y/z are the group counts, the maximum command count,
// or vertex (index) / instance counts; for rays the width, height and depth, and no shader.
void Command(uint32_t node,int side,ID3D12PipelineState*,Kind,uint32_t x,uint32_t y,uint32_t z);

// Builds a pipeline from the engine's description with `code` in place of the patched stage.
using Creator=std::function<ID3D12PipelineState*(const D3D12_SHADER_BYTECODE& code)>;
bool HasPatch(const D3D12_SHADER_BYTECODE&);
// The fixed twin of a vertex shader whose group is on, or empty: a pipeline twinned for its pixel shader keeps
// its vertex twin (particle facing) through this.
D3D12_SHADER_BYTECODE FixedVertexTwin(const D3D12_SHADER_BYTECODE&);
// The stereo_rt\ twin of a state object's DXIL library while that group is on, or empty; and the outcome of
// creating a state object with `libraries` of them.
D3D12_SHADER_BYTECODE LibraryTwin(const D3D12_SHADER_BYTECODE&);
void LibraryTwinCreated(bool ok,uint32_t libraries);
void Register(ID3D12PipelineState* original,const D3D12_SHADER_BYTECODE&,Creator);
// The visible ellipse of view `side` as a*x*x + b*y*y <= 1 in NDC (y down): x<0, x>=0, y<0, y>=0.
void Publish(int side,const float scales[4]);
// The pipeline to bind for `pso` in view `side`.
ID3D12PipelineState* Bind(ID3D12PipelineState* pso,int side);
}
