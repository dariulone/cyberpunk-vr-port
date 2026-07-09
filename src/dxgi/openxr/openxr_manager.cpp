#include "openxr_manager.h"
#include "shared_slots.h"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include "ngx_hook.h"
#include "runtime_fov_correction.h"
#include "openxr_math.h"   // extracted pure quaternion/vector math (inline)
#include "openxr_internal.h"   // shared inline statics/helpers for the split OpenXR TUs
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <vector>
#include <dxgi1_4.h>
#include <cstring>
#include <cmath>
#include <utility>
#include <chrono>

static void EulerToQuat(float pitchDeg, float yawDeg, float rollDeg, float& qx, float& qy, float& qz, float& qw) {
    float p = pitchDeg * (3.1415926535f / 180.0f) * 0.5f;
    float y = yawDeg * (3.1415926535f / 180.0f) * 0.5f;
    float r = rollDeg * (3.1415926535f / 180.0f) * 0.5f;
    
    float cp = cosf(p), sp = sinf(p);
    float cy = cosf(y), sy = sinf(y);
    float cr = cosf(r), sr = sinf(r);
    
    qw = cr * cp * cy + sr * sp * sy;
    qx = sr * cp * cy - cr * sp * sy;
    qy = cr * sp * cy + sr * cp * sy;
    qz = cr * cp * sy - sr * sp * cy;
}

static void MulQuatLoc(float ax, float ay, float az, float aw, float bx, float by, float bz, float bw,
    float& outX, float& outY, float& outZ, float& outW) {
    outX = ax * bw + aw * bx + ay * bz - az * by;
    outY = ay * bw + aw * by + az * bx - ax * bz;
    outZ = az * bw + aw * bz + ax * by - ay * bx;
    outW = aw * bw - ax * bx - ay * by - az * bz;
}

extern void Log(const char* fmt, ...);
extern volatile int g_verboseLog; // gate per-frame hand-tracking spam
extern "C" int GetDisableRoll();
extern "C" float GetForcedFov();
extern "C" float GetGameRenderFovDeg(); // FOV (deg) the game actually renders with (native or forced); 0 if unknown
extern "C" float GetTargetRenderVfovDegC(); // overscanned vertical FOV (deg) the game renders = lens*overscan; 0 if unknown
extern "C" float GetMenuFov();
extern "C" float GetMenuFollowDeg(); // head-vs-panel yaw offset (deg) that starts the lazy menu re-center
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

// ── AER V2 warp tuning knobs (runtime-adjustable from the F10 overlay) ──
// [AER runtime tunables moved to openxr_internal.h (inline). The extern "C"
//  Get*/Set* accessors below still resolve them by name via that header.]
extern "C" float GetAerMaxExtrap()        { return g_aerMaxExtrap.load(std::memory_order_relaxed); }
extern "C" void  SetAerMaxExtrap(float v) { g_aerMaxExtrap.store(v, std::memory_order_relaxed); }
extern "C" float GetAerRefineStrength()        { return g_aerRefineStrength.load(std::memory_order_relaxed); }
extern "C" void  SetAerRefineStrength(float v) { g_aerRefineStrength.store(v, std::memory_order_relaxed); }
extern "C" float GetAerOcclusionSharp()        { return g_aerOcclusionSharp.load(std::memory_order_relaxed); }
extern "C" void  SetAerOcclusionSharp(float v) { g_aerOcclusionSharp.store(v, std::memory_order_relaxed); }
extern "C" float GetAerFoveation()        { return g_aerFoveation.load(std::memory_order_relaxed); }
extern "C" void  SetAerFoveation(float v) { g_aerFoveation.store(v, std::memory_order_relaxed); }
extern "C" float GetPoseBlend()        { return g_poseBlend.load(std::memory_order_relaxed); }
extern "C" void  SetPoseBlend(float v) { g_poseBlend.store(v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v), std::memory_order_relaxed); }
extern "C" float GetFlowSmooth()        { return g_flowSmooth.load(std::memory_order_relaxed); }
extern "C" void  SetFlowSmooth(float v) { g_flowSmooth.store(v < 0.0f ? 0.0f : (v > 0.9f ? 0.9f : v), std::memory_order_relaxed); }
extern "C" float GetHmdTrackingSmooth()        { return g_hmdTrackingSmooth.load(std::memory_order_relaxed); }
extern "C" void  SetHmdTrackingSmooth(float v) { g_hmdTrackingSmooth.store(v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v), std::memory_order_relaxed); }
extern "C" float GetHandTrackingSmooth()         { return g_handTrackingSmooth.load(std::memory_order_relaxed); }
extern "C" void  SetHandTrackingSmooth(float v)  { g_handTrackingSmooth.store(v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v), std::memory_order_relaxed); }
extern "C" int GetInputActionsEnabled(); // 0 = pose-only legacy behaviour, 1 = full gameplay action set
extern "C" int GetMonoXQueueWait();      // 0 = mono path skips cross-queue Wait (kills hang); 1 = legacy depth-safe behaviour
extern "C" int GetMonoDepthCapture();    // 0 = mono path skips depth capture entirely (kills CP2077 mono hang); 1 = legacy depth-aware reprojection
extern "C" int GetAerXQueueWait();       // 0 = AER path skips cross-queue Wait + depth capture (smooth, NvOF-only warp); 1 = legacy depth-safe behaviour
extern "C" float GetHmdTrackingSmooth();
extern "C" float GetHandTrackingSmooth();
// Implemented in dxgi_factory_wrapper.cpp. Issues GPU-side ID3D12CommandQueue::
// Wait() on the consumer queue for every tracked game queue's latest Signal —
// so a subsequent CopyResource on that consumer queue cannot race the game's
// render-side writer. No CPU stall. See xr_depth_submit cross-queue notes.
extern "C" void CyberpunkVRPort_WaitOnAllGameSignals(ID3D12CommandQueue* consumerQueue);

// [kAERV2* constants + GetAERV2OpticalFlowFormat / SetD3DName / SetD3DNamef
//  moved to openxr_internal.h (inline)]

static const char* ClassifyOpenXRRuntime(const char* runtimeName) {
    if (!runtimeName || !runtimeName[0]) return "Unknown";
    if (strstr(runtimeName, "SteamVR") != nullptr) return "SteamVR";
    if (strstr(runtimeName, "VirtualDesktop") != nullptr || strstr(runtimeName, "Virtual Desktop") != nullptr) return "Virtual Desktop";
    if (strstr(runtimeName, "Oculus") != nullptr || strstr(runtimeName, "Meta") != nullptr) return "Meta/Oculus";
    if (strstr(runtimeName, "Windows Mixed Reality") != nullptr || strstr(runtimeName, "Mixed Reality") != nullptr) return "Windows Mixed Reality";
    if (strstr(runtimeName, "OpenComposite") != nullptr) return "OpenComposite";
    return "OpenXR";
}

static bool FileExistsA(const char* path) {
    if (!path || !path[0]) {
        return false;
    }
    const DWORD attrs = GetFileAttributesA(path);
    return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) == 0;
}

static void TrimTrailingSlashes(char* path) {
    if (!path) {
        return;
    }
    size_t len = strlen(path);
    while (len > 0 && (path[len - 1] == '\\' || path[len - 1] == '/')) {
        path[len - 1] = '\0';
        --len;
    }
}

static bool JoinPath(char* out, size_t outSize, const char* base, const char* suffix) {
    if (!out || outSize == 0 || !base || !base[0] || !suffix || !suffix[0]) {
        return false;
    }
    if (strcpy_s(out, outSize, base) != 0) {
        return false;
    }
    TrimTrailingSlashes(out);
    if (strcat_s(out, outSize, "\\") != 0) {
        return false;
    }
    return strcat_s(out, outSize, suffix) == 0;
}

static bool TryReadRegistryString(HKEY root, const char* subKey, const char* valueName, char* out, DWORD outBytes) {
    if (!out || outBytes < 2) {
        return false;
    }
    DWORD type = 0;
    DWORD size = outBytes;
    const LONG status = RegGetValueA(root, subKey, valueName, RRF_RT_REG_SZ, &type, out, &size);
    if (status != ERROR_SUCCESS || type != REG_SZ || out[0] == '\0') {
        return false;
    }
    out[outBytes - 1] = '\0';
    return true;
}

static bool TryGetSteamVRRuntimeJsonFromOpenVR(char* outJsonPath, size_t outJsonPathSize) {
    if (!outJsonPath || outJsonPathSize == 0) {
        return false;
    }

    HMODULE openvrModule = nullptr;
    char gameDir[MAX_PATH]{};
    if (GetModuleFileNameA(nullptr, gameDir, MAX_PATH) > 0) {
        char* lastSlash = strrchr(gameDir, '\\');
        if (lastSlash) {
            *lastSlash = '\0';
            char localOpenVrPath[MAX_PATH]{};
            if (JoinPath(localOpenVrPath, sizeof(localOpenVrPath), gameDir, "openvr_api.dll") && FileExistsA(localOpenVrPath)) {
                openvrModule = LoadLibraryA(localOpenVrPath);
            }
        }
    }
    if (!openvrModule) {
        openvrModule = LoadLibraryA("openvr_api.dll");
    }
    if (!openvrModule) {
        Log("OpenXRManager: SteamVR runtime request could not load openvr_api.dll. Falling back to registry lookup.\n");
        return false;
    }

    using VR_GetRuntimePathFn = bool(*)(char*, uint32_t, int*);
    auto getRuntimePath = reinterpret_cast<VR_GetRuntimePathFn>(GetProcAddress(openvrModule, "VR_GetRuntimePath"));
    if (!getRuntimePath) {
        Log("OpenXRManager: openvr_api.dll loaded but VR_GetRuntimePath export is missing.\n");
        FreeLibrary(openvrModule);
        return false;
    }

    char runtimeRoot[2048]{};
    int openVrError = 0;
    const bool ok = getRuntimePath(runtimeRoot, static_cast<uint32_t>(sizeof(runtimeRoot)), &openVrError);
    FreeLibrary(openvrModule);
    if (!ok || !runtimeRoot[0]) {
        Log("OpenXRManager: VR_GetRuntimePath failed (error=%d).\n", openVrError);
        return false;
    }

    if (!JoinPath(outJsonPath, outJsonPathSize, runtimeRoot, "steamxr_win64.json")) {
        return false;
    }
    if (!FileExistsA(outJsonPath)) {
        Log("OpenXRManager: SteamVR runtime root found via openvr_api.dll, but steamxr_win64.json is missing at \"%s\".\n", outJsonPath);
        return false;
    }

    Log("OpenXRManager: SteamVR runtime resolved via openvr_api.dll: \"%s\"\n", outJsonPath);
    return true;
}

