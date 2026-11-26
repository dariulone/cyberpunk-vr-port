// The eye images hold display-encoded (sRGB) bytes. When the runtime offers no swapchain format that
// declares them as such (PickMonoSwapchainFormat, include/Runtimes/OpenXRInternal.hpp), the submit draws
// each eye through this pass instead of copying it: decode to linear, and let the target view encode
// again if it is an sRGB format. Ported from the TofuExpress fork (2026-10-16).
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <cstdint>
#include <mutex>
#include <wrl.h>

#if CVR_RUST_VRIK
// The pass: rust/crates/cvr-gpu/src/srgb_pass.rs.
extern "C" {
void* cvr_gpu_srgb_pass_new();
void cvr_gpu_srgb_pass_free(void* pass);
bool cvr_gpu_srgb_pass_ensure(const void* pass, ID3D12Device* device, int format, uint32_t width, uint32_t height);
bool cvr_gpu_srgb_pass_record(const void* pass, ID3D12GraphicsCommandList* cmdList, ID3D12Resource* source,
                              ID3D12Resource* destination, uint32_t descriptorIndex);
void cvr_gpu_srgb_pass_shutdown(const void* pass);
}
class SrgbToLinearPass {
public:
    SrgbToLinearPass() : m_rust(cvr_gpu_srgb_pass_new()) {}
    ~SrgbToLinearPass() { cvr_gpu_srgb_pass_free(m_rust); }
    SrgbToLinearPass(const SrgbToLinearPass&) = delete;
    SrgbToLinearPass& operator=(const SrgbToLinearPass&) = delete;
    // outputFormat: the swapchain's typed format (the render target view). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device, DXGI_FORMAT outputFormat, uint32_t width, uint32_t height) {
        return cvr_gpu_srgb_pass_ensure(m_rust, device, static_cast<int>(outputFormat), width, height);
    }
    // source in PIXEL_SHADER_RESOURCE, destination in RENDER_TARGET. descriptorIndex < 6: one per submit
    // command list (three) and eye, so a list still in flight never has its descriptors rewritten.
    bool Record(ID3D12GraphicsCommandList* cmdList, ID3D12Resource* source, ID3D12Resource* destination, uint32_t descriptorIndex) {
        return cvr_gpu_srgb_pass_record(m_rust, cmdList, source, destination, descriptorIndex);
    }
    void Shutdown() { cvr_gpu_srgb_pass_shutdown(m_rust); }
    // The Rust pass, for rust/crates/cvr-render/src/xr_cycle_end.rs.
    void* Raw() const { return m_rust; }
private:
    void* m_rust;
};
#else
class SrgbToLinearPass {
public:
    SrgbToLinearPass() = default;
    ~SrgbToLinearPass();

    // outputFormat: the swapchain's typed format (the render target view). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device, DXGI_FORMAT outputFormat, uint32_t width, uint32_t height);
    // source in PIXEL_SHADER_RESOURCE, destination in RENDER_TARGET. descriptorIndex < 6: one per submit
    // command list (three) and eye, so a list still in flight never has its descriptors rewritten.
    bool Record(ID3D12GraphicsCommandList* cmdList, ID3D12Resource* source,
                ID3D12Resource* destination, uint32_t descriptorIndex);
    void Shutdown();

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSignature;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pipelineState;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    DXGI_FORMAT m_outputFormat = DXGI_FORMAT_UNKNOWN;
    UINT m_srvDescriptorSize = 0;
    UINT m_rtvDescriptorSize = 0;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
};
#endif
