#pragma once

#include <windows.h>
#include <d3d12.h>
#include <wrl.h>
#include <atomic>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>

#include "optical_flow_d3d12.h"
#include "mv_warp.h"
#include "depth_resolve.h"
#include "stereo_reproject.h"
#include "sharpen_pass.h"
#include "color_blit.h"
#include "aer_v2_pipeline.h"
#include "aer_v2/pose_predictor.h"

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

// Aggregated VR-controller snapshot, queried each frame from the XInput hook
// to merge into the game's XINPUT_GAMEPAD state. Button bits intentionally
// mirror XInput's XINPUT_GAMEPAD_* constants so we can OR them directly.
struct VRControllerState {
    uint16_t buttons = 0;
    float    leftTrigger  = 0.0f;
    float    rightTrigger = 0.0f;
    float    leftThumbX   = 0.0f;
    float    leftThumbY   = 0.0f;
    float    rightThumbX  = 0.0f;
    float    rightThumbY  = 0.0f;
    float    leftGrip     = 0.0f;
    float    rightGrip    = 0.0f;
    bool     leftHandValid  = false;
    bool     rightHandValid = false;
};

extern "C" int GetXrRuntimeMode();
extern "C" float GetWeaponPitch();
extern "C" float GetWeaponYaw();
extern "C" float GetWeaponRoll();
extern "C" float GetWeaponOffsetX();
extern "C" float GetWeaponOffsetY();
extern "C" float GetWeaponOffsetZ();

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
    // Frame logic
    void StoreRenderEyePose(int eye, const OpenXRHeadPose& pose, uint32_t seq);

    // Write hand positions/orientations + HMD orientation to shared memory.
    // Called from OnLocateCameraCallback (BEFORE render) to eliminate the 1-frame
    // hand lag that causes AER hand displacement artifacts.
    // POSE PAIR LOCKING (45 Hz skeleton). Snapshots the whole tracking state (head
    // pose, hands, HMD ori, physical height). Called from OnPresent at the PAIR
    // BOUNDARY (follower eye) so the snapshot is published BEFORE the next pair's
    // animation pass — the VRIK plugin reads shared memory during anim eval, which
    // runs before render/LocateCamera, so taking the snapshot in LocateCamera was
    // one stage too late and tore the skeleton across the pair.
    void UpdatePairLock();

    // Write the pair-locked hands + HMD orientation + body height ([0..19],[89],[90])
    // to shared memory. Reads the frozen snapshot, so both eyes of a pair feed VRIK
    // identical inputs. Called right after UpdatePairLock in OnPresent (early enough
    // that the next frame's animation reads it).
    void FlushHandsToShared();

    // Pair-locked head pose for the camera: returns the snapshot UpdatePairLock
    // froze for the current stereo pair (both eyes share ONE head pose), so the
    // engine stops re-sampling tracking ~11 ms apart per eye and VRIK stops
    // rebuilding a different skeleton per eye.
    bool GetPairLockedHeadPose(OpenXRHeadPose* out);

    // Hands
    bool GetHandPose(int handIndex, OpenXRHeadPose* out) const;
    void SetWeaponOffsets(float pitch, float yaw, float roll, float dx, float dy, float dz);

    // VR hand-tracking activation, driven from the in-headset overlay menu. The
    // value is published into shared-memory slot [32] each present; the RED4ext
    // plugin polls it, installs/arms, and sets g_VRBind to this value. Use 4 =
    // full-arm IK (same as the CET button); 0 = off. 1-3 are legacy fallbacks.
    void SetVRHandTrackingMode(int mode) { m_vrHandTrackingMode.store(mode, std::memory_order_relaxed); }
    int GetVRHandTrackingMode() const { return m_vrHandTrackingMode.load(std::memory_order_relaxed); }
    // "Bullet from weapon barrel" VR aim: published to shared[58] (enable), read by the RED4ext
    // plugin's GetOrientation VMT override.
    void SetWeaponAimEnable(int v) { m_weaponAimEnable.store(v, std::memory_order_relaxed); }
    int GetWeaponAimEnable() const { return m_weaponAimEnable.load(std::memory_order_relaxed); }
    // Weapon holster mode: 1 = immersive (visual-holster equip), 0 = simple slot mapping.
    // Published to shared[23], read by the CET Holster mod via GetVRSharedSlot(23).
    void SetImmersiveHolsters(int v) { m_immersiveHolsters.store(v, std::memory_order_relaxed); }
    int GetImmersiveHolsters() const { return m_immersiveHolsters.load(std::memory_order_relaxed); }
    // Read a shared-mem slot (plugin publishes the muzzle world forward to [24..26], valid [27]).
    float GetSharedSlot(int i) const { float* p = m_sharedHandsPtr; return (p && i >= 0 && i < 128) ? p[i] : 0.0f; }
    // Write a shared-mem slot (XInput merge publishes the melee power-trigger flag to [30]).
    void SetSharedSlot(int i, float v) { float* p = m_sharedHandsPtr; if (p && i >= 0 && i < 128) p[i] = v; }

    // Publish IK calibration to the plugin (see m_calib). Order matches the [33..47] slots.
    void SetVRHandCalib(float scaleR, float scaleL, float heightR, float heightL,
                        float swingR, float swingL, float poleR, float poleL,
                        float wRp, float wRy, float wRr, float wLp, float wLy, float wLr) {
        float v[14] = { scaleR, scaleL, heightR, heightL, swingR, swingL, poleR, poleL,
                        wRp, wRy, wRr, wLp, wLy, wLr };
        for (int i = 0; i < 14; ++i) m_calib[i].store(v[i], std::memory_order_relaxed);
        m_calibValid.store(1, std::memory_order_relaxed);
    }

    // ==== AUTO-CALIBRATION ====
    // T-pose calibration: user holds arms straight out sideways and stands straight.
    // We sample the live HMD + controller positions over `secs` seconds and compute:
    //   * playerHeight    = HMD Y in world (above floor)
    //   * armSpan         = max(|leftCtrl - rightCtrl|) during the sample
    //   * shoulderOffsets = anatomical HMD/body -> shoulder offsets in body-frame OpenXR axes,
    //                       derived from the visible controller/gizmo T-pose
    //   * armScale        = proven near-1.0 reach scale with small per-hand correction from
    //                       measured shoulder-to-controller asymmetry
    // Start with StartAutoCalibration(); after `secs` seconds the result is published via
    // SetVRHandCalib + the shoulder anatomical offsets (also stored in m_calibExt).
    void StartAutoCalibration(float secs = 4.0f);
    void TickAutoCalibration();   // called from the frame thread
    bool IsCalibrating() const { return m_calibState.load(std::memory_order_relaxed) != 0; }
    float GetCalibrationProgress() const { return m_calibProgress.load(std::memory_order_relaxed); }
    int GetCalibrationState() const { return m_calibState.load(std::memory_order_relaxed); }

    // Extra calibration parameters (anatomical HMD->shoulder offsets) that didn't fit in the
    // original 14-slot block. Stored on the manager so the plugin can pull them through shared
    // mem slots [70..76] and Save/Load can persist them.
    void SetShoulderAnatomical(float rx, float ry, float rz, float lx, float ly, float lz) {
        m_calibExt[0].store(rx, std::memory_order_relaxed);
        m_calibExt[1].store(ry, std::memory_order_relaxed);
        m_calibExt[2].store(rz, std::memory_order_relaxed);
        m_calibExt[3].store(lx, std::memory_order_relaxed);
        m_calibExt[4].store(ly, std::memory_order_relaxed);
        m_calibExt[5].store(lz, std::memory_order_relaxed);
        m_calibExtValid.store(1, std::memory_order_relaxed);
    }
    float GetCalibExt(int i) const {
        return (i >= 0 && i < 6) ? m_calibExt[i].load(std::memory_order_relaxed) : 0.0f;
    }

    // CAMERA->HEAD bake offset. The CP2077 FPP camera is mounted ~0.45 m ahead of the head bone;
    // the plugin publishes the (head - camera) offset into shared [85..87] and we bake it here so
    // dxgi's LocateCamera shifts the view back onto the avatar's head. The Tracking/Camera Head
    // sliders are applied ON TOP of this (they stay at 0 after baking, for fine adjustment).
    void BakeCameraOffset();              // capture shared [85..87] -> m_camBakeOffset
    void ClearCameraOffset() {
        m_camBakeOffset[0].store(0.0f, std::memory_order_relaxed);
        m_camBakeOffset[1].store(0.0f, std::memory_order_relaxed);
        m_camBakeOffset[2].store(0.0f, std::memory_order_relaxed);
    }
    void SetCameraOffset(float x, float y, float z) {
        m_camBakeOffset[0].store(x, std::memory_order_relaxed);
        m_camBakeOffset[1].store(y, std::memory_order_relaxed);
        m_camBakeOffset[2].store(z, std::memory_order_relaxed);
    }
    void GetCameraOffset(float* out) const {
        out[0] = m_camBakeOffset[0].load(std::memory_order_relaxed);
        out[1] = m_camBakeOffset[1].load(std::memory_order_relaxed);
        out[2] = m_camBakeOffset[2].load(std::memory_order_relaxed);
    }

    // Persist current calibration (everything: scales, heights, swings, poles, wrists, shoulder
    // offsets) to a file next to dxgi.dll. Returns true on success.
    bool SaveCalibrationToFile();
    bool LoadCalibrationFromFile();   // restored at startup so the user doesn't recalibrate each launch
    // Monotonic counter: the plugin dumps a diag whenever the published value changes.
    void RequestVRDiag() { m_logDiagReq.fetch_add(1, std::memory_order_relaxed); }

    // HMD yaw (radians) relative to the recenter base (= body forward), derived
    // from relOri. Used for head-oriented locomotion (rotate on-foot move vector).
    float GetHmdYawRelToBody() const;
    // Same idea for a controller (side: 0=left, 1=right). Used for hand-oriented
    // locomotion (character walks the way the chosen controller points). Returns
    // 0 if the controller pose isn't valid this frame.
    float GetHandYawRelToBody(int side) const;

    // Per-frame snapshot of all controller buttons/axes (OpenXR action state).
    // Filled by the frame thread under m_inputMutex; copied out by readers.
    bool GetControllerState(VRControllerState* out) const;

    float GetRuntimeHorizontalFovDeg() const { return m_runtimeHorizontalFovDeg.load(std::memory_order_relaxed); }
    bool IsRuntimeSteamVR() const { return m_runtimeIsSteamVR.load(std::memory_order_relaxed); }
    bool IsRuntimeVirtualDesktop() const { return m_runtimeIsVirtualDesktop.load(std::memory_order_relaxed); }
    float GetRuntimeVerticalFovDeg() const { return m_runtimeVerticalFovDeg.load(std::memory_order_relaxed); }
    float GetRuntimeIpd() const { return m_runtimeIpd.load(std::memory_order_relaxed); }
    bool GetCurrentEyeCenterOffset(int eye, XrVector3f* out);
    bool GetCurrentEyeFov(int eye, XrFovf* out);
    
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
    // Records barriers + (copy OR shader resolve) into m_captureCmdList that
    // captures the current frame's scene depth into m_depthSnapshot. Uses a
    // simple CopyTextureRegion for R32-family sources (32bpp bit-compat) and
    // the DepthResolve shader for R32G8X24-family sources (64bpp plane 0
    // extract). Returns true if the snapshot was successfully recorded.
    bool RecordDepthCapture(ID3D12GraphicsCommandList* cmdList,
                            ID3D12Resource* gameDepth,
                            D3D12_RESOURCE_STATES gameDepthState);
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
    XrActionSet m_actionSet = XR_NULL_HANDLE;
    XrAction m_handPoseAction = XR_NULL_HANDLE;          // grip pose (palm) -- used by VRIK
    XrAction m_handAimPoseAction = XR_NULL_HANDLE;       // aim pose (pointing) -- used by hand-locomotion
    XrSpace  m_handAimSpaces[2] = { XR_NULL_HANDLE, XR_NULL_HANDLE };
    // Gameplay input actions (synced each frame, exposed via GetControllerState).
    XrAction m_thumbstickAction = XR_NULL_HANDLE;        // Vector2f, per hand
    XrAction m_triggerAction = XR_NULL_HANDLE;           // Float, per hand
    XrAction m_gripAction = XR_NULL_HANDLE;              // Float, per hand
    XrAction m_thumbstickClickAction = XR_NULL_HANDLE;   // Bool, per hand (L3/R3)
    XrAction m_primaryButtonAction = XR_NULL_HANDLE;     // Bool, per hand (X / A)
    XrAction m_secondaryButtonAction = XR_NULL_HANDLE;   // Bool, per hand (Y / B)
    XrAction m_menuButtonAction = XR_NULL_HANDLE;        // Bool, left only on Touch
    XrPath m_handPaths[2] = { XR_NULL_PATH, XR_NULL_PATH };
    XrSpace m_handSpaces[2] = { XR_NULL_HANDLE, XR_NULL_HANDLE };
    // Latest controller snapshot, owned by the frame thread.
    mutable std::mutex m_inputMutex;
    VRControllerState m_controllerState{};
    // Yaws (radians, recenter-base relative) for hand-oriented locomotion.
    std::atomic<float> m_handYawRelToBody[2]{ {0.0f}, {0.0f} };
    std::atomic<bool>  m_handYawValid[2]{ {false}, {false} };
    struct TrackingPoseFilterState {
        bool initialized = false;
        XrVector3f position{};
        XrQuaternionf orientation{};
    };
    struct TrackingAngleFilterState {
        bool initialized = false;
        float angleRad = 0.0f;
    };
    // Adaptive tracking filters: remove static jitter, but release quickly once
    // the player actually moves the HMD/controller.
    TrackingPoseFilterState m_headFilterState{};
    TrackingPoseFilterState m_handFilterState[2]{};
    TrackingAngleFilterState m_handAimYawFilter[2]{};
    XrSessionState m_sessionState = XR_SESSION_STATE_UNKNOWN;
    std::vector<XrViewConfigurationView> m_viewConfigViews;
    std::vector<XrView> m_views;

    // Hands
    std::mutex m_handMutex;
    OpenXRHeadPose m_hands[2]{};
    // ── Pose Pair Locking (45 Hz skeleton) ── guarded by m_handMutex.
    // Snapshot of the pair-leader eye's tracking, replayed on eye1 so VRIK's
    // skeleton is frozen across the stereo pair. m_pairLockHmdOri is the RAW
    // (un-remapped, un-predicted) OpenXR HMD orientation written to shared[16..19].
    bool m_pairLockHeadValid = false;
    bool m_pairLockHandsValid = false;
    // Last PRESENT eye seen by the OnPresent pair-lock publish — edge-detects the
    // follower present (0->1) so the snapshot is taken once per pair. -1 = none yet.
    int m_pairLockLastEye = -1;
    // Seqlock counter for the shared-memory pose block (FlushHandsToShared). Bumped
    // odd before / even after each write so the VRIK plugin can detect torn reads.
    uint32_t m_sharedSeq = 0;
    OpenXRHeadPose m_pairLockHeadPose{};
    OpenXRHeadPose m_pairLockHands[2]{};
    float m_pairLockHmdOri[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    // Frozen RAW physical head height (m_posY, no prediction) — drives VRIK body
    // height/squat shared slots [89]/[90], which otherwise sampled live per eye and
    // bobbed the body at the alternation rate. Snapshot alongside m_pairLockHmdOri.
    float m_pairLockHmdPosY = 0.0f;
    float m_weaponPitch = 0.0f;
    float m_weaponYaw = 0.0f;
    float m_weaponRoll = 0.0f;
    float m_weaponDx = 0.0f;
    float m_weaponDy = 0.0f;
    float m_weaponDz = 0.0f;
    // Defaults ON: VR hand tracking (mode 4 = full-arm IK) + decoupled VR weapon aim (bullet follows
    // controller). The F10 overlay checkboxes also default to true so the UI stays in sync.
    std::atomic<int> m_vrHandTrackingMode{4};
    std::atomic<int> m_weaponAimEnable{1};
    std::atomic<int> m_immersiveHolsters{1};   // 1 = visual-holster equip (default), 0 = simple slot mapping
    float* m_sharedHandsPtr = nullptr;   // cached shared-mem view (set in OnPresent) for GetSharedSlot

    // VR hand IK calibration, pushed from the overlay into shared-mem slots [33..47] each
    // present; the RED4ext plugin reads them when [33] (valid) is set, else keeps its own
    // baked defaults. [48] = one-shot "write diag" request the plugin clears after dumping.
    std::atomic<int>   m_calibValid{0};
    std::atomic<float> m_calib[14]{}; // scaleR,scaleL,heightR,heightL,swingR,swingL,poleR,poleL, wristR pyr(3), wristL pyr(3)

    // Extra calibration: anatomical HMD/body->shoulder offsets in body-frame OpenXR axes (right, up, back).
    // Layout: [0..2] right shoulder (rx, ry, rz), [3..5] left shoulder (lx, ly, lz).
    std::atomic<int>   m_calibExtValid{0};
    std::atomic<float> m_calibExt[6]{};

    // T-pose measured anatomy published to shared slots [77..80] each present: real arm length
    // per hand (m) + HMD eye height (m). The plugin scales the avatar arm bones to match
    // (gizmo-path), instead of the old position-scale hack. [80] = valid.
    std::atomic<int>   m_measureValid{0};
    std::atomic<float> m_userArmLenR{0.0f};
    std::atomic<float> m_userArmLenL{0.0f};
    std::atomic<float> m_userEyeHeight{0.0f};

    // Camera->head bake offset (game-local right/forward/up), applied by dxgi's LocateCamera.
    std::atomic<float> m_camBakeOffset[3]{};

    // Auto-calibration state machine:
    //   m_calibState  0 = idle, 1 = sampling, 2 = done
    //   m_calibSeconds = total target duration (typically 3.0)
    //   m_calibStart   = sim time when sampling began
    //   m_calibProgress = 0..1 fraction of the way through sampling
    //   sample stats (best armSpan + accumulators) tracked in cpp
    std::atomic<int>   m_calibState{0};
    std::atomic<float> m_calibSeconds{3.0f};
    std::atomic<float> m_calibProgress{0.0f};
    double             m_calibStart = 0.0;
    float              m_calibArmSpanMax = 0.0f;
    float              m_calibHmdHeightSum = 0.0f;
    int                m_calibSampleCount = 0;
    float              m_calibCtrlPosSumR[3] = {0,0,0};
    float              m_calibCtrlPosSumL[3] = {0,0,0};
    std::atomic<int>   m_logDiagReq{0};

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
    int m_interpolatedSyntheticEye = -1;
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
        ID3D12Resource* depthTexture = nullptr;
        bool textureShareable = false;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t format = 0;
        uint32_t depthWidth = 0;
        uint32_t depthHeight = 0;
        uint32_t depthFormat = 0;
        uint64_t serial = 0;
        uint64_t pairId = 0;
        uint64_t depthSerial = 0;
        uint64_t captureQpc = 0;  // QueryPerformanceCounter at capture
        // Value the optical-flow convert fence is signaled to once this eye's color
        // has been swizzled into opticalFlowTexture (fire-and-forget in OnPresent).
        // The frame thread GPU-Waits on this before the NvOF warp reads the texture.
        uint64_t opticalFlowConvertValue = 0;
        bool depthInCopySource = false;
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
    // [DEPTH-AERV2] Plain R32_FLOAT COLOR copy of the scene depth, resolved from
    // the game's typeless depth (the D32 snapshot is a depth-stencil resource that
    // CUDA cannot import). Shareable -> imported by the warp pipeline for
    // depth-aware occlusion. Freshest-snapshot model (like the mono depth path).
    ID3D12Resource* m_depthSnapshotR32 = nullptr;
    uint64_t m_depthSnapshotR32Serial = 0;
    uint32_t m_depthSnapshotW = 0;
    uint32_t m_depthSnapshotH = 0;
    uint64_t m_depthSnapshotSerial = 0; // serial of the color frame this depth matches; 0 = invalid/empty
    bool m_depthLayerSupported = false;  // runtime supports XR_KHR_composition_layer_depth and a depth swapchain format
    int64_t m_depthSwapchainFormat = 0;  // chosen runtime depth format (e.g. DXGI_FORMAT_D32_FLOAT)
    CapturedEyeFrame m_capturedEyeFrames[2];
    CapturedEyeFrame m_previousCapturedEyeFrames[2];
    CapturedEyeFrame m_pendingEyeFrames[2];
    std::unique_ptr<OpticalFlowD3D12> m_opticalFlow;
    // alternate-eye AER V2 path: CUDA interop + NvOF + warp kernel. The older
    // OpticalFlowD3D12 path stays as a fallback until the CUDA route is proven
    // end-to-end on hardware.
    std::unique_ptr<aer_v2::AerV2Pipeline> m_aerV2Pipeline;
    // Per-eye 100-sample online least-squares pose predictor for display-time
    // pose estimation in the AER V2 synth path.
    aer_v2::PosePredictor m_posePredictor[2];
    // Cached QueryPerformanceFrequency (constant per boot; avoids re-querying
    // every frame tick).
    LONGLONG m_qpcFreq = 0;
    // Phase 2b/2c: motion-vector forward warp + dedicated command machinery
    // for the AER V2 worker. The worker uses its own queue/alloc/list/fence so
    // it cannot race the present-thread submit path that uses m_d3dQueue.
    std::unique_ptr<MotionVectorWarp> m_mvWarp;
    Microsoft::WRL::ComPtr<ID3D12CommandQueue> m_mvWarpQueue;
    Microsoft::WRL::ComPtr<ID3D12CommandAllocator> m_mvWarpAlloc;
    Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList> m_mvWarpList;
    Microsoft::WRL::ComPtr<ID3D12Fence> m_mvWarpFence;
    HANDLE m_mvWarpEvent = nullptr;
    UINT64 m_mvWarpFenceValue = 0;
    // Per-eye scratch texture the warp pass writes into (output-res, eye fmt).
    Microsoft::WRL::ComPtr<ID3D12Resource> m_mvWarpedEye[2];
    // Tracks whether the warp scratch is currently in COMMON state (next use
    // transitions back from COMMON; otherwise it's still in RENDER_TARGET).
    bool m_mvWarpedEyeReady[2] = {false, false};

    // 64bpp depth-plane resolve. CP2077 scene depth is R32G8X24_TYPELESS in
    // most scenes; a shader pass converts plane 0 (depth) into a D32_FLOAT
    // texture compatible with the OpenXR depth swapchain. Without this, depth
    // submit only works in the small subset of scenes that happen to use
    // R32-family depth, and `depth=0` shows up in submit logs everywhere
    // else — which prevents compositor positional async-timewarp (the key to
    // alternate-eye smooth AER).
    std::unique_ptr<DepthResolve> m_depthResolve;

    // Depth-based stereo reprojection: synthesizes the OTHER eye from one
    // rendered eye + its depth. Used in AER submit so every pair contains
    // {actual_renderedEye, synthesized_otherEye} representing a SINGLE sim
    // timestamp — kills the "fps/2 per eye" symptom of alternate-eye AER
    // because each game frame produces a complete pair.
    std::unique_ptr<StereoReproject> m_stereoReproject;
    // CAS sharpen pass. Records onto the submit command list right where the eye
    // image is written to the swapchain when xr_sharpness > 0.
    std::unique_ptr<SharpenPass> m_sharpenPass;
    bool m_sharpenReady = false;

    // Reuse-last-frame path: persistent "last good" eye images + their pose/fov.
    // On a stale tick we re-present these with the stashed pose so the runtime
    // reprojects the last clean frame to the current head instead of warping a
    // stale eye.
    Microsoft::WRL::ComPtr<ID3D12Resource> m_lastGoodEye[2];
    bool m_lastGoodEyeInited[2] = {false, false};
    XrPosef m_lastGoodPose[2]{};
    XrFovf m_lastGoodFov[2]{};
    bool m_lastGoodValid = false;
    // Submit-thread scratch RT for the synthesized eye (output res, color fmt
    // matching swapchain).
    Microsoft::WRL::ComPtr<ID3D12Resource> m_stereoSynthEye;
    std::unique_ptr<ColorBlit> m_colorBlit;
    // alternate-eye NvOF synthetic midpoint output, one per eye. Kept separate
    // from m_mvWarpedEye (MV-only forward-warp experiment) so submit can choose
    // between full NvOF synth and experimental substitutes independently.
    Microsoft::WRL::ComPtr<ID3D12Resource> m_aerV2SynthEye[2][2];
    Microsoft::WRL::ComPtr<ID3D12Resource> m_aerV2SubmitEye[2][2];
    bool m_aerV2SubmitEyeReady[2][2] = {};
    uint32_t m_interpolatedSynthSlot = 0;
    // Half-rate AER V2 path: the temporally
    // interpolated IN-BETWEEN frame for BOTH eyes (mid(prevReal,currReal) at
    // blend 0.5). On the display interval that falls between two real pairs we
    // submit THIS instead of deferring to runtime SSW/ASW. The per-eye NvOF interpolation is
    // blend-factor driven; here the
    // single N=2 midpoint, blend=0.5). Double-buffered, CUDA writes
    // m_aerV2InBetween (kept COPY_SOURCE), submit copies via the *Submit scratch
    // so the two never share GPU state (mirrors the synth-eye path).
    Microsoft::WRL::ComPtr<ID3D12Resource> m_aerV2InBetween[2][2];
    Microsoft::WRL::ComPtr<ID3D12Resource> m_aerV2InBetweenSubmit[2][2];
    bool m_aerV2InBetweenSubmitReady[2][2] = {};
    uint64_t m_inBetweenReadyPairId = 0;     // newest pair (PAIR counter) whose in-between is GPU-ready
    uint32_t m_inBetweenSlot = 0;            // double-buffer slot of m_inBetweenReadyPairId
    int m_inBetweenSyntheticEye = -1;        // that eye's in-between half = m_aerV2SynthEye (reused, no 2nd warp); other eye = m_aerV2InBetween
    XrPosef m_inBetweenEyePoses[2]{};
    XrFovf m_inBetweenEyeFovs[2]{};
    bool m_inBetweenEyeViewsValid[2]{};
    uint64_t m_inBetweenShownForPairId = 0;  // frame-thread: last pair we displayed the in-between for
    // Engine present pacing: see openxr_manager.cpp OnPresent. The 1/2-rate
    // engine throttle that this field was meant to back is NOT implemented --
    // the current mode-6 path runs the engine at FULL HMD rate (alternate-eye),
    // not half-rate; the HMD-paced gate is m_frameSyncEvent (mono + AER V2). See
    // docs/aer-v2-mode6-corrected.md.
    std::atomic<int64_t> m_predictedDisplayPeriodNs{0};
    // Shareable copy of engine DLSS motion vectors for the CUDA/NvOF path.
    // Game-owned NGX resources are not guaranteed to be importable into CUDA,
    // so the worker copies MV into this shared NT-handle texture first.
    Microsoft::WRL::ComPtr<ID3D12Resource> m_aerV2MvScratch;

    // AER V2 frame-gen worker: runs ConvertToInputTexture + ExecuteFlow +
    // SynthesizeMidpoint OFF the OnPresent thread so the game's Present hook
    // is not blocked by our ~3.5ms/frame GPU CPU-wait stack. OnPresent only
    // builds a job (addref source pointers) and signals; the worker processes
    // it and atomically publishes m_interpolatedPairId when GPU work done.
    // Single-slot pending: if the worker is still busy when a new pair lands,
    // we drop the older queued job (release its refs) and queue the newer one
    // — this caps wall-clock latency to one synth interval.
    struct AERV2Job {
        uint64_t pairId = 0;
        // Pair counter (m_aerPairCounter / m_capturedEyeFrames.pairId space). The
        // submit thread keys the in-between cadence on THIS, not pairId (which is
        // the present counter s_presentCount); mixing the two disabled the
        // in-between entirely (realPair pair-counter never == ready present-counter).
        uint64_t submitPairId = 0;
        int renderedEye = -1;
        int syntheticEye = -1;
        ID3D12Resource* flowPrevSource[2] = {nullptr, nullptr};
        ID3D12Resource* flowCurrSource[2] = {nullptr, nullptr};
        ID3D12Resource* flowPrev[2] = {nullptr, nullptr};
        ID3D12Resource* flowCurr[2] = {nullptr, nullptr};
        ID3D12Resource* prevDepth[2] = {nullptr, nullptr};
        ID3D12Resource* currDepth[2] = {nullptr, nullptr};
        XrPosef prevPose[2]{};
        XrPosef currPose[2]{};
        XrFovf fov[2]{};
        bool hasView[2] = {false, false};
        // ── Engine-side ground-truth resources captured from Streamline's
        // slSetTag (see ngx_hook.cpp). MotionVectors describe the per-pixel
        // velocity the engine itself computed for DLSS this frame — used by
        // Phase 2b/2c forward warp to produce a "stale eye advanced in time"
        // image so both submitted eyes share a single simulation timestamp.
        // Engine depth (R32_TYPELESS at render res) supports occlusion-edge
        // handling in the warp shader and as a depth-layer fallback when our
        // OM-hook scene depth catches a non-compatible format.
        // Pointers are AddRef'd into the job (worker Releases on completion).
        ID3D12Resource* engineMv = nullptr;
        ID3D12Resource* engineDepth = nullptr;
        // Serial of each current-pair eye capture. Used by worker to identify
        // which eye in the pair was captured FIRST (lower serial = older =
        // the one to warp forward by 1 frame of MV).
        uint64_t currSerial[2] = {0, 0};
    };

    // Phase 2d publication: per-eye atomic flag set by the AER V2 worker
    // after successfully MV-warping the older eye of the pair. The submit
    // thread reads these and substitutes m_mvWarpedEye[eye] for the raw
    // captured eye whenever pairId matches. Independent of m_interpolatedPairId
    // so MV-warp can ship without depending on the NvOF synth path landing.
    std::atomic<uint64_t> m_mvWarpedValidPairId[2] = {{0}, {0}};
    std::thread m_aerV2WorkerThread;
    std::mutex m_aerV2JobMutex;
    std::condition_variable m_aerV2JobCv;
    std::unique_ptr<AERV2Job> m_aerV2PendingJob;
    std::atomic<bool> m_aerV2WorkerShutdown{false};
    std::atomic<uint64_t> m_aerV2BusyPairId{0};   // pair the worker is currently processing
    std::atomic<uint64_t> m_aerV2DonePairId{0};   // last pair the worker fully completed (matches m_interpolatedPairId once published)

    void StartAERV2WorkerIfNeeded();
    void StopAERV2Worker();
    void AERV2WorkerThreadMain();
    void ProcessAERV2Job(std::unique_ptr<AERV2Job> job);
    void ReleaseAERV2JobRefs(AERV2Job& job);

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
    std::atomic<bool> m_runtimeIsVirtualDesktop = false;
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
