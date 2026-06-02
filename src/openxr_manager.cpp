#include "openxr_manager.h"
#include <cstdarg>
#include <cstdio>
#include <vector>
#include <dxgi1_4.h>
#include <cstring>
#include <cmath>
#include <utility>

extern void Log(const char* fmt, ...);
extern "C" int GetDisableRoll();
extern "C" float GetForcedFov();
extern "C" float GetMenuFov();
extern "C" int GetMenuRectMode();
extern "C" int GetMenuMode();
extern "C" int GetSyncSequential();
extern "C" int Get3DofMovement();
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

static int64_t PickMonoSwapchainFormat(const std::vector<int64_t>& runtimeFormats, int64_t gameFormat) {
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

static XrFovf ApplyForcedProjectionFov(const XrFovf& sourceFov, float width, float height) {
    const float forceFov = GetForcedFov();
    if (forceFov <= 1.0f || forceFov >= 170.0f) {
        return sourceFov;
    }

    float aspect = 1.0f;
    if (width > 1.0f && height > 1.0f) {
        aspect = width / height;
    }
    if (aspect < 0.01f) {
        aspect = 1.0f;
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
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_previousCapturedEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.hasView = false;
    }
    for (CapturedEyeFrame& frame : m_pendingEyeFrames) {
        frame.serial = 0;
        frame.pairId = 0;
        frame.hasView = false;
    }
    m_lastSubmittedPairId = 0;
    m_interpolatedPairId = 0;
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
    // depth gave no confirmed benefit (the left-eye fix was the RealVR-style pose-pair
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
    if (desc.Format != DXGI_FORMAT_R32_TYPELESS &&
        desc.Format != DXGI_FORMAT_D32_FLOAT &&
        desc.Format != DXGI_FORMAT_R32_FLOAT) {
        if (m_depthLayerSupported) {
            Log("OpenXRManager: [DEPTH] disabling depth layer for unsupported source format=%u\n",
                static_cast<unsigned>(desc.Format));
        }
        m_depthLayerSupported = false;
        m_depthSwapchainFormat = 0;
        m_depthSnapshotSerial = 0;
        return false;
    }
    DXGI_FORMAT targetFormat = desc.Format;
    if (m_depthSwapchainFormat != 0) {
        targetFormat = static_cast<DXGI_FORMAT>(m_depthSwapchainFormat);
    }
    if (m_depthSnapshot) {
        const D3D12_RESOURCE_DESC cur = m_depthSnapshot->GetDesc();
        if (cur.Width == desc.Width && cur.Height == desc.Height && cur.Format == targetFormat) {
            return true;
        }
        m_depthSnapshot->Release();
        m_depthSnapshot = nullptr;
        m_depthSnapshotSerial = 0;
    }
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC sd = desc;
    // A depth format requires ALLOW_DEPTH_STENCIL; drop any unrelated source flags.
    sd.Format = targetFormat;
    sd.Flags = D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    const HRESULT hr = m_d3dDevice->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &sd,
        D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&m_depthSnapshot));
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
        static_cast<unsigned>(desc.Format), static_cast<unsigned>(targetFormat));
    return true;
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
    ID3D12Resource* gameDepth = OmoGetSceneDepthResource();
    const D3D12_RESOURCE_STATES gameDepthState = static_cast<D3D12_RESOURCE_STATES>(OmoGetSceneDepthState());
    bool depthCaptured = false;
    if (gameDepth && OmoGetSceneDepthState() != 0 && EnsureDepthSnapshot(gameDepth)) {
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
        if (preCount > 0) {
            m_captureCmdList->ResourceBarrier(preCount, pre);
        }
        D3D12_TEXTURE_COPY_LOCATION depthDst{};
        depthDst.pResource = m_depthSnapshot;
        depthDst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        depthDst.SubresourceIndex = 0;
        D3D12_TEXTURE_COPY_LOCATION depthSrc{};
        depthSrc.pResource = gameDepth;
        depthSrc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        depthSrc.SubresourceIndex = 0;
        m_captureCmdList->CopyTextureRegion(&depthDst, 0, 0, 0, &depthSrc, nullptr);
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
        m_captureCmdList->ResourceBarrier(postCount, post);
        depthCaptured = true;
    }

    m_captureCmdList->Close();
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

    // [TEAR-DIAG] One-shot readback dumps of the captured mono snapshot, to check
    // whether the source pixels are torn BEFORE any VR reprojection/submit. Guarded
    // by m_captureMutex (held on entry), so a plain static counter is safe.
    {
        static int s_dumpCount = 0;
        static bool s_prevDumpKey = false;
        // Manual trigger: press Home in-game exactly when tearing is visible.
        // Rising edge only, so one dump per keypress.
        const bool dumpKeyDown = (GetAsyncKeyState(VK_HOME) & 0x8000) != 0;
        const bool wantDump = dumpKeyDown && !s_prevDumpKey;
        s_prevDumpKey = dumpKeyDown;
        if (wantDump && s_dumpCount < 12) {
            const D3D12_RESOURCE_DESC snapDesc = snapshot->GetDesc();
            D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
            UINT numRows = 0;
            UINT64 rowBytes = 0;
            UINT64 totalBytes = 0;
            m_d3dDevice->GetCopyableFootprints(&snapDesc, 0, 1, 0, &footprint, &numRows, &rowBytes, &totalBytes);

            D3D12_HEAP_PROPERTIES rbHeap{};
            rbHeap.Type = D3D12_HEAP_TYPE_READBACK;
            D3D12_RESOURCE_DESC rbDesc{};
            rbDesc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
            rbDesc.Width = totalBytes;
            rbDesc.Height = 1;
            rbDesc.DepthOrArraySize = 1;
            rbDesc.MipLevels = 1;
            rbDesc.Format = DXGI_FORMAT_UNKNOWN;
            rbDesc.SampleDesc.Count = 1;
            rbDesc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;

            ID3D12Resource* readback = nullptr;
            if (SUCCEEDED(m_d3dDevice->CreateCommittedResource(&rbHeap, D3D12_HEAP_FLAG_NONE, &rbDesc,
                    D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&readback)))) {
                if (SUCCEEDED(currentAllocator->Reset()) &&
                    SUCCEEDED(m_captureCmdList->Reset(currentAllocator, nullptr))) {
                    D3D12_TEXTURE_COPY_LOCATION dst{};
                    dst.pResource = readback;
                    dst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
                    dst.PlacedFootprint = footprint;
                    D3D12_TEXTURE_COPY_LOCATION src{};
                    src.pResource = snapshot;
                    src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                    src.SubresourceIndex = 0;
                    m_captureCmdList->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
                    m_captureCmdList->Close();
                    ID3D12CommandList* lists[] = {m_captureCmdList};
                    m_d3dQueue->ExecuteCommandLists(1, lists);
                    if (WaitForQueueIdle(m_d3dQueue, m_captureFence, m_captureFenceEvent, m_captureFenceValue)) {
                        void* mapped = nullptr;
                        D3D12_RANGE readRange{0, static_cast<SIZE_T>(totalBytes)};
                        if (SUCCEEDED(readback->Map(0, &readRange, &mapped)) && mapped) {
                            char path[512];
                            snprintf(path, sizeof(path),
                                "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Cyberpunk 2077\\bin\\x64\\mono_snap_%llu.bin",
                                static_cast<unsigned long long>(serial));
                            FILE* fp = nullptr;
                            fopen_s(&fp, path, "wb");
                            if (fp) {
                                const uint32_t hdr[6] = {
                                    0x52414554u,
                                    static_cast<uint32_t>(snapDesc.Width),
                                    static_cast<uint32_t>(snapDesc.Height),
                                    static_cast<uint32_t>(snapDesc.Format),
                                    static_cast<uint32_t>(footprint.Footprint.RowPitch),
                                    static_cast<uint32_t>(numRows)};
                                fwrite(hdr, sizeof(hdr), 1, fp);
                                fwrite(mapped, 1, static_cast<size_t>(totalBytes), fp);
                                fclose(fp);
                                ++s_dumpCount;
                                Log("[TEAR-DIAG] dumped snapshot serial=%llu %ux%u fmt=%u rowPitch=%u rows=%u -> %s\n",
                                    static_cast<unsigned long long>(serial),
                                    static_cast<unsigned>(snapDesc.Width),
                                    static_cast<unsigned>(snapDesc.Height),
                                    static_cast<unsigned>(snapDesc.Format),
                                    static_cast<unsigned>(footprint.Footprint.RowPitch),
                                    static_cast<unsigned>(numRows), path);
                            }
                            const D3D12_RANGE noWrite{0, 0};
                            readback->Unmap(0, &noWrite);
                        }
                    }
                }
                readback->Release();
            }
        }
    }

    snapshot->Release();
    if ((serial % 300) == 1) {
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

    const DXGI_FORMAT opticalFlowFormat = GetAERV2OpticalFlowFormat(sourceDesc.Format);
    auto framesMatch = [width, height, format](const CapturedEyeFrame* frames) {
        for (int eye = 0; eye < 2; ++eye) {
            const CapturedEyeFrame& frame = frames[eye];
            if (!frame.texture || frame.width != width || frame.height != height || frame.format != format) {
                return false;
            }
        }
        return true;
    };
    const bool aerV2Enabled = GetAERV2Enabled() != 0;
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
                frame.width = 0;
                frame.height = 0;
                frame.format = 0;
                frame.serial = 0;
                frame.pairId = 0;
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
                    D3D12_HEAP_FLAG_NONE,
                    &sourceDesc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    nullptr,
                    IID_PPV_ARGS(&frame.texture)))) {
                Log("OpenXRManager: Failed to create AER %s texture\n", label);
                return false;
            }
            SetD3DNamef(frame.texture, L"AERV2_%ls_eye%d_color", nameLabel, eye);
            if (aerV2Enabled) {
                if (FAILED(m_d3dDevice->CreateCommittedResource(
                        &heapProps,
                        D3D12_HEAP_FLAG_NONE,
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
            frame.serial = 0;
            frame.pairId = 0;
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
    // runtime can do DEPTH-AWARE (positional) reprojection of the half-rate stale eye,
    // matching RealVR. Observed-state barriers only (never a guessed StateBefore).
    bool depthCaptured = false;
    {
        ID3D12Resource* gameDepth = OmoGetSceneDepthResource();
        const D3D12_RESOURCE_STATES gameDepthState = static_cast<D3D12_RESOURCE_STATES>(OmoGetSceneDepthState());
        if (gameDepth && OmoGetSceneDepthState() != 0 && EnsureDepthSnapshot(gameDepth)) {
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
            if (preCount > 0) {
                m_captureCmdList->ResourceBarrier(preCount, pre);
            }
            D3D12_TEXTURE_COPY_LOCATION depthDst{};
            depthDst.pResource = m_depthSnapshot;
            depthDst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            depthDst.SubresourceIndex = 0;
            D3D12_TEXTURE_COPY_LOCATION depthSrc{};
            depthSrc.pResource = gameDepth;
            depthSrc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            depthSrc.SubresourceIndex = 0;
            m_captureCmdList->CopyTextureRegion(&depthDst, 0, 0, 0, &depthSrc, nullptr);
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
            m_captureCmdList->ResourceBarrier(postCount, post);
            depthCaptured = true;
        }
    }

    m_captureCmdList->Close();
    ID3D12CommandList* cmdLists[] = {m_captureCmdList};
    m_d3dQueue->ExecuteCommandLists(1, cmdLists);
    
    ++m_captureFenceValue;
    m_d3dQueue->Signal(m_captureFence, m_captureFenceValue);

    {
        std::lock_guard<std::mutex> lock(m_presentMutex);
        frame->serial = serial;
        frame->pairId = pairId;
        if (depthCaptured) {
            m_depthSnapshotSerial = serial;
        }
        SetD3DNamef(frame->texture, L"AERV2_pending_eye%d_color_pair%llu_serial%llu", eyeIndex,
            static_cast<unsigned long long>(pairId),
            static_cast<unsigned long long>(serial));
        SetD3DNamef(frame->opticalFlowTexture, L"AERV2_pending_eye%d_ofinput_pair%llu_serial%llu", eyeIndex,
            static_cast<unsigned long long>(pairId),
            static_cast<unsigned long long>(serial));
    }

    if ((serial % 300) == 1) {
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

    const int64_t selectedFormat = PickMonoSwapchainFormat(runtimeFormats, static_cast<int64_t>(format));

    // Pick a runtime-supported depth format ONLY AFTER the game's scene depth resource
    // has been pinned. This remains intentionally conservative: only the R32-family
    // depth path is considered stable. The 64-bit R32G8X24 typeless family caused
    // repeated GPU removal during snapshot/submission experiments, so depth is kept
    // disabled there to preserve a working Mono baseline.
    ID3D12Resource* pinnedDepth = OmoGetSceneDepthResource();
    const DXGI_FORMAT pinnedDepthFormat = pinnedDepth ? pinnedDepth->GetDesc().Format : DXGI_FORMAT_UNKNOWN;
    int64_t selectedDepthFormat = 0;
    if (GetDepthSubmit() != 0 && m_depthLayerSupported && pinnedDepth) {
        for (int64_t rf : runtimeFormats) {
            if (rf == static_cast<int64_t>(DXGI_FORMAT_D32_FLOAT)) {
                selectedDepthFormat = static_cast<int64_t>(DXGI_FORMAT_D32_FLOAT);
                break;
            }
        }
        if (selectedDepthFormat == 0) {
            Log("OpenXRManager: no runtime D32_FLOAT depth format available — disabling depth layer\n");
            m_depthLayerSupported = false;
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
        if (GetXrRuntimeMode() == 1 && strcmp(ClassifyOpenXRRuntime(instanceProps.runtimeName), "SteamVR") != 0) {
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
        }

        event = {XR_TYPE_EVENT_DATA_BUFFER};
    }
}

DWORD OpenXRManager::FrameThreadMain() {
    Log("OpenXRManager: Frame thread started.\n");
    uint64_t monoWaitLogCounter = 0;
    uint64_t runtimeViewLogCounter = 0;
    uint64_t steamVrStartupWaitLogCounter = 0;

    while (!m_stopFrameThread.load(std::memory_order_relaxed)) {
        PollEvents();

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
                if (((++steamVrStartupWaitLogCounter % 300) == 1)) {
                    Log("OpenXRManager: SteamVR startup wait. Deferring frame loop until first present provides a backbuffer. width=%u height=%u format=%u\n",
                        startupWidth,
                        startupHeight,
                        startupFormat);
                }
                Sleep(1);
                continue;
            }
        }

        // Half-rate AER submit: when enabled and a complete pair has already been
        // submitted, skip this display interval entirely (no xrWaitFrame/Begin/End)
        // instead of resubmitting the stale pair. The runtime then sees the app
        // presenting at the pair rate (1/2 game rate) and engages its motion
        // smoothing (SSW/ASW) to synthesize the in-between frames, matching
        // RealVR's "1/2 Rate" reprojection. Requires runtime spacewarp enabled;
        // without it this looks like half-fps judder. Only gates on an already-
        // submitted complete pair, so startup / mono fallback run normally.
        if (IsAERSubmitEnabled() &&
            m_monoSubmitEnabled.load(std::memory_order_relaxed) &&
            GetAERHalfRate() != 0 &&
            GetMenuRectMode() == 0 && GetMenuMode() == 0) {
            uint64_t currentPair = 0;
            {
                std::lock_guard<std::mutex> lock(m_presentMutex);
                if (m_capturedEyeFrames[0].pairId != 0 &&
                    m_capturedEyeFrames[0].pairId == m_capturedEyeFrames[1].pairId) {
                    currentPair = m_capturedEyeFrames[0].pairId;
                }
            }
            if (currentPair != 0 && currentPair == m_lastSubmittedPairId) {
                if (m_frameSyncEvent) {
                    SetEvent(m_frameSyncEvent);
                }
                Sleep(1);
                continue;
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

        XrFrameBeginInfo beginInfo{XR_TYPE_FRAME_BEGIN_INFO};
        xrBeginFrame(m_session, &beginInfo);

        uint32_t viewCountOutput = 0;
        const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
        const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
        const bool aerEnabled = monoEnabled && IsAERSubmitEnabled();
        const bool useAerSubmit = aerEnabled && !menuRectActive;
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

                if (((++runtimeViewLogCounter % 300) == 1)) {
                    Log("OpenXRManager: Runtime view data. hfov=(%.2f, %.2f) vfov=(%.2f, %.2f) ipd=%.4f leftPos=(%.4f, %.4f, %.4f) rightPos=(%.4f, %.4f, %.4f)\n",
                        hfov0,
                        hfov1,
                        vfov0,
                        vfov1,
                        ipd,
                        m_views[0].pose.position.x,
                        m_views[0].pose.position.y,
                        m_views[0].pose.position.z,
                        m_views[1].pose.position.x,
                        m_views[1].pose.position.y,
                        m_views[1].pose.position.z);
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
            {
                std::lock_guard<std::mutex> renderLock(m_renderPoseMutex);
                if (!m_basePoseSet || m_recenterRequested.exchange(false, std::memory_order_relaxed)) {
                    m_basePose = location.pose;
                    m_basePoseSet = true;
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

            m_posX.store(relPos.x, std::memory_order_relaxed);
            m_posY.store(relPos.y, std::memory_order_relaxed);
            m_posZ.store(relPos.z, std::memory_order_relaxed);
            m_oriX.store(relOri.x, std::memory_order_relaxed);
            m_oriY.store(relOri.y, std::memory_order_relaxed);
            m_oriZ.store(relOri.z, std::memory_order_relaxed);
            m_oriW.store(relOri.w, std::memory_order_relaxed);
            m_poseValid.store(true, std::memory_order_relaxed);

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
            m_velValid.store(false, std::memory_order_relaxed);
        }

        if (monoReady && viewCountOutput == m_eyeSwapchains.size()) {
            if (useAerSubmit) {
                ID3D12Resource* eyeSources[2] = {};
                ID3D12Resource* generatedSources[2] = {};
                uint64_t eyeSerials[2] = {};
                uint64_t eyePairIds[2] = {};
                XrPosef eyePoses[2]{};
                XrFovf eyeFovs[2]{};
                bool eyeHasView[2] = {};
                uint64_t interpolatedPairId = 0;
                XrPosef interpolatedEyePoses[2]{};
                XrFovf interpolatedEyeFovs[2]{};
                bool interpolatedEyeHasView[2] = {};
                ID3D12Resource* depthSource = nullptr;
                {
                    std::lock_guard<std::mutex> lock(m_presentMutex);
                    for (int eye = 0; eye < 2; ++eye) {
                        if (m_capturedEyeFrames[eye].texture) {
                            eyeSources[eye] = m_capturedEyeFrames[eye].texture;
                            eyeSources[eye]->AddRef();
                            eyeSerials[eye] = m_capturedEyeFrames[eye].serial;
                            eyePairIds[eye] = m_capturedEyeFrames[eye].pairId;
                            eyePoses[eye] = m_capturedEyeFrames[eye].pose;
                            eyeFovs[eye] = m_capturedEyeFrames[eye].fov;
                            eyeHasView[eye] = m_capturedEyeFrames[eye].hasView;
                        }
                    }
                    interpolatedPairId = m_interpolatedPairId;
                    for (int eye = 0; eye < 2; ++eye) {
                        interpolatedEyePoses[eye] = m_interpolatedEyePoses[eye];
                        interpolatedEyeFovs[eye] = m_interpolatedEyeFovs[eye];
                        interpolatedEyeHasView[eye] = m_interpolatedEyeViewsValid[eye];
                    }
                    // [DEPTH] Pin the latest scene-depth snapshot for depth-aware AER
                    // reprojection. Same shared resource as the mono path; using the
                    // freshest snapshot (rather than strict per-serial match) is fine —
                    // depth changes slowly frame-to-frame and reprojection is approximate.
                    if (m_depthLayerSupported && m_depthSnapshot && m_depthSnapshotSerial != 0) {
                        depthSource = m_depthSnapshot;
                        depthSource->AddRef();
                    }
                }

                const uint64_t submitSerial = eyeSerials[0] > eyeSerials[1] ? eyeSerials[0] : eyeSerials[1];
                const bool completePair = GetAERPairGate() == 0 || (eyePairIds[0] != 0 && eyePairIds[0] == eyePairIds[1]);
                const bool useInterpolatedPair =
                    GetAERV2Enabled() != 0 &&
                    completePair &&
                    eyePairIds[0] != 0 &&
                    eyePairIds[0] == interpolatedPairId &&
                    m_opticalFlow != nullptr;
                if (useInterpolatedPair) {
                    for (int eye = 0; eye < 2; ++eye) {
                        ID3D12Resource* interp = m_opticalFlow->GetInterpolatedResource(eye);
                        if (interp) {
                            interp->AddRef();
                            generatedSources[eye] = interp;
                        }
                        if (interpolatedEyeHasView[eye]) {
                            eyePoses[eye] = interpolatedEyePoses[eye];
                            eyeFovs[eye] = interpolatedEyeFovs[eye];
                        }
                    }
                }
                
                m_cmdAllocatorIndex = (m_cmdAllocatorIndex + 1) % 3;
                ID3D12CommandAllocator* currentAllocator = m_cmdAllocators[m_cmdAllocatorIndex];
                if (m_fenceValue >= 3 && m_fence->GetCompletedValue() < m_fenceValue - 2) {
                    m_fence->SetEventOnCompletion(m_fenceValue - 2, m_fenceEvent);
                    WaitForSingleObject(m_fenceEvent, INFINITE);
                }

                ID3D12GraphicsCommandList* m_cmdList = m_cmdLists[m_cmdAllocatorIndex];

                if (eyeSources[0] && eyeSources[1] && eyeSerials[0] != 0 && eyeSerials[1] != 0 && completePair && eyeHasView[0] && eyeHasView[1] &&
                    SUCCEEDED(currentAllocator->Reset()) && SUCCEEDED(m_cmdList->Reset(currentAllocator, nullptr))) {
                    bool copyReady = true;
                    bool releaseOk = true;
                    bool useDepthLayer = (depthSource != nullptr) && m_depthLayerSupported;
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
                        if (sourceEye < 2) {
                            copySource = (useInterpolatedPair && generatedSources[sourceEye]) ? generatedSources[sourceEye] : eyeSources[sourceEye];
                        }
                        if (!texture || sourceEye >= 2 || !copySource) {
                            Log("OpenXRManager: AER source/target missing for eye %u sourceEye %u image %u\n", eye, sourceEye, imageIndex);
                            copyReady = false;
                            break;
                        }

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

                        // Submit the pose the frame was ACTUALLY rendered with (captured
                        // at camera-hook time in OnPresent). The submitted pose must match
                        // the rendered image or the runtime reprojects it the wrong way
                        // (tearing / wobble). For debugEye source overrides, follow the
                        // SOURCE eye's pose so the image+pose pair stays matched.
                        const uint32_t poseEye = (debugEye == 1 || debugEye == 2 || debugEye == 3) ? sourceEye : eye;
                        if (poseEye < 2) {
                            projectionViews[eye].pose = eyePoses[poseEye];
                            projectionViews[eye].fov = eyeFovs[poseEye];
                        } else {
                            projectionViews[eye].pose = eyePoses[eye];
                            projectionViews[eye].fov = eyeFovs[eye];
                        }
                        projectionViews[eye].subImage.swapchain = m_eyeSwapchains[eye].handle;
                        projectionViews[eye].subImage.imageRect.offset = {0, 0};
                        projectionViews[eye].subImage.imageRect.extent = {m_eyeSwapchains[eye].width, m_eyeSwapchains[eye].height};
                        projectionViews[eye].subImage.imageArrayIndex = 0;
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
                            if (eyePairIds[0] == 1 || (eyePairIds[0] % 300) == 0) {
                                Log("OpenXRManager: AER frame submitted. left=%llu right=%llu pair=(%llu,%llu) fresh=%d shouldRender=%d debugEye=%d synth=%d depth=%d\n",
                                    static_cast<unsigned long long>(eyeSerials[0]),
                                    static_cast<unsigned long long>(eyeSerials[1]),
                                    static_cast<unsigned long long>(eyePairIds[0]),
                                    static_cast<unsigned long long>(eyePairIds[1]),
                                    submitSerial != m_lastSubmittedSerial ? 1 : 0,
                                    frameState.shouldRender ? 1 : 0,
                                    GetAERDebugEye(),
                                    useInterpolatedPair ? 1 : 0,
                                    useDepthLayer ? 1 : 0);
                            }
                            m_lastSubmittedSerial = submitSerial;
                            m_lastSubmittedPairId = eyePairIds[0];
                            if (useInterpolatedPair) {
                                std::lock_guard<std::mutex> lock(m_presentMutex);
                                if (m_interpolatedPairId == eyePairIds[0]) {
                                    m_interpolatedPairId = 0;
                                    m_interpolatedEyeViewsValid[0] = false;
                                    m_interpolatedEyeViewsValid[1] = false;
                                }
                            }
                            eyeSources[0]->Release();
                            eyeSources[1]->Release();
                            if (generatedSources[0]) generatedSources[0]->Release();
                            if (generatedSources[1]) generatedSources[1]->Release();
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

                        projectionViews[eye].pose = monoPoses[eye];
                        projectionViews[eye].fov = monoFovs[eye];
                        if (menuRectActive) {
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

void OpenXRManager::OnPresent(IDXGISwapChain* swapChain) {
    if (!swapChain) return;

    uint64_t s_presentCount = m_presentCount.fetch_add(1, std::memory_order_relaxed) + 1;
    const bool monoEnabled = m_monoSubmitEnabled.load(std::memory_order_relaxed);
    const bool aerEnabled = monoEnabled && IsAERSubmitEnabled();
    const bool menuRectActive = (GetMenuRectMode() != 0) || (GetMenuMode() != 0);
    const bool syncSequential = aerEnabled && GetSyncSequential() != 0;
    const int scheduledEye = aerEnabled ? m_renderEyeIndex.load(std::memory_order_relaxed) : 0;
    const int presentEye = aerEnabled ? GetRenderedCameraEye() : 0;
    const bool aerWarmupFrame = aerEnabled && m_aerWarmupRemaining > 0;

    if (aerEnabled && scheduledEye != presentEye && (s_presentCount % 300) == 1) {
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
        if (m_syncedPairId == 1 || (m_syncedPairId % 300) == 0) {
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
            capturedFov = ApplyForcedProjectionFov(sourceFov, fovWidth, fovHeight);
            hasCapturedView = true;

            // Render-pose submit (AER V2): replace the present-time pose with the
            // exact head pose this eye's frame was rendered with (captured by the
            // camera hook), so the compositor time-warps the older 1/2-rate eye
            // forward to display time instead of showing it stale. Keep the runtime
            // per-eye offset for the correct stereo baseline; only head pos+ori
            // carry the per-eye render timestamp. This is the fix for left-eye
            // judder on head turns.
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
                monoCapturedFovs[eye] = ApplyForcedProjectionFov(m_views[eye].fov, fovWidth, fovHeight);
                monoCapturedViews[eye] = true;
            }
        }
    }
    bool monoCaptureOk = false;
    if (monoEnabled && (!aerEnabled || menuRectActive) && backBuffer) {
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

            const bool pairReady =
                m_pendingEyeFrames[0].pairId == presentPairId &&
                m_pendingEyeFrames[1].pairId == presentPairId &&
                m_pendingEyeFrames[0].serial != 0 &&
                m_pendingEyeFrames[1].serial != 0 &&
                m_pendingEyeFrames[0].hasView &&
                m_pendingEyeFrames[1].hasView;
            if (pairReady) {
                std::swap(m_previousCapturedEyeFrames[0], m_capturedEyeFrames[0]);
                std::swap(m_previousCapturedEyeFrames[1], m_capturedEyeFrames[1]);
                std::swap(m_capturedEyeFrames[0], m_pendingEyeFrames[0]);
                std::swap(m_capturedEyeFrames[1], m_pendingEyeFrames[1]);
                auto nameFrameRole = [](CapturedEyeFrame& frame, const wchar_t* role, int eye) {
                    SetD3DNamef(frame.texture, L"AERV2_%ls_eye%d_color_pair%llu_serial%llu", role, eye,
                        static_cast<unsigned long long>(frame.pairId),
                        static_cast<unsigned long long>(frame.serial));
                    SetD3DNamef(frame.opticalFlowTexture, L"AERV2_%ls_eye%d_ofinput_pair%llu_serial%llu", role, eye,
                        static_cast<unsigned long long>(frame.pairId),
                        static_cast<unsigned long long>(frame.serial));
                };
                nameFrameRole(m_previousCapturedEyeFrames[0], L"previous", 0);
                nameFrameRole(m_previousCapturedEyeFrames[1], L"previous", 1);
                nameFrameRole(m_capturedEyeFrames[0], L"current", 0);
                nameFrameRole(m_capturedEyeFrames[1], L"current", 1);
                nameFrameRole(m_pendingEyeFrames[0], L"pending_reuse", 0);
                nameFrameRole(m_pendingEyeFrames[1], L"pending_reuse", 1);
                if (m_interpolatedPairId != 0 &&
                    m_interpolatedPairId != presentPairId) {
                    if ((presentPairId % 300) == 0) {
                        Log("OpenXRManager: AER V2 stale synth dropped. stale=%llu current=%llu lastSubmitted=%llu\n",
                            static_cast<unsigned long long>(m_interpolatedPairId),
                            static_cast<unsigned long long>(presentPairId),
                            static_cast<unsigned long long>(m_lastSubmittedPairId));
                    }
                    m_interpolatedPairId = 0;
                    m_interpolatedEyeViewsValid[0] = false;
                    m_interpolatedEyeViewsValid[1] = false;
                }
                const bool synthSlotBusy = m_interpolatedPairId != 0;
                if (GetAERV2Enabled() != 0 && presentPairId == 1) {
                    Log("OpenXRManager: AER V2 optical-flow warmup active until pair=%llu\n",
                        static_cast<unsigned long long>(kAERV2FlowWarmupPairId));
                }
                if (GetAERV2Enabled() != 0 &&
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
                        flowPrevSource[eye]->AddRef();
                        flowCurrSource[eye]->AddRef();
                        flowPrev[eye]->AddRef();
                        flowCurr[eye]->AddRef();
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

    if (runAERV2Flow && m_opticalFlow) {
        bool convertedOk = true;
        for (int eye = 0; eye < 2; ++eye) {
            if (!m_opticalFlow->ConvertToInputTexture(flowPrevSource[eye], flowPrev[eye]) ||
                !m_opticalFlow->ConvertToInputTexture(flowCurrSource[eye], flowCurr[eye])) {
                convertedOk = false;
                break;
            }
        }
        if (!convertedOk) {
            Log("OpenXRManager: AER V2 input conversion failed. pair=%llu\n",
                static_cast<unsigned long long>(flowPairId));
            runAERV2Flow = false;
        }
    }
    if (!runAERV2Flow && presentLock.owns_lock()) {
        presentLock.unlock();
    }

    if (runAERV2Flow && m_opticalFlow) {
        const bool leftOk = m_opticalFlow->ExecuteFlow(flowPrev[0], flowCurr[0], 0);
        const bool rightOk = m_opticalFlow->ExecuteFlow(flowPrev[1], flowCurr[1], 1);
        bool leftSynth = false;
        bool rightSynth = false;
        if (leftOk) {
            leftSynth = m_opticalFlow->SynthesizeMidpoint(flowPrev[0], flowCurr[0], 0);
        }
        if (rightOk) {
            rightSynth = m_opticalFlow->SynthesizeMidpoint(flowPrev[1], flowCurr[1], 1);
        }
        if (leftSynth && rightSynth) {
            m_interpolatedPairId = flowPairId;
            for (int eye = 0; eye < 2; ++eye) {
                m_interpolatedEyePoses[eye] = ExtrapolatePose(
                    m_previousCapturedEyeFrames[eye].pose,
                    m_capturedEyeFrames[eye].pose,
                    kAERV2FrameGenPoseT);
                m_interpolatedEyeFovs[eye] = m_capturedEyeFrames[eye].fov;
                m_interpolatedEyeViewsValid[eye] =
                    m_previousCapturedEyeFrames[eye].hasView &&
                    m_capturedEyeFrames[eye].hasView;
            }
        } else {
            m_interpolatedPairId = 0;
            m_interpolatedEyeViewsValid[0] = false;
            m_interpolatedEyeViewsValid[1] = false;
        }
        if (presentLock.owns_lock()) {
            presentLock.unlock();
        }
        if ((flowPairId % 300) == 0 || !leftOk || !rightOk || !leftSynth || !rightSynth) {
            Log("OpenXRManager: AER V2 flow/synth. pair=%llu flowL=%d flowR=%d synthL=%d synthR=%d\n",
                static_cast<unsigned long long>(flowPairId),
                leftOk ? 1 : 0,
                rightOk ? 1 : 0,
                leftSynth ? 1 : 0,
                rightSynth ? 1 : 0);
        }
    }

    for (int eye = 0; eye < 2; ++eye) {
        if (flowPrevSource[eye]) flowPrevSource[eye]->Release();
        if (flowCurrSource[eye]) flowCurrSource[eye]->Release();
        if (flowPrev[eye]) flowPrev[eye]->Release();
        if (flowCurr[eye]) flowCurr[eye]->Release();
    }

    if (syncSequential && presentEye == 1 && !aerWarmupFrame) {
        latchSyncedSequentialPair();
    }

    if (aerEnabled) {
        if (aerWarmupFrame) {
            --m_aerWarmupRemaining;
            m_renderEyeIndex.store(presentEye, std::memory_order_relaxed);
        } else if (presentEye == 1) {
            m_aerWarmupRemaining = GetAERWarmupFrames();
            m_renderEyeIndex.store(0, std::memory_order_relaxed);
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
    // NOTE: AER mode submits pairs asynchronously at half-rate, so locking
    // the game thread here causes severe frame pacing issues and 1-FPS drops.
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

void OpenXRManager::Shutdown() {
    std::lock_guard<std::mutex> initLock(m_initMutex);
    m_stopFrameThread.store(true, std::memory_order_relaxed);
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
    if (m_opticalFlow) {
        m_opticalFlow->Shutdown();
    }
    if (m_monoCapturedFrame.texture) {
        m_monoCapturedFrame.texture->Release();
        m_monoCapturedFrame.texture = nullptr;
    }
    if (m_depthSnapshot) {
        m_depthSnapshot->Release();
        m_depthSnapshot = nullptr;
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
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.serial = 0;
        frame.pairId = 0;
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
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.serial = 0;
        frame.pairId = 0;
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
        frame.width = 0;
        frame.height = 0;
        frame.format = 0;
        frame.serial = 0;
        frame.pairId = 0;
        frame.pose = {};
        frame.pose.orientation.w = 1.0f;
        frame.fov = {};
        frame.hasView = false;
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
    m_interpolatedEyeViewsValid[0] = false;
    m_interpolatedEyeViewsValid[1] = false;
    Log("OpenXRManager: Shutdown complete.\n");
}
