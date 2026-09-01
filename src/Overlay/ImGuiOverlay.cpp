#include "Overlay/ImGuiOverlay.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Runtimes/OpenXRManager.hpp"

#include <algorithm>
#include <cfloat>
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>

#include <imgui.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>

#include "im3d.h"
#include "Overlay/OverlayInternal.hpp"

extern volatile int g_verboseLog; // per-frame log spam toggle (default off)


extern void Log(const char* fmt, ...);

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

// located (HMD-injected) game-world camera quaternion, defined in vr_core.cpp. Declared at GLOBAL
// scope (NOT inside the anonymous namespace below) so it keeps external linkage.
extern volatile float g_lastLocateQuat[4];

// ---- sync_stereo tunables (stereo/sync_stereo.cpp, all dllexported) ------------------------
// Live values, not part of LiveControlsUiState: the engine hooks read these globals directly
// every frame, so a change takes effect on the next one and the Save button does not apply.
extern "C" int   CyberpunkVR_StereoModuleEnable;   // vr_core.cpp: did we install at all
extern "C" int   CyberpunkVR_StereoModuleLoaded;
extern "C" int32_t CyberpunkVR_StereoLog;
// The right eye: capture the VRCAM colour, and send it to the HMD.
extern "C" int      CyberpunkVR_StereoSubmit;              // openxr_frameloop.cpp
extern "C" int32_t  CyberpunkVR_StereoEyeCapture;
extern "C" uint32_t CyberpunkVR_StereoEyeMaxAgeMs;
extern "C" uint32_t CyberpunkVR_DebugVrcamEyeAgeMs;        // 0xFFFFFFFF = never produced
extern "C" unsigned long long CyberpunkVR_DebugStereoEyeSubmits;
extern "C" int32_t CyberpunkVR_StableCopy;
extern "C" int32_t CyberpunkVR_StableFromTonemap;
extern "C" uint64_t CyberpunkVR_DebugStableCopies;
extern "C" uint64_t CyberpunkVR_DebugStableSkips;
extern "C" int32_t CyberpunkVR_VrcamDlss;
extern "C" int32_t CyberpunkVR_ForceVrcamCam;
extern "C" uint32_t CyberpunkVR_VrcamEnabled;
extern "C" void        CyberpunkVR_SetVrcamEnabled(uint32_t on);
extern "C" const char* CyberpunkVR_VrcamComponentName();
extern "C" const char* CyberpunkVR_VrcamCameraName();
extern "C" uint32_t CyberpunkVR_MirrorOutput;
extern "C" uint64_t CyberpunkVR_DebugVrcamNodeHits;   // 0 => the second view never dispatched
extern "C" uint64_t CyberpunkVR_DebugMirrorRtvHits;
extern "C" int CyberpunkVR_IsVrcamViewActive();
extern "C" float CyberpunkVR_DebugMainProjYY;
extern "C" float CyberpunkVR_DebugMainCamFov;
extern "C" float CyberpunkVR_MainAdsZoomFactor;
// 1 = everything the overlay projects (the sight mark above all) follows the weapon's ADS
// magnification. 0 = the old behaviour, projected from the lens FOV alone.
extern "C" float CyberpunkVR_DebugVrcamWantFov;
extern "C" float CyberpunkVR_DebugVrcamBaseFov;
// per-node CPU profiler
extern "C" int32_t  CyberpunkVR_ProfEnable;
extern "C" double   CyberpunkVR_ProfFrameMs;
extern "C" double   CyberpunkVR_ProfDispMainMs;
extern "C" double   CyberpunkVR_ProfDispVrcamMs;
extern "C" uint32_t CyberpunkVR_ProfDispMainNodes;
extern "C" uint32_t CyberpunkVR_ProfDispVrcamNodes;
extern "C" void     CyberpunkVR_ProfDumpNodes();
extern "C" int      CyberpunkVR_ProfSnapshotNodes(uint32_t* rva, double* msv, double* msm,
                                                  uint32_t* cv, uint32_t* cm, int maxn);
extern "C" const char* CyberpunkVR_ProfNodeName(uint32_t rva);
// view-identity counters (vr_core.cpp, published by the stereo node hook)
extern "C" uint64_t CyberpunkVR_DebugViewKeyMainNodes;
extern "C" uint64_t CyberpunkVR_DebugViewKeyOtherNodes;

// Defined in vr_core.cpp -- same DLL. Must sit OUTSIDE the anonymous namespace below or
// it gets internal linkage and never resolves.
extern volatile int32_t g_lastLocatePosFP[3];
extern "C" float CyberpunkVRPort_HalfIpd();

