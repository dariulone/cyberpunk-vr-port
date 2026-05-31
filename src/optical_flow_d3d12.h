#pragma once

#include <d3d12.h>
#include <memory>
#include <mutex>

class OpticalFlowD3D12 {
public:
    OpticalFlowD3D12();
    ~OpticalFlowD3D12();

    OpticalFlowD3D12(const OpticalFlowD3D12&) = delete;
    OpticalFlowD3D12& operator=(const OpticalFlowD3D12&) = delete;

    bool EnsureInitialized(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format);
    bool ConvertToInputTexture(ID3D12Resource* source, ID3D12Resource* converted);
    bool ExecuteFlow(ID3D12Resource* previous, ID3D12Resource* current, int eyeIndex);
    bool SynthesizeMidpoint(ID3D12Resource* previous, ID3D12Resource* current, int eyeIndex);
    void Shutdown();
    bool IsReady() const;
    DXGI_FORMAT GetInputTextureFormat() const;
    ID3D12Resource* GetFlowResource(int eyeIndex) const;
    ID3D12Resource* GetInterpolatedResource(int eyeIndex) const;

private:
    struct Impl;

    bool MatchesFailedAttemptLocked(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format) const;

    mutable std::mutex m_mutex;
    std::unique_ptr<Impl> m_impl;
    ID3D12Device* m_lastAttemptDevice = nullptr;
    uint32_t m_lastAttemptWidth = 0;
    uint32_t m_lastAttemptHeight = 0;
    DXGI_FORMAT m_lastAttemptFormat = DXGI_FORMAT_UNKNOWN;
    bool m_lastAttemptFailed = false;
};
