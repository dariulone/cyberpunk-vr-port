#include <MinHook.h>
#include "Hooks/RenderDocBridge.hpp"
#include "Hooks/SwapChainInternal.hpp"
#include <thread>
#include "Hooks/SwapChain.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Hooks/Ngx.hpp"
#include <cstdint>
#include <cstdio>
#include <unordered_map>
#include <unordered_set>
#include <mutex>
#include <atomic>
#include <wrl.h>

// Defined in Stereo/CommandListCensus.cpp: hooks the game-facing command-list vtable so the port markers
// land above a capture layer. Declared here because this is where the game-facing device is in hand.
extern "C" void RegisterGameFacingListVtable(ID3D12Device* device);

extern void Log(const char* fmt, ...);
extern volatile int g_verboseLog; // gate per-frame spam (ClipCursor / depth-diag)
extern "C" void PrepareStartupLiveControls();
// View identity from the render-graph dispatcher (vr_core.cpp): which view is recording
// on this thread. Used to pick MAIN's depth-stencil without relying on resolution.
extern "C" int CyberpunkVR_IsMainViewRecording();
extern "C" int CyberpunkVR_ViewKeyHookActive();
// Stereo per-node profiler (stereo/sync_stereo.cpp): ends the accounting frame, see the call
// site in the Present path.
extern "C" void CyberpunkVR_ProfPublish();
extern "C" UINT GetForcedSwapchainWidth();
extern "C" UINT GetForcedSwapchainHeight();
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();
extern "C" UINT GetForcedWindowWidth();
extern "C" UINT GetForcedWindowHeight();
extern "C" int GetMenuMode();
// Nothing in this file may re-derive a size: the swapchain gets the launcher's pick verbatim.
// The helper that used to do it is gone entirely -- see HookedResizeBuffers.


// THESE DEFINITIONS SIT OUTSIDE THE ANONYMOUS NAMESPACE BELOW, and that is the whole reason the
// swapchain family could be split at all. Everything in `namespace { }` has internal linkage, so
// Dred.cpp, OsInput.cpp, DepthCapture.cpp and DisplayModes.cpp could not reach one name of it --
// the same wall that kept SyncStereo.cpp a single 13,000-line file until its anonymous namespace
// was named. Only what Hooks/SwapChainInternal.hpp declares was lifted out; the rest stays private.

std::unordered_map<uintptr_t, void*> g_originalVtableMethods;
std::mutex g_vtableMutex;
std::mutex g_dredMutex;
Microsoft::WRL::ComPtr<ID3D12Device> g_dredDevice;
bool g_dredDumped = false;
bool g_cursorClipped = false;
std::atomic<void*> g_presentQueue{nullptr};
std::atomic<uint64_t> g_eclTotalCalls{0};
std::atomic<uint64_t> g_eclTotalLists{0};
std::atomic<uint64_t> g_eclPresentCalls{0};
std::atomic<uint64_t> g_eclPresentLists{0};
std::atomic<uint32_t> g_eclDistinctQueues{0};
std::atomic<void*> g_eclSeenQueues[16];

const char* WideToUtf8(const wchar_t* value, char* buffer, size_t bufferSize) {
if (!value) {
    return "<unnamed>";
}
if (!buffer || bufferSize == 0) {
    return "<invalid-buffer>";
}
const int written = WideCharToMultiByte(CP_UTF8, 0, value, -1, buffer, static_cast<int>(bufferSize), nullptr, nullptr);
return written > 0 ? buffer : "<wide-conversion-failed>";
}

// Out of the anonymous namespace for the same reason as the state above: Dred.cpp calls it.
const char* DebugName(const char* ansiName, const wchar_t* wideName, char* buffer, size_t bufferSize) {
if (ansiName && ansiName[0] != '\0') {
    return ansiName;
}
return WideToUtf8(wideName, buffer, bufferSize);
}

namespace {
using PresentFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
using EnumOutputsFn = HRESULT(STDMETHODCALLTYPE*)(IDXGIAdapter*, UINT, IDXGIOutput**);
using GetDisplayModeListFn = HRESULT(STDMETHODCALLTYPE*)(IDXGIOutput*, DXGI_FORMAT, UINT, UINT*, DXGI_MODE_DESC*);
using GetDisplayModeList1Fn = HRESULT(STDMETHODCALLTYPE*)(IDXGIOutput1*, DXGI_FORMAT, UINT, UINT*, DXGI_MODE_DESC1*);
using SetFullscreenStateFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, BOOL, IDXGIOutput*);
using ResizeBuffersFn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT, UINT, DXGI_FORMAT, UINT);
using ResizeBuffers1Fn = HRESULT(STDMETHODCALLTYPE*)(IDXGISwapChain3*, UINT, UINT, UINT, DXGI_FORMAT, UINT, const UINT*, IUnknown* const*);


// [ECL-DIAG] Temporary tearing diagnostic. Hypothesis: the swapchain backbuffer
// is rendered on a command queue different from the present queue (m_d3dQueue),
// so our capture copy (issued on the present queue) races the game's render ->
// torn source frame. If the present queue receives ~0% of the game's command
// lists while other queues receive the bulk, the hypothesis is confirmed.
using ExecuteCommandListsFn = void(STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);



// Moved to src/Hooks/Dred.cpp: device-removed diagnostics.

// Moved to src/Hooks/OsInput.cpp: the Win32 IAT hooks: cursor, window rect, clip.

// Moved to src/Hooks/DepthCapture.cpp: the depth capture, the command-list vtable and the queue-signal wait.

// Moved to src/Hooks/DisplayModes.cpp: making the VR resolution appear in the game's own mode list.

// True for a window WE created (mirror, dummies). Everything of ours registers a
// "CyberpunkVR*" window class.
//
// Our own swapchains must stay outside the proxy's bookkeeping entirely. Binding the overlay
// to the mirror's window steals it from the game, and patching the mirror's vtable pulls a
// second, 11on12-backed swapchain into paths built around the game's D3D12 device -- which
// ends as DXGI_ERROR_ACCESS_DENIED on the shared surface. The testbed had no proxy at all and
// its mirror simply worked; this puts ours back in the same position.
static bool IsOurOwnWindow(HWND hwnd) {
    if (!hwnd) return false;
    wchar_t cls[64] = {};
    GetClassNameW(hwnd, cls, 63);
    return wcsncmp(cls, L"CyberpunkVR", 11) == 0;
}

