#pragma once
#include <d3d12.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstdint>
#include <vector>

namespace cvr::vrs {
struct Settings {
    int mode=0; // Off, 2x2, 4x4 (with a 2x2 transition).
    float width=70, height=65, offsetX=0, offsetY=0;
    bool operator==(const Settings&) const = default;
};
struct Status {
    bool checked{},supported{},additionalRates{},failed{};
    unsigned tileSize{},readyMaps{};
    uint64_t draws[2]{};
    uint64_t sceneDraws[2]{},eligibleDraws[2]{};
};
#if CVR_RUST_VRIK
}
// The policy: rust/crates/cvr-render/src/vrs.rs.
extern "C" {
cvr::vrs::Settings cvr_vrs_normalize(cvr::vrs::Settings);
bool cvr_vrs_parse_setting(const char*, cvr::vrs::Settings*);
void cvr_vrs_write_settings(FILE*, cvr::vrs::Settings);
size_t cvr_vrs_rate_count(unsigned width, unsigned height, unsigned tileSize);
void cvr_vrs_build_rates(cvr::vrs::Settings, unsigned width, unsigned height, unsigned tileSize, bool additionalRates,
                         uint8_t* out, size_t count);
bool cvr_vrs_eligible_pipeline(const D3D12_GRAPHICS_PIPELINE_STATE_DESC*);
}
namespace cvr::vrs {
static_assert(sizeof(Settings) == 20, "Settings is rust/crates/cvr-render/src/vrs.rs Settings");
inline Settings Normalize(Settings s) { return cvr_vrs_normalize(s); }
inline bool ParseSetting(const char* line, Settings& s) { return cvr_vrs_parse_setting(line, &s); }
inline void WriteSettings(FILE* file, Settings s) { cvr_vrs_write_settings(file, s); }
inline std::vector<uint8_t> BuildRates(Settings s, unsigned width, unsigned height, unsigned tileSize, bool additionalRates) {
    std::vector<uint8_t> rates(cvr_vrs_rate_count(width, height, tileSize));
    if (!rates.empty()) cvr_vrs_build_rates(s, width, height, tileSize, additionalRates, rates.data(), rates.size());
    return rates;
}
inline bool EligiblePipeline(const D3D12_GRAPHICS_PIPELINE_STATE_DESC& d) { return cvr_vrs_eligible_pipeline(&d); }
#else
Settings Normalize(Settings);
bool ParseSetting(const char*,Settings&);
void WriteSettings(FILE*,Settings);
std::vector<uint8_t> BuildRates(Settings,unsigned width,unsigned height,unsigned tileSize,bool additionalRates);
bool EligiblePipeline(const D3D12_GRAPHICS_PIPELINE_STATE_DESC&);
#endif
using RateFn=void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList5*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
using ImageFn=void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList5*,ID3D12Resource*);
#if CVR_RUST_VRIK
static_assert(sizeof(Status)==64,"Status is rust/crates/cvr-render/src/fixed_vrs.rs Status");
}
// The GPU half: rust/crates/cvr-render/src/fixed_vrs.rs (StreamCreated, the exports and the engine-stereo query stay
// in src/Render/FixedVrs.cpp).
extern "C" {
cvr::vrs::Settings cvr_vrs_get_settings();
void cvr_vrs_set_settings(cvr::vrs::Settings);
void cvr_vrs_get_status(cvr::vrs::Status*);
void cvr_vrs_graphics_created(ID3D12PipelineState*,const D3D12_GRAPHICS_PIPELINE_STATE_DESC*);
void cvr_vrs_reset(ID3D12GraphicsCommandList*,bool rateKnown,ID3D12PipelineState*);
void cvr_vrs_invalidate(ID3D12GraphicsCommandList*);
void cvr_vrs_pipeline(ID3D12GraphicsCommandList*,ID3D12PipelineState*);
void cvr_vrs_viewports(ID3D12GraphicsCommandList*,UINT,const D3D12_VIEWPORT*);
void cvr_vrs_targets(ID3D12GraphicsCommandList*,UINT,bool depth);
void cvr_vrs_shading_rate(ID3D12GraphicsCommandList*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
void cvr_vrs_shading_image(ID3D12GraphicsCommandList*,ID3D12Resource*);
void cvr_vrs_tick(ID3D12Device*,ID3D12CommandQueue*);
void cvr_vrs_shutdown();
void cvr_vrs_prepare_draw(ID3D12GraphicsCommandList*,uint32_t node,int eye,cvr::vrs::RateFn,cvr::vrs::ImageFn);
void cvr_vrs_restore(ID3D12GraphicsCommandList*);
}
namespace cvr::vrs {
inline Settings GetSettings(){return cvr_vrs_get_settings();}
inline void SetSettings(Settings s){cvr_vrs_set_settings(s);}
inline Status GetStatus(){Status s{};cvr_vrs_get_status(&s);return s;}
inline void GraphicsCreated(ID3D12PipelineState* pso,const D3D12_GRAPHICS_PIPELINE_STATE_DESC& d){cvr_vrs_graphics_created(pso,&d);}
void StreamCreated(ID3D12PipelineState*,const D3D12_PIPELINE_STATE_STREAM_DESC&);
inline void Reset(ID3D12GraphicsCommandList* list,bool rateKnown,ID3D12PipelineState* pso){cvr_vrs_reset(list,rateKnown,pso);}
inline void Invalidate(ID3D12GraphicsCommandList* list){cvr_vrs_invalidate(list);}
inline void Pipeline(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso){cvr_vrs_pipeline(list,pso);}
inline void Viewports(ID3D12GraphicsCommandList* list,UINT count,const D3D12_VIEWPORT* viewports){cvr_vrs_viewports(list,count,viewports);}
inline void Targets(ID3D12GraphicsCommandList* list,UINT count,bool depth){cvr_vrs_targets(list,count,depth);}
inline void ShadingRate(ID3D12GraphicsCommandList* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* combiners){cvr_vrs_shading_rate(list,rate,combiners);}
inline void ShadingImage(ID3D12GraphicsCommandList* list,ID3D12Resource* image){cvr_vrs_shading_image(list,image);}
inline void Tick(ID3D12Device* device,ID3D12CommandQueue* queue){cvr_vrs_tick(device,queue);}
inline void Shutdown(){cvr_vrs_shutdown();}
inline void PrepareDraw(ID3D12GraphicsCommandList* list,uint32_t node,int eye,RateFn rate,ImageFn image){cvr_vrs_prepare_draw(list,node,eye,rate,image);}
inline void Restore(ID3D12GraphicsCommandList* list){cvr_vrs_restore(list);}
#else
Settings GetSettings();
void SetSettings(Settings);
Status GetStatus();
extern std::atomic<bool> enabled;

void GraphicsCreated(ID3D12PipelineState*,const D3D12_GRAPHICS_PIPELINE_STATE_DESC&);
void StreamCreated(ID3D12PipelineState*,const D3D12_PIPELINE_STATE_STREAM_DESC&);
void Reset(ID3D12GraphicsCommandList*,bool rateKnown,ID3D12PipelineState*);
void Invalidate(ID3D12GraphicsCommandList*);
void Pipeline(ID3D12GraphicsCommandList*,ID3D12PipelineState*);
void Viewports(ID3D12GraphicsCommandList*,UINT,const D3D12_VIEWPORT*);
void Targets(ID3D12GraphicsCommandList*,UINT,bool depth);
void ShadingRate(ID3D12GraphicsCommandList*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
void ShadingImage(ID3D12GraphicsCommandList*,ID3D12Resource*);
void Tick(ID3D12Device*,ID3D12CommandQueue*);
void Shutdown();

void PrepareDraw(ID3D12GraphicsCommandList*,uint32_t node,int eye,RateFn,ImageFn);
void Restore(ID3D12GraphicsCommandList*);
#endif
}
