#include "openxr_manager.h"
#include "ngx_hook.h"
#include "runtime_fov_correction.h"
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <vector>
#include <dxgi1_4.h>
#include <cstring>
#include <cmath>
#include <utility>

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
extern "C" float GetMenuFov();
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
// Lightweight atomics (g_verboseLog pattern). PERSISTED to vrport.ini by the
// dxgi proxy (it reads the Get* accessors on Save and pushes parsed values back
// via Set* on load). The overlay reads/writes them via Get/Set; the frame thread
// + warp kernel read them each frame.
static std::atomic<float> g_aerMaxExtrap{1.8f};   // forward-extrapolation cap (smear↔frozen on turns)
// Idea #4 — quality boost defaults. The kernel's agreement-gates on MV + pose
// flow make a MODERATE refine factor stable enough on CP2077 while noticeably
// sharpening the stale eye vs raw NvOF-only. Keep it conservative: the user can
// still dial it down to 0 in the overlay/ini if a scene exposes artifacts.
static std::atomic<float> g_aerRefineStrength{0.35f}; // 0..1 MV+pose refine
static std::atomic<float> g_aerOcclusionSharp{1.15f}; // >1 slightly crisps occlusion edges
// Fixed-foveation (no eye-tracking): fraction of the lens-edge radius that uses
// a CHEAP warp (deep periphery = raw nearest frame, no bidirectional/refine).
// 0 = off (full-quality everywhere). 0.4 = outer 40% of the radius simplified.
static std::atomic<float> g_aerFoveation{0.0f};
// Idea #2 — synced-pose freshness. syncSequential(default on) freezes the head
// pose per stereo pair so both eyes render coherently (no per-eye tear), but
// that adds up to ~1 pair (2 vsyncs) of pose lag on head turns. g_poseBlend is
// a per-present low-pass nudge of the synced pose toward the live pose: 0.0 =
// fully frozen (current behavior, max coherence), 1.0 = tracks live every
// present (min lag, ~mono smoothness, slight per-eye delta). ~0.35 is a good
// middle ground: cuts head-turn pose lag noticeably while keeping stereo
// coherence. Applied EVERY present (the full snap at the pair boundary still
// runs as the hard reset).
static std::atomic<float> g_poseBlend{0.35f};
// Idea #3 — NvOF flow temporal smoothing. Stale-eye warp quality on motion is
// limited by per-frame NvOF flow noise. g_flowSmooth EMA-blends each eye's flow
// toward its previous-frame flow: 0.0 = off (raw NvOF every frame), up to ~0.6
// (heavier smoothing, stabler but can trail on fast cuts). Reduces the
// stale-eye "shimmer" on textured/animated surfaces.
static std::atomic<float> g_flowSmooth{0.35f};
static std::atomic<float> g_hmdTrackingSmooth{0.35f};
static std::atomic<float> g_handTrackingSmooth{0.45f};
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

static constexpr uint64_t kAERV2FlowWarmupPairId = 300;
static constexpr float kAERV2FrameGenPoseT = 0.5f;

static DXGI_FORMAT GetAERV2OpticalFlowFormat(DXGI_FORMAT sourceFormat) {
    switch (sourceFormat) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
        return DXGI_FORMAT_B8G8R8A8_UNORM;
    default:
        return sourceFormat;
    }
}

static void SetD3DName(ID3D12Object* object, const wchar_t* name) {
    if (object && name) {
        object->SetName(name);
    }
}

static void SetD3DNamef(ID3D12Object* object, const wchar_t* format, ...) {
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


static XrQuaternionf MultiplyQuat(const XrQuaternionf& a, const XrQuaternionf& b) {
    XrQuaternionf out{};
    out.x = a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y;
    out.y = a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x;
    out.z = a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w;
    out.w = a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z;
    return out;
}

static XrQuaternionf ConjugateQuat(const XrQuaternionf& q) {
    XrQuaternionf out{ -q.x, -q.y, -q.z, q.w };
    return out;
}

static XrQuaternionf NlerpQuat(const XrQuaternionf& a, const XrQuaternionf& b, float t) {
    float bx = b.x;
    float by = b.y;
    float bz = b.z;
    float bw = b.w;
    const float dot = a.x * bx + a.y * by + a.z * bz + a.w * bw;
    if (dot < 0.0f) {
        bx = -bx;
        by = -by;
        bz = -bz;
        bw = -bw;
    }

    XrQuaternionf out{
        a.x + (bx - a.x) * t,
        a.y + (by - a.y) * t,
        a.z + (bz - a.z) * t,
        a.w + (bw - a.w) * t};
    const float norm = sqrtf(out.x * out.x + out.y * out.y + out.z * out.z + out.w * out.w);
    if (norm > 1e-8f) {
        const float invNorm = 1.0f / norm;
        out.x *= invNorm;
        out.y *= invNorm;
        out.z *= invNorm;
        out.w *= invNorm;
    } else {
        out = a;
    }
    return out;
}

static XrPosef ExtrapolatePose(const XrPosef& previous, const XrPosef& current, float t) {
    XrPosef out{};
    out.orientation = NlerpQuat(previous.orientation, current.orientation, t);
    out.position.x = previous.position.x + (current.position.x - previous.position.x) * t;
    out.position.y = previous.position.y + (current.position.y - previous.position.y) * t;
    out.position.z = previous.position.z + (current.position.z - previous.position.z) * t;
    return out;
}

static XrVector3f RotateVector(const XrQuaternionf& q, const XrVector3f& v) {
    const XrQuaternionf pure{v.x, v.y, v.z, 0.0f};
    const XrQuaternionf rotated = MultiplyQuat(MultiplyQuat(q, pure), ConjugateQuat(q));
    return {rotated.x, rotated.y, rotated.z};
}

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

static bool ContainsSwapchainFormat(const std::vector<int64_t>& formats, int64_t candidate) {
    for (const int64_t format : formats) {
        if (format == candidate) {
            return true;
        }
    }
    return false;
}

static int64_t PickMonoSwapchainFormat(const std::vector<int64_t>& runtimeFormats, int64_t gameFormat, bool preferSrgbForVD) {
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

static XrFovf ApplyForcedProjectionFov(const XrFovf& sourceFov, const XrFovf* pairFovs, int eyeIndex, float width, float height) {
    float forceFov = GetForcedFov();
    if (forceFov <= 1.0f || forceFov >= 170.0f) {
        // Default path = keep the runtime frusta, with quarter-angle asymmetry
        // compensation when both eyes are available.
        if (pairFovs && eyeIndex >= 0 && eyeIndex <= 1) {
            const RuntimeFovCorrection corr = ComputeRuntimeFovCorrection(pairFovs[0], pairFovs[1]);
            return corr.eye[eyeIndex];
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

// Reuse-last-frame output path. When enabled, the AER submit path re-submits the
// last clean eye on stale ticks instead of warping stale content again.
static bool ReuseLastFrameOutputEnabled() {
    return GetReuseLastFrameOutput() != 0;
}

DWORD WINAPI OpenXRManager::FrameThreadThunk(LPVOID param) {
    return static_cast<OpenXRManager*>(param)->FrameThreadMain();
}

OpenXRManager& OpenXRManager::Get() {
    static OpenXRManager instance;
    return instance;
}

void OpenXRManager::RequestRecenter() {
    m_recenterRequested.store(true, std::memory_order_relaxed);
}

// ==== AUTO-CALIBRATION ====
//
// Procedure (matches standard VR title flow):
//   1. User clicks Start; HUD shows "Stretch arms out to the SIDES and stand straight. 3..2..1".
//   2. While sampling we read live HMD + both controller HMD-local gizmo positions, keep max armSpan.
//   3. After secs seconds we derive anatomical numbers and publish them via SetVRHandCalib +
//      SetShoulderAnatomical, then save to file.
//
// Body proportions used:
//   * armSpan ~= height (Da Vinci) -> we use armSpan as the calibration baseline.
//   * shoulder half-width  ~= 0.135 * armSpan  (from human anthropometry tables)
//   * HMD -> shoulder backward depth ~= 0.04 * armSpan (eyes are ahead of neck)
//   * arm length is measured directly from calibrated shoulder to controller/gizmo wrist
void OpenXRManager::StartAutoCalibration(float secs) {
    m_calibSeconds.store(secs, std::memory_order_relaxed);
    m_calibProgress.store(0.0f, std::memory_order_relaxed);
    m_calibArmSpanMax = 0.0f;
    m_calibHmdHeightSum = 0.0f;
    m_calibSampleCount = 0;
    m_calibCtrlPosSumR[0]=m_calibCtrlPosSumR[1]=m_calibCtrlPosSumR[2]=0.0f;
    m_calibCtrlPosSumL[0]=m_calibCtrlPosSumL[1]=m_calibCtrlPosSumL[2]=0.0f;
    m_calibStart = 0.0;
    // INITIALIZE wrist defaults if they've never been set. Without this, finalisation reads zero
    // values from m_calib[] (atomic float default = 0), publishes identity wrist quats, and the
    // hand orientation breaks (palm faces wrong way). These match the plugin's baked-in defaults
    // (g_VRWristR_* and g_VRWristL_*) so the user sees the same wrist behaviour as before auto-cal.
    if (m_calib[0].load(std::memory_order_relaxed) == 0.0f
        && m_calib[1].load(std::memory_order_relaxed) == 0.0f
        && m_calib[9].load(std::memory_order_relaxed) == 0.0f) {
        m_calib[0].store(1.05f, std::memory_order_relaxed);   // scaleR
        m_calib[1].store(1.06f, std::memory_order_relaxed);   // scaleL
        m_calib[2].store(0.0f,  std::memory_order_relaxed);   // heightR
        m_calib[3].store(0.0f,  std::memory_order_relaxed);   // heightL
        m_calib[4].store(1.0f,  std::memory_order_relaxed);   // swingR
        m_calib[5].store(1.0f,  std::memory_order_relaxed);   // swingL
        m_calib[6].store(0.0f,  std::memory_order_relaxed);   // poleR
        m_calib[7].store(0.0f,  std::memory_order_relaxed);   // poleL
        m_calib[8].store(0.0f,    std::memory_order_relaxed); // wRp
        m_calib[9].store(-90.0f,  std::memory_order_relaxed); // wRy
        m_calib[10].store(0.0f,   std::memory_order_relaxed); // wRr
        m_calib[11].store(-180.0f,std::memory_order_relaxed); // wLp
        m_calib[12].store(-90.0f, std::memory_order_relaxed); // wLy
        m_calib[13].store(0.0f,   std::memory_order_relaxed); // wLr
        Log("Auto-calibration: initialised wrist/elbow defaults (R yaw=-90, L pitch=-180 yaw=-90).\n");
    }
    m_calibState.store(1, std::memory_order_relaxed);
    Log("Auto-calibration: started (%.1fs T-pose sample). Stretch arms STRAIGHT OUT to the sides.\n", secs);
}

void OpenXRManager::TickAutoCalibration() {
    if (m_calibState.load(std::memory_order_relaxed) != 1) return;

    // Sim time in seconds (use QueryPerformanceCounter for steady clock; the frame loop runs
    // continuously while VR is active so this is monotonic).
    LARGE_INTEGER now, freq;
    QueryPerformanceCounter(&now);
    QueryPerformanceFrequency(&freq);
    double t = static_cast<double>(now.QuadPart) / static_cast<double>(freq.QuadPart);
    if (m_calibStart == 0.0) m_calibStart = t;
    float elapsed = static_cast<float>(t - m_calibStart);
    float total = m_calibSeconds.load(std::memory_order_relaxed);
    float prog = (total > 0.0f) ? (elapsed / total) : 1.0f;
    if (prog > 1.0f) prog = 1.0f;
    m_calibProgress.store(prog, std::memory_order_relaxed);

    // Sample current frame (under hand-mutex for atomicity with the frame loop writer).
    {
        std::lock_guard<std::mutex> lock(m_handMutex);
        if (m_hands[0].valid && m_hands[1].valid) {
            float dx = m_hands[1].posX - m_hands[0].posX;
            float dy = m_hands[1].posY - m_hands[0].posY;
            float dz = m_hands[1].posZ - m_hands[0].posZ;
            float span = std::sqrt(dx*dx + dy*dy + dz*dz);
            if (span > m_calibArmSpanMax) m_calibArmSpanMax = span;
            // Accumulate HMD-local controller positions so we know where each hand actually sits
            // (so X-sign tells us which is left/right -> calibration auto-detects swapped sticks).
            m_calibCtrlPosSumR[0] += m_hands[1].posX;
            m_calibCtrlPosSumR[1] += m_hands[1].posY;
            m_calibCtrlPosSumR[2] += m_hands[1].posZ;
            m_calibCtrlPosSumL[0] += m_hands[0].posX;
            m_calibCtrlPosSumL[1] += m_hands[0].posY;
            m_calibCtrlPosSumL[2] += m_hands[0].posZ;
            m_calibHmdHeightSum += m_posY.load(std::memory_order_relaxed);
            m_calibSampleCount++;
        }
    }

    if (elapsed >= total) {
        // Finalise.
        float armSpan = m_calibArmSpanMax;
        if (armSpan < 0.5f || armSpan > 2.5f || m_calibSampleCount == 0) {
            Log("Auto-calibration: armSpan %.3fm out of plausible range — aborting.\n", armSpan);
            m_calibState.store(0, std::memory_order_relaxed);
            return;
        }
        float invN = 1.0f / static_cast<float>(m_calibSampleCount);
        float avgR[3] = { m_calibCtrlPosSumR[0]*invN, m_calibCtrlPosSumR[1]*invN, m_calibCtrlPosSumR[2]*invN };
        float avgL[3] = { m_calibCtrlPosSumL[0]*invN, m_calibCtrlPosSumL[1]*invN, m_calibCtrlPosSumL[2]*invN };

        // SHOULDER ANATOMICAL OFFSETS. These are body-frame OpenXR axes (X right, Y up,
        // Z back), sampled from the same gizmo/controller coordinates used for hand drawing.
        const float kShoulderHalfWidth = 0.105f;     // X | wrist-span to shoulder half-width; keep torso narrow
        const float kShoulderBackFromHmd = 0.05f;    // Z | eyes sit slightly in front of shoulders
        float shoulderHalf = kShoulderHalfWidth * armSpan;
        if (shoulderHalf < 0.10f) shoulderHalf = 0.10f;
        if (shoulderHalf > 0.20f) shoulderHalf = 0.20f;
        float rightSign = (avgR[0] >= avgL[0]) ? 1.0f : -1.0f;
        float rx = shoulderHalf * rightSign;
        float lx = -shoulderHalf * rightSign;
        // Y: both shoulders should share one height. Use the higher T-pose hand as the shoulder
        // level so a tired/lowered arm does not pull that shoulder down and shorten reach.
        float shoulderY = (avgR[1] > avgL[1]) ? avgR[1] : avgL[1];
        if (shoulderY > -0.12f) shoulderY = -0.12f;
        if (shoulderY < -0.30f) shoulderY = -0.30f;
        float ry = shoulderY;
        float lyv = shoulderY;
        float rz  = kShoulderBackFromHmd;
        float lzv = kShoulderBackFromHmd;
        SetShoulderAnatomical(rx, ry, rz, lx, lyv, lzv);

        // Arm-scale: realArmLen / modelArmLen. Real arm length is measured from the calibrated
        // shoulder pivot to the visible controller/gizmo wrist in the T-pose.
        auto len3 = [](float ax, float ay, float az, float bx, float by, float bz) -> float {
            float dx = ax - bx, dy = ay - by, dz = az - bz;
            return std::sqrt(dx*dx + dy*dy + dz*dz);
        };
        // Arm length from the ARM SPAN (max controller-to-controller distance over the T-pose),
        // not the per-frame AVERAGE controller position. The average is blurred by non-T-pose
        // frames and systematically UNDER-reads (diag showed 0.37 m for a ~0.55 m arm); the span
        // max captures the best fully-extended frame. armLen = (span - shoulderWidth) / 2.
        float spanArm = (armSpan - 2.0f * shoulderHalf) * 0.5f;
        if (spanArm < 0.40f) spanArm = 0.40f; if (spanArm > 0.85f) spanArm = 0.85f;
        // SYMMETRIC: real arms are the same length. The old per-hand normalisation from the blurred
        // averages produced a fake asymmetry (e.g. 0.62 vs 0.51), which made one hand under-reach
        // (the short side ended up "in the belt textures"). Use the span length for both.
        (void)len3;
        float realArmLenR = spanArm;
        float realArmLenL = spanArm;
        float realArmLen = spanArm;
        // The controller/gizmo coordinates are already in the same meter-like space consumed by
        // the solver. Keep the global scale near the proven defaults and only use T-pose length
        // asymmetry for small per-hand correction; dividing by an approximate model arm length
        // over-shrank targets and caused relaxed arms to keep an elbow bend.
        // Position-scale is GONE. With the gizmo-exact 1:1 hand target the hand sits on the
        // real controller position; avatar proportions are matched by scaling the arm BONES to
        // the measured arm length (plugin VRIK_ArmScale), not by stretching the target. Publish
        // the measured anatomy (arm length per hand + eye height) into [77..80] instead.
        float eyeHeight = (m_calibSampleCount > 0) ? (m_calibHmdHeightSum / static_cast<float>(m_calibSampleCount)) : 0.0f;
        m_userArmLenR.store(realArmLenR, std::memory_order_relaxed);
        m_userArmLenL.store(realArmLenL, std::memory_order_relaxed);
        m_userEyeHeight.store(eyeHeight, std::memory_order_relaxed);
        m_measureValid.store(1, std::memory_order_relaxed);
        // Legacy modes (1..3) + the head-relative fallback still read a reach scale; keep it
        // neutral (1.0) so they are unaffected by the new measured-length path.
        float scaleR = 1.0f;
        float scaleL = 1.0f;
        (void)realArmLen;

        // PRESERVE all user-tunable values: swing, pole, wrist orientation. Auto-cal only
        // overwrites the anatomy (scale + shoulder offsets) — elbow/wrist tweaks stay.
        float swingR = m_calib[4].load(std::memory_order_relaxed);
        float swingL = m_calib[5].load(std::memory_order_relaxed);
        float poleR  = m_calib[6].load(std::memory_order_relaxed);
        float poleL  = m_calib[7].load(std::memory_order_relaxed);
        float wRp = m_calib[8].load(std::memory_order_relaxed);
        float wRy = m_calib[9].load(std::memory_order_relaxed);
        float wRr = m_calib[10].load(std::memory_order_relaxed);
        float wLp = m_calib[11].load(std::memory_order_relaxed);
        float wLy = m_calib[12].load(std::memory_order_relaxed);
        float wLr = m_calib[13].load(std::memory_order_relaxed);
        // Re-apply current swing/pole/wrist with new scale.
        if (swingR == 0.0f && swingL == 0.0f) { swingR = 1.0f; swingL = 1.0f; }
        SetVRHandCalib(scaleR, scaleL, 0.0f, 0.0f,
                       swingR, swingL, poleR, poleL,
                       wRp, wRy, wRr, wLp, wLy, wLr);

        SaveCalibrationToFile();
        Log("Auto-calibration DONE.\n");
        Log("  armSpan = %.3fm  armLenR/L = %.3f/%.3fm  eyeHeight = %.3fm\n", armSpan, realArmLenR, realArmLenL, eyeHeight);
        Log("  ctrl R world-local avg = (%.3f, %.3f, %.3f)\n", avgR[0], avgR[1], avgR[2]);
        Log("  ctrl L world-local avg = (%.3f, %.3f, %.3f)\n", avgL[0], avgL[1], avgL[2]);
        Log("  shoulder R = (%.3f, %.3f, %.3f)\n", rx, ry, rz);
        Log("  shoulder L = (%.3f, %.3f, %.3f)\n", lx, lyv, lzv);

        // Auto-apply the camera->head bake (no separate button needed): the user stood straight
        // in the T-pose, so the published head-vs-camera offset is exactly what we want to bake.
        BakeCameraOffset();

        // Auto-recenter at the end of calibration so the player's body forward direction matches
        // the OpenXR forward they just used during the T-pose. Without this the just-measured
        // shoulder anatomy is in the calibration frame but the runtime's tracking frame
        // may have drifted slightly.
        RequestRecenter();

        m_calibState.store(2, std::memory_order_relaxed);
        m_calibProgress.store(1.0f, std::memory_order_relaxed);
    }
}

// Path is next to dxgi.dll (same dir as the OpenXR config).
static void GetCalibFilePath(char* out, size_t outSize) {
    HMODULE self = nullptr;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       reinterpret_cast<LPCSTR>(&GetCalibFilePath), &self);
    char dir[MAX_PATH] = {0};
    if (self) {
        GetModuleFileNameA(self, dir, MAX_PATH);
        char* slash = strrchr(dir, '\\');
        if (slash) *slash = 0;
    }
    if (dir[0] == 0) strcpy_s(dir, MAX_PATH, ".");
    _snprintf_s(out, outSize, _TRUNCATE, "%s\\vrik_calibration.ini", dir);
}

void OpenXRManager::BakeCameraOffset() {
    // The plugin publishes the (head bone - camera) offset into shared [85..87] (game-local
    // right/forward/up) with [88]=valid. Capture it as the baked camera offset so LocateCamera
    // shifts the FPP view back onto the avatar's head. SET semantics (not accumulate): the FPP
    // camera component the plugin samples does NOT include this LocateCamera offset, so the
    // published value stays the true mount and re-baking is idempotent.
    float* sh = m_sharedHandsPtr;
    if (!sh) return;
    if (sh[88] == 0.0f) {
        Log("BakeCameraOffset: no published offset yet (start VR tracking + calibrate first).\n");
        return;
    }
    float x = sh[85], y = sh[86], z = sh[87];
    // Clamp to a sane range so a bad frame can't fling the camera.
    auto clamp = [](float v) { return v < -0.8f ? -0.8f : (v > 0.8f ? 0.8f : v); };
    m_camBakeOffset[0].store(clamp(x), std::memory_order_relaxed);
    m_camBakeOffset[1].store(clamp(y), std::memory_order_relaxed);
    m_camBakeOffset[2].store(clamp(z), std::memory_order_relaxed);
    Log("BakeCameraOffset: baked (%.3f, %.3f, %.3f) right/fwd/up.\n", clamp(x), clamp(y), clamp(z));
    SaveCalibrationToFile();
}

bool OpenXRManager::SaveCalibrationToFile() {
    char path[MAX_PATH];
    GetCalibFilePath(path, MAX_PATH);
    FILE* f = nullptr;
    if (fopen_s(&f, path, "w") != 0 || !f) {
        Log("SaveCalibration: failed to open %s\n", path);
        return false;
    }
    fprintf(f, "# CyberpunkVRPort VRIK auto-calibration\n");
    fprintf(f, "version=3\n");
    fprintf(f, "scaleR=%.4f\nscaleL=%.4f\n",
            m_calib[0].load(std::memory_order_relaxed),
            m_calib[1].load(std::memory_order_relaxed));
    fprintf(f, "heightR=%.4f\nheightL=%.4f\n",
            m_calib[2].load(std::memory_order_relaxed),
            m_calib[3].load(std::memory_order_relaxed));
    fprintf(f, "swingR=%.4f\nswingL=%.4f\n",
            m_calib[4].load(std::memory_order_relaxed),
            m_calib[5].load(std::memory_order_relaxed));
    fprintf(f, "poleR=%.4f\npoleL=%.4f\n",
            m_calib[6].load(std::memory_order_relaxed),
            m_calib[7].load(std::memory_order_relaxed));
    fprintf(f, "wRp=%.4f\nwRy=%.4f\nwRr=%.4f\n",
            m_calib[8].load(std::memory_order_relaxed),
            m_calib[9].load(std::memory_order_relaxed),
            m_calib[10].load(std::memory_order_relaxed));
    fprintf(f, "wLp=%.4f\nwLy=%.4f\nwLr=%.4f\n",
            m_calib[11].load(std::memory_order_relaxed),
            m_calib[12].load(std::memory_order_relaxed),
            m_calib[13].load(std::memory_order_relaxed));
    fprintf(f, "shoulderRX=%.4f\nshoulderRY=%.4f\nshoulderRZ=%.4f\n",
            m_calibExt[0].load(std::memory_order_relaxed),
            m_calibExt[1].load(std::memory_order_relaxed),
            m_calibExt[2].load(std::memory_order_relaxed));
    fprintf(f, "shoulderLX=%.4f\nshoulderLY=%.4f\nshoulderLZ=%.4f\n",
            m_calibExt[3].load(std::memory_order_relaxed),
            m_calibExt[4].load(std::memory_order_relaxed),
            m_calibExt[5].load(std::memory_order_relaxed));
    fprintf(f, "camBakeX=%.4f\ncamBakeY=%.4f\ncamBakeZ=%.4f\n",
            m_camBakeOffset[0].load(std::memory_order_relaxed),
            m_camBakeOffset[1].load(std::memory_order_relaxed),
            m_camBakeOffset[2].load(std::memory_order_relaxed));
    fclose(f);
    Log("Calibration saved -> %s\n", path);
    return true;
}

bool OpenXRManager::LoadCalibrationFromFile() {
    char path[MAX_PATH];
    GetCalibFilePath(path, MAX_PATH);
    FILE* f = nullptr;
    if (fopen_s(&f, path, "r") != 0 || !f) return false;
    float v[14] = {
        m_calib[0].load(std::memory_order_relaxed),
        m_calib[1].load(std::memory_order_relaxed),
        m_calib[2].load(std::memory_order_relaxed),
        m_calib[3].load(std::memory_order_relaxed),
        m_calib[4].load(std::memory_order_relaxed),
        m_calib[5].load(std::memory_order_relaxed),
        m_calib[6].load(std::memory_order_relaxed),
        m_calib[7].load(std::memory_order_relaxed),
        m_calib[8].load(std::memory_order_relaxed),
        m_calib[9].load(std::memory_order_relaxed),
        m_calib[10].load(std::memory_order_relaxed),
        m_calib[11].load(std::memory_order_relaxed),
        m_calib[12].load(std::memory_order_relaxed),
        m_calib[13].load(std::memory_order_relaxed),
    };
    float e[6] = {
        m_calibExt[0].load(std::memory_order_relaxed),
        m_calibExt[1].load(std::memory_order_relaxed),
        m_calibExt[2].load(std::memory_order_relaxed),
        m_calibExt[3].load(std::memory_order_relaxed),
        m_calibExt[4].load(std::memory_order_relaxed),
        m_calibExt[5].load(std::memory_order_relaxed),
    };
    float cb[3] = {
        m_camBakeOffset[0].load(std::memory_order_relaxed),
        m_camBakeOffset[1].load(std::memory_order_relaxed),
        m_camBakeOffset[2].load(std::memory_order_relaxed),
    };
    int version = 0;
    char line[128];
    while (fgets(line, sizeof(line), f)) {
        char key[32]; float val;
        if (sscanf_s(line, "%31[^=]=%f", key, (unsigned)_countof(key), &val) != 2) continue;
        if (strcmp(key, "version") == 0) version = static_cast<int>(val);
        #define M(name, idx) if (strcmp(key, name) == 0) v[idx] = val;
        #define E(name, idx) if (strcmp(key, name) == 0) e[idx] = val;
        #define C(name, idx) if (strcmp(key, name) == 0) cb[idx] = val;
        M("scaleR",0) M("scaleL",1) M("heightR",2) M("heightL",3)
        M("swingR",4) M("swingL",5) M("poleR",6) M("poleL",7)
        M("wRp",8) M("wRy",9) M("wRr",10) M("wLp",11) M("wLy",12) M("wLr",13)
        E("shoulderRX",0) E("shoulderRY",1) E("shoulderRZ",2)
        E("shoulderLX",3) E("shoulderLY",4) E("shoulderLZ",5)
        C("camBakeX",0) C("camBakeY",1) C("camBakeZ",2)
        #undef M
        #undef E
        #undef C
    }
    fclose(f);
    if (version < 2) {
        v[2] = 0.0f;
        v[3] = 0.0f;
    }
    if (version < 3) {
        if (v[0] < 0.90f || v[0] > 1.25f) v[0] = 1.05f;
        if (v[1] < 0.90f || v[1] > 1.25f) v[1] = 1.06f;
        float shoulderY = (e[1] > e[4]) ? e[1] : e[4];
        if (shoulderY > -0.12f) shoulderY = -0.12f;
        if (shoulderY < -0.30f) shoulderY = -0.30f;
        e[1] = shoulderY;
        e[4] = shoulderY;
    }
    SetVRHandCalib(v[0],v[1],v[2],v[3],v[4],v[5],v[6],v[7],v[8],v[9],v[10],v[11],v[12],v[13]);
    SetShoulderAnatomical(e[0],e[1],e[2],e[3],e[4],e[5]);
    SetCameraOffset(cb[0], cb[1], cb[2]);
    Log("Calibration loaded <- %s\n", path);
    return true;
}

void OpenXRManager::SetMonoSubmitEnabled(bool enabled) {
    m_monoSubmitEnabled.store(enabled, std::memory_order_relaxed);
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

bool OpenXRManager::EnsureMonoCaptureResource(const D3D12_RESOURCE_DESC& sourceDesc) {
    if (!m_d3dDevice || !m_d3dQueue) {
        return false;
    }

    const uint32_t width = static_cast<uint32_t>(sourceDesc.Width);
    const uint32_t height = sourceDesc.Height;
    const uint32_t format = static_cast<uint32_t>(sourceDesc.Format);
    if (width == 0 || height == 0 || format == 0) {
        return false;
    }

    if (!m_captureCmdAllocators[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_captureCmdAllocators[i])))) {
                Log("OpenXRManager: Failed to create mono capture command allocator %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdAllocators[i], L"OpenXR_capture_allocator");
        }
    }
    if (!m_captureCmdLists[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_captureCmdAllocators[i], nullptr, IID_PPV_ARGS(&m_captureCmdLists[i])))) {
                Log("OpenXRManager: Failed to create mono capture command list %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdLists[i], L"OpenXR_capture_command_list");
            m_captureCmdLists[i]->Close();
        }
    }
    if (!m_captureFence) {
        if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_captureFence)))) {
            Log("OpenXRManager: Failed to create mono capture fence\n");
            return false;
        }
        SetD3DName(m_captureFence, L"OpenXR_capture_fence");
    }
    if (!m_captureFenceEvent) {
        m_captureFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!m_captureFenceEvent) {
            Log("OpenXRManager: Failed to create mono capture fence event\n");
            return false;
        }
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        if (m_monoCapturedFrame.texture &&
            m_monoCapturedFrame.width == width &&
            m_monoCapturedFrame.height == height &&
            m_monoCapturedFrame.format == format) {
            return true;
        }

        if (m_monoCapturedFrame.texture) {
            m_monoCapturedFrame.texture->Release();
            m_monoCapturedFrame.texture = nullptr;
        }
        m_monoCapturedFrame.width = 0;
        m_monoCapturedFrame.height = 0;
        m_monoCapturedFrame.format = 0;
        m_monoCapturedFrame.serial = 0;
        m_monoCapturedFrame.hasView[0] = false;
        m_monoCapturedFrame.hasView[1] = false;
    }

    D3D12_HEAP_PROPERTIES heapProps{};
    heapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
    ID3D12Resource* texture = nullptr;
    if (FAILED(m_d3dDevice->CreateCommittedResource(
            &heapProps,
            D3D12_HEAP_FLAG_NONE,
            &sourceDesc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            nullptr,
            IID_PPV_ARGS(&texture)))) {
        Log("OpenXRManager: Failed to create mono captured texture\n");
        return false;
    }
    SetD3DName(texture, L"OpenXR_mono_snapshot_color");

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        m_monoCapturedFrame.texture = texture;
        m_monoCapturedFrame.width = width;
        m_monoCapturedFrame.height = height;
        m_monoCapturedFrame.format = format;
        m_monoCapturedFrame.serial = 0;
        m_monoCapturedFrame.hasView[0] = false;
        m_monoCapturedFrame.hasView[1] = false;
    }

    Log("OpenXRManager: Mono snapshot resource ready. size=%ux%u format=%u\n", width, height, format);
    return true;
}

