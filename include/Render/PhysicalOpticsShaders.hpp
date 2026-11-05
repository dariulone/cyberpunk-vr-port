#pragma once
#include <d3d12.h>
#include <wrl/client.h>
#include <array>

namespace cvr::optics {
bool GlassShaderReplacement(const D3D12_SHADER_BYTECODE& native,D3D12_SHADER_BYTECODE& replacement);

// The lens shader magnifies the scene image, so one coarse VRS pixel becomes a
// visible block of M x M pixels. The engine's adaptive shading-rate image
// coarsens exactly while the camera turns. Glass draws therefore shade every
// pixel; every other draw keeps the engine's shading rate.
// velocity: the glass draw's twin writing lens velocity; mask: its twin writing the
// upscaler history mask (both owned, may be null).
void GlassPipelineCreated(ID3D12PipelineState* glass,ID3D12PipelineState* velocity,ID3D12PipelineState* mask);
// Whether a glass pipeline drew within the last half second.
bool GlassRecent();
// Engine shading-rate state per command list, so a glass draw can hand it back.
void ListReset(ID3D12GraphicsCommandList*);
void ShadingRateSet(ID3D12GraphicsCommandList*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
using RateFn=void (STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList5*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
class FullRateDraw {
public:
    // set must be the original (unhooked) RSSetShadingRate.
    FullRateDraw(ID3D12GraphicsCommandList*,ID3D12PipelineState*,RateFn set);
    ~FullRateDraw();
    FullRateDraw(const FullRateDraw&)=delete;
    FullRateDraw& operator=(const FullRateDraw&)=delete;
private:
    ID3D12GraphicsCommandList5* list{};
    RateFn set{};
    D3D12_SHADING_RATE rate=D3D12_SHADING_RATE_1X1;
    std::array<D3D12_SHADING_RATE_COMBINER,2> combiners{};
};

// Lens velocity. The glass pass writes no velocity, so the upscaler moved lens
// history with the unmagnified world behind it and dropped it on every turn.
// After each glass draw the port redraws the lens into the G-buffer velocity
// target with lens_velocity_ps (velocity of the magnified image).
bool LensVelocityShader(D3D12_SHADER_BYTECODE&);
// Streamline's MotionVectors tag (sl::BufferType 1).
void MotionVectorsTagged(ID3D12Resource*);
Microsoft::WRL::ComPtr<ID3D12Resource> MotionVectors();
struct BoundTargets {
    UINT count{};
    BOOL contiguous{};
    std::array<D3D12_CPU_DESCRIPTOR_HANDLE,8> handles{};
    D3D12_CPU_DESCRIPTOR_HANDLE depth{};
    bool hasDepth{},valid{};
};
// Every engine OMSetRenderTargets, so the redraw can restore the glass targets.
void Targets(ID3D12GraphicsCommandList*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*);
struct LensVelocityPass {
    ID3D12PipelineState* pipeline{};
    Microsoft::WRL::ComPtr<ID3D12Resource> target;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv{};
    BoundTargets targets;
};
bool PrepareLensVelocity(ID3D12GraphicsCommandList*,ID3D12PipelineState* glass,LensVelocityPass&);
// Whether the pipeline is a registered glass (the bound one cached per thread): what PrepareLensVelocity and
// GlassMaskPipeline test first, for a draw hook to ask before building a pass (ETW: ~0.25 ms of CPU a frame went
// into building and clearing a LensVelocityPass for every draw).
bool GlassPipeline(ID3D12PipelineState* pso);
// The render targets the engine last bound on this list (this thread).
bool BoundTargetsOf(ID3D12GraphicsCommandList*,BoundTargets&);

// Upscaler history mask (lens_mask_ps.hlsl, sight_mask_ps.hlsl, LensMask.cpp).
// Inside the scope image one internal pixel spans M x M output pixels and a turn
// moves the image M times faster than its surface; the upscaler's history there
// blinked in points and blocks. The image is stabilised before it reaches the
// glass (lens source resolve, camera source resolve), so the upscaler takes the
// current frame there: the port redraws the glass (camera path: the camera
// surface) into an R8 mask that rides with the velocity tag as
// kBufferTypeBiasCurrentColorHint. CyberpunkVR_OpticsLensClean 0 turns it off.
bool LensMaskShader(D3D12_SHADER_BYTECODE&);
bool SightMaskShader(D3D12_SHADER_BYTECODE&);
// The mask twin of a glass pipeline, null for any other pipeline.
ID3D12PipelineState* GlassMaskPipeline(ID3D12PipelineState* glass);
namespace mask {
bool Enabled();
// The eye's mask reached the upscaler within the last half second.
bool Active(int side);
// The eye's mask and its render target view, once its velocity was tagged.
bool Target(int side,Microsoft::WRL::ComPtr<ID3D12Resource>&,D3D12_CPU_DESCRIPTOR_HANDLE&);
// A mask redraw was recorded for the eye.
void Drawn(int side);
using SetTagFn=uint32_t (*)(const void*,const void*,uint32_t,void*);
// slSetTag through the original: adds the eye's mask to a velocity tag and clears it after.
uint32_t SetTag(const void* viewport,const void* tags,uint32_t count,void* commandList,SetTagFn original,int side);
}

// Lens source resolve (lens_source_resolve_cs.hlsl). The glass magnifies the
// jittered pre-upscale scene copy, so sub-pixel lights and grate bars blink in
// M x M blobs the upscaler cannot settle. Before the list holding the glass
// pass runs, the port accumulates the scene over frames and hands the glass a
// temporally stable image jittered like the frame, copied over the scene copy
// by that list. CyberpunkVR_OpticsSourceResolve 0 turns it off.
bool LensSourceResolveShader(D3D12_SHADER_BYTECODE&);
namespace source {
// At every engine CopyTextureRegion, after the original copy. For the glass
// pass' scene copy (whole R11G11B10 copy of the scene colour, a glass drawn
// recently, view side 0/1) the list is registered for a resolve and the image
// to copy over the scene copy is returned (null: copy nothing, e.g. the first
// frame). Owned by the port; the caller keeps it alive with the list.
ID3D12Resource* SceneCopied(ID3D12GraphicsCommandList*,int side,const D3D12_TEXTURE_COPY_LOCATION* dst,
    UINT x,UINT y,UINT z,const D3D12_TEXTURE_COPY_LOCATION* src,const D3D12_BOX*);
// Every render-graph node dispatch of an eye (side 0/1): its view's jitter,
// so the resolve places each jittered sample where it really lies.
void ViewDispatched(int side,const void* workContext);
// xr_scope_stabilize: percent of history in a moving lens, 0 = off.
void SetStabilize(int percent);
int GetStabilize();
void ListReset(ID3D12GraphicsCommandList*);
using ExecuteFn=void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
// Submission through the original ExecuteCommandLists: each registered list
// runs right after its own resolve.
void Execute(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*,ExecuteFn);
}

// Camera-scope source resolve (camera_source_resolve_cs.hlsl). The scope's RTT
// view renders without temporal anti-aliasing (no jitter, no history), so the
// effects built for a temporal resolve -- reflections, AO, contact shadows --
// leave dark single-pixel dots that move every frame. Right after the engine
// list that writes the camera image (the view's CopyToTexture node) runs, the
// port accumulates that image over frames in place: history reprojected by
// the camera's own rotation, clipped to the current neighbourhood, isolated
// dark outliers replaced. CyberpunkVR_OpticsCameraResolve 0 turns it off.
bool CameraSourceResolveShader(D3D12_SHADER_BYTECODE&);
namespace camera {
// Around the scope view's CopyToTexture node, on its recording thread.
void CopyNodeBegin(const void* workContext);
void CopyNodeEnd();
bool NodeActive();
// Render target bound by the engine (resolved resource), any thread.
void TargetBound(ID3D12GraphicsCommandList*,ID3D12Resource*);
void Copied(ID3D12GraphicsCommandList*,ID3D12Resource* destination);
void Barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void ListReset(ID3D12GraphicsCommandList*);
// Splits a submission after each list that wrote the camera image; the rest
// goes through source::Execute.
void Execute(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*,source::ExecuteFn);
}
}
