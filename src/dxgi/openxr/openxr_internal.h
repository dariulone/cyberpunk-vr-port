#pragma once
// Private implementation header for the OpenXR module: openxr_manager.cpp and any
// method-group translation units split out of it. Holds shared file-scope state and
// helpers as `inline` so there is exactly ONE instance/definition across all those
// TUs (no silent state divergence, no linkage clash).
//
// NOT a public API header. Include it only from the openxr_*.cpp implementation
// files, and only AFTER "openxr_manager.h" (some helpers use manager-level free
// functions declared there).
#include <atomic>
#include <cstdint>
#include <cstdarg>
#include <cstdio>
#include <d3d12.h>
#include <vector>
#include "openxr_math.h"              // XR types + quaternion math (used by the cant helpers)
#include "runtime_fov_correction.h"   // ComputeRuntimeFovCorrection for cant/FOV helpers

// The central logger is defined in dxgi_proxy.cpp; every TU forward-declares it.
extern void Log(const char* fmt, ...);

#include "ngx_hook.h"   // NgxGetMvScaleX/Y used by the AER V2 warp path

// Shared free-function / global decls used across the split OpenXR TUs.
// Definitions live in openxr_manager.cpp or dxgi_proxy.cpp; these mirror the
// forward-decls that the monolith kept at its top.
extern volatile int g_verboseLog;   // gate per-frame spam

// Config getters (definitions in dxgi_proxy.cpp / openxr_manager.cpp). Mirrors the
// forward-decl block the monolith kept at its top so every split TU can call them.
extern "C" int GetDisableRoll();
extern "C" float GetForcedFov();
extern "C" float GetGameRenderFovDeg();
extern "C" float GetTargetRenderVfovDegC();
extern "C" float GetMenuFov();
extern "C" float GetMenuFollowDeg();
extern "C" int GetMenuRectMode();
extern "C" int GetMenuMode();
extern "C" int GetSyncSequential();
extern "C" int Get3DofMovement();
extern "C" float GetVrSharpness();
extern "C" float GetVrSharpmix();
extern "C" int GetReuseLastFrameOutput();
extern "C" int GetVrPairLock();
extern "C" int GetAERPairGate();
extern "C" int GetAERStartEye();
extern "C" int GetAERDebugEye();
extern "C" int GetAERWarmupFrames();
extern "C" float GetMotionPredictMs();
extern "C" int GetRenderPoseSubmit();
extern "C" int GetDepthSubmit();
extern "C" int GetPoseLag();
extern "C" int GetRenderedCameraEye();
extern "C" uint32_t GetRenderedCameraSeq();
extern "C" int GetAERHalfRate();
extern "C" int GetAERV2Enabled();
extern "C" int GetXrRuntimeMode();
// AER tunable accessors (defined in openxr_manager.cpp; read the g_* atomics above)
extern "C" float GetAerMaxExtrap();
extern "C" float GetAerRefineStrength();
extern "C" float GetAerOcclusionSharp();
extern "C" float GetAerFoveation();
extern "C" float GetPoseBlend();
extern "C" float GetFlowSmooth();
extern "C" float GetHmdTrackingSmooth();
extern "C" float GetHandTrackingSmooth();
extern "C" int GetInputActionsEnabled();
extern "C" int GetMonoXQueueWait();
extern "C" int GetMonoDepthCapture();
extern "C" int GetAerXQueueWait();
// GPU cross-queue barrier helper (defined in dxgi_factory_wrapper.cpp).
extern "C" void CyberpunkVRPort_WaitOnAllGameSignals(ID3D12CommandQueue* consumerQueue);

// Small shared helper (was a file-scope static in openxr_manager.cpp).
inline bool ReuseLastFrameOutputEnabled() { return GetReuseLastFrameOutput() != 0; }

