// Motion-vector forward warp (Phase 2b).
//
// Takes engine-captured motion vectors (R16G16_FLOAT at render resolution,
// e.g. 2048×3072) plus a previously captured eye color (R8G8B8A8 at output
// resolution, e.g. 3072×3072) and produces a "next frame" prediction by
// sampling `prevColor(uv - mv)` per output pixel. The sampler bilinearly
// upscales the lower-res MV implicitly.
//
// Purpose: in AER the stale eye is one game frame behind the fresh one.
// Warping it forward via engine MVs collapses the inter-eye simulation gap
// (1 game frame) so both submitted eyes share a single timestamp — this is
// what kills the "pushed back when walking" sim-mismatch jitter the user
// reported.
//
// This is intentionally simple (single fullscreen draw, no occlusion edge
// handling yet). Depth-aware refinement is a follow-up.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <mutex>
#include <wrl.h>

class MotionVectorWarp {
public:
    MotionVectorWarp() = default;
    ~MotionVectorWarp();

    // Lazy-init for a given (device, output format / dims). Repeatable —
    // returns true on cached success.
    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT outputFormat,
                           uint32_t outputWidth,
                           uint32_t outputHeight);

    // Records a fullscreen draw on the supplied command list that warps
    // `prevColor` (sampled at output-res UVs) by `mvTexture` (sampled with
    // linear filter so render-res → output-res is implicit) and writes the
    // result to `dstColor`. Caller MUST transition resources:
    //   prevColor: PIXEL_SHADER_RESOURCE
    //   mvTexture: PIXEL_SHADER_RESOURCE
    //   dstColor:  RENDER_TARGET
    // Returns false if not initialized.
    bool RecordWarp(ID3D12GraphicsCommandList* cmdList,
                    ID3D12Resource* prevColor,
                    ID3D12Resource* mvTexture,
                    ID3D12Resource* dstColor,
                    float mvScaleX,
                    float mvScaleY);

    void Shutdown();

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    Microsoft::WRL::ComPtr<ID3D12Resource> m_cb;
    DXGI_FORMAT m_outputFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_outputWidth = 0;
    uint32_t m_outputHeight = 0;
    UINT m_srvStride = 0;
};
