#pragma once

#include <windows.h>
#include <d3d12.h>
#include <mutex>
#include <wrl.h>

class ColorBlit {
public:
    ColorBlit() = default;
    ~ColorBlit();

    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT colorFormat,
                           uint32_t width,
                           uint32_t height);

    bool RecordBlit(ID3D12GraphicsCommandList* cmdList,
                    ID3D12Resource* srcColor,
                    ID3D12Resource* dstColor);

    // Alpha-blend an overlay over what is already in dstColor, which must be in RENDER_TARGET.
    // The source is expected to carry PREMULTIPLIED alpha -- the blend is ONE / INV_SRC_ALPHA,
    // matching the engine's own HUD pipelines exactly, so the result is what the flat screen
    // would have shown.
    // mode 0 = premultiplied (ONE / INV_SRC_ALPHA), the engine's own HUD blend.
    // mode 1 = OPAQUE replace -- diagnostic: shows the source exactly as it is, so "the layer is
    //          empty" and "the layer is there but the blend eats it" stop being indistinguishable.
    // mode 2 = ADDITIVE (ONE / ONE) -- for a layer whose alpha channel is unused.
    // mode 3 = STRAIGHT alpha (SRC_ALPHA / INV_SRC_ALPHA) -- lerp(dst, src.rgb, src.a). This is
    //          what the engine's own vision composite does; see the note in color_blit.cpp.
    // pixelExact: map source texel (x,y) onto destination texel (x,y) instead of stretching the
    // source across the target. Needed whenever the two differ in size for a reason other than
    // resolution -- the second eye's image is the TOP 2444 rows of a 2444x2560 render, so a
    // stretched overlay lands 4.7% too high at the bottom of the frame.
    bool RecordOverlay(ID3D12GraphicsCommandList* cmdList,
                       ID3D12Resource* srcOverlay,
                       ID3D12Resource* dstColor,
                       int mode = 0,
                       bool pixelExact = false,
                       float offX = 0.0f,
                       float offY = 0.0f);

    // The engine's own HUD composite, ported from PipelineState_576 (the indirect compute
    // dispatch that produces the final colour). Constants below default to the values read out
    // of the game's b6 buffer; they are the whole difference between "the HUD is there" and
    // "the HUD looks right".
    struct HudParams {
        float curvature[2]   = { 0.009017f, 0.084242f };  // _43_m0[3].xy
        float shadowUV[2]    = { 0.0008f,   0.0008f   };  // _43_m0[3].zw
        float mipOffset1[2]  = { -0.0050f, -0.0025f   };  // _43_m0[7].xy
        float mipOffset2[2]  = { -0.0108f, -0.0068f   };  // _43_m0[7].zw
        float mipOffset4[2]  = { -0.0126f, -0.0073f   };  // _43_m0[8].xy
        float glowWeight1    = 0.08f;                     // _43_m0[8].z
        float glowWeight2    = 0.07f;                     // _43_m0[8].w
        float glowWeight4    = 0.06f;                     // _43_m0[9].x
        float aberration     = 0.0001f;                   // _43_m0[6].w
        float hudGain        = 2.0f;                      // the engine's literal x2
        float glowGain       = 1.0f;                      // _43_m0[1].x
        float shadowStrength = 1.0f;                      // _43_m0[4].x
        // The WIDE highlight -- the halo that makes the map, the weapon icons and the tracked
        // quest read as lit instead of flat. The engine adds its own half-res 4-mip HUD pyramid
        // at lod _43_m0[5].y with weight _43_m0[5].x. (It is HUD, not scene bloom: the shadow
        // term beside it lerps that texture's ALPHA against the HUD's.) We snapshot the engine's
        // pyramid rather than approximate it, so these are its numbers, unchanged.
        float bloomGain      = 0.65f;                     // _43_m0[5].x
        float bloomLod       = 1.8f;                      // _43_m0[5].y
        float shadowMip      = 0.036f;                    // _43_m0[4].y
        float time           = 0.0f;                      // _33_m0[0].x
        float flicker        = 1.0f;                      // 0 = hold the scanlines still
        // Filled by RecordHudComposite: the shader compares these against the engine constant
        // buffer's own target size to decide whether that buffer is the one we meant to bind.
        float targetW        = 0.0f;
        float targetH        = 0.0f;
        // Bisection for "the HUD is less sharp than MAIN's": 0 = the full composite, 1 = the HUD
        // term alone (no warp, no glow, no halo, no aberration), 2 = no warp, 3 = no halo,
        // 4 = no glow, 5 = no aberration. Whichever value makes it sharp names the culprit.
        float debugMode      = 0.0f;
        // Horizontal shift of the HUD SAMPLE, in UV. Filled by the caller for the second eye only,
        // because MAIN's HUD is the engine's own draw and cannot move -- so the whole disparity goes
        // into this eye.
        //
        // ZERO IS THE DEFAULT AND THE SHIPPED BEHAVIOUR: the HUD sits at the same pixel in both eyes,
        // which is optical infinity and therefore identical placement -- markers and labels land in the
        // same spot in each eye.
        //
        // Non-zero puts the HUD at a finite distance instead. That is better optics and it fixed a real
        // complaint on a Pimax ("both images too far to the sides to converge", because an icon at
        // infinity cannot fuse while the world is an arm's length away) -- but it is also visible as
        // per-eye misalignment on a headset where it is not needed. Which is right depends on the
        // headset, so the caller decides via CyberpunkVR_HudDistanceM and the note there explains both
        // sides.
        float hudShiftU      = 0.0f;
    };

    // Full composite: scene + HUD -> dstColor (which must be in RENDER_TARGET). Unlike
    // RecordOverlay this READS the scene, because the engine's glow is modulated by scene luma
    // and the scene is attenuated under the HUD -- neither is expressible as a blend state.
    // srcHudBlur is the engine's own half-res HUD pyramid and srcExposure its FrameExposureData
    // buffer; both are required, because the halo and the overall HUD brightness are the engine's
    // arithmetic on the engine's data, not something to approximate.
    bool RecordHudComposite(ID3D12GraphicsCommandList* cmdList,
                            ID3D12Resource* srcScene,
                            ID3D12Resource* srcHud,
                            ID3D12Resource* srcHudBlur,
                            ID3D12Resource* srcExposure,
                            ID3D12Resource* srcFrameCB,
                            ID3D12Resource* srcHudCB,
                            ID3D12Resource* dstColor,
                            const HudParams& params);

    // A procedural radial-mesh laser spot at a point in NDC (-1..1, +Y up), straight-alpha
    // blended over dstColor, which must be in RENDER_TARGET. Retained for the desktop second-eye
    // mirror; both HMD eyes use the matching ImGui mesh from the overlay path.
    bool RecordDot(ID3D12GraphicsCommandList* cmdList,
                   ID3D12Resource* dstColor,
                   float ndcX, float ndcY,
                   float radiusPx,
                   float r, float g, float b, float a);

    void Shutdown();

    // Descriptors are consumed when the command list EXECUTES, not when it is recorded, and
    // the submit path keeps up to three frames in flight. A single SRV/RTV slot would be
    // rewritten by frame N+1 while the GPU is still reading it for frame N.
    // Two passes per frame (blit + HUD overlay) share this ring, so it is three frames' worth
    // of BOTH -- six, not three.
    static constexpr uint32_t kSlots = 6;

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_psoOverlay;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_psoOverlayAdd;   // ONE / ONE
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_psoOverlayStraight;  // SRC_ALPHA / INV_SRC_ALPHA
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSigHud;   // 2 SRVs + root constants
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_psoHud;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSigDot;   // root constants only
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_psoDot;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_hudSrvHeap;  // pairs: scene, hud
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    uint32_t m_slot = 0;
    uint32_t m_srvStride = 0;
    uint32_t m_rtvStride = 0;
    DXGI_FORMAT m_colorFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
};