// ---- AER runtime tunables ------------------------------------------------------
// Lightweight atomics (g_verboseLog pattern). PERSISTED to vrport.ini by the
// dxgi proxy (it reads the Get* accessors on Save and pushes parsed values back
// via Set* on load). The overlay reads/writes them via Get/Set; the frame thread
// + warp kernel read them each frame.
inline std::atomic<float> g_aerMaxExtrap{1.8f};   // forward-extrapolation cap (smear↔frozen on turns)
// Idea #4 — quality boost defaults. The kernel's agreement-gates on MV + pose
// flow make a MODERATE refine factor stable enough on CP2077 while noticeably
// sharpening the stale eye vs raw NvOF-only. Keep it conservative: the user can
// still dial it down to 0 in the overlay/ini if a scene exposes artifacts.
inline std::atomic<float> g_aerRefineStrength{0.35f}; // 0..1 MV+pose refine
inline std::atomic<float> g_aerOcclusionSharp{1.15f}; // >1 slightly crisps occlusion edges
// Fixed-foveation (no eye-tracking): fraction of the lens-edge radius that uses
// a CHEAP warp (deep periphery = raw nearest frame, no bidirectional/refine).
// 0 = off (full-quality everywhere). 0.4 = outer 40% of the radius simplified.
inline std::atomic<float> g_aerFoveation{0.0f};
// Idea #2 — synced-pose freshness. syncSequential(default on) freezes the head
// pose per stereo pair so both eyes render coherently (no per-eye tear), but
// that adds up to ~1 pair (2 vsyncs) of pose lag on head turns. g_poseBlend is
// a per-present low-pass nudge of the synced pose toward the live pose: 0.0 =
// fully frozen (current behavior, max coherence), 1.0 = tracks live every
// present (min lag, ~mono smoothness, slight per-eye delta). ~0.35 is a good
// middle ground: cuts head-turn pose lag noticeably while keeping stereo
// coherence. Applied EVERY present (the full snap at the pair boundary still
// runs as the hard reset).
inline std::atomic<float> g_poseBlend{0.35f};
// Idea #3 — NvOF flow temporal smoothing. Stale-eye warp quality on motion is
// limited by per-frame NvOF flow noise. g_flowSmooth EMA-blends each eye's flow
// toward its previous-frame flow: 0.0 = off (raw NvOF every frame), up to ~0.6
// (heavier smoothing, stabler but can trail on fast cuts). Reduces the
// stale-eye "shimmer" on textured/animated surfaces.
inline std::atomic<float> g_flowSmooth{0.35f};
inline std::atomic<float> g_hmdTrackingSmooth{0.35f};
inline std::atomic<float> g_handTrackingSmooth{0.45f};

// ---- AER V2 shared constants + tiny D3D helpers -------------------------------
inline constexpr uint64_t kAERV2FlowWarmupPairId = 300;
inline constexpr float kAERV2FrameGenPoseT = 0.5f;

inline DXGI_FORMAT GetAERV2OpticalFlowFormat(DXGI_FORMAT sourceFormat) {
    switch (sourceFormat) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
        return DXGI_FORMAT_B8G8R8A8_UNORM;
    default:
        return sourceFormat;
    }
}

inline void SetD3DName(ID3D12Object* object, const wchar_t* name) {
    if (object && name) {
        object->SetName(name);
    }
}

inline void SetD3DNamef(ID3D12Object* object, const wchar_t* format, ...) {
    if (!object || !format) {
        return;
    }
    wchar_t name[192]{};
    va_list args;
    va_start(args, format);
    _vsnwprintf_s(name, sizeof(name) / sizeof(name[0]), _TRUNCATE, format, args);
    va_end(args);
    object->SetName(name);
}