// ONE NAMED NAMESPACE WHERE THERE WAS AN ANONYMOUS ONE, and this is the change that lets the overlay
// split at all -- the same prerequisite SyncStereo.cpp and SwapChain.cpp each needed.
//
// Everything from here to the close had INTERNAL linkage, so no other file could name one symbol of it.
// `overlay` rather than nothing: the names get external linkage and can cross a file boundary, while
// staying out of the global namespace. A `using namespace overlay;` after the close keeps every
// reference in the public entry points below resolving exactly as it did.
//
// Behaviourally a no-op.
namespace overlay {
struct FrameContext {
    ID3D12CommandAllocator* allocator = nullptr;
    ID3D12Resource* renderTarget = nullptr;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv{};
    // The overlay fence value that last used this slot's allocator. Meaningless while every frame
    // drained; the whole point of the pacing below is that it now is not.
    UINT64 fenceValue = 0;
};

ID3D12Device* g_device = nullptr;
ID3D12CommandQueue* g_queue = nullptr;
IDXGISwapChain3* g_swapChain3 = nullptr;
ID3D12DescriptorHeap* g_rtvHeap = nullptr;
ID3D12DescriptorHeap* g_srvHeap = nullptr;
ID3D12GraphicsCommandList* g_cmdList = nullptr;
ID3D12Fence* g_fence = nullptr;
HANDLE g_fenceEvent = nullptr;
UINT64 g_fenceValue = 0;
// The last submission's value, and the guard window. See the pacing block below.
UINT64 g_previousOverlayFenceValue = 0;
std::atomic<ULONGLONG> g_loadGuardUntilMs{0};
std::atomic<bool> g_loadGuardEngaged{false};
// How long the full drain comes back for after a resource-churn event. Deliberately generous: the
// hangs it prevents landed within a few frames of the signal but the churn continues past it, and the
// frames it slows are loading-screen frames either way.
constexpr ULONGLONG kLoadGuardMs = 5000;
UINT g_rtvDescriptorSize = 0;
UINT g_frameCount = 0;
DXGI_FORMAT g_rtvFormat = DXGI_FORMAT_UNKNOWN;
// THE FORMAT THE IMGUI PIPELINE STATE WAS BUILT FOR, which is not the same fact as
// g_rtvFormat: that one follows the swapchain, while the PSO is created once at Init and keeps
// whatever the format was then. Binding a render target whose format disagrees with the PSO's is
// a lie to the driver of exactly the kind that has hung this device before, so the second-eye
// pass validates against THIS value.
DXGI_FORMAT g_imguiPsoFormat = DXGI_FORMAT_UNKNOWN;
// The list the world-projected markers draw into, captured while the frame is still open.
ImDrawList* g_bgDrawList = nullptr;
// A small ring of RTVs for targets that are not the swapchain (the second eye). Round-robin
// rather than one, so a descriptor is never rewritten while a frame that referenced it is still
// in flight -- the same reason ColorBlit keeps a ring.
ID3D12DescriptorHeap* g_eyeRtvHeap = nullptr;
UINT g_eyeRtvStride = 0;
uint32_t g_eyeRtvSlot = 0;
std::vector<FrameContext> g_frames;

HWND g_hwnd = nullptr;
WNDPROC g_originalWndProc = nullptr;
bool g_imguiInitialized = false;
bool g_menuVisible = false;
bool g_drawHandLocator = false;
bool g_drawHandProxy3D = false;
bool g_drawHandDebugAxes = false;
float g_handLocatorScale = 1.0f;
// Long aim ray down each hand's forward (the visible "where I'm pointing / where the
// gun barrel looks" line). Reuses the same head-relative projection as the hand proxy.
bool g_drawAimRay = true;
float g_aimRayLenM = 8.0f;
// EXACT barrel crosshair: project the GAME muzzle forward (plugin publishes it to shared[24..26])
// through the located game camera (= the eye view) -> a dot exactly where the bullet goes.
// The compact ADS panel: OFF by default -- an instrument, not a HUD -- with its position given in
// normalised backbuffer coordinates so it lands inside the lens-visible area rather than at the
// desktop mirror's outer corner.
bool g_showCompactAdsTelemetry = false;
float g_compactAdsTelemetryX = 0.57f;
float g_compactAdsTelemetryY = 0.30f;
bool g_drawBarrelCross = true;   // g_lastLocateQuat is declared above, at global scope

// Moved to src/Overlay/OverlayProjection.cpp: projecting a world or head-space point onto the overlay, and the Im3d bridge.

// Moved to src/Overlay/OverlayDebugDraw.cpp: the hand locator and the barrel crosshair.

template <typename T>
void SafeRelease(T*& value) {
    if (value) {
        value->Release();
        value = nullptr;
    }
}

// Moved to src/Overlay/OverlayPanels.cpp: every control panel the headset overlay draws.

// Defined below, with the rest of the pacing: this is the one caller that sits above it.
bool DrainOverlayGpu(DWORD timeoutMs, const char* reason);

void ReleaseRenderTargets() {
    // ORDINARY FRAMES NO LONGER DRAIN, so this is the one place that must: backbuffers, allocators,
    // the command list and ImGui's upload resources are about to be released, and the GPU may still be
    // reading them. Bounded -- a removed device must not hang the teardown.
    if (g_queue && g_fence && (g_cmdList || !g_frames.empty())) {
        if (!DrainOverlayGpu(2000, "resource teardown")) {
            Log("Overlay teardown drain timed out; continuing (the device may be gone).\n");
        }
    }
    for (FrameContext& frame : g_frames) {
        SafeRelease(frame.renderTarget);
        SafeRelease(frame.allocator);
    }
    g_frames.clear();
    g_previousOverlayFenceValue = 0;
    SafeRelease(g_rtvHeap);
    SafeRelease(g_cmdList);
    SafeRelease(g_swapChain3);
    g_frameCount = 0;
    g_rtvFormat = DXGI_FORMAT_UNKNOWN;
}

void ShutdownOverlay() {
    ReleaseRenderTargets();
    if (g_imguiInitialized) {
        ImGui_ImplDX12_Shutdown();
        ImGui_ImplWin32_Shutdown();
        ImGui::DestroyContext();
        g_imguiInitialized = false;
    }
    SafeRelease(g_srvHeap);
    SafeRelease(g_eyeRtvHeap);
    g_eyeRtvStride = 0;
    g_eyeRtvSlot = 0;
    g_imguiPsoFormat = DXGI_FORMAT_UNKNOWN;
    g_bgDrawList = nullptr;
    SafeRelease(g_fence);
    if (g_fenceEvent) {
        CloseHandle(g_fenceEvent);
        g_fenceEvent = nullptr;
    }
}

// ================================================================================================
// OVERLAY PACING (satyaloka93, psvr2-tweaks 980f4406 + ce8d36f1; his measurements)
//
//     drain every frame        43.8 fps median present   stable
//     previous-overlay fence   62.2 fps                  EIGHT DXGI_ERROR_DEVICE_HUNG
//     the fence plus a guard   70.3 fps                  stable
//
// The overlay records a small ImGui command list on CYBERPUNK'S OWN Present queue. Signalling a fence
// there and waiting for it does not wait for the overlay -- a D3D12 queue signal completes only after
// all earlier work on the queue, so the wait drains the game's whole frame. CPU submission stops while
// the GPU finishes, then the GPU idles while the CPU builds the next frame: that lockstep is why low
// FPS, low CPU load and low GPU load can all be true at the same time.
//
// What replaces it is not "no wait" -- D3D12's ownership rules are real. It is the EXACT waits: the
// command allocator for this backbuffer slot, and Dear ImGui's per-frame upload buffers. Both are
// covered by waiting for the PREVIOUS OVERLAY SUBMISSION, because fences are monotonic: if N-1 has
// completed then so has everything before it, and the ImGui ring only comes round every few frames.
//
// 0 = drain every frame, exactly what shipped. 2 = pacing with no guard -- the arm that hung, kept
// because it is how the guard was proven necessary. 3 = pacing plus the guard (default).
// ================================================================================================
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OverlayPacing = 3;
// Diagnostics: how long the two waits actually cost, and how often the guard was open.
extern "C" __declspec(dllexport) double   CyberpunkVR_DebugOverlayWaitMs = 0.0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOverlayDrains = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOverlayWaitTimeouts = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOverlayGuardArms = 0;

// A BOUNDED wait on an already-signalled value. Bounded rather than INFINITE on purpose: this runs on
// the Present thread, and a lost fence must degrade into a dropped overlay frame, not a hung game.
bool WaitForOverlayFence(UINT64 target, DWORD timeoutMs, const char* reason) {
    if (!g_fence || !g_fenceEvent || target == 0) return true;
    if (g_fence->GetCompletedValue() >= target) return true;
    const LARGE_INTEGER t0 = [] { LARGE_INTEGER v{}; QueryPerformanceCounter(&v); return v; }();
    if (FAILED(g_fence->SetEventOnCompletion(target, g_fenceEvent))) return false;
    const DWORD r = WaitForSingleObject(g_fenceEvent, timeoutMs);
    LARGE_INTEGER t1{}, freq{};
    QueryPerformanceCounter(&t1);
    QueryPerformanceFrequency(&freq);
    if (freq.QuadPart > 0) {
        CyberpunkVR_DebugOverlayWaitMs =
            static_cast<double>(t1.QuadPart - t0.QuadPart) * 1000.0 / static_cast<double>(freq.QuadPart);
    }
    if (r != WAIT_OBJECT_0) {
        ++CyberpunkVR_DebugOverlayWaitTimeouts;
        static uint64_t s_logged = 0;
        if (++s_logged <= 4) {
            Log("Overlay fence wait timed out (%s, %lu ms) -- continuing.\n", reason, timeoutMs);
        }
        return false;
    }
    return true;
}

// The old behaviour, kept for the places that genuinely need the queue empty: teardown, resize, and
// the guard window. Signals a fresh value and waits for it, which is what drains everything earlier.
bool DrainOverlayGpu(DWORD timeoutMs, const char* reason) {
    if (!g_queue || !g_fence || !g_fenceEvent) return true;
    const UINT64 target = ++g_fenceValue;
    if (FAILED(g_queue->Signal(g_fence, target))) return false;
    ++CyberpunkVR_DebugOverlayDrains;
    return WaitForOverlayFence(target, timeoutMs, reason);
}

// Whether this submission must be followed by a full drain: always in mode 0, and in mode 3 only while
// the churn window is open. Logs the two edges rather than once a frame.
bool ShouldDrainThisFrame() {
    const int mode = CyberpunkVR_OverlayPacing;
    if (mode == 0) return true;
    if (mode != 3) return false;
    const bool active = GetTickCount64() < g_loadGuardUntilMs.load(std::memory_order_relaxed);
    if (active != g_loadGuardEngaged.exchange(active, std::memory_order_relaxed)) {
        Log("Overlay load guard %s.\n",
            active ? "engaged -- full drain" : "released -- back to previous-overlay pacing");
    }
    return active;
}

bool EnsureSwapchainResources(IDXGISwapChain* swapChain) {
    if (!swapChain || !g_device || !g_queue || !g_hwnd) return false;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc)) || desc.BufferCount == 0) {
        return false;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    if (FAILED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3))) || !swapChain3) {
        return false;
    }

    const bool needsResources = !g_swapChain3 || g_frameCount != desc.BufferCount || g_rtvFormat != desc.BufferDesc.Format;
    if (!needsResources) {
        swapChain3->Release();
        return true;
    }

    ReleaseRenderTargets();
    g_swapChain3 = swapChain3;
    g_frameCount = desc.BufferCount;
    g_rtvFormat = desc.BufferDesc.Format;

    D3D12_DESCRIPTOR_HEAP_DESC rtvDesc{};
    rtvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    rtvDesc.NumDescriptors = g_frameCount;
    if (FAILED(g_device->CreateDescriptorHeap(&rtvDesc, IID_PPV_ARGS(&g_rtvHeap)))) {
        ReleaseRenderTargets();
        return false;
    }
    g_rtvDescriptorSize = g_device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);

    g_frames.resize(g_frameCount);
    D3D12_CPU_DESCRIPTOR_HANDLE rtv = g_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    for (UINT i = 0; i < g_frameCount; ++i) {
        FrameContext& frame = g_frames[i];
        if (FAILED(g_device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&frame.allocator)))) {
            ReleaseRenderTargets();
            return false;
        }
        if (FAILED(swapChain->GetBuffer(i, IID_PPV_ARGS(&frame.renderTarget)))) {
            ReleaseRenderTargets();
            return false;
        }
        frame.rtv = rtv;
        g_device->CreateRenderTargetView(frame.renderTarget, nullptr, frame.rtv);
        rtv.ptr += g_rtvDescriptorSize;
    }

    if (FAILED(g_device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, g_frames[0].allocator, nullptr, IID_PPV_ARGS(&g_cmdList)))) {
        ReleaseRenderTargets();
        return false;
    }
    g_cmdList->SetName(L"CVR_imgui_command_list");
    g_cmdList->Close();
    return true;
}