static bool TryGetSteamVRRuntimeJsonFromRegistry(char* outJsonPath, size_t outJsonPathSize) {
    if (!outJsonPath || outJsonPathSize == 0) {
        return false;
    }

    char steamPath[2048]{};
    const bool foundSteam =
        TryReadRegistryString(HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath", steamPath, sizeof(steamPath)) ||
        TryReadRegistryString(HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath", steamPath, sizeof(steamPath)) ||
        TryReadRegistryString(HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam", "InstallPath", steamPath, sizeof(steamPath));
    if (!foundSteam) {
        return false;
    }

    if (!JoinPath(outJsonPath, outJsonPathSize, steamPath, "steamapps\\common\\SteamVR\\steamxr_win64.json")) {
        return false;
    }
    if (!FileExistsA(outJsonPath)) {
        Log("OpenXRManager: Steam install found, but SteamVR OpenXR manifest is missing at \"%s\".\n", outJsonPath);
        return false;
    }

    Log("OpenXRManager: SteamVR runtime resolved via Steam install: \"%s\"\n", outJsonPath);
    return true;
}

static void ConfigurePreferredOpenXRRuntime() {
    if (GetXrRuntimeMode() != 1) {
        return;
    }

    char runtimeJson[2048]{};
    if (!TryGetSteamVRRuntimeJsonFromOpenVR(runtimeJson, sizeof(runtimeJson)) &&
        !TryGetSteamVRRuntimeJsonFromRegistry(runtimeJson, sizeof(runtimeJson))) {
        Log("OpenXRManager: xr_runtime=1 requested SteamVR, but no SteamVR OpenXR runtime manifest was found. Using system default runtime.\n");
        return;
    }

    char previousRuntime[2048]{};
    const DWORD previousLen = GetEnvironmentVariableA("XR_RUNTIME_JSON", previousRuntime, static_cast<DWORD>(sizeof(previousRuntime)));
    if (previousLen > 0 && strcmp(previousRuntime, runtimeJson) == 0) {
        Log("OpenXRManager: XR_RUNTIME_JSON already points to SteamVR: \"%s\"\n", runtimeJson);
        return;
    }

    if (!SetEnvironmentVariableA("XR_RUNTIME_JSON", runtimeJson)) {
        Log("OpenXRManager: Failed to set XR_RUNTIME_JSON to SteamVR manifest \"%s\" (gle=%lu).\n", runtimeJson, GetLastError());
        return;
    }

    Log("OpenXRManager: xr_runtime=1 forcing SteamVR OpenXR runtime via XR_RUNTIME_JSON=\"%s\"\n", runtimeJson);
}

static void LogDxgiAdapterForDevice(ID3D12Device* device) {
    if (!device) return;

    IDXGIFactory4* factory = nullptr;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || !factory) {
        Log("OpenXRManager: GPU adapter lookup failed (CreateDXGIFactory1).\n");
        return;
    }

    IDXGIAdapter1* adapter = nullptr;
    const LUID luid = device->GetAdapterLuid();
    if (FAILED(factory->EnumAdapterByLuid(luid, IID_PPV_ARGS(&adapter))) || !adapter) {
        factory->Release();
        Log("OpenXRManager: GPU adapter lookup failed (EnumAdapterByLuid).\n");
        return;
    }

    DXGI_ADAPTER_DESC1 desc{};
    adapter->GetDesc1(&desc);
    LARGE_INTEGER driverVersion{};
    const bool haveDriver = SUCCEEDED(adapter->CheckInterfaceSupport(__uuidof(IDXGIDevice), &driverVersion));

    const unsigned driverA = haveDriver ? HIWORD(driverVersion.HighPart) : 0;
    const unsigned driverB = haveDriver ? LOWORD(driverVersion.HighPart) : 0;
    const unsigned driverC = haveDriver ? HIWORD(driverVersion.LowPart) : 0;
    const unsigned driverD = haveDriver ? LOWORD(driverVersion.LowPart) : 0;

    Log("OpenXRManager: GPU adapter=\"%ls\" vendor=0x%04X device=0x%04X subsystem=0x%08X dedicatedVRAM=%lluMB sharedRAM=%lluMB software=%d driver=%u.%u.%u.%u\n",
        desc.Description,
        desc.VendorId,
        desc.DeviceId,
        desc.SubSysId,
        static_cast<unsigned long long>(desc.DedicatedVideoMemory / (1024ull * 1024ull)),
        static_cast<unsigned long long>(desc.SharedSystemMemory / (1024ull * 1024ull)),
        (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0 ? 1 : 0,
        driverA, driverB, driverC, driverD);

    adapter->Release();
    factory->Release();
}


// [MultiplyQuat / ConjugateQuat / NlerpQuat moved to openxr_math.h (inline)]

void OpenXRManager::MaybeLogRuntimeFovDetails(const XrFovf& left, const XrFovf& right, float runtimeHfovDeg, float runtimeVfovDeg, float runtimeIpdMeters) {
    const float forcedProjectionFovDeg = GetForcedFov();
    const RuntimeFovCorrection corr = ComputeRuntimeFovCorrection(left, right);
    const float correctedGameHfovDeg = GetCorrectedGameHorizontalFovDeg(corr);

    auto valueChanged = [](float a, float b) {
        return fabsf(a - b) > 0.01f;
    };

    const bool changed = !m_runtimeFovLogInitialized ||
        valueChanged(m_loggedRuntimeEyeFovs[0].angleLeft, left.angleLeft) ||
        valueChanged(m_loggedRuntimeEyeFovs[0].angleRight, left.angleRight) ||
        valueChanged(m_loggedRuntimeEyeFovs[0].angleUp, left.angleUp) ||
        valueChanged(m_loggedRuntimeEyeFovs[0].angleDown, left.angleDown) ||
        valueChanged(m_loggedRuntimeEyeFovs[1].angleLeft, right.angleLeft) ||
        valueChanged(m_loggedRuntimeEyeFovs[1].angleRight, right.angleRight) ||
        valueChanged(m_loggedRuntimeEyeFovs[1].angleUp, right.angleUp) ||
        valueChanged(m_loggedRuntimeEyeFovs[1].angleDown, right.angleDown) ||
        valueChanged(m_loggedRuntimeHorizontalFovDeg, runtimeHfovDeg) ||
        valueChanged(m_loggedRuntimeVerticalFovDeg, runtimeVfovDeg) ||
        valueChanged(m_loggedRuntimeIpd, runtimeIpdMeters) ||
        valueChanged(m_loggedForcedProjectionFovDeg, forcedProjectionFovDeg);
    if (!changed) {
        return;
    }

    m_runtimeFovLogInitialized = true;
    m_loggedRuntimeEyeFovs[0] = left;
    m_loggedRuntimeEyeFovs[1] = right;
    m_loggedRuntimeHorizontalFovDeg = runtimeHfovDeg;
    m_loggedRuntimeVerticalFovDeg = runtimeVfovDeg;
    m_loggedRuntimeIpd = runtimeIpdMeters;
    m_loggedForcedProjectionFovDeg = forcedProjectionFovDeg;

    Log("OpenXRManager[FOV]: raw left=(L=%.3f R=%.3f U=%.3f D=%.3f) right=(L=%.3f R=%.3f U=%.3f D=%.3f) runtimeHFov=%.3f runtimeVFov=%.3f runtimeIPD=%.4f correctedGameHFov=%.3f correctionYaw=%.3f correctionPitch=%.3f xr_force_fov=%.3f useRuntimeProjection=%d\n",
        left.angleLeft * (180.0f / 3.1415926535f),
        left.angleRight * (180.0f / 3.1415926535f),
        left.angleUp * (180.0f / 3.1415926535f),
        left.angleDown * (180.0f / 3.1415926535f),
        right.angleLeft * (180.0f / 3.1415926535f),
        right.angleRight * (180.0f / 3.1415926535f),
        right.angleUp * (180.0f / 3.1415926535f),
        right.angleDown * (180.0f / 3.1415926535f),
        runtimeHfovDeg,
        runtimeVfovDeg,
        runtimeIpdMeters,
        correctedGameHfovDeg,
        corr.yawDeltaRad * (180.0f / 3.1415926535f),
        corr.pitchDeltaRad * (180.0f / 3.1415926535f),
        forcedProjectionFovDeg,
        forcedProjectionFovDeg <= 1.0f ? 1 : 0);
}

// [ExtrapolatePose / RotateVector moved to openxr_math.h (inline)]

static bool WaitForQueueIdle(ID3D12CommandQueue* queue, ID3D12Fence* fence, HANDLE fenceEvent, UINT64& fenceValue) {
    if (!queue || !fence || !fenceEvent) return false;

    fenceValue++;
    if (FAILED(queue->Signal(fence, fenceValue))) return false;
    if (fence->GetCompletedValue() < fenceValue) {
        if (FAILED(fence->SetEventOnCompletion(fenceValue, fenceEvent))) return false;
        WaitForSingleObject(fenceEvent, INFINITE);
    }
    return true;
}

// [ContainsSwapchainFormat / PickMonoSwapchainFormat moved to openxr_internal.h (inline)]

// [ApplyForcedProjectionFov / ComputeCantPoseDelta / ApplyCantToPose moved to openxr_internal.h (inline)]

// Reuse-last-frame output path. When enabled, the AER submit path re-submits the
// last clean eye on stale ticks instead of warping stale content again.
// [ReuseLastFrameOutputEnabled() moved to openxr_internal.h (inline)]

DWORD WINAPI OpenXRManager::FrameThreadThunk(LPVOID param) {
    return static_cast<OpenXRManager*>(param)->FrameThreadMain();
}

DWORD WINAPI OpenXRManager::AerSubmitThreadThunk(LPVOID param) {
    return static_cast<OpenXRManager*>(param)->AerSubmitThreadMain();
}

void OpenXRManager::NotifyAerThread() {
    m_aerThreadWakeCv.notify_all();
}

// Take ownership of the XR frame loop. Returns false if another owner still
// holds it after timeoutMs (caller must then skip driving the loop this tick).
bool OpenXRManager::AcquireFrameLoop(FrameLoopOwner who, unsigned int timeoutMs) {
    std::unique_lock<std::mutex> lock(m_frameLoopMutex);
    if (!m_frameLoopCv.wait_for(lock, std::chrono::milliseconds(timeoutMs),
            [this]() { return m_frameLoopOwner == FrameLoopOwner::None; })) {
        return false;
    }
    m_frameLoopOwner = who;
    return true;
}

void OpenXRManager::ReleaseFrameLoop(FrameLoopOwner who) {
    {
        std::lock_guard<std::mutex> lock(m_frameLoopMutex);
        if (m_frameLoopOwner == who) {
            m_frameLoopOwner = FrameLoopOwner::None;
        }
    }
    m_frameLoopCv.notify_all();
}

// Dedicated AER submit thread. Parks (≈0% CPU) while AER is disabled; when AER is
// on it owns the frame loop and drives FrameThreadMain in a tight loop that is
// self-paced by xrWaitFrame (~90 Hz), independent of the game's Present rate.
DWORD OpenXRManager::AerSubmitThreadMain() {
    Log("OpenXRManager: VR submit thread started (dormant until AER, or mono on SteamVR).\n");
    while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
        {
            std::unique_lock<std::mutex> lock(m_aerThreadMutex);
            // Wake on AER-enabled ALONE (do NOT require m_sessionRunning). The
            // session only becomes running via BeginSession, which runs inside
            // FrameThreadMain->PollEvents. In AER mode the Present thread returns
            // early and cannot pump that, so if we also gated this thread on
            // sessionRunning, nobody would ever pump the session when AER is on
            // from launch -> the OpenXR session never reaches READY -> VR never
            // starts. Waking on AER-enabled lets this thread BOOTSTRAP the session.
            m_aerThreadWakeCv.wait_for(lock, std::chrono::milliseconds(200), [this]() {
                return m_stopFrameThread.load(std::memory_order_relaxed) ||
                       UseThreadedSubmit();
            });
        }
        if (m_stopFrameThread.load(std::memory_order_relaxed)) break;
        if (!UseThreadedSubmit()) {
            continue;
        }
        // Take the frame loop from the inline/mono owner. This is NOT the Present
        // thread, so a bounded wait-and-retry here is safe.
        if (!AcquireFrameLoop(FrameLoopOwner::Aer, 100)) {
            continue;
        }
        Log("OpenXRManager: VR submit thread acquired frame loop (aer=%d steamvrMono=%d).\n",
            IsAERSubmitEnabled() ? 1 : 0,
            (IsRuntimeSteamVR() && m_monoSubmitEnabled.load(std::memory_order_relaxed)) ? 1 : 0);
        // FrameThreadMain self-handles the not-yet-running phase (PollEvents ->
        // session state advances -> BeginSession), so this loop bootstraps the
        // session when AER is enabled from startup, then drives AER submit once
        // running (self-paced by xrWaitFrame).
        while (!m_stopFrameThread.load(std::memory_order_relaxed) &&
               UseThreadedSubmit()) {
            FrameThreadMain();
        }
        ReleaseFrameLoop(FrameLoopOwner::Aer);
        Log("OpenXRManager: AER submit thread released frame loop (parking).\n");
    }
    Log("OpenXRManager: AER submit thread exiting.\n");
    return 0;
}

OpenXRManager& OpenXRManager::Get() {
    static OpenXRManager instance;
    return instance;
}

// [recenter / auto-calibration / calibration-file methods (RotateBaseYaw ... LoadCalibrationFromFile) moved to openxr_calibration.cpp]

void OpenXRManager::SetMonoSubmitEnabled(bool enabled) {
    m_monoSubmitEnabled.store(enabled, std::memory_order_relaxed);
    // Wake the dedicated submit thread: on SteamVR it owns the loop for mono too
    // (UseThreadedSubmit()), so enabling mono must un-park it promptly.
    NotifyAerThread();
    if (m_monoPresentEvent) {
        ResetEvent(m_monoPresentEvent);
    }
    std::lock_guard<std::mutex> lock(m_presentMutex);
    m_monoCapturedFrame.serial = 0;
    m_monoCapturedFrame.hasView[0] = false;
    m_monoCapturedFrame.hasView[1] = false;
    m_depthSnapshotSerial = 0;
}

void OpenXRManager::SetAERSubmitEnabled(bool enabled) {
    m_aerSubmitEnabled.store(enabled, std::memory_order_relaxed);
    if (enabled) {
        NotifyAerThread();   // wake the dormant AER submit thread
    }
    // Always start the capture cadence at eye 0. start_eye=1 desynchronized the
    // even/odd present parity (eye 0 was never captured -> pair never completed
    // -> xrEndFrame never ran -> black screen), so it is intentionally ignored.
    m_renderEyeIndex.store(0, std::memory_order_relaxed);
    m_aerWarmupRemaining = GetAERWarmupFrames();
    m_aerPairCounter = 0;

    std::lock_guard<std::mutex> lock(m_presentMutex);
    for (CapturedEyeFrame& frame : m_capturedEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.depthSerial = 0;
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_previousCapturedEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.depthSerial = 0;
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_pendingEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.depthSerial = 0;
        frame.hasView = false;
    }
    m_lastSubmittedPairId = 0;
    m_interpolatedPairId = 0;
    m_interpolatedSynthSlot = 0;
    m_interpolatedSyntheticEye = -1;
    for (int eye = 0; eye < 2; ++eye) {
        for (int slot = 0; slot < 2; ++slot) {
            m_aerV2SubmitEyeReady[eye][slot] = false;
        }
    }
    m_interpolatedEyeViewsValid[0] = false;
    m_interpolatedEyeViewsValid[1] = false;
}

// [mono/AER capture + submit-resource methods (EnsureMonoCaptureResource ... EnsureMonoSubmitResources) moved to openxr_capture.cpp]

bool OpenXRManager::Init() {
    std::lock_guard<std::mutex> initLock(m_initMutex);
    if (m_initialized) return true;

    Log("OpenXRManager: Initializing...\n");
    ConfigurePreferredOpenXRRuntime();

    // Extensions we need
    std::vector<const char*> extensions = {
        XR_KHR_D3D12_ENABLE_EXTENSION_NAME
    };

    // Depth-layer support: submitting the game depth as XR_KHR_composition_layer_depth
    // gives the runtime depth for correct reprojection (kills the flat-color tearing).
    {
        uint32_t extCount = 0;
        xrEnumerateInstanceExtensionProperties(nullptr, 0, &extCount, nullptr);
        std::vector<XrExtensionProperties> props(extCount, {XR_TYPE_EXTENSION_PROPERTIES});
        if (extCount > 0 &&
            XR_SUCCEEDED(xrEnumerateInstanceExtensionProperties(nullptr, extCount, &extCount, props.data()))) {
            for (const auto& p : props) {
                if (strcmp(p.extensionName, XR_KHR_COMPOSITION_LAYER_DEPTH_EXTENSION_NAME) == 0) {
                    m_depthLayerSupported = true;
                    extensions.push_back(XR_KHR_COMPOSITION_LAYER_DEPTH_EXTENSION_NAME);
                    break;
                }
            }
        }
        Log("OpenXRManager: depth-layer (XR_KHR_composition_layer_depth) supported=%d\n", m_depthLayerSupported ? 1 : 0);
    }

    XrInstanceCreateInfo createInfo{XR_TYPE_INSTANCE_CREATE_INFO};
    createInfo.applicationInfo.apiVersion = XR_CURRENT_API_VERSION;
    strcpy_s(createInfo.applicationInfo.applicationName, "CyberpunkVRPort");
    createInfo.enabledExtensionCount = static_cast<uint32_t>(extensions.size());
    createInfo.enabledExtensionNames = extensions.data();

    XrResult res = xrCreateInstance(&createInfo, &m_instance);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create XrInstance (res=%d)\n", res);
        return false;
    }

    XrInstanceProperties instanceProps{XR_TYPE_INSTANCE_PROPERTIES};
    if (XR_SUCCEEDED(xrGetInstanceProperties(m_instance, &instanceProps))) {
        Log("OpenXRManager: OpenXR runtime name=\"%s\" kind=%s version=%u.%u.%u\n",
            instanceProps.runtimeName,
            ClassifyOpenXRRuntime(instanceProps.runtimeName),
            XR_VERSION_MAJOR(instanceProps.runtimeVersion),
            XR_VERSION_MINOR(instanceProps.runtimeVersion),
            XR_VERSION_PATCH(instanceProps.runtimeVersion));
        const bool actuallySteamVR = strcmp(ClassifyOpenXRRuntime(instanceProps.runtimeName), "SteamVR") == 0;
        const bool actuallyVD = strcmp(ClassifyOpenXRRuntime(instanceProps.runtimeName), "Virtual Desktop") == 0;
        m_runtimeIsVirtualDesktop.store(actuallyVD, std::memory_order_relaxed);
        // Detect the ACTIVE runtime by name, independent of the xr_runtime ini flag.
        // The pose-pair lock (GetSyncSequential) keys off this so SteamVR gets the
        // fix even when launched as the SYSTEM default OpenXR runtime with
        // xr_runtime=0 (otherwise left-eye judder/tearing returns).
        m_runtimeIsSteamVR.store(actuallySteamVR, std::memory_order_relaxed);
        Log("OpenXRManager: runtimeIsSteamVR=%d (pose-pair lock %s)\n",
            actuallySteamVR ? 1 : 0, actuallySteamVR ? "ENABLED" : "off");
        if (GetXrRuntimeMode() == 1 && !actuallySteamVR) {
            Log("OpenXRManager: xr_runtime=1 requested SteamVR, but the active runtime identified as %s.\n", ClassifyOpenXRRuntime(instanceProps.runtimeName));
        }
    }

    XrSystemGetInfo systemInfo{XR_TYPE_SYSTEM_GET_INFO};
    systemInfo.formFactor = XR_FORM_FACTOR_HEAD_MOUNTED_DISPLAY;
    res = xrGetSystem(m_instance, &systemInfo, &m_systemId);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to get XrSystemId (res=%d)\n", res);
        xrDestroyInstance(m_instance);
        m_instance = XR_NULL_HANDLE;
        return false;
    }

    XrSystemProperties systemProps{XR_TYPE_SYSTEM_PROPERTIES};
    if (XR_SUCCEEDED(xrGetSystemProperties(m_instance, m_systemId, &systemProps))) {
        Log("OpenXRManager: OpenXR system vendorId=0x%X systemName=\"%s\" maxSwapchain=%ux%u maxLayerCount=%u positionTracking=%d orientationTracking=%d\n",
            systemProps.vendorId,
            systemProps.systemName,
            systemProps.graphicsProperties.maxSwapchainImageWidth,
            systemProps.graphicsProperties.maxSwapchainImageHeight,
            systemProps.graphicsProperties.maxLayerCount,
            systemProps.trackingProperties.positionTracking ? 1 : 0,
            systemProps.trackingProperties.orientationTracking ? 1 : 0);
    }

    uint32_t viewCount = 0;
    if (XR_SUCCEEDED(xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, 0, &viewCount, nullptr)) && viewCount > 0) {
        m_viewConfigViews.resize(viewCount, {XR_TYPE_VIEW_CONFIGURATION_VIEW});
        xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, viewCount, &viewCount, m_viewConfigViews.data());
        for (uint32_t eye = 0; eye < viewCount; ++eye) {
            const XrViewConfigurationView& view = m_viewConfigViews[eye];
            Log("OpenXRManager[FOV]: viewConfig eye=%u recommended=%ux%u max=%ux%u samples=%u\n",
                eye,
                view.recommendedImageRectWidth,
                view.recommendedImageRectHeight,
                view.maxImageRectWidth,
                view.maxImageRectHeight,
                view.recommendedSwapchainSampleCount);
        }
    }

    Log("OpenXRManager: OpenXR Initialized. SystemID=%llu\n", m_systemId);

    // [INPUT] Action Set Initialization -- gameplay locomotion + buttons
    const bool inputActionsEnabled = GetInputActionsEnabled() != 0;
    {
        XrActionSetCreateInfo actionSetInfo{XR_TYPE_ACTION_SET_CREATE_INFO};
        strcpy_s(actionSetInfo.actionSetName, "gameplay");
        strcpy_s(actionSetInfo.localizedActionSetName, "Gameplay");
        actionSetInfo.priority = 0;
        xrCreateActionSet(m_instance, &actionSetInfo, &m_actionSet);

        xrStringToPath(m_instance, "/user/hand/left", &m_handPaths[0]);
        xrStringToPath(m_instance, "/user/hand/right", &m_handPaths[1]);

        auto makeAction = [&](XrAction& out, XrActionType type, const char* name, const char* loc, bool perHand) {
            XrActionCreateInfo info{XR_TYPE_ACTION_CREATE_INFO};
            info.actionType = type;
            strcpy_s(info.actionName, name);
            strcpy_s(info.localizedActionName, loc);
            if (perHand) {
                info.countSubactionPaths = 2;
                info.subactionPaths = m_handPaths;
            }
            xrCreateAction(m_actionSet, &info, &out);
        };

        makeAction(m_handPoseAction,        XR_ACTION_TYPE_POSE_INPUT,     "hand_pose",        "Hand Pose",            true);
        // aim pose has a runtime-stable forward direction (-Z = pointing) that
        // is NOT mirrored between left/right grip poses. We use it for the
        // hand-locomotion yaw so the player walks where they point, not where
        // their palm faces.
        if (inputActionsEnabled) {
            makeAction(m_handAimPoseAction, XR_ACTION_TYPE_POSE_INPUT, "hand_aim_pose", "Hand Aim Pose", true);
        }
        if (inputActionsEnabled) {
            makeAction(m_thumbstickAction,      XR_ACTION_TYPE_VECTOR2F_INPUT, "thumbstick",       "Thumbstick",           true);
            makeAction(m_triggerAction,         XR_ACTION_TYPE_FLOAT_INPUT,    "trigger",          "Trigger",              true);
            makeAction(m_gripAction,            XR_ACTION_TYPE_FLOAT_INPUT,    "grip",             "Grip",                 true);
            makeAction(m_thumbstickClickAction, XR_ACTION_TYPE_BOOLEAN_INPUT,  "thumbstick_click", "Thumbstick Click",     true);
            makeAction(m_primaryButtonAction,   XR_ACTION_TYPE_BOOLEAN_INPUT,  "primary_button",   "Primary Button (A/X)", true);
            makeAction(m_secondaryButtonAction, XR_ACTION_TYPE_BOOLEAN_INPUT,  "secondary_button", "Secondary Button (B/Y)", true);
            makeAction(m_menuButtonAction,      XR_ACTION_TYPE_BOOLEAN_INPUT,  "menu",             "Menu Button",          false);
        }
        Log("OpenXRManager[Input]: gameplay action set %s (xr_input_actions=%d)\n",
            inputActionsEnabled ? "ENABLED" : "DISABLED (pose-only)", (int)inputActionsEnabled);

        struct Bind { XrAction action; const char* path; };

        auto suggest = [&](const char* profileStr, std::initializer_list<Bind> list) {
            XrPath profile = XR_NULL_PATH;
            if (XR_FAILED(xrStringToPath(m_instance, profileStr, &profile))) return;
            std::vector<XrActionSuggestedBinding> v;
            v.reserve(list.size());
            for (const Bind& b : list) {
                XrPath p = XR_NULL_PATH;
                if (XR_SUCCEEDED(xrStringToPath(m_instance, b.path, &p))) {
                    v.push_back({ b.action, p });
                }
            }
            XrInteractionProfileSuggestedBinding sb{XR_TYPE_INTERACTION_PROFILE_SUGGESTED_BINDING};
            sb.interactionProfile = profile;
            sb.suggestedBindings = v.data();
            sb.countSuggestedBindings = static_cast<uint32_t>(v.size());
            XrResult r = xrSuggestInteractionProfileBindings(m_instance, &sb);
            Log("OpenXRManager[Input]: suggest bindings %s -> %d (count=%u)\n", profileStr, r, sb.countSuggestedBindings);
        };

        if (!inputActionsEnabled) {
            // Pose-only legacy behaviour: only suggest the grip-pose pair (matches the
            // pre-Controls-tab build, useful as a kill-switch if the runtime chokes on
            // the larger binding set).
            const std::initializer_list<Bind> poseOnly = {
                { m_handPoseAction, "/user/hand/left/input/grip/pose" },
                { m_handPoseAction, "/user/hand/right/input/grip/pose" },
            };
            for (const char* profile : { "/interaction_profiles/oculus/touch_controller",
                                          "/interaction_profiles/valve/index_controller",
                                          "/interaction_profiles/htc/vive_controller",
                                          "/interaction_profiles/microsoft/motion_controller",
                                          "/interaction_profiles/khr/simple_controller" }) {
                suggest(profile, poseOnly);
            }
            goto bindings_done;
        }

        // -- Oculus Touch (Quest/Rift): X/Y on left, A/B on right, menu = left menu button --
        suggest("/interaction_profiles/oculus/touch_controller", {
            { m_handPoseAction,        "/user/hand/left/input/grip/pose" },
            { m_handPoseAction,        "/user/hand/right/input/grip/pose" },
            { m_handAimPoseAction,     "/user/hand/left/input/aim/pose" },
            { m_handAimPoseAction,     "/user/hand/right/input/aim/pose" },
            { m_thumbstickAction,      "/user/hand/left/input/thumbstick" },
            { m_thumbstickAction,      "/user/hand/right/input/thumbstick" },
            { m_thumbstickClickAction, "/user/hand/left/input/thumbstick/click" },
            { m_thumbstickClickAction, "/user/hand/right/input/thumbstick/click" },
            { m_triggerAction,         "/user/hand/left/input/trigger/value" },
            { m_triggerAction,         "/user/hand/right/input/trigger/value" },
            { m_gripAction,            "/user/hand/left/input/squeeze/value" },
            { m_gripAction,            "/user/hand/right/input/squeeze/value" },
            { m_primaryButtonAction,   "/user/hand/left/input/x/click" },
            { m_primaryButtonAction,   "/user/hand/right/input/a/click" },
            { m_secondaryButtonAction, "/user/hand/left/input/y/click" },
            { m_secondaryButtonAction, "/user/hand/right/input/b/click" },
            { m_menuButtonAction,      "/user/hand/left/input/menu/click" },
        });

        // -- Valve Index: A/B on both hands, system as menu --
        suggest("/interaction_profiles/valve/index_controller", {
            { m_handPoseAction,        "/user/hand/left/input/grip/pose" },
            { m_handPoseAction,        "/user/hand/right/input/grip/pose" },
            { m_handAimPoseAction,     "/user/hand/left/input/aim/pose" },
            { m_handAimPoseAction,     "/user/hand/right/input/aim/pose" },
            { m_thumbstickAction,      "/user/hand/left/input/thumbstick" },
            { m_thumbstickAction,      "/user/hand/right/input/thumbstick" },
            { m_thumbstickClickAction, "/user/hand/left/input/thumbstick/click" },
            { m_thumbstickClickAction, "/user/hand/right/input/thumbstick/click" },
            { m_triggerAction,         "/user/hand/left/input/trigger/value" },
            { m_triggerAction,         "/user/hand/right/input/trigger/value" },
            { m_gripAction,            "/user/hand/left/input/squeeze/value" },
            { m_gripAction,            "/user/hand/right/input/squeeze/value" },
            { m_primaryButtonAction,   "/user/hand/left/input/a/click" },
            { m_primaryButtonAction,   "/user/hand/right/input/a/click" },
            { m_secondaryButtonAction, "/user/hand/left/input/b/click" },
            { m_secondaryButtonAction, "/user/hand/right/input/b/click" },
            { m_menuButtonAction,      "/user/hand/left/input/system/click" },
        });

        // -- HTC Vive Wand: no A/B/X/Y, no thumbstick (touchpad as v2f), grip is bool --
        suggest("/interaction_profiles/htc/vive_controller", {
            { m_handPoseAction,        "/user/hand/left/input/grip/pose" },
            { m_handPoseAction,        "/user/hand/right/input/grip/pose" },
            { m_handAimPoseAction,     "/user/hand/left/input/aim/pose" },
            { m_handAimPoseAction,     "/user/hand/right/input/aim/pose" },
            { m_thumbstickAction,      "/user/hand/left/input/trackpad" },
            { m_thumbstickAction,      "/user/hand/right/input/trackpad" },
            { m_thumbstickClickAction, "/user/hand/left/input/trackpad/click" },
            { m_thumbstickClickAction, "/user/hand/right/input/trackpad/click" },
            { m_triggerAction,         "/user/hand/left/input/trigger/value" },
            { m_triggerAction,         "/user/hand/right/input/trigger/value" },
            { m_menuButtonAction,      "/user/hand/left/input/menu/click" },
        });

        // -- Windows MR motion controller: trackpad+thumbstick combo --
        suggest("/interaction_profiles/microsoft/motion_controller", {
            { m_handPoseAction,        "/user/hand/left/input/grip/pose" },
            { m_handPoseAction,        "/user/hand/right/input/grip/pose" },
            { m_handAimPoseAction,     "/user/hand/left/input/aim/pose" },
            { m_handAimPoseAction,     "/user/hand/right/input/aim/pose" },
            { m_thumbstickAction,      "/user/hand/left/input/thumbstick" },
            { m_thumbstickAction,      "/user/hand/right/input/thumbstick" },
            { m_thumbstickClickAction, "/user/hand/left/input/thumbstick/click" },
            { m_thumbstickClickAction, "/user/hand/right/input/thumbstick/click" },
            { m_triggerAction,         "/user/hand/left/input/trigger/value" },
            { m_triggerAction,         "/user/hand/right/input/trigger/value" },
            { m_menuButtonAction,      "/user/hand/left/input/menu/click" },
        });

        // -- KHR simple controller (fallback: only select + menu + grip pose) --
        suggest("/interaction_profiles/khr/simple_controller", {
            { m_handPoseAction,        "/user/hand/left/input/grip/pose" },
            { m_handPoseAction,        "/user/hand/right/input/grip/pose" },
            { m_primaryButtonAction,   "/user/hand/left/input/select/click" },
            { m_primaryButtonAction,   "/user/hand/right/input/select/click" },
            { m_menuButtonAction,      "/user/hand/left/input/menu/click" },
        });

bindings_done:
        (void)0;
    }

    m_initialized = true;
    return true;
}

