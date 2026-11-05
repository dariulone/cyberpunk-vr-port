#pragma once
#include <d3d12.h>
#include <array>
#include <functional>
#include <string>
#include <vector>
#include <cstdint>

namespace cvr::stereo {
// Caller supplies validated view-aware shaders and matching array RTV/DSV views.
// This only creates a PSO; it does not enable or redirect game rendering.
struct ViewInstancedLayout {
    std::array<D3D12_VIEW_INSTANCE_LOCATION,2> locations{{{0,0},{0,1}}};
    bool allowMasking=false;
};
HRESULT CreateViewInstancedPipeline(ID3D12Device2*,
    const D3D12_GRAPHICS_PIPELINE_STATE_DESC&,const ViewInstancedLayout&,
    ID3D12PipelineState**);
struct PipelineStreamInfo {
    D3D12_SHADER_BYTECODE vs{},ps{};
    ID3D12RootSignature* root{};
    D3D12_RT_FORMAT_ARRAY targets{};
    DXGI_FORMAT depthFormat=DXGI_FORMAT_UNKNOWN;
    D3D12_BLEND_DESC blend{};
    D3D12_DEPTH_STENCIL_DESC depth{};
    DXGI_SAMPLE_DESC samples{};
    D3D12_INPUT_LAYOUT_DESC input{};
    bool haveBlend{},haveDepth{},depthBounds{};
    bool otherStages{},viewInstancing{},streamOutput{};
    UINT forcedSamples{};
    D3D12_PRIMITIVE_TOPOLOGY_TYPE topology{};
    D3D12_PIPELINE_STATE_FLAGS flags{};
};
// Pointers remain borrowed from the caller during these synchronous operations.
// Scalar payloads start at +4, pointer-aligned payloads at +8 on x64.
bool ReadPipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC&,PipelineStreamInfo&);
struct PipelineStreamOffsets {size_t vs=SIZE_MAX,ps=SIZE_MAX,cs=SIZE_MAX,cached=SIZE_MAX,ms=SIZE_MAX,as=SIZE_MAX;};
bool FindPipelineStreamOffsets(const D3D12_PIPELINE_STATE_STREAM_DESC&,PipelineStreamOffsets&);
// One line of the subobjects and their main fields (diagnostics).
std::string DescribePipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC&);
// A copy of the stream with each subobject's payload handed to `edit` (diagnostics: one change at a time).
bool EditPipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC&,std::vector<uint8_t>&,
                        const std::function<void(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE,uint8_t*)>&);
// A classic description as a subobject stream (no view instancing part); pointers stay the description's.
bool GraphicsDescStream(const D3D12_GRAPHICS_PIPELINE_STATE_DESC&,std::vector<uint8_t>&);
bool BuildViewInstancedStream(const D3D12_PIPELINE_STATE_STREAM_DESC&,const D3D12_SHADER_BYTECODE& replacementVs,
    const ViewInstancedLayout&,std::vector<uint8_t>&,const D3D12_SHADER_BYTECODE& replacementPs={},ID3D12RootSignature* replacementRoot=nullptr);
HRESULT CreateViewInstancedPipelineStream(ID3D12Device2*,const D3D12_PIPELINE_STATE_STREAM_DESC&,
    const D3D12_SHADER_BYTECODE& replacementVs,const ViewInstancedLayout&,ID3D12PipelineState**,const D3D12_SHADER_BYTECODE& replacementPs={},ID3D12RootSignature* replacementRoot=nullptr);

// Pipeline descriptions that own everything they point at (shaders, input layout, stream output,
// view instancing), so a pipeline can be built again after the caller's memory is gone. The cached
// blob is dropped. Root signatures stay borrowed: the engine keeps them alive.
struct OwnedPipelineParts {
    std::vector<std::vector<uint8_t>> shaders;
    std::vector<D3D12_INPUT_ELEMENT_DESC> inputs;
    std::vector<D3D12_SO_DECLARATION_ENTRY> outputs;
    std::vector<UINT> strides;
    std::vector<D3D12_VIEW_INSTANCE_LOCATION> views;
    std::vector<char> names;
};
struct OwnedPipelineStream {
    std::vector<uint8_t> stream;
    PipelineStreamOffsets offsets;
    OwnedPipelineParts parts;
};
struct OwnedGraphicsDesc {
    D3D12_GRAPHICS_PIPELINE_STATE_DESC desc{};
    OwnedPipelineParts parts;
};
// The results point into themselves: fill them in place (e.g. inside a shared_ptr) and never copy them.
bool OwnPipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC&,OwnedPipelineStream&);
bool OwnGraphicsDesc(const D3D12_GRAPHICS_PIPELINE_STATE_DESC&,OwnedGraphicsDesc&);
}