bool EnsureImGui(IDXGISwapChain* swapChain) {
    if (!swapChain || !g_device || !g_queue || !g_hwnd) return false;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        return false;
    }

    if (!g_imguiInitialized) {
        D3D12_DESCRIPTOR_HEAP_DESC srvDesc{};
        srvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        srvDesc.NumDescriptors = 1;
        srvDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(g_device->CreateDescriptorHeap(&srvDesc, IID_PPV_ARGS(&g_srvHeap)))) {
            return false;
        }

        IMGUI_CHECKVERSION();
        ImGui::CreateContext();
        ImGuiIO& io = ImGui::GetIO();
        io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
        io.MouseDrawCursor = true;
        io.IniFilename = nullptr;
        io.FontGlobalScale = 1.35f;
        ImGui::StyleColorsDark();
        ImGui::GetStyle().ScaleAllSizes(1.35f);

        if (!ImGui_ImplWin32_Init(g_hwnd)) {
            ShutdownOverlay();
            return false;
        }
        // TWICE the frames in flight, because the overlay is now rendered TWICE per frame -- once into
        // the backbuffer and once into the second eye -- and the backend takes one buffer set from
        // its ring per RenderDrawData call. At one set per swapchain buffer a set would come round
        // again within a frame and a half, while the second-eye pass is still recorded into the
        // capture list and executed later; doubling the ring puts three frames between reuses. The
        // cost is a few hundred KB of upload buffers, against a garbled menu that would appear only
        // under load and only in one eye.
        if (!ImGui_ImplDX12_Init(g_device, static_cast<int>(2 * std::max<UINT>(desc.BufferCount, 2)), desc.BufferDesc.Format, g_srvHeap,
                g_srvHeap->GetCPUDescriptorHandleForHeapStart(), g_srvHeap->GetGPUDescriptorHandleForHeapStart())) {
            ShutdownOverlay();
            return false;
        }

        if (FAILED(g_device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&g_fence)))) {
            ShutdownOverlay();
            return false;
        }
        g_fenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!g_fenceEvent) {
            ShutdownOverlay();
            return false;
        }

        g_imguiPsoFormat = desc.BufferDesc.Format;
        g_imguiInitialized = true;
        if (g_verboseLog) Log("ImGui overlay initialized. Toggle with F10 or Insert.\n");
    }

    return EnsureSwapchainResources(swapChain);
}

extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();

bool IsBlockedInputMessage(UINT msg) {
    switch (msg) {
    case WM_INPUT:
    case WM_INPUT_DEVICE_CHANGE:
    case WM_MOUSEMOVE:
    case WM_NCMOUSEMOVE:
    case WM_LBUTTONDOWN:
    case WM_LBUTTONUP:
    case WM_RBUTTONDOWN:
    case WM_RBUTTONUP:
    case WM_MBUTTONDOWN:
    case WM_MBUTTONUP:
    case WM_MOUSEWHEEL:
    case WM_MOUSEHWHEEL:
    case WM_KEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYDOWN:
    case WM_SYSKEYUP:
    case WM_CHAR:
        return true;
    default:
        return false;
    }
}

LRESULT CALLBACK OverlayWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static int totalMsgCount = 0;
    if (g_verboseLog && totalMsgCount++ % 5000 == 0) {
        Log("OverlayWndProc: msg=%u, hwnd=%p, count=%d\n", msg, hwnd, totalMsgCount);
    }

    // 1. Scale mouse coordinates FIRST so ImGui and game receive the scaled input
    if (msg == WM_MOUSEMOVE || msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP || msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP || msg == WM_MBUTTONDOWN || msg == WM_MBUTTONUP) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            if (GetClientRect(hwnd, &rect)) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                int x = (short)LOWORD(lParam);
                int y = (short)HIWORD(lParam);
                int oldX = x;
                int oldY = y;
                
                if (winWidth > 0 && winHeight > 0 && (winWidth != static_cast<int>(virtualWidth) || winHeight != static_cast<int>(virtualHeight))) {
                    x = (x * static_cast<int>(virtualWidth)) / winWidth;
                    y = (y * static_cast<int>(virtualHeight)) / winHeight;
                    
                    lParam = MAKELPARAM(static_cast<WORD>(x), static_cast<WORD>(y));
                }

                static int mouseLogCount = 0;
                if (g_verboseLog && mouseLogCount++ % 100 == 0) {
                    Log("OverlayWndProc (scaled): msg=%u physical=(%d,%d) -> scaled=(%d,%d) win=%dx%d virt=%ux%u g_menuVisible=%d\n",
                        msg, oldX, oldY, x, y, winWidth, winHeight, virtualWidth, virtualHeight, g_menuVisible ? 1 : 0);
                }
            }
        }
    }

    // 2. Handle menu toggle
    if ((msg == WM_KEYUP || msg == WM_SYSKEYUP) && (wParam == VK_F10 || wParam == VK_INSERT)) {
        g_menuVisible = !g_menuVisible;
        if (g_menuVisible) {
            ReleaseGameMouseCapture();
        }
        if (g_verboseLog) Log("ImGui overlay %s.\n", g_menuVisible ? "shown" : "hidden");
        return 0;
    }

    // 3. Feed to ImGui if visible
    if (g_menuVisible) {
        if (g_imguiInitialized && ImGui_ImplWin32_WndProcHandler(hwnd, msg, wParam, lParam)) {
            return 1;
        }
        if (IsBlockedInputMessage(msg)) {
            return 0;
        }
    }

    return g_originalWndProc ? CallWindowProcA(g_originalWndProc, hwnd, msg, wParam, lParam) : DefWindowProcA(hwnd, msg, wParam, lParam);
}
}  // namespace overlay
using namespace overlay;