bool OpenXRManager::GetRecommendedRenderTargetSize(uint32_t* width, uint32_t* height) const {
    if (m_viewConfigViews.empty()) return false;
    if (width) *width = m_viewConfigViews[0].recommendedImageRectWidth;
    if (height) *height = m_viewConfigViews[0].recommendedImageRectHeight;
    return true;
}

bool OpenXRManager::InitGraphics(ID3D12Device* device, ID3D12CommandQueue* queue) {
    if (!m_initialized || m_session != XR_NULL_HANDLE) return false;

    Log("OpenXRManager: Initializing D3D12 Graphics Binding...\n");

    // Load D3D12 extension function
    PFN_xrGetD3D12GraphicsRequirementsKHR pfnGetD3D12GraphicsRequirementsKHR = nullptr;
    xrGetInstanceProcAddr(m_instance, "xrGetD3D12GraphicsRequirementsKHR", 
        (PFN_xrVoidFunction*)&pfnGetD3D12GraphicsRequirementsKHR);

    if (!pfnGetD3D12GraphicsRequirementsKHR) {
        Log("OpenXRManager: xrGetD3D12GraphicsRequirementsKHR not found!\n");
        return false;
    }

    XrGraphicsRequirementsD3D12KHR reqs{XR_TYPE_GRAPHICS_REQUIREMENTS_D3D12_KHR};
    pfnGetD3D12GraphicsRequirementsKHR(m_instance, m_systemId, &reqs);
    Log("OpenXRManager: D3D12 graphics requirements minFeatureLevel=0x%X luid=(0x%08X,0x%08X)\n",
        reqs.minFeatureLevel,
        static_cast<unsigned>(reqs.adapterLuid.HighPart),
        static_cast<unsigned>(reqs.adapterLuid.LowPart));

    m_graphicsBinding.type = XR_TYPE_GRAPHICS_BINDING_D3D12_KHR;
    m_graphicsBinding.device = device;
    m_graphicsBinding.queue = queue;

    LogDxgiAdapterForDevice(device);

    XrSessionCreateInfo sessionInfo{XR_TYPE_SESSION_CREATE_INFO};
    sessionInfo.next = &m_graphicsBinding;
    sessionInfo.systemId = m_systemId;

    XrResult res = xrCreateSession(m_instance, &sessionInfo, &m_session);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create XrSession (res=%d)\n", res);
        return false;
    }

    m_runtimeFovLogInitialized = false;
    m_loggedRuntimeEyeFovs[0] = {};
    m_loggedRuntimeEyeFovs[1] = {};
    m_loggedRuntimeHorizontalFovDeg = 0.0f;
    m_loggedRuntimeVerticalFovDeg = 0.0f;
    m_loggedRuntimeIpd = 0.0f;
    m_loggedForcedProjectionFovDeg = 0.0f;

    m_d3dDevice = device;
    m_d3dQueue = queue;
    if (m_d3dDevice) m_d3dDevice->AddRef();
    if (m_d3dQueue) m_d3dQueue->AddRef();
    if (!m_opticalFlow) {
        m_opticalFlow = std::make_unique<OpticalFlowD3D12>();
    }
    if (!m_monoPresentEvent) {
        m_monoPresentEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!m_monoPresentEvent) {
            Log("OpenXRManager: Failed to create mono present event\n");
            return false;
        }
    }
    if (!m_frameSyncEvent) {
        m_frameSyncEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
    }

    Log("OpenXRManager: Pose-only mode active until xr_mono_submit is enabled.\n");

    XrReferenceSpaceCreateInfo localSpaceInfo{XR_TYPE_REFERENCE_SPACE_CREATE_INFO};
    localSpaceInfo.referenceSpaceType = XR_REFERENCE_SPACE_TYPE_LOCAL;
    localSpaceInfo.poseInReferenceSpace.orientation.w = 1.0f;
    res = xrCreateReferenceSpace(m_session, &localSpaceInfo, &m_localSpace);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create local space (res=%d)\n", res);
        return false;
    }

    XrReferenceSpaceCreateInfo viewSpaceInfo{XR_TYPE_REFERENCE_SPACE_CREATE_INFO};
    viewSpaceInfo.referenceSpaceType = XR_REFERENCE_SPACE_TYPE_VIEW;
    viewSpaceInfo.poseInReferenceSpace.orientation.w = 1.0f;
    res = xrCreateReferenceSpace(m_session, &viewSpaceInfo, &m_viewSpace);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: Failed to create view space (res=%d)\n", res);
        return false;
    }

    // [HANDS] Attach action sets and create spaces
    if (m_actionSet != XR_NULL_HANDLE) {
        XrSessionActionSetsAttachInfo attachInfo{XR_TYPE_SESSION_ACTION_SETS_ATTACH_INFO};
        attachInfo.countActionSets = 1;
        attachInfo.actionSets = &m_actionSet;
        xrAttachSessionActionSets(m_session, &attachInfo);

        for (int i = 0; i < 2; i++) {
            XrActionSpaceCreateInfo spaceInfo{XR_TYPE_ACTION_SPACE_CREATE_INFO};
            spaceInfo.action = m_handPoseAction;
            spaceInfo.subactionPath = m_handPaths[i];
            spaceInfo.poseInActionSpace.orientation.w = 1.0f;
            xrCreateActionSpace(m_session, &spaceInfo, &m_handSpaces[i]);

            if (m_handAimPoseAction != XR_NULL_HANDLE) {
                XrActionSpaceCreateInfo aimSpaceInfo{XR_TYPE_ACTION_SPACE_CREATE_INFO};
                aimSpaceInfo.action = m_handAimPoseAction;
                aimSpaceInfo.subactionPath = m_handPaths[i];
                aimSpaceInfo.poseInActionSpace.orientation.w = 1.0f;
                xrCreateActionSpace(m_session, &aimSpaceInfo, &m_handAimSpaces[i]);
            }
        }
    }

    m_stopFrameThread.store(false, std::memory_order_relaxed);
    if (!m_frameThread) {
        m_frameThread = CreateThread(nullptr, 0, &OpenXRManager::AerSubmitThreadThunk, this, 0, nullptr);
        if (m_frameThread) {
            Log("OpenXRManager: AER submit thread created (dormant).\n");
        } else {
            Log("OpenXRManager: WARNING failed to create AER submit thread (err=%lu); AER submit unavailable.\n", GetLastError());
        }
    }

    Log("OpenXRManager: Session created successfully.\n");
    return true;
}

