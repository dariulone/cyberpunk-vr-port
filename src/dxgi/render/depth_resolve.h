// Depth-plane resolve for 64bpp scene depth.
//
// CP2077 (and many modern engines) use R32G8X24_TYPELESS for the main scene
// depth — a 64-bits-per-pixel format with separate depth (32-bit float plane
// 0) and stencil (8-bit uint plane 1) subresources. OpenXR composition_layer_
// depth wants a single typed depth swapchain (D32_FLOAT 32bpp). Naive
// CopyResource between the two formats triggers DEVICE_HUNG because the bit
// layouts don't match.
//
// Solution: fullscreen shader pass. Sample the depth plane via
// R32_FLOAT_X8X24_TYPELESS SRV (reads plane 0 of R32G8X24_TYPELESS as a single
// float channel) and write to a D32_FLOAT render target (DepthStencilView).
// The shader uses SV_Depth to write into the depth plane.
//
// This unlocks the depth-layer submit for the majority of scenes — without
// it positional async-timewarp (alternate-eye smooth AER) is not possible.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <mutex>
#include <wrl.h>

class DepthResolve {
public:
    DepthResolve() = default;
    ~DepthResolve();

    // Lazy init for a given (device, output dims/format). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT depthOutputFormat,
                           uint32_t width,
                           uint32_t height);

    // Record a fullscreen draw that resolves the depth plane of `src` (a
    // R32G8X24_TYPELESS texture) into the depth plane of `dst` (D32_FLOAT or
    // R32_FLOAT, depending on what the swapchain expects). Caller must
    // transition:
    //   src: PIXEL_SHADER_RESOURCE
    //   dst: DEPTH_WRITE
    bool RecordResolve(ID3D12GraphicsCommandList* cmdList,
                       ID3D12Resource* src,
                       ID3D12Resource* dst);

    // COLOR variant for AER V2: resolve the depth plane of `src` into a plain
    // R32_FLOAT COLOR render target `dst` (SV_Target, not SV_Depth). Unlike the
    // depth-stencil output above, a plain R32_FLOAT color texture created with
    // D3D12_HEAP_FLAG_SHARED IS importable into CUDA — this is how the warp kernel
    // gets a usable linear depth for occlusion. Caller transitions:
    //   src: PIXEL_SHADER_RESOURCE,  dst: RENDER_TARGET.
    bool RecordResolveColor(ID3D12GraphicsCommandList* cmdList,
                            ID3D12Resource* src,
                            ID3D12Resource* dst);

    void Shutdown();

private:
    bool EnsureColorPipeline();   // lazy-build the SV_Target R32_FLOAT variant

    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_dsvHeap;
    // Color (R32_FLOAT RTV) variant — its own PSO + SRV/RTV heaps so it never
    // disturbs the depth-output path used by the mono depth-layer submit.
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_psoColor;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeapColor;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    DXGI_FORMAT m_outputFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
};