void OverlaySetDeviceAndQueue(ID3D12Device* device, ID3D12CommandQueue* queue) {
    if (device == g_device && queue == g_queue) return;

    ShutdownOverlay();
    SafeRelease(g_device);
    SafeRelease(g_queue);
    if (device) {
        g_device = device;
        g_device->AddRef();
    }
    if (queue) {
        g_queue = queue;
        g_queue->AddRef();
    }
}

void OverlaySetWindow(HWND hwnd) {
    if (g_verboseLog) Log("OverlaySetWindow called: hwnd=%p (previous g_hwnd=%p)\n", hwnd, g_hwnd);
    if (!hwnd || hwnd == g_hwnd) return;

    if (g_hwnd && g_originalWndProc) {
        SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(g_originalWndProc));
        if (g_verboseLog) Log("OverlaySetWindow: Restored original WndProc on old hwnd %p\n", g_hwnd);
    }
    g_hwnd = hwnd;
    g_originalWndProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(OverlayWndProc)));
    if (g_verboseLog) Log("OverlaySetWindow: Subclassed hwnd %p, original WndProc=%p, new WndProc=%p\n", g_hwnd, g_originalWndProc, OverlayWndProc);
}

void OverlayRender(IDXGISwapChain* swapChain) {
    if (!EnsureImGui(swapChain)) return;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) return;

    const UINT frameIndex = g_swapChain3 ? g_swapChain3->GetCurrentBackBufferIndex() : 0;
    if (frameIndex >= g_frames.size()) return;
    FrameContext& frame = g_frames[frameIndex];
    if (!frame.renderTarget || !frame.allocator || !g_cmdList) return;

    // OWNERSHIP, BEFORE ANYTHING IS RECORDED. This slot's allocator is about to be reset and ImGui is
    // about to write the next upload buffer in its ring; both are still the GPU's until the submission
    // that used them has completed. Waiting for the PREVIOUS submission covers both, fences being
    // monotonic. Bounded: a missed wait costs one overlay frame, never the game.
    {
        const UINT64 owned = (frame.fenceValue > g_previousOverlayFenceValue)
                           ? frame.fenceValue : g_previousOverlayFenceValue;
        WaitForOverlayFence(owned, 100, "overlay frame ownership");
    }

    ImGui_ImplDX12_NewFrame();
    ImGui_ImplWin32_NewFrame();

    ImGuiIO& io = ImGui::GetIO();
    float backbufferWidth = static_cast<float>(desc.BufferDesc.Width);
    float backbufferHeight = static_cast<float>(desc.BufferDesc.Height);
    if ((backbufferWidth <= 1.0f || backbufferHeight <= 1.0f) && frame.renderTarget) {
        const D3D12_RESOURCE_DESC resourceDesc = frame.renderTarget->GetDesc();
        backbufferWidth = static_cast<float>(resourceDesc.Width);
        backbufferHeight = static_cast<float>(resourceDesc.Height);
    }
    if (backbufferWidth > 1.0f && backbufferHeight > 1.0f) {
        io.DisplaySize = ImVec2(backbufferWidth, backbufferHeight);
        io.DisplayFramebufferScale = ImVec2(1.0f, 1.0f);
    }
    if (g_menuVisible) {
        ReleaseGameMouseCapture();
        UpdateImGuiMouseFromCursor(desc.OutputWindow, backbufferWidth, backbufferHeight);
    }

    ImGui::GetIO().MouseDrawCursor = g_menuVisible;

    ImGui::NewFrame();

    DrawHandLocatorOverlay();
    DrawBarrelCrosshair();
    DrawCompactAdsCameraTelemetry();

    LiveControlsUiState state{};
    GetLiveControlsUiState(&state);

    bool changed = false;
    if (g_menuVisible) {
        const ImVec2 display = ImGui::GetIO().DisplaySize;
        const ImVec2 menuSize(std::min(1000.0f, display.x * 0.58f), std::min(1180.0f, display.y * 0.64f));
        ImGui::SetNextWindowSize(menuSize, ImGuiCond_Appearing);
        ImGui::SetNextWindowPos(ImVec2((display.x - menuSize.x) * 0.5f, (display.y - menuSize.y) * 0.5f), ImGuiCond_Appearing);
        ImGui::Begin("CyberpunkVRPort Controls", &g_menuVisible, ImGuiWindowFlags_NoCollapse);
        ImGui::TextUnformatted("F10 / Insert: toggle menu");
        ImGui::Separator();
        changed = DrawLiveControls(state);
        ImGui::End();
    }

    if (changed) {
        SetLiveControlsUiState(&state, 1);
    }

    // Captured BEFORE Render(), so the second-eye pass never calls an ImGui function after the
    // frame is closed: this is the list it must skip, and it is the only thing it needs from the
    // context besides the draw data itself.
    g_bgDrawList = ImGui::GetBackgroundDrawList();

    ImGui::Render();

    frame.allocator->Reset();
    g_cmdList->Reset(frame.allocator, nullptr);

    D3D12_RESOURCE_BARRIER toRt{};
    toRt.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toRt.Transition.pResource = frame.renderTarget;
    toRt.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    toRt.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toRt.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_cmdList->ResourceBarrier(1, &toRt);

    g_cmdList->OMSetRenderTargets(1, &frame.rtv, FALSE, nullptr);
    ID3D12DescriptorHeap* heaps[] = {g_srvHeap};
    g_cmdList->SetDescriptorHeaps(1, heaps);
    ImGui_ImplDX12_RenderDrawData(ImGui::GetDrawData(), g_cmdList);

    D3D12_RESOURCE_BARRIER toPresent{};
    toPresent.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toPresent.Transition.pResource = frame.renderTarget;
    toPresent.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toPresent.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    toPresent.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_cmdList->ResourceBarrier(1, &toPresent);

    g_cmdList->Close();
    ID3D12CommandList* lists[] = {g_cmdList};
    g_queue->ExecuteCommandLists(1, lists);

    // SIGNAL, DO NOT WAIT. The value is remembered twice: on this backbuffer slot, whose allocator we
    // will reuse, and as "the previous submission", which is what the next frame waits for before it
    // records anything. See the pacing block above for why that one wait covers both this allocator
    // and ImGui's upload ring.
    if (g_queue && g_fence) {
        const UINT64 submitted = ++g_fenceValue;
        if (SUCCEEDED(g_queue->Signal(g_fence, submitted))) {
            frame.fenceValue = submitted;
            g_previousOverlayFenceValue = submitted;
            // The full drain, only while the churn guard is open (or in mode 0). Every recorded device
            // hang on the branch this came from was at a save-load transition, and this is the window.
            if (ShouldDrainThisFrame()) {
                WaitForOverlayFence(submitted, 2000, "load guard");
            }
        }
    }
}