// [DEPTH] Accessors implemented in dxgi_factory_wrapper.cpp — the game's pinned
// scene depth resource and its CURRENT (observed) D3D12 resource state.
extern "C" ID3D12Resource* OmoGetSceneDepthResource();
extern "C" unsigned int OmoGetSceneDepthState();
extern "C" unsigned int OmoGetSceneDepthWidth();
extern "C" unsigned int OmoGetSceneDepthHeight();
extern "C" unsigned int OmoGetSceneDepthFormat();

bool OpenXRManager::EnsureDepthSnapshot(ID3D12Resource* gameDepth) {
    if (!gameDepth || !m_d3dDevice) {
        return false;
    }
    // Depth submit is OFF by default (xr_depth_submit=0). Copying the game's LIVE
    // scene-depth resource on our capture queue races the game's own queue (which is
    // simultaneously writing DepthPrepass/GBuffer and reallocating render targets on
    // load/spawn). That cross-queue access caused GPU device-hung (0x887a0006) under
    // VDXR, where the scene depth is an R32-family format the snapshot path accepts.
    // depth gave no confirmed benefit (the left-eye fix was the alternate-eye pose-pair
    // lock, not depth), so keep it gated unless explicitly re-enabled for experiments.
    if (GetDepthSubmit() == 0) {
        if (m_depthLayerSupported) {
            Log("OpenXRManager: [DEPTH] depth submit disabled (xr_depth_submit=0)\n");
        }
        m_depthLayerSupported = false;
        m_depthSwapchainFormat = 0;
        m_depthSnapshotSerial = 0;
        return false;
    }
    const D3D12_RESOURCE_DESC desc = gameDepth->GetDesc();
    // Accept both R32 (32bpp) and R32G8X24 (64bpp) source families. The 64bpp
    // path uses DepthResolve shader to extract plane 0 (the float depth) into
    // a 32bpp D32_FLOAT snapshot which is bit-compatible with the standard
    // depth swapchain — no more DEVICE_HUNG, no more sceneindentation depth=0
    // in submit logs. Old comment about "TYPELESS snapshot required" is
    // obsolete: now the snapshot is typed D32_FLOAT, populated by shader.
    const bool acceptable32 =
        desc.Format == DXGI_FORMAT_R32_TYPELESS ||
        desc.Format == DXGI_FORMAT_D32_FLOAT ||
        desc.Format == DXGI_FORMAT_R32_FLOAT;
    const bool acceptable64 =
        desc.Format == DXGI_FORMAT_R32G8X24_TYPELESS ||
        desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
        desc.Format == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
        desc.Format == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
    if (!acceptable32 && !acceptable64) {
        if (m_depthLayerSupported) {
            Log("OpenXRManager: [DEPTH] disabling depth layer for unsupported source format=%u\n",
                static_cast<unsigned>(desc.Format));
        }
        m_depthLayerSupported = false;
        m_depthSwapchainFormat = 0;
        m_depthSnapshotSerial = 0;
        return false;
    }
    // Snapshot always D32_FLOAT 32bpp now. For R32-family sources the capture
    // path uses CopyTextureRegion (bit-compat). For 64bpp sources the capture
    // path uses DepthResolve shader (plane 0 extract). Either way, downstream
    // depth swapchain copy works with a single typed format.
    const DXGI_FORMAT snapshotFormat = DXGI_FORMAT_D32_FLOAT;
    // Ensure the CUDA-importable R32 depth exists whenever AER V2 is on, even when
    // the D32 snapshot already exists (otherwise the early-return below skipped its
    // creation and depth-aware silently fell back to depth-free).
    auto ensureR32 = [&](const D3D12_RESOURCE_DESC& d) {
        if (GetAERV2Enabled() == 0) return;
        if (m_depthSnapshotR32) {
            const auto cur = m_depthSnapshotR32->GetDesc();
            if (cur.Width == d.Width && cur.Height == d.Height) return;
            m_depthSnapshotR32->Release();
            m_depthSnapshotR32 = nullptr;
            m_depthSnapshotR32Serial = 0;
        }
        D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
        D3D12_RESOURCE_DESC rd = d;
        rd.Format = DXGI_FORMAT_R32_FLOAT;
        rd.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE cv{}; cv.Format = DXGI_FORMAT_R32_FLOAT;
        if (FAILED(m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_SHARED, &rd,
                D3D12_RESOURCE_STATE_COMMON, &cv, IID_PPV_ARGS(&m_depthSnapshotR32)))) {
            Log("OpenXRManager: [DEPTH-AERV2] R32 snapshot create failed (depth-aware off)\n");
            m_depthSnapshotR32 = nullptr;
        } else {
            SetD3DName(m_depthSnapshotR32, L"AERV2_scene_depth_R32_cuda");
            Log("OpenXRManager: [DEPTH-AERV2] R32 depth snapshot created %llux%u\n",
                static_cast<unsigned long long>(d.Width), d.Height);
        }
    };
    if (m_depthSnapshot) {
        const D3D12_RESOURCE_DESC cur = m_depthSnapshot->GetDesc();
        if (cur.Width == desc.Width && cur.Height == desc.Height && cur.Format == snapshotFormat) {
            ensureR32(desc);
            return true;
        }
        m_depthSnapshot->Release();
        m_depthSnapshot = nullptr;
        m_depthSnapshotSerial = 0;
    }
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC sd = desc;
    sd.Format = snapshotFormat;
    sd.Flags = D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    // Typed depth resources with ALLOW_DEPTH_STENCIL require a clear value.
    D3D12_CLEAR_VALUE clearVal{};
    clearVal.Format = snapshotFormat;
    clearVal.DepthStencil.Depth = 1.0f;
    clearVal.DepthStencil.Stencil = 0;
    const HRESULT hr = m_d3dDevice->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &sd,
        D3D12_RESOURCE_STATE_COPY_DEST, &clearVal, IID_PPV_ARGS(&m_depthSnapshot));
    if (FAILED(hr)) {
        Log("OpenXRManager: [DEPTH] CreateCommittedResource(depthSnapshot) failed hr=0x%08X\n", hr);
        m_depthSnapshot = nullptr;
        return false;
    }
    m_depthSnapshotW = static_cast<uint32_t>(desc.Width);
    m_depthSnapshotH = desc.Height;
    m_depthSnapshotSerial = 0;
    SetD3DName(m_depthSnapshot, L"OpenXR_scene_depth_snapshot");
    Log("OpenXRManager: [DEPTH] snapshot created %llux%u srcFmt=%u snapFmt=%u\n",
        static_cast<unsigned long long>(desc.Width), desc.Height,
        static_cast<unsigned>(desc.Format), static_cast<unsigned>(snapshotFormat));

    // [DEPTH-AERV2] Parallel R32_FLOAT COLOR snapshot (CUDA-importable). Only the
    // AER V2 warp uses it; failure is non-fatal (warp falls back to depth-free).
    ensureR32(desc);
    return true;
}

bool OpenXRManager::RecordDepthCapture(ID3D12GraphicsCommandList* cmdList,
                                       ID3D12Resource* gameDepth,
                                       D3D12_RESOURCE_STATES gameDepthState) {
    if (!cmdList || !gameDepth || !m_depthSnapshot) return false;
    const DXGI_FORMAT srcFmt = gameDepth->GetDesc().Format;
    const bool is64bpp =
        srcFmt == DXGI_FORMAT_R32G8X24_TYPELESS ||
        srcFmt == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
        srcFmt == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
        srcFmt == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;

    if (!is64bpp) {
        // 32bpp path: simple CopyTextureRegion (bit-compat between R32/D32).
        D3D12_RESOURCE_BARRIER pre[2] = {};
        UINT preCount = 0;
        if (gameDepthState != D3D12_RESOURCE_STATE_COPY_SOURCE) {
            pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            pre[preCount].Transition.pResource = gameDepth;
            pre[preCount].Transition.StateBefore = gameDepthState;
            pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
            pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            ++preCount;
        }
        if (m_depthSnapshotSerial != 0) {
            pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            pre[preCount].Transition.pResource = m_depthSnapshot;
            pre[preCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
            pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            ++preCount;
        }
        if (preCount > 0) cmdList->ResourceBarrier(preCount, pre);
        D3D12_TEXTURE_COPY_LOCATION dst{};
        dst.pResource = m_depthSnapshot;
        dst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        dst.SubresourceIndex = 0;
        D3D12_TEXTURE_COPY_LOCATION src{};
        src.pResource = gameDepth;
        src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        src.SubresourceIndex = 0;
        cmdList->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        D3D12_RESOURCE_BARRIER post[2] = {};
        UINT postCount = 0;
        post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        post[postCount].Transition.pResource = m_depthSnapshot;
        post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
        post[postCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
        post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++postCount;
        if (gameDepthState != D3D12_RESOURCE_STATE_COPY_SOURCE) {
            post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            post[postCount].Transition.pResource = gameDepth;
            post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            post[postCount].Transition.StateAfter = gameDepthState;
            post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            ++postCount;
        }
        cmdList->ResourceBarrier(postCount, post);
        return true;
    }

    // 64bpp path: shader resolve plane 0 → D32_FLOAT DSV.
    if (!m_depthResolve) m_depthResolve = std::make_unique<DepthResolve>();
    if (!m_depthResolve->EnsureInitialized(m_d3dDevice, DXGI_FORMAT_D32_FLOAT,
            m_depthSnapshotW, m_depthSnapshotH)) {
        return false;
    }

    D3D12_RESOURCE_BARRIER pre[2] = {};
    UINT preCount = 0;
    if (gameDepthState != D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) {
        pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        pre[preCount].Transition.pResource = gameDepth;
        pre[preCount].Transition.StateBefore = gameDepthState;
        pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++preCount;
    }
    // Snapshot was last left in COPY_SOURCE if we've written before; first
    // time it's in COPY_DEST (created with that state). Either way go to
    // DEPTH_WRITE for the resolve draw.
    pre[preCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    pre[preCount].Transition.pResource = m_depthSnapshot;
    pre[preCount].Transition.StateBefore = (m_depthSnapshotSerial != 0)
        ? D3D12_RESOURCE_STATE_COPY_SOURCE
        : D3D12_RESOURCE_STATE_COPY_DEST;
    pre[preCount].Transition.StateAfter = D3D12_RESOURCE_STATE_DEPTH_WRITE;
    pre[preCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++preCount;
    cmdList->ResourceBarrier(preCount, pre);

    const bool ok = m_depthResolve->RecordResolve(cmdList, gameDepth, m_depthSnapshot);

    // [DEPTH-AERV2] Second resolve of the SAME gameDepth SRV (still in
    // PIXEL_SHADER_RESOURCE) into the plain R32_FLOAT color snapshot that CUDA can
    // import. Self-contained barriers on m_depthSnapshotR32 only; does not touch
    // the depth-output path above. Non-fatal.
    if (m_depthSnapshotR32) {
        D3D12_RESOURCE_BARRIER rtBar{};
        rtBar.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        rtBar.Transition.pResource = m_depthSnapshotR32;
        rtBar.Transition.StateBefore = (m_depthSnapshotR32Serial != 0)
            ? D3D12_RESOURCE_STATE_COPY_SOURCE : D3D12_RESOURCE_STATE_COMMON;
        rtBar.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
        rtBar.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        cmdList->ResourceBarrier(1, &rtBar);

        m_depthResolve->RecordResolveColor(cmdList, gameDepth, m_depthSnapshotR32);

        rtBar.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
        rtBar.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
        cmdList->ResourceBarrier(1, &rtBar);
        m_depthSnapshotR32Serial = 1;  // marked produced; serial set on publish below
    }

    D3D12_RESOURCE_BARRIER post[2] = {};
    UINT postCount = 0;
    post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    post[postCount].Transition.pResource = m_depthSnapshot;
    post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_DEPTH_WRITE;
    post[postCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++postCount;
    if (gameDepthState != D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE) {
        post[postCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        post[postCount].Transition.pResource = gameDepth;
        post[postCount].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        post[postCount].Transition.StateAfter = gameDepthState;
        post[postCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++postCount;
    }
    cmdList->ResourceBarrier(postCount, post);
    return ok;
}

bool OpenXRManager::CaptureMonoPresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, uint64_t serial,
    const XrPosef poses[2], const XrFovf fovs[2], const bool hasView[2]) {
    if (!backBuffer || !hasView[0] || !hasView[1]) {
        return false;
    }

    std::lock_guard<std::mutex> captureLock(m_captureMutex);
    if (!EnsureMonoCaptureResource(sourceDesc)) {
        return false;
    }

    ID3D12Resource* snapshot = nullptr;
    uint64_t previousSerial = 0;
    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        snapshot = m_monoCapturedFrame.texture;
        previousSerial = m_monoCapturedFrame.serial;
        if (snapshot) {
            snapshot->AddRef();
        }
    }
    if (!snapshot) {
        return false;
    }

    m_captureAllocatorIndex = (m_captureAllocatorIndex + 1) % 3;
    ID3D12CommandAllocator* currentAllocator = m_captureCmdAllocators[m_captureAllocatorIndex];
    
    if (m_captureFenceValue >= 3 && m_captureFence->GetCompletedValue() < m_captureFenceValue - 2) {
        m_captureFence->SetEventOnCompletion(m_captureFenceValue - 2, m_captureFenceEvent);
        WaitForSingleObject(m_captureFenceEvent, INFINITE);
    }

    ID3D12GraphicsCommandList* m_captureCmdList = m_captureCmdLists[m_captureAllocatorIndex];

    if (FAILED(currentAllocator->Reset()) || FAILED(m_captureCmdList->Reset(currentAllocator, nullptr))) {
        Log("OpenXRManager: Failed to reset mono capture command list\n");
        snapshot->Release();
        return false;
    }

    D3D12_RESOURCE_BARRIER barriers[2] = {};
    UINT barrierCount = 0;

    barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[barrierCount].Transition.pResource = backBuffer;
    barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++barrierCount;

    if (previousSerial != 0) {
        barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barriers[barrierCount].Transition.pResource = snapshot;
        barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
        barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++barrierCount;
    }

    m_captureCmdList->ResourceBarrier(barrierCount, barriers);
    m_captureCmdList->CopyResource(snapshot, backBuffer);

    D3D12_RESOURCE_BARRIER afterCopy[2] = {};
    afterCopy[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[0].Transition.pResource = snapshot;
    afterCopy[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    afterCopy[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    afterCopy[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[1].Transition.pResource = backBuffer;
    afterCopy[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[1].Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    afterCopy[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    m_captureCmdList->ResourceBarrier(2, afterCopy);

    // [DEPTH] Snapshot the game's scene depth into our own resource on the SAME
    // capture list. We transition with the OBSERVED current state (never a guess);
    // if no explicit transition has been seen yet (state==0), we skip to avoid a
    // bad barrier (the classic device-removed cause).
    //
    // *** Mono path: depth capture is OFF by default ***
    // CaptureMonoPresentedFrame runs EVERY present, transitioning the game's
    // scene-depth resource state on the swapchain queue. The game's render
    // queue is simultaneously writing that same resource (DepthPrepass + late
    // DLSS history). Without a cross-queue Wait this races -> can leave
    // m_captureFence un-signaled -> WaitForSingleObject(INFINITE) on next
    // present locks the game. With a cross-queue Wait, async-compute fence
    // dependencies form a cycle -> same hang. Net result: mono depth submit
    // is currently incompatible with CP2077's render queue layout, so we skip
    // it entirely unless the user opts in with xr_mono_depth_capture=1.
    ID3D12Resource* gameDepth = OmoGetSceneDepthResource();
    const D3D12_RESOURCE_STATES gameDepthState = static_cast<D3D12_RESOURCE_STATES>(OmoGetSceneDepthState());
    bool depthCaptured = false;
    if (GetMonoDepthCapture() != 0 &&
        gameDepth && OmoGetSceneDepthState() != 0 && EnsureDepthSnapshot(gameDepth)) {
        depthCaptured = RecordDepthCapture(m_captureCmdList, gameDepth, gameDepthState);
    }

    m_captureCmdList->Close();
    // Cross-queue safety for depth read: game writes scene depth from its render
    // queue while we copy it on the swapchain queue. Insert GPU-side Waits on
    // every tracked game queue's latest Signal value before our
    // ExecuteCommandLists. SKIPPED in mono mode by default -- mono captures EVERY
    // present and the cross-queue Wait on CP2077's async-compute fences was
    // creating a Wait cycle that froze the present thread on
    // m_captureFenceEvent INFINITE. The race window without it is small
    // (depth might be one frame stale) compared to a full freeze. Re-enable
    // by setting xr_mono_xqueue_wait=1 in vrport.ini.
    if (depthCaptured && GetMonoXQueueWait() != 0) {
        CyberpunkVRPort_WaitOnAllGameSignals(m_d3dQueue);
    }
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);

    ++m_captureFenceValue;
    m_d3dQueue->Signal(m_captureFence, m_captureFenceValue);

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        if (m_monoCapturedFrame.texture == snapshot) {
            m_monoCapturedFrame.serial = serial;
            for (int eye = 0; eye < 2; ++eye) {
                m_monoCapturedFrame.poses[eye] = poses[eye];
                m_monoCapturedFrame.fovs[eye] = fovs[eye];
                m_monoCapturedFrame.hasView[eye] = hasView[eye];
            }
            SetD3DNamef(m_monoCapturedFrame.texture, L"OpenXR_mono_snapshot_serial%llu",
                static_cast<unsigned long long>(serial));
            if (depthCaptured) {
                m_depthSnapshotSerial = serial;
            }
        }
    }
    if (m_monoPresentEvent) {
        SetEvent(m_monoPresentEvent);
    }

    snapshot->Release();
    if (g_verboseLog && (serial % 300) == 1) {
        Log("OpenXRManager: Mono frame captured. serial=%llu\n",
            static_cast<unsigned long long>(serial));
    }
    return true;
}

bool OpenXRManager::EnsureAERCaptureResources(const D3D12_RESOURCE_DESC& sourceDesc) {
    if (!m_d3dDevice || !m_d3dQueue) {
        return false;
    }

    const uint32_t width = static_cast<uint32_t>(sourceDesc.Width);
    const uint32_t height = sourceDesc.Height;
    const uint32_t format = static_cast<uint32_t>(sourceDesc.Format);
    if (width == 0 || height == 0 || format == 0) {
        return false;
    }

    if (!m_captureCmdAllocators[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_captureCmdAllocators[i])))) {
                Log("OpenXRManager: Failed to create AER capture command allocator %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdAllocators[i], L"AERV2_capture_allocator");
        }
    }
    if (!m_captureCmdLists[0]) {
        for (int i = 0; i < 3; ++i) {
            if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_captureCmdAllocators[i], nullptr, IID_PPV_ARGS(&m_captureCmdLists[i])))) {
                Log("OpenXRManager: Failed to create AER capture command list %d\n", i);
                return false;
            }
            SetD3DName(m_captureCmdLists[i], L"AERV2_capture_command_list");
            m_captureCmdLists[i]->Close();
        }
    }
    if (!m_captureFence) {
        if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_captureFence)))) {
            Log("OpenXRManager: Failed to create AER capture fence\n");
            return false;
        }
        SetD3DName(m_captureFence, L"AERV2_capture_fence");
    }
    if (!m_captureFenceEvent) {
        m_captureFenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!m_captureFenceEvent) {
            Log("OpenXRManager: Failed to create AER capture fence event\n");
            return false;
        }
    }

    const bool aerV2Enabled = GetAERV2Enabled() != 0;
    const DXGI_FORMAT opticalFlowFormat = GetAERV2OpticalFlowFormat(sourceDesc.Format);
    auto framesMatch = [width, height, format, aerV2Enabled](const CapturedEyeFrame* frames) {
        for (int eye = 0; eye < 2; ++eye) {
            const CapturedEyeFrame& frame = frames[eye];
            if (!frame.texture || frame.width != width || frame.height != height || frame.format != format) {
                return false;
            }
            if (aerV2Enabled && !frame.textureShareable) {
                return false;
            }
        }
        return true;
    };
    auto opticalFlowFramesMatch = [aerV2Enabled, opticalFlowFormat](const CapturedEyeFrame* frames) {
        if (!aerV2Enabled) {
            return true;
        }
        for (int eye = 0; eye < 2; ++eye) {
            if (!frames[eye].opticalFlowTexture) {
                return false;
            }
            if (frames[eye].opticalFlowTexture->GetDesc().Format != opticalFlowFormat) {
                return false;
            }
        }
        return true;
    };

    const bool texturesMatch =
        framesMatch(m_capturedEyeFrames) &&
        framesMatch(m_previousCapturedEyeFrames) &&
        framesMatch(m_pendingEyeFrames) &&
        opticalFlowFramesMatch(m_capturedEyeFrames) &&
        opticalFlowFramesMatch(m_previousCapturedEyeFrames) &&
        opticalFlowFramesMatch(m_pendingEyeFrames);
    if (texturesMatch) {
        return true;
    }

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        auto releaseFrames = [](CapturedEyeFrame* frames) {
            for (int eye = 0; eye < 2; ++eye) {
                CapturedEyeFrame& frame = frames[eye];
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
                // Reset the convert-fence value: the optical-flow module (and thus its
                // convert fence) is rebuilt on resolution change, so a stale value here
                // would make the frame thread's GPU-Wait block forever on a fence that
                // never reaches it.
                frame.opticalFlowConvertValue = 0;
                frame.depthInCopySource = false;
                frame.pose = {};
                frame.pose.orientation.w = 1.0f;
                frame.fov = {};
                frame.hasView = false;
            }
        };
        releaseFrames(m_capturedEyeFrames);
        releaseFrames(m_previousCapturedEyeFrames);
        releaseFrames(m_pendingEyeFrames);
    }

    D3D12_HEAP_PROPERTIES heapProps{};
    heapProps.Type = D3D12_HEAP_TYPE_DEFAULT;
    const D3D12_HEAP_FLAGS sharedHeapFlags = aerV2Enabled ? D3D12_HEAP_FLAG_SHARED : D3D12_HEAP_FLAG_NONE;
    D3D12_RESOURCE_DESC opticalFlowDesc{};
    opticalFlowDesc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    opticalFlowDesc.Width = width;
    opticalFlowDesc.Height = height;
    opticalFlowDesc.DepthOrArraySize = 1;
    opticalFlowDesc.MipLevels = 1;
    opticalFlowDesc.Format = opticalFlowFormat;
    opticalFlowDesc.SampleDesc.Count = 1;
    opticalFlowDesc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    opticalFlowDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    auto createFrames = [&](CapturedEyeFrame* frames, const char* label, const wchar_t* nameLabel) {
        for (int eye = 0; eye < 2; ++eye) {
            CapturedEyeFrame& frame = frames[eye];
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps,
                    sharedHeapFlags,
                    &sourceDesc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    nullptr,
                    IID_PPV_ARGS(&frame.texture)))) {
                Log("OpenXRManager: Failed to create AER %s texture\n", label);
                return false;
            }
            SetD3DNamef(frame.texture, L"AERV2_%ls_eye%d_color", nameLabel, eye);
            frame.textureShareable = aerV2Enabled;
            if (aerV2Enabled) {
                if (FAILED(m_d3dDevice->CreateCommittedResource(
                        &heapProps,
                        sharedHeapFlags,
                        &opticalFlowDesc,
                        D3D12_RESOURCE_STATE_COMMON,
                        nullptr,
                        IID_PPV_ARGS(&frame.opticalFlowTexture)))) {
                    Log("OpenXRManager: Failed to create AER V2 %s optical-flow texture\n", label);
                    return false;
                }
                SetD3DNamef(frame.opticalFlowTexture, L"AERV2_%ls_eye%d_ofinput", nameLabel, eye);
            }

            frame.width = width;
            frame.height = height;
            frame.format = format;
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
        return true;
    };

    if (!createFrames(m_capturedEyeFrames, "completed", L"completed") ||
        !createFrames(m_previousCapturedEyeFrames, "previous", L"previous") ||
        !createFrames(m_pendingEyeFrames, "pending", L"pending")) {
        return false;
    }

    // Phase 3: scratch RT for depth-based stereo reprojection. Always created
    // for AER (independent of V2 NvOF synth flag) because the reprojection
    // path is the FPS-per-eye fix and runs unconditionally on AER submit.
    {
        D3D12_RESOURCE_DESC stereoDesc = sourceDesc;
        stereoDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE stereoClear{};
        stereoClear.Format = sourceDesc.Format;
        bool recreate = true;
        if (m_stereoSynthEye) {
            auto cur = m_stereoSynthEye->GetDesc();
            if (cur.Width == stereoDesc.Width && cur.Height == stereoDesc.Height &&
                cur.Format == stereoDesc.Format) {
                recreate = false;
            } else {
                m_stereoSynthEye.Reset();
            }
        }
        if (recreate) {
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps, D3D12_HEAP_FLAG_NONE,
                    &stereoDesc, D3D12_RESOURCE_STATE_COMMON,
                    &stereoClear, IID_PPV_ARGS(&m_stereoSynthEye)))) {
                Log("OpenXRManager: Failed to create stereo-synth scratch\n");
                return false;
            }
            SetD3DName(m_stereoSynthEye.Get(), L"AER_stereo_synth_scratch");
        }
    }

    // Phase 2b output target: scratch RT for the MV-warp pass. Only created
    // when AER V2 is enabled (the warp consumes engine MV captured via NGX
    // hook, which only matters for V2 frame-gen).
    if (aerV2Enabled) {
        // alternate-eye NvOF midpoint outputs. These are imported into CUDA as
        // writable surfaces, then copied into the XR eye swapchain on submit.
        // Kept in logical COPY_SOURCE state so submit can CopyResource without
        // extra barriers after CUDA signals the shared fence.
        D3D12_RESOURCE_DESC synthDesc = sourceDesc;
        synthDesc.Flags = D3D12_RESOURCE_FLAG_NONE;
        for (int eye = 0; eye < 2; ++eye) {
            for (int slot = 0; slot < 2; ++slot) {
                if (m_aerV2SynthEye[eye][slot]) {
                    auto cur = m_aerV2SynthEye[eye][slot]->GetDesc();
                    if (cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format) {
                        continue;
                    }
                    m_aerV2SynthEye[eye][slot].Reset();
                }
                if (FAILED(m_d3dDevice->CreateCommittedResource(
                        &heapProps,
                        sharedHeapFlags,
                        &synthDesc,
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                        nullptr,
                        IID_PPV_ARGS(&m_aerV2SynthEye[eye][slot])))) {
                    Log("OpenXRManager: Failed to create AER V2 NvOF synth eye=%d slot=%d\n", eye, slot);
                    return false;
                }
                SetD3DNamef(m_aerV2SynthEye[eye][slot].Get(), L"AERV2_nvof_synth_eye%d_slot%d", eye, slot);

                if (m_aerV2SubmitEye[eye][slot]) {
                    auto cur = m_aerV2SubmitEye[eye][slot]->GetDesc();
                    if (!(cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format)) {
                        m_aerV2SubmitEye[eye][slot].Reset();
                    }
                }
                if (!m_aerV2SubmitEye[eye][slot]) {
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &heapProps,
                            D3D12_HEAP_FLAG_NONE,
                            &synthDesc,
                            D3D12_RESOURCE_STATE_COMMON,
                            nullptr,
                            IID_PPV_ARGS(&m_aerV2SubmitEye[eye][slot])))) {
                        Log("OpenXRManager: Failed to create AER V2 submit eye=%d slot=%d\n", eye, slot);
                        return false;
                    }
                    SetD3DNamef(m_aerV2SubmitEye[eye][slot].Get(), L"AERV2_submit_synth_eye%d_slot%d", eye, slot);
                    m_aerV2SubmitEyeReady[eye][slot] = false;
                }

                // Half-rate in-between frame for both eyes: identical desc to the
                // synth eye. CUDA writes m_aerV2InBetween (COPY_SOURCE);
                // submit copies via m_aerV2InBetweenSubmit so submit and CUDA
                // never touch the same resource concurrently.
                if (m_aerV2InBetween[eye][slot]) {
                    auto cur = m_aerV2InBetween[eye][slot]->GetDesc();
                    if (!(cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format)) {
                        m_aerV2InBetween[eye][slot].Reset();
                    }
                }
                if (!m_aerV2InBetween[eye][slot]) {
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &heapProps,
                            sharedHeapFlags,
                            &synthDesc,
                            D3D12_RESOURCE_STATE_COPY_SOURCE,
                            nullptr,
                            IID_PPV_ARGS(&m_aerV2InBetween[eye][slot])))) {
                        Log("OpenXRManager: Failed to create AER V2 in-between eye=%d slot=%d\n", eye, slot);
                        return false;
                    }
                    SetD3DNamef(m_aerV2InBetween[eye][slot].Get(), L"AERV2_nvof_inbetween_eye%d_slot%d", eye, slot);
                }
                if (m_aerV2InBetweenSubmit[eye][slot]) {
                    auto cur = m_aerV2InBetweenSubmit[eye][slot]->GetDesc();
                    if (!(cur.Width == synthDesc.Width && cur.Height == synthDesc.Height && cur.Format == synthDesc.Format)) {
                        m_aerV2InBetweenSubmit[eye][slot].Reset();
                    }
                }
                if (!m_aerV2InBetweenSubmit[eye][slot]) {
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &heapProps,
                            D3D12_HEAP_FLAG_NONE,
                            &synthDesc,
                            D3D12_RESOURCE_STATE_COMMON,
                            nullptr,
                            IID_PPV_ARGS(&m_aerV2InBetweenSubmit[eye][slot])))) {
                        Log("OpenXRManager: Failed to create AER V2 in-between submit eye=%d slot=%d\n", eye, slot);
                        return false;
                    }
                    SetD3DNamef(m_aerV2InBetweenSubmit[eye][slot].Get(), L"AERV2_submit_inbetween_eye%d_slot%d", eye, slot);
                    m_aerV2InBetweenSubmitReady[eye][slot] = false;
                }
            }
        }

        D3D12_RESOURCE_DESC warpDesc = sourceDesc;
        warpDesc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_CLEAR_VALUE warpClear{};
        warpClear.Format = sourceDesc.Format;
        for (int eye = 0; eye < 2; ++eye) {
            if (m_mvWarpedEye[eye]) {
                auto cur = m_mvWarpedEye[eye]->GetDesc();
                if (cur.Width == warpDesc.Width && cur.Height == warpDesc.Height && cur.Format == warpDesc.Format) {
                    continue;
                }
                m_mvWarpedEye[eye].Reset();
            }
            if (FAILED(m_d3dDevice->CreateCommittedResource(
                    &heapProps, D3D12_HEAP_FLAG_NONE,
                    &warpDesc, D3D12_RESOURCE_STATE_COMMON,
                    &warpClear, IID_PPV_ARGS(&m_mvWarpedEye[eye])))) {
                Log("OpenXRManager: Failed to create MV-warp scratch eye=%d\n", eye);
                return false;
            }
            SetD3DNamef(m_mvWarpedEye[eye].Get(), L"AERV2_mvwarped_eye%d", eye);
        }
    }

    Log("OpenXRManager: AER capture resources ready. size=%ux%u format=%u tripleBuffered=1\n", width, height, format);
    return true;
}