bool OpenXRManager::BeginSession() {
    if (m_session == XR_NULL_HANDLE || m_sessionRunning.load(std::memory_order_relaxed)) return false;

    XrSessionBeginInfo beginInfo{XR_TYPE_SESSION_BEGIN_INFO};
    beginInfo.primaryViewConfigurationType = XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO;
    XrResult res = xrBeginSession(m_session, &beginInfo);
    if (XR_FAILED(res)) {
        Log("OpenXRManager: xrBeginSession failed (res=%d)\n", res);
        return false;
    }

    m_sessionRunning.store(true, std::memory_order_relaxed);
    NotifyAerThread();   // in case AER was enabled before the session started
    Log("OpenXRManager: Session begun.\n");
    return true;
}

void OpenXRManager::EndSession() {
    if (m_session == XR_NULL_HANDLE || !m_sessionRunning.load(std::memory_order_relaxed)) return;
    xrEndSession(m_session);
    m_sessionRunning.store(false, std::memory_order_relaxed);
    Log("OpenXRManager: Session ended.\n");
}

void OpenXRManager::PollEvents() {
    if (m_instance == XR_NULL_HANDLE) return;

    XrEventDataBuffer event{XR_TYPE_EVENT_DATA_BUFFER};
    while (xrPollEvent(m_instance, &event) == XR_SUCCESS) {
        if (event.type == XR_TYPE_EVENT_DATA_SESSION_STATE_CHANGED) {
            auto* changed = reinterpret_cast<XrEventDataSessionStateChanged*>(&event);
            m_sessionState = changed->state;
            Log("OpenXRManager: Session state -> %d\n", static_cast<int>(m_sessionState));

            if (m_sessionState == XR_SESSION_STATE_READY) {
                BeginSession();
            } else if (m_sessionState == XR_SESSION_STATE_STOPPING) {
                EndSession();
            } else if (m_sessionState == XR_SESSION_STATE_EXITING || m_sessionState == XR_SESSION_STATE_LOSS_PENDING) {
                m_stopFrameThread.store(true, std::memory_order_relaxed);
            }
        } else if (event.type == XR_TYPE_EVENT_DATA_REFERENCE_SPACE_CHANGE_PENDING) {
            // Native OpenXR recenter (user held the home / system button, or used the runtime menu) —
            // the runtime is about to remap "forward" of its tracking space at changed->changeTime.
            // Trigger our local recenter so the mod's stored base pose lines up with the runtime's
            // new tracking space; the next frame's HMD pose then reads (0,0,0,facing-forward) as the
            // user expects.
            auto* changed = reinterpret_cast<XrEventDataReferenceSpaceChangePending*>(&event);
            Log("OpenXRManager: Tracking space change pending (native recenter), refSpace=%d -> local recenter.\n",
                static_cast<int>(changed->referenceSpaceType));
            RequestRecenter();
        }

        event = {XR_TYPE_EVENT_DATA_BUFFER};
    }
}