HRESULT STDMETHODCALLTYPE HookedPresent(IDXGISwapChain* swapChain, UINT syncInterval, UINT flags) {
    RememberDredDeviceFromSwapChain(swapChain);

    DXGI_SWAP_CHAIN_DESC desc{};
    const bool hasDesc = SUCCEEDED(swapChain->GetDesc(&desc));

    // OUR OWN swapchains present through this same hook -- pass them straight to DXGI.
    //
    // The Present vtable is shared by every swapchain in the process, so when the VRCAM mirror
    // opens its window we start getting called for a surface that is not the game's. Running
    // the per-frame work on it renders the ImGui overlay into the MIRROR's backbuffer, hands
    // the XR submit path a foreign swapchain, and republishes the depth-picker size from it.
    // That is an access violation the moment the mirror appears -- which is exactly what
    // enabling it did. The testbed overlay carried this same filter, and for the same reason;
    // the proxy never needed it until there was a second swapchain in the process.
    //
    // Identified by window class: everything we create registers a "CyberpunkVR*" class.
    if (hasDesc && IsOurOwnWindow(desc.OutputWindow)) {
        {
            void** vt = *reinterpret_cast<void***>(swapChain);
            PresentFn orig = GetOriginalMethod<PresentFn>(vt, 8);
            return orig ? orig(swapChain, syncInterval, flags) : DXGI_ERROR_INVALID_CALL;
        }
    }

    // Bind the overlay to the game's window on first sight.
    //
    // As a proxy we learned the HWND from CreateSwapChain. A plugin never sees that call, so
    // the first Present on a window that is not ours IS the game's -- take it from there.
    // Idempotent, and OverlaySetWindow is cheap once bound.
    if (hasDesc && desc.OutputWindow && g_gameHwnd != desc.OutputWindow) {
        g_gameHwnd = desc.OutputWindow;
        OverlaySetWindow(desc.OutputWindow);
        Log("Present: bound overlay to game window %p\n", desc.OutputWindow);
    }

    if (hasDesc) {
        UpdateCursorCapture(desc.OutputWindow);
    }

    // DLSS is loaded lazily after the renderer first evaluates a frame, so
    // nvngx_dlss.dll may not yet be in the process at startup. Try to install
    // the NGX EvaluateFeature hook on every Present until it succeeds; once
    // installed the function returns true cheaply.
    static std::atomic<bool> s_ngxHookTried{false};
    if (!s_ngxHookTried.load(std::memory_order_acquire)) {
        if (NgxInstallEvaluateFeatureHook()) {
            s_ngxHookTried.store(true, std::memory_order_release);
        }
    }

    // Close the stereo profiler's frame: it accumulates per-node ticks continuously and only
    // normalises them into ms/frame when told a frame ended. Without this call its counter
    // stays at zero and every number it reports is an un-normalised window total -- which is
    // exactly what the testbed's own Present hook was for. Unconditional: it is a QPC read and
    // a few atomic exchanges, and the per-node accumulation it drains is what respects
    // CyberpunkVR_ProfEnable.
    CyberpunkVR_ProfPublish();

    // Publish the presented size for the scene-depth picker and re-open the choice for the
    // next frame: the pick must be per-frame, not a session maximum (see the OM hook).
    if (swapChain) {
        DXGI_SWAP_CHAIN_DESC scd{};
        if (SUCCEEDED(swapChain->GetDesc(&scd))) {
            g_presentWidth.store(scd.BufferDesc.Width, std::memory_order_relaxed);
            g_presentHeight.store(scd.BufferDesc.Height, std::memory_order_relaxed);
        }
    }
    // Reopen the scene-depth choice only when the current pick has gone quiet. Resetting
    // unconditionally is what let it alternate between MAIN's two same-size depth buffers.
    if (g_sceneDepthSeen.exchange(false, std::memory_order_relaxed)) {
        g_sceneDepthMissFrames.store(0, std::memory_order_relaxed);
    } else if (g_sceneDepthMissFrames.fetch_add(1, std::memory_order_relaxed) >= 8) {
        g_sceneDepthMissFrames.store(0, std::memory_order_relaxed);
        g_sceneDepthArea.store(0, std::memory_order_relaxed);
    }

    OverlayRender(swapChain);
    OpenXRManager::Get().OnPresent(swapChain);
    // Drive one XR frame inline on the Present thread to avoid the old
    // cross-thread submit drift while keeping the rest of the pipeline at HEAD.
    OpenXRManager::Get().PumpInlineFrame();
    // AFTER the pump, so an inline cycle is complete when its numbers are read and the
    // wait/begin/end totals are allowed to be exactly equal. Once a second; see the definition.
    OpenXRManager::Get().ReportXrFrameRates();
    void** vtable = *reinterpret_cast<void***>(swapChain);
    PresentFn originalFn = GetOriginalMethod<PresentFn>(vtable, 8);
    const HRESULT hr = originalFn ? originalFn(swapChain, syncInterval, flags) : DXGI_ERROR_INVALID_CALL;
    if (FAILED(hr)) {
        Log("[DRED] Present failure observed. hr=0x%08X removed=%d swapChain=%p\n",
            static_cast<unsigned>(hr),
            IsDeviceRemovedHr(hr) ? 1 : 0,
            swapChain);
    }
    if (FAILED(hr) && IsDeviceRemovedHr(hr)) {
        DumpDredOnce(swapChain, hr);
    }

    static uint64_t presentLogCounter = 0;
    if (hr != S_OK || ((++presentLogCounter % 600) == 1)) {
        HWND foreground = GetForegroundWindow();
        RECT clientRect{};
        RECT windowRect{};
        if (hasDesc && desc.OutputWindow) {
            GetClientRect(desc.OutputWindow, &clientRect);
            GetWindowRect(desc.OutputWindow, &windowRect);
        }
        Log("Present result: hr=0x%08X hwnd=%p fg=%p windowed=%d desc=%ux%u client=%ldx%ld window=(%ld,%ld)-(%ld,%ld) cursorClipped=%d\n",
            static_cast<unsigned int>(hr),
            hasDesc ? desc.OutputWindow : nullptr,
            foreground,
            hasDesc ? (desc.Windowed ? 1 : 0) : -1,
            hasDesc ? desc.BufferDesc.Width : 0,
            hasDesc ? desc.BufferDesc.Height : 0,
            clientRect.right - clientRect.left,
            clientRect.bottom - clientRect.top,
            windowRect.left,
            windowRect.top,
            windowRect.right,
            windowRect.bottom,
            g_cursorClipped ? 1 : 0);

        const uint64_t totalLists = g_eclTotalLists.load(std::memory_order_relaxed);
        const uint64_t presentLists = g_eclPresentLists.load(std::memory_order_relaxed);
        Log("[ECL-DIAG] presentQueue=%p lists present=%llu/%llu calls=%llu/%llu share=%.1f%% distinctQueues=%u\n",
            g_presentQueue.load(std::memory_order_relaxed),
            static_cast<unsigned long long>(presentLists),
            static_cast<unsigned long long>(totalLists),
            static_cast<unsigned long long>(g_eclPresentCalls.load(std::memory_order_relaxed)),
            static_cast<unsigned long long>(g_eclTotalCalls.load(std::memory_order_relaxed)),
            totalLists ? (100.0 * static_cast<double>(presentLists) / static_cast<double>(totalLists)) : 0.0,
            g_eclDistinctQueues.load(std::memory_order_relaxed));
    }
    return hr;
}