bool OpenXRManager::CapturePresentedFrame(ID3D12Resource* backBuffer, const D3D12_RESOURCE_DESC& sourceDesc, int eyeIndex, uint64_t serial, uint64_t pairId) {
    if (!backBuffer || eyeIndex < 0 || eyeIndex > 1) {
        return false;
    }

    std::lock_guard<std::mutex> captureLock(m_captureMutex);
    if (!EnsureAERCaptureResources(sourceDesc)) {
        return false;
    }

    CapturedEyeFrame* frame = &m_pendingEyeFrames[eyeIndex];
    if (!frame->texture) {
        return false;
    }

    m_captureAllocatorIndex = (m_captureAllocatorIndex + 1) % 3;
    ID3D12CommandAllocator* currentAllocator = m_captureCmdAllocators[m_captureAllocatorIndex];
    
    if (m_captureFenceValue >= 3 && m_captureFence->GetCompletedValue() < m_captureFenceValue - 2) {
        m_captureFence->SetEventOnCompletion(m_captureFenceValue - 2, m_captureFenceEvent);
        WaitForSingleObject(m_captureFenceEvent, INFINITE);
    }

    ID3D12GraphicsCommandList* m_captureCmdList = m_captureCmdLists[m_captureAllocatorIndex];

    if (FAILED(currentAllocator->Reset()) || FAILED(m_captureCmdList->Reset(currentAllocator, nullptr))) {
        Log("OpenXRManager: Failed to reset AER capture command list\n");
        return false;
    }

    D3D12_RESOURCE_BARRIER barriers[6] = {};
    UINT barrierCount = 0;

    barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[barrierCount].Transition.pResource = backBuffer;
    barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++barrierCount;

    if (frame->serial != 0) {
        barriers[barrierCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barriers[barrierCount].Transition.pResource = frame->texture;
        barriers[barrierCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        barriers[barrierCount].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
        barriers[barrierCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        ++barrierCount;
    }

    m_captureCmdList->ResourceBarrier(barrierCount, barriers);
    m_captureCmdList->CopyResource(frame->texture, backBuffer);

    D3D12_RESOURCE_BARRIER afterCopy[3] = {};
    afterCopy[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[0].Transition.pResource = frame->texture;
    afterCopy[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    afterCopy[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    UINT afterCopyCount = 1;
    afterCopy[afterCopyCount].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    afterCopy[afterCopyCount].Transition.pResource = backBuffer;
    afterCopy[afterCopyCount].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    afterCopy[afterCopyCount].Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    afterCopy[afterCopyCount].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    ++afterCopyCount;
    m_captureCmdList->ResourceBarrier(afterCopyCount, afterCopy);

    // [DEPTH] Snapshot the game's scene depth into m_depthSnapshot on the SAME AER
    // capture list, mirroring the mono path (CaptureMonoPresentedFrame). This gives
    // the AER submit a depth buffer to chain as XR_KHR_composition_layer_depth so the
    // runtime can do DEPTH-AWARE (positional) reprojection of the half-rate stale eye.
    // Observed-state barriers only (never a guessed StateBefore).
    //
    // Gated on GetAerXQueueWait() (default 0). Depth capture forces a cross-queue GPU
    // wait (CyberpunkVRPort_WaitOnAllGameSignals) that serializes the present queue
    // behind CP2077's async-compute -> depressed sim rate (NPC/animation stutter while
    // shader-time things stay smooth). With it OFF (the mono default) the AER V2 warp
    // falls back to NvOF-only flow (its depth-reprojection path is agreement-gated and
    // self-disables), which is smooth at the cost of positional reprojection quality.
    bool depthCaptured = false;
    bool frameDepthCaptured = false;
    const bool aerDepthEnabled = (GetAerXQueueWait() != 0);
    {
        ID3D12Resource* gameDepth = aerDepthEnabled ? OmoGetSceneDepthResource() : nullptr;
        const D3D12_RESOURCE_STATES gameDepthState = static_cast<D3D12_RESOURCE_STATES>(OmoGetSceneDepthState());
        if (gameDepth && OmoGetSceneDepthState() != 0 && EnsureDepthSnapshot(gameDepth)) {
            depthCaptured = RecordDepthCapture(m_captureCmdList, gameDepth, gameDepthState);
            if (depthCaptured && GetAERV2Enabled() != 0 && m_depthSnapshot) {
                const D3D12_RESOURCE_DESC depthDesc = m_depthSnapshot->GetDesc();
                bool recreateFrameDepth = true;
                if (frame->depthTexture) {
                    const D3D12_RESOURCE_DESC cur = frame->depthTexture->GetDesc();
                    if (cur.Width == depthDesc.Width && cur.Height == depthDesc.Height && cur.Format == depthDesc.Format) {
                        recreateFrameDepth = false;
                    } else {
                        frame->depthTexture->Release();
                        frame->depthTexture = nullptr;
                        frame->depthSerial = 0;
                        frame->depthInCopySource = false;
                    }
                }
                if (recreateFrameDepth) {
                    D3D12_HEAP_PROPERTIES hp{};
                    hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                    D3D12_CLEAR_VALUE clearVal{};
                    clearVal.Format = depthDesc.Format;
                    clearVal.DepthStencil.Depth = 1.0f;
                    clearVal.DepthStencil.Stencil = 0;
                    if (FAILED(m_d3dDevice->CreateCommittedResource(
                            &hp,
                            D3D12_HEAP_FLAG_SHARED,
                            &depthDesc,
                            D3D12_RESOURCE_STATE_COPY_DEST,
                            &clearVal,
                            IID_PPV_ARGS(&frame->depthTexture)))) {
                        Log("OpenXRManager: Failed to create AER V2 frame depth texture eye=%d\n", eyeIndex);
                        frame->depthTexture = nullptr;
                        frame->depthSerial = 0;
                        frame->depthInCopySource = false;
                    } else {
                        frame->depthWidth = static_cast<uint32_t>(depthDesc.Width);
                        frame->depthHeight = depthDesc.Height;
                        frame->depthFormat = static_cast<uint32_t>(depthDesc.Format);
                        frame->depthSerial = 0;
                        frame->depthInCopySource = false;
                        SetD3DNamef(frame->depthTexture, L"AERV2_pending_eye%d_depth", eyeIndex);
                    }
                }
                if (frame->depthTexture) {
                    D3D12_RESOURCE_BARRIER bars[2] = {};
                    UINT bc = 0;
                    if (frame->depthInCopySource) {
                        bars[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        bars[bc].Transition.pResource = frame->depthTexture;
                        bars[bc].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                        bars[bc].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                        bars[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        ++bc;
                    }
                    if (bc > 0) m_captureCmdList->ResourceBarrier(bc, bars);
                    m_captureCmdList->CopyResource(frame->depthTexture, m_depthSnapshot);
                    bars[0] = {};
                    bars[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                    bars[0].Transition.pResource = frame->depthTexture;
                    bars[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                    bars[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                    bars[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                    m_captureCmdList->ResourceBarrier(1, bars);
                    frameDepthCaptured = true;
                }
            }
        }
    }

    m_captureCmdList->Close();
    // Cross-queue safety for depth read (AER capture path). Mirrors the mono
    // path's guard in CaptureMonoPresentedFrame so VDXR depth submit no longer
    // races the game's render queue → no more DEVICE_HUNG on save/load when
    // the depth resource is being recycled.
    if (depthCaptured) {
        CyberpunkVRPort_WaitOnAllGameSignals(m_d3dQueue);
    }
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);

    ++m_captureFenceValue;
    m_d3dQueue->Signal(m_captureFence, m_captureFenceValue);

    // [AER V2 unified producer] Convert this eye's freshly captured color into its
    // NvOF input texture ONCE per capture (R8G8B8A8 -> BGRA swizzle). Ordered AFTER
    // the capture copy above via a GPU-side Wait on m_captureFence (no race), and
    // run HERE on the free-running present thread so the 90 Hz frame thread's warp/
    // submit hot path never stalls on it. FrameThreadMain later imports this
    // opticalFlowTexture into CUDA for the temporal warp. No-op unless V2 is on and
    // the flow-input texture exists.
    uint64_t flowConvertValue = 0;
    if (GetAERV2Enabled() != 0 && m_opticalFlow && frame->opticalFlowTexture && frame->texture) {
        const uint32_t cw = sourceDesc.Width != 0 ? static_cast<uint32_t>(sourceDesc.Width) : frame->width;
        const uint32_t ch = sourceDesc.Height != 0 ? sourceDesc.Height : frame->height;
        if (m_opticalFlow->EnsureInitialized(m_d3dDevice, cw, ch, sourceDesc.Format)) {
            // Fire-and-forget: ordered after the capture copy via m_captureFence,
            // signals the convert fence to flowConvertValue, returns immediately (no
            // CPU stall on this present thread). The frame thread GPU-Waits on it.
            if (!m_opticalFlow->ConvertToInputTexture(frame->texture, frame->opticalFlowTexture,
                                                      m_captureFence, m_captureFenceValue,
                                                      &flowConvertValue) &&
                (serial % 600) == 1) {
                Log("OpenXRManager: [AER V2] flow-input conversion failed eye=%d serial=%llu\n",
                    eyeIndex, static_cast<unsigned long long>(serial));
            }
        }
    }

    {
        LARGE_INTEGER captureQpc{};
        QueryPerformanceCounter(&captureQpc);
        std::lock_guard<std::mutex> lock(m_presentMutex);
        frame->serial = serial;
        frame->pairId = pairId;
        frame->captureQpc = static_cast<uint64_t>(captureQpc.QuadPart);
        frame->opticalFlowConvertValue = flowConvertValue;
        if (depthCaptured) {
            m_depthSnapshotSerial = serial;
            if (m_depthSnapshotR32) m_depthSnapshotR32Serial = serial;
        }
        if (frameDepthCaptured) {
            frame->depthSerial = serial;
            frame->depthInCopySource = true;
        } else {
            frame->depthSerial = 0;
        }
        SetD3DNamef(frame->texture, L"AERV2_pending_eye%d_color_pair%llu_serial%llu", eyeIndex,
            static_cast<unsigned long long>(pairId),
            static_cast<unsigned long long>(serial));
        SetD3DNamef(frame->opticalFlowTexture, L"AERV2_pending_eye%d_ofinput_pair%llu_serial%llu", eyeIndex,
            static_cast<unsigned long long>(pairId),
            static_cast<unsigned long long>(serial));
        if (frameDepthCaptured) {
            SetD3DNamef(frame->depthTexture, L"AERV2_pending_eye%d_depth_pair%llu_serial%llu", eyeIndex,
                static_cast<unsigned long long>(pairId),
                static_cast<unsigned long long>(serial));
        }
    }

    if (g_verboseLog && (serial % 300) == 1) {
        Log("OpenXRManager: AER frame captured. eye=%d serial=%llu pair=%llu\n",
            eyeIndex,
            static_cast<unsigned long long>(serial),
            static_cast<unsigned long long>(pairId));
    }
    return true;
}

bool OpenXRManager::EnsureMonoSubmitResources() {
    if (!m_monoSubmitEnabled.load(std::memory_order_relaxed)) {
        return false;
    }
    if (!m_d3dDevice || !m_d3dQueue || m_session == XR_NULL_HANDLE) {
        return false;
    }

    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t format = 0;
    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        width = m_lastPresentedWidth;
        height = m_lastPresentedHeight;
        format = m_lastPresentedFormat;
    }

    if (width == 0 || height == 0 || format == 0) {
        return false;
    }

    uint32_t runtimeFormatCount = 0;
    XrResult xrRes = xrEnumerateSwapchainFormats(m_session, 0, &runtimeFormatCount, nullptr);
    if (XR_FAILED(xrRes) || runtimeFormatCount == 0) {
        Log("OpenXRManager: xrEnumerateSwapchainFormats count failed (res=%d count=%u)\n", xrRes, runtimeFormatCount);
        return false;
    }

    std::vector<int64_t> runtimeFormats(runtimeFormatCount);
    xrRes = xrEnumerateSwapchainFormats(m_session, runtimeFormatCount, &runtimeFormatCount, runtimeFormats.data());
    if (XR_FAILED(xrRes) || runtimeFormatCount == 0) {
        Log("OpenXRManager: xrEnumerateSwapchainFormats list failed (res=%d count=%u)\n", xrRes, runtimeFormatCount);
        return false;
    }

    const int64_t selectedFormat = PickMonoSwapchainFormat(
        runtimeFormats,
        static_cast<int64_t>(format),
        IsRuntimeVirtualDesktop());

    // Pick a runtime-supported depth format ONLY AFTER the game's scene depth resource
    // has been pinned. This remains intentionally conservative: only the R32-family
    // depth path is considered stable. The 64-bit R32G8X24 typeless family caused
    // repeated GPU removal during snapshot/submission experiments, so depth is kept
    // disabled there to preserve a working Mono baseline.
    ID3D12Resource* pinnedDepth = OmoGetSceneDepthResource();
    const DXGI_FORMAT pinnedDepthFormat = pinnedDepth ? pinnedDepth->GetDesc().Format : DXGI_FORMAT_UNKNOWN;
    int64_t selectedDepthFormat = 0;
    // CP2077 mono-only mode hangs at start-up when a depth swapchain is created
    // but never populated (VirtualDesktopXR stalls waiting on the depth layer).
    // Skip depth swapchain creation unless either (a) AER is on so the AER
    // capture path will fill the depth, or (b) the user explicitly opted in to
    // mono depth capture.
    const bool aerSubmitOn = IsAERSubmitEnabled();
    const bool depthWanted = GetDepthSubmit() != 0 && (aerSubmitOn || GetMonoDepthCapture() != 0);
    if (depthWanted && m_depthLayerSupported && pinnedDepth) {
        // Only R32-family (D32_FLOAT 32bpp) is supported for now. CP2077's
        // R32-family (32bpp) accepted directly. R32G8X24-family (64bpp) accepted
        // too: the depth-plane resolve shader (DepthResolve) converts plane 0
        // of the typeless source into the same 32bpp D32_FLOAT snapshot used
        // by the 32bpp path, so the depth swapchain is always D32_FLOAT
        // regardless of game depth format.
        const bool gameIs32bpp =
            pinnedDepthFormat == DXGI_FORMAT_R32_TYPELESS ||
            pinnedDepthFormat == DXGI_FORMAT_D32_FLOAT ||
            pinnedDepthFormat == DXGI_FORMAT_R32_FLOAT;
        const bool gameIs64bpp =
            pinnedDepthFormat == DXGI_FORMAT_R32G8X24_TYPELESS ||
            pinnedDepthFormat == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
            pinnedDepthFormat == DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS ||
            pinnedDepthFormat == DXGI_FORMAT_X32_TYPELESS_G8X24_UINT;
        if (gameIs32bpp || gameIs64bpp) {
            for (int64_t rf : runtimeFormats) {
                if (rf == static_cast<int64_t>(DXGI_FORMAT_D32_FLOAT)) {
                    selectedDepthFormat = rf;
                    break;
                }
            }
        }
        if (selectedDepthFormat == 0) {
            // Log once per format transition only — EnsureMonoSubmitResources runs
            // every frame and would otherwise flood the log with thousands of
            // duplicate lines.
            static DXGI_FORMAT s_lastLoggedRejected = DXGI_FORMAT_UNKNOWN;
            if (s_lastLoggedRejected != pinnedDepthFormat) {
                s_lastLoggedRejected = pinnedDepthFormat;
                Log("OpenXRManager: depth layer disabled (gameFmt=%u not depth-resolvable, or runtime lacks D32_FLOAT)\n",
                    static_cast<unsigned>(pinnedDepthFormat));
            }
            m_depthLayerSupported = false;
        } else {
            static DXGI_FORMAT s_lastLoggedSelected = DXGI_FORMAT_UNKNOWN;
            static int64_t s_lastLoggedDepthSel = 0;
            if (s_lastLoggedSelected != pinnedDepthFormat ||
                s_lastLoggedDepthSel != selectedDepthFormat) {
                s_lastLoggedSelected = pinnedDepthFormat;
                s_lastLoggedDepthSel = selectedDepthFormat;
                Log("OpenXRManager: depth format gameFmt=%u selected=%lld\n",
                    static_cast<unsigned>(pinnedDepthFormat),
                    selectedDepthFormat);
            }
        }
    }
    const bool wantDepthSwapchains = m_depthLayerSupported && selectedDepthFormat != 0;
    if (wantDepthSwapchains && selectedDepthFormat != m_depthSwapchainFormat) {
        Log("OpenXRManager: depth swapchain format selected=%lld (pinnedDepthFmt=%u)\n",
            selectedDepthFormat, static_cast<unsigned>(pinnedDepthFormat));
    }

    uint32_t viewCount = 0;
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, 0, &viewCount, nullptr);
    m_viewConfigViews.resize(viewCount, {XR_TYPE_VIEW_CONFIGURATION_VIEW});
    xrEnumerateViewConfigurationViews(m_instance, m_systemId, XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO, viewCount, &viewCount, m_viewConfigViews.data());
    m_views.resize(viewCount, {XR_TYPE_VIEW});

    const bool haveDepthSwapchains = !wantDepthSwapchains ||
        (!m_eyeSwapchains.empty() &&
         m_eyeSwapchains[0].depthHandle != XR_NULL_HANDLE &&
         (m_eyeSwapchains.size() < 2 || m_eyeSwapchains[1].depthHandle != XR_NULL_HANDLE));
    const bool colorResourcesReady = !m_eyeSwapchains.empty() &&
        m_eyeSwapchains[0].width == static_cast<int32_t>(width) &&
        m_eyeSwapchains[0].height == static_cast<int32_t>(height) &&
        m_cmdAllocators[0] && m_cmdLists[0] && m_fence && m_fenceEvent;
    if (colorResourcesReady && (!wantDepthSwapchains || haveDepthSwapchains)) {
        return true;
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

    // Drop the cached last-good textures: a swapchain (re)create may change size/
    // format, which would mismatch CopyResource. They re-create lazily next frame.
    m_lastGoodValid = false;
    for (int e = 0; e < 2; ++e) { m_lastGoodEye[e].Reset(); m_lastGoodEyeInited[e] = false; }

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
        if (m_cmdAllocators[i]) {
            m_cmdAllocators[i]->Release();
            m_cmdAllocators[i] = nullptr;
        }
    }

    m_eyeSwapchains.resize(viewCount);

    for (uint32_t eye = 0; eye < viewCount; ++eye) {
        XrSwapchainCreateInfo swapchainInfo{XR_TYPE_SWAPCHAIN_CREATE_INFO};
        swapchainInfo.usageFlags = XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT;
        swapchainInfo.format = selectedFormat;
        swapchainInfo.sampleCount = 1;
        swapchainInfo.width = static_cast<int32_t>(width);
        swapchainInfo.height = static_cast<int32_t>(height);
        swapchainInfo.faceCount = 1;
        swapchainInfo.arraySize = 1;
        swapchainInfo.mipCount = 1;

        const XrResult res = xrCreateSwapchain(m_session, &swapchainInfo, &m_eyeSwapchains[eye].handle);
        if (XR_FAILED(res)) {
            Log("OpenXRManager: Failed to create mono swapchain for eye %u (res=%d)\n", eye, res);
            return false;
        }

        m_eyeSwapchains[eye].width = swapchainInfo.width;
        m_eyeSwapchains[eye].height = swapchainInfo.height;

        uint32_t imageCount = 0;
        xrEnumerateSwapchainImages(m_eyeSwapchains[eye].handle, 0, &imageCount, nullptr);
        m_eyeSwapchains[eye].images.resize(imageCount, {XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
        xrEnumerateSwapchainImages(
            m_eyeSwapchains[eye].handle,
            imageCount,
            &imageCount,
            reinterpret_cast<XrSwapchainImageBaseHeader*>(m_eyeSwapchains[eye].images.data()));

        if (wantDepthSwapchains) {
            XrSwapchainCreateInfo depthInfo{XR_TYPE_SWAPCHAIN_CREATE_INFO};
            depthInfo.usageFlags = XR_SWAPCHAIN_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT | XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT;
            depthInfo.format = selectedDepthFormat;
            depthInfo.sampleCount = 1;
            depthInfo.width = static_cast<int32_t>(width);
            depthInfo.height = static_cast<int32_t>(height);
            depthInfo.faceCount = 1;
            depthInfo.arraySize = 1;
            depthInfo.mipCount = 1;
            const XrResult dres = xrCreateSwapchain(m_session, &depthInfo, &m_eyeSwapchains[eye].depthHandle);
            if (XR_FAILED(dres)) {
                Log("OpenXRManager: Failed to create depth swapchain for eye %u (res=%d) — disabling depth layer\n", eye, dres);
                m_eyeSwapchains[eye].depthHandle = XR_NULL_HANDLE;
                m_depthLayerSupported = false;
            } else {
                uint32_t dImageCount = 0;
                xrEnumerateSwapchainImages(m_eyeSwapchains[eye].depthHandle, 0, &dImageCount, nullptr);
                m_eyeSwapchains[eye].depthImages.resize(dImageCount, {XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
                xrEnumerateSwapchainImages(
                    m_eyeSwapchains[eye].depthHandle,
                    dImageCount,
                    &dImageCount,
                    reinterpret_cast<XrSwapchainImageBaseHeader*>(m_eyeSwapchains[eye].depthImages.data()));
            }
        }
    }
    m_depthSwapchainFormat = selectedDepthFormat;

    char formatSummary[512] = {};
    int summaryPos = sprintf_s(formatSummary, "OpenXRManager: Mono swapchain formats. game=%u selected=%lld runtime:", format, selectedFormat);
    if (summaryPos > 0) {
        for (uint32_t i = 0; i < runtimeFormatCount && summaryPos > 0 && summaryPos < static_cast<int>(sizeof(formatSummary) - 32); ++i) {
            summaryPos += sprintf_s(formatSummary + summaryPos, sizeof(formatSummary) - summaryPos, " %lld", runtimeFormats[i]);
        }
        Log("%s\n", formatSummary);
    }

    for (int i = 0; i < 3; ++i) {
        if (FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_cmdAllocators[i])))) {
            Log("OpenXRManager: Failed to create submit command allocator %d\n", i);
            return false;
        }
        SetD3DName(m_cmdAllocators[i], L"OpenXR_submit_allocator");
    }
    for (int i = 0; i < 3; ++i) {
        if (FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_cmdAllocators[i], nullptr, IID_PPV_ARGS(&m_cmdLists[i])))) {
            Log("OpenXRManager: Failed to create submit command list %d\n", i);
            return false;
        }
        SetD3DName(m_cmdLists[i], L"OpenXR_submit_command_list");
        m_cmdLists[i]->Close();
    }

    if (FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_fence)))) {
        Log("OpenXRManager: Failed to create mono fence\n");
        return false;
    }
    SetD3DName(m_fence, L"OpenXR_submit_fence");
    m_fenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
    if (!m_fenceEvent) {
        Log("OpenXRManager: Failed to create mono fence event\n");
        return false;
    }

    Log("OpenXRManager: Mono submit resources ready. game=%ux%u eye0=%dx%d rec0=%ux%u format=%u\n",
        width,
        height,
        viewCount != 0 ? m_eyeSwapchains[0].width : 0,
        viewCount != 0 ? m_eyeSwapchains[0].height : 0,
        viewCount != 0 ? m_viewConfigViews[0].recommendedImageRectWidth : 0,
        viewCount != 0 ? m_viewConfigViews[0].recommendedImageRectHeight : 0,
        format);
    return true;
}

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
    m_frameThread = CreateThread(nullptr, 0, &OpenXRManager::FrameThreadThunk, this, 0, nullptr);
    if (!m_frameThread) {
        Log("OpenXRManager: Failed to create frame thread\n");
        return false;
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

DWORD OpenXRManager::FrameThreadMain() {
    Log("OpenXRManager: Frame thread started.\n");
    uint64_t monoWaitLogCounter = 0;
    uint64_t steamVrStartupWaitLogCounter = 0;
    uint64_t displayFrameIndex = 0;

    // Try to restore the user's saved VRIK calibration on startup so they don't recalibrate
    // every launch. If no file, seed m_calib[] with plugin defaults so any subsequent Apply
    // Calibration / Auto-Calibration produces sensible wrist orientation (rather than identity).
    if (!LoadCalibrationFromFile()) {
        m_calib[0].store(1.05f, std::memory_order_relaxed);
        m_calib[1].store(1.06f, std::memory_order_relaxed);
        m_calib[4].store(1.0f,  std::memory_order_relaxed);
        m_calib[5].store(1.0f,  std::memory_order_relaxed);
        m_calib[9].store(-90.0f,  std::memory_order_relaxed); // wRy
        m_calib[11].store(-180.0f,std::memory_order_relaxed); // wLp
        m_calib[12].store(-90.0f, std::memory_order_relaxed); // wLy
    }

    constexpr float kPi = 3.1415926535f;
    auto clamp01 = [](float v) {
        if (v < 0.0f) return 0.0f;
        if (v > 1.0f) return 1.0f;
        return v;
    };
    auto smoothStep01 = [&](float v) {
        const float x = clamp01(v);
        return x * x * (3.0f - 2.0f * x);
    };
    auto quatAngleRad = [](const XrQuaternionf& a, const XrQuaternionf& b) {
        float dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
        if (dot < 0.0f) dot = -dot;
        if (dot > 1.0f) dot = 1.0f;
        return 2.0f * acosf(dot);
    };
    auto normalizeAngle = [&](float angle) {
        while (angle > kPi) angle -= 2.0f * kPi;
        while (angle < -kPi) angle += 2.0f * kPi;
        return angle;
    };
    auto adaptiveFollow = [&](float strength, float delta, float quiet, float release) {
        if (strength <= 0.001f || release <= quiet) {
            return 1.0f;
        }
        const float stillFollow = 1.0f / (1.0f + 20.0f * strength);
        const float motion = smoothStep01((delta - quiet) / (release - quiet));
        return stillFollow + (1.0f - stillFollow) * motion;
    };
    auto resetTrackingPose = [](auto& state, const XrPosef& pose) {
        state.initialized = true;
        state.position = pose.position;
        state.orientation = pose.orientation;
    };
    auto filterTrackingPose = [&](auto& state,
                                  const XrPosef& rawPose,
                                  float strength,
                                  float quietPosMeters,
                                  float releasePosMeters,
                                  float quietAngleRad,
                                  float releaseAngleRad) {
        if (!state.initialized || strength <= 0.001f) {
            resetTrackingPose(state, rawPose);
            return rawPose;
        }

        const float dx = rawPose.position.x - state.position.x;
        const float dy = rawPose.position.y - state.position.y;
        const float dz = rawPose.position.z - state.position.z;
        const float posDelta = sqrtf(dx * dx + dy * dy + dz * dz);
        const float angDelta = quatAngleRad(state.orientation, rawPose.orientation);
        const float posT = adaptiveFollow(strength, posDelta, quietPosMeters, releasePosMeters);
        const float angT = adaptiveFollow(strength, angDelta, quietAngleRad, releaseAngleRad);

        state.position.x += dx * posT;
        state.position.y += dy * posT;
        state.position.z += dz * posT;
        state.orientation = NlerpQuat(state.orientation, rawPose.orientation, angT);

        XrPosef filtered = rawPose;
        filtered.position = state.position;
        filtered.orientation = state.orientation;
        return filtered;
    };
    auto resetTrackingAngle = [](auto& state, float angleRad) {
        state.initialized = true;
        state.angleRad = angleRad;
    };
    auto filterTrackingAngle = [&](auto& state,
                                   float rawAngleRad,
                                   float strength,
                                   float quietAngleRad,
                                   float releaseAngleRad) {
        rawAngleRad = normalizeAngle(rawAngleRad);
        if (!state.initialized || strength <= 0.001f) {
            resetTrackingAngle(state, rawAngleRad);
            return rawAngleRad;
        }

        const float delta = normalizeAngle(rawAngleRad - state.angleRad);
        const float angleT = adaptiveFollow(strength, fabsf(delta), quietAngleRad, releaseAngleRad);
        state.angleRad = normalizeAngle(state.angleRad + delta * angleT);
        return state.angleRad;
    };

    while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
        PollEvents();
        TickAutoCalibration();

        if (!m_sessionRunning.load(std::memory_order_relaxed)) {
            Sleep(10);
            continue;
        }

        if (GetXrRuntimeMode() == 1) {
            uint32_t startupWidth = 0;
            uint32_t startupHeight = 0;
            uint32_t startupFormat = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                startupWidth = m_lastPresentedWidth;
                startupHeight = m_lastPresentedHeight;
                startupFormat = m_lastPresentedFormat;
            }
            if (startupWidth == 0 || startupHeight == 0 || startupFormat == 0) {
                if (g_verboseLog && ((++steamVrStartupWaitLogCounter % 300) == 1)) {
                    Log("OpenXRManager: SteamVR startup wait. Deferring frame loop until first present provides a backbuffer. width=%u height=%u format=%u\n",
                        startupWidth,
                        startupHeight,
                        startupFormat);
                }
                Sleep(1);
                continue;
            }
        }

        // Half-rate submit — two modes:
        //  * AER V2 ON: do NOT skip. The engine is HMD-paced
        //    to ~90 Hz via the m_frameSyncEvent wait in OnPresent (one eye capture
        //    per vsync, alternating), and on EVERY display interval the submit
        //    block below sends the mod's OWN NvOF-synthesized frame for the stale
        //    eye warped to the fresh predicted pose -- no runtime SSW needed.
        //  * AER V2 OFF (legacy): skip the in-between interval so the runtime's
        //    own SSW/ASW fills it when available. Only gates on an already-
        //    submitted complete pair, so
        //    startup / mono fallback run normally.
        if (IsAERSubmitEnabled() &&
            m_monoSubmitEnabled.load(std::memory_order_relaxed) &&
            GetAERHalfRate() != 0 &&
            GetMenuRectMode() == 0 && GetMenuMode() == 0) {
            uint64_t currentPair = 0;
            uint64_t inBetweenReady = 0;
            uint64_t inBetweenShown = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                if (m_capturedEyeFrames[0].pairId != 0 &&
                    m_capturedEyeFrames[0].pairId == m_capturedEyeFrames[1].pairId) {
                    currentPair = m_capturedEyeFrames[0].pairId;
                }
                inBetweenReady = m_inBetweenReadyPairId;
                inBetweenShown = m_inBetweenShownForPairId;
            }
            if (GetAERV2Enabled() != 0) {
                // UNIFIED PRODUCER: do NOT gate on capture novelty. Every xrWaitFrame
                // tick (90 Hz) we re-warp the latest captures to a fresh target time
                // with a new continuous QPC blend, so each eye is updated every display
                // interval even between real captures (45 -> 90). NvOF flow is cached
                // per (prev,curr) pair (AerV2Pipeline::RunNvOF), so only the cheap warp
                // re-runs on synth-only ticks; xrWaitFrame itself paces the loop.
                (void)inBetweenReady; (void)inBetweenShown; (void)currentPair;
            } else {
                if (currentPair != 0 && currentPair == m_lastSubmittedPairId) {
                    if (m_frameSyncEvent) {
                        SetEvent(m_frameSyncEvent);
                    }
                    Sleep(1);
                    continue;
                }
            }
        }

        // Mono cadence gate: if no NEW successfully captured game frame exists,
        // do not keep submitting the same snapshot as a brand new XR frame. That
        // defeats runtime motion smoothing and manifests as strong ghosting/double
        // images on head turns. Instead, wait for a fresh present and let the
        // runtime see the app's true cadence.
        if (m_monoSubmitEnabled.load(std::memory_order_relaxed) &&
            !IsAERSubmitEnabled() &&
            m_monoPresentEvent) {
            uint64_t latestMonoSerial = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                latestMonoSerial = m_monoCapturedFrame.serial;
            }
            // Never block startup: until the first successful Mono submit, the frame
            // thread must keep running so xrLocateViews populates m_views and the
            // Present hook can produce the very first mono snapshot.
            if (m_lastSubmittedSerial != 0 && latestMonoSerial == m_lastSubmittedSerial) {
                // The event can still be signaled from an ALREADY-consumed frame if the
                // thread did not actually wait on it during the fresh submit path. So do
                // not trust the event by itself: only proceed when the serial changed.
                while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
                    // Bail out if AER got enabled while we were parked here. Once AER
                    // is on, OnPresent stops producing mono snapshots (mono capture is
                    // guarded by !aerEnabled), so m_monoCapturedFrame.serial freezes and
                    // this loop would wait forever -> frame thread stalls -> xrWaitFrame/
                    // xrEndFrame never run -> HMD freezes while AER pairs pile up
                    // unsubmitted. Break so the outer loop re-evaluates and takes the
                    // AER submit path.
                    if (IsAERSubmitEnabled()) {
                        break;
                    }
                    const DWORD waitRes = WaitForSingleObject(m_monoPresentEvent, 10);
                    {
                        std::lock_guard<std::mutex> lock(m_presentMutex);
                        latestMonoSerial = m_monoCapturedFrame.serial;
                    }
                    if (latestMonoSerial != 0 && latestMonoSerial != m_lastSubmittedSerial) {
                        break;
                    }
                    if (!m_sessionRunning.load(std::memory_order_relaxed)) {
                        break;
                    }
                    if (waitRes == WAIT_TIMEOUT) {
                        Sleep(1);
                    }
                }
                if (latestMonoSerial == 0 || latestMonoSerial == m_lastSubmittedSerial) {
                    if (m_frameSyncEvent) {
                        SetEvent(m_frameSyncEvent);
                    }
                    continue;
                }
            }
        }

        XrFrameWaitInfo waitInfo{XR_TYPE_FRAME_WAIT_INFO};
        XrFrameState frameState{XR_TYPE_FRAME_STATE};
        XrResult res = xrWaitFrame(m_session, &waitInfo, &frameState);
        if (XR_FAILED(res)) {
            if (m_frameSyncEvent) {
                SetEvent(m_frameSyncEvent);
            }
            Sleep(10);
            continue;
        }
        // Advance the local 90 Hz slot index (only used to
        // ping-pong the synth scratch slot + stride logs). The blendFactor itself
        // is computed from QPC capture timestamps, not this counter.
        ++displayFrameIndex;
        if (frameState.predictedDisplayPeriod > 0) {
            m_predictedDisplayPeriodNs.store(frameState.predictedDisplayPeriod, std::memory_order_relaxed);
        }

        XrFrameBeginInfo beginInfo{XR_TYPE_FRAME_BEGIN_INFO};
        xrBeginFrame(m_session, &beginInfo);

        uint32_t viewCountOutput = 0;
        const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
        const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
        const bool aerEnabled = monoEnabled && IsAERSubmitEnabled();
        bool useAerSubmit = aerEnabled && !menuRectActive;
        const bool monoReady = monoEnabled && EnsureMonoSubmitResources() && !m_eyeSwapchains.empty();
        if (monoReady && !m_views.empty()) {
            XrViewLocateInfo viewLocateInfo{XR_TYPE_VIEW_LOCATE_INFO};
            viewLocateInfo.viewConfigurationType = XR_VIEW_CONFIGURATION_TYPE_PRIMARY_STEREO;
            viewLocateInfo.displayTime = frameState.predictedDisplayTime;
            viewLocateInfo.space = m_localSpace;

            XrViewState viewState{XR_TYPE_VIEW_STATE};
            std::lock_guard<std::mutex> viewLock(m_viewMutex);
            const XrResult locateRes = xrLocateViews(m_session, &viewLocateInfo, &viewState, static_cast<uint32_t>(m_views.size()), &viewCountOutput, m_views.data());
            if (XR_FAILED(locateRes)) {
                Log("OpenXRManager: xrLocateViews failed (res=%d)\n", locateRes);
                viewCountOutput = 0;
            } else if (viewCountOutput >= 2) {
                const float hfov0 = (m_views[0].fov.angleRight - m_views[0].fov.angleLeft) * (180.0f / 3.1415926535f);
                const float hfov1 = (m_views[1].fov.angleRight - m_views[1].fov.angleLeft) * (180.0f / 3.1415926535f);
                const float vfov0 = (m_views[0].fov.angleUp - m_views[0].fov.angleDown) * (180.0f / 3.1415926535f);
                const float vfov1 = (m_views[1].fov.angleUp - m_views[1].fov.angleDown) * (180.0f / 3.1415926535f);
                const float dx = m_views[1].pose.position.x - m_views[0].pose.position.x;
                const float dy = m_views[1].pose.position.y - m_views[0].pose.position.y;
                const float dz = m_views[1].pose.position.z - m_views[0].pose.position.z;
                const float ipd = sqrtf(dx * dx + dy * dy + dz * dz);

                m_runtimeHorizontalFovDeg.store((hfov0 + hfov1) * 0.5f, std::memory_order_relaxed);
                m_runtimeVerticalFovDeg.store((vfov0 + vfov1) * 0.5f, std::memory_order_relaxed);
                m_runtimeIpd.store(ipd, std::memory_order_relaxed);

                // Feed the per-eye pose predictor used by the AER V2 synth path.
                if (viewState.viewStateFlags & XR_VIEW_STATE_ORIENTATION_VALID_BIT) {
                    if (m_qpcFreq == 0) {
                        LARGE_INTEGER qf; QueryPerformanceFrequency(&qf);
                        m_qpcFreq = qf.QuadPart;
                    }
                    LARGE_INTEGER pq; QueryPerformanceCounter(&pq);
                    const double tSec = static_cast<double>(pq.QuadPart) / static_cast<double>(m_qpcFreq);
                    for (int eye = 0; eye < 2 && eye < static_cast<int>(m_views.size()); ++eye) {
                        m_posePredictor[eye].AddSample(tSec, m_views[eye].pose);
                    }
                }
            }
        }

        XrSpaceVelocity headVelocity{XR_TYPE_SPACE_VELOCITY};
        XrSpaceLocation location{XR_TYPE_SPACE_LOCATION};
        location.next = &headVelocity;
        res = xrLocateSpace(m_viewSpace, m_localSpace, frameState.predictedDisplayTime, &location);
        const bool headPoseLocated = XR_SUCCEEDED(res) &&
            (location.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) &&
            (location.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT);
        if (headPoseLocated) {
            XrPosef basePose{};
            bool baseReset = false;
            {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                if (!m_basePoseSet || m_recenterRequested.exchange(false, std::memory_order_relaxed)) {
                    m_basePose = location.pose;
                    m_basePoseSet = true;
                    baseReset = true;
                    Log("OpenXRManager: Base pose captured.\n");
                }
                basePose = m_basePose;
            }

            XrQuaternionf baseInv = ConjugateQuat(basePose.orientation);
            XrVector3f relPosWorld{};
            relPosWorld.x = location.pose.position.x - basePose.position.x;
            relPosWorld.y = location.pose.position.y - basePose.position.y;
            relPosWorld.z = location.pose.position.z - basePose.position.z;
            XrVector3f relPos = RotateVector(baseInv, relPosWorld);
            XrQuaternionf relOri = MultiplyQuat(baseInv, location.pose.orientation);
            XrPosef filteredHeadPose{};
            filteredHeadPose.position = relPos;
            filteredHeadPose.orientation = relOri;
            if (baseReset) {
                m_headFilterState.initialized = false;
                m_handAimYawFilter[0].initialized = false;
                m_handAimYawFilter[1].initialized = false;
            }
            filteredHeadPose = filterTrackingPose(
                m_headFilterState,
                filteredHeadPose,
                GetHmdTrackingSmooth(),
                0.0012f,
                0.0080f,
                0.0035f,
                0.0350f);

            m_posX.store(filteredHeadPose.position.x, std::memory_order_relaxed);
            m_posY.store(filteredHeadPose.position.y, std::memory_order_relaxed);
            m_posZ.store(filteredHeadPose.position.z, std::memory_order_relaxed);
            m_oriX.store(filteredHeadPose.orientation.x, std::memory_order_relaxed);
            m_oriY.store(filteredHeadPose.orientation.y, std::memory_order_relaxed);
            m_oriZ.store(filteredHeadPose.orientation.z, std::memory_order_relaxed);
            m_oriW.store(filteredHeadPose.orientation.w, std::memory_order_relaxed);
            m_poseValid.store(true, std::memory_order_relaxed);

            // [HANDS] Sync actions and locate hands
            static int s_handLogCounter = 0;
            bool doHandLog = g_verboseLog && (s_handLogCounter++ % 120 == 0);

            if (m_actionSet != XR_NULL_HANDLE) {
                XrActiveActionSet activeActionSet{};
                activeActionSet.actionSet = m_actionSet;
                activeActionSet.subactionPath = XR_NULL_PATH;

                XrActionsSyncInfo syncInfo{XR_TYPE_ACTIONS_SYNC_INFO};
                syncInfo.countActiveActionSets = 1;
                syncInfo.activeActionSets = &activeActionSet;
                XrResult syncRes = xrSyncActions(m_session, &syncInfo);
                
                if (doHandLog) {
                    Log("OpenXRManager[Hands]: syncRes=%d sessionState=%d\n", syncRes, (int)m_sessionState);
                }

                // Build a fresh controller snapshot for the XInput merge. Only used
                // when the gameplay-input kill switch is on; otherwise we stay byte-
                // for-byte identical to the pre-Controls-tab behaviour.
                const bool gameplayInputActive = (GetInputActionsEnabled() != 0) && (m_thumbstickAction != XR_NULL_HANDLE);
                VRControllerState ctrl{};

                std::lock_guard<std::mutex> handLock(m_handMutex);
                for (int i = 0; i < 2; i++) {
                    XrActionStateGetInfo getInfo{XR_TYPE_ACTION_STATE_GET_INFO};
                    getInfo.action = m_handPoseAction;
                    getInfo.subactionPath = m_handPaths[i];

                    XrActionStatePose poseState{XR_TYPE_ACTION_STATE_POSE};
                    XrResult poseRes = xrGetActionStatePose(m_session, &getInfo, &poseState);

                    if (doHandLog) {
                        Log("OpenXRManager[Hands]: eye=%d poseRes=%d isActive=%d\n", i, poseRes, poseState.isActive);
                    }

                    m_hands[i].valid = false;
                    bool poseValid = false;
                    XrQuaternionf handRelOri{0,0,0,1};
                    if (poseState.isActive) {
                        XrSpaceLocation handLoc{XR_TYPE_SPACE_LOCATION};
                        XrResult locRes = xrLocateSpace(m_handSpaces[i], m_localSpace, frameState.predictedDisplayTime, &handLoc);

                        if (doHandLog) {
                            Log("OpenXRManager[Hands]: eye=%d locRes=%d flags=0x%X\n", i, locRes, handLoc.locationFlags);
                        }

                        if (XR_SUCCEEDED(locRes)) {
                            if ((handLoc.locationFlags & XR_SPACE_LOCATION_POSITION_VALID_BIT) &&
                                (handLoc.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT)) {

                                XrQuaternionf headInv = ConjugateQuat(location.pose.orientation);
                                XrVector3f hrelPosWorld{};
                                hrelPosWorld.x = handLoc.pose.position.x - location.pose.position.x;
                                hrelPosWorld.y = handLoc.pose.position.y - location.pose.position.y;
                                hrelPosWorld.z = handLoc.pose.position.z - location.pose.position.z;
                                XrVector3f hrelPos = RotateVector(headInv, hrelPosWorld);
                                XrQuaternionf hrelOri = MultiplyQuat(headInv, handLoc.pose.orientation);

                                XrPosef filteredHandPose{};
                                filteredHandPose.position = hrelPos;
                                filteredHandPose.orientation = hrelOri;
                                filteredHandPose = filterTrackingPose(
                                    m_handFilterState[i],
                                    filteredHandPose,
                                    GetHandTrackingSmooth(),
                                    0.0018f,
                                    0.0120f,
                                    0.0050f,
                                    0.0450f);

                                m_hands[i].posX = filteredHandPose.position.x;
                                m_hands[i].posY = filteredHandPose.position.y;
                                m_hands[i].posZ = filteredHandPose.position.z;
                                m_hands[i].oriX = filteredHandPose.orientation.x;
                                m_hands[i].oriY = filteredHandPose.orientation.y;
                                m_hands[i].oriZ = filteredHandPose.orientation.z;
                                m_hands[i].oriW = filteredHandPose.orientation.w;
                                m_hands[i].valid = true;
                                poseValid = true;

                                if (gameplayInputActive) {
                                    // Yaw of the controller relative to the recenter base
                                    // (= body forward). Used by hand-oriented locomotion.
                                    handRelOri = MultiplyQuat(baseInv, handLoc.pose.orientation);
                                }
                            }
                        }
                    }
                    if (!poseValid) {
                        m_handFilterState[i].initialized = false;
                    }

                    if (!gameplayInputActive) continue; // legacy pose-only path, no new bookkeeping

                    if (i == 0) ctrl.leftHandValid  = poseValid;
                    else        ctrl.rightHandValid = poseValid;

                    // Aim-pose yaw -- this is where the controller POINTS, not
                    // where the palm faces. grip-pose -Z is "away from palm",
                    // which is MIRRORED between left and right hands and gave
                    // inverted/diverging locomotion direction.
                    bool aimYawValid = false;
                    if (poseValid && m_handAimSpaces[i] != XR_NULL_HANDLE) {
                        XrSpaceLocation aimLoc{XR_TYPE_SPACE_LOCATION};
                        if (XR_SUCCEEDED(xrLocateSpace(m_handAimSpaces[i], m_localSpace, frameState.predictedDisplayTime, &aimLoc)) &&
                            (aimLoc.locationFlags & XR_SPACE_LOCATION_ORIENTATION_VALID_BIT)) {
                            const XrQuaternionf q = MultiplyQuat(baseInv, aimLoc.pose.orientation);
                            // Same yaw extraction as GetHmdYawRelToBody so both
                            // HMD-locomotion and Hand-locomotion use the SAME
                            // sign convention. atan2(fwd.x, -fwd.z) was sign-
                            // inverted relative to this and produced mirrored
                            // walking direction.
                            const float yaw = std::atan2(2.0f * (q.w * q.y + q.x * q.z),
                                                         1.0f - 2.0f * (q.y * q.y + q.z * q.z));
                            const float filteredYaw = filterTrackingAngle(
                                m_handAimYawFilter[i],
                                yaw,
                                GetHandTrackingSmooth(),
                                0.0040f,
                                0.0800f);
                            m_handYawRelToBody[i].store(filteredYaw, std::memory_order_relaxed);
                            m_handYawValid[i].store(true, std::memory_order_relaxed);
                            aimYawValid = true;
                        }
                    }
                    if (!aimYawValid) {
                        m_handAimYawFilter[i].initialized = false;
                        m_handYawValid[i].store(false, std::memory_order_relaxed);
                    }

                    // -- Gameplay inputs (trigger/grip/stick/buttons) --
                    if (m_thumbstickAction == XR_NULL_HANDLE) continue;
                    auto getFloat = [&](XrAction a) -> float {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = a;
                        gi.subactionPath = m_handPaths[i];
                        XrActionStateFloat st{XR_TYPE_ACTION_STATE_FLOAT};
                        if (XR_SUCCEEDED(xrGetActionStateFloat(m_session, &gi, &st)) && st.isActive)
                            return st.currentState;
                        return 0.0f;
                    };
                    auto getBool = [&](XrAction a) -> bool {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = a;
                        gi.subactionPath = m_handPaths[i];
                        XrActionStateBoolean st{XR_TYPE_ACTION_STATE_BOOLEAN};
                        if (XR_SUCCEEDED(xrGetActionStateBoolean(m_session, &gi, &st)) && st.isActive)
                            return st.currentState != XR_FALSE;
                        return false;
                    };
                    auto getVec2 = [&](XrAction a, float& outX, float& outY) {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = a;
                        gi.subactionPath = m_handPaths[i];
                        XrActionStateVector2f st{XR_TYPE_ACTION_STATE_VECTOR2F};
                        if (XR_SUCCEEDED(xrGetActionStateVector2f(m_session, &gi, &st)) && st.isActive) {
                            outX = st.currentState.x;
                            outY = st.currentState.y;
                        } else {
                            outX = 0.0f;
                            outY = 0.0f;
                        }
                    };

                    const float trig = getFloat(m_triggerAction);
                    const float grip = getFloat(m_gripAction);
                    float sx = 0.0f, sy = 0.0f;
                    getVec2(m_thumbstickAction, sx, sy);
                    const bool sclick = getBool(m_thumbstickClickAction);
                    const bool prim   = getBool(m_primaryButtonAction);
                    const bool sec    = getBool(m_secondaryButtonAction);

                    // XInput-compatible button bits so the hook can OR them into
                    // XINPUT_GAMEPAD.wButtons directly (XINPUT_GAMEPAD_*).
                    constexpr uint16_t XB_A              = 0x1000;
                    constexpr uint16_t XB_B              = 0x2000;
                    constexpr uint16_t XB_X              = 0x4000;
                    constexpr uint16_t XB_Y              = 0x8000;
                    constexpr uint16_t XB_LEFT_SHOULDER  = 0x0100;
                    constexpr uint16_t XB_RIGHT_SHOULDER = 0x0200;
                    constexpr uint16_t XB_LEFT_THUMB     = 0x0040;
                    constexpr uint16_t XB_RIGHT_THUMB    = 0x0080;

                    if (i == 0) {
                        ctrl.leftTrigger = trig;
                        ctrl.leftGrip    = grip;
                        ctrl.leftThumbX  = sx;
                        ctrl.leftThumbY  = sy;
                        if (sclick) ctrl.buttons |= XB_LEFT_THUMB;
                        if (prim)   ctrl.buttons |= XB_X;
                        if (sec)    ctrl.buttons |= XB_Y;
                        if (grip >= 0.7f) ctrl.buttons |= XB_LEFT_SHOULDER;
                    } else {
                        ctrl.rightTrigger = trig;
                        ctrl.rightGrip    = grip;
                        ctrl.rightThumbX  = sx;
                        ctrl.rightThumbY  = sy;
                        if (sclick) ctrl.buttons |= XB_RIGHT_THUMB;
                        if (prim)   ctrl.buttons |= XB_A;
                        if (sec)    ctrl.buttons |= XB_B;
                        // Right grip is RESERVED for the hand-to-holster equip system: a CET mod reads
                        // the grip value (shared[31] or similar) and the controller pose, and equips the
                        // weapon whose visual holster the hand is touching. Do NOT merge into XInput as
                        // ThrowGrenade — that would fire a grenade every time the player reaches for a
                        // holstered weapon.
                    }
                }

                if (gameplayInputActive) {
                    // Menu button is single (no per-hand binding) on Touch/Index/Vive/WMR.
                    if (m_menuButtonAction != XR_NULL_HANDLE) {
                        XrActionStateGetInfo gi{XR_TYPE_ACTION_STATE_GET_INFO};
                        gi.action = m_menuButtonAction;
                        gi.subactionPath = XR_NULL_PATH;
                        XrActionStateBoolean st{XR_TYPE_ACTION_STATE_BOOLEAN};
                        if (XR_SUCCEEDED(xrGetActionStateBoolean(m_session, &gi, &st)) && st.isActive && st.currentState)
                            ctrl.buttons |= 0x0010; // XINPUT_GAMEPAD_START
                    }

                    // Publish the snapshot for the XInput hook.
                    std::lock_guard<std::mutex> inLock(m_inputMutex);
                    m_controllerState = ctrl;
                }
            }

            // Rotate the head velocity into the same base-recentered frame as the
            // pose so GetHeadPose() can forward-predict (AER pose extrapolation).
            const bool angVelValid = (headVelocity.velocityFlags & XR_SPACE_VELOCITY_ANGULAR_VALID_BIT) != 0;
            const bool linVelValid = (headVelocity.velocityFlags & XR_SPACE_VELOCITY_LINEAR_VALID_BIT) != 0;
            if (angVelValid && linVelValid) {
                const XrVector3f angRel = RotateVector(baseInv, headVelocity.angularVelocity);
                const XrVector3f linRel = RotateVector(baseInv, headVelocity.linearVelocity);
                m_angVelX.store(angRel.x, std::memory_order_relaxed);
                m_angVelY.store(angRel.y, std::memory_order_relaxed);
                m_angVelZ.store(angRel.z, std::memory_order_relaxed);
                m_linVelX.store(linRel.x, std::memory_order_relaxed);
                m_linVelY.store(linRel.y, std::memory_order_relaxed);
                m_linVelZ.store(linRel.z, std::memory_order_relaxed);
                m_velValid.store(true, std::memory_order_relaxed);
            } else {
                m_velValid.store(false, std::memory_order_relaxed);
            }
        } else {
            m_headFilterState.initialized = false;
            m_velValid.store(false, std::memory_order_relaxed);
        }

        if (monoReady && viewCountOutput == m_eyeSwapchains.size()) {
            if (useAerSubmit) {
                ID3D12Resource* eyeSources[2] = {};
                ID3D12Resource* generatedSources[2] = {};  // synth texture per synth eye (null = raw)
                uint64_t eyeSerials[2] = {};
                uint64_t eyePairIds[2] = {};
                XrPosef eyePoses[2]{};
                XrFovf eyeFovs[2]{};
                bool eyeHasView[2] = {};
                ID3D12Resource* depthSource = nullptr;
                ID3D12Resource* r32depth = nullptr;   // CUDA-importable scene depth for the warp
                bool eyeIsSynth[2] = {false, false};
                bool anyEyeSynth = false;
                const uint32_t synthSlot = static_cast<uint32_t>(displayFrameIndex & 1ull);
                bool useInBetweenPair = false;
                uint32_t activeInBetweenSlot = 0;

                // ===== Frame-thread synth =====
                // Per eye, compute the CONTINUOUS blendFactor v24 from HIGH-RES QPC
                // timestamps (microsecond precision, signed math = no underflow):
                //   blend = (targetQpc - tPrev) / (tCurr - tPrev)
                // where tPrev/tCurr are the QPC of this eye's previous/current real
                // captures and targetQpc is the time this frame will be displayed.
                // blend≈1 -> raw current; ≈0 -> raw previous; (0,1) -> NvOF temporal
                // warp at that fraction so the synth lands exactly on display time
                // (no hardcoded 0.5 -> no head-rotation jitter). The warp is queued
                // AFTER the lock (ProcessTemporalFrame is non-blocking now).
                bool doSynth[2] = {false, false};
                float synthBlend[2] = {1.0f, 1.0f};
                float poseTargetBlend[2] = {1.0f, 1.0f};
                ID3D12Resource* sPrevTex[2] = {}, *sCurrTex[2] = {};
                ID3D12Resource* sPrevFlow[2] = {}, *sCurrFlow[2] = {};
                uint64_t sFlowConvVal[2] = {0, 0};  // max convert-fence value to GPU-Wait before NvOF
                ID3D12Resource* sPrevDepth[2] = {}, *sCurrDepth[2] = {};
                XrPosef sPrevPose[2]{}, sCurrPose[2]{}, sPredPose[2]{};
                XrFovf sFov[2]{};
                uint32_t sW[2] = {}, sH[2] = {};
                const bool v2 = (GetAERV2Enabled() != 0);

                // ANCHOR-TO-CAPTURE model (kills async present<->frame drift).
                // The engine renders mono alternate-eye, so each display frame one
                // eye is FRESH and the other is STALE by ~one capture interval. We
                // set the target time to the FRESHEST capture (max captureQpc), then:
                //   fresh eye  (tCurr == target)        -> blend≈1 -> raw
                //   stale eye  (tCurr <  target)        -> blend>1 -> FORWARD-warp it
                //                                          to the fresh eye's time.
                // Both eyes therefore represent the SAME instant -> coherent stereo,
                // and the target is anchored to real captures (no now+period drift,
                // no missed window). xr_pose_lag adds optional forward lead (periods)
                // to push the pair slightly toward actual display time.
                LARGE_INTEGER qpcFreq{};
                QueryPerformanceFrequency(&qpcFreq);
                uint64_t periodNs = m_predictedDisplayPeriodNs.load(std::memory_order_relaxed);
                if (periodNs == 0) periodNs = 11111111ull;  // 90 Hz default
                const int64_t periodTicks = (static_cast<int64_t>(periodNs) * qpcFreq.QuadPart) / 1000000000ll;
                // Forward lead in periods. Default xr_pose_lag=1 -> lead=0 so the
                // FRESH eye stays raw (target == its capture -> blend=1) and only
                // the STALE eye warps. Raising xr_pose_lag pushes the pair forward
                // toward real display time (more extrapolation, less latency).
                int lead = GetPoseLag() - 1; if (lead < 0) lead = 0;

                uint64_t capQpc[2] = {0, 0}, prevQpc[2] = {0, 0};
                bool haveHist[2] = {false, false};
                int64_t dbgBlendMilli[2] = {1000, 1000};
                int64_t dbgSpanUs[2] = {0, 0};

                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    for (int eye = 0; eye < 2; ++eye) {
                        eyeSerials[eye] = m_capturedEyeFrames[eye].serial;
                        eyePairIds[eye] = m_capturedEyeFrames[eye].pairId;
                        eyeHasView[eye] = m_capturedEyeFrames[eye].hasView;
                        capQpc[eye]  = m_capturedEyeFrames[eye].captureQpc;
                        prevQpc[eye] = m_previousCapturedEyeFrames[eye].captureQpc;
                        // Default: raw current capture (also the synth fallback).
                        if (m_capturedEyeFrames[eye].texture) {
                            eyeSources[eye] = m_capturedEyeFrames[eye].texture;
                            eyeSources[eye]->AddRef();
                            eyePoses[eye] = m_capturedEyeFrames[eye].pose;
                            eyeFovs[eye] = m_capturedEyeFrames[eye].fov;
                        }
                        // Unified producer: this is now the single synth producer for
                        // AER V2. The old OnPresent /
                        // ProcessAERV2Job worker producers are disabled for V2 (so no
                        // race), and OnPresent only captures + converts the per-eye
                        // NvOF input texture. Every 90 Hz display tick we warp each
                        // eye's latest real capture to ONE common target time with a
                        // continuous QPC blend (computed below), so both eyes land on
                        // the same instant. (v2 used to `continue` here, which left
                        // zero active producers = raw alternate-eye submit.)
                        // Snapshot the warp inputs unconditionally (decide raw vs
                        // synth AFTER the lock once the target is known).
                        haveHist[eye] =
                            m_previousCapturedEyeFrames[eye].texture && m_capturedEyeFrames[eye].texture &&
                            m_previousCapturedEyeFrames[eye].opticalFlowTexture &&
                            m_capturedEyeFrames[eye].opticalFlowTexture &&
                            m_capturedEyeFrames[eye].hasView && m_previousCapturedEyeFrames[eye].hasView &&
                            m_aerV2SynthEye[eye][synthSlot] != nullptr;
                        if (haveHist[eye]) {
                            sPrevTex[eye]  = m_previousCapturedEyeFrames[eye].texture;
                            sCurrTex[eye]  = m_capturedEyeFrames[eye].texture;
                            sPrevFlow[eye] = m_previousCapturedEyeFrames[eye].opticalFlowTexture;
                            sCurrFlow[eye] = m_capturedEyeFrames[eye].opticalFlowTexture;
                            // GPU-Wait target: both flow inputs must have finished their
                            // fire-and-forget convert before NvOF reads them.
                            {
                                const uint64_t pv = m_previousCapturedEyeFrames[eye].opticalFlowConvertValue;
                                const uint64_t cv = m_capturedEyeFrames[eye].opticalFlowConvertValue;
                                sFlowConvVal[eye] = pv > cv ? pv : cv;
                            }
                            if (m_previousCapturedEyeFrames[eye].depthTexture &&
                                m_previousCapturedEyeFrames[eye].depthSerial == m_previousCapturedEyeFrames[eye].serial)
                                sPrevDepth[eye] = m_previousCapturedEyeFrames[eye].depthTexture;
                            if (m_capturedEyeFrames[eye].depthTexture &&
                                m_capturedEyeFrames[eye].depthSerial == m_capturedEyeFrames[eye].serial)
                                sCurrDepth[eye] = m_capturedEyeFrames[eye].depthTexture;
                            sPrevPose[eye] = m_previousCapturedEyeFrames[eye].pose;
                            sCurrPose[eye] = m_capturedEyeFrames[eye].pose;
                            sFov[eye] = m_capturedEyeFrames[eye].fov;
                            sW[eye] = m_capturedEyeFrames[eye].width;
                            sH[eye] = m_capturedEyeFrames[eye].height;
                            sPrevTex[eye]->AddRef();  sCurrTex[eye]->AddRef();
                            sPrevFlow[eye]->AddRef(); sCurrFlow[eye]->AddRef();
                            if (sPrevDepth[eye]) sPrevDepth[eye]->AddRef();
                            if (sCurrDepth[eye]) sCurrDepth[eye]->AddRef();
                        }
                    }
                    if (m_depthLayerSupported && m_depthSnapshot && m_depthSnapshotSerial != 0) {
                        depthSource = m_depthSnapshot;
                        depthSource->AddRef();
                    }
                    // [DEPTH-AERV2] freshest CUDA-importable R32 depth for the warp.
                    if (m_depthSnapshotR32 && m_depthSnapshotR32Serial != 0) {
                        r32depth = m_depthSnapshotR32;
                        r32depth->AddRef();
                    }
                }

                // MODE-6 PER-EYE ALTERNATE-EYE.
                //
                // Each 90 Hz vsync, the eye that received a FRESH capture this
                // interval is submitted RAW (its captured texture @ captured pose;
                // the runtime's ATW reprojects it to display time -- same as mono).
                // The OTHER eye is NvOF-warped from its previous two SAME-EYE
                // captures (m_capturedEyeFrames[eye]/m_previousCapturedEyeFrames[eye]
                // = the per-eye source ring) to the display pose. With the engine
                // HMD-paced + the camera hook alternating, exactly one eye is fresh
                // per vsync -> each eye is real at 45 Hz, both eyes get a fresh
                // image at every 90 Hz submit.
                //
                // Replaces the old global isRealTick/synthTick pair-gate which
                // submitted BOTH eyes raw on a real tick (one up to ~22ms stale ->
                // temporal eye mismatch when moving) and BOTH warped on a synth
                // tick. Mode-6 always has one fresh + one warped -> no pair mismatch.
                static thread_local uint64_t s_lastEyePair[2] = {0, 0};
                LARGE_INTEGER mode6NowQpc; QueryPerformanceCounter(&mode6NowQpc);
                const int64_t mode6TargetQpc = mode6NowQpc.QuadPart + periodTicks +
                                                static_cast<int64_t>(lead) * periodTicks;
                // Fresh-window = 1.5 HMD periods. With the engine alternating eyes,
                // the eye captured this vsync has age~0 (fresh -> raw), the other
                // eye was captured ~2 periods ago (age>1.5 -> stale -> NvOF-warp).
                const int64_t freshWindowTicks = periodTicks + (periodTicks >> 1);
                for (int eye = 0; eye < 2; ++eye) {
                    // TIMESTAMP-BASED fresh detection (no eye0-first bias). An eye is
                    // "fresh" (raw submit) if it was captured within the fresh window.
                    // The old pairId-based test (eyePairIds[eye] > s_lastEyePair) was
                    // structurally biased: presentPairId increments ONLY on eye0
                    // capture, so eye0's pairId always led eye1's by 1, and with
                    // decoupled frame/present threads eye0 was detected "fresh" (raw,
                    // runtime-ATW-smooth) more often than eye1 (NvOF-warped, lower
                    // quality) -> the consistent "right eye worse than left" symptom.
                    // Actual capture age removes that bias: each eye gets an equal
                    // shot at being the raw eye based purely on temporal recency.
                    const int64_t ageTicks = mode6NowQpc.QuadPart - static_cast<int64_t>(capQpc[eye]);
                    const bool fresh = eyePairIds[eye] != 0 && ageTicks < freshWindowTicks;
                    s_lastEyePair[eye] = eyePairIds[eye];
                    const int64_t span =
                        static_cast<int64_t>(capQpc[eye]) - static_cast<int64_t>(prevQpc[eye]);
                    if (span > 0) dbgSpanUs[eye] = span * 1000000ll / qpcFreq.QuadPart;

                    if (!haveHist[eye] || span <= 0) {
                        // Warmup / no history: stays raw (captured texture).
                        continue;
                    }
                    // Blend to the display-target instant for THIS eye. Used as:
                    //   stale eye  -> image blend AND pose target
                    //   fresh eye  -> pose target only (image stays current-heavy)
                    // so the real eye can get a cheap pose-warp to display time.
                    double blend = static_cast<double>(mode6TargetQpc - static_cast<int64_t>(prevQpc[eye])) /
                                   static_cast<double>(span);
                    float maxExtrap = GetAerMaxExtrap();
                    if (maxExtrap < 1.0f) maxExtrap = 1.0f;
                    if (blend < 0.0) blend = 0.0;
                    if (blend > static_cast<double>(maxExtrap)) blend = static_cast<double>(maxExtrap);
                    // Small OF-overshoot bias. NvOF slightly over-extrapolates linear
                    // motion; subtract a small constant to pull the midpoint back.
                    blend -= 0.02;
                    if (blend < 0.0) blend = 0.0;
                    poseTargetBlend[eye] = static_cast<float>(blend);

                    if (fresh) {
                        // Idea #1 — lightweight real-eye improvement. When depth is
                        // enabled (GetAerXQueueWait!=0) we also run the FRESH eye
                        // through the warp pipeline, but keep imageBlend=1.0 so the
                        // CURRENT frame dominates while the pose target advances to
                        // the display instant (poseTargetBlend). This approximates a
                        // cheap pose-warp of the current frame: better left/right
                        // consistency and less ATW work, without the heavy temporal
                        // mixing of a normal stale-eye synth. When depth is off, raw
                        // + runtime ATW remains cheaper and usually better.
                        if (GetAerXQueueWait() != 0) {
                            doSynth[eye] = true;
                            synthBlend[eye] = 1.0f;
                        }
                        dbgBlendMilli[eye] = 1000;
                        continue;
                    }

                    // STALE eye: full NvOF temporal synth to the display target.
                    doSynth[eye] = true;
                    synthBlend[eye] = static_cast<float>(blend);
                    dbgBlendMilli[eye] = static_cast<int64_t>(blend * 1000.0);
                }
                // Suppress the now-unused legacy gate counters (kept for log compat).
                static thread_local uint64_t s_lastRealPairTickGate = 0; (void)s_lastRealPairTickGate;
                static thread_local int s_synthTickCount = 0; (void)s_synthTickCount;
                // Release the snapshot refs for eyes we ended up NOT synthesizing.
                for (int eye = 0; eye < 2; ++eye) {
                    if (haveHist[eye] && !doSynth[eye]) {
                        if (sPrevTex[eye]) { sPrevTex[eye]->Release(); sPrevTex[eye] = nullptr; }
                        if (sCurrTex[eye]) { sCurrTex[eye]->Release(); sCurrTex[eye] = nullptr; }
                        if (sPrevFlow[eye]) { sPrevFlow[eye]->Release(); sPrevFlow[eye] = nullptr; }
                        if (sCurrFlow[eye]) { sCurrFlow[eye]->Release(); sCurrFlow[eye] = nullptr; }
                        if (sPrevDepth[eye]) { sPrevDepth[eye]->Release(); sPrevDepth[eye] = nullptr; }
                        if (sCurrDepth[eye]) { sCurrDepth[eye]->Release(); sCurrDepth[eye] = nullptr; }
                    }
                }

                // [DLSS-MV] Copy the engine motion vectors into a CUDA-importable
                // shared scratch once per frame. Executed on m_d3dQueue so it is
                // FIFO-ordered BEFORE the pipeline's own m_d3dQueue->Signal that
                // CUDA waits on — no CPU sync needed. Failure -> mvSource stays
                // null -> kernel falls back to NvOF-only (haveMv=false).
                ID3D12Resource* mvSource = nullptr;
                float mvScaleX = NgxGetMvScaleX();
                float mvScaleY = NgxGetMvScaleY();
                if (anyEyeSynth) {
                    // Lazily create the DIRECT allocator+list used for the MV copy
                    // (the V2 worker that used to make these is disabled now).
                    if (!m_mvWarpAlloc || !m_mvWarpList) {
                        if (SUCCEEDED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_mvWarpAlloc))) &&
                            SUCCEEDED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_mvWarpAlloc.Get(), nullptr, IID_PPV_ARGS(&m_mvWarpList)))) {
                            m_mvWarpList->Close();
                            m_mvWarpAlloc->SetName(L"AERV2_mvcopy_alloc");
                            m_mvWarpList->SetName(L"AERV2_mvcopy_list");
                        } else {
                            m_mvWarpAlloc.Reset();
                            m_mvWarpList.Reset();
                        }
                    }
                    ID3D12Resource* engineMv = NgxAcquireMotionVectors();  // AddRef'd
                    if (engineMv && m_mvWarpAlloc && m_mvWarpList) {
                        const D3D12_RESOURCE_DESC mvDesc = engineMv->GetDesc();
                        bool recreate = !m_aerV2MvScratch;
                        if (m_aerV2MvScratch) {
                            const auto cur = m_aerV2MvScratch->GetDesc();
                            if (cur.Width != mvDesc.Width || cur.Height != mvDesc.Height || cur.Format != mvDesc.Format) {
                                m_aerV2MvScratch.Reset();
                                recreate = true;
                            }
                        }
                        if (recreate) {
                            D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                            if (FAILED(m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_SHARED, &mvDesc,
                                    D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&m_aerV2MvScratch)))) {
                                m_aerV2MvScratch.Reset();
                            } else {
                                SetD3DName(m_aerV2MvScratch.Get(), L"AERV2_engine_mv_scratch_shared");
                            }
                        }
                        if (m_aerV2MvScratch && SUCCEEDED(m_mvWarpAlloc->Reset()) &&
                            SUCCEEDED(m_mvWarpList->Reset(m_mvWarpAlloc.Get(), nullptr))) {
                            D3D12_RESOURCE_BARRIER b[2] = {};
                            b[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            b[0].Transition.pResource = engineMv;
                            b[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            b[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            b[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            b[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            b[1].Transition.pResource = m_aerV2MvScratch.Get();
                            b[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            b[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            b[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_mvWarpList->ResourceBarrier(2, b);
                            m_mvWarpList->CopyResource(m_aerV2MvScratch.Get(), engineMv);
                            std::swap(b[0].Transition.StateBefore, b[0].Transition.StateAfter);
                            std::swap(b[1].Transition.StateBefore, b[1].Transition.StateAfter);
                            m_mvWarpList->ResourceBarrier(2, b);
                            m_mvWarpList->Close();
                            ID3D12CommandList* lists[] = { m_mvWarpList.Get() };
                            m_d3dQueue->ExecuteCommandLists(1, lists);  // FIFO before pipeline signal
                            mvSource = m_aerV2MvScratch.Get();
                        }
                    }
                    if (engineMv) engineMv->Release();
                }

                // NOTE: a previous "matched stereo" override forced BOTH submitted
                // eyes (and the warp targets) onto the freshest eye's orientation.
                // With pose-pair-locking already rendering both eyes from eye0's
                // frozen head pose, that override fought the rendered content: the
                // stale eye's pixels carry eye0's orientation but were submitted at a
                // different (freshest) orientation, so the compositor reprojected the
                // delta -> severe one-eye "orbital" tearing on head turns. Reverted:
                // each eye keeps its OWN extrapolated pose, consistent with its
                // rendered+warped content.

                // POSE/IMAGE TIME CONSISTENCY (critical for no head-turn jitter).
                // The stale eye's submitted pose and its NvOF-warped IMAGE must
                // represent the SAME temporal instant, or the image "swims" vs the
                // pose on head turns -> whole-image jitter in both eyes. We derive
                // BOTH from the same source: the time-based blend over the eye's
                // prev/curr captures. The pose is ExtrapolatePose(prev,curr,blend)
                // and the warp kernel interpolates the image at that same blend, so
                // they agree by construction. The runtime's ATW then corrects the
                // small residual from this blend instant to actual photon display
                // (a uniform, smooth correction).
                //
                // Do NOT set the submitted pose to m_views[eye].pose (the runtime's
                // predictedDisplayTime pose) while the image blend targets a
                // different instant (now+period): that desync was the head-turn
                // jitter. m_views is display-anchored, the capture-based blend is
                // capture-anchored, and without an epoch-exact XrTime->QPC map
                // they cannot be safely aligned -- so keep pose+image both
                // capture-anchored and let ATW do the rest.

                // Run the per-eye warp OUTSIDE the lock (non-blocking GPU queue).
                for (int eye = 0; eye < 2; ++eye) {
                    if (!doSynth[eye]) continue;
                    if (!m_aerV2Pipeline) m_aerV2Pipeline = std::make_unique<aer_v2::AerV2Pipeline>();
                    m_aerV2Pipeline->SetMode(aer_v2::AerV2Pipeline::Mode::AERv2HighQ);
                    m_aerV2Pipeline->SetForceMatchedEyePoses(true);
                    m_aerV2Pipeline->SetWarpTuning(GetAerRefineStrength(), GetAerOcclusionSharp(), GetAerFoveation(), GetFlowSmooth());
                    // Pose target for this warp. Normally equals synthBlend
                    // (stale eye: pose and image at the same extrapolated instant),
                    // but for Idea #1 the fresh eye can keep imageBlend=1.0
                    // (current-heavy) while still advancing its pose target toward
                    // the display instant via poseTargetBlend.
                    sPredPose[eye] = ExtrapolatePose(sPrevPose[eye], sCurrPose[eye], poseTargetBlend[eye]);
                    bool ok = false;
                    if (m_d3dDevice && m_d3dQueue && sW[eye] && sH[eye] &&
                        m_aerV2Pipeline->EnsureInitialized(m_d3dDevice, m_d3dQueue, sW[eye], sH[eye])) {
                        // GPU-Wait for the fire-and-forget flow-input conversion to
                        // finish before NvOF reads it. Issued on m_d3dQueue, which the
                        // pipeline uses for its own CUDA fence handshake, so the warp's
                        // CUDA work transitively waits for the conversion. No CPU stall.
                        if (sFlowConvVal[eye] != 0 && m_opticalFlow) {
                            if (ID3D12Fence* cf = m_opticalFlow->GetConvertFence()) {
                                m_d3dQueue->Wait(cf, sFlowConvVal[eye]);
                            }
                        }
                        ok = m_aerV2Pipeline->ProcessTemporalFrame(
                            eye,
                            sPrevFlow[eye], sCurrFlow[eye], sPrevTex[eye], sCurrTex[eye],
                            mvSource, r32depth, r32depth, mvScaleX, mvScaleY,
                            sFov[eye], sPrevPose[eye], sCurrPose[eye], sPredPose[eye],
                            synthBlend[eye], m_aerV2SynthEye[eye][synthSlot].Get());
                    }
                    if (ok) {
                        if (eyeSources[eye]) eyeSources[eye]->Release();
                        eyeSources[eye] = m_aerV2SynthEye[eye][synthSlot].Get();
                        eyeSources[eye]->AddRef();
                        generatedSources[eye] = m_aerV2SynthEye[eye][synthSlot].Get();
                        generatedSources[eye]->AddRef();
                        eyePoses[eye] = sPredPose[eye];
                        eyeFovs[eye] = sFov[eye];
                        eyeIsSynth[eye] = true;
                        anyEyeSynth = true;
                    }
                    if (sPrevTex[eye]) sPrevTex[eye]->Release();
                    if (sCurrTex[eye]) sCurrTex[eye]->Release();
                    if (sPrevFlow[eye]) sPrevFlow[eye]->Release();
                    if (sCurrFlow[eye]) sCurrFlow[eye]->Release();
                    if (sPrevDepth[eye]) sPrevDepth[eye]->Release();
                    if (sCurrDepth[eye]) sCurrDepth[eye]->Release();
                }
                const bool depthAvail = (r32depth != nullptr);
                if (r32depth) { r32depth->Release(); r32depth = nullptr; }
                if (g_verboseLog && v2 && (displayFrameIndex % 300) == 1) {
                    Log("OpenXRManager: [AER V2] depthAware=%d dlssMv=%d foveation=%.2f (mvScale=%.3f,%.3f)\n",
                        depthAvail ? 1 : 0, mvSource ? 1 : 0, GetAerFoveation(), mvScaleX, mvScaleY);
                }
                if (g_verboseLog && v2 && (displayFrameIndex % 300) == 1) {
                    // Anchor-to-capture: target = freshest capture (+lead). One eye
                    // is fresh (blend≈1 -> raw), the other is stale (blend>1 ->
                    // FORWARD-warped to match). blendMilli is blend*1000; span = that
                    // eye's real capture interval (us). Healthy: one eye ~1000, the
                    // other >1100 and synth=1.
                    Log("OpenXRManager: [AER V2] idx=%llu lead=%d blendL=%.3f blendR=%.3f synthL=%d synthR=%d | L:span=%lldus blendx1k=%lld R:span=%lldus blendx1k=%lld slot=%u\n",
                        static_cast<unsigned long long>(displayFrameIndex), lead,
                        synthBlend[0], synthBlend[1], eyeIsSynth[0] ? 1 : 0, eyeIsSynth[1] ? 1 : 0,
                        static_cast<long long>(dbgSpanUs[0]), static_cast<long long>(dbgBlendMilli[0]),
                        static_cast<long long>(dbgSpanUs[1]), static_cast<long long>(dbgBlendMilli[1]), synthSlot);
                }

                // ===== 1:1 half-rate consumer: on the in-between display interval,
                // submit the BOTH-eye NvOF-produced pair instead of resubmitting a
                // raw/current pair with one stale eye. This is the key cadence step
                // that gives both eyes a fresh image every display interval.
                if (v2 && GetAERHalfRate() != 0) {
                    uint64_t inBetweenPairId = 0;
                    XrPosef inBetweenPoses[2]{};
                    XrFovf inBetweenFovs[2]{};
                    bool inBetweenViews[2] = {};
                    {
                        std::lock_guard<std::mutex> lock(m_presentMutex);
                        const uint64_t currentPair =
                            (eyePairIds[0] != 0 && eyePairIds[0] == eyePairIds[1]) ? eyePairIds[0] : 0;
                        if (m_inBetweenReadyPairId != 0 &&
                            currentPair == m_lastSubmittedPairId &&
                            m_inBetweenReadyPairId > m_inBetweenShownForPairId) {
                            useInBetweenPair = true;
                            inBetweenPairId = m_inBetweenReadyPairId;
                            activeInBetweenSlot = m_inBetweenSlot;
                            for (int eye = 0; eye < 2; ++eye) {
                                inBetweenPoses[eye] = m_inBetweenEyePoses[eye];
                                inBetweenFovs[eye] = m_inBetweenEyeFovs[eye];
                                inBetweenViews[eye] = m_inBetweenEyeViewsValid[eye];
                            }
                        }
                    }
                    if (useInBetweenPair) {
                        for (int eye = 0; eye < 2; ++eye) {
                            if (eyeSources[eye]) { eyeSources[eye]->Release(); eyeSources[eye] = nullptr; }
                            if (generatedSources[eye]) { generatedSources[eye]->Release(); generatedSources[eye] = nullptr; }
                            if (m_aerV2InBetween[eye][activeInBetweenSlot]) {
                                eyeSources[eye] = m_aerV2InBetween[eye][activeInBetweenSlot].Get();
                                eyeSources[eye]->AddRef();
                                generatedSources[eye] = m_aerV2InBetween[eye][activeInBetweenSlot].Get();
                                generatedSources[eye]->AddRef();
                                eyePoses[eye] = inBetweenPoses[eye];
                                eyeFovs[eye] = inBetweenFovs[eye];
                                eyeHasView[eye] = inBetweenViews[eye];
                                eyeIsSynth[eye] = true;
                                anyEyeSynth = true;
                                // Keep the pair ids coherent for submit telemetry.
                                eyePairIds[eye] = inBetweenPairId;
                            }
                        }
                        m_inBetweenShownForPairId = inBetweenPairId;
                    }
                }

                const uint64_t submitSerial = eyeSerials[0] > eyeSerials[1] ? eyeSerials[0] : eyeSerials[1];
                const bool completePair = GetAERPairGate() == 0 || (eyePairIds[0] != 0 && eyePairIds[0] == eyePairIds[1]);
                const bool submitReadyAERV2 =
                    eyeSources[0] && eyeSources[1] &&
                    eyeSerials[0] != 0 && eyeSerials[1] != 0 &&
                    eyeHasView[0] && eyeHasView[1];
                // Phase 2d: substitute the OLDER eye in the current pair with
                // the worker-produced MV-warped frame. The warp advances that
                // eye's image one game frame forward using engine motion
                // vectors, so the submitted pair shares a single timestamp.
                // We addref a private snapshot of m_mvWarpedEye here for the
                // submit copy below; the slot itself is reused by the next
                // worker pass (worker's m_mvWarpEvent wait guarantees the GPU
                // write is complete before this addref/copy starts).
                ID3D12Resource* mvWarpedReplacement[2] = {nullptr, nullptr};
                const bool aerV2On = GetAERV2Enabled() != 0;
                if (!aerV2On && completePair && eyePairIds[0] != 0) {
                    for (int eye = 0; eye < 2; ++eye) {
                        if (m_mvWarpedValidPairId[eye].load(std::memory_order_acquire) == eyePairIds[0] &&
                            m_mvWarpedEye[eye]) {
                            m_mvWarpedEye[eye]->AddRef();
                            mvWarpedReplacement[eye] = m_mvWarpedEye[eye].Get();
                        }
                    }
                }
                
                // Reuse-last-frame path: OpenXR cannot "skip submit" without showing
                // nothing, so stale ticks must re-submit the last clean eye image.

                m_cmdAllocatorIndex = (m_cmdAllocatorIndex + 1) % 3;
                ID3D12CommandAllocator* currentAllocator = m_cmdAllocators[m_cmdAllocatorIndex];
                if (m_fenceValue >= 3 && m_fence->GetCompletedValue() < m_fenceValue - 2) {
                    m_fence->SetEventOnCompletion(m_fenceValue - 2, m_fenceEvent);
                    WaitForSingleObject(m_fenceEvent, INFINITE);
                }

                ID3D12GraphicsCommandList* m_cmdList = m_cmdLists[m_cmdAllocatorIndex];

                // Reuse the last clean frame on stale ticks instead of re-warping stale
                // content again.
                const bool reuseLastFrame = ReuseLastFrameOutputEnabled() && useAerSubmit &&
                    submitSerial != 0 && submitSerial == m_lastSubmittedSerial &&
                    m_lastGoodValid && m_lastGoodEye[0] && m_lastGoodEye[1];

                if (eyeSources[0] && eyeSources[1] && eyeSerials[0] != 0 && eyeSerials[1] != 0 && (aerV2On ? submitReadyAERV2 : completePair) && eyeHasView[0] && eyeHasView[1] &&
                    SUCCEEDED(currentAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(currentAllocator, nullptr))) {
                    bool copyReady = true;
                    bool releaseOk = true;
                    bool useDepthLayer = (depthSource != nullptr) && m_depthLayerSupported && !reuseLastFrame;
                    // alternate-eye AER V2 needs depth that matches the synthetic
                    // midpoint image. Feeding the compositor raw-frame depth for a
                    // synthetic color frame creates severe mismatches on nearby
                    // geometry (experienced as black/incorrect synthetic flashes).
                    // Until we synthesize matching midpoint depth, disable the XR
                    // depth layer on synthetic submits and let the compositor use
                    // color-only async warp for those frames.
                    if (anyEyeSynth) {
                        useDepthLayer = false;
                    }

                    // Phase 3: depth-based stereo reprojection. Identify the
                    // FRESHER eye in the pair (higher serial = newer capture)
                    // and synthesize the other from it + scene depth. This
                    // collapses the inter-eye sim gap (the OLDER eye is now
                    // replaced with a same-timestamp view derived from the
                    // FRESHER one).
                    bool stereoSynthOk = false;
                    int stereoSynthForEye = -1;  // which eye gets the synthesized texture
                    // AER V2 does not use the older D3D12 stereoSynth
                    // reprojection pass. In AER V2 mode the only synthetic
                    // image source must be the CUDA/NvOF pipeline below.
                    if (!aerV2On && depthSource && m_stereoSynthEye && m_d3dDevice) {
                        if (!m_stereoReproject) m_stereoReproject = std::make_unique<StereoReproject>();
                        const D3D12_RESOURCE_DESC stereoDesc = m_stereoSynthEye->GetDesc();
                        if (m_stereoReproject->EnsureInitialized(m_d3dDevice,
                                stereoDesc.Format,
                                static_cast<uint32_t>(stereoDesc.Width),
                                stereoDesc.Height)) {
                            const int fresherEye = (eyeSerials[0] >= eyeSerials[1]) ? 0 : 1;
                            const int targetEye = fresherEye ^ 1;
                            ID3D12Resource* srcColor = eyeSources[fresherEye];
                            ID3D12Resource* srcDepth = depthSource;

                            // FOV in radians (use the fresher eye's fov_x).
                            float fovLeft = std::fabs(eyeFovs[fresherEye].angleLeft);
                            float fovRight = std::fabs(eyeFovs[fresherEye].angleRight);
                            float horizFovRad = fovLeft + fovRight;
                            if (horizFovRad < 0.1f || horizFovRad > 3.14f) horizFovRad = 1.658f; // ~95°
                            // IPD: use runtime view positions if available, else default 0.065.
                            float ipdMeters = 0.065f;
                            if (m_views.size() >= 2) {
                                float dx = m_views[1].pose.position.x - m_views[0].pose.position.x;
                                float dy = m_views[1].pose.position.y - m_views[0].pose.position.y;
                                float dz = m_views[1].pose.position.z - m_views[0].pose.position.z;
                                float distance = std::sqrt(dx*dx + dy*dy + dz*dz);
                                if (distance > 0.04f && distance < 0.10f) ipdMeters = distance;
                            }
                            // CP2077 near plane (per FinalCamera log f40=0.02 observed).
                            const float nearZ = 0.02f;
                            // Convention: when synthesizing right(=eye1) from left(=eye0),
                            // sample further LEFT in source for each output pixel →
                            // positive sign. When synthesizing left from right → negative.
                            const float signLR = (targetEye == 1) ? +1.0f : -1.0f;

                            // Barriers: src color from COPY_SOURCE → PSR; depth
                            // snapshot from COPY_SOURCE → PSR; synth scratch from
                            // COMMON → RENDER_TARGET.
                            D3D12_RESOURCE_BARRIER pre[3] = {};
                            UINT preBc = 0;
                            auto addBar = [&](D3D12_RESOURCE_BARRIER* arr, UINT& bc, ID3D12Resource* r,
                                              D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
                                if (!r || before == after) return;
                                arr[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                                arr[bc].Transition.pResource = r;
                                arr[bc].Transition.StateBefore = before;
                                arr[bc].Transition.StateAfter = after;
                                arr[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                                ++bc;
                            };
                            addBar(pre, preBc, srcColor, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                            addBar(pre, preBc, srcDepth, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                            addBar(pre, preBc, m_stereoSynthEye.Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
                            if (preBc > 0) m_cmdList->ResourceBarrier(preBc, pre);

                            stereoSynthOk = m_stereoReproject->RecordReproject(
                                m_cmdList, srcColor, srcDepth, m_stereoSynthEye.Get(),
                                ipdMeters, horizFovRad, nearZ, signLR);

                            D3D12_RESOURCE_BARRIER post[3] = {};
                            UINT postBc = 0;
                            addBar(post, postBc, srcColor, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                            addBar(post, postBc, srcDepth, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                            addBar(post, postBc, m_stereoSynthEye.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
                            if (postBc > 0) m_cmdList->ResourceBarrier(postBc, post);

                            if (stereoSynthOk) stereoSynthForEye = targetEye;
                            static uint64_t s_logCounter = 0;
                            if (g_verboseLog && (s_logCounter++ % 300) == 0) {
                                Log("OpenXRManager: [stereoSynth] pair=%llu fresherEye=%d targetEye=%d ipd=%.4f fovH=%.3f synth=%d\n",
                                    static_cast<unsigned long long>(eyePairIds[0]),
                                    fresherEye, targetEye, ipdMeters, horizFovRad,
                                    stereoSynthOk ? 1 : 0);
                            }
                        }
                    }
                    std::vector<bool> acquiredEyes(viewCountOutput, false);
                    std::vector<bool> acquiredDepthEyes(viewCountOutput, false);
                    std::vector<XrCompositionLayerProjectionView> projectionViews(viewCountOutput);
                    std::vector<XrCompositionLayerDepthInfoKHR> depthInfos;
                    bool submittedSynthetic = false;
                    for (uint32_t i = 0; i < viewCountOutput; ++i) {
                        projectionViews[i] = {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW};
                    }
                    if (useDepthLayer) {
                        depthInfos.resize(viewCountOutput);
                        for (uint32_t i = 0; i < viewCountOutput; ++i) {
                            depthInfos[i] = {XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR};
                        }
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        uint32_t sourceEye = eye;
                        const int debugEye = GetAERDebugEye();
                        if (debugEye == 1) {
                            sourceEye = 1;
                        } else if (debugEye == 2) {
                            sourceEye = 0;
                        } else if (debugEye == 3) {
                            sourceEye = eye ^ 1;
                        }

                        uint32_t imageIndex = 0;
                        XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                        const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].handle, &acquireInfo, &imageIndex);
                        if (XR_FAILED(acquireRes)) {
                            Log("OpenXRManager: xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                            copyReady = false;
                            break;
                        }
                        acquiredEyes[eye] = true;

                        XrSwapchainImageWaitInfo waitSwapchainInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                        waitSwapchainInfo.timeout = XR_INFINITE_DURATION;
                        const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].handle, &waitSwapchainInfo);
                        if (XR_FAILED(waitRes)) {
                            Log("OpenXRManager: xrWaitSwapchainImage failed for eye %u (res=%d)\n", eye, waitRes);
                            copyReady = false;
                            break;
                        }

                        ID3D12Resource* texture = m_eyeSwapchains[eye].images[imageIndex].texture;
                        ID3D12Resource* copySource = nullptr;
                        const bool syntheticSource = !reuseLastFrame && aerV2On &&
                            sourceEye < 2 &&
                            eyeIsSynth[sourceEye] &&
                            generatedSources[sourceEye] != nullptr;
                        if (syntheticSource) {
                            submittedSynthetic = true;
                        }
                        if (sourceEye < 2) {
                            if (aerV2On) {
                                copySource = syntheticSource
                                    ? generatedSources[sourceEye]
                                    : eyeSources[sourceEye];
                            } else if (static_cast<int>(sourceEye) == stereoSynthForEye && m_stereoSynthEye) {
                                copySource = m_stereoSynthEye.Get();
                            } else {
                                copySource = eyeSources[sourceEye];
                            }
                        }
                        if (reuseLastFrame && sourceEye < 2) {
                            copySource = m_lastGoodEye[sourceEye].Get();
                        }
                        if (!texture || sourceEye >= 2 || !copySource) {
                            Log("OpenXRManager: AER source/target missing for eye %u sourceEye %u image %u\n", eye, sourceEye, imageIndex);
                            copyReady = false;
                            break;
                        }

                        if (syntheticSource) {
                            // Synth eye: copy via the per-eye submit scratch so the
                            // CUDA-written m_aerV2SynthEye is never read concurrently.
                            const uint32_t scratchSlot = useInBetweenPair ? activeInBetweenSlot : synthSlot;
                            ID3D12Resource* submitScratch = useInBetweenPair
                                ? m_aerV2InBetweenSubmit[sourceEye][scratchSlot].Get()
                                : m_aerV2SubmitEye[sourceEye][scratchSlot].Get();
                            bool* scratchReady = useInBetweenPair
                                ? &m_aerV2InBetweenSubmitReady[sourceEye][scratchSlot]
                                : &m_aerV2SubmitEyeReady[sourceEye][scratchSlot];
                            if (!submitScratch) {
                                Log("OpenXRManager: AER submit scratch missing for eye %u slot %u\n",
                                    sourceEye, scratchSlot);
                                copyReady = false;
                                break;
                            }
                            D3D12_RESOURCE_BARRIER preScratch{};
                            preScratch.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            preScratch.Transition.pResource = submitScratch;
                            preScratch.Transition.StateBefore = *scratchReady
                                ? D3D12_RESOURCE_STATE_COPY_SOURCE
                                : D3D12_RESOURCE_STATE_COMMON;
                            preScratch.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            preScratch.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &preScratch);

                            m_cmdList->CopyResource(submitScratch, copySource);

                            D3D12_RESOURCE_BARRIER postScratch{};
                            postScratch.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            postScratch.Transition.pResource = submitScratch;
                            postScratch.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            postScratch.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            postScratch.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &postScratch);
                            *scratchReady = true;
                            copySource = submitScratch;
                        }

                        // CAS sharpen: when enabled, draw the sharpened source straight
                        // into the swapchain image instead of
                        // a plain copy. Source state differs by path (synth scratch is
                        // COPY_SOURCE, raw capture is COMMON), so transition from that.
                        const float sharpStrength = GetVrSharpness();
                        bool doSharpen = false;
                        // TEMP-DISABLED: in-submit CAS GPU-crashes (device removal) on
                        // this runtime (VirtualDesktop OpenXR) -- rendering an RTV into
                        // the runtime swapchain image and/or sampling the capture as SRV
                        // faults. Needs a dedicated SRV-capable scratch (copy source ->
                        // own RT, sharpen that -> swapchain). Gated behind CPVR_CAS_ENABLE
                        // until reworked; the Sharpen slider is inert meanwhile.
                        if (std::getenv("CPVR_CAS_ENABLE") && sharpStrength > 0.0001f && m_d3dDevice && texture) {
                            if (!m_sharpenPass) m_sharpenPass = std::make_unique<SharpenPass>();
                            const D3D12_RESOURCE_DESC sd = texture->GetDesc();
                            m_sharpenReady = m_sharpenPass->EnsureInitialized(
                                m_d3dDevice, sd.Format,
                                static_cast<uint32_t>(sd.Width), sd.Height);
                            doSharpen = m_sharpenReady;
                        }
                        if (doSharpen) {
                            // Both paths leave copySource in COPY_SOURCE: the raw eye
                            // capture rests in COPY_SOURCE (CapturePresentedFrame), and
                            // the synth scratch was just transitioned to COPY_SOURCE.
                            // (Assuming COMMON for raw was the CAS crash / device-removal.)
                            const D3D12_RESOURCE_STATES srcPrev = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            D3D12_RESOURCE_BARRIER pre[2] = {};
                            pre[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[0].Transition.pResource = copySource;
                            pre[0].Transition.StateBefore = srcPrev;
                            pre[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            pre[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            pre[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[1].Transition.pResource = texture;
                            pre[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            pre[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            pre[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, pre);

                            m_sharpenPass->RecordSharpen(m_cmdList, copySource, texture,
                                                         sharpStrength, GetVrSharpmix());

                            D3D12_RESOURCE_BARRIER post[2] = {};
                            post[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[0].Transition.pResource = texture;
                            post[0].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            post[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            post[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            post[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[1].Transition.pResource = copySource;
                            post[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            post[1].Transition.StateAfter = srcPrev;
                            post[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, post);
                        } else {
                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = texture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            m_cmdList->CopyResource(texture, copySource);

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = texture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);
                        }

                        // AER V2 submits the synthetic eye with its own predicted
                        // pose/FOV, not with the raw source eye's pose. The older debug/raw
                        // source-eye remap logic is only valid for non-synthetic paths.
                        if (syntheticSource) {
                            projectionViews[eye].pose = eyePoses[sourceEye];
                            projectionViews[eye].fov = eyeFovs[sourceEye];
                        } else {
                            const uint32_t poseEye = (debugEye == 1 || debugEye == 2 || debugEye == 3) ? sourceEye : eye;
                            if (poseEye < 2) {
                                projectionViews[eye].pose = eyePoses[poseEye];
                                projectionViews[eye].fov = eyeFovs[poseEye];
                            } else {
                                projectionViews[eye].pose = eyePoses[eye];
                                projectionViews[eye].fov = eyeFovs[eye];
                            }
                        }
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;

                        // Re-present: use the stashed pose so the runtime reprojects the
                        // last-good image to the current head. Normal: capture this eye
                        // into the persistent last-good texture
                        // (CopyResource from copySource, which rests in COPY_SOURCE) and
                        // stash its pose/fov for future re-presents.
                        if (reuseLastFrame && sourceEye < 2) {
                            projectionViews[eye].pose = m_lastGoodPose[sourceEye];
                            projectionViews[eye].fov = m_lastGoodFov[sourceEye];
                        } else if (ReuseLastFrameOutputEnabled() && sourceEye < 2 && copySource && m_d3dDevice) {
                            if (!m_lastGoodEye[sourceEye]) {
                                const D3D12_RESOURCE_DESC td = texture->GetDesc();
                                D3D12_HEAP_PROPERTIES hp{}; hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                                D3D12_RESOURCE_DESC rd{};
                                rd.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
                                rd.Width = td.Width; rd.Height = td.Height;
                                rd.DepthOrArraySize = 1; rd.MipLevels = 1;
                                rd.Format = td.Format; rd.SampleDesc.Count = 1;
                                rd.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
                                m_d3dDevice->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &rd,
                                    D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&m_lastGoodEye[sourceEye]));
                                m_lastGoodEyeInited[sourceEye] = false;
                            }
                            if (m_lastGoodEye[sourceEye]) {
                                D3D12_RESOURCE_BARRIER b{};
                                b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                                b.Transition.pResource = m_lastGoodEye[sourceEye].Get();
                                b.Transition.StateBefore = m_lastGoodEyeInited[sourceEye]
                                    ? D3D12_RESOURCE_STATE_COPY_SOURCE : D3D12_RESOURCE_STATE_COMMON;
                                b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                                b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                                m_cmdList->ResourceBarrier(1, &b);
                                m_cmdList->CopyResource(m_lastGoodEye[sourceEye].Get(), copySource);
                                b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                                b.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                                m_cmdList->ResourceBarrier(1, &b);
                                m_lastGoodEyeInited[sourceEye] = true;
                                m_lastGoodPose[sourceEye] = projectionViews[eye].pose;
                                m_lastGoodFov[sourceEye] = projectionViews[eye].fov;
                            }
                        }
                    }

                    if (!reuseLastFrame && copyReady &&
                        m_lastGoodEyeInited[0] && m_lastGoodEyeInited[1]) {
                        m_lastGoodValid = true;
                    }

                    // [DEPTH] Acquire each eye's depth swapchain, copy the scene-depth
                    // snapshot into it, and chain XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR
                    // onto the projection view. Same shared snapshot for both eyes (the
                    // game renders one depth buffer per present). Reversed-Z is encoded via
                    // nearZ > farZ, NOT by swapping min/max depth. Mirrors the mono path.
                    if (copyReady && useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (m_eyeSwapchains[eye].depthHandle == XR_NULL_HANDLE) {
                                Log("OpenXRManager: [DEPTH] AER depthHandle missing for eye %u\n", eye);
                                useDepthLayer = false;
                                break;
                            }
                            uint32_t depthImageIndex = 0;
                            XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                            const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].depthHandle, &acquireInfo, &depthImageIndex);
                            if (XR_FAILED(acquireRes)) {
                                Log("OpenXRManager: [DEPTH] AER xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                                useDepthLayer = false;
                                break;
                            }
                            acquiredDepthEyes[eye] = true;
                            XrSwapchainImageWaitInfo waitInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                            waitInfo.timeout = XR_INFINITE_DURATION;
                            const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].depthHandle, &waitInfo);
                            if (XR_FAILED(waitRes)) {
                                Log("OpenXRManager: [DEPTH] AER xrWaitSwapchainImage failed for eye %u (res=%d)\n", eye, waitRes);
                                useDepthLayer = false;
                                break;
                            }
                            ID3D12Resource* depthTexture = m_eyeSwapchains[eye].depthImages[depthImageIndex].texture;
                            if (!depthTexture) {
                                Log("OpenXRManager: [DEPTH] AER depth swapchain texture missing for eye %u image %u\n", eye, depthImageIndex);
                                useDepthLayer = false;
                                break;
                            }
                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = depthTexture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            const D3D12_RESOURCE_DESC depthSrcDesc = depthSource->GetDesc();
                            const D3D12_RESOURCE_DESC depthDstDesc = depthTexture->GetDesc();
                            if (depthSrcDesc.Format == depthDstDesc.Format) {
                                m_cmdList->CopyResource(depthTexture, depthSource);
                            } else {
                                D3D12_TEXTURE_COPY_LOCATION dstLoc{};
                                dstLoc.pResource = depthTexture;
                                dstLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                dstLoc.SubresourceIndex = 0;
                                D3D12_TEXTURE_COPY_LOCATION srcLoc{};
                                srcLoc.pResource = depthSource;
                                srcLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                srcLoc.SubresourceIndex = 0;
                                m_cmdList->CopyTextureRegion(&dstLoc, 0, 0, 0, &srcLoc, nullptr);
                            }

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = depthTexture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);

                            depthInfos[eye].subImage.swapchain = m_eyeSwapchains[eye].depthHandle;
                            depthInfos[eye].subImage.imageRect.offset = {0, 0};
                            depthInfos[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                            depthInfos[eye].subImage.imageArrayIndex = 0;
                            depthInfos[eye].minDepth = 0.0f;
                            depthInfos[eye].maxDepth = 1.0f;
                            depthInfos[eye].nearZ = 10000.0f;
                            depthInfos[eye].farZ = 0.01f;
                            projectionViews[eye].next = &depthInfos[eye];
                        }
                    } else {
                        useDepthLayer = false;
                    }

                    // Phase 3: return stereo-synth scratch to COMMON for the
                    // next pair's reproject pre-barrier.
                    if (stereoSynthOk && m_stereoSynthEye) {
                        D3D12_RESOURCE_BARRIER toCommon{};
                        toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        toCommon.Transition.pResource = m_stereoSynthEye.Get();
                        toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                        toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                        toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                        m_cmdList->ResourceBarrier(1, &toCommon);
                    }

                    m_cmdList->Close();
                    ID3D12CommandList* cmdLists[] = {m_cmdList};
                    m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                    
                    ++m_fenceValue;
                    m_d3dQueue->Signal(m_fence, m_fenceValue);

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        if (!acquiredEyes[eye]) {
                            continue;
                        }
                        XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                        const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                        if (XR_FAILED(releaseRes)) {
                            Log("OpenXRManager: xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                            releaseOk = false;
                        }
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        if (!acquiredDepthEyes[eye]) {
                            continue;
                        }
                        XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                        const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].depthHandle, &releaseInfo);
                        if (XR_FAILED(releaseRes)) {
                            Log("OpenXRManager: [DEPTH] AER xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                            useDepthLayer = false;
                        }
                    }

                    if (!useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            projectionViews[eye].next = nullptr;
                        }
                    }

                    if (copyReady && releaseOk) {
                        XrCompositionLayerProjection layerProj{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
                        XrCompositionLayerQuad layerQuad{XR_TYPE_COMPOSITION_LAYER_QUAD};
                        const XrCompositionLayerBaseHeader* layers[1] = {nullptr};

                        if (menuRectActive) {
                            layerQuad.space = m_localSpace;
                            layerQuad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
                            layerQuad.subImage = projectionViews[0].subImage;

                            XrPosef menuPose{};
                            menuPose.orientation.w = 1.0f;
                            if (headPoseLocated) {
                                menuPose = location.pose;
                            } else if (m_basePoseSet) {
                                menuPose = m_basePose;
                            }
                            XrQuaternionf baseOri = menuPose.orientation;
                             
                            // Flatten the orientation to pure yaw to keep the menu perfectly vertical
                            float fx = -2.0f * (baseOri.x * baseOri.z + baseOri.y * baseOri.w);
                            float fz = 2.0f * (baseOri.x * baseOri.x + baseOri.y * baseOri.y) - 1.0f;
                            float flatYaw = atan2f(-fx, -fz);
                            XrQuaternionf qYaw = {0.0f, sinf(flatYaw * 0.5f), 0.0f, cosf(flatYaw * 0.5f)};

                            XrQuaternionf menuOri = qYaw;

                            XrVector3f fwd = {0.0f, 0.0f, -1.5f};
                            XrVector3f rotatedFwd = RotateVector(qYaw, fwd);

                            layerQuad.pose.orientation = menuOri;
                            layerQuad.pose.position.x = menuPose.position.x + rotatedFwd.x;
                            layerQuad.pose.position.y = menuPose.position.y + rotatedFwd.y;
                            layerQuad.pose.position.z = menuPose.position.z + rotatedFwd.z;

                            float quadWidth = 2.0f * 1.5f * tanf(GetMenuFov() * 3.14159f / 180.0f * 0.5f);
                            layerQuad.size = {quadWidth, quadWidth};
                            layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerQuad);
                        } else {
                            layerProj.space = m_localSpace;
                            layerProj.viewCount = viewCountOutput;
                            layerProj.views = projectionViews.data();
                            layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerProj);
                        }

                        XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
                        endInfo.displayTime = frameState.predictedDisplayTime;
                        endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
                        endInfo.layerCount = 1;
                        endInfo.layers = layers;
                        const XrResult endRes = xrEndFrame(m_session, &endInfo);
                        if (XR_SUCCEEDED(endRes)) {
                            // AER submit telemetry. Track ratio of raw vs synth
                            // submits per 300-frame window — that's the lever
                            // we're trying to bend (synth share = perceived per-
                            // eye rate boost above baseline 34 Hz).
                            static uint64_t s_aerSubmitsRaw = 0;
                            static uint64_t s_aerSubmitsSynth = 0;
                            static uint64_t s_aerFreshSubmits = 0;
                            static uint64_t s_aerResubmits = 0;
                            const bool wasFresh = submitSerial != m_lastSubmittedSerial;
                            if (submittedSynthetic) ++s_aerSubmitsSynth; else ++s_aerSubmitsRaw;
                            if (wasFresh) ++s_aerFreshSubmits; else ++s_aerResubmits;
                            const bool stride = (eyePairIds[0] == 1 || (eyePairIds[0] % 300) == 0);
                            if (stride || g_verboseLog != 0) {
                                Log("OpenXRManager: AER frame submitted. left=%llu right=%llu pair=(%llu,%llu) fresh=%d shouldRender=%d debugEye=%d synth=%d depth=%d (window: raw=%llu synth=%llu fresh=%llu resub=%llu)\n",
                                    static_cast<unsigned long long>(eyeSerials[0]),
                                    static_cast<unsigned long long>(eyeSerials[1]),
                                    static_cast<unsigned long long>(eyePairIds[0]),
                                    static_cast<unsigned long long>(eyePairIds[1]),
                                    wasFresh ? 1 : 0,
                                    frameState.shouldRender ? 1 : 0,
                                    GetAERDebugEye(),
                                    submittedSynthetic ? 1 : 0,
                                    useDepthLayer ? 1 : 0,
                                    static_cast<unsigned long long>(s_aerSubmitsRaw),
                                    static_cast<unsigned long long>(s_aerSubmitsSynth),
                                    static_cast<unsigned long long>(s_aerFreshSubmits),
                                    static_cast<unsigned long long>(s_aerResubmits));
                            }
                            if (stride) {
                                // Reset window counters on the 300-pair stride
                                // so we always see the LAST 300-pair behavior,
                                // not a cumulative average that hides recent
                                // regressions.
                                s_aerSubmitsRaw = 0;
                                s_aerSubmitsSynth = 0;
                                s_aerFreshSubmits = 0;
                                s_aerResubmits = 0;
                            }
                            m_lastSubmittedSerial = submitSerial;
                            m_lastSubmittedPairId = eyePairIds[0];
                            eyeSources[0]->Release();
                            eyeSources[1]->Release();
                            if (generatedSources[0]) generatedSources[0]->Release();
                            if (generatedSources[1]) generatedSources[1]->Release();
                            if (mvWarpedReplacement[0]) mvWarpedReplacement[0]->Release();
                            if (mvWarpedReplacement[1]) mvWarpedReplacement[1]->Release();
                            if (depthSource) depthSource->Release();
                            if (m_frameSyncEvent) {
                                SetEvent(m_frameSyncEvent);
                            }
                            continue;
                        }

                        Log("OpenXRManager: xrEndFrame AER submit failed (res=%d)\n", endRes);
                    }
                } else if (((++monoWaitLogCounter % 300) == 1)) {
                    Log("OpenXRManager: AER submit waiting. left=%llu right=%llu pair=(%llu,%llu) complete=%d leftView=%d rightView=%d views=%u shouldRender=%d\n",
                        static_cast<unsigned long long>(eyeSerials[0]),
                        static_cast<unsigned long long>(eyeSerials[1]),
                        static_cast<unsigned long long>(eyePairIds[0]),
                        static_cast<unsigned long long>(eyePairIds[1]),
                        completePair ? 1 : 0,
                        eyeHasView[0] ? 1 : 0,
                        eyeHasView[1] ? 1 : 0,
                        viewCountOutput,
                        frameState.shouldRender ? 1 : 0);
                }

                if (eyeSources[0]) {
                    eyeSources[0]->Release();
                }
                if (eyeSources[1]) {
                    eyeSources[1]->Release();
                }
                if (generatedSources[0]) {
                    generatedSources[0]->Release();
                }
                if (generatedSources[1]) {
                    generatedSources[1]->Release();
                }
                if (mvWarpedReplacement[0]) {
                    mvWarpedReplacement[0]->Release();
                }
                if (mvWarpedReplacement[1]) {
                    mvWarpedReplacement[1]->Release();
                }
                if (depthSource) {
                    depthSource->Release();
                }
            } else {
                ID3D12Resource* monoSource = nullptr;
                ID3D12Resource* monoDepthSource = nullptr;
                uint64_t presentSerial = 0;
                XrPosef monoPoses[2]{};
                XrFovf monoFovs[2]{};
                bool monoHasView[2] = {};
                bool monoHasDepth = false;
                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    if (m_monoCapturedFrame.texture &&
                        m_monoCapturedFrame.serial != 0 &&
                        m_monoCapturedFrame.hasView[0] &&
                        m_monoCapturedFrame.hasView[1]) {
                        monoSource = m_monoCapturedFrame.texture;
                        monoSource->AddRef();
                        presentSerial = m_monoCapturedFrame.serial;
                        for (int eye = 0; eye < 2; ++eye) {
                            monoPoses[eye] = m_monoCapturedFrame.poses[eye];
                            monoFovs[eye] = m_monoCapturedFrame.fovs[eye];
                            monoHasView[eye] = m_monoCapturedFrame.hasView[eye];
                        }
                        if (m_depthLayerSupported &&
                            m_depthSnapshot &&
                            m_depthSnapshotSerial == m_monoCapturedFrame.serial) {
                            monoDepthSource = m_depthSnapshot;
                            monoDepthSource->AddRef();
                            monoHasDepth = true;
                        }
                    }
                }

                std::unique_lock<std::mutex> monoSubmitLock(m_captureMutex, std::defer_lock);
                if (monoSource) {
                    monoSubmitLock.lock();
                }

                m_cmdAllocatorIndex = (m_cmdAllocatorIndex + 1) % 3;
                ID3D12CommandAllocator* currentAllocator = m_cmdAllocators[m_cmdAllocatorIndex];
                if (m_fenceValue >= 3 && m_fence->GetCompletedValue() < m_fenceValue - 2) {
                    m_fence->SetEventOnCompletion(m_fenceValue - 2, m_fenceEvent);
                    WaitForSingleObject(m_fenceEvent, INFINITE);
                }

                ID3D12GraphicsCommandList* m_cmdList = m_cmdLists[m_cmdAllocatorIndex];

                if (monoSource && monoHasView[0] && monoHasView[1] &&
                    SUCCEEDED(currentAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(currentAllocator, nullptr))) {
                    bool copyReady = true;
                    bool useDepthLayer = monoHasDepth && m_depthLayerSupported;
                    std::vector<bool> acquiredEyes(viewCountOutput, false);
                    std::vector<bool> acquiredDepthEyes(viewCountOutput, false);
                    std::vector<XrCompositionLayerProjectionView> projectionViews(viewCountOutput);
                    std::vector<XrCompositionLayerDepthInfoKHR> depthInfos;
                    for (uint32_t i = 0; i < viewCountOutput; ++i) {
                        projectionViews[i] = {XR_TYPE_COMPOSITION_LAYER_PROJECTION_VIEW};
                    }
                    if (useDepthLayer) {
                        depthInfos.resize(viewCountOutput);
                        for (uint32_t i = 0; i < viewCountOutput; ++i) {
                            depthInfos[i] = {XR_TYPE_COMPOSITION_LAYER_DEPTH_INFO_KHR};
                        }
                    }

                    for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                        uint32_t imageIndex = 0;
                        XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                        const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].handle, &acquireInfo, &imageIndex);
                        if (XR_FAILED(acquireRes)) {
                            Log("OpenXRManager: xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                            copyReady = false;
                            break;
                        }
                        acquiredEyes[eye] = true;

                        XrSwapchainImageWaitInfo waitSwapchainInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                        waitSwapchainInfo.timeout = XR_INFINITE_DURATION;
                        const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].handle, &waitSwapchainInfo);
                        if (XR_FAILED(waitRes)) {
                            Log("OpenXRManager: xrWaitSwapchainImage failed for eye %u (res=%d)\n", eye, waitRes);
                            copyReady = false;
                            break;
                        }

                        ID3D12Resource* texture = m_eyeSwapchains[eye].images[imageIndex].texture;
                        if (!texture) {
                            Log("OpenXRManager: XR swapchain texture missing for eye %u image %u\n", eye, imageIndex);
                            copyReady = false;
                            break;
                        }

                        // CAS sharpen (same as the AER path): when xr_sharpness>0, draw
                        // the sharpened mono source straight into the swapchain image.
                        // monoSource is always COMMON here (no synth scratch in mono).
                        const float monoSharp = GetVrSharpness();
                        bool doMonoSharpen = false;
                        // TEMP-DISABLED (see AER path): in-submit CAS GPU-crashes; gated
                        // behind CPVR_CAS_ENABLE until reworked with an SRV scratch.
                        if (std::getenv("CPVR_CAS_ENABLE") && monoSharp > 0.0001f && m_d3dDevice && texture && monoSource) {
                            if (!m_sharpenPass) m_sharpenPass = std::make_unique<SharpenPass>();
                            const D3D12_RESOURCE_DESC sd = texture->GetDesc();
                            m_sharpenReady = m_sharpenPass->EnsureInitialized(
                                m_d3dDevice, sd.Format,
                                static_cast<uint32_t>(sd.Width), sd.Height);
                            doMonoSharpen = m_sharpenReady;
                        }
                        if (doMonoSharpen) {
                            // monoSource rests in COPY_SOURCE (CaptureMonoPresentedFrame).
                            D3D12_RESOURCE_BARRIER pre[2] = {};
                            pre[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[0].Transition.pResource = monoSource;
                            pre[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            pre[0].Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            pre[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            pre[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            pre[1].Transition.pResource = texture;
                            pre[1].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            pre[1].Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            pre[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, pre);

                            m_sharpenPass->RecordSharpen(m_cmdList, monoSource, texture,
                                                         monoSharp, GetVrSharpmix());

                            D3D12_RESOURCE_BARRIER post[2] = {};
                            post[0].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[0].Transition.pResource = texture;
                            post[0].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                            post[0].Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            post[0].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            post[1].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            post[1].Transition.pResource = monoSource;
                            post[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                            post[1].Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
                            post[1].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(2, post);
                        } else {
                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = texture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            m_cmdList->CopyResource(texture, monoSource);

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = texture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);
                        }

                        projectionViews[eye].pose = monoPoses[eye];
                        projectionViews[eye].fov = monoFovs[eye];
                        if (menuRectActive) {
                            // Head-locked menus. The menu/
                            // world-map image is screen-space UI; submitting it with the
                            // captured (lagged, HMD-oriented) pose makes the compositor
                            // reproject it, so the map SWIMS/slides as you turn your head.
                            // Submit at the CURRENT eye pose instead -> no reprojection ->
                            // the menu stays fixed in front of the head (stable flat panel).
                            {
                                std::lock_guard<std::mutex> vl(m_viewMutex);
                                if (static_cast<size_t>(eye) < m_views.size()) {
                                    projectionViews[eye].pose = m_views[eye].pose;
                                }
                            }
                            const float menuFovDeg = GetMenuFov();
                            if (menuFovDeg > 1.0f && menuFovDeg < 170.0f) {
                                const float halfFov = (menuFovDeg * 3.1415926535f / 180.0f) * 0.5f;
                                projectionViews[eye].fov.angleLeft = -halfFov;
                                projectionViews[eye].fov.angleRight = halfFov;
                                projectionViews[eye].fov.angleDown = -halfFov;
                                projectionViews[eye].fov.angleUp = halfFov;
                            }
                        }
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;
                    }

                    if (copyReady && useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (m_eyeSwapchains[eye].depthHandle == XR_NULL_HANDLE) {
                                Log("OpenXRManager: [DEPTH] depthHandle missing for eye %u\n", eye);
                                useDepthLayer = false;
                                break;
                            }

                            uint32_t depthImageIndex = 0;
                            XrSwapchainImageAcquireInfo acquireInfo{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
                            const XrResult acquireRes = xrAcquireSwapchainImage(m_eyeSwapchains[eye].depthHandle, &acquireInfo, &depthImageIndex);
                            if (XR_FAILED(acquireRes)) {
                                Log("OpenXRManager: [DEPTH] xrAcquireSwapchainImage failed for eye %u (res=%d)\n", eye, acquireRes);
                                useDepthLayer = false;
                                break;
                            }
                            acquiredDepthEyes[eye] = true;

                            XrSwapchainImageWaitInfo waitInfo{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};
                            waitInfo.timeout = XR_INFINITE_DURATION;
                            const XrResult waitRes = xrWaitSwapchainImage(m_eyeSwapchains[eye].depthHandle, &waitInfo);
                            if (XR_FAILED(waitRes)) {
                                Log("OpenXRManager: [DEPTH] xrWaitSwapchainImage failed for eye %u (res=%d)\n", eye, waitRes);
                                useDepthLayer = false;
                                break;
                            }

                            ID3D12Resource* depthTexture = m_eyeSwapchains[eye].depthImages[depthImageIndex].texture;
                            if (!depthTexture) {
                                Log("OpenXRManager: [DEPTH] depth swapchain texture missing for eye %u image %u\n", eye, depthImageIndex);
                                useDepthLayer = false;
                                break;
                            }

                            D3D12_RESOURCE_BARRIER toCopyDest{};
                            toCopyDest.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCopyDest.Transition.pResource = depthTexture;
                            toCopyDest.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                            toCopyDest.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCopyDest.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCopyDest);

                            const D3D12_RESOURCE_DESC depthSrcDesc = monoDepthSource->GetDesc();
                            const D3D12_RESOURCE_DESC depthDstDesc = depthTexture->GetDesc();
                            if (depthSrcDesc.Format == depthDstDesc.Format) {
                                m_cmdList->CopyResource(depthTexture, monoDepthSource);
                            } else {
                                D3D12_TEXTURE_COPY_LOCATION dstLoc{};
                                dstLoc.pResource = depthTexture;
                                dstLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                dstLoc.SubresourceIndex = 0;
                                D3D12_TEXTURE_COPY_LOCATION srcLoc{};
                                srcLoc.pResource = monoDepthSource;
                                srcLoc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                                srcLoc.SubresourceIndex = 0;
                                m_cmdList->CopyTextureRegion(&dstLoc, 0, 0, 0, &srcLoc, nullptr);
                            }

                            D3D12_RESOURCE_BARRIER toCommon{};
                            toCommon.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            toCommon.Transition.pResource = depthTexture;
                            toCommon.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                            toCommon.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
                            toCommon.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            m_cmdList->ResourceBarrier(1, &toCommon);

                            depthInfos[eye].subImage.swapchain = m_eyeSwapchains[eye].depthHandle;
                            depthInfos[eye].subImage.imageRect.offset = {0, 0};
                            depthInfos[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                            depthInfos[eye].subImage.imageArrayIndex = 0;
                            // OpenXR requires minDepth < maxDepth in [0,1]. Reversed-Z is encoded
                            // by swapping nearZ/farZ (nearZ > farZ), NOT by swapping min/max depth.
                            depthInfos[eye].minDepth = 0.0f;
                            depthInfos[eye].maxDepth = 1.0f;
                            depthInfos[eye].nearZ = 10000.0f;
                            depthInfos[eye].farZ = 0.01f;
                            projectionViews[eye].next = &depthInfos[eye];
                        }
                    }

                    if (!useDepthLayer) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            projectionViews[eye].next = nullptr;
                        }
                    }

                    if (!copyReady) {
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: xrReleaseSwapchainImage cleanup failed for eye %u (res=%d)\n", eye, releaseRes);
                            }
                        }
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredDepthEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].depthHandle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: [DEPTH] xrReleaseSwapchainImage cleanup failed for eye %u (res=%d)\n", eye, releaseRes);
                            }
                        }

                        m_cmdList->Close();
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                        
                        ++m_fenceValue;
                        m_d3dQueue->Signal(m_fence, m_fenceValue);
                    } else {
                        m_cmdList->Close();
                        ID3D12CommandList* cmdLists[] = {m_cmdList};
                        m_d3dQueue->ExecuteCommandLists(1, cmdLists);
                        
                        ++m_fenceValue;
                        m_d3dQueue->Signal(m_fence, m_fenceValue);

                        bool releaseOk = true;
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].handle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                                releaseOk = false;
                            }
                        }
                        for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                            if (!acquiredDepthEyes[eye]) {
                                continue;
                            }
                            XrSwapchainImageReleaseInfo releaseInfo{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                            const XrResult releaseRes = xrReleaseSwapchainImage(m_eyeSwapchains[eye].depthHandle, &releaseInfo);
                            if (XR_FAILED(releaseRes)) {
                                Log("OpenXRManager: [DEPTH] xrReleaseSwapchainImage failed for eye %u (res=%d)\n", eye, releaseRes);
                                useDepthLayer = false;
                            }
                        }

                        if (!useDepthLayer) {
                            for (uint32_t eye = 0; eye < viewCountOutput; ++eye) {
                                projectionViews[eye].next = nullptr;
                            }
                        }

                        if (releaseOk) {
                            XrCompositionLayerProjection layerProj{XR_TYPE_COMPOSITION_LAYER_PROJECTION};
                            XrCompositionLayerQuad layerQuad{XR_TYPE_COMPOSITION_LAYER_QUAD};
                            const XrCompositionLayerBaseHeader* layers[1] = {nullptr};

                            if (menuRectActive) {
                                layerQuad.space = m_localSpace;
                                layerQuad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
                                layerQuad.subImage = projectionViews[0].subImage;

                            XrPosef menuPose{};
                            menuPose.orientation.w = 1.0f;
                            if (headPoseLocated) {
                                menuPose = location.pose;
                            } else if (m_basePoseSet) {
                                menuPose = m_basePose;
                            }
                            XrQuaternionf baseOri = menuPose.orientation;
                             
                            // Flatten the orientation to pure yaw to keep the menu perfectly vertical
                            float fx = -2.0f * (baseOri.x * baseOri.z + baseOri.y * baseOri.w);
                            float fz = 2.0f * (baseOri.x * baseOri.x + baseOri.y * baseOri.y) - 1.0f;
                            float flatYaw = atan2f(-fx, -fz);
                            XrQuaternionf qYaw = {0.0f, sinf(flatYaw * 0.5f), 0.0f, cosf(flatYaw * 0.5f)};

                            XrQuaternionf menuOri = qYaw;

                            XrVector3f fwd = {0.0f, 0.0f, -1.5f};
                            XrVector3f rotatedFwd = RotateVector(qYaw, fwd);

                            layerQuad.pose.orientation = menuOri;
                            layerQuad.pose.position.x = menuPose.position.x + rotatedFwd.x;
                            layerQuad.pose.position.y = menuPose.position.y + rotatedFwd.y;
                            layerQuad.pose.position.z = menuPose.position.z + rotatedFwd.z;

                                float quadWidth = 2.0f * 1.5f * tanf(GetMenuFov() * 3.14159f / 180.0f * 0.5f);
                                layerQuad.size = {quadWidth, quadWidth};
                                layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerQuad);
                            } else {
                                layerProj.space = m_localSpace;
                                layerProj.viewCount = viewCountOutput;
                                layerProj.views = projectionViews.data();
                                layers[0] = reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layerProj);
                            }

                            XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
                            endInfo.displayTime = frameState.predictedDisplayTime;
                            endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
                            endInfo.layerCount = 1;
                            endInfo.layers = layers;
                            const XrResult endRes = xrEndFrame(m_session, &endInfo);
                            if (XR_SUCCEEDED(endRes)) {
                                if ((presentSerial % 300) == 1) {
                                    Log("OpenXRManager: Mono frame submitted. serial=%llu fresh=%d views=%u shouldRender=%d depth=%d\n",
                                        static_cast<unsigned long long>(presentSerial),
                                        presentSerial != m_lastSubmittedSerial ? 1 : 0,
                                        viewCountOutput,
                                        frameState.shouldRender ? 1 : 0,
                                        useDepthLayer ? 1 : 0);
                                }
                                m_lastSubmittedSerial = presentSerial;
                                monoSource->Release();
                                if (monoDepthSource) monoDepthSource->Release();
                                if (m_frameSyncEvent) {
                                    SetEvent(m_frameSyncEvent);
                                }
                                continue;
                            }

                            Log("OpenXRManager: xrEndFrame mono submit failed (res=%d)\n", endRes);
                        }
                    }
                }

                if (monoSource) {
                    monoSource->Release();
                }
                if (monoDepthSource) {
                    monoDepthSource->Release();
                }
            }
        } else if (monoEnabled && ((++monoWaitLogCounter % 300) == 1)) {
            Log("OpenXRManager: %s submit waiting. ready=%d views=%zu shouldRender=%d\n",
                useAerSubmit ? "AER" : "Mono",
                monoReady ? 1 : 0,
                m_views.size(),
                frameState.shouldRender ? 1 : 0);
        }

        XrFrameEndInfo endInfo{XR_TYPE_FRAME_END_INFO};
        endInfo.displayTime = frameState.predictedDisplayTime;
        endInfo.environmentBlendMode = XR_ENVIRONMENT_BLEND_MODE_OPAQUE;
        endInfo.layerCount = 0;
        endInfo.layers = nullptr;
        xrEndFrame(m_session, &endInfo);
        
        if (m_frameSyncEvent) {
            SetEvent(m_frameSyncEvent);
        }
    }

    Log("OpenXRManager: Frame thread stopped.\n");
    return 0;
}

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
        s_hMapFile2 = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 512, "CyberpunkVR_Hands_Shared");
        if (s_hMapFile2) sShared = (float*)MapViewOfFile(s_hMapFile2, FILE_MAP_ALL_ACCESS, 0, 0, 512);
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

    // ===== SEQLOCK END =====
    // All payload slots are written; publish an EVEN sequence (= complete) so readers
    // that snapshot this value (and find it unchanged + even across their read) accept
    // the frame. Release fence first so the payload stores are visible before seq.
    std::atomic_thread_fence(std::memory_order_release);
    *seqSlot = seqStart + 1u;   // seqStart was forced odd -> +1 = EVEN = complete
}