// [PumpInlineFrame() moved to openxr_frameloop.cpp]

bool OpenXRManager::GetHandPose(int handIndex, OpenXRHeadPose* out) const {
    if (!out || handIndex < 0 || handIndex > 1) return false;
    std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));
    *out = m_hands[handIndex];
    return out->valid;
}

void OpenXRManager::SetWeaponOffsets(float pitch, float yaw, float roll, float dx, float dy, float dz) {
    m_weaponPitch = pitch;
    m_weaponYaw = yaw;
    m_weaponRoll = roll;
    m_weaponDx = dx;
    m_weaponDy = dy;
    m_weaponDz = dz;
}

float OpenXRManager::GetHmdYawRelToBody() const {
    // relOri (m_ori*) is the HMD orientation relative to the recenter base, in XR
    // space (Y up). Extract the heading/yaw about the Y axis.
    float x = m_oriX.load(std::memory_order_relaxed);
    float y = m_oriY.load(std::memory_order_relaxed);
    float z = m_oriZ.load(std::memory_order_relaxed);
    float w = m_oriW.load(std::memory_order_relaxed);
    return std::atan2(2.0f * (w * y + x * z), 1.0f - 2.0f * (y * y + z * z));
}

float OpenXRManager::GetHandYawRelToBody(int side) const {
    if (side < 0 || side > 1) return 0.0f;
    if (!m_handYawValid[side].load(std::memory_order_relaxed)) {
        // Fall back to HMD heading so locomotion doesn't snap to 0 when a
        // controller drops tracking mid-step.
        return GetHmdYawRelToBody();
    }
    return m_handYawRelToBody[side].load(std::memory_order_relaxed);
}

