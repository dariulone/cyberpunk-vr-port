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
extern "C" float GetMotionPredictMs();
extern "C" int GetRenderPoseSubmit();
extern "C" int GetDepthSubmit();
extern "C" int GetPoseLag();
extern "C" int GetRenderedCameraEye();
extern "C" uint32_t GetRenderedCameraSeq();
extern "C" int GetXrRuntimeMode();

extern "C" float GetHmdTrackingSmooth()        { return g_hmdTrackingSmooth.load(std::memory_order_relaxed); }
extern "C" void  SetHmdTrackingSmooth(float v) { g_hmdTrackingSmooth.store(v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v), std::memory_order_relaxed); }
extern "C" float GetHandTrackingSmooth()         { return g_handTrackingSmooth.load(std::memory_order_relaxed); }
extern "C" void  SetHandTrackingSmooth(float v)  { g_handTrackingSmooth.store(v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v), std::memory_order_relaxed); }
extern "C" int GetInputActionsEnabled(); // 0 = pose-only legacy behaviour, 1 = full gameplay action set
extern "C" int GetMonoXQueueWait();      // 0 = mono path skips cross-queue Wait (kills hang); 1 = legacy depth-safe behaviour
extern "C" int GetMonoDepthCapture();    // 0 = mono path skips depth capture entirely (kills CP2077 mono hang); 1 = legacy depth-aware reprojection
extern "C" float GetHmdTrackingSmooth();
// 1 = smooth the pose that comes back from a PREDICTED locate. This is the path the camera is
// composed from; unsmoothed, the runtime's velocity integration arrives in the view as noise.
// See LocateHeadPoseAt.
extern "C" __declspec(dllexport) int CyberpunkVR_PredictFilter = 1;
extern "C" float GetHandTrackingSmooth();
// Implemented in swapchain_hooks.cpp. Issues GPU-side ID3D12CommandQueue::
// Wait() on the consumer queue for every tracked game queue's latest Signal —
// so a subsequent CopyResource on that consumer queue cannot race the game's
// render-side writer. No CPU stall. See xr_depth_submit cross-queue notes.
extern "C" void CyberpunkVRPort_WaitOnAllGameSignals(ID3D12CommandQueue* consumerQueue);

// [SetD3DName / SetD3DNamef moved to openxr_internal.h (inline)]

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

// [ContainsSwapchainFormat / PickMonoSwapchainFormat moved to openxr_internal.h (inline)]

// [ApplyForcedProjectionFov moved to openxr_internal.h (inline)]

// Reuse-last-frame output path. When enabled, submit re-presents the last clean eye on
// stale ticks instead of shipping stale content again.
// [ReuseLastFrameOutputEnabled() moved to openxr_internal.h (inline)]

DWORD WINAPI OpenXRManager::FrameThreadThunk(LPVOID param) {
    return static_cast<OpenXRManager*>(param)->FrameThreadMain();
}

DWORD WINAPI OpenXRManager::SubmitThreadThunk(LPVOID param) {
    return static_cast<OpenXRManager*>(param)->SubmitThreadMain();
}

