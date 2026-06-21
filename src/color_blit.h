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

    void Shutdown();

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;
    DXGI_FORMAT m_colorFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
};
