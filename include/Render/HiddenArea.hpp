#pragma once
// Hidden-area mask (docs/hidden-area-mask-20261009.md).
//
// The parts of each eye's render that the headset lens never shows hold near-plane depth while the
// G-buffer is drawn, so no geometry lands there, and far-plane depth while the scene is read, so the
// passes that skip sky skip them too. The visible area is a configurably sized ellipse in the eye's tangent
// space, normalised per quadrant to the runtime eye FOV: 100% touches the panel edges, about 141%
// leaves only what lies outside the panel's FOV rectangle. Everything outside that rectangle is hidden.
#include <d3d12.h>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <vector>

struct D3D12_BARRIER_GROUP;   // enhanced barriers; the full type comes with the Agility SDK header

namespace cvr::hidden_area {
struct Settings {
    int enabled=0;
    float radius=115;   // visible ellipse radius, percent of each half-FOV
    bool operator==(const Settings&) const = default;
};
inline constexpr float MinRadius=70, MaxRadius=145;

// Runtime eye FOV as tangents: left/down negative, right/up positive.
struct Fov { float left=-1, right=1, up=1, down=-1; };
struct Rect { int32_t left, top, right, bottom; };

#if CVR_RUST_VRIK
}
// The geometry: rust/crates/cvr-render/src/hidden_area.rs.
extern "C" {
cvr::hidden_area::Settings cvr_hidden_area_normalize(cvr::hidden_area::Settings);
bool cvr_hidden_area_parse_setting(const char*, cvr::hidden_area::Settings*);
void cvr_hidden_area_write_settings(FILE*, cvr::hidden_area::Settings);
bool cvr_hidden_area_hidden(const cvr::hidden_area::Fov*, float radiusPercent, float x, float y);
bool cvr_hidden_area_shader_ellipse_scales(const cvr::hidden_area::Fov*, float radiusPercent, const cvr::hidden_area::Fov* rendered,
                                           float* scales);
size_t cvr_hidden_area_build_rects(const cvr::hidden_area::Fov*, float radiusPercent, uint32_t width, uint32_t height,
                                   const cvr::hidden_area::Fov* rendered, uint32_t cell, uint32_t margin,
                                   cvr::hidden_area::Rect* out, size_t capacity);
double cvr_hidden_area_coverage(const cvr::hidden_area::Rect*, size_t count, uint32_t width, uint32_t height);
}
namespace cvr::hidden_area {
static_assert(sizeof(Settings) == 8 && sizeof(Fov) == 16 && sizeof(Rect) == 16, "mirrored by rust/crates/cvr-render/src/hidden_area.rs");
inline Settings Normalize(Settings s) { return cvr_hidden_area_normalize(s); }
inline bool ParseSetting(const char* line, Settings& s) { return cvr_hidden_area_parse_setting(line, &s); }
inline void WriteSettings(FILE* file, Settings s) { cvr_hidden_area_write_settings(file, s); }
inline bool Hidden(const Fov& f, float radiusPercent, float x, float y) { return cvr_hidden_area_hidden(&f, radiusPercent, x, y); }
inline std::vector<Rect> BuildRects(const Fov& f, float radiusPercent, uint32_t width, uint32_t height, const Fov& rendered,
                                    uint32_t cell, uint32_t margin) {
    std::vector<Rect> rects;
    if (!cell) return rects;
    rects.resize(size_t((width + cell - 1) / cell) * ((height + cell - 1) / cell));
    rects.resize(cvr_hidden_area_build_rects(&f, radiusPercent, width, height, &rendered, cell, margin, rects.data(), rects.size()));
    return rects;
}
inline std::vector<Rect> BuildRects(const Fov& f, float radiusPercent, uint32_t width, uint32_t height, float tanX, float tanY,
                                    uint32_t cell, uint32_t margin) {
    if (!(tanX > 0) || !(tanY > 0)) return {};
    return BuildRects(f, radiusPercent, width, height, Fov{-tanX, tanX, tanY, -tanY}, cell, margin);
}
inline bool ShaderEllipseScales(const Fov& f, float radiusPercent, const Fov& rendered, float scales[4]) {
    return cvr_hidden_area_shader_ellipse_scales(&f, radiusPercent, &rendered, scales);
}
inline double Coverage(const std::vector<Rect>& rects, uint32_t width, uint32_t height) {
    return cvr_hidden_area_coverage(rects.data(), rects.size(), width, height);
}
#else
Settings Normalize(Settings);
bool ParseSetting(const char*,Settings&);
void WriteSettings(FILE*,Settings);

// Whether the eye-space direction with tangents (x right, y up) is outside the visible area.
bool Hidden(const Fov&,float radiusPercent,float x,float y);
// Rectangles covering every cell of `cell` pixels that is hidden in full and whose neighbours
// within `margin` cells are hidden too, for a width x height target rendering the symmetric
// frustum with half-angle tangents tanX, tanY. Rows with identical runs are merged.
std::vector<Rect> BuildRects(const Fov&,float radiusPercent,uint32_t width,uint32_t height,
                             float tanX,float tanY,uint32_t cell,uint32_t margin);
// The same for a target rendering the window `rendered` (tangents left/right/up/down, which need
// not be centred: the asymmetric projection renders the eye's own off-centre window).
std::vector<Rect> BuildRects(const Fov&,float radiusPercent,uint32_t width,uint32_t height,
                             const Fov& rendered,uint32_t cell,uint32_t margin);
// Per-direction scales (left, right, up, down) of the ellipse centred on NDC 0 that the patched
// shaders test, chosen so that it contains every visible pixel of the window `rendered`: the axis fit
// grown uniformly until it holds the whole visible area. False when the centre itself is hidden.
bool ShaderEllipseScales(const Fov&,float radiusPercent,const Fov& rendered,float scales[4]);
double Coverage(const std::vector<Rect>&,uint32_t width,uint32_t height);
#endif

struct Status {
    uint64_t masked[2]{};       // scene-depth clears masked per view (0 MAIN, 1 VRCAM)
    uint32_t rects[2]{};        // rectangles in the last masked clear
    float hiddenPercent[2]{};   // share of the target covered by them
    uint32_t width[2]{},height[2]{};
};
Settings GetSettings();
void SetSettings(Settings);
Status GetStatus();

// The engine clears COPIES of its depth-stencil descriptors, so the descriptor -> target map follows
// both creation and copies (Hooks/DepthCapture.cpp).
void DsvCreated(ID3D12Resource*,const D3D12_DEPTH_STENCIL_VIEW_DESC*,D3D12_CPU_DESCRIPTOR_HANDLE);
void CopyDescriptors(ID3D12Device*,UINT destinations,const D3D12_CPU_DESCRIPTOR_HANDLE* dest,const UINT* sizes,
                     UINT sources,const D3D12_CPU_DESCRIPTOR_HANDLE* src,const UINT* counts,D3D12_DESCRIPTOR_HEAP_TYPE);

using ClearFn=void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,D3D12_CPU_DESCRIPTOR_HANDLE,D3D12_CLEAR_FLAGS,
                                        FLOAT,UINT8,UINT,const D3D12_RECT*);
// Called after every native ClearDepthStencilView with the current render-graph node and view side.
void AfterDepthClear(ID3D12GraphicsCommandList*,D3D12_CPU_DESCRIPTOR_HANDLE,D3D12_CLEAR_FLAGS,FLOAT depth,
                     UINT rects,int side,uint32_t node,ClearFn);
// Called before the native barriers. When the masked scene depth leaves DEPTH_WRITE, the hidden
// cells go back to the far plane: every later pass then sees them as sky, which the engine's own
// sky checks skip, and screen-space rays and AO samples crossing them see open space instead of a
// wall at the near plane.
void BeforeBarriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*,int side,uint32_t node,ClearFn);
void BeforeEnhancedBarriers(ID3D12GraphicsCommandList*,UINT,const D3D12_BARRIER_GROUP*,int side,uint32_t node,ClearFn);
// After the native barriers: a G-buffer node taking the scene depth back into DEPTH_WRITE gets the
// near plane in the hidden cells again, so its geometry stays out of them.
void AfterBarriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*,int side,uint32_t node,ClearFn);
void AfterEnhancedBarriers(ID3D12GraphicsCommandList*,UINT,const D3D12_BARRIER_GROUP*,int side,uint32_t node,ClearFn);
}
