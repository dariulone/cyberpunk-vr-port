// CAS Sharpening: saturate(center + 4*amount*(CAS-center)).
// The private scratch SRV never changes. All calls must execute in recording
// order on the same queue; barriers serialize scratch copies and shader reads.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <cstdint>
#include <mutex>
#include <wrl.h>

#if CVR_RUST_VRIK
// The pass: rust/crates/cvr-gpu/src/sharpen_pass.rs.
extern "C" {
void* cvr_gpu_sharpen_pass_new();
void cvr_gpu_sharpen_pass_free(void* pass);
bool cvr_gpu_sharpen_pass_ensure(const void* pass, ID3D12Device* device, int format, uint32_t width, uint32_t height);
bool cvr_gpu_sharpen_pass_record(const void* pass, ID3D12GraphicsCommandList* cmdList, ID3D12Resource* srcColor,
                                 ID3D12Resource* dstColor, float sharpness, float amount);
void cvr_gpu_sharpen_pass_shutdown(const void* pass);
}
class SharpenPass {
public:
    SharpenPass() : m_rust(cvr_gpu_sharpen_pass_new()) {}
    ~SharpenPass() { cvr_gpu_sharpen_pass_free(m_rust); }
    SharpenPass(const SharpenPass&) = delete;
    SharpenPass& operator=(const SharpenPass&) = delete;

    // (device, output color format, width, height). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device, DXGI_FORMAT colorFormat, uint32_t width, uint32_t height) {
        return cvr_gpu_sharpen_pass_ensure(m_rust, device, static_cast<int>(colorFormat), width, height);
    }
    // Source stays COPY_SOURCE, destination stays RENDER_TARGET; src==dst sharpens in place and restores COPY_SOURCE.
    // sharpness = Emphasis (0..1), amount = CAS Sharpening (0..1). False records no commands.
    bool RecordSharpen(ID3D12GraphicsCommandList* cmdList, ID3D12Resource* srcColor, ID3D12Resource* dstColor,
                       float sharpness, float amount) {
        return cvr_gpu_sharpen_pass_record(m_rust, cmdList, srcColor, dstColor, sharpness, amount);
    }
    void Shutdown() { cvr_gpu_sharpen_pass_shutdown(m_rust); }

private:
    void* m_rust;
};
#else
class SharpenPass {
public:
    SharpenPass() = default;
    ~SharpenPass();

    // (device, output color format, width, height). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT colorFormat,
                           uint32_t width,
                           uint32_t height);

    // Source stays COPY_SOURCE, destination stays RENDER_TARGET. A private copy
    // also accepts sources with DENY_SHADER_RESOURCE. False records no commands.
    // For an exclusively owned capture, src==dst sharpens in place and restores
    // COPY_SOURCE, so real frames can be filtered before interpolation.
    // sharpness = Emphasis (0..1), amount = CAS Sharpening (0..1, default 0).
    bool RecordSharpen(ID3D12GraphicsCommandList* cmdList,
                       ID3D12Resource* srcColor,
                       ID3D12Resource* dstColor,
                       float sharpness,
                       float amount);

    void Shutdown();

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    Microsoft::WRL::ComPtr<ID3D12Resource> m_scratch;
    DXGI_FORMAT m_colorFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
};
#endif