void OverlayArmLoadGuard(const char* reason) {
    (void)reason;
    // COUNTED, NOT LOGGED. One caller is the VRCAM component re-bind, which can fire often -- a
    // line per arm would be a flood, and the window's own edges are already logged by
    // ShouldDrainThisFrame. If this counter climbs steadily while "released" never appears then the
    // guard is permanently open and the pacing is buying nothing: that is the thing to look at.
    ++CyberpunkVR_DebugOverlayGuardArms;
    g_loadGuardUntilMs.store(GetTickCount64() + kLoadGuardMs, std::memory_order_relaxed);
}

void OverlayInvalidateSwapchainResources() {
    // The backbuffers are going away, so the churn window applies to the frames after the rebuild too:
    // ReleaseRenderTargets drains once, the guard covers what follows.
    OverlayArmLoadGuard("swapchain invalidate");
    ReleaseRenderTargets();
}

bool OverlayIsVisible() {
    return g_menuVisible;
}

// ================================================================================================
// THE OVERLAY IN THE SECOND EYE
//
// Eye 0 is MAIN's backbuffer and the overlay is drawn straight into it. Eye 1 is the VRCAM view --
// a texture the engine fills knowing nothing about us -- so everything ImGui draws was missing from
// that eye entirely, the F10 menu included. Two ways were available and this is the cheaper one by a
// wide margin: instead of rendering ImGui into a transparent layer and compositing that layer per
// eye (an extra render target, a clear and a blend pass), the SAME draw data is recorded a second
// time straight into the eye texture.
//
// Four facts make that legal, and all four were read out of the code and the log rather than assumed:
//
//   1. The game swapchain is R8G8B8A8_UNORM and the eye pool is R8G8B8A8_TYPELESS, so a UNORM render
//      target view over the eye texture matches the pipeline state ImGui built for the backbuffer.
//      No second backend, no PSO of our own, no conversion.
//   2. The eye texture is created from the presented description, so it is backbuffer-sized and the
//      draw data -- which is in backbuffer pixels and is NOT scaled -- maps one to one.
//   3. ImGui 1.90.9's DX12 backend advances its frame-resource ring INSIDE RenderDrawData, so the
//      second call of a frame writes a different vertex buffer than the first. No aliasing, and no
//      dependency on the first pass having finished.
//   4. Between the HUD composite and the transition back to COPY_SOURCE the eye slot rests in
//      RENDER_TARGET, which is where the caller records this -- so it needs no barrier of its own,
//      and asserts nothing about a resource it does not own.
//
// What is NOT recorded is the background draw list: the hand locator, the aim ray and the barrel
// cross are projected with one frustum, so their pixels are only true for the eye they were
// projected for. Reusing them here would put a second, flat copy at optical infinity beside the
// correct one -- and the barrel dot already has its own properly projected second-eye draw
// (ColorBlit::RecordDot with CyberpunkVR_BarrelDotNdcX2). The foreground list is kept, because after
// that move it holds exactly what IS screen-space: ImGui's software mouse cursor, which has to be in
// both eyes to be usable.
// ================================================================================================