void OpenXRManager::NotifySubmitThread() {
    m_submitThreadWakeCv.notify_all();
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

// Dedicated submit thread. Parks (~0% CPU) while the inline Present pump owns the loop;
// when UseThreadedSubmit() turns true it takes the loop and drives FrameThreadMain in a
// tight loop that is self-paced by xrWaitFrame (~90 Hz), independent of the game's
// Present rate.
DWORD OpenXRManager::SubmitThreadMain() {
    Log("OpenXRManager: VR submit thread started (dormant until threaded submit is wanted).\n");
    while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
        {
            std::unique_lock<std::mutex> lock(m_submitThreadMutex);
            // Wake on UseThreadedSubmit() ALONE (do NOT require m_sessionRunning). The
            // session only becomes running via BeginSession, which runs inside
            // FrameThreadMain->PollEvents, so gating this thread on sessionRunning would
            // mean nobody pumps the session when threaded submit is on from launch -> the
            // OpenXR session never reaches READY -> VR never starts. Waking on the flag
            // alone lets this thread BOOTSTRAP the session.
            m_submitThreadWakeCv.wait_for(lock, std::chrono::milliseconds(200), [this]() {
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
        if (!AcquireFrameLoop(FrameLoopOwner::Threaded, 100)) {
            continue;
        }
        Log("OpenXRManager: VR submit thread acquired frame loop (steamvr=%d mono=%d).\n",
            IsRuntimeSteamVR() ? 1 : 0,
            m_monoSubmitEnabled.load(std::memory_order_relaxed) ? 1 : 0);
        // FrameThreadMain self-handles the not-yet-running phase (PollEvents -> session
        // state advances -> BeginSession), so this loop bootstraps the session and then
        // drives submit once running (self-paced by xrWaitFrame).
        while (!m_stopFrameThread.load(std::memory_order_relaxed) &&
               UseThreadedSubmit()) {
            FrameThreadMain();
        }
        ReleaseFrameLoop(FrameLoopOwner::Threaded);
        Log("OpenXRManager: VR submit thread released frame loop (parking).\n");
    }
    Log("OpenXRManager: VR submit thread exiting.\n");
    return 0;
}

OpenXRManager& OpenXRManager::Get() {
    static OpenXRManager instance;
    return instance;
}

// [recenter / auto-calibration / calibration-file methods (RotateBaseYaw ... LoadCalibrationFromFile) moved to openxr_calibration.cpp]

void OpenXRManager::SetMonoSubmitEnabled(bool enabled) {
    m_monoSubmitEnabled.store(enabled, std::memory_order_relaxed);
    // Wake the dedicated submit thread: it owns the loop for mono whenever
    // UseThreadedSubmit() is true, so enabling mono must un-park it promptly.
    NotifySubmitThread();
    if (m_monoPresentEvent) {
        ResetEvent(m_monoPresentEvent);
    }
    std::lock_guard<std::mutex> lock(m_presentMutex);
    m_monoCapturedFrame.serial = 0;
    m_monoCapturedFrame.hasView[0] = false;
    m_monoCapturedFrame.hasView[1] = false;
    m_depthSnapshotSerial = 0;
}

// [mono capture + submit-resource methods (EnsureMonoCaptureResource ... EnsureMonoSubmitResources) moved to openxr_capture.cpp]

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
    // Also probes XR_HTCX_vive_tracker_interaction (SteamVR) for leg tracking.
    bool viveTrackerExtAdvertised = false;
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
                }
#ifdef XR_HTCX_vive_tracker_interaction
                if (strcmp(p.extensionName, XR_HTCX_VIVE_TRACKER_INTERACTION_EXTENSION_NAME) == 0) {
                    viveTrackerExtAdvertised = true;
                }
#endif
            }
        }
        Log("OpenXRManager: depth-layer (XR_KHR_composition_layer_depth) supported=%d\n", m_depthLayerSupported ? 1 : 0);
    }
#ifdef XR_HTCX_vive_tracker_interaction
    if (viveTrackerExtAdvertised) {
        extensions.push_back(XR_HTCX_VIVE_TRACKER_INTERACTION_EXTENSION_NAME);
        Log("OpenXRManager: XR_HTCX_vive_tracker_interaction advertised -- enabling for leg trackers.\n");
    } else {
        Log("OpenXRManager: XR_HTCX_vive_tracker_interaction NOT advertised by the runtime (leg trackers need SteamVR).\n");
    }