// ---- FOV / display-cant helpers (were file-scope static in openxr_manager.cpp) ----
inline XrFovf ApplyForcedProjectionFov(const XrFovf& sourceFov, const XrFovf* pairFovs, int eyeIndex, float width, float height) {
    float forceFov = GetForcedFov();
    if (forceFov <= 1.0f || forceFov >= 170.0f) {
        if (pairFovs && eyeIndex >= 0 && eyeIndex <= 1) {
            const RuntimeFovCorrection corr = ComputeRuntimeFovCorrection(pairFovs[0], pairFovs[1]);
            XrFovf fov = corr.eye[eyeIndex];
            
            const float gameFovDeg = GetGameRenderFovDeg();
            if (gameFovDeg > 1.0f) {
                const float wantHalfH = (gameFovDeg * 3.1415926535f / 180.0f) * 0.5f;
                const float hCenter = (fov.angleRight + fov.angleLeft) * 0.5f;
                
                // FIX: Usa il lensVFov REALE del runtime, NON ricalcolarlo dall'aspect
                const float realHalfV = (fov.angleUp - fov.angleDown) * 0.5f;
                const float vCenter = (fov.angleUp + fov.angleDown) * 0.5f;
                
                fov.angleLeft  = hCenter - wantHalfH;
                fov.angleRight = hCenter + wantHalfH;
                fov.angleUp    = vCenter + realHalfV;  // ← USA lensVFov reale
                fov.angleDown  = vCenter - realHalfV;  // ← USA lensVFov reale
                
                static uint32_t s_sfLogN = 0;
                if (g_verboseLog || (s_sfLogN++ % 200) == 0) {
                    const float hfovDeg = (fov.angleRight - fov.angleLeft) * (180.0f / 3.1415926535f);
                    const float vfovDeg = (fov.angleUp - fov.angleDown) * (180.0f / 3.1415926535f);
                    Log("OpenXRManager[SUBMITFOV]: eye=%d gameFov=%.2f -> submitHFov=%.2f submitVFov=%.2f (lensHFov=%.2f lensVFov=%.2f decantYaw=%d)\n",
                        eyeIndex, gameFovDeg, hfovDeg, vfovDeg,
                        (sourceFov.angleRight - sourceFov.angleLeft) * (180.0f / 3.1415926535f),
                        (sourceFov.angleUp - sourceFov.angleDown) * (180.0f / 3.1415926535f),
                        corr.yawEnabled ? 1 : 0);
                }
                return fov;
            }
            return fov;
        }
        return sourceFov;
    }

    // Custom user override (xr_force_fov): keep the historical "forced symmetric
    // projection" behavior, but derive vertical FOV from the ACTUAL render aspect,
    // not a broken hardcoded 1.0. This matches the UI text: it changes only the
    // OpenXR projection layer FOV, not the CP2077 camera FOV.
    float aspect = (height > 1.0f) ? (width / height) : 0.0f;
    if (!(aspect > 0.01f && aspect < 10.0f)) {
        const float tanLeft = std::tanf(-sourceFov.angleLeft);
        const float tanRight = std::tanf(sourceFov.angleRight);
        const float tanDown = std::tanf(-sourceFov.angleDown);
        const float tanUp = std::tanf(sourceFov.angleUp);
        const float v = tanUp + tanDown;
        aspect = (v > 1.0e-5f) ? ((tanLeft + tanRight) / v) : 1.0f;
    }

    const float halfFovH = (forceFov * 3.1415926535f / 180.0f) * 0.5f;
    const float halfFovV = atanf(tanf(halfFovH) / aspect);
    XrFovf fov{};
    fov.angleLeft = -halfFovH;
    fov.angleRight = halfFovH;
    fov.angleDown = -halfFovV;
    fov.angleUp = halfFovV;
    return fov;
}

// ---- Display-cant SUBMIT pose (pairs with the RENDER-side cant in dxgi_proxy
// OnFinalCameraCallback). The render camera of each AER eye is rotated by its
// frustum-center cant so the de-canted symmetric FOV content lands on the canted
// lens; the SUBMIT pose must carry the SAME cant so the compositor's reprojection
// agrees with the rendered content (otherwise the canted content is reprojected as
// if un-canted -> mismatch). Render + submit are both canted with a symmetric FOV.
// ONLY for AER (per-eye render); mono renders ONE frame so a per-eye
// cant can't be baked into its content -> mono stays un-canted (compositor handles
// the lens). No-op on symmetric HMDs (Pico).
inline XrQuaternionf ComputeCantPoseDelta(const XrFovf* pairFovs, int eye) {
    const XrQuaternionf identity{0.0f, 0.0f, 0.0f, 1.0f};
    if (!pairFovs || eye < 0 || eye > 1) return identity;
    const RuntimeFovCorrection corr = ComputeRuntimeFovCorrection(pairFovs[0], pairFovs[1]);
    if (!corr.yawEnabled && !corr.pitchEnabled) return identity;
    const float yaw   = corr.yawEnabled   ? (eye == 0 ? corr.yawDeltaRad : -corr.yawDeltaRad) : 0.0f;
    const float pitch = corr.pitchEnabled ? corr.pitchDeltaRad : 0.0f;
    
    const XrQuaternionf qYaw{0.0f, sinf(yaw * 0.5f), 0.0f, cosf(yaw * 0.5f)};       // about +Y (up)
    const XrQuaternionf qPitch{sinf(pitch * 0.5f), 0.0f, 0.0f, cosf(pitch * 0.5f)}; // about +X (right)
    return MultiplyQuat(qYaw, qPitch);
}

