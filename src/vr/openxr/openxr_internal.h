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

// The central logger is defined in vr_core.cpp; every TU forward-declares it.
extern void Log(const char* fmt, ...);

#include "ngx_hook.h"
#include "../../common/log_throttle.h"

// Shared free-function / global decls used across the split OpenXR TUs.
// Definitions live in openxr_manager.cpp or vr_core.cpp; these mirror the
// forward-decls that the monolith kept at its top.
extern volatile int g_verboseLog;   // gate per-frame spam

// Config getters (definitions in vr_core.cpp / openxr_manager.cpp). Mirrors the
// forward-decl block the monolith kept at its top so every split TU can call them.
extern "C" int GetDisableRoll();
extern "C" float GetForcedFov();
extern "C" float GetGameRenderFovDeg();
extern "C" float GetGameRenderVerticalFovDeg();
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
extern "C" float GetMotionPredictMs();
extern "C" int GetRenderPoseSubmit();
extern "C" int GetDepthSubmit();
extern "C" int GetPoseLag();
extern "C" int GetRenderedCameraEye();
extern "C" uint32_t GetRenderedCameraSeq();
extern "C" int GetXrRuntimeMode();
// Tunable accessors (defined in openxr_manager.cpp; read the g_* atomics above)
extern "C" float GetPoseBlend();
extern "C" float GetFlowSmooth();
extern "C" float GetHmdTrackingSmooth();
extern "C" float GetHandTrackingSmooth();
extern "C" int GetInputActionsEnabled();
extern "C" int GetMonoXQueueWait();
extern "C" int GetMonoDepthCapture();
// GPU cross-queue barrier helper (defined in swapchain_hooks.cpp).
extern "C" void CyberpunkVRPort_WaitOnAllGameSignals(ID3D12CommandQueue* consumerQueue);

// Small shared helper (was a file-scope static in openxr_manager.cpp).
inline bool ReuseLastFrameOutputEnabled() { return GetReuseLastFrameOutput() != 0; }

// ---- runtime tunables -----------------------------------------------------------
// Lightweight atomics (g_verboseLog pattern). PERSISTED to vrport.ini: the ini reader
// pushes parsed values in via the Set* accessors, Save reads them back out via Get*.
inline std::atomic<float> g_hmdTrackingSmooth{0.35f};
inline std::atomic<float> g_handTrackingSmooth{0.45f};

