// CAS (Contrast Adaptive Sharpening) pass: 3x3 neighborhood, sharpness peak =
// -1/(8 - 3*x),
// final lerp(center, sharpened, mix). filterParam.x = FilterSharpness, .y = mix.
//
// Modeled on StereoReproject: records a fullscreen draw onto the CALLER's command
// list (the submit thread's m_cmdList) -- no separate queue, no idle wait -- so it
// is safe to run per-frame right before the swapchain release / xrEndFrame.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <mutex>
#include <wrl.h>

class SharpenPass {
public:
    SharpenPass() = default;
    ~SharpenPass();

    // (device, output color format, width, height). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT colorFormat,
                           uint32_t width,
                           uint32_t height);

    // Record a fullscreen CAS draw that sharpens `srcColor` into `dstColor`.
    // Caller transitions:  srcColor -> PIXEL_SHADER_RESOURCE,  dstColor -> RENDER_TARGET.
    // sharpness/mix are CAS strength/mix in [0,1].
    bool RecordSharpen(ID3D12GraphicsCommandList* cmdList,
                       ID3D12Resource* srcColor,
                       ID3D12Resource* dstColor,
                       float sharpness,
                       float mix);

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
};