extern "C" __declspec(dllexport) int      CyberpunkVR_OverlaySecondEye = 1;
extern "C" __declspec(dllexport) float    CyberpunkVR_OverlaySecondEyeDistM = 0.0f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOverlaySecondEyeDraws = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugOverlaySecondEyeSkips = 0;

namespace {

// Same family = same typeless parent, which is the only thing a render target view cares about. Kept
// to the formats this swapchain can actually be: 8-bit, 10-bit HDR, and 16-bit float. An unknown
// pairing is refused rather than guessed, because the failure mode of guessing is a removed device.
bool RtvFormatFits(DXGI_FORMAT resource, DXGI_FORMAT view) {
    auto family = [](DXGI_FORMAT f) -> int {
        switch (f) {
            case DXGI_FORMAT_R8G8B8A8_TYPELESS:
            case DXGI_FORMAT_R8G8B8A8_UNORM:
            case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:       return 1;
            case DXGI_FORMAT_B8G8R8A8_TYPELESS:
            case DXGI_FORMAT_B8G8R8A8_UNORM:
            case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:       return 2;
            case DXGI_FORMAT_R10G10B10A2_TYPELESS:
            case DXGI_FORMAT_R10G10B10A2_UNORM:         return 3;
            case DXGI_FORMAT_R16G16B16A16_TYPELESS:
            case DXGI_FORMAT_R16G16B16A16_FLOAT:        return 4;
            default:                                    return 0;
        }
    };
    const int a = family(resource), b = family(view);
    return a != 0 && a == b;
}

}  // namespace

