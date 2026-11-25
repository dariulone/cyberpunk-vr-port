#pragma once
#include <d3d12.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstdint>
#include <vector>

namespace cvr::vrs {
inline constexpr uint32_t kRenderElementsNode = 0x33DDA4u;
struct Settings {
    int mode=0; // Off, 2x2, 4x4 (with a 2x2 transition).
    float width=70, height=65, offsetX=0, offsetY=0;
    bool operator==(const Settings&) const = default;
};
struct Status {
    bool checked{},supported{},additionalRates{},failed{},stateCapHit{};
    unsigned tileSize{},readyMaps{};
    uint64_t draws[2]{};
    uint64_t sceneDraws[2]{},eligibleDraws[2]{};
};
Settings Normalize(Settings);
Settings GetSettings();
void SetSettings(Settings);
bool ParseSetting(const char*,Settings&);
void WriteSettings(FILE*,Settings);
std::vector<uint8_t> BuildRates(Settings,unsigned width,unsigned height,unsigned tileSize,bool additionalRates);
std::vector<uint8_t> BuildStereoRates(Settings leftEye,Settings rightEye,unsigned targetWidth,unsigned targetHeight,unsigned tileSize,bool additionalRates);
bool EligiblePipeline(const D3D12_GRAPHICS_PIPELINE_STATE_DESC&);
Status GetStatus();
extern std::atomic<bool> enabled;

void GraphicsCreated(ID3D12PipelineState*,const D3D12_GRAPHICS_PIPELINE_STATE_DESC&);
void StreamCreated(ID3D12PipelineState*,const D3D12_PIPELINE_STATE_STREAM_DESC&);
void Reset(ID3D12GraphicsCommandList*,bool rateKnown,ID3D12PipelineState*);
void Invalidate(ID3D12GraphicsCommandList*);
void Pipeline(ID3D12GraphicsCommandList*,ID3D12PipelineState*);
void Viewports(ID3D12GraphicsCommandList*,UINT,const D3D12_VIEWPORT*);
void Targets(ID3D12GraphicsCommandList*,UINT count,bool depth,unsigned targetWidth=0,unsigned targetHeight=0,bool blocked=false);
void ShadingRate(ID3D12GraphicsCommandList*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
void ShadingImage(ID3D12GraphicsCommandList*,ID3D12Resource*);
void Tick(ID3D12Device*,ID3D12CommandQueue*);
void Shutdown();

using RateFn=void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList5*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
using ImageFn=void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList5*,ID3D12Resource*);
void PrepareDraw(ID3D12GraphicsCommandList*,uint32_t node,int eye,RateFn,ImageFn);
void Restore(ID3D12GraphicsCommandList*);
}