bool OpenXRManager::GetBodyYawFromHands(float* outYaw) const {
    if (!outYaw) return false;
    OpenXRHeadPose l{}, r{};
    {
        std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));
        l = m_hands[0];
        r = m_hands[1];
    }
    if (!l.valid || !r.valid) return false;

    // Hand poses are HMD-local; rotate them into the recenter-base frame with the
    // HMD's base-relative orientation so the line is measured in body space.
    const XrQuaternionf rel{
        m_oriX.load(std::memory_order_relaxed),
        m_oriY.load(std::memory_order_relaxed),
        m_oriZ.load(std::memory_order_relaxed),
        m_oriW.load(std::memory_order_relaxed)};
    const XrVector3f pl = RotateVector(rel, XrVector3f{l.posX, l.posY, l.posZ});
    const XrVector3f pr = RotateVector(rel, XrVector3f{r.posX, r.posY, r.posZ});

    // Horizontal left->right hand line (XR base frame: +Y up, -Z forward at yaw 0).
    const float dx = pr.x - pl.x;
    const float dz = pr.z - pl.z;
    if ((dx * dx + dz * dz) < 0.12f * 0.12f) return false; // hands together: no line

    // Chest forward = the line rotated +90 deg about +Y -> (dz, 0, -dx). Yaw with the
    // same convention as GetHmdYawRelToBody (facing -Z = 0, left = positive):
    // yaw = atan2(-fwd.x, -fwd.z) = atan2(-dz, dx).
    *outYaw = atan2f(-dz, dx);
    return true;
}

XrPosef OpenXRManager::ComputeMenuQuadPose(bool headPoseLocated, const XrPosef& headPose) {
    // Head tracking dropped this frame: hold the last pose (don't jump to the base pose
    // -- that snapped the panel sideways / to the floor origin on brief tracking gaps).
    if (!headPoseLocated) {
        if (m_menuAnchorValid) return m_menuQuadPose;
    }

    // Reference head pose (live head, or the recenter base only for the FIRST anchor).
    XrPosef ref{};
    ref.orientation.w = 1.0f;
    if (headPoseLocated) {
        ref = headPose;
    } else {
        std::lock_guard<std::mutex> lock(m_renderPoseMutex);
        if (m_basePoseSet) ref = m_basePose;
    }

    // Live head yaw, flattened to pure yaw (keeps the panel vertical).
    const XrQuaternionf o = ref.orientation;
    const float fx = -2.0f * (o.x * o.z + o.y * o.w);
    const float fz = 2.0f * (o.x * o.x + o.y * o.y) - 1.0f;
    const float headYaw = atan2f(-fx, -fz);

    auto wrapPi = [](float a) {
        while (a >  3.14159265f) a -= 6.28318531f;
        while (a < -3.14159265f) a += 6.28318531f;
        return a;
    };

    // dt for the ease.
    LARGE_INTEGER qf, qn;
    QueryPerformanceFrequency(&qf);
    QueryPerformanceCounter(&qn);
    float dt = 0.0f;
    if (m_menuLastQpc != 0) dt = static_cast<float>(qn.QuadPart - m_menuLastQpc) / static_cast<float>(qf.QuadPart);
    m_menuLastQpc = qn.QuadPart;
    if (dt < 0.0f) dt = 0.0f;
    if (dt > 0.05f) dt = 0.05f;

    if (!m_menuAnchorValid) {
        m_menuYaw = headYaw;
        m_menuFollowing = false;
        m_menuAnchorValid = true;
    }

    // Follow the head TRANSLATION every frame (walking/leaning keeps the panel ahead);
    // only the YAW is lazy (yaw tracking is what caused the swim / motion sickness).
    m_menuPivot = ref.position;

    float startRad = GetMenuFollowDeg() * 0.01745329252f;
    if (startRad < 0.0872f)  startRad = 0.0872f;   // clamp 5..90 deg
    if (startRad > 1.5708f)  startRad = 1.5708f;
    constexpr float kStopRad = 0.0349f;   // 2 deg: re-centered, stop
    constexpr float kRate    = 3.0f;      // rad/s ease toward the head (~170 deg/s)

    float offset = wrapPi(headYaw - m_menuYaw);
    if (!m_menuFollowing && fabsf(offset) > startRad) m_menuFollowing = true;
    if (m_menuFollowing) {
        float step = kRate * dt;
        if (step > fabsf(offset)) step = fabsf(offset);
        m_menuYaw += (offset < 0.0f) ? -step : step;
        offset = wrapPi(headYaw - m_menuYaw);
        if (fabsf(offset) < kStopRad) m_menuFollowing = false;
    }

    const XrQuaternionf qYaw = {0.0f, sinf(m_menuYaw * 0.5f), 0.0f, cosf(m_menuYaw * 0.5f)};
    const XrVector3f rotatedFwd = RotateVector(qYaw, XrVector3f{0.0f, 0.0f, -1.5f});
    XrPosef pose{};
    pose.orientation = qYaw;
    pose.position.x = m_menuPivot.x + rotatedFwd.x;
    pose.position.y = m_menuPivot.y + rotatedFwd.y;
    pose.position.z = m_menuPivot.z + rotatedFwd.z;
    m_menuQuadPose = pose;
    return pose;
}

bool OpenXRManager::GetControllerState(VRControllerState* out) const {
    if (!out) return false;
    std::lock_guard<std::mutex> lock(m_inputMutex);
    *out = m_controllerState;
    return true;
}

bool OpenXRManager::GetHeadPose(OpenXRHeadPose* out) const {
    if (!out) return false;

    const bool useSyncedPose = GetSyncSequential() != 0 && m_syncedPoseValid.load(std::memory_order_relaxed);
    out->valid = useSyncedPose ? true : m_poseValid.load(std::memory_order_relaxed);
    out->posX = useSyncedPose ? m_syncedPosX.load(std::memory_order_relaxed) : m_posX.load(std::memory_order_relaxed);
    out->posY = useSyncedPose ? m_syncedPosY.load(std::memory_order_relaxed) : m_posY.load(std::memory_order_relaxed);
    out->posZ = useSyncedPose ? m_syncedPosZ.load(std::memory_order_relaxed) : m_posZ.load(std::memory_order_relaxed);
    out->oriX = useSyncedPose ? m_syncedOriX.load(std::memory_order_relaxed) : m_oriX.load(std::memory_order_relaxed);
    out->oriY = useSyncedPose ? m_syncedOriY.load(std::memory_order_relaxed) : m_oriY.load(std::memory_order_relaxed);
    out->oriZ = useSyncedPose ? m_syncedOriZ.load(std::memory_order_relaxed) : m_oriZ.load(std::memory_order_relaxed);
    out->oriW = useSyncedPose ? m_syncedOriW.load(std::memory_order_relaxed) : m_oriW.load(std::memory_order_relaxed);

    // AER forward pose prediction (1/2-rate pacing): lead the head pose by
    // predictMs using the sampled head velocity, compensating for the high
    // render-to-photon latency when each eye only refreshes every other game
    // frame. Applies in both live and sequential-sync modes so sync (pose
    // consistency, which keeps the runtime's reprojection aligned) and prediction
    // (latency reduction) can be used together for the smoothest head turning.
    // The live velocity barely changes within one pair, so the pair stays
    // effectively consistent.
    const float predictMs = GetMotionPredictMs();
    if (out->valid && predictMs > 0.01f &&
        m_velValid.load(std::memory_order_relaxed)) {
        const float dt = predictMs * 0.001f;
        out->posX += m_linVelX.load(std::memory_order_relaxed) * dt;
        out->posY += m_linVelY.load(std::memory_order_relaxed) * dt;
        out->posZ += m_linVelZ.load(std::memory_order_relaxed) * dt;

        const float wx = m_angVelX.load(std::memory_order_relaxed);
        const float wy = m_angVelY.load(std::memory_order_relaxed);
        const float wz = m_angVelZ.load(std::memory_order_relaxed);
        const float speed = sqrtf(wx * wx + wy * wy + wz * wz);
        const float angle = speed * dt;
        if (angle > 1e-6f) {
            const float halfAngle = angle * 0.5f;
            const float s = sinf(halfAngle) / speed;
            const XrQuaternionf delta{wx * s, wy * s, wz * s, cosf(halfAngle)};
            const XrQuaternionf current{out->oriX, out->oriY, out->oriZ, out->oriW};
            XrQuaternionf predicted = MultiplyQuat(delta, current);
            const float norm = sqrtf(predicted.x * predicted.x + predicted.y * predicted.y +
                                     predicted.z * predicted.z + predicted.w * predicted.w);
            if (norm > 1e-8f) {
                const float invNorm = 1.0f / norm;
                out->oriX = predicted.x * invNorm;
                out->oriY = predicted.y * invNorm;
                out->oriZ = predicted.z * invNorm;
                out->oriW = predicted.w * invNorm;
            }
        }
    }

    if (Get3DofMovement() != 0) {
        out->posX = 0.0f;
        out->posY = 0.0f;
        out->posZ = 0.0f;
    }
    return out->valid;
}