HRESULT STDMETHODCALLTYPE HookedSetFullscreenState(IDXGISwapChain* swapChain, BOOL fullscreen, IDXGIOutput* target) {
    if (target) {
        InstallOutputHook(target);
    }

    void** vtable = *reinterpret_cast<void***>(swapChain);
    SetFullscreenStateFn originalFn = GetOriginalMethod<SetFullscreenStateFn>(vtable, 10);
    Log("SetFullscreenState intercepted: fullscreen=%d target=%p.\n", fullscreen ? 1 : 0, target);
    if (fullscreen) {
        return DXGI_ERROR_NOT_CURRENTLY_AVAILABLE;
    }
    return originalFn ? originalFn(swapChain, FALSE, target) : DXGI_ERROR_INVALID_CALL;
}

// THE LAUNCHER'S SIZE, VERBATIM. Nothing is recomputed here.
//
// This used to take the width from the launcher and RE-DERIVE the height as
// launcherWidth / (the runtime's RECOMMENDED render-target aspect), which is not the aspect
// that matters. On a Quest 3 it is 1680/1760 = 0.95455, so a 2560x2848 pick came out as
//
//     ResizeBuffers override: 2560x2848 -> 2560x2682          (2560 / 0.95455 = 2681.9)
//
// Three things wrong with it. It contradicted the contract written on the function it called
// ("only the RENDER override uses this; window/swapchain getters keep the launcher value").
// It disagreed with CreateSwapChainForHwnd, which forces the launcher size, so the same
// swapchain was created at one shape and resized to another. And the aspect it used is the
// wrong one: the rule this port is built on is rect aspect == frustum TANGENT aspect, and for
// a Quest 3 that is 1.072369/1.191754 = 0.89982 -- which is what the launcher's 2560x2848
// (0.89888) already is. Recomputing replaced a correct height with a panel-shaped one and cost
// 3.4 degrees of vertical field: the engine derives tanV = tanH / renderAspect, so 2682 gives
// V = 96.66 while the lens wants 100.00 and 2848 gives 100.06.
//
// It went unnoticed because on a PICO the recommended size is near-square and so is the
// tangent aspect, so the two agreed and this was the identity.
//
// Both of the other things that read that aspect -- the DLSS resolution override and the DLSS
// projection-matrix injection -- were deleted with it, so there is no second path back in.
HRESULT STDMETHODCALLTYPE HookedResizeBuffers(IDXGISwapChain* swapChain, UINT bufferCount, UINT width, UINT height, DXGI_FORMAT newFormat, UINT flags) {
    const UINT forcedWidth = GetForcedSwapchainWidth();
    const UINT forcedHeight = GetForcedSwapchainHeight();

    const UINT outWidth = forcedWidth != 0 ? forcedWidth : width;
    const UINT outHeight = forcedHeight != 0 ? forcedHeight : height;
    if (outWidth != width || outHeight != height) {
        Log("ResizeBuffers override: %ux%u -> %ux%u\n", width, height, outWidth, outHeight);
    }

    void** vtable = *reinterpret_cast<void***>(swapChain);
    ResizeBuffersFn originalFn = GetOriginalMethod<ResizeBuffersFn>(vtable, 13);
    OverlayInvalidateSwapchainResources();
    return originalFn ? originalFn(swapChain, bufferCount, outWidth, outHeight, newFormat, flags) : DXGI_ERROR_INVALID_CALL;
}

// Same rule as HookedResizeBuffers above, and for the same reasons: the launcher's size, verbatim.
// The two must agree -- a swapchain that supports ResizeBuffers1 goes through this one instead, so
// fixing only the other would leave the bug alive on exactly the runtimes that take this path.
HRESULT STDMETHODCALLTYPE HookedResizeBuffers1(IDXGISwapChain3* swapChain, UINT bufferCount, UINT width, UINT height, DXGI_FORMAT format, UINT flags, const UINT* creationNodeMask, IUnknown* const* presentQueue) {
    const UINT forcedWidth = GetForcedSwapchainWidth();
    const UINT forcedHeight = GetForcedSwapchainHeight();


    const UINT outWidth = forcedWidth != 0 ? forcedWidth : width;
    const UINT outHeight = forcedHeight != 0 ? forcedHeight : height;
    if (outWidth != width || outHeight != height) {
        Log("ResizeBuffers1 override: %ux%u -> %ux%u\n", width, height, outWidth, outHeight);
    }

    void** vtable = *reinterpret_cast<void***>(swapChain);
    ResizeBuffers1Fn originalFn = GetOriginalMethod<ResizeBuffers1Fn>(vtable, 39);
    OverlayInvalidateSwapchainResources();
    return originalFn ? originalFn(swapChain, bufferCount, outWidth, outHeight, format, flags, creationNodeMask, presentQueue) : DXGI_ERROR_INVALID_CALL;
}

void InstallSwapchainHooks(IDXGISwapChain* swapChain) {
    if (!swapChain) return;

    void*** objectVtable = reinterpret_cast<void***>(swapChain);
    if (!objectVtable || !*objectVtable) return;

    void** vtable = *objectVtable;
    PatchVtableMethod(vtable, 8, reinterpret_cast<void*>(&HookedPresent));
    PatchVtableMethod(vtable, 10, reinterpret_cast<void*>(&HookedSetFullscreenState));
    PatchVtableMethod(vtable, 13, reinterpret_cast<void*>(&HookedResizeBuffers));

    IDXGISwapChain3* swapChain3 = nullptr;
    if (SUCCEEDED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3))) && swapChain3) {
        void** vtable3 = *reinterpret_cast<void***>(swapChain3);
        PatchVtableMethod(vtable3, 39, reinterpret_cast<void*>(&HookedResizeBuffers1));
        swapChain3->Release();
    }
}
}

// Extern "C" forwarder so vr_core.cpp can invoke the anonymous-namespace
// DRED initializer at each CreateDXGIFactory* entry (runs before any D3D12
// device is created — required for breadcrumbs/page-fault data to populate).
extern "C" void CyberpunkVRPort_EnableDredOnce() {
    EnableDredOnce();
}

DXGIFactoryWrapper::DXGIFactoryWrapper(IDXGIFactory7* realFactory) : m_real(realFactory), m_refCount(1) {}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::QueryInterface(REFIID riid, void** ppvObject) {
    if (riid == __uuidof(IUnknown) || riid == __uuidof(IDXGIObject) || riid == __uuidof(IDXGIFactory) ||
        riid == __uuidof(IDXGIFactory1) || riid == __uuidof(IDXGIFactory2) || riid == __uuidof(IDXGIFactory3) ||
        riid == __uuidof(IDXGIFactory4) || riid == __uuidof(IDXGIFactory5) || riid == __uuidof(IDXGIFactory6) ||
        riid == __uuidof(IDXGIFactory7)) {
        *ppvObject = this;
        AddRef();
        return S_OK;
    }
    return m_real->QueryInterface(riid, ppvObject);
}

ULONG STDMETHODCALLTYPE DXGIFactoryWrapper::AddRef() {
    return InterlockedIncrement(&m_refCount);
}

