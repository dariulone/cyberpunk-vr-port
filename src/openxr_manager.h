#pragma once

#include <windows.h>
#include <d3d12.h>
#include <atomic>
#include <memory>
#include <mutex>
#include <vector>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>

#include "optical_flow_d3d12.h"

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

extern "C" int GetXrRuntimeMode();

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
    // Record the exact OpenXR head pose a given eye's frame was rendered with
    // (called from the camera hook). The AER submit uses this so the runtime can
    // time-warp the older 1/2-rate eye forward to display time. See render-pose submit.
    void StoreRenderEyePose(int eye, const OpenXRHeadPose& pose, uint32_t seq);
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
    bool EnsureMonoCaptureResource(const D3D12_RESOURCE_DESC& sourceDesc);
    bool EnsureDepthSnapshot(ID3D12Resource* gameDepth);
    bool EnsureAERCaptureResources(const D3D12_RESOURCE_DESC& sourceDesc);
    bool CaptureMonoPresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, uint64_t serial,
        const XrPosef poses[2], const XrFovf fovs[2], const bool hasView[2]);
    bool CapturePresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, int eyeIndex, uint64_t serial, uint64_t pairId);

    OpenXRManager() = default;
    ~OpenXRManager() = default;

    std::mutex m_initMutex;
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
    ID3D12CommandAllocator* m_cmdAllocators[3] = {};
    uint32_t m_cmdAllocatorIndex = 0;
    ID3D12GraphicsCommandList* m_cmdLists[3] = {};
    ID3D12Fence* m_fence = nullptr;
    HANDLE m_fenceEvent = nullptr;
    UINT64 m_fenceValue = 0;
    std::mutex m_captureMutex;
    ID3D12CommandAllocator* m_captureCmdAllocators[3] = {};
    uint32_t m_captureAllocatorIndex = 0;
    ID3D12GraphicsCommandList* m_captureCmdLists[3] = {};
    ID3D12Fence* m_captureFence = nullptr;
    HANDLE m_captureFenceEvent = nullptr;
    UINT64 m_captureFenceValue = 0;
    HANDLE m_monoPresentEvent = nullptr;
    HANDLE m_frameSyncEvent = nullptr;
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
    uint64_t m_lastSubmittedPairId = 0;
    uint64_t m_interpolatedPairId = 0;
    XrPosef m_interpolatedEyePoses[2]{};
    XrFovf m_interpolatedEyeFovs[2]{};
    bool m_interpolatedEyeViewsValid[2]{};

    struct EyeSwapchain {
        XrSwapchain handle = XR_NULL_HANDLE;
        int32_t width = 0;
        int32_t height = 0;
        std::vector<XrSwapchainImageD3D12KHR> images;
        std::vector<D3D12_CPU_DESCRIPTOR_HANDLE> rtvs;
        XrSwapchain depthHandle = XR_NULL_HANDLE;
        std::vector<XrSwapchainImageD3D12KHR> depthImages;
    };
    struct CapturedEyeFrame {
        ID3D12Resource* texture = nullptr;
        ID3D12Resource* opticalFlowTexture = nullptr;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t format = 0;
        uint64_t serial = 0;
        uint64_t pairId = 0;
        XrPosef pose{};
        XrFovf fov{};
        bool hasView = false;
    };
    struct CapturedMonoFrame {
        ID3D12Resource* texture = nullptr;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t format = 0;
        uint64_t serial = 0;
        XrPosef poses[2]{};
        XrFovf fovs[2]{};
        bool hasView[2]{};
    };
    std::vector<EyeSwapchain> m_eyeSwapchains;
    CapturedMonoFrame m_monoCapturedFrame;
    // [DEPTH] Game scene-depth snapshot for the XR depth layer (parallel to color).
    ID3D12Resource* m_depthSnapshot = nullptr;
    uint32_t m_depthSnapshotW = 0;
    uint32_t m_depthSnapshotH = 0;
    uint64_t m_depthSnapshotSerial = 0; // serial of the color frame this depth matches; 0 = invalid/empty
    bool m_depthLayerSupported = false;  // runtime supports XR_KHR_composition_layer_depth and a depth swapchain format
    int64_t m_depthSwapchainFormat = 0;  // chosen runtime depth format (e.g. DXGI_FORMAT_D32_FLOAT)
    CapturedEyeFrame m_capturedEyeFrames[2];
    CapturedEyeFrame m_previousCapturedEyeFrames[2];
    CapturedEyeFrame m_pendingEyeFrames[2];
    std::unique_ptr<OpticalFlowD3D12> m_opticalFlow;

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
    std::atomic<bool> m_runtimeIsSteamVR = false;
    // Head velocity in the base-recentered frame (rad/s, m/s) for AER forward pose
    // prediction. Sampled from xrLocateSpace and consumed by GetHeadPose().
    std::atomic<bool> m_velValid = false;
    std::atomic<float> m_angVelX = 0.0f;
    std::atomic<float> m_angVelY = 0.0f;
    std::atomic<float> m_angVelZ = 0.0f;
    std::atomic<float> m_linVelX = 0.0f;
    std::atomic<float> m_linVelY = 0.0f;
    std::atomic<float> m_linVelZ = 0.0f;
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
    // Per-eye head pose captured at render time by the camera hook (render-pose submit).
    std::mutex m_renderPoseMutex;
    XrPosef m_renderEyeHeadPose[2]{};
    bool m_renderEyeHeadPoseValid[2]{};
    
    // Pose queue for accurately syncing frames with pipeline lag
    std::atomic<uint64_t> m_presentCount{0};
    XrPosef m_poseQueue[256]{};
    uint32_t m_poseQueueFrame[256]{};
    
    uint64_t m_syncedPairId = 0;
    int m_aerWarmupRemaining = 0;
    // AER pair id, advanced in lockstep with the eye-capture cadence (bumped on
    // each eye-0 present). Tying it to the eye cadence instead of global present
    // parity keeps left/right of a pair sharing the same id no matter what the
    // global present counter parity is when AER is toggled on. Using present
    // parity desynced the pair (eye0/eye1 got different ids) ~50% of toggles ->
    // pair never completed -> only empty frames submitted -> frozen/blank HMD.
    uint64_t m_aerPairCounter = 0;

    bool m_basePoseSet = false;
    XrPosef m_basePose{};
};
