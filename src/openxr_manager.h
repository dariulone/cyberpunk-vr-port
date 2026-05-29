#pragma once

#include <windows.h>
#include <d3d12.h>
#include <atomic>
#include <mutex>
#include <vector>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>

struct IDXGISwapChain;

struct OpenXRHeadPose {
    float posX;
    float posY;
    float posZ;
    float oriX;
    float oriY;
    float oriZ;
    float oriW;
    bool valid;
};

class OpenXRManager {
public:
    static OpenXRManager& Get();

    bool Init();
    void Shutdown();

    // D3D12 specific initialization
    bool InitGraphics(ID3D12Device* device, ID3D12CommandQueue* queue);
    bool GetHeadPose(OpenXRHeadPose* out) const;
    void RequestRecenter();
    void OnPresent(IDXGISwapChain* swapChain);
    void SetMonoSubmitEnabled(bool enabled);
    void SetAERSubmitEnabled(bool enabled);
    bool IsAERSubmitEnabled() const { return m_aerSubmitEnabled.load(std::memory_order_relaxed); }
    int GetCurrentRenderEyeIndex() const { return m_renderEyeIndex.load(std::memory_order_relaxed); }
    float GetRuntimeHorizontalFovDeg() const { return m_runtimeHorizontalFovDeg.load(std::memory_order_relaxed); }
    float GetRuntimeVerticalFovDeg() const { return m_runtimeVerticalFovDeg.load(std::memory_order_relaxed); }
    float GetRuntimeIpd() const { return m_runtimeIpd.load(std::memory_order_relaxed); }
    
    bool GetRecommendedRenderTargetSize(uint32_t* width, uint32_t* height) const;

    bool IsInitialized() const { return m_initialized; }
    bool IsSessionRunning() const { return m_sessionRunning.load(std::memory_order_relaxed); }

private:
    static DWORD WINAPI FrameThreadThunk(LPVOID param);
    DWORD FrameThreadMain();
    void PollEvents();
    bool BeginSession();
    void EndSession();
    bool EnsureMonoSubmitResources();
    bool EnsureAERCaptureResources(const D3D12_RESOURCE_DESC& sourceDesc);
    bool CapturePresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, int eyeIndex, uint64_t serial, uint64_t pairId);

    OpenXRManager() = default;
    ~OpenXRManager() = default;

    bool m_initialized = false;
    XrInstance m_instance = XR_NULL_HANDLE;
    XrSystemId m_systemId = XR_NULL_SYSTEM_ID;
    XrSession m_session = XR_NULL_HANDLE;
    XrSpace m_localSpace = XR_NULL_HANDLE;
    XrSpace m_viewSpace = XR_NULL_HANDLE;
    XrSessionState m_sessionState = XR_SESSION_STATE_UNKNOWN;
    std::vector<XrViewConfigurationView> m_viewConfigViews;
    std::vector<XrView> m_views;

    // Graphics binding
    XrGraphicsBindingD3D12KHR m_graphicsBinding{};
    ID3D12Device* m_d3dDevice = nullptr;
    ID3D12CommandQueue* m_d3dQueue = nullptr;
    ID3D12CommandAllocator* m_cmdAllocator = nullptr;
    ID3D12GraphicsCommandList* m_cmdList = nullptr;
    ID3D12Fence* m_fence = nullptr;
    HANDLE m_fenceEvent = nullptr;
    UINT64 m_fenceValue = 0;
    std::mutex m_captureMutex;
    ID3D12CommandAllocator* m_captureCmdAllocator = nullptr;
    ID3D12GraphicsCommandList* m_captureCmdList = nullptr;
    ID3D12Fence* m_captureFence = nullptr;
    HANDLE m_captureFenceEvent = nullptr;
    UINT64 m_captureFenceValue = 0;
    ID3D12DescriptorHeap* m_rtvHeap = nullptr;
    UINT m_rtvDescriptorSize = 0;
    std::mutex m_viewMutex;
    std::mutex m_presentMutex;
    ID3D12Resource* m_lastPresentedBackBuffer = nullptr;
    uint32_t m_lastPresentedWidth = 0;
    uint32_t m_lastPresentedHeight = 0;
    uint32_t m_lastPresentedFormat = 0;
    uint32_t m_lastPresentedBufferIndex = 0;
    uint64_t m_lastPresentSerial = 0;
    uint64_t m_lastSubmittedSerial = 0;

    struct EyeSwapchain {
        XrSwapchain handle = XR_NULL_HANDLE;
        int32_t width = 0;
        int32_t height = 0;
        std::vector<XrSwapchainImageD3D12KHR> images;
        std::vector<D3D12_CPU_DESCRIPTOR_HANDLE> rtvs;
    };
    struct CapturedEyeFrame {
        ID3D12Resource* texture = nullptr;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t format = 0;
        uint64_t serial = 0;
        uint64_t pairId = 0;
        XrPosef pose{};
        XrFovf fov{};
        bool hasView = false;
    };
    std::vector<EyeSwapchain> m_eyeSwapchains;
    CapturedEyeFrame m_capturedEyeFrames[2];
    CapturedEyeFrame m_pendingEyeFrames[2];

    HANDLE m_frameThread = nullptr;
    std::atomic<bool> m_stopFrameThread = false;
    std::atomic<bool> m_sessionRunning = false;
    std::atomic<bool> m_monoSubmitEnabled = false;
    std::atomic<bool> m_aerSubmitEnabled = false;
    std::atomic<int> m_renderEyeIndex = 0;
    std::atomic<bool> m_poseValid = false;
    std::atomic<float> m_posX = 0.0f;
    std::atomic<float> m_posY = 0.0f;
    std::atomic<float> m_posZ = 0.0f;
    std::atomic<float> m_oriX = 0.0f;
    std::atomic<float> m_oriY = 0.0f;
    std::atomic<float> m_oriZ = 0.0f;
    std::atomic<float> m_oriW = 1.0f;
    std::atomic<float> m_runtimeHorizontalFovDeg = 0.0f;
    std::atomic<float> m_runtimeVerticalFovDeg = 0.0f;
    std::atomic<float> m_runtimeIpd = 0.0f;
    std::atomic<bool> m_recenterRequested = false;
    std::atomic<bool> m_syncedPoseValid = false;
    std::atomic<float> m_syncedPosX = 0.0f;
    std::atomic<float> m_syncedPosY = 0.0f;
    std::atomic<float> m_syncedPosZ = 0.0f;
    std::atomic<float> m_syncedOriX = 0.0f;
    std::atomic<float> m_syncedOriY = 0.0f;
    std::atomic<float> m_syncedOriZ = 0.0f;
    std::atomic<float> m_syncedOriW = 1.0f;
    XrPosef m_syncedEyePoses[2]{};
    XrFovf m_syncedEyeFovs[2]{};
    bool m_syncedEyeViewsValid = false;
    uint64_t m_syncedPairId = 0;
    int m_aerWarmupRemaining = 0;

    bool m_basePoseSet = false;
    XrPosef m_basePose{};
};
