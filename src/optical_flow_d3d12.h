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
    // waitFence/waitValue: if set, the internal convert queue issues a GPU-side
    // Wait(waitFence, waitValue) before reading `source`, so the conversion is
    // ordered AFTER an external producer (e.g. the AER capture copy on another
    // queue) without a CPU stall on the caller's hot path. nullptr = no wait.
    // outSignalValue: if set, the call is FIRE-AND-FORGET — it submits the
    // conversion, signals GetConvertFence() to a fresh value (returned here) and
    // returns IMMEDIATELY with no CPU wait (safe to call from the game/present
    // thread). The consumer must GPU-Wait(GetConvertFence(), *outSignalValue)
    // before reading `converted`. nullptr = legacy synchronous (CPU idle wait).
    bool ConvertToInputTexture(ID3D12Resource* source, ID3D12Resource* converted,
                               ID3D12Fence* waitFence = nullptr, uint64_t waitValue = 0,
                               uint64_t* outSignalValue = nullptr);

    // Fence the internal convert queue signals; used by consumers to GPU-Wait on
    // a fire-and-forget ConvertToInputTexture before reading its output.
    ID3D12Fence* GetConvertFence() const;
    bool ExecuteFlow(ID3D12Resource* previous, ID3D12Resource* current, int eyeIndex);
    bool SynthesizeMidpoint(ID3D12Resource* previous, ID3D12Resource* current, int eyeIndex);
    // CAS (Contrast Adaptive Sharpening) pass: sharpens `source` into `dest` (both
    // caller-owned, COMMON->...->COMMON). sharpness/mix = CAS strength/mix
    // in [0,1]. Synchronous (waits for queue idle). Self-contained -- not yet wired
    // into the live submit path; gated by xr_sharpness>0 at the call site.
    bool ApplySharpen(ID3D12Resource* source, ID3D12Resource* dest, float sharpness, float mix);
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