inline void ApplyCantToPose(XrPosef& pose, const XrFovf* pairFovs, int eye) {
    const XrQuaternionf d = ComputeCantPoseDelta(pairFovs, eye);
    if (d.x == 0.0f && d.y == 0.0f && d.z == 0.0f && d.w == 1.0f) return; // no-op (symmetric)
    XrQuaternionf o = MultiplyQuat(pose.orientation, d); // local (post-multiply)
    const float n = sqrtf(o.x * o.x + o.y * o.y + o.z * o.z + o.w * o.w);
    if (n > 1e-8f) { const float inv = 1.0f / n; o.x *= inv; o.y *= inv; o.z *= inv; o.w *= inv; }
    pose.orientation = o;
}

// ---- mono swapchain format pick (were file-scope static in openxr_manager.cpp) ----
inline bool ContainsSwapchainFormat(const std::vector<int64_t>& formats, int64_t candidate) {
    for (const int64_t format : formats) {
        if (format == candidate) {
            return true;
        }
    }
    return false;
}

inline int64_t PickMonoSwapchainFormat(const std::vector<int64_t>& runtimeFormats, int64_t gameFormat, bool preferSrgbForVD) {
    // VirtualDesktopXR honors the swapchain format strictly: a UNORM swapchain
    // is treated as linear data, so the compositor applies an extra sRGB
    // encode → washed-out / overbright look that the user reported. SteamVR
    // historically treats UNORM as already-sRGB display data and doesn't apply
    // the extra encode, which is why colors look "normal" there. CP2077's
    // backbuffer is already tonemapped sRGB-encoded bytes despite being typed
    // R8G8B8A8_UNORM, so a UNORM_SRGB swapchain views the same bits as sRGB
    // and the runtime skips the redundant encode. Direction confirmed by an
    // external VR modder consulted by the user.
    if (preferSrgbForVD) {
        if (gameFormat == static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM) &&
            ContainsSwapchainFormat(runtimeFormats, static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB))) {
            return static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB);
        }
        if (gameFormat == static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM) &&
            ContainsSwapchainFormat(runtimeFormats, static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB))) {
            return static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB);
        }
    }
    if (ContainsSwapchainFormat(runtimeFormats, gameFormat)) {
        return gameFormat;
    }

    // Prefer bit-compatible sRGB companions before falling back to unrelated
    // formats. SteamVR commonly advertises R8G8B8A8_UNORM_SRGB (29) but not
    // R8G8B8A8_UNORM (28); picking 16-bit float there caused a blank HMD
    // because our submit path is a straight resource copy, not a format-convert
    // blit.
    if (gameFormat == static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM) &&
        ContainsSwapchainFormat(runtimeFormats, static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB))) {
        return static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB);
    }
    if (gameFormat == static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM) &&
        ContainsSwapchainFormat(runtimeFormats, static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB))) {
        return static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB);
    }

    const int64_t preferredFormats[] = {
        static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM),
        static_cast<int64_t>(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB),
        static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM),
        static_cast<int64_t>(DXGI_FORMAT_B8G8R8A8_UNORM_SRGB),
        static_cast<int64_t>(DXGI_FORMAT_R16G16B16A16_FLOAT)
    };
    for (const int64_t preferred : preferredFormats) {
        if (ContainsSwapchainFormat(runtimeFormats, preferred)) {
            return preferred;
        }
    }

    return runtimeFormats.empty() ? gameFormat : runtimeFormats[0];
}