ULONG STDMETHODCALLTYPE DXGIFactoryWrapper::Release() {
    ULONG ref = InterlockedDecrement(&m_refCount);
    if (ref == 0) {
        m_real->Release();
        delete this;
    }
    return ref;
}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::SetPrivateData(REFGUID Name, UINT DataSize, const void* pData) { return m_real->SetPrivateData(Name, DataSize, pData); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::SetPrivateDataInterface(REFGUID Name, const IUnknown* pUnknown) { return m_real->SetPrivateDataInterface(Name, pUnknown); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetPrivateData(REFGUID Name, UINT* pDataSize, void* pData) { return m_real->GetPrivateData(Name, pDataSize, pData); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetParent(REFIID riid, void** ppParent) { return m_real->GetParent(riid, ppParent); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapters(UINT Adapter, IDXGIAdapter** ppAdapter) {
    HRESULT hr = m_real->EnumAdapters(Adapter, ppAdapter);
    if (SUCCEEDED(hr) && ppAdapter && *ppAdapter) {
        InstallAdapterHook(*ppAdapter);
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::MakeWindowAssociation(HWND WindowHandle, UINT Flags) { return m_real->MakeWindowAssociation(WindowHandle, Flags); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetWindowAssociation(HWND* pWindowHandle) { return m_real->GetWindowAssociation(pWindowHandle); }

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChain(IUnknown* pDevice, DXGI_SWAP_CHAIN_DESC* pDesc, IDXGISwapChain** ppSwapChain) {
    Log("CreateSwapChain intercepted! pDevice=%p\n", pDevice);
    PrepareStartupLiveControls();

    DXGI_SWAP_CHAIN_DESC localDesc{};
    DXGI_SWAP_CHAIN_DESC* swapDesc = pDesc;
    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (pDesc) {
        localDesc = *pDesc;
        bool changed = false;
        if (forcedWidth != 0 && forcedHeight != 0) {
            localDesc.BufferDesc.Width = forcedWidth;
            localDesc.BufferDesc.Height = forcedHeight;
            // Standard DXGI_SWAP_CHAIN_DESC does not have Scaling.
            changed = true;
            Log("CreateSwapChain override: %ux%u -> %ux%u\n",
                pDesc->BufferDesc.Width,
                pDesc->BufferDesc.Height,
                forcedWidth,
                forcedHeight);
        }
        if (!localDesc.Windowed) {
            localDesc.Windowed = TRUE;
            changed = true;
            Log("CreateSwapChain: Forced Windowed=TRUE\n");
        }
        if (changed) {
            swapDesc = &localDesc;
        }
    }

    // pDevice in D3D12 is ID3D12CommandQueue
    ID3D12CommandQueue* pQueue = nullptr;
    if (SUCCEEDED(pDevice->QueryInterface(IID_PPV_ARGS(&pQueue)))) {
        ID3D12Device* d3dDevice = nullptr;
        if (SUCCEEDED(pQueue->GetDevice(IID_PPV_ARGS(&d3dDevice)))) {
            RememberDredDevice(d3dDevice);
            OpenXRManager::Get().InitGraphics(d3dDevice, pQueue);
            OverlaySetDeviceAndQueue(d3dDevice, pQueue);
            InstallCommandQueueDiagHook(pQueue);
            InstallDepthCaptureHooks(d3dDevice);
            // The device here came from the queue the GAME passed, so it is the game-facing one --
            // wrapped when a capture layer is present, which is what makes the port markers reachable.
            // See the note at RegisterGameFacingListVtable; a no-op in an ordinary session, and it
            // self-guards so calling it from every swapchain path costs nothing.
            RegisterGameFacingListVtable(d3dDevice);
            d3dDevice->Release();
        }
        pQueue->Release();
    }

    HWND hWnd = swapDesc ? swapDesc->OutputWindow : nullptr;
    g_gameHwnd = hWnd;
    UINT windowWidth = GetForcedWindowWidth();
    UINT windowHeight = GetForcedWindowHeight();
    if (windowWidth == forcedWidth) {
        windowWidth = 0;
    }
    if (windowWidth == 0 && hWnd) {
        HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO mi = { sizeof(MONITORINFO) };
        if (GetMonitorInfoA(monitor, &mi)) {
            int monWidth = mi.rcMonitor.right - mi.rcMonitor.left;
            int monHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;
            windowWidth = monWidth;
            windowHeight = monHeight;
        }
    }
    if (windowWidth != 0 && windowHeight != 0 && hWnd) {
        SetWindowPos(hWnd, nullptr, 0, 0, windowWidth, windowHeight, SWP_NOMOVE | SWP_NOZORDER);
        Log("CreateSwapChain: Capped window size to %ux%u\n", windowWidth, windowHeight);
    }
    const HRESULT hr = m_real->CreateSwapChain(pDevice, swapDesc, ppSwapChain);
    if (SUCCEEDED(hr) && ppSwapChain && *ppSwapChain) {
        const bool ours = swapDesc && IsOurOwnWindow(swapDesc->OutputWindow);
        if (!ours) {
            if (swapDesc && swapDesc->OutputWindow) {
                OverlaySetWindow(swapDesc->OutputWindow);
            }
            InstallSwapchainHooks(*ppSwapChain);
            InstallOSHooks();
        }
    }
    return hr;
}

// ============================================================================================
// RED4ext plugin bootstrap
// ============================================================================================
//
// Everything the DXGI proxy used to do at CreateSwapChain time, done from a plugin instead.
// The proxy is being retired: as dxgi.dll we owned the process's only swapchain and every code
// path assumed it, which is why the VRCAM mirror -- a SECOND window, swapchain and queue --
// kept taking the whole thing down in a new way each time. A red4ext plugin sits beside the
// game rather than in front of it, which is the shape the build that actually worked had.
//
// Two things the proxy got for free have to be acquired here:
//   device + queue : from sync_stereo's D3D12CreateDevice hook, which runs before the game
//                    creates its device and captures both (CyberpunkVR_GetGameDevice/Queue).
//   Present        : the swapchain vtable is SHARED by every swapchain in the process, so a
//                    throwaway 8x8 swapchain hands us the same function table the game's one
//                    uses. Patch it there and the game's Present is hooked without ever
//                    holding the game's swapchain.
extern "C" ID3D12Device*       CyberpunkVR_GetGameDevice();
extern "C" ID3D12CommandQueue* CyberpunkVR_GetGameQueue();

// The proxy did all of this from DXGIFactoryWrapper::CreateSwapChain. A plugin never gets
// that call -- so hook the factory's vtable instead and do it at the identical moment.
//
// A dummy swapchain was the first attempt: it gets the shared Present vtable without the
// factory. But it hands you Present and nothing else, and the resolution override, the DRED
// enable, the live-controls preload and the early OpenXR init ALL happened in that same call.
// Losing them is what left the game hanging on the loading screen with no override applied.
// Hooking the factory brings the whole sequence back, in order, on the real swapchain.
extern "C" void CyberpunkVRPort_EnableDredOnce();
extern "C" void InitOpenXREarly();

using FacCreateSwapChainFn = HRESULT (STDMETHODCALLTYPE*)(
    IDXGIFactory*, IUnknown*, DXGI_SWAP_CHAIN_DESC*, IDXGISwapChain**);
using FacCreateSwapChainForHwndFn = HRESULT (STDMETHODCALLTYPE*)(
    IDXGIFactory2*, IUnknown*, HWND, const DXGI_SWAP_CHAIN_DESC1*,
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);

static FacCreateSwapChainFn        g_origFacCSC = nullptr;
static FacCreateSwapChainForHwndFn g_origFacCSCFH = nullptr;

// Everything the wrapper did BEFORE handing the call on: one-time early init, then the
// device/queue wiring taken off the command queue the game is creating the swapchain with.
static void PluginPreSwapchain(IUnknown* pDevice) {
    static std::atomic<bool> s_early{false};
    bool expected = false;
    if (s_early.compare_exchange_strong(expected, true)) {
        CyberpunkVRPort_EnableDredOnce();
        PrepareStartupLiveControls();
        InitOpenXREarly();          // creates the XR instance/session; InitGraphics needs it
    }
    ID3D12CommandQueue* pQueue = nullptr;
    if (!pDevice || FAILED(pDevice->QueryInterface(IID_PPV_ARGS(&pQueue))) || !pQueue) return;
    ID3D12Device* d3dDevice = nullptr;
    if (SUCCEEDED(pQueue->GetDevice(IID_PPV_ARGS(&d3dDevice))) && d3dDevice) {
        static std::atomic<bool> s_wired{false};
        bool w = false;
        if (s_wired.compare_exchange_strong(w, true)) {
            Log("PluginBootstrap: device=%p queue=%p\n", d3dDevice, pQueue);
            RememberDredDevice(d3dDevice);
            OpenXRManager::Get().InitGraphics(d3dDevice, pQueue);
            OverlaySetDeviceAndQueue(d3dDevice, pQueue);
            InstallCommandQueueDiagHook(pQueue);
            InstallDepthCaptureHooks(d3dDevice);
            // The device here came from the queue the GAME passed, so it is the game-facing one --
            // wrapped when a capture layer is present, which is what makes the port markers reachable.
            // See the note at RegisterGameFacingListVtable; a no-op in an ordinary session, and it
            // self-guards so calling it from every swapchain path costs nothing.
            RegisterGameFacingListVtable(d3dDevice);
        }
        d3dDevice->Release();
    }
    pQueue->Release();
}

// ...and everything it did AFTER, on the swapchain that came back.
static void PluginPostSwapchain(IDXGISwapChain* sc, HWND hwnd) {
    if (!sc || IsOurOwnWindow(hwnd)) return;

    // Cap the WINDOW to the monitor. The swapchain is forced to the VR render size (2560x2560
    // here), and without this the game sizes its window to match and hangs off the screen --
    // which is exactly what happened once the proxy stopped doing it. The backbuffer stays at
    // the forced size; only the window is clamped.
    if (hwnd) {
        UINT winW = GetForcedWindowWidth();
        UINT winH = GetForcedWindowHeight();
        if (winW == GetForcedDisplayModeWidth()) winW = 0;   // not a real window override
        if (winW == 0) {
            HMONITOR mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
            MONITORINFO mi = { sizeof(MONITORINFO) };
            if (GetMonitorInfoA(mon, &mi)) {
                winW = static_cast<UINT>(mi.rcMonitor.right - mi.rcMonitor.left);
                winH = static_cast<UINT>(mi.rcMonitor.bottom - mi.rcMonitor.top);
            }
        }
        if (winW && winH) {
            SetWindowPos(hwnd, nullptr, 0, 0, static_cast<int>(winW), static_cast<int>(winH),
                         SWP_NOMOVE | SWP_NOZORDER);
            Log("PluginBootstrap: capped window to %ux%u\n", winW, winH);
        }
    }

    if (hwnd) {
        g_gameHwnd = hwnd;
        OverlaySetWindow(hwnd);
    }
    InstallSwapchainHooks(sc);
    InstallOSHooks();
    Log("PluginBootstrap: swapchain hooked, overlay bound to %p\n", hwnd);
}

static HRESULT STDMETHODCALLTYPE Detour_FacCreateSwapChain(
        IDXGIFactory* self, IUnknown* pDevice, DXGI_SWAP_CHAIN_DESC* pDesc,
        IDXGISwapChain** ppSwapChain) {
    PluginPreSwapchain(pDevice);

    DXGI_SWAP_CHAIN_DESC local{};
    DXGI_SWAP_CHAIN_DESC* useDesc = pDesc;
    const UINT fw = GetForcedDisplayModeWidth();
    const UINT fh = GetForcedDisplayModeHeight();
    if (pDesc) {
        local = *pDesc;
        bool changed = false;
        if (fw && fh) {
            local.BufferDesc.Width = fw;
            local.BufferDesc.Height = fh;
            changed = true;
            Log("CreateSwapChain override: %ux%u -> %ux%u\n",
                pDesc->BufferDesc.Width, pDesc->BufferDesc.Height, fw, fh);
        }
        if (!local.Windowed) { local.Windowed = TRUE; changed = true; }
        if (changed) useDesc = &local;
    }
    const HRESULT hr = g_origFacCSC(self, pDevice, useDesc, ppSwapChain);
    if (SUCCEEDED(hr) && ppSwapChain && *ppSwapChain) {
        PluginPostSwapchain(*ppSwapChain, useDesc ? useDesc->OutputWindow : nullptr);
    }
    return hr;
}

static HRESULT STDMETHODCALLTYPE Detour_FacCreateSwapChainForHwnd(
        IDXGIFactory2* self, IUnknown* pDevice, HWND hWnd, const DXGI_SWAP_CHAIN_DESC1* pDesc,
        const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* pFsDesc, IDXGIOutput* pRestrict,
        IDXGISwapChain1** ppSwapChain) {
    // Our own mirror/dummy windows go straight through: they must stay outside every path
    // built around the game's single swapchain.
    if (IsOurOwnWindow(hWnd)) {
        return g_origFacCSCFH(self, pDevice, hWnd, pDesc, pFsDesc, pRestrict, ppSwapChain);
    }
    PluginPreSwapchain(pDevice);

    DXGI_SWAP_CHAIN_DESC1 local{};
    const DXGI_SWAP_CHAIN_DESC1* useDesc = pDesc;
    const UINT fw = GetForcedSwapchainWidth();
    const UINT fh = GetForcedSwapchainHeight();
    if (pDesc && fw && fh) {
        local = *pDesc;
        local.Width = fw;
        local.Height = fh;
        useDesc = &local;
        Log("CreateSwapChainForHwnd override: %ux%u -> %ux%u\n",
            pDesc->Width, pDesc->Height, fw, fh);
    }
    const HRESULT hr = g_origFacCSCFH(self, pDevice, hWnd, useDesc, pFsDesc, pRestrict, ppSwapChain);
    if (SUCCEEDED(hr) && ppSwapChain && *ppSwapChain) {
        PluginPostSwapchain(*ppSwapChain, hWnd);
    }
    return hr;
}

// Patch the factory vtable. It is shared by every IDXGIFactory in the process, so our own
// throwaway factory exposes the same table the game's factory dispatches through -- we never
// need to hold the game's instance, only to be installed before it creates its swapchain.
// Patch the vtable of a factory the GAME created.
//
// Creating our own to read the shared vtable does not work here: CreateDXGIFactory2 returns
// E_FAIL for the whole startup (measured, hr=0x80004005). Streamline's interposer sits on DXGI
// in this process, and a factory we ask for is not one it is willing to give. So we never ask
// -- we hook the factory EXPORTS, let the game make the call it was going to make anyway, and
// patch the vtable of what comes back. The table is shared, so one patch covers every factory.
static bool HookFactoryVtable(void* factoryObj) {
    static std::atomic<bool> s_patched{false};
    if (!factoryObj) return false;
    IDXGIFactory2* f2 = nullptr;
    if (FAILED(static_cast<IUnknown*>(factoryObj)->QueryInterface(IID_PPV_ARGS(&f2))) || !f2) {
        return false;
    }
    bool expected = false;
    if (!s_patched.compare_exchange_strong(expected, true)) { f2->Release(); return true; }
    void** vt = *reinterpret_cast<void***>(f2);
    // IDXGIFactory::CreateSwapChain = 10, IDXGIFactory2::CreateSwapChainForHwnd = 15
    g_origFacCSC   = reinterpret_cast<FacCreateSwapChainFn>(vt[10]);
    g_origFacCSCFH = reinterpret_cast<FacCreateSwapChainForHwndFn>(vt[15]);
    const bool a = PatchVtableMethod(vt, 10, reinterpret_cast<void*>(&Detour_FacCreateSwapChain));
    const bool b = PatchVtableMethod(vt, 15, reinterpret_cast<void*>(&Detour_FacCreateSwapChainForHwnd));
    Log("PluginBootstrap: factory vtable hooked csc=%d cscfh=%d fac=%p\n", (int)a, (int)b, factoryObj);
    f2->Release();
    return a || b;
}

// ---- the factory exports themselves ------------------------------------------------------
using CreateFactoryFn  = HRESULT (WINAPI*)(REFIID, void**);
using CreateFactory2Fn = HRESULT (WINAPI*)(UINT, REFIID, void**);
static CreateFactoryFn  g_origCreateFactory  = nullptr;
static CreateFactoryFn  g_origCreateFactory1 = nullptr;
static CreateFactory2Fn g_origCreateFactory2 = nullptr;

static HRESULT WINAPI Hook_CreateDXGIFactory(REFIID riid, void** pp) {
    const HRESULT hr = g_origCreateFactory(riid, pp);
    if (SUCCEEDED(hr) && pp && *pp) HookFactoryVtable(*pp);
    return hr;
}
static HRESULT WINAPI Hook_CreateDXGIFactory1(REFIID riid, void** pp) {
    const HRESULT hr = g_origCreateFactory1(riid, pp);
    if (SUCCEEDED(hr) && pp && *pp) HookFactoryVtable(*pp);
    return hr;
}
static HRESULT WINAPI Hook_CreateDXGIFactory2(UINT flags, REFIID riid, void** pp) {
    const HRESULT hr = g_origCreateFactory2(flags, riid, pp);
    if (SUCCEEDED(hr) && pp && *pp) HookFactoryVtable(*pp);
    return hr;
}

static void HookFactoryExportsIn(const wchar_t* moduleName) {
    HMODULE m = GetModuleHandleW(moduleName);
    if (!m) return;
    struct { const char* name; void* detour; void** orig; } targets[] = {
        { "CreateDXGIFactory",  reinterpret_cast<void*>(&Hook_CreateDXGIFactory),  reinterpret_cast<void**>(&g_origCreateFactory)  },
        { "CreateDXGIFactory1", reinterpret_cast<void*>(&Hook_CreateDXGIFactory1), reinterpret_cast<void**>(&g_origCreateFactory1) },
        { "CreateDXGIFactory2", reinterpret_cast<void*>(&Hook_CreateDXGIFactory2), reinterpret_cast<void**>(&g_origCreateFactory2) },
    };
    for (auto& t : targets) {
        // Resolved through RenderDoc when a capture layer is resident, so the trampoline calls its
        // serializer and the factory we return is WRAPPED. A plain GetProcAddress here is what made the
        // game refuse to start under RenderDoc: an unwrapped factory yields an unwrapped adapter, then an
        // unwrapped device, and NVAPI initialisation fails -- reported by the game as ray tracing failing
        // to load. See src/Hooks/RenderDocBridge.cpp for the log line that proves it.
        void* fn = cvr::RenderDocResolveHookTarget(m, t.name);
        if (!fn || *t.orig) continue;
        if (MH_CreateHook(fn, t.detour, t.orig) == MH_OK && MH_EnableHook(fn) == MH_OK) {
            Log("PluginBootstrap: hooked %S!%s\n", moduleName, t.name);
            // ...and OPTIONALLY tell a resident capture layer that ITS original is our trampoline, so the
            // order becomes game -> RenderDoc -> us -> the real export.
            //
            // OFF BY DEFAULT BECAUSE IT KILLS THE GAME. With it on, the plugin log ends exactly here:
            //
            //     [RenderDoc] chained behind 'dxgi.dll!CreateDXGIFactory2': ok (our trampoline ...)
            //
            // and nothing follows -- not even the "hooked sl.interposer.dll!..." line that comes a few
            // instructions later -- while RenderDoc's own log stays 319 bytes long. So the process dies
            // while the second module is being re-pointed, before any factory is ever created. The
            // mechanism is not established, and re-pointing another library's ORIGINAL function pointer
            // from outside is exactly the kind of thing that deserves a debugger rather than a guess.
            //
            // Set CPVR_RD_CHAIN=1 to try it again under a debugger. Everything else here is unchanged, so
            // the default path is the one that was measured to run.
            char chain[8] = {};
            if (GetEnvironmentVariableA("CPVR_RD_CHAIN", chain, sizeof(chain)) > 0 && chain[0] == '1') {
                char ansiModule[64] = {};
                WideCharToMultiByte(CP_ACP, 0, moduleName, -1, ansiModule,
                                    static_cast<int>(sizeof(ansiModule)) - 1, nullptr, nullptr);
                cvr::RenderDocChainBehindHook(ansiModule, t.name, *t.orig);
            }
        }
    }
}

// Installed from the plugin entry, as early as possible: the game has to still be ahead of its
// own CreateDXGIFactory call for this to catch anything.
extern "C" __declspec(dllexport) void CyberpunkVRPort_PluginBootstrap() {
    static std::atomic<bool> s_started{false};
    bool expected = false;
    if (!s_started.compare_exchange_strong(expected, true)) return;
    const MH_STATUS st = MH_Initialize();
    if (st != MH_OK && st != MH_ERROR_ALREADY_INITIALIZED) {
        Log("PluginBootstrap: MH_Initialize failed %d\n", static_cast<int>(st));
        return;
    }
    // BEFORE anything touches NVAPI. RenderDoc disables NVAPI by default and the game then fails its
    // NVIDIA-side init, reporting on screen that ray tracing could not load -- with ray tracing off. This
    // flips the one official switch that allows it through, and is a no-op when no capture layer is
    // loaded. See src/Hooks/RenderDocBridge.cpp for the log line that identified it.
    cvr::RenderDocAllowNvApi();
    // A/B SWITCH FOR ONE QUESTION: is our factory wrapper what blinds RenderDoc?
    //
    // With a capture layer resident the game still reports "API None" and its capture buttons stay dead,
    // even after RenderDoc was taught to hook Streamline's interposer exports. The remaining suspect is
    // this very hook: we intercept the interposer's CreateDXGIFactory*, hand the game OUR wrapper, and it
    // forwards to the interposer's factory. If RenderDoc did not get to wrap that factory first, it never
    // sees a swapchain or a Present, and "no graphics API" is exactly what that looks like.
    //
    // Set CPVR_NO_FACTORY_HOOKS=1 in the launch environment to leave the factory alone. Stereo and the
    // mirror window depend on this wrapper, so that run is a diagnostic and not a playable build -- but
    // it answers the question in one launch instead of by argument. Default behaviour is unchanged.
    // ...AND IT ARMS ITSELF WHEN A CAPTURE LAYER IS IN THE PROCESS.
    //
    // Two wrappers over one DXGI factory is not a preference, it is a crash. Launched under Nsight the
    // game died the moment a capture was taken, entirely inside the capture layer:
    //
    //     0xC0000005, read at 0x280
    //     FAULT  ngfx-capture-interception.dll +0x4184FE
    //     stack  ngfx-capture-interception -> nvwgf2umx -> D3D12Core, not one frame of the exe
    //
    // A null dereference at a small offset is that layer meeting an object whose creation it never
    // intercepted -- which is what happens when we hand the game OUR factory wrapper before it has
    // wrapped the real one. So the switch added as a RenderDoc experiment becomes the default whenever
    // such a layer is resident: leave the factory alone and let the capture work.
    //
    // Stereo and the mirror window depend on that wrapper, so a capture run is a diagnostic run, and
    // the log says so. That is the trade asked for: the render has to go, the stereo need not.
    //
    // The variable still overrides both ways: 1 forces the hooks off with no layer present, 0 forces
    // them on with one.
    static const wchar_t* const kCaptureLayers[] = {
        L"ngfx-capture-interception.dll",   // Nsight Graphics, the frame interceptor
        L"ngfx-capture-injection.dll",      // ...and its injector, which loads first
    };
    // RENDERDOC IS DELIBERATELY NOT IN THAT LIST.
    //
    // The note in docs/memory for the RenderDoc setup records CPVR_NO_FACTORY_HOOKS=1 as required
    // there, established by A/B -- API None with the wrapper, D3D12 without. But the user reports
    // RenderDoc runs working with the variable unset, and no launcher here sets it, so on their machine
    // the wrapper is evidently not blinding it. Nsight is the case with a crash dump behind it, so it
    // is the case that arms itself; RenderDoc keeps the variable, which is one word to type on the runs
    // where it turns out to be needed. Auto-disabling stereo for a configuration that already worked
    // would be a regression bought with a guess.
    const wchar_t* capture = nullptr;
    for (const wchar_t* m : kCaptureLayers) {
        if (GetModuleHandleW(m)) { capture = m; break; }
    }

    char noFactory[8] = {};
    const bool haveVar =
        GetEnvironmentVariableA("CPVR_NO_FACTORY_HOOKS", noFactory, sizeof(noFactory)) > 0;
    // THE DETECTION ONLY WARNS; IT DOES NOT DISARM ANYTHING.
    //
    // Skipping the factory hooks does let a capture through, and it was briefly made automatic --
    // wrongly. That wrapper is what stereo, the mirror and the forced FOV are built on, so a run
    // without it is not the run worth capturing: the frame would not be one the port shaped, which
    // answers nothing about the port.
    //
    // So the variable stays the only thing that disarms them. The line below exists to name the layer
    // in the log, because the crash is INSIDE that layer -- ngfx-capture-interception +0x4184FE,
    // reading 0x280, not one frame of the exe -- and knowing which layer was resident is the first
    // thing anyone reading such a dump needs.
    const bool skip = haveVar && noFactory[0] == '1';
    if (skip) {
        Log("PluginBootstrap: factory exports left unhooked (%s) -- stereo and the mirror will not "
            "work, the render and the capture will. capture layer: %ls\n",
            haveVar ? "CPVR_NO_FACTORY_HOOKS=1" : "capture layer detected",
            capture ? capture : L"none");
        return;
    }
    if (capture) {
        Log("PluginBootstrap: capture layer %ls is resident and the factory hooks are ON -- "
            "stereo, the mirror and the forced FOV work; a capture from Nsight has been seen to "
            "crash inside that layer.\n",
            capture);
    }
    // Both the real DXGI and Streamline's interposer: with frame generation the game talks to
    // the interposer's exports, not dxgi's, and the swapchain it gets back is a Streamline
    // proxy whose vtable lives there.
    HookFactoryExportsIn(L"dxgi.dll");
    HookFactoryExportsIn(L"sl.interposer.dll");
}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSoftwareAdapter(HMODULE Module, IDXGIAdapter** ppAdapter) { return m_real->CreateSoftwareAdapter(Module, ppAdapter); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapters1(UINT Adapter, IDXGIAdapter1** ppAdapter) {
    HRESULT hr = m_real->EnumAdapters1(Adapter, ppAdapter);
    if (SUCCEEDED(hr) && ppAdapter && *ppAdapter) {
        InstallAdapterHook(*ppAdapter);
    }
    return hr;
}
BOOL STDMETHODCALLTYPE DXGIFactoryWrapper::IsCurrent() { return m_real->IsCurrent(); }
BOOL STDMETHODCALLTYPE DXGIFactoryWrapper::IsWindowedStereoEnabled() { return m_real->IsWindowedStereoEnabled(); }

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChainForHwnd(IUnknown* pDevice, HWND hWnd, const DXGI_SWAP_CHAIN_DESC1* pDesc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* pFullscreenDesc, IDXGIOutput* pRestrictToOutput, IDXGISwapChain1** ppSwapChain) {
    Log("CreateSwapChainForHwnd intercepted! pDevice=%p\n", pDevice);
    g_gameHwnd = hWnd;
    PrepareStartupLiveControls();

    DXGI_SWAP_CHAIN_DESC1 localDesc{};
    const DXGI_SWAP_CHAIN_DESC1* swapDesc = pDesc;
    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (pDesc && forcedWidth != 0 && forcedHeight != 0) {
        localDesc = *pDesc;
        localDesc.Width = forcedWidth;
        localDesc.Height = forcedHeight;
        localDesc.Scaling = DXGI_SCALING_STRETCH;
        swapDesc = &localDesc;
        Log("CreateSwapChainForHwnd override: %ux%u -> %ux%u\n",
            pDesc->Width,
            pDesc->Height,
            forcedWidth,
            forcedHeight);
    }

    DXGI_SWAP_CHAIN_FULLSCREEN_DESC localFsDesc{};
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fsDesc = pFullscreenDesc;
    if (pFullscreenDesc && !pFullscreenDesc->Windowed) {
        localFsDesc = *pFullscreenDesc;
        localFsDesc.Windowed = TRUE;
        fsDesc = &localFsDesc;
        Log("CreateSwapChainForHwnd: Forced Windowed=TRUE\n");
    }

    UINT windowWidth = GetForcedWindowWidth();
    UINT windowHeight = GetForcedWindowHeight();
    if (windowWidth == forcedWidth) {
        windowWidth = 0; // If they are the same as swapchain, user wants auto-cap
    }
    if (windowWidth == 0 && hWnd) {
        HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTOPRIMARY);
        MONITORINFO mi = { sizeof(MONITORINFO) };
        if (GetMonitorInfoA(monitor, &mi)) {
            int monWidth = mi.rcMonitor.right - mi.rcMonitor.left;
            int monHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;
            windowWidth = monWidth;
            windowHeight = monHeight;
        }
    }
    if (windowWidth != 0 && windowHeight != 0 && hWnd) {
        SetWindowPos(hWnd, nullptr, 0, 0, windowWidth, windowHeight, SWP_NOMOVE | SWP_NOZORDER);
        Log("CreateSwapChainForHwnd: Capped window size to %ux%u\n", windowWidth, windowHeight);
    }

    ID3D12CommandQueue* pQueue = nullptr;
    if (SUCCEEDED(pDevice->QueryInterface(IID_PPV_ARGS(&pQueue)))) {
        ID3D12Device* d3dDevice = nullptr;
        if (SUCCEEDED(pQueue->GetDevice(IID_PPV_ARGS(&d3dDevice)))) {
            RememberDredDevice(d3dDevice);
            OpenXRManager::Get().InitGraphics(d3dDevice, pQueue);
            OverlaySetDeviceAndQueue(d3dDevice, pQueue);
            InstallCommandQueueDiagHook(pQueue);
            InstallDepthCaptureHooks(d3dDevice);
            // The device here came from the queue the GAME passed, so it is the game-facing one --
            // wrapped when a capture layer is present, which is what makes the port markers reachable.
            // See the note at RegisterGameFacingListVtable; a no-op in an ordinary session, and it
            // self-guards so calling it from every swapchain path costs nothing.
            RegisterGameFacingListVtable(d3dDevice);
            d3dDevice->Release();
        }
        pQueue->Release();
    }
    const HRESULT hr = m_real->CreateSwapChainForHwnd(pDevice, hWnd, swapDesc, fsDesc, pRestrictToOutput, ppSwapChain);
    if (SUCCEEDED(hr) && ppSwapChain && *ppSwapChain) {
        if (!IsOurOwnWindow(hWnd)) {
            OverlaySetWindow(hWnd);
            InstallSwapchainHooks(*ppSwapChain);
            InstallOSHooks();
        }
    }
    return hr;
}

HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChainForCoreWindow(IUnknown* pDevice, IUnknown* pWindow, const DXGI_SWAP_CHAIN_DESC1* pDesc, IDXGIOutput* pRestrictToOutput, IDXGISwapChain1** ppSwapChain) { return m_real->CreateSwapChainForCoreWindow(pDevice, pWindow, pDesc, pRestrictToOutput, ppSwapChain); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::GetSharedResourceAdapterLuid(HANDLE hResource, LUID* pLuid) { return m_real->GetSharedResourceAdapterLuid(hResource, pLuid); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterStereoStatusWindow(HWND WindowHandle, UINT wMsg, DWORD* pdwCookie) { return m_real->RegisterStereoStatusWindow(WindowHandle, wMsg, pdwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterStereoStatusEvent(HANDLE hEvent, DWORD* pdwCookie) { return m_real->RegisterStereoStatusEvent(hEvent, pdwCookie); }
void STDMETHODCALLTYPE DXGIFactoryWrapper::UnregisterStereoStatus(DWORD dwCookie) { m_real->UnregisterStereoStatus(dwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterOcclusionStatusWindow(HWND WindowHandle, UINT wMsg, DWORD* pdwCookie) { return m_real->RegisterOcclusionStatusWindow(WindowHandle, wMsg, pdwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterOcclusionStatusEvent(HANDLE hEvent, DWORD* pdwCookie) { return m_real->RegisterOcclusionStatusEvent(hEvent, pdwCookie); }
void STDMETHODCALLTYPE DXGIFactoryWrapper::UnregisterOcclusionStatus(DWORD dwCookie) { m_real->UnregisterOcclusionStatus(dwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CreateSwapChainForComposition(IUnknown* pDevice, const DXGI_SWAP_CHAIN_DESC1* pDesc, IDXGIOutput* pRestrictToOutput, IDXGISwapChain1** ppSwapChain) { return m_real->CreateSwapChainForComposition(pDevice, pDesc, pRestrictToOutput, ppSwapChain); }
UINT STDMETHODCALLTYPE DXGIFactoryWrapper::GetCreationFlags() { return m_real->GetCreationFlags(); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapterByLuid(LUID AdapterLuid, REFIID riid, void** ppvAdapter) {
    HRESULT hr = m_real->EnumAdapterByLuid(AdapterLuid, riid, ppvAdapter);
    if (SUCCEEDED(hr) && ppvAdapter && *ppvAdapter) {
        InstallAdapterHook(reinterpret_cast<IDXGIAdapter*>(*ppvAdapter));
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumWarpAdapter(REFIID riid, void** ppvAdapter) { return m_real->EnumWarpAdapter(riid, ppvAdapter); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::CheckFeatureSupport(DXGI_FEATURE Feature, void* pFeatureSupportData, UINT FeatureSupportDataSize) { return m_real->CheckFeatureSupport(Feature, pFeatureSupportData, FeatureSupportDataSize); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::EnumAdapterByGpuPreference(UINT Adapter, DXGI_GPU_PREFERENCE GpuPreference, REFIID riid, void** ppvAdapter) {
    HRESULT hr = m_real->EnumAdapterByGpuPreference(Adapter, GpuPreference, riid, ppvAdapter);
    if (SUCCEEDED(hr) && ppvAdapter && *ppvAdapter) {
        InstallAdapterHook(reinterpret_cast<IDXGIAdapter*>(*ppvAdapter));
    }
    return hr;
}
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::RegisterAdaptersChangedEvent(HANDLE hEvent, DWORD* pdwCookie) { return m_real->RegisterAdaptersChangedEvent(hEvent, pdwCookie); }
HRESULT STDMETHODCALLTYPE DXGIFactoryWrapper::UnregisterAdaptersChangedEvent(DWORD dwCookie) { return m_real->UnregisterAdaptersChangedEvent(dwCookie); }

// [DEPTH] Accessors for the submit path (openxr_manager) to snapshot the game's
// scene depth with the correct (observed) resource state.
extern "C" ID3D12Resource* OmoGetSceneDepthResource() { return g_sceneDepthRes.load(std::memory_order_relaxed); }
extern "C" ID3D12CommandQueue* OmoGetSceneDepthWriterQueue() { return g_sceneDepthWriterQueue.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthState() { return g_sceneDepthState.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthWidth() { return g_sceneDepthW.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthHeight() { return g_sceneDepthH.load(std::memory_order_relaxed); }
extern "C" unsigned int OmoGetSceneDepthFormat() { return g_sceneDepthFmt.load(std::memory_order_relaxed); }