bool OpenXRManager::GetCurrentEyeCenterOffset(int eye, XrVector3f* out) {
    if (!out || eye < 0 || eye > 1) return false;
    std::lock_guard<std::mutex> viewLock(m_viewMutex);
    const bool useSyncedViews = (GetSyncSequential() != 0) &&
        m_syncedPoseValid.load(std::memory_order_relaxed);
    if (!useSyncedViews && m_views.size() < 2) return false;
    const XrPosef& p0 = useSyncedViews ? m_syncedEyePoses[0] : m_views[0].pose;
    const XrPosef& p1 = useSyncedViews ? m_syncedEyePoses[1] : m_views[1].pose;
    const XrVector3f center{
        (p0.position.x + p1.position.x) * 0.5f,
        (p0.position.y + p1.position.y) * 0.5f,
        (p0.position.z + p1.position.z) * 0.5f};
    const XrPosef& pe = (eye == 0) ? p0 : p1;
    out->x = pe.position.x - center.x;
    out->y = pe.position.y - center.y;
    out->z = pe.position.z - center.z;
    return true;
}

bool OpenXRManager::GetCurrentEyeFov(int eye, XrFovf* out) {
    if (!out || eye < 0 || eye > 1) return false;
    std::lock_guard<std::mutex> viewLock(m_viewMutex);
    const bool useSyncedViews = (GetSyncSequential() != 0) &&
        m_syncedPoseValid.load(std::memory_order_relaxed);
    if (!useSyncedViews && static_cast<size_t>(eye) >= m_views.size()) return false;
    *out = useSyncedViews ? m_syncedEyeFovs[eye] : m_views[eye].fov;
    return true;
}

void OpenXRManager::StoreRenderEyePose(int eye, const OpenXRHeadPose& pose, uint32_t seq) {
    if (eye < 0 || eye > 1 || !pose.valid) return;
    // GetHeadPose() returns a base-RECENTERED pose (see the m_basePose math in the
    // frame loop), but the AER submit layer is in raw m_localSpace like m_views.
    // Submitting the recentered pose directly corrupts the compositor's timewarp
    // delta by the base rotation (static shift + bad warp). So undo the recenter
    // here: raw = basePose ?? relative.
    const XrQuaternionf relOri{pose.oriX, pose.oriY, pose.oriZ, pose.oriW};
    const XrVector3f relPos{pose.posX, pose.posY, pose.posZ};
    XrPosef raw;
    std::lock_guard<std::mutex> lock(m_renderPoseMutex);
    if (m_basePoseSet) {
        raw.orientation = MultiplyQuat(m_basePose.orientation, relOri);
        const XrVector3f rotated = RotateVector(m_basePose.orientation, relPos);
        raw.position = {m_basePose.position.x + rotated.x,
                        m_basePose.position.y + rotated.y,
                        m_basePose.position.z + rotated.z};
    } else {
        raw.orientation = relOri;
        raw.position = relPos;
    }
    
    // Store in queue using the exact sequence ID from the game engine
    if (eye == 0 && seq > 0) {
        int idx = seq % 256;
        m_poseQueue[idx] = raw;
        m_poseQueueFrame[idx] = seq;
    }
    
    m_renderEyeHeadPose[eye] = raw;
    m_renderEyeHeadPoseValid[eye] = true;
}

void OpenXRManager::UpdatePairLock() {
    // PIPELINE SHIFT: this snapshot must be taken at the EARLIEST point of the
    // frame timeline — BEFORE the engine's animation pass (the VRIK plugin reads
    // shared memory inside Hooked_ComponentFunc21 during anim eval, which runs
    // before render/LocateCamera). So it is now called from OnPresent at the PAIR
    // BOUNDARY (follower eye), publishing the snapshot for the NEXT pair: both eyes
    // of that pair then animate from this one frozen tracking state -> no per-eye
    // skeleton tear. Sampling it in LocateCamera (during render) was too late —
    // eye0 animated off the previous frame, eye1 off the fresh write -> the body
    // jitter the user saw even on the flat mirror.
    std::lock_guard<std::mutex> lock(m_handMutex);
    OpenXRHeadPose live{};
    GetHeadPose(&live);
    m_pairLockHeadPose = live;
    m_pairLockHeadValid = live.valid;
    m_pairLockHands[0] = m_hands[0];
    m_pairLockHands[1] = m_hands[1];
    m_pairLockHmdOri[0] = m_oriX.load(std::memory_order_relaxed);
    m_pairLockHmdOri[1] = m_oriY.load(std::memory_order_relaxed);
    m_pairLockHmdOri[2] = m_oriZ.load(std::memory_order_relaxed);
    m_pairLockHmdOri[3] = m_oriW.load(std::memory_order_relaxed);
    m_pairLockHmdPosY = m_posY.load(std::memory_order_relaxed);
    m_pairLockHandsValid = true;
}

bool OpenXRManager::GetPairLockedHeadPose(OpenXRHeadPose* out) {
    if (!out) return false;
    std::lock_guard<std::mutex> lock(m_handMutex);
    // Return the snapshot UpdatePairLock froze for this pair (both eyes share it).
    if (m_pairLockHeadValid) {
        *out = m_pairLockHeadPose;
        return out->valid;
    }
    return GetHeadPose(out);  // before the first snapshot
}

void OpenXRManager::FlushHandsToShared() {
    static HANDLE s_hMapFile2 = NULL;
    static float* sShared = nullptr;
    if (!s_hMapFile2) {
        s_hMapFile2 = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 1024, "CyberpunkVR_Hands_Shared");
        if (s_hMapFile2) sShared = (float*)MapViewOfFile(s_hMapFile2, FILE_MAP_ALL_ACCESS, 0, 0, 1024);
    }
    if (!sShared) return;
    std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));

    // Publish hands + HMD orientation to the VRIK plugin. Default to the LIVE pose
    // every flush. xr_pair_lock=1 restores the frozen snapshot for anyone who
    // prefers the anti-tear tradeoff.
    const bool usePairLock = m_pairLockHandsValid && GetVrPairLock() != 0;
    OpenXRHeadPose srcHands[2];
    float hmdOri[4];
    if (usePairLock) {
        srcHands[0] = m_pairLockHands[0];
        srcHands[1] = m_pairLockHands[1];
        hmdOri[0] = m_pairLockHmdOri[0];
        hmdOri[1] = m_pairLockHmdOri[1];
        hmdOri[2] = m_pairLockHmdOri[2];
        hmdOri[3] = m_pairLockHmdOri[3];
    } else {
        srcHands[0] = m_hands[0];
        srcHands[1] = m_hands[1];
        hmdOri[0] = m_oriX.load(std::memory_order_relaxed);
        hmdOri[1] = m_oriY.load(std::memory_order_relaxed);
        hmdOri[2] = m_oriZ.load(std::memory_order_relaxed);
        hmdOri[3] = m_oriW.load(std::memory_order_relaxed);
    }
    const float baseY = usePairLock ? m_pairLockHmdPosY : m_posY.load(std::memory_order_relaxed);

    // ===== SEQLOCK BEGIN (torn-read fix) =====
    // The VRIK plugin reads these pose slots from the engine's animation JOB threads
    // while we write from the present thread; there is no lock spanning the two
    // modules, so a half-written quaternion made the IK solver swing to a garbage arm
    // -> body/hands jitter even at FULL REST. Seqlock: publish an ODD sequence number
    // (= "write in progress"), write all fields, then an EVEN number (= "complete").
    // The reader (plugin) snapshots seq before+after and retries while it is odd or
    // changed, so it only ever consumes a fully consistent frame. Writer never blocks.
    // Slot [127] = sequence counter (well clear of the [0..93] payload).
    volatile uint32_t* seqSlot = reinterpret_cast<volatile uint32_t*>(&sShared[127]);
    const uint32_t seqStart = (m_sharedSeq += 1u);   // odd after the first increment
    *seqSlot = seqStart | 1u;                          // force ODD = write-in-progress
    std::atomic_thread_fence(std::memory_order_release);

    // Left hand [0-7]
    sShared[0] = srcHands[0].valid ? 1.0f : 0.0f;
    sShared[1] = srcHands[0].posX;
    sShared[2] = srcHands[0].posY;
    sShared[3] = srcHands[0].posZ;
    sShared[4] = srcHands[0].oriX;
    sShared[5] = srcHands[0].oriY;
    sShared[6] = srcHands[0].oriZ;
    sShared[7] = srcHands[0].oriW;
    // Right hand [8-15] with weapon offset
    float rx = srcHands[1].posX, ry = srcHands[1].posY, rz = srcHands[1].posZ;
    float rqx = srcHands[1].oriX, rqy = srcHands[1].oriY, rqz = srcHands[1].oriZ, rqw = srcHands[1].oriW;
    float offQx, offQy, offQz, offQw;
    EulerToQuat(m_weaponPitch, m_weaponYaw, m_weaponRoll, offQx, offQy, offQz, offQw);
    float fQx, fQy, fQz, fQw;
    MulQuatLoc(rqx, rqy, rqz, rqw, offQx, offQy, offQz, offQw, fQx, fQy, fQz, fQw);
    float tx = 2.0f * (rqy * m_weaponDz - rqz * m_weaponDy);
    float ty = 2.0f * (rqz * m_weaponDx - rqx * m_weaponDz);
    float tz = 2.0f * (rqx * m_weaponDy - rqy * m_weaponDx);
    float vx = m_weaponDx + rqw * tx + (rqy * tz - rqz * ty);
    float vy = m_weaponDy + rqw * ty + (rqz * tx - rqx * tz);
    float vz = m_weaponDz + rqw * tz + (rqx * ty - rqy * tx);
    sShared[8]  = srcHands[1].valid ? 1.0f : 0.0f;
    sShared[9]  = rx + vx;
    sShared[10] = ry + vy;
    sShared[11] = rz + vz;
    sShared[12] = fQx;
    sShared[13] = fQy;
    sShared[14] = fQz;
    sShared[15] = fQw;
    // HMD relative orientation [16-19]
    sShared[16] = hmdOri[0];
    sShared[17] = hmdOri[1];
    sShared[18] = hmdOri[2];
    sShared[19] = hmdOri[3];

    // [89] physical head height + [90] neck-pivot (false-squat fix) — pose-locked
    // from the SAME frozen snapshot as the hands (baseY computed above), written HERE
    // (early-pipeline, before the next pair's animation) so VRIK body height/squat no
    // longer bobs per eye. These used to be written live every present in OnPresent.
    sShared[89] = baseY;
    {
        XrQuaternionf relOri{ hmdOri[0], hmdOri[1], hmdOri[2], hmdOri[3] };
        const float kOptFwd = 0.15f; // optical centre this far FORWARD of the neck pivot (m)
        const float kOptUp  = 0.08f; // and this far ABOVE it (m)
        XrVector3f optLocal{ 0.0f, kOptUp, -kOptFwd }; // OpenXR head-local: +Y up, -Z forward
        XrVector3f optW = RotateVector(relOri, optLocal);
        sShared[90] = baseY - optW.y;
    }
    // [124..126] full HMD POSITION relative to the recenter base (base axes, OpenXR X/Y/Z).
    // VRIK adds this to the controller-from-base vector (hmdRel * handHmdLocal + this) so the
    // real head TRANSLATION (the ~5-10cm eye/neck lever on head turns, leaning, physical
    // crouch) is part of the hand target -- hands stay room-fixed when the head moves, matching
    // the render view which gets the same translation from dxgi's posScale path.
    sShared[124] = m_posX.load(std::memory_order_relaxed);
    sShared[125] = baseY;
    sShared[126] = m_posZ.load(std::memory_order_relaxed);

    // ===== SEQLOCK END =====
    // All payload slots are written; publish an EVEN sequence (= complete) so readers
    // that snapshot this value (and find it unchanged + even across their read) accept
    // the frame. Release fence first so the payload stores are visible before seq.
    std::atomic_thread_fence(std::memory_order_release);
    *seqSlot = seqStart + 1u;   // seqStart was forced odd -> +1 = EVEN = complete
}