bool OverlayRecordIntoTarget(ID3D12GraphicsCommandList* cmdList, ID3D12Resource* target,
                             float shiftPx) {
    using namespace overlay;
    if (!CyberpunkVR_OverlaySecondEye) return false;
    if (!cmdList || !target || !g_device || !g_srvHeap || !g_imguiInitialized) return false;
    if (g_imguiPsoFormat == DXGI_FORMAT_UNKNOWN) return false;

    ImDrawData* drawData = ImGui::GetDrawData();
    if (!drawData || !drawData->Valid || drawData->CmdListsCount <= 0) return false;

    const D3D12_RESOURCE_DESC desc = target->GetDesc();
    const uint32_t wantW = static_cast<uint32_t>(drawData->DisplaySize.x);
    const uint32_t wantH = static_cast<uint32_t>(drawData->DisplaySize.y);
    // REFUSED, NOT STRETCHED. The draw data carries no scale, so a size mismatch would render the
    // overlay at backbuffer pixel coordinates into a differently sized eye and clip it -- a menu with
    // its right half missing in one eye is worse than a menu in one eye.
    if (static_cast<uint32_t>(desc.Width) != wantW || desc.Height != wantH ||
        !RtvFormatFits(desc.Format, g_imguiPsoFormat)) {
        static bool s_told = false;
        if (!s_told) {
            s_told = true;
            Log("Overlay[eye]: refusing the second-eye pass -- target %ux%u fmt=%d against draw data "
                "%ux%u pso fmt=%d. Same size and same format family are required.\n",
                static_cast<unsigned>(desc.Width), static_cast<unsigned>(desc.Height),
                static_cast<int>(desc.Format), wantW, wantH, static_cast<int>(g_imguiPsoFormat));
        }
        ++CyberpunkVR_DebugOverlaySecondEyeSkips;
        return false;
    }

    if (!g_eyeRtvHeap) {
        D3D12_DESCRIPTOR_HEAP_DESC hd{};
        hd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
        hd.NumDescriptors = 4;
        if (FAILED(g_device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&g_eyeRtvHeap)))) {
            g_eyeRtvHeap = nullptr;
            ++CyberpunkVR_DebugOverlaySecondEyeSkips;
            return false;
        }
        g_eyeRtvStride = g_device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    }

    D3D12_CPU_DESCRIPTOR_HANDLE rtv = g_eyeRtvHeap->GetCPUDescriptorHandleForHeapStart();
    rtv.ptr += static_cast<SIZE_T>(g_eyeRtvSlot) * g_eyeRtvStride;
    g_eyeRtvSlot = (g_eyeRtvSlot + 1) % 4;

    D3D12_RENDER_TARGET_VIEW_DESC rtvDesc{};
    rtvDesc.Format = g_imguiPsoFormat;
    rtvDesc.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    g_device->CreateRenderTargetView(target, &rtvDesc, rtv);

    // THE TWO EDITS TO THE DRAW DATA, both undone before returning: the draw data is the context's,
    // and eye 0 has already consumed it this frame, but anything left changed here would be read by
    // the NEXT frame's overlay before NewFrame resets it.
    //
    // DisplayPos is how a flat panel is given a distance. The projection maps
    // [DisplayPos, DisplayPos + DisplaySize] onto the viewport, so adding to DisplayPos.x slides the
    // whole overlay LEFT -- which is where a panel nearer than infinity sits in the right eye. Clip
    // rectangles are taken relative to DisplayPos by the backend, so they follow it exactly.
    const ImVec2 savedPos = drawData->DisplayPos;
    drawData->DisplayPos.x += shiftPx;

    // And the background list is dropped for this pass. Erasing from the vector keeps its capacity,
    // so this allocates nothing; CmdListsCount is kept consistent with it because the backend reads
    // both.
    int bgIndex = -1;
    if (g_bgDrawList) {
        for (int i = 0; i < drawData->CmdLists.Size; ++i) {
            if (drawData->CmdLists[i] == g_bgDrawList) { bgIndex = i; break; }
        }
    }
    if (bgIndex >= 0) {
        drawData->CmdLists.erase(drawData->CmdLists.Data + bgIndex);
        drawData->CmdListsCount = drawData->CmdLists.Size;
    }

    const bool anything = drawData->CmdListsCount > 0;
    if (anything) {
        cmdList->OMSetRenderTargets(1, &rtv, FALSE, nullptr);
        ID3D12DescriptorHeap* heaps[] = {g_srvHeap};
        cmdList->SetDescriptorHeaps(1, heaps);
        ImGui_ImplDX12_RenderDrawData(drawData, cmdList);
    }

    if (bgIndex >= 0) {
        drawData->CmdLists.insert(drawData->CmdLists.Data + bgIndex, g_bgDrawList);
        drawData->CmdListsCount = drawData->CmdLists.Size;
    }
    drawData->DisplayPos = savedPos;

    if (anything) ++CyberpunkVR_DebugOverlaySecondEyeDraws;
    return anything;
}

