#pragma once
// SinglePassPipelines -- the pipelines and root signature single-pass stereo draws with (Stereo/SinglePass.hpp has
// the draws).
//
// MATERIAL ROOT: the engine's root signature of RenderElements: 36 descriptor tables, parameter 0 = the vertex
// stage's CBVs b0..b3, parameter 18 = the pixel stage's. Its TWIN appends parameter 36 (vertex) and 37 (pixel): the
// same four CBVs in register space 10, where the twin shaders read the left eye's b0..b3.
//
// MATERIAL PIPELINE: a graphics pipeline created with a material root (VS + optional PS, no other stage, no stream
// output, no view instancing). Its description is owned at creation; its view-instanced TWIN is built in the
// background at first use: VS and PS rewritten (Render/ViewTwinIr.hpp; cached on disk by the hash of the shader and
// the recipe), the twin root, and two views -- view 0 on viewport 0 (left eye), view 1 on viewport 1 (right eye),
// both on render target slice 0 (side-by-side frame).
//
// The shaders are the ones the engine's pipeline is BOUND with: when the hidden-area twins replace a stage
// (Render/HiddenAreaShaders.hpp, fixed groups under engine stereo), the twin is built from that replacement and is
// only used while the replacement is what gets bound.
#include <d3d12.h>
#include <atomic>
#include <cstdint>

namespace cvr::single_pass {
using CreateStreamFn=HRESULT(STDMETHODCALLTYPE*)(ID3D12Device*,const D3D12_PIPELINE_STATE_STREAM_DESC*,REFIID,void**);
struct Pipeline;

// Device hooks (Stereo/DeviceHooks.cpp). create: the device's original CreatePipelineState.
void RootCreated(ID3D12Device*,ID3D12RootSignature*,const void* blob,size_t bytes);
void GraphicsCreated(ID3D12Device*,const D3D12_GRAPHICS_PIPELINE_STATE_DESC&,ID3D12PipelineState*,CreateStreamFn create);
void StreamCreated(ID3D12Device*,const D3D12_PIPELINE_STATE_STREAM_DESC&,ID3D12PipelineState*,CreateStreamFn create);

bool Available();                                        // DXC loaded and the feature not disabled
ID3D12RootSignature* TwinRoot(ID3D12RootSignature*);     // nullptr unless a material root with a twin
Pipeline* Find(ID3D12PipelineState*);                    // nullptr unless a material pipeline; lives as long as the PSO
ID3D12RootSignature* RootOf(const Pipeline*);
uint32_t InputSlots(const Pipeline*);   // bit n: the input layout reads vertex buffer slot n
// The view-instanced twin for a draw bound with `bound` (the engine's pipeline or its hidden-area replacement):
// nullptr until built -- the first call queues the build -- or when it cannot be built.
ID3D12PipelineState* Twin(Pipeline*,ID3D12PipelineState* engine,ID3D12PipelineState* bound);
}

extern "C" {
// [0] material roots, [1] material pipelines registered, [2] twins queued, [3] twins built, [4] twins failed,
// [5] shaders rewritten, [6] shaders that need no rewrite, [7] shaders failed, [8] shaders from the disk cache,
// [9] owned description bytes, [10] build milliseconds (shaders + pipeline) summed, [11] bound replacement unknown
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_SinglePassPipelineCounts[12];
__declspec(dllexport) extern char CyberpunkVR_SinglePassError[256];
}
