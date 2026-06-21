// Depth-based stereo reprojection (DBSR).
//
// Synthesizes the OTHER eye's image from one rendered eye + its depth buffer
// by computing per-pixel horizontal disparity from world depth and the
// inter-pupillary distance, then sampling the source color at the shifted UV.
//
// Purpose: kill the "FPS divides by 2 across eyes" problem of AER. With
// alternate-eye rendering the game produces fresh content for ONLY one eye
// per game frame; the other eye in the submitted pair is stale by one game
// frame, causing inter-eye sim-state mismatch (visible as ghosting / "pushed
// back" when walking / dynamic objects out of sync).
//
// With DBSR each submitted pair is {actual_renderedEye, synthesized_otherEye}
// — both represent the SAME simulation timestamp. Effective fresh rate per
// eye equals the game framerate (no divide-by-2). Compositor async-timewarp
// then handles the gap between game frames and HMD ticks as usual.
//
// Cost: ~0.5-1ms per pair (fullscreen sample + tex2D lookups), runs on the
// submit thread's command list right before xrEndFrame.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <mutex>
#include <wrl.h>

class StereoReproject {
public:
    StereoReproject() = default;
    ~StereoReproject();

    // (device, output color format, depth format we'll sample). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT colorFormat,
                           uint32_t width,
                           uint32_t height);

    // Record a fullscreen draw that produces `dstColor` from `srcColor` +
    // `srcDepth` using horizontal disparity. Caller transitions:
    //   srcColor:  PIXEL_SHADER_RESOURCE
    //   srcDepth:  PIXEL_SHADER_RESOURCE (Texture2D<float>; uses red plane)
    //   dstColor:  RENDER_TARGET
    //
    // Parameters:
    //   ipdMeters     — IPD (e.g. 0.065)
    //   horizFovRad   — horizontal field of view in radians
    //   nearZ         — projection near plane (e.g. 0.02)
    //   signLR        — +1 to synthesize right eye from left source,
    //                   -1 to synthesize left eye from right source
    bool RecordReproject(ID3D12GraphicsCommandList* cmdList,
                         ID3D12Resource* srcColor,
                         ID3D12Resource* srcDepth,
                         ID3D12Resource* dstColor,
                         float ipdMeters,
                         float horizFovRad,
                         float nearZ,
                         float signLR);

    void Shutdown();

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    Microsoft::WRL::ComPtr<ID3D12Resource> m_cb;
    DXGI_FORMAT m_colorFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
    UINT m_srvStride = 0;
};