void OpenXRManager::OnPresent(IDXGISwapChain* swapChain) {
    // [HANDS] Shared Memory Output
    static HANDLE s_hMapFile = NULL;
    static float* s_pSharedHands = nullptr;
    if (!s_hMapFile) {
        s_hMapFile = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 512, "CyberpunkVR_Hands_Shared");
        if (s_hMapFile) {
            s_pSharedHands = (float*)MapViewOfFile(s_hMapFile, FILE_MAP_ALL_ACCESS, 0, 0, 512);
            m_sharedHandsPtr = s_pSharedHands;   // expose to GetSharedSlot (overlay barrel crosshair)
        }
    }
    if (s_pSharedHands) {
        std::lock_guard<std::mutex> lock(const_cast<std::mutex&>(m_handMutex));
        // Slot [32]: VR hand-tracking request for the RED4ext plugin (set from the overlay menu).
        s_pSharedHands[32] = static_cast<float>(m_vrHandTrackingMode.load(std::memory_order_relaxed));
        s_pSharedHands[58] = static_cast<float>(m_weaponAimEnable.load(std::memory_order_relaxed)); // weapon-aim enable
        // shared[23]: 0/unset = immersive holsters (default), 1 = simple slot mapping. Inverted so the
        // zero-initialized shared block defaults to the immersive (current) behaviour before the first
        // publish. The CET Holster mod reads this via GetVRSharedSlot(23).
        s_pSharedHands[23] = (m_immersiveHolsters.load(std::memory_order_relaxed) != 0) ? 0.0f : 1.0f;
        s_pSharedHands[59] = 5.0f;  // mode 5 = game muzzle xform (the working solution)
        // [70..75]: anatomical HMD/body->shoulder offsets (auto-calibration result).
        // Right (rx,ry,rz), then left (lx,ly,lz). [76] = valid flag. Kept outside
        // [34..47], which is the regular calibration block.
        if (m_calibExtValid.load(std::memory_order_relaxed)) {
            for (int i = 0; i < 6; ++i) s_pSharedHands[70 + i] = m_calibExt[i].load(std::memory_order_relaxed);
            s_pSharedHands[76] = 1.0f;
        }
        // [77..80]: T-pose measured anatomy (real arm length R/L, HMD eye height) + valid flag.
        // The plugin scales the avatar arm bones to match (gizmo-path), straightening a relaxed arm.
        if (m_measureValid.load(std::memory_order_relaxed)) {
            s_pSharedHands[77] = m_userArmLenR.load(std::memory_order_relaxed);
            s_pSharedHands[78] = m_userArmLenL.load(std::memory_order_relaxed);
            s_pSharedHands[79] = m_userEyeHeight.load(std::memory_order_relaxed);
            s_pSharedHands[80] = 1.0f;
        }
        // [89]: HMD PHYSICAL height relative to the recenter base (~0 standing, negative when the
        // user physically squats). The game FPP camera Lua samples is a FIXED eye height, so the
        // plugin needs this to actually lower the body / bend the knees on a real-life squat.
        // [85..88] are written by the plugin (camera->head offset) -- do not touch them here.
        // PAIR-LOCKED: use the frozen physical head height (snapshot at the pair
        // boundary). [89] head height + [90] neck-pivot are now written from the
        // frozen snapshot inside FlushHandsToShared (published at the pair boundary,
        // BEFORE the next pair's animation) together with the hand slots [0..19], so
        // they are no longer sampled live per present here.
        // [91..93]: the ACTIVE baked camera->head offset (game-local right/fwd/up). dxgi shifts the
        // VIEW by this in LocateCamera; the plugin adds the SAME offset to camModelPos so the avatar
        // head sits exactly where the (offset-tuned) view sits -> head = camera, body follows.
        {
            float cb[3]; GetCameraOffset(cb);
            s_pSharedHands[91] = cb[0]; s_pSharedHands[92] = cb[1]; s_pSharedHands[93] = cb[2];
        }
        // IMPORTANT: hand pose slots [0..19] are flushed in OnLocateCameraCallback
        // BEFORE render (FlushHandsToShared). Do NOT rewrite them here after render,
        // or the next frame may see a mixed temporal state (one wrong frame even
        // on the flat monitor, amplified in AER). Keep OnPresent for config/static
        // slots only.

        // [33..47] IK calibration from the overlay; [48] one-shot diag request.
        s_pSharedHands[33] = static_cast<float>(m_calibValid.load(std::memory_order_relaxed));
        for (int i = 0; i < 14; ++i) s_pSharedHands[34 + i] = m_calib[i].load(std::memory_order_relaxed);
        s_pSharedHands[48] = static_cast<float>(m_logDiagReq.load(std::memory_order_relaxed));
    }

    if (!swapChain) return;

    uint64_t s_presentCount = m_presentCount.fetch_add(1, std::memory_order_relaxed) + 1;
    const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
    const bool aerEnabled = monoEnabled && IsAERSubmitEnabled();
    const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
    const bool syncSequential = aerEnabled && GetSyncSequential() != 0;
    const int scheduledEye = aerEnabled ? m_renderEyeIndex.load(std::memory_order_relaxed) : 0;
    const int presentEye = aerEnabled ? GetRenderedCameraEye() : 0;
    const bool aerWarmupFrame = aerEnabled && m_aerWarmupRemaining > 0;

    // ===== POSE PAIR LOCKING — publish point (pipeline shift) =====
    // Snapshot the tracking state + write the VRIK shared slots HERE, BEFORE the
    // next animation/render pass (the plugin reads it during anim eval, which
    // precedes render/LocateCamera).
    //
    // Publish EVERY present in both mono AND AER. An earlier AER-only gate
    // (presentEye==1, the pair boundary) published hands only every OTHER
    // present -> VRIK/skeleton updated at HALF the present rate -> hands
    // "teleport" at ~20-45 Hz while the world rendered at 90. Publishing every
    // present gives each eye's render the freshest hand pose (the live-pose path,
    // usePairLock=0 default, already avoids the frozen-pair coherence issue, and
    // any per-eye skeleton tear from a mid-pair update is far less visible than
    // half-rate hands).
    {
        if (aerEnabled) m_pairLockLastEye = presentEye;
        UpdatePairLock();
        FlushHandsToShared();
    }

    // NOTE: NO present-thread SLEEP throttle here. There is no busy-wait/Sleep
    // cap on the game present thread. HMD-pacing for AER V2 is done by the
    // m_frameSyncEvent wait at the end of OnPresent. The old Sleep-to-45-pairs/s
    // "GPU boost" was removed because it stalled the engine to ~7 pairs/s; the
    // current path runs at full HMD rate, one eye per frame. To cap the engine
    // further, use an
    // EXTERNAL fps limiter (in-game / RTSS / driver).

    if (g_verboseLog && aerEnabled && scheduledEye != presentEye && (s_presentCount % 300) == 1) {
        Log("OpenXRManager: AER eye mismatch corrected. scheduled=%d rendered=%d serial=%llu\n",
            scheduledEye,
            presentEye,
            static_cast<unsigned long long>(s_presentCount));
    }

    auto latchSyncedSequentialPair = [this]() {
        m_syncedPoseValid.store(false, std::memory_order_relaxed);
        m_syncedPosX.store(m_posX.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedPosY.store(m_posY.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedPosZ.store(m_posZ.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriX.store(m_oriX.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriY.store(m_oriY.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriZ.store(m_oriZ.load(std::memory_order_relaxed), std::memory_order_relaxed);
        m_syncedOriW.store(m_oriW.load(std::memory_order_relaxed), std::memory_order_relaxed);

        bool viewsValid = false;
        {
            std::lock_guard<std::mutex> viewLock(m_viewMutex);
            if (m_views.size() >= 2) {
                for (int eye = 0; eye < 2; ++eye) {
                    m_syncedEyePoses[eye] = m_views[eye].pose;
                    m_syncedEyeFovs[eye] = m_views[eye].fov;
                }
                viewsValid = true;
            }
        }

        m_syncedEyeViewsValid = viewsValid;
        m_syncedPoseValid.store(true, std::memory_order_relaxed);
        ++m_syncedPairId;
        if (g_verboseLog && (m_syncedPairId == 1 || (m_syncedPairId % 300) == 0)) {
            Log("OpenXRManager: synchronized sequential pair latched. pair=%llu views=%d 3dof=%d\n",
                static_cast<unsigned long long>(m_syncedPairId),
                viewsValid ? 1 : 0,
                Get3DofMovement() != 0 ? 1 : 0);
        }
    };

    if (syncSequential && !m_syncedPoseValid.load(std::memory_order_relaxed)) {
        latchSyncedSequentialPair();
    }
    // Pair id MUST be coupled to the eye-capture cadence, not the global present
    // counter parity. m_renderEyeIndex is reset to 0 on AER enable, but the
    // global present counter parity is arbitrary at that moment; deriving the
    // pair id from (s_presentCount+1)/2 split eye0/eye1 of the first pair across
    // a pair boundary ~50% of toggles, so the pair never completed and only
    // empty frames were submitted (frozen/blank HMD). Bump a dedicated counter
    // on each eye-0 capture so left and right always share the id.
    uint64_t presentPairId = 0;
    if (aerEnabled) {
        if (syncSequential && m_syncedPairId != 0) {
            presentPairId = m_syncedPairId;
        } else {
            if (!aerWarmupFrame && presentEye == 0) {
                ++m_aerPairCounter;
            }
            presentPairId = m_aerPairCounter;
        }
    }

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        Log("OpenXRManager: Present hook could not read swapchain desc.\n");
        return;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    UINT backBufferIndex = 0;
    if (SUCCEEDED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3)))) {
        backBufferIndex = swapChain3->GetCurrentBackBufferIndex();
        swapChain3->Release();
    }

    ID3D12Resource* backBuffer = nullptr;
    D3D12_RESOURCE_DESC resourceDesc{};
    if (SUCCEEDED(swapChain->GetBuffer(backBufferIndex, IID_PPV_ARGS(&backBuffer)))) {
        resourceDesc = backBuffer->GetDesc();
    }

    XrPosef capturedPose{};
    capturedPose.orientation.w = 1.0f;
    XrFovf capturedFov{};
    bool hasCapturedView = false;
    XrPosef monoCapturedPoses[2]{};
    XrFovf monoCapturedFovs[2]{};
    bool monoCapturedViews[2] = {};
    if (aerEnabled && !menuRectActive) {
        std::lock_guard<std::mutex> viewLock(m_viewMutex);
        const bool useSyncedView = syncSequential && m_syncedEyeViewsValid && presentEye >= 0 && presentEye < 2;
        const bool useCurrentView = presentEye >= 0 && presentEye < static_cast<int>(m_views.size());
        if (useSyncedView || useCurrentView) {
            capturedPose = useSyncedView ? m_syncedEyePoses[presentEye] : m_views[presentEye].pose;
            const XrFovf sourceFov = useSyncedView ? m_syncedEyeFovs[presentEye] : m_views[presentEye].fov;
            float fovWidth = static_cast<float>(desc.BufferDesc.Width);
            float fovHeight = static_cast<float>(desc.BufferDesc.Height);
            if ((fovWidth <= 1.0f || fovHeight <= 1.0f) && resourceDesc.Width != 0 && resourceDesc.Height != 0) {
                fovWidth = static_cast<float>(resourceDesc.Width);
                fovHeight = static_cast<float>(resourceDesc.Height);
            }
            XrFovf pairFovs[2]{};
            const XrFovf* pairFovPtr = nullptr;
            if (m_views.size() >= 2) {
                pairFovs[0] = useSyncedView ? m_syncedEyeFovs[0] : m_views[0].fov;
                pairFovs[1] = useSyncedView ? m_syncedEyeFovs[1] : m_views[1].fov;
                pairFovPtr = pairFovs;
            }
            capturedFov = ApplyForcedProjectionFov(sourceFov, pairFovPtr, presentEye, fovWidth, fovHeight);
            hasCapturedView = true;

            // Render-pose submit (AER V2): replace the present-time pose with the
            // exact head pose this eye's frame was rendered with (captured by the
            // camera hook), so the compositor time-warps the older 1/2-rate eye
            // forward to display time instead of showing it stale. Keep the runtime
            // per-eye offset for the correct stereo baseline; only head pos+ori
            // carry the per-eye render timestamp.
            if (GetRenderPoseSubmit() != 0 && m_views.size() >= 2) {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                
                uint32_t renderedSeq = GetRenderedCameraSeq();
                int idx = renderedSeq % 256;
                
                XrPosef rp{};
                bool validRp = false;
                if (renderedSeq > 0 && m_poseQueueFrame[idx] == renderedSeq) {
                    rp = m_poseQueue[idx];
                    validRp = true;
                } else if (m_renderEyeHeadPoseValid[presentEye]) {
                    rp = m_renderEyeHeadPose[presentEye];
                    validRp = true;
                }

                if (validRp) {
                    const XrVector3f headCenter{
                        (m_views[0].pose.position.x + m_views[1].pose.position.x) * 0.5f,
                        (m_views[0].pose.position.y + m_views[1].pose.position.y) * 0.5f,
                        (m_views[0].pose.position.z + m_views[1].pose.position.z) * 0.5f};
                    const XrVector3f eyeOffset{
                        m_views[presentEye].pose.position.x - headCenter.x,
                        m_views[presentEye].pose.position.y - headCenter.y,
                        m_views[presentEye].pose.position.z - headCenter.z};
                    capturedPose.orientation = rp.orientation;
                    capturedPose.position = {
                        rp.position.x + eyeOffset.x,
                        rp.position.y + eyeOffset.y,
                        rp.position.z + eyeOffset.z};
                }
            }
        }
    } else if (monoEnabled) {
        std::lock_guard<std::mutex> viewLock(m_viewMutex);
        if (m_views.size() >= 2) {
            float fovWidth = static_cast<float>(desc.BufferDesc.Width);
            float fovHeight = static_cast<float>(desc.BufferDesc.Height);
            if ((fovWidth <= 1.0f || fovHeight <= 1.0f) && resourceDesc.Width != 0 && resourceDesc.Height != 0) {
                fovWidth = static_cast<float>(resourceDesc.Width);
                fovHeight = static_cast<float>(resourceDesc.Height);
            }

            bool hasRenderHeadPose = false;
            XrPosef renderHeadPose{};
            renderHeadPose.orientation.w = 1.0f;
            {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                uint32_t renderedSeq = GetRenderedCameraSeq();
                int idx = renderedSeq % 256;
                
                if (renderedSeq > 0 && m_poseQueueFrame[idx] == renderedSeq) {
                    renderHeadPose = m_poseQueue[idx];
                    hasRenderHeadPose = true;
                } else if (m_renderEyeHeadPoseValid[0]) {
                    renderHeadPose = m_renderEyeHeadPose[0];
                    hasRenderHeadPose = true;
                }
            }

            const XrVector3f headCenter{
                (m_views[0].pose.position.x + m_views[1].pose.position.x) * 0.5f,
                (m_views[0].pose.position.y + m_views[1].pose.position.y) * 0.5f,
                (m_views[0].pose.position.z + m_views[1].pose.position.z) * 0.5f};
            XrPosef monoCenterPose{};
            monoCenterPose.orientation = m_views[0].pose.orientation;
            monoCenterPose.position = headCenter;
            if (GetRenderPoseSubmit() != 0 && hasRenderHeadPose) {
                monoCenterPose = renderHeadPose;
            }
            for (int eye = 0; eye < 2; ++eye) {
                XrVector3f eyeOffset{
                    m_views[eye].pose.position.x - headCenter.x,
                    m_views[eye].pose.position.y - headCenter.y,
                    m_views[eye].pose.position.z - headCenter.z};
                monoCapturedPoses[eye] = monoCenterPose;
                monoCapturedPoses[eye].position.x += eyeOffset.x;
                monoCapturedPoses[eye].position.y += eyeOffset.y;
                monoCapturedPoses[eye].position.z += eyeOffset.z;
                XrFovf monoPairFovs[2] = { m_views[0].fov, m_views[1].fov };
                monoCapturedFovs[eye] = ApplyForcedProjectionFov(m_views[eye].fov, monoPairFovs, eye, fovWidth, fovHeight);
                monoCapturedViews[eye] = true;
            }
        }
    }
    bool monoCaptureOk = false;
    // In AER mode the mono capture is REDUNDANT during gameplay: its output is
    // never submitted (the AER submit path uses m_capturedEyeFrames, not
    // m_monoCapturedFrame), but it still runs a full CopyResource +
    // ExecuteCommandLists + Signal + its own ring-buffer drain wait
    // (WaitForSingleObject INFINITE) EVERY present -- doubling the per-present
    // GPU work and tripping the capture-queue drain wait, which throttles the
    // game's present rate. CP2077 ties simulation ticks to present frames, so a
    // depressed present rate makes sim-driven things (NPC skeletal anim, world
    // logic) stutter while shader-time things (windmills/fans) stay smooth.
    //
    // BUT: in the MENU the AER capture is deliberately skipped (the
    // `!menuRectActive` guard on CapturePresentedFrame below) because menus
    // render as a single mono surface, not alternate-eye. So in the menu we MUST
    // run the mono capture or there is no image at all. Run mono capture when
    // either we're not in AER, or we're in AER but currently in a menu.
    if (monoEnabled && backBuffer && (!aerEnabled || menuRectActive)) {
        monoCaptureOk = CaptureMonoPresentedFrame(backBuffer, resourceDesc, s_presentCount,
            monoCapturedPoses, monoCapturedFovs, monoCapturedViews);
        if (!monoCaptureOk && (s_presentCount % 300) == 1) {
            Log("OpenXRManager: Mono capture failed. serial=%llu views=(%d,%d)\n",
                static_cast<unsigned long long>(s_presentCount),
                monoCapturedViews[0] ? 1 : 0,
                monoCapturedViews[1] ? 1 : 0);
        }
    }

    bool aerCaptureOk = false;
    if (aerEnabled && !menuRectActive && backBuffer && !aerWarmupFrame) {
        aerCaptureOk = CapturePresentedFrame(backBuffer, resourceDesc, presentEye, s_presentCount, presentPairId);
        if (!aerCaptureOk) {
            Log("OpenXRManager: AER capture failed for eye %d serial=%llu\n", presentEye, static_cast<unsigned long long>(s_presentCount));
        }
    } else if (aerWarmupFrame && (presentPairId == 1 || (presentPairId % 300) == 0)) {
        Log("OpenXRManager: AER warmup discarded. eye=%d serial=%llu pair=%llu remaining=%d\n",
            presentEye,
            static_cast<unsigned long long>(s_presentCount),
            static_cast<unsigned long long>(presentPairId),
            m_aerWarmupRemaining);
    }

    bool runAERV2Flow = false;
    uint64_t flowPairId = 0;
    ID3D12Resource* flowPrevSource[2] = {};
    ID3D12Resource* flowCurrSource[2] = {};
    ID3D12Resource* flowPrev[2] = {};
    ID3D12Resource* flowCurr[2] = {};
    ID3D12Resource* flowPrevDepth[2] = {};
    ID3D12Resource* flowCurrDepth[2] = {};
    std::unique_lock<std::mutex> presentLock(m_presentMutex);
        if (m_lastPresentedBackBuffer) {
            m_lastPresentedBackBuffer->Release();
            m_lastPresentedBackBuffer = nullptr;
        }

        m_lastPresentedWidth = resourceDesc.Width != 0 ? static_cast<uint32_t>(resourceDesc.Width) : desc.BufferDesc.Width;
        m_lastPresentedHeight = resourceDesc.Height != 0 ? resourceDesc.Height : desc.BufferDesc.Height;
        m_lastPresentedFormat = resourceDesc.Format != DXGI_FORMAT_UNKNOWN ? static_cast<uint32_t>(resourceDesc.Format) : static_cast<uint32_t>(desc.BufferDesc.Format);
        m_lastPresentedBufferIndex = backBufferIndex;
        m_lastPresentSerial = s_presentCount;
        if (aerEnabled && aerCaptureOk && presentEye >= 0 && presentEye < 2) {
            m_pendingEyeFrames[presentEye].pose = capturedPose;
            m_pendingEyeFrames[presentEye].fov = capturedFov;
            m_pendingEyeFrames[presentEye].hasView = hasCapturedView;

            const bool aerV2On = (GetAERV2Enabled() != 0);
            const bool pairReady =
                m_pendingEyeFrames[0].pairId == presentPairId &&
                m_pendingEyeFrames[1].pairId == presentPairId &&
                m_pendingEyeFrames[0].serial != 0 &&
                m_pendingEyeFrames[1].serial != 0 &&
                m_pendingEyeFrames[0].hasView &&
                m_pendingEyeFrames[1].hasView;
            if (aerV2On || pairReady) {
                if (aerV2On) {
                    std::swap(m_previousCapturedEyeFrames[presentEye], m_capturedEyeFrames[presentEye]);
                    std::swap(m_capturedEyeFrames[presentEye], m_pendingEyeFrames[presentEye]);
                } else {
                std::swap(m_previousCapturedEyeFrames[0], m_capturedEyeFrames[0]);
                std::swap(m_previousCapturedEyeFrames[1], m_capturedEyeFrames[1]);
                std::swap(m_capturedEyeFrames[0], m_pendingEyeFrames[0]);
                std::swap(m_capturedEyeFrames[1], m_pendingEyeFrames[1]);
                }
                // Per-frame resource renaming (nameFrameRole) removed: it formatted
                // 6 wide strings + called SetName every present UNDER m_presentMutex,
                // contending with the frame thread's submit. The resources are already
                // named at creation (L"AERV2_pending_eye%d_color/depth"), so the
                // per-frame pair/serial suffix is redundant debug verbosity. Re-enable
                // here only when chasing a capture-slot race in PIX/VS Graphics Diag.
                if (GetAERV2Enabled() == 0 &&
                    m_interpolatedPairId != 0 &&
                    m_interpolatedPairId != presentPairId) {
                    if ((presentPairId % 300) == 0) {
                        Log("OpenXRManager: AER V2 stale synth dropped. stale=%llu current=%llu lastSubmitted=%llu\n",
                            static_cast<unsigned long long>(m_interpolatedPairId),
                            static_cast<unsigned long long>(presentPairId),
                            static_cast<unsigned long long>(m_lastSubmittedPairId));
                    }
                    m_interpolatedPairId = 0;
                    m_interpolatedSynthSlot = 0;
                    m_interpolatedSyntheticEye = -1;
                    m_interpolatedEyeViewsValid[0] = false;
                    m_interpolatedEyeViewsValid[1] = false;
                }
                const bool synthSlotBusy = (GetAERV2Enabled() == 0) && (m_interpolatedPairId != 0);
                if (GetAERV2Enabled() != 0 && presentPairId == 1) {
                    Log("OpenXRManager: AER V2 optical-flow warmup active until pair=%llu\n",
                        static_cast<unsigned long long>(kAERV2FlowWarmupPairId));
                }
                // The NvOF synth now runs in FrameThreadMain
                // (continuous blendFactor matched to predictedDisplayTime), NOT
                // inline here. Disable the old present-thread V2 producer entirely;
                // OnPresent for V2 just captures+swaps. (V1/legacy worker path
                // below is unchanged.)
                if (false && aerV2On &&
                    m_lastSubmittedPairId != 0 &&
                    presentPairId >= kAERV2FlowWarmupPairId &&
                    !synthSlotBusy) {
                    const int renderedEye = presentEye;
                    const int syntheticEye = presentEye ^ 1;
                    const bool pairAlreadyPublished =
                        m_interpolatedPairId == presentPairId &&
                        m_interpolatedSyntheticEye >= 0;
                    if (!pairAlreadyPublished) {
                        // NvOF flow is TEMPORAL between same-eye
                        // prev/curr captures (not cross-eye). The synthetic eye's
                        // previous render (2 presents ago) is warped forward by
                        // blendFactor using same-eye temporal flow.
                        if (m_previousCapturedEyeFrames[syntheticEye].serial != 0 &&
                            m_previousCapturedEyeFrames[syntheticEye].texture &&
                            m_previousCapturedEyeFrames[syntheticEye].opticalFlowTexture &&
                            m_capturedEyeFrames[syntheticEye].serial != 0 &&
                            m_capturedEyeFrames[syntheticEye].texture &&
                            m_capturedEyeFrames[syntheticEye].opticalFlowTexture &&
                            m_capturedEyeFrames[syntheticEye].hasView) {
                            runAERV2Flow = true;
                            flowPairId = s_presentCount;
                            // Same-eye temporal: prev=older, curr=newer of synthetic eye
                            flowPrevSource[syntheticEye] = m_previousCapturedEyeFrames[syntheticEye].texture;
                            flowCurrSource[syntheticEye] = m_capturedEyeFrames[syntheticEye].texture;
                            flowPrev[syntheticEye] = m_previousCapturedEyeFrames[syntheticEye].opticalFlowTexture;
                            flowCurr[syntheticEye] = m_capturedEyeFrames[syntheticEye].opticalFlowTexture;
                            if (m_previousCapturedEyeFrames[syntheticEye].depthTexture &&
                                m_previousCapturedEyeFrames[syntheticEye].depthSerial == m_previousCapturedEyeFrames[syntheticEye].serial) {
                                flowPrevDepth[syntheticEye] = m_previousCapturedEyeFrames[syntheticEye].depthTexture;
                            }
                            if (m_capturedEyeFrames[syntheticEye].depthTexture &&
                                m_capturedEyeFrames[syntheticEye].depthSerial == m_capturedEyeFrames[syntheticEye].serial) {
                                flowCurrDepth[syntheticEye] = m_capturedEyeFrames[syntheticEye].depthTexture;
                            }
                            flowPrevSource[syntheticEye]->AddRef();
                            flowCurrSource[syntheticEye]->AddRef();
                            flowPrev[syntheticEye]->AddRef();
                            flowCurr[syntheticEye]->AddRef();
                            if (flowPrevDepth[syntheticEye]) flowPrevDepth[syntheticEye]->AddRef();
                            if (flowCurrDepth[syntheticEye]) flowCurrDepth[syntheticEye]->AddRef();
                        }
                        // 1:1 half-rate: also stage renderedEye's own temporal
                        // history so ProcessAERV2Job can warp BOTH eyes into the
                        // in-between pair (m_aerV2InBetween). Only when half-rate.
                        if (runAERV2Flow && GetAERHalfRate() != 0 && !flowPrevSource[renderedEye] &&
                            m_previousCapturedEyeFrames[renderedEye].serial != 0 &&
                            m_previousCapturedEyeFrames[renderedEye].texture &&
                            m_previousCapturedEyeFrames[renderedEye].opticalFlowTexture &&
                            m_capturedEyeFrames[renderedEye].serial != 0 &&
                            m_capturedEyeFrames[renderedEye].texture &&
                            m_capturedEyeFrames[renderedEye].opticalFlowTexture &&
                            m_capturedEyeFrames[renderedEye].hasView) {
                            flowPrevSource[renderedEye] = m_previousCapturedEyeFrames[renderedEye].texture;
                            flowCurrSource[renderedEye] = m_capturedEyeFrames[renderedEye].texture;
                            flowPrev[renderedEye] = m_previousCapturedEyeFrames[renderedEye].opticalFlowTexture;
                            flowCurr[renderedEye] = m_capturedEyeFrames[renderedEye].opticalFlowTexture;
                            if (m_previousCapturedEyeFrames[renderedEye].depthTexture &&
                                m_previousCapturedEyeFrames[renderedEye].depthSerial == m_previousCapturedEyeFrames[renderedEye].serial) {
                                flowPrevDepth[renderedEye] = m_previousCapturedEyeFrames[renderedEye].depthTexture;
                            }
                            if (m_capturedEyeFrames[renderedEye].depthTexture &&
                                m_capturedEyeFrames[renderedEye].depthSerial == m_capturedEyeFrames[renderedEye].serial) {
                                flowCurrDepth[renderedEye] = m_capturedEyeFrames[renderedEye].depthTexture;
                            }
                            flowPrevSource[renderedEye]->AddRef();
                            flowCurrSource[renderedEye]->AddRef();
                            flowPrev[renderedEye]->AddRef();
                            flowCurr[renderedEye]->AddRef();
                            if (flowPrevDepth[renderedEye]) flowPrevDepth[renderedEye]->AddRef();
                            if (flowCurrDepth[renderedEye]) flowCurrDepth[renderedEye]->AddRef();
                        }
                    }
                } else if (!aerV2On &&
                    m_lastSubmittedPairId != 0 &&
                    presentPairId >= kAERV2FlowWarmupPairId &&
                    !synthSlotBusy &&
                    m_previousCapturedEyeFrames[0].serial != 0 &&
                    m_previousCapturedEyeFrames[1].serial != 0 &&
                    m_previousCapturedEyeFrames[0].texture &&
                    m_previousCapturedEyeFrames[1].texture &&
                    m_previousCapturedEyeFrames[0].opticalFlowTexture &&
                    m_previousCapturedEyeFrames[1].opticalFlowTexture &&
                    m_capturedEyeFrames[0].texture &&
                    m_capturedEyeFrames[1].texture &&
                    m_capturedEyeFrames[0].opticalFlowTexture &&
                    m_capturedEyeFrames[1].opticalFlowTexture) {
                    runAERV2Flow = true;
                    flowPairId = presentPairId;
                    flowPrevSource[0] = m_previousCapturedEyeFrames[0].texture;
                    flowPrevSource[1] = m_previousCapturedEyeFrames[1].texture;
                    flowCurrSource[0] = m_capturedEyeFrames[0].texture;
                    flowCurrSource[1] = m_capturedEyeFrames[1].texture;
                    flowPrev[0] = m_previousCapturedEyeFrames[0].opticalFlowTexture;
                    flowPrev[1] = m_previousCapturedEyeFrames[1].opticalFlowTexture;
                    flowCurr[0] = m_capturedEyeFrames[0].opticalFlowTexture;
                    flowCurr[1] = m_capturedEyeFrames[1].opticalFlowTexture;
                    for (int eye = 0; eye < 2; ++eye) {
                        if (m_previousCapturedEyeFrames[eye].depthTexture &&
                            m_previousCapturedEyeFrames[eye].depthSerial == m_previousCapturedEyeFrames[eye].serial) {
                            flowPrevDepth[eye] = m_previousCapturedEyeFrames[eye].depthTexture;
                        }
                        if (m_capturedEyeFrames[eye].depthTexture &&
                            m_capturedEyeFrames[eye].depthSerial == m_capturedEyeFrames[eye].serial) {
                            flowCurrDepth[eye] = m_capturedEyeFrames[eye].depthTexture;
                        }
                        flowPrevSource[eye]->AddRef();
                        flowCurrSource[eye]->AddRef();
                        flowPrev[eye]->AddRef();
                        flowCurr[eye]->AddRef();
                        if (flowPrevDepth[eye]) flowPrevDepth[eye]->AddRef();
                        if (flowCurrDepth[eye]) flowCurrDepth[eye]->AddRef();
                    }
                }
                if ((presentPairId % 300) == 0) {
                    Log("OpenXRManager: AER complete pair promoted. pair=%llu left=%llu right=%llu historyL=%llu historyR=%llu\n",
                        static_cast<unsigned long long>(presentPairId),
                        static_cast<unsigned long long>(m_capturedEyeFrames[0].serial),
                        static_cast<unsigned long long>(m_capturedEyeFrames[1].serial),
                        static_cast<unsigned long long>(m_previousCapturedEyeFrames[0].serial),
                        static_cast<unsigned long long>(m_previousCapturedEyeFrames[1].serial));
                }
            }
        }
    if (runAERV2Flow && m_opticalFlow) {
        if (!m_opticalFlow->EnsureInitialized(m_d3dDevice,
                m_capturedEyeFrames[0].width,
                m_capturedEyeFrames[0].height,
                static_cast<DXGI_FORMAT>(m_capturedEyeFrames[0].format))) {
            Log("OpenXRManager: AER V2 optical-flow init failed at flow stage. pair=%llu\n",
                static_cast<unsigned long long>(flowPairId));
            runAERV2Flow = false;
        }
    }
    if (!runAERV2Flow || !m_opticalFlow) {
        if (presentLock.owns_lock()) {
            presentLock.unlock();
        }
    }

    // Hand off Convert+Execute+Synth to the background worker so OnPresent does
    // NOT CPU-block on ~3.5ms/frame of GPU sync. Source refs are already addref'd
    // above; ownership is transferred into the job (worker releases on completion).
    // Submit thread reads m_interpolatedPairId atomically — if the worker has
    // published for the current pair, V2 frames are used; otherwise we fall back
    // to raw current eye. Single-slot queue: if a fresh pair lands while worker
    // is busy, the older queued (but not yet picked) job is replaced (refs
    // released) so we never grow latency past one synth interval.
    if (presentLock.owns_lock()) {
        presentLock.unlock();
    }
    if (runAERV2Flow && m_opticalFlow) {
        auto job = std::make_unique<AERV2Job>();
        job->pairId = flowPairId;
        // Pair-counter id for the in-between submit cadence (see AERV2Job).
        job->submitPairId = presentPairId;
        if (GetAERV2Enabled() != 0) {
            job->renderedEye = presentEye;
            job->syntheticEye = presentEye ^ 1;
        }
        for (int eye = 0; eye < 2; ++eye) {
            job->flowPrevSource[eye] = flowPrevSource[eye];
            job->flowCurrSource[eye] = flowCurrSource[eye];
            job->flowPrev[eye]       = flowPrev[eye];
            job->flowCurr[eye]       = flowCurr[eye];
            job->prevDepth[eye]      = flowPrevDepth[eye];
            job->currDepth[eye]      = flowCurrDepth[eye];
            job->prevPose[eye] = m_previousCapturedEyeFrames[eye].pose;
            job->currPose[eye] = m_capturedEyeFrames[eye].pose;
            job->fov[eye]      = m_capturedEyeFrames[eye].fov;
            job->hasView[eye]  =
                m_previousCapturedEyeFrames[eye].hasView &&
                m_capturedEyeFrames[eye].hasView;
            // Cleared so the cleanup-fallback below sees only owned-by-job pointers.
            flowPrevSource[eye] = nullptr;
            flowCurrSource[eye] = nullptr;
            flowPrev[eye]       = nullptr;
            flowCurr[eye]       = nullptr;
            flowPrevDepth[eye]  = nullptr;
            flowCurrDepth[eye]  = nullptr;
        }
        // Attach engine-side ground truth: motion vectors + depth captured via
        // Streamline slSetTag hook. These are AddRef'd here and Released by the
        // worker on completion (see ReleaseAERV2JobRefs). They may be null on
        // very early frames before the NGX hook fires; worker handles that.
        job->engineMv = NgxAcquireMotionVectors();
        {
            std::lock_guard<std::mutex> lock(m_presentMutex);
            if (m_depthSnapshot && m_depthSnapshotSerial != 0) {
                m_depthSnapshot->AddRef();
                job->engineDepth = m_depthSnapshot;
            }
        }

        if (!job->engineDepth) {
            job->engineDepth = NgxAcquireDepth();
        }
        // Record each captured eye's serial so the worker can identify the
        // older eye of the pair (lower serial = captured earlier = candidate
        // for MV-warp forward extrapolation).
        job->currSerial[0] = m_capturedEyeFrames[0].serial;
        job->currSerial[1] = m_capturedEyeFrames[1].serial;
        if (GetAERV2Enabled() != 0) {
            ProcessAERV2Job(std::move(job));
        } else {
            StartAERV2WorkerIfNeeded();
            std::unique_ptr<AERV2Job> replacedJob;
            {
                std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
                if (m_aerV2PendingJob) {
                    replacedJob = std::move(m_aerV2PendingJob);
                }
                m_aerV2PendingJob = std::move(job);
            }
            m_aerV2JobCv.notify_one();
            if (replacedJob) {
                ReleaseAERV2JobRefs(*replacedJob);
                if ((flowPairId % 300) == 0) {
                    Log("OpenXRManager: AER V2 worker behind, replaced stale queued pair=%llu with %llu\n",
                        static_cast<unsigned long long>(replacedJob->pairId),
                        static_cast<unsigned long long>(flowPairId));
                }
            }
        }
    }

    // Any refs we addref'd but couldn't transfer (e.g. runAERV2Flow false after
    // the addref above, or m_opticalFlow null) get released here. After the job
    // was built, these are already nulled — Release on null is a no-op.
    for (int eye = 0; eye < 2; ++eye) {
        if (flowPrevSource[eye]) flowPrevSource[eye]->Release();
        if (flowCurrSource[eye]) flowCurrSource[eye]->Release();
        if (flowPrev[eye]) flowPrev[eye]->Release();
        if (flowCurr[eye]) flowCurr[eye]->Release();
        if (flowPrevDepth[eye]) flowPrevDepth[eye]->Release();
        if (flowCurrDepth[eye]) flowCurrDepth[eye]->Release();
    }

    // Idea #2 — synced-pose freshness (per-present low-pass nudge). syncSequential
    // freezes the head pose per pair for coherent stereo, but that lags on head
    // turns. Each present we nudge the synced pose toward the LIVE pose by
    // g_poseBlend (0=frozen, 1=live). The pair-boundary full snap below still
    // runs as the hard reset; this just closes the lag gap between snaps.
    // Both eyes still read the SAME synced pose -> stereo stays coherent; only
    // the lag shrinks. Skipped until the first pair has latched (valid) and when
    // the blend is 0 (no-op).
    if (syncSequential && !aerWarmupFrame) {
        const float pb = GetPoseBlend();
        if (pb > 0.0f && m_syncedPoseValid.load(std::memory_order_relaxed)) {
            const float keep = 1.0f - pb;
            m_syncedPosX.store(m_syncedPosX.load(std::memory_order_relaxed) * keep +
                               m_posX.load(std::memory_order_relaxed) * pb, std::memory_order_relaxed);
            m_syncedPosY.store(m_syncedPosY.load(std::memory_order_relaxed) * keep +
                               m_posY.load(std::memory_order_relaxed) * pb, std::memory_order_relaxed);
            m_syncedPosZ.store(m_syncedPosZ.load(std::memory_order_relaxed) * keep +
                               m_posZ.load(std::memory_order_relaxed) * pb, std::memory_order_relaxed);
            // Orientation: component lerp + renormalize (valid for small per-present deltas).
            float ox = m_syncedOriX.load(std::memory_order_relaxed) * keep + m_oriX.load(std::memory_order_relaxed) * pb;
            float oy = m_syncedOriY.load(std::memory_order_relaxed) * keep + m_oriY.load(std::memory_order_relaxed) * pb;
            float oz = m_syncedOriZ.load(std::memory_order_relaxed) * keep + m_oriZ.load(std::memory_order_relaxed) * pb;
            float ow = m_syncedOriW.load(std::memory_order_relaxed) * keep + m_oriW.load(std::memory_order_relaxed) * pb;
            const float on = std::sqrt(ox*ox + oy*oy + oz*oz + ow*ow);
            if (on > 1e-9f) { const float inv = 1.0f / on;
                ox *= inv; oy *= inv; oz *= inv; ow *= inv; }
            m_syncedOriX.store(ox, std::memory_order_relaxed);
            m_syncedOriY.store(oy, std::memory_order_relaxed);
            m_syncedOriZ.store(oz, std::memory_order_relaxed);
            m_syncedOriW.store(ow, std::memory_order_relaxed);
        }
    }

    if (syncSequential && presentEye == 1 && !aerWarmupFrame) {
        latchSyncedSequentialPair();
    }

    if (aerEnabled) {
        if (aerWarmupFrame) {
            --m_aerWarmupRemaining;
        } else {
            m_renderEyeIndex.store(presentEye ^ 1, std::memory_order_relaxed);
        }
    }

    if (backBuffer) {
        backBuffer->Release();
        backBuffer = nullptr;
    }

    // [HMD-Paced Frame Sync] Lock the game engine to the OpenXR compositor rate.
    // By waiting for the compositor to finish xrEndFrame, we ensure the game
    // never runs ahead, and the next GetHeadPose will have the absolutely
    // fresh predicted display time for the subsequent frame.
    //
    // IMPORTANT: this wait is MONO-ONLY. An earlier attempt extended it to AER
    // V2 (mode-6) on the theory that pacing captures to submits would help, but
    // it introduced a feedback loop with the alternating-eye captures: the game
    // thread waking point drifted vsync-to-vsync, so the rendered eye's pose age
    // at display time varied -> variable ATW delta -> whole-image jitter in BOTH
    // eyes on head turns. Mono is immune (single eye, submits at a consistent
    // pose baseline). AER V2 instead lets the engine free-run and relies on the
    // per-eye mode-6 warp + runtime ATW for smoothness. See
    // docs/aer-v2-mode6-corrected.md.
    if (monoEnabled && !aerEnabled && m_frameSyncEvent) {
        WaitForSingleObject(m_frameSyncEvent, 1000);
    }

    if ((s_presentCount % 300) != 1) return;

    Log("OpenXRManager: Present observed. hwnd=%p size=%ux%u format=%u backbufferIndex=%u resourceWidth=%llu resourceHeight=%u sessionRunning=%d aer=%d sync=%d eye=%d warmup=%d pair=%llu\n",
        desc.OutputWindow,
        desc.BufferDesc.Width,
        desc.BufferDesc.Height,
        static_cast<unsigned>(desc.BufferDesc.Format),
        backBufferIndex,
        static_cast<unsigned long long>(resourceDesc.Width),
        resourceDesc.Height,
        IsSessionRunning() ? 1 : 0,
        aerEnabled ? 1 : 0,
        syncSequential ? 1 : 0,
        presentEye,
        aerWarmupFrame ? 1 : 0,
        static_cast<unsigned long long>(presentPairId));
}

void OpenXRManager::ReleaseAERV2JobRefs(AERV2Job& job) {
    for (int eye = 0; eye < 2; ++eye) {
        if (job.flowPrevSource[eye]) { job.flowPrevSource[eye]->Release(); job.flowPrevSource[eye] = nullptr; }
        if (job.flowCurrSource[eye]) { job.flowCurrSource[eye]->Release(); job.flowCurrSource[eye] = nullptr; }
        if (job.flowPrev[eye])       { job.flowPrev[eye]->Release();       job.flowPrev[eye] = nullptr; }
        if (job.flowCurr[eye])       { job.flowCurr[eye]->Release();       job.flowCurr[eye] = nullptr; }
        if (job.prevDepth[eye])      { job.prevDepth[eye]->Release();      job.prevDepth[eye] = nullptr; }
        if (job.currDepth[eye])      { job.currDepth[eye]->Release();      job.currDepth[eye] = nullptr; }
    }
    if (job.engineMv)    { job.engineMv->Release();    job.engineMv = nullptr; }
    if (job.engineDepth) { job.engineDepth->Release(); job.engineDepth = nullptr; }
}

void OpenXRManager::StartAERV2WorkerIfNeeded() {
    if (m_aerV2WorkerThread.joinable()) {
        return;
    }
    m_aerV2WorkerShutdown.store(false, std::memory_order_release);
    m_aerV2WorkerThread = std::thread(&OpenXRManager::AERV2WorkerThreadMain, this);
}

void OpenXRManager::StopAERV2Worker() {
    if (!m_aerV2WorkerThread.joinable()) {
        // Still might have a pending job sitting from before the worker started.
        std::unique_ptr<OpenXRManager::AERV2Job> stale;
        {
            std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
            stale = std::move(m_aerV2PendingJob);
        }
        if (stale) {
            ReleaseAERV2JobRefs(*stale);
        }
        return;
    }
    {
        std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
        m_aerV2WorkerShutdown.store(true, std::memory_order_release);
    }
    m_aerV2JobCv.notify_all();
    m_aerV2WorkerThread.join();
    // Drain any leftover queued job (worker exited without picking it up).
    std::unique_ptr<OpenXRManager::AERV2Job> stale;
    {
        std::lock_guard<std::mutex> lock(m_aerV2JobMutex);
        stale = std::move(m_aerV2PendingJob);
    }
    if (stale) {
        ReleaseAERV2JobRefs(*stale);
    }
    m_aerV2BusyPairId.store(0, std::memory_order_relaxed);
}

void OpenXRManager::ProcessAERV2Job(std::unique_ptr<AERV2Job> job) {
    if (!job) {
        return;
    }
    m_aerV2BusyPairId.store(job->pairId, std::memory_order_release);

    // Phase 2b/2c/2d: run motion-vector forward warp on the OLDER eye of
    // the current pair so the AER submit can substitute that eye's stale
    // image with an "advanced 1 frame forward by engine MV" version. Both
    // submitted eyes then represent the SAME simulation timestamp,
    // collapsing the within-pair gap that produces the "pushed back when
    // walking" jitter.
    bool mvWarpAttempted = false;
    bool mvWarpOk[2] = {false, false};
    const int olderEye = (job->syntheticEye >= 0 && job->syntheticEye < 2)
        ? job->syntheticEye
        : ((job->currSerial[0] <= job->currSerial[1]) ? 0 : 1);
    if (GetAERV2Enabled() == 0 && job->engineMv && job->flowCurrSource[olderEye] &&
        m_d3dDevice && m_mvWarpedEye[olderEye]) {
        mvWarpAttempted = true;
        if (!m_mvWarp) m_mvWarp = std::make_unique<MotionVectorWarp>();
        const D3D12_RESOURCE_DESC dstDesc = m_mvWarpedEye[0]->GetDesc();
        const bool warpInitOk = m_mvWarp->EnsureInitialized(
            m_d3dDevice, dstDesc.Format,
            static_cast<uint32_t>(dstDesc.Width), dstDesc.Height);
        if (!m_mvWarpQueue) {
            D3D12_COMMAND_QUEUE_DESC qd{};
            qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
            if (FAILED(m_d3dDevice->CreateCommandQueue(&qd, IID_PPV_ARGS(&m_mvWarpQueue))) ||
                FAILED(m_d3dDevice->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_mvWarpAlloc))) ||
                FAILED(m_d3dDevice->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_mvWarpAlloc.Get(), nullptr, IID_PPV_ARGS(&m_mvWarpList))) ||
                FAILED(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_mvWarpFence)))) {
                Log("OpenXRManager: [worker] MV-warp queue/list/fence init FAILED\n");
                m_mvWarpQueue.Reset(); m_mvWarpAlloc.Reset();
                m_mvWarpList.Reset(); m_mvWarpFence.Reset();
            } else {
                m_mvWarpQueue->SetName(L"AERV2_mvwarp_queue");
                m_mvWarpAlloc->SetName(L"AERV2_mvwarp_alloc");
                m_mvWarpList->SetName(L"AERV2_mvwarp_list");
                m_mvWarpFence->SetName(L"AERV2_mvwarp_fence");
                m_mvWarpList->Close();
                m_mvWarpEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
            }
        }

        if (warpInitOk && m_mvWarpQueue && m_mvWarpList && m_mvWarpFence && m_mvWarpEvent) {
            if (SUCCEEDED(m_mvWarpAlloc->Reset()) &&
                SUCCEEDED(m_mvWarpList->Reset(m_mvWarpAlloc.Get(), nullptr))) {
                D3D12_RESOURCE_BARRIER barriers[4] = {};
                UINT bc = 0;
                auto addBarrier = [&](ID3D12Resource* r, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
                    if (!r || before == after) return;
                    barriers[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                    barriers[bc].Transition.pResource = r;
                    barriers[bc].Transition.StateBefore = before;
                    barriers[bc].Transition.StateAfter = after;
                    barriers[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                    ++bc;
                };
                addBarrier(job->flowCurrSource[olderEye], D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                addBarrier(job->engineMv, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
                addBarrier(m_mvWarpedEye[olderEye].Get(), D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
                if (bc > 0) m_mvWarpList->ResourceBarrier(bc, barriers);

                const float kStartScaleX = 1.0f;
                const float kStartScaleY = 1.0f;
                mvWarpOk[olderEye] = m_mvWarp->RecordWarp(
                    m_mvWarpList.Get(),
                    job->flowCurrSource[olderEye],
                    job->engineMv,
                    m_mvWarpedEye[olderEye].Get(),
                    kStartScaleX, kStartScaleY);

                bc = 0;
                addBarrier(job->flowCurrSource[olderEye], D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
                addBarrier(job->engineMv, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COMMON);
                addBarrier(m_mvWarpedEye[olderEye].Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COMMON);
                if (bc > 0) m_mvWarpList->ResourceBarrier(bc, barriers);
                m_mvWarpList->Close();

                if (mvWarpOk[olderEye]) {
                    ID3D12CommandList* lists[] = { m_mvWarpList.Get() };
                    m_mvWarpQueue->ExecuteCommandLists(1, lists);
                    ++m_mvWarpFenceValue;
                    m_mvWarpQueue->Signal(m_mvWarpFence.Get(), m_mvWarpFenceValue);
                    if (m_mvWarpFence->GetCompletedValue() < m_mvWarpFenceValue) {
                        m_mvWarpFence->SetEventOnCompletion(m_mvWarpFenceValue, m_mvWarpEvent);
                        WaitForSingleObject(m_mvWarpEvent, INFINITE);
                    }
                    m_mvWarpedEyeReady[olderEye] = true;
                    m_mvWarpedValidPairId[olderEye].store(job->pairId, std::memory_order_release);
                }
            }
        }
    }

    const bool mvAvail = (job->engineMv != nullptr);
    const bool depthAvail = (job->engineDepth != nullptr);
    const bool temporalDepthAvail =
        job->prevDepth[0] && job->currDepth[0] &&
        job->prevDepth[1] && job->currDepth[1];
    if ((job->pairId % 300) == 0 || job->pairId == 1) {
        uint32_t mvW = 0, mvH = 0, mvFmt = 0;
        uint32_t dW = 0, dH = 0, dFmt = 0;
        if (job->engineMv) {
            auto d = job->engineMv->GetDesc();
            mvW = static_cast<uint32_t>(d.Width);
            mvH = d.Height;
            mvFmt = static_cast<uint32_t>(d.Format);
        }
        if (job->engineDepth) {
            auto d = job->engineDepth->GetDesc();
            dW = static_cast<uint32_t>(d.Width);
            dH = d.Height;
            dFmt = static_cast<uint32_t>(d.Format);
        }
        Log("OpenXRManager: [worker] pair=%llu engineMv=%p (%ux%u fmt=%u avail=%d) engineDepth=%p avail=%d temporalDepth=%d olderEye=%d (serials=[%llu,%llu]) warpAttempted=%d warpOk=[%d,%d]\n",
            static_cast<unsigned long long>(job->pairId),
            job->engineMv, mvW, mvH, mvFmt, mvAvail ? 1 : 0,
            job->engineDepth, depthAvail ? 1 : 0,
            temporalDepthAvail ? 1 : 0,
            olderEye,
            static_cast<unsigned long long>(job->currSerial[0]),
            static_cast<unsigned long long>(job->currSerial[1]),
            mvWarpAttempted ? 1 : 0,
            mvWarpOk[0] ? 1 : 0,
            mvWarpOk[1] ? 1 : 0);
    }

    bool convertedOk = true;
    bool inBetweenInputsOk = false;  // 1:1 half-rate: renderedEye history also converted
    if (m_opticalFlow) {
        if (GetAERV2Enabled() != 0 && job->renderedEye >= 0 && job->syntheticEye >= 0) {
            // Same-eye temporal: the population path (OnPresent) fills the
            // syntheticEye flow slots (prev+curr captures of that eye), and
            // ProcessTemporalFrame below consumes flowPrev/flowCurr[syntheticEye].
            if (!m_opticalFlow->ConvertToInputTexture(job->flowPrevSource[job->syntheticEye], job->flowPrev[job->syntheticEye]) ||
                !m_opticalFlow->ConvertToInputTexture(job->flowCurrSource[job->syntheticEye], job->flowCurr[job->syntheticEye])) {
                convertedOk = false;
            }
            // 1:1 half-rate: the in-between pair warps BOTH eyes, so also convert
            // the renderedEye's own temporal history (OnPresent stages it only
            // when half-rate is on). Non-fatal — failure just skips the in-between.
            if (convertedOk && GetAERHalfRate() != 0 &&
                job->flowPrevSource[job->renderedEye] && job->flowCurrSource[job->renderedEye] &&
                job->flowPrev[job->renderedEye] && job->flowCurr[job->renderedEye]) {
                inBetweenInputsOk =
                    m_opticalFlow->ConvertToInputTexture(job->flowPrevSource[job->renderedEye], job->flowPrev[job->renderedEye]) &&
                    m_opticalFlow->ConvertToInputTexture(job->flowCurrSource[job->renderedEye], job->flowCurr[job->renderedEye]);
            }
        } else {
            for (int eye = 0; eye < 2 && convertedOk; ++eye) {
                if (!m_opticalFlow->ConvertToInputTexture(job->flowPrevSource[eye], job->flowPrev[eye]) ||
                    !m_opticalFlow->ConvertToInputTexture(job->flowCurrSource[eye], job->flowCurr[eye])) {
                    convertedOk = false;
                }
            }
        }
    } else {
        convertedOk = false;
    }

    bool leftOk = false;
    bool rightOk = false;
    bool leftSynth = false;
    bool rightSynth = false;
    bool realAERV2Used = false;
    bool syntheticOk = false;
    if (convertedOk) {
        const uint32_t synthSlot = static_cast<uint32_t>(job->pairId & 1ull);
        if (GetAERV2Enabled() != 0 && m_d3dDevice && m_d3dQueue && m_aerV2SynthEye[0][synthSlot] && m_aerV2SynthEye[1][synthSlot]) {
            const int syntheticEye = (job->syntheticEye >= 0 && job->syntheticEye < 2) ? job->syntheticEye : olderEye;
            const int renderedEye = (job->renderedEye >= 0 && job->renderedEye < 2) ? job->renderedEye : (syntheticEye ^ 1);
            if (!m_aerV2Pipeline) {
                m_aerV2Pipeline = std::make_unique<aer_v2::AerV2Pipeline>();
            }
            ID3D12Resource* flowInitSource = job->flowCurr[renderedEye] ? job->flowCurr[renderedEye] : job->flowPrev[syntheticEye];
            if (!flowInitSource) {
                Log("OpenXRManager: [worker] missing flow init source pair=%llu syntheticEye=%d renderedEye=%d\n",
                    static_cast<unsigned long long>(job->pairId),
                    syntheticEye,
                    renderedEye);
            } else {
            const D3D12_RESOURCE_DESC flowDesc = flowInitSource->GetDesc();
            m_aerV2Pipeline->SetMode(aer_v2::AerV2Pipeline::Mode::AERv2HighQ);
            m_aerV2Pipeline->SetForceMatchedEyePoses(true);
            if (m_aerV2Pipeline->EnsureInitialized(m_d3dDevice,
                                                  m_d3dQueue,
                                                  static_cast<uint32_t>(flowDesc.Width),
                                                  flowDesc.Height)) {
                const XrPosef predictedMatched = ExtrapolatePose(
                    job->prevPose[renderedEye],
                    job->currPose[renderedEye],
                    kAERV2FrameGenPoseT);
                XrPosef predictedSynthetic = ExtrapolatePose(
                    job->prevPose[syntheticEye],
                    job->currPose[syntheticEye],
                    kAERV2FrameGenPoseT);
                predictedSynthetic.orientation = predictedMatched.orientation;
                const float mvScaleX = NgxGetMvScaleX();
                const float mvScaleY = NgxGetMvScaleY();
                ID3D12Resource* realMvSource = nullptr;
                if (job->engineMv && m_d3dDevice && m_mvWarpQueue && m_mvWarpAlloc && m_mvWarpList && m_mvWarpFence && m_mvWarpEvent) {
                    const D3D12_RESOURCE_DESC mvDesc = job->engineMv->GetDesc();
                    bool recreateMvScratch = true;
                    if (m_aerV2MvScratch) {
                        const auto cur = m_aerV2MvScratch->GetDesc();
                        if (cur.Width == mvDesc.Width && cur.Height == mvDesc.Height && cur.Format == mvDesc.Format) {
                            recreateMvScratch = false;
                        } else {
                            m_aerV2MvScratch.Reset();
                        }
                    }
                    if (recreateMvScratch) {
                        D3D12_HEAP_PROPERTIES hp{};
                        hp.Type = D3D12_HEAP_TYPE_DEFAULT;
                        if (FAILED(m_d3dDevice->CreateCommittedResource(
                                &hp,
                                D3D12_HEAP_FLAG_SHARED,
                                &mvDesc,
                                D3D12_RESOURCE_STATE_COMMON,
                                nullptr,
                                IID_PPV_ARGS(&m_aerV2MvScratch)))) {
                            Log("OpenXRManager: [worker] failed to create shareable MV scratch\n");
                            m_aerV2MvScratch.Reset();
                        } else {
                            SetD3DName(m_aerV2MvScratch.Get(), L"AERV2_engine_mv_scratch_shared");
                        }
                    }
                    if (m_aerV2MvScratch &&
                        SUCCEEDED(m_mvWarpAlloc->Reset()) &&
                        SUCCEEDED(m_mvWarpList->Reset(m_mvWarpAlloc.Get(), nullptr))) {
                        const D3D12_RESOURCE_STATES mvScratchBefore = recreateMvScratch
                            ? D3D12_RESOURCE_STATE_COMMON
                            : D3D12_RESOURCE_STATE_COPY_SOURCE;
                        D3D12_RESOURCE_BARRIER bars[3] = {};
                        UINT bc = 0;
                        auto addBar = [&](ID3D12Resource* r, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
                            if (!r || before == after) return;
                            bars[bc].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                            bars[bc].Transition.pResource = r;
                            bars[bc].Transition.StateBefore = before;
                            bars[bc].Transition.StateAfter = after;
                            bars[bc].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                            ++bc;
                        };
                        addBar(job->engineMv, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE);
                        addBar(m_aerV2MvScratch.Get(), mvScratchBefore, D3D12_RESOURCE_STATE_COPY_DEST);
                        if (bc > 0) m_mvWarpList->ResourceBarrier(bc, bars);
                        m_mvWarpList->CopyResource(m_aerV2MvScratch.Get(), job->engineMv);
                        bc = 0;
                        addBar(job->engineMv, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
                        addBar(m_aerV2MvScratch.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
                        if (bc > 0) m_mvWarpList->ResourceBarrier(bc, bars);
                        m_mvWarpList->Close();
                        ID3D12CommandList* lists[] = { m_mvWarpList.Get() };
                        m_mvWarpQueue->ExecuteCommandLists(1, lists);
                        ++m_mvWarpFenceValue;
                        m_mvWarpQueue->Signal(m_mvWarpFence.Get(), m_mvWarpFenceValue);
                        if (m_mvWarpFence->GetCompletedValue() < m_mvWarpFenceValue) {
                            m_mvWarpFence->SetEventOnCompletion(m_mvWarpFenceValue, m_mvWarpEvent);
                            WaitForSingleObject(m_mvWarpEvent, INFINITE);
                        }
                        realMvSource = m_aerV2MvScratch.Get();
                    }
                }
                ID3D12Resource* sourcePrevDepth = job->prevDepth[syntheticEye] ? job->prevDepth[syntheticEye] : job->engineDepth;
                ID3D12Resource* sourceCurrDepth = job->currDepth[syntheticEye] ? job->currDepth[syntheticEye] : job->engineDepth;
                syntheticOk = m_aerV2Pipeline->ProcessTemporalFrame(
                    syntheticEye,
                    job->flowPrev[syntheticEye], job->flowCurr[syntheticEye],
                    job->flowCurrSource[syntheticEye], job->flowCurrSource[syntheticEye],
                    realMvSource, sourcePrevDepth, sourceCurrDepth,
                    mvScaleX, mvScaleY,
                    job->fov[syntheticEye],
                    job->prevPose[syntheticEye], job->currPose[syntheticEye], predictedSynthetic,
                    kAERV2FrameGenPoseT,
                    m_aerV2SynthEye[syntheticEye][synthSlot].Get());
                if (syntheticEye == 0) {
                    leftOk = syntheticOk;
                    leftSynth = syntheticOk;
                } else {
                    rightOk = syntheticOk;
                    rightSynth = syntheticOk;
                }
                realAERV2Used = syntheticOk;

                // ===== 1:1 half-rate: BOTH-EYE in-between frame (mid, blend 0.5) =====
                // Each eye gets its OWN fresh NvOF temporal mid so BOTH eyes update
                // every display interval (90 Hz). The earlier 1-warp version reused
                // the keyframe synth for the syntheticEye half -> that eye froze for
                // 2 intervals (45 Hz, stuttery) = the "not 90 Hz both eyes" jitter.
                // Pure NvOF (engineMv=null): same-eye temporal flow = real stereo.
                // 2 warps/pair; the present-thread throttle that made this too slow
                // before is now removed, so the budget is there.
                if (syntheticOk && GetAERHalfRate() != 0 && inBetweenInputsOk &&
                    m_aerV2InBetween[0][synthSlot] && m_aerV2InBetween[1][synthSlot]) {
                    bool bothEyesOk = true;
                    XrPosef predInB[2]{};
                    for (int eye = 0; eye < 2 && bothEyesOk; ++eye) {
                        if (!job->flowPrev[eye] || !job->flowCurr[eye] || !job->flowCurrSource[eye]) {
                            bothEyesOk = false;
                            break;
                        }
                        predInB[eye] = ExtrapolatePose(job->prevPose[eye], job->currPose[eye], kAERV2FrameGenPoseT);
                        predInB[eye].orientation = predictedMatched.orientation;  // matched stereo
                        ID3D12Resource* pD = job->prevDepth[eye] ? job->prevDepth[eye] : job->engineDepth;
                        ID3D12Resource* cD = job->currDepth[eye] ? job->currDepth[eye] : job->engineDepth;
                        bothEyesOk = m_aerV2Pipeline->ProcessTemporalFrame(
                            eye,
                            job->flowPrev[eye], job->flowCurr[eye],
                            job->flowCurrSource[eye], job->flowCurrSource[eye],
                            nullptr, pD, cD,
                            mvScaleX, mvScaleY,
                            job->fov[eye],
                            job->prevPose[eye], job->currPose[eye], predInB[eye],
                            kAERV2FrameGenPoseT,
                            m_aerV2InBetween[eye][synthSlot].Get());
                    }
                    if (bothEyesOk) {
                        std::lock_guard<std::mutex> publishLock(m_presentMutex);
                        m_inBetweenReadyPairId = job->submitPairId;   // PAIR counter
                        m_inBetweenSlot = synthSlot;
                        m_inBetweenSyntheticEye = syntheticEye;        // -1 also fine now (both eyes fresh)
                        for (int eye = 0; eye < 2; ++eye) {
                            m_inBetweenEyePoses[eye] = predInB[eye];
                            m_inBetweenEyeFovs[eye] = job->fov[eye];
                            m_inBetweenEyeViewsValid[eye] = job->hasView[eye];
                        }
                    }
                    if ((job->submitPairId % 300) == 1) {
                        Log("OpenXRManager: [1:1 producer] BOTH-eye in-between pair=%llu ok=%d publishedReady=%llu\n",
                            static_cast<unsigned long long>(job->submitPairId), bothEyesOk ? 1 : 0,
                            static_cast<unsigned long long>(m_inBetweenReadyPairId));
                    }
                } else if (GetAERHalfRate() != 0 && (job->submitPairId % 300) == 1) {
                    Log("OpenXRManager: [1:1 producer] in-between SKIPPED pair=%llu synthOk=%d inputsOk=%d haveTex0=%d haveTex1=%d\n",
                        static_cast<unsigned long long>(job->submitPairId), syntheticOk ? 1 : 0, inBetweenInputsOk ? 1 : 0,
                        m_aerV2InBetween[0][synthSlot] ? 1 : 0, m_aerV2InBetween[1][synthSlot] ? 1 : 0);
                }
            }
            }
        }

        if (!realAERV2Used && GetAERV2Enabled() == 0 && m_opticalFlow) {
            leftOk  = m_opticalFlow->ExecuteFlow(job->flowPrev[0], job->flowCurr[0], 0);
            rightOk = m_opticalFlow->ExecuteFlow(job->flowPrev[1], job->flowCurr[1], 1);
            if (leftOk)  leftSynth  = m_opticalFlow->SynthesizeMidpoint(job->flowPrev[0], job->flowCurr[0], 0);
            if (rightOk) rightSynth = m_opticalFlow->SynthesizeMidpoint(job->flowPrev[1], job->flowCurr[1], 1);
        }
    }

    bool published = false;
    if (syntheticOk) {
        std::lock_guard<std::mutex> publishLock(m_presentMutex);
        if (job->pairId > m_interpolatedPairId) {
            m_interpolatedPairId = job->pairId;
            m_interpolatedSynthSlot = static_cast<uint32_t>(job->pairId & 1ull);
            m_interpolatedSyntheticEye = olderEye;
            for (int eye = 0; eye < 2; ++eye) {
                if (eye == olderEye) {
                    m_interpolatedEyePoses[eye] = ExtrapolatePose(
                        job->prevPose[eye],
                        job->currPose[eye],
                        kAERV2FrameGenPoseT);
                    m_interpolatedEyePoses[eye].orientation = ExtrapolatePose(
                        job->prevPose[olderEye ^ 1],
                        job->currPose[olderEye ^ 1],
                        kAERV2FrameGenPoseT).orientation;
                    m_interpolatedEyeFovs[eye] = job->fov[eye];
                    m_interpolatedEyeViewsValid[eye] = job->hasView[eye];
                } else {
                    m_interpolatedEyeViewsValid[eye] = false;
                }
            }
            published = true;
        }
        m_aerV2DonePairId.store(job->pairId, std::memory_order_release);
    }

    const bool verbose = (g_verboseLog != 0);
    if (verbose || (job->pairId % 300) == 0 || !leftOk || !rightOk || !leftSynth || !rightSynth) {
        Log("OpenXRManager: [worker] pair=%llu flowL=%d flowR=%d synthL=%d synthR=%d converted=%d published=%d realAERV2=%d (lastSubmittedPair=%llu interpolatedPair=%llu)\n",
            static_cast<unsigned long long>(job->pairId),
            leftOk ? 1 : 0,
            rightOk ? 1 : 0,
            leftSynth ? 1 : 0,
            rightSynth ? 1 : 0,
            convertedOk ? 1 : 0,
            published ? 1 : 0,
            realAERV2Used ? 1 : 0,
            static_cast<unsigned long long>(m_lastSubmittedPairId),
            static_cast<unsigned long long>(m_interpolatedPairId));
    }

    ReleaseAERV2JobRefs(*job);
    m_aerV2BusyPairId.store(0, std::memory_order_release);
}

void OpenXRManager::AERV2WorkerThreadMain() {
    Log("OpenXRManager: AER V2 worker thread started.\n");
    while (true) {
        std::unique_ptr<AERV2Job> job;
        {
            std::unique_lock<std::mutex> lock(m_aerV2JobMutex);
            m_aerV2JobCv.wait(lock, [this]() {
                return m_aerV2WorkerShutdown.load(std::memory_order_acquire) ||
                       m_aerV2PendingJob != nullptr;
            });
            if (m_aerV2WorkerShutdown.load(std::memory_order_acquire) && !m_aerV2PendingJob) {
                break;
            }
            job = std::move(m_aerV2PendingJob);
            if (job) {
                m_aerV2BusyPairId.store(job->pairId, std::memory_order_release);
            }
        }
        if (!job) {
            continue;
        }
        ProcessAERV2Job(std::move(job));
    }
    Log("OpenXRManager: AER V2 worker thread exiting.\n");
}

void OpenXRManager::Shutdown() {
    std::lock_guard<std::mutex> initLock(m_initMutex);
    m_stopFrameThread.store(true, std::memory_order_relaxed);
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