// [OnPresent() moved to openxr_present.cpp]

// [AER V2 worker thread methods (ReleaseAERV2JobRefs / StartAERV2WorkerIfNeeded /
//  StopAERV2Worker / ProcessAERV2Job / AERV2WorkerThreadMain) moved to openxr_aer_v2_worker.cpp]

void OpenXRManager::Shutdown() {
    std::lock_guard<std::mutex> initLock(m_initMutex);
    m_stopFrameThread.store(true, std::memory_order_relaxed);
    NotifyAerThread();   // wake the parked AER thread so it observes the stop flag
    if (m_frameThread) {
        WaitForSingleObject(m_frameThread, 2000);
        CloseHandle(m_frameThread);
        m_frameThread = nullptr;
    }
    StopAERV2Worker();

    if (m_viewSpace != XR_NULL_HANDLE) {
        xrDestroySpace(m_viewSpace);
        m_viewSpace = XR_NULL_HANDLE;
    }
    if (m_localSpace != XR_NULL_HANDLE) {
        xrDestroySpace(m_localSpace);
        m_localSpace = XR_NULL_HANDLE;
    }

    EndSession();

    if (m_session != XR_NULL_HANDLE) {
        xrDestroySession(m_session);
        m_session = XR_NULL_HANDLE;
    }

    for (auto& eye : m_eyeSwapchains) {
        if (eye.handle != XR_NULL_HANDLE) {
            xrDestroySwapchain(eye.handle);
            eye.handle = XR_NULL_HANDLE;
        }
        if (eye.depthHandle != XR_NULL_HANDLE) {
            xrDestroySwapchain(eye.depthHandle);
            eye.depthHandle = XR_NULL_HANDLE;
        }
    }
    m_eyeSwapchains.clear();
    m_views.clear();
    m_viewConfigViews.clear();
    m_runtimeIsSteamVR.store(false, std::memory_order_relaxed);

    if (m_fenceEvent) {
        CloseHandle(m_fenceEvent);
        m_fenceEvent = nullptr;
    }
    if (m_fence) {
        m_fence->Release();
        m_fence = nullptr;
    }
    for (int i = 0; i < 3; ++i) {
        if (m_cmdLists[i]) {
            m_cmdLists[i]->Release();
            m_cmdLists[i] = nullptr;
        }
        if (m_captureCmdLists[i]) {
            m_captureCmdLists[i]->Release();
            m_captureCmdLists[i] = nullptr;
        }
    }
    if (m_captureFenceEvent) {
        CloseHandle(m_captureFenceEvent);
        m_captureFenceEvent = nullptr;
    }
    if (m_monoPresentEvent) {
        CloseHandle(m_monoPresentEvent);
        m_monoPresentEvent = nullptr;
    }
    if (m_frameSyncEvent) {
        CloseHandle(m_frameSyncEvent);
        m_frameSyncEvent = nullptr;
    }
    if (m_captureFence) {
        m_captureFence->Release();
        m_captureFence = nullptr;
    }
    for (int i = 0; i < 3; ++i) {
        if (m_captureCmdAllocators[i]) {
            m_captureCmdAllocators[i]->Release();
            m_captureCmdAllocators[i] = nullptr;
        }
        if (m_cmdAllocators[i]) {
            m_cmdAllocators[i]->Release();
            m_cmdAllocators[i] = nullptr;
        }
    }
    if (m_rtvHeap) {
        m_rtvHeap->Release();
        m_rtvHeap = nullptr;
    }
    if (m_lastPresentedBackBuffer) {
        m_lastPresentedBackBuffer->Release();
        m_lastPresentedBackBuffer = nullptr;
    }
    if (m_d3dQueue) {
        m_d3dQueue->Release();
        m_d3dQueue = nullptr;
    }
    if (m_d3dDevice) {
        m_d3dDevice->Release();
        m_d3dDevice = nullptr;
    }
    if (m_opticalFlow) {
        m_opticalFlow->Shutdown();
    }
    if (m_aerV2Pipeline) {
        m_aerV2Pipeline->Shutdown();
        m_aerV2Pipeline.reset();
    }
    if (m_colorBlit) {
        m_colorBlit->Shutdown();
        m_colorBlit.reset();
    }
    if (m_monoCapturedFrame.texture) {
        m_monoCapturedFrame.texture->Release();
        m_monoCapturedFrame.texture = nullptr;
    }
    if (m_depthSnapshot) {
        m_depthSnapshot->Release();
        m_depthSnapshot = nullptr;
    }
    if (m_depthSnapshotR32) {
        m_depthSnapshotR32->Release();
        m_depthSnapshotR32 = nullptr;
        m_depthSnapshotR32Serial = 0;
    }
    if (m_depthWriterList) { m_depthWriterList->Release(); m_depthWriterList = nullptr; }
    if (m_depthWriterAlloc) { m_depthWriterAlloc->Release(); m_depthWriterAlloc = nullptr; }
    if (m_depthWriterFence) { m_depthWriterFence->Release(); m_depthWriterFence = nullptr; }
    m_depthWriterFenceValue = 0;
    m_depthSnapshotWriterFence = 0;
    m_monoCapturedFrame.width = 0;
    m_monoCapturedFrame.height = 0;
    m_monoCapturedFrame.format = 0;
    m_monoCapturedFrame.serial = 0;
    m_monoCapturedFrame.hasView[0] = false;
    m_monoCapturedFrame.hasView[1] = false;
    m_depthSnapshotW = 0;
    m_depthSnapshotH = 0;
    m_depthSnapshotSerial = 0;
    m_depthSwapchainFormat = 0;
    for (CapturedEyeFrame& frame : m_capturedEyeFrames) {
        if (frame.texture) {
            frame.texture->Release();
            frame.texture = nullptr;
        }
        if (frame.opticalFlowTexture) {
            frame.opticalFlowTexture->Release();
            frame.opticalFlowTexture = nullptr;
        }
        if (frame.depthTexture) {
            frame.depthTexture->Release();
            frame.depthTexture = nullptr;
        }
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.textureShareable = false;
        frame.depthWidth = 0;
        frame.depthHeight = 0;
        frame.depthFormat = 0;
        frame.serial = 0;
        frame.pairId = 0;
        frame.depthSerial = 0;
        frame.depthInCopySource = false;
        frame.pose = {};
        frame.pose.orientation.w = 1.0f;
        frame.fov = {};
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_previousCapturedEyeFrames) {
        if (frame.texture) {
            frame.texture->Release();
            frame.texture = nullptr;
        }
        if (frame.opticalFlowTexture) {
            frame.opticalFlowTexture->Release();
            frame.opticalFlowTexture = nullptr;
        }
        if (frame.depthTexture) {
            frame.depthTexture->Release();
            frame.depthTexture = nullptr;
        }
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.textureShareable = false;
        frame.depthWidth = 0;
        frame.depthHeight = 0;
        frame.depthFormat = 0;
        frame.serial = 0;
        frame.pairId = 0;
        frame.depthSerial = 0;
        frame.depthInCopySource = false;
        frame.pose = {};
        frame.pose.orientation.w = 1.0f;
        frame.fov = {};
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_pendingEyeFrames) {
        if (frame.texture) {
            frame.texture->Release();
            frame.texture = nullptr;
        }
        if (frame.opticalFlowTexture) {
            frame.opticalFlowTexture->Release();
            frame.opticalFlowTexture = nullptr;
        }
        if (frame.depthTexture) {
            frame.depthTexture->Release();
            frame.depthTexture = nullptr;
        }
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.textureShareable = false;
        frame.depthWidth = 0;
        frame.depthHeight = 0;
        frame.depthFormat = 0;
        frame.serial = 0;
        frame.pairId = 0;
        frame.depthSerial = 0;
        frame.depthInCopySource = false;
        frame.pose = {};
        frame.pose.orientation.w = 1.0f;
        frame.fov = {};
        frame.hasView = false;
    }
    for (int eye = 0; eye < 2; ++eye) {
        for (int slot = 0; slot < 2; ++slot) {
            m_aerV2SynthEye[eye][slot].Reset();
            m_aerV2SubmitEye[eye][slot].Reset();
            m_aerV2SubmitEyeReady[eye][slot] = false;
        }
    }

    if (m_instance != XR_NULL_HANDLE) {
        xrDestroyInstance(m_instance);
        m_instance = XR_NULL_HANDLE;
    }
    m_initialized = false;
    m_poseValid.store(false, std::memory_order_relaxed);
    {
        std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
        m_basePoseSet = false;
    }
    m_interpolatedPairId = 0;
    m_interpolatedSynthSlot = 0;
    m_interpolatedSyntheticEye = -1;
    m_interpolatedEyeViewsValid[0] = false;
    m_interpolatedEyeViewsValid[1] = false;
    Log("OpenXRManager: Shutdown complete.\n");
}