// ---- tiny D3D helpers ---------------------------------------------------------

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

                // DESCRIBE WHAT WAS RENDERED, NOT WHAT THE PANEL IS.
                //
                // A projection view is a promise that this rectangle contains exactly this frustum.
                // The engine renders a SYMMETRIC frustum about the camera axis on both axes, so the
                // honest submit is symmetric on both. Passing the runtime's raw asymmetric vertical
                // through instead was a 1.8% lie: measured, the engine rendered V 100.02 while the
                // submit claimed the runtime's 99.0, and the compositor squashes the difference.
                //
                // R.E.A.L. VR keeps the runtime's +50/-49 and CROPS the rectangle to match -- its
                // eye rect is 3128 of a 3184-tall target, and 1.19203 - (3128/3184)*2.38406 =
                // -1.15009, i.e. exactly -48.99 deg. Same contract, satisfied from the other end.
                // Submitting the rendered frustum is the same result with no crop: the degree of
                // extra vertical simply falls outside the panel and is never shown.
                const float gameVFovDeg = GetGameRenderVerticalFovDeg();
                const float realHalfV = (fov.angleUp - fov.angleDown) * 0.5f;
                const float vCenter = (fov.angleUp + fov.angleDown) * 0.5f;
                const bool  haveRenderedV = (gameVFovDeg > 1.0f && gameVFovDeg < 179.0f);
                const float wantHalfV = haveRenderedV
                    ? (gameVFovDeg * 3.1415926535f / 180.0f) * 0.5f
                    : realHalfV;

                fov.angleLeft  = hCenter - wantHalfH;
                fov.angleRight = hCenter + wantHalfH;
                // Symmetric about the camera axis when the rendered vertical is known; the runtime's
                // own centre only as a fallback before the FOV hook has fired.
                fov.angleUp    = (haveRenderedV ? 0.0f : vCenter) + wantHalfV;
                fov.angleDown  = (haveRenderedV ? 0.0f : vCenter) - wantHalfV;
                
                // Was every 200th call, which still came to 169 lines a session saying the same
                // four numbers. The numbers only move when the FOV or the zoom does, so a 30 s
                // heartbeat loses nothing; DEBUG keeps the old cadence.
                static uint32_t s_sfLogN = 0;
                if (g_verboseLog ? ((s_sfLogN++ % 200) == 0) : true) {
                    const float hfovDeg = (fov.angleRight - fov.angleLeft) * (180.0f / 3.1415926535f);
                    const float vfovDeg = (fov.angleUp - fov.angleDown) * (180.0f / 3.1415926535f);
                    LOG_THROTTLED(30000, "OpenXRManager[SUBMITFOV]: eye=%d gameFov=%.2f -> submitHFov=%.2f submitVFov=%.2f (lensHFov=%.2f lensVFov=%.2f decantYaw=%d)\n",
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

// ---------------------------------------------------------------------------
// Body-tracker shared helpers (used by BOTH tracker providers: the
// XR_HTCX_vive_tracker_interaction path in openxr_vive_trackers.cpp and the
// OpenVR fallback path in openvr_trackers.cpp).
// ---------------------------------------------------------------------------

// Steady clock in seconds (QPC), for the re-enumerate / remap timers.
inline double FbtNowSeconds() {
    LARGE_INTEGER c{}, f{};
    QueryPerformanceCounter(&c);
    QueryPerformanceFrequency(&f);
    return (f.QuadPart > 0) ? static_cast<double>(c.QuadPart) / static_cast<double>(f.QuadPart) : 0.0;
}

// Same adaptive jitter filter the hand locate uses (openxr_frameloop.cpp):
// freeze sub-mm lighthouse/IMU noise, release to 1:1 once the tracker really
// moves. Templated: the state type is a private member struct of OpenXRManager.
template <typename FilterState>
XrPosef FilterTrackerPose(FilterState& state, const XrPosef& raw, float strength) {
    if (!state.initialized || strength <= 0.001f) {
        state.initialized = true;
        state.position = raw.position;
        state.orientation = raw.orientation;
        return raw;
    }
    const float dx = raw.position.x - state.position.x;
    const float dy = raw.position.y - state.position.y;
    const float dz = raw.position.z - state.position.z;
    const float posDelta = sqrtf(dx * dx + dy * dy + dz * dz);
    float dot = state.orientation.x * raw.orientation.x + state.orientation.y * raw.orientation.y
              + state.orientation.z * raw.orientation.z + state.orientation.w * raw.orientation.w;
    if (dot < 0.0f) dot = -dot;
    if (dot > 1.0f) dot = 1.0f;
    const float angDelta = 2.0f * acosf(dot);

    auto follow = [&](float delta, float quiet, float release) {
        if (release <= quiet) return 1.0f;
        const float stillFollow = 1.0f / (1.0f + 20.0f * strength);
        float m = (delta - quiet) / (release - quiet);
        if (m < 0.0f) m = 0.0f; if (m > 1.0f) m = 1.0f;
        m = m * m * (3.0f - 2.0f * m);
        return stillFollow + (1.0f - stillFollow) * m;
    };
    const float posT = follow(posDelta, 0.0018f, 0.0120f);
    const float angT = follow(angDelta, 0.0050f, 0.0450f);

    state.position.x += dx * posT;
    state.position.y += dy * posT;
    state.position.z += dz * posT;
    state.orientation = NlerpQuat(state.orientation, raw.orientation, angT);

    XrPosef out = raw;
    out.position = state.position;
    out.orientation = state.orientation;
    return out;
}