#endif

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

    // HTC Vive trackers (leg tracking): resolves xrEnumerateViveTrackerPathsHTCX
    // and logs connected trackers/roles. No-op when the extension wasn't enabled.
    InitViveTrackerSupport();
    // OpenVR fallback provider (Panda trackers etc.): SteamVR only enumerates
    // genuine HTC trackers through the HTCX extension; emulated trackers are
    // read through the OpenVR API instead. No-op unless the runtime is SteamVR.
    InitOpenVRTrackerSupport();

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

        // HTC Vive trackers: one pose action for the two foot roles + binding
        // suggestion for the vive_tracker_htcx profile. Independent of the
        // gameplay-input kill switch (pose-only, like the hands).
        CreateViveTrackerActions();

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

        // HTC Vive tracker foot spaces (re-tried from the poll, so trackers that
        // connect or get a role assigned mid-session are picked up live).
        EnsureViveTrackerSpaces();
    }

    m_stopFrameThread.store(false, std::memory_order_relaxed);
    if (!m_frameThread) {
        m_frameThread = CreateThread(nullptr, 0, &OpenXRManager::SubmitThreadThunk, this, 0, nullptr);
        if (m_frameThread) {
            Log("OpenXRManager: VR submit thread created (dormant).\n");
        } else {
            Log("OpenXRManager: WARNING failed to create the VR submit thread (err=%lu); threaded submit unavailable.\n", GetLastError());
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
    NotifySubmitThread();   // in case threaded submit was wanted before the session started
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

// LOCATE THE HEAD AT THE MOMENT OF THE WRITE, FOR THE FRAME BEING WRITTEN.
//
// GetHeadPose() below returns atomics that a DIFFERENT thread refreshes once per XR cycle. Read
// from the camera write, that value is anywhere from 0 to a full frame old, and -- this is the
// part that matters -- its age changes from frame to frame, because the XR cycle and the game's
// camera update are two free-running loops at nearly the same rate. The pose is not wrong; its
// staleness is unsteady, and unsteady staleness is exactly what judder is. It cannot be filtered
// out downstream and it hits both eyes equally, because both are written from one composition.
//
// RealVR never reads a cached pose here. update_cp2077_head_pose_and_fov_state takes a sequence
// number and calls locate_or_fake_headset_poses(seq) synchronously at the camera hook, which
// reaches fetch_backend_pose_frame -> `predicted = seq*period + base` -> xrLocateSpace at that
// predicted time. The pose is therefore always the same age relative to the frame it drives.
// This is that call: one xrLocateSpace, at the write, aimed at the display time predicted for the
// frame being built.
//
// Cheap and legal off the render thread: xrLocateSpace is a query, it does not block on the
// compositor, and OpenXR requires it to be callable concurrently. It runs once per present
// interval (the composition is already gated by a compare-exchange), not once per camera.
bool OpenXRManager::LocateHeadPoseAt(XrTime displayTime, OpenXRHeadPose* out) {
    if (!out || !m_session || m_viewSpace == XR_NULL_HANDLE || m_localSpace == XR_NULL_HANDLE) {
        return false;
    }
    if (displayTime <= 0 || !m_sessionRunning.load(std::memory_order_relaxed)) return false;

    XrSpaceLocation loc{XR_TYPE_SPACE_LOCATION};
    if (XR_FAILED(xrLocateSpace(m_viewSpace, m_localSpace, displayTime, &loc))) return false;
    constexpr XrSpaceLocationFlags wanted =
        XR_SPACE_LOCATION_POSITION_VALID_BIT | XR_SPACE_LOCATION_ORIENTATION_VALID_BIT;
    if ((loc.locationFlags & wanted) != wanted) return false;

    // Same recenter transform the frame loop applies, so this pose is interchangeable with
    // GetHeadPose()'s: relOri = conj(base.ori) * raw, relPos = conj(base.ori) * (raw - base.pos).
    XrPosef base{};
    {
        std::lock_guard<std::mutex> lock(m_renderPoseMutex);
        if (!m_basePoseSet) return false;
        base = m_basePose;
    }
    const XrQuaternionf baseInv = ConjugateQuat(base.orientation);
    const XrVector3f relWorld{ loc.pose.position.x - base.position.x,
                               loc.pose.position.y - base.position.y,
                               loc.pose.position.z - base.position.z };
    const XrVector3f relPos = RotateVector(baseInv, relWorld);
    const XrQuaternionf relOri = MultiplyQuat(baseInv, loc.pose.orientation);

    // FILTER THE PREDICTED POSE, HERE, because this is the path the camera is actually built
    // from. GetHeadPose()'s filtered cache does not reach the view at all while OneSamplePerFrame
    // is set -- the camera write calls this function instead, and until now it handed the pose
    // back exactly as the runtime extrapolated it.
    //
    // And extrapolate it does: the aim time is ahead of now, so xrLocateSpace integrates velocity
    // to get there, and on a headset that is not moving there is nothing to integrate but sensor
    // noise. Measured, that term was 0.30..0.33 mm per read against 0.04..0.06 for the underlying
    // filtered pose -- an order of magnitude, arriving fresh every frame.
    //
    // The same adaptive follow the head filter uses, deliberately: it barely smooths while the
    // head turns, where the lead is worth having, and clamps down hard at rest, where the lead is
    // pure noise. A plain low-pass would hand the latency straight back.
    //
    // Its own state, not the head filter's -- this runs on the game thread at the game's rate,
    // the other on the XR thread at the headset's. Guarded because AcquireFrameHeadSample can
    // reach here from either camera hook.
    if (CyberpunkVR_PredictFilter) {
        static std::mutex s_mtx;
        static bool s_init = false;
        static XrVector3f s_pos{};
        static XrQuaternionf s_ori{0.0f, 0.0f, 0.0f, 1.0f};
        const float strength = GetHmdTrackingSmooth();
        std::lock_guard<std::mutex> lock(s_mtx);
        if (!s_init || strength <= 0.001f) {
            s_init = true;
            s_pos = relPos;
            s_ori = relOri;
        } else {
            // 1/(1+20*strength) of the step while still, rising to the whole step once it passes
            // the release threshold -- the shape of adaptiveFollow() in the frame loop.
            const auto follow = [strength](float delta, float quiet, float release) {
                if (release <= quiet) return 1.0f;
                float t = (delta - quiet) / (release - quiet);
                if (t < 0.0f) t = 0.0f;
                if (t > 1.0f) t = 1.0f;
                const float motion = t * t * (3.0f - 2.0f * t);
                const float still = 1.0f / (1.0f + 20.0f * strength);
                return still + (1.0f - still) * motion;
            };
            const float dx = relPos.x - s_pos.x;
            const float dy = relPos.y - s_pos.y;
            const float dz = relPos.z - s_pos.z;
            const float posDelta = sqrtf(dx * dx + dy * dy + dz * dz);
            float dot = s_ori.x * relOri.x + s_ori.y * relOri.y +
                        s_ori.z * relOri.z + s_ori.w * relOri.w;
            if (dot < 0.0f) dot = -dot;
            if (dot > 1.0f) dot = 1.0f;
            const float angDelta = 2.0f * acosf(dot);
            const float posT = follow(posDelta, 0.0012f, 0.0080f);
            const float angT = follow(angDelta, 0.0035f, 0.0350f);
            s_pos.x += dx * posT;
            s_pos.y += dy * posT;
            s_pos.z += dz * posT;
            s_ori = NlerpQuat(s_ori, relOri, angT);
        }
        out->valid = true;
        out->posX = s_pos.x;  out->posY = s_pos.y;  out->posZ = s_pos.z;
        out->oriX = s_ori.x;  out->oriY = s_ori.y;  out->oriZ = s_ori.z;  out->oriW = s_ori.w;
        return true;
    }

    out->valid = true;
    out->posX = relPos.x;  out->posY = relPos.y;  out->posZ = relPos.z;
    out->oriX = relOri.x;  out->oriY = relOri.y;  out->oriZ = relOri.z;  out->oriW = relOri.w;
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

    // MOTION PREDICTION REMOVED (xr_motion_predict_ms).
    //
    // It added `linear velocity * predictMs` to the head position and the matching angular sweep
    // to its orientation, on every call -- and AFTER the filter, so a smoothed pose was handed an
    // unsmoothed term on top. The position going in is filtered; the velocity never was, and at
    // rest an IMU-derived linear velocity wanders by millimetres per second. At the 50.9 ms the
    // shipped ini actually carried, that is ~0.3 mm of fresh jitter per read.
    //
    // Prediction still happens, in the two places that can do it correctly: the runtime, when the
    // locate is aimed at a display time -- and that result is now filtered, see LocateHeadPoseAt
    // -- and the compositor, which reprojects to photon time regardless. A third prediction of our
    // own, off raw velocity, only ever added noise. The ini key and its live control stay so saved
    // settings still round-trip; nothing reads the value any more.



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
    // frame loop), but the submit layer is in raw m_localSpace like m_views.
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
    for (int i = 0; i < kBodyTrackerCount; ++i) m_pairLockTrackers[i] = m_trackers[i];
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

// Filled by the camera locate; see the comment there. Same DLL, so these are read directly.
extern volatile float g_anchorOff[3];
extern volatile float g_anchorCy, g_anchorSy, g_anchorScale;
extern volatile int   g_anchorRecipeValid;
extern "C" __declspec(dllexport) int CyberpunkVR_CoherentHandAnchor;
// 1 = resample the published hand/head pose to the publish instant instead of shipping the
// tracking loop's last sample. Live switch, so the two can be compared by feel.

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
    OpenXRHeadPose srcTrack[kBodyTrackerCount];
    float hmdOri[4];
    if (usePairLock) {
        srcHands[0] = m_pairLockHands[0];
        srcHands[1] = m_pairLockHands[1];
        for (int i = 0; i < kBodyTrackerCount; ++i) srcTrack[i] = m_pairLockTrackers[i];
        hmdOri[0] = m_pairLockHmdOri[0];
        hmdOri[1] = m_pairLockHmdOri[1];
        hmdOri[2] = m_pairLockHmdOri[2];
        hmdOri[3] = m_pairLockHmdOri[3];
    } else {
        srcHands[0] = m_hands[0];
        srcHands[1] = m_hands[1];
        for (int i = 0; i < kBodyTrackerCount; ++i) srcTrack[i] = m_trackers[i];
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
    // MEASURED BUG, fixed 2026-07-30: this published a COMPLETE marker that was odd on every
    // second flush, and the reader -- correctly -- treats odd as "write in progress", retries
    // eight times and keeps its previous snapshot. Half of all hand publishes were therefore
    // thrown away: the census read handPub=28.0/s against present=56.5/s, and the pose the arms
    // solved from averaged 30 ms old with 61 ms peaks. That is the VRIK judder.
    //
    // The old arithmetic: seqStart|1 made the START odd but left seqStart itself alone, so the
    // END wrote seqStart+1 -- even only when seqStart happened to be odd. Counting in twos makes
    // both markers correct by construction and every even value strictly new, which is what the
    // reader's "changed since my last latch" test needs.
    m_sharedSeq += 2u;
    const uint32_t seqStart = m_sharedSeq | 1u;        // always ODD, always advancing
    *seqSlot = seqStart;                               // ODD = write-in-progress
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
    // [112..114] the view anchor built from THIS sample's head position, [115] = valid.
    // The head pose (m_posX/Y/Z) and the controller poses are written by the same XR loop
    // iteration, so combining them reconstructs the hand's true world position; combining the
    // controllers with the render packet's older head pose does not, and that error is what the
    // arms were shaking by. Everything else in the expression is the slow recipe from the locate.
    if (g_anchorRecipeValid && CyberpunkVR_CoherentHandAnchor && !usePairLock &&
        m_handSampleHeadValid) {
        const float sc = g_anchorScale;
        const float lr =  m_handSampleHeadPos[0] * sc + g_anchorOff[0];
        const float lf = -m_handSampleHeadPos[2] * sc + g_anchorOff[1];
        const float lu =  m_handSampleHeadPos[1] * sc + g_anchorOff[2];
        sShared[112] = g_anchorCy * lr - g_anchorSy * lf;
        sShared[113] = g_anchorSy * lr + g_anchorCy * lf;
        sShared[114] = lu;
        sShared[115] = 1.0f;
    } else {
        sShared[115] = 0.0f;
    }
    // [67] the sample stamp, INSIDE the seqlock so it travels with the pose it belongs to.
    // Outside it the reader would pick up the newest stamp against an older pose and report an
    // age that is too small -- the one number this census exists to get right.
    sShared[67] = m_handSampleMs.load(std::memory_order_relaxed);
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

    // [157..172] body trackers: foot poses in the SAME HMD-local convention
    // as the hands ([0..15]), so the plugin drops them into the same gizmo frame.
    // [173] enable, [174..176] measured leg anatomy, [177] count, [178..180] mount
    // euler. [181..188] waist tracker, same convention (drives hip placement).
    // Written INSIDE the hands seqlock so the plugin's legs latch shares the
    // hands frame's consistency guarantee (no torn foot quats).
    sShared[157] = srcTrack[0].valid ? 1.0f : 0.0f;
    sShared[158] = srcTrack[0].posX;
    sShared[159] = srcTrack[0].posY;
    sShared[160] = srcTrack[0].posZ;
    sShared[161] = srcTrack[0].oriX;
    sShared[162] = srcTrack[0].oriY;
    sShared[163] = srcTrack[0].oriZ;
    sShared[164] = srcTrack[0].oriW;
    sShared[165] = srcTrack[1].valid ? 1.0f : 0.0f;
    sShared[166] = srcTrack[1].posX;
    sShared[167] = srcTrack[1].posY;
    sShared[168] = srcTrack[1].posZ;
    sShared[169] = srcTrack[1].oriX;
    sShared[170] = srcTrack[1].oriY;
    sShared[171] = srcTrack[1].oriZ;
    sShared[172] = srcTrack[1].oriW;
    sShared[173] = (m_legTrackersEnable.load(std::memory_order_relaxed) != 0) ? 1.0f : 0.0f;
    // fix15: kick-damage checkbox. Lives outside the [157..189] tracker block
    // ([140], reclaimed graveyard) but rides the same seqlocked publish.
    sShared[vrshared::kKickDamageEnable] =
        (m_legKickDamageEnable.load(std::memory_order_relaxed) != 0) ? 1.0f : 0.0f;
    sShared[174] = m_userLegLen.load(std::memory_order_relaxed);
    sShared[175] = m_legAnkleOffset.load(std::memory_order_relaxed);
    sShared[176] = (m_legLenValid.load(std::memory_order_relaxed) != 0) ? 1.0f : 0.0f;
    sShared[177] = static_cast<float>(m_viveTrackerCount.load(std::memory_order_relaxed));
    // [178..180] no longer carry the dead euler mount sliders (removed in
    // fix6, never consumed since) -- they hold the ACTIVE right foot mount
    // quat (hemisphere-packed) now, published in the mount block below.
    sShared[181] = srcTrack[2].valid ? 1.0f : 0.0f;
    sShared[182] = srcTrack[2].posX;
    sShared[183] = srcTrack[2].posY;
    sShared[184] = srcTrack[2].posZ;
    sShared[185] = srcTrack[2].oriX;
    sShared[186] = srcTrack[2].oriY;
    sShared[187] = srcTrack[2].oriZ;
    sShared[188] = srcTrack[2].oriW;
    sShared[189] = (m_waistTrackerEnable.load(std::memory_order_relaxed) != 0) ? 1.0f : 0.0f;

    // [190] T-pose mount-calibration sampling flag (openxr -> plugin). While
    // high, the plugin solves per-foot YAW-ONLY mount corrections (the
    // tracker's own orientation yaw-rotated so the toe lands on bodyFwd;
    // pitch/roll stay with the tracker 1:1); on the falling edge it publishes
    // them to [191..199] plus diagnostics on [233..249].
    sShared[190] = (m_mountCalibSampling.load(std::memory_order_relaxed) != 0) ? 1.0f : 0.0f;

    // Adopt the plugin's foot-rotation solve (runs in the short window after
    // a T-pose finalise). Timeout keeps a stale hands DLL (no sampler) from
    // leaving us waiting forever.
    if (m_mountSolveAwaiting) {
        const float seq = sShared[199];
        if (seq != m_mountSolveConsumedSeq) {
            auto adoptQ = [](const float* sh, int base, std::atomic<float>* dst) {
                float q[4] = { sh[base], sh[base+1], sh[base+2], sh[base+3] };
                const float n2 = q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3];
                if (n2 > 0.49f && n2 < 2.0f) {
                    const float inv = 1.0f / sqrtf(n2);
                    for (int i = 0; i < 4; ++i) dst[i].store(q[i] * inv, std::memory_order_relaxed);
                }
            };
            adoptQ(sShared, 191, m_legMountQuatL);
            adoptQ(sShared, 195, m_legMountQuatR);
            m_mountSolveConsumedSeq = seq;
            m_mountSolveAwaiting = false;
            Log("Auto-calibration[FBT]: foot rotation calibrated -- L mount (%.3f, %.3f, %.3f, %.3f), R mount (%.3f, %.3f, %.3f, %.3f).\n",
                m_legMountQuatL[0].load(std::memory_order_relaxed), m_legMountQuatL[1].load(std::memory_order_relaxed),
                m_legMountQuatL[2].load(std::memory_order_relaxed), m_legMountQuatL[3].load(std::memory_order_relaxed),
                m_legMountQuatR[0].load(std::memory_order_relaxed), m_legMountQuatR[1].load(std::memory_order_relaxed),
                m_legMountQuatR[2].load(std::memory_order_relaxed), m_legMountQuatR[3].load(std::memory_order_relaxed));
            // Solve diagnostics ([233..249]): the yaw-corrected tracker
            // orientation each foot was bound to (fix10: the tracker's own
            // orientation rotated about world +Z so the toe lands on body
            // forward -- pitch/roll stay with the tracker), the measured RAW
            // animation toe directions, and the signed yaw correction applied.
            // toe (0,0,0) + angle 0 = no toe bone resolved, raw animation
            // orientation was used as the target (old behaviour).
            if (sShared[249] == 1.0f) {
                // fix11 note: the adopted mounts now also carry the boot-mesh
                // visual fix (plugin-side, angles from vrik_footfix.ini), so
                // they are no longer pure Z/yaw rotations -- x/y components
                // are expected. The "yaw correction" below is still only the
                // skeleton toe-heading part of the solve.
                Log("Auto-calibration[FBT]: foot-rotation diag -- "
                    "target L (%.3f, %.3f, %.3f, %.3f) R (%.3f, %.3f, %.3f, %.3f), "
                    "anim toe L (%.3f, %.3f, %.3f) R (%.3f, %.3f, %.3f), "
                    "yaw correction L %+.1f deg R %+.1f deg.\n",
                    sShared[233], sShared[234], sShared[235], sShared[236],
                    sShared[237], sShared[238], sShared[239], sShared[240],
                    sShared[241], sShared[242], sShared[243],
                    sShared[244], sShared[245], sShared[246],
                    sShared[247], sShared[248]);
            }
            SaveCalibrationToFile();
        } else if (FbtNowSeconds() - m_mountSolveAwaitStart > 2.0) {
            m_mountSolveAwaiting = false;
            Log("Auto-calibration[FBT]: no foot-rotation solve received (old hands DLL or trackers off) -- keeping previous mount.\n");
        }
    }
    // [137..139] / [178..180] ACTIVE per-foot mount correction quats
    // (openxr -> plugin), HEMISPHERE-PACKED: x,y,z stored with w >= 0, the
    // reader recovers w = +sqrt(1-|xyz|^2) and fails safe to identity on
    // garbage. fix10 slot history: the first assignment ([208..215], full
    // quats) collided with the plugin's legacy VRIK solve-stats block, which
    // stomped the mounts with per-frame counters and left the feet running
    // uncorrected (fix9 probe finding). These two 3-float holes are verified
    // free by grep: [137..139] had its writer removed sessions ago, and
    // [178..180] carried the dead euler mount sliders removed in fix6.
    auto publishMount = [&](int slot, const std::atomic<float>* src, const std::atomic<float>* adj) {
        float q[4] = { src[0].load(std::memory_order_relaxed), src[1].load(std::memory_order_relaxed),
                       src[2].load(std::memory_order_relaxed), src[3].load(std::memory_order_relaxed) };
        const float n2 = q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3];
        if (n2 < 0.25f || n2 > 4.0f) { q[0] = q[1] = q[2] = 0.0f; q[3] = 1.0f; }
        else { const float inv = 1.0f / sqrtf(n2); for (int i = 0; i < 4; ++i) q[i] *= inv; }
        // fix12: manual per-foot trim from the F10 sliders, stacked on the
        // calibrated mount as a WORLD-space rotation (left-multiply):
        //   Q = Rx(pitch) * Ry(roll) * Rz(yaw)   (yaw about world up, then roll
        //   about body forward, then pitch about body right). Standing, that
        //   reads exactly like the slider labels: +yaw turns the toes left,
        //   +roll tips the boot's top to your right, +pitch lifts the toes.
        const float yawDeg   = adj[0].load(std::memory_order_relaxed);
        const float pitchDeg = adj[1].load(std::memory_order_relaxed);
        const float rollDeg  = adj[2].load(std::memory_order_relaxed);
        if (yawDeg != 0.0f || pitchDeg != 0.0f || rollDeg != 0.0f) {
            const float ky = yawDeg   * 0.00872664626f;   // pi/360 (half-angle)
            const float kp = pitchDeg * 0.00872664626f;
            const float kr = rollDeg  * 0.00872664626f;
            const float qy[4] = { 0.0f, 0.0f, sinf(ky), cosf(ky) };
            const float qp[4] = { sinf(kp), 0.0f, 0.0f, cosf(kp) };
            const float qr[4] = { 0.0f, sinf(kr), 0.0f, cosf(kr) };
            auto qmul = [](const float* a, const float* b, float* o) {
                o[0] = a[3]*b[0] + a[0]*b[3] + a[1]*b[2] - a[2]*b[1];
                o[1] = a[3]*b[1] - a[0]*b[2] + a[1]*b[3] + a[2]*b[0];
                o[2] = a[3]*b[2] + a[0]*b[1] - a[1]*b[0] + a[2]*b[3];
                o[3] = a[3]*b[3] - a[0]*b[0] - a[1]*b[1] - a[2]*b[2];
            };
            float t[4], u[4];
            qmul(qr, qy, t);          // un-yaw first
            qmul(qp, t, u);           // then roll, then pitch
            qmul(u, q, t);
            q[0] = t[0]; q[1] = t[1]; q[2] = t[2]; q[3] = t[3];
        }
        if (q[3] < 0.0f) { q[0] = -q[0]; q[1] = -q[1]; q[2] = -q[2]; }   // w >= 0 hemisphere
        sShared[slot] = q[0]; sShared[slot + 1] = q[1]; sShared[slot + 2] = q[2];
    };
    publishMount(137, m_legMountQuatL, m_legFootAdjDegL);
    publishMount(178, m_legMountQuatR, m_legFootAdjDegR);

    // ===== SEQLOCK END =====
    // All payload slots are written; publish an EVEN sequence (= complete) so readers
    // that snapshot this value (and find it unchanged + even across their read) accept
    // the frame. Release fence first so the payload stores are visible before seq.
    std::atomic_thread_fence(std::memory_order_release);
    *seqSlot = seqStart + 1u;   // seqStart IS odd -> +1 = EVEN = complete, and never repeats
}

// [OnPresent() moved to openxr_present.cpp]


void OpenXRManager::Shutdown() {
    std::lock_guard<std::mutex> initLock(m_initMutex);
    m_stopFrameThread.store(true, std::memory_order_relaxed);
    NotifySubmitThread();   // wake the parked thread so it observes the stop flag
    if (m_frameThread) {
        WaitForSingleObject(m_frameThread, 2000);
        CloseHandle(m_frameThread);
        m_frameThread = nullptr;
    }

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
    if (m_vrcamEyeTex) {
        m_vrcamEyeTex->Release();
        m_vrcamEyeTex = nullptr;
        m_vrcamEyeW = m_vrcamEyeH = m_vrcamEyeFmt = 0;
        m_vrcamEyeSerial = 0;
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
    Log("OpenXRManager: Shutdown complete.\n");
}
