#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// DisplayModes -- making the VR resolution appear in the game's own mode list.
//
// The engine will not render at a resolution its enumeration does not contain, and the headset's
// per-eye size is not a desktop mode. So GetDisplayModeList is answered with the real list PLUS the one
// we need, which is the smallest lie that gets the engine to agree.
//
// THE LIST IS QUERIED TWICE, and both calls must agree. The first asks how many modes there are with a
// null buffer, the second asks for them. Adding an entry to the second without counting it in the first
// is a buffer overrun in the caller, so HasMode/HasMode1 check before either.
//
// The adapter and output hooks exist only to reach the outputs: an IDXGIOutput is obtained from an
// adapter, and there is no other point at which one can be intercepted.

#if CVR_RUST_VRIK
// The Rust build's hooks are rust/crates/cvr-gpu/src/display_modes.rs; the names Hooks/SwapChain.cpp calls forward there.
#include "Hooks/SwapChainInternal.hpp"
#else
#include <MinHook.h>
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

extern void Log(const char* fmt, ...);
extern volatile int g_verboseLog; // gate per-frame spam (ClipCursor / depth-diag)
extern "C" void PrepareStartupLiveControls();
extern "C" int CyberpunkVR_IsMainViewRecording();
extern "C" int CyberpunkVR_ViewKeyHookActive();
extern "C" void CyberpunkVR_ProfPublish();
extern "C" UINT GetForcedSwapchainWidth();
extern "C" UINT GetForcedSwapchainHeight();
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();
extern "C" UINT GetForcedWindowWidth();
extern "C" UINT GetForcedWindowHeight();
extern "C" int GetMenuMode();

static bool HasMode(const DXGI_MODE_DESC* modes, UINT count, UINT width, UINT height) {
    if (!modes) return false;
    for (UINT i = 0; i < count; ++i) {
        if (modes[i].Width == width && modes[i].Height == height) {
            return true;
        }
    }
    return false;
}

static bool HasMode1(const DXGI_MODE_DESC1* modes, UINT count, UINT width, UINT height) {
    if (!modes) return false;
    for (UINT i = 0; i < count; ++i) {
        if (modes[i].Width == width && modes[i].Height == height) {
            return true;
        }
    }
    return false;
}

static void FillMode(DXGI_MODE_DESC& mode, DXGI_FORMAT format, UINT width, UINT height) {
    mode.Width = width;
    mode.Height = height;
    mode.RefreshRate.Numerator = 90;
    mode.RefreshRate.Denominator = 1;
    mode.Format = format;
    mode.ScanlineOrdering = DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED;
    mode.Scaling = DXGI_MODE_SCALING_UNSPECIFIED;
}

static void FillMode1(DXGI_MODE_DESC1& mode, DXGI_FORMAT format, UINT width, UINT height) {
    mode.Width = width;
    mode.Height = height;
    mode.RefreshRate.Numerator = 90;
    mode.RefreshRate.Denominator = 1;
    mode.Format = format;
    mode.ScanlineOrdering = DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED;
    mode.Scaling = DXGI_MODE_SCALING_UNSPECIFIED;
    mode.Stereo = FALSE;
}

HRESULT STDMETHODCALLTYPE HookedGetDisplayModeList(IDXGIOutput* output, DXGI_FORMAT EnumFormat, UINT Flags, UINT* pNumModes, DXGI_MODE_DESC* pDesc) {
    void** vtable = *reinterpret_cast<void***>(output);
    GetDisplayModeListFn originalFn = GetOriginalMethod<GetDisplayModeListFn>(vtable, 8);
    
    if (!pNumModes) {
        return originalFn ? originalFn(output, EnumFormat, Flags, pNumModes, pDesc) : DXGI_ERROR_INVALID_CALL;
    }

    const UINT capacity = pDesc ? *pNumModes : 0;
    UINT originalCount = *pNumModes;
    HRESULT hr = originalFn ? originalFn(output, EnumFormat, Flags, &originalCount, pDesc) : DXGI_ERROR_INVALID_CALL;
    if (FAILED(hr) && hr != DXGI_ERROR_MORE_DATA) {
        *pNumModes = originalCount;
        return hr;
    }

    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (forcedWidth == 0 || forcedHeight == 0 || HasMode(pDesc, originalCount, forcedWidth, forcedHeight)) {
        *pNumModes = originalCount;
        return hr == DXGI_ERROR_MORE_DATA ? hr : S_OK;
    }

    const UINT requiredCount = originalCount + 1;
    if (!pDesc) {
        *pNumModes = requiredCount;
        return S_OK;
    }

    if (capacity <= originalCount || hr == DXGI_ERROR_MORE_DATA) {
        *pNumModes = requiredCount;
        return DXGI_ERROR_MORE_DATA;
    }

    FillMode(pDesc[originalCount], EnumFormat, forcedWidth, forcedHeight);
    *pNumModes = requiredCount;
    return S_OK;
}

HRESULT STDMETHODCALLTYPE HookedGetDisplayModeList1(IDXGIOutput1* output, DXGI_FORMAT EnumFormat, UINT Flags, UINT* pNumModes, DXGI_MODE_DESC1* pDesc) {
    void** vtable = *reinterpret_cast<void***>(output);
    GetDisplayModeList1Fn originalFn = GetOriginalMethod<GetDisplayModeList1Fn>(vtable, 19);

    if (!pNumModes) {
        return originalFn ? originalFn(output, EnumFormat, Flags, pNumModes, pDesc) : DXGI_ERROR_INVALID_CALL;
    }

    const UINT capacity = pDesc ? *pNumModes : 0;
    UINT originalCount = *pNumModes;
    HRESULT hr = originalFn ? originalFn(output, EnumFormat, Flags, &originalCount, pDesc) : DXGI_ERROR_INVALID_CALL;
    if (FAILED(hr) && hr != DXGI_ERROR_MORE_DATA) {
        *pNumModes = originalCount;
        return hr;
    }

    const UINT forcedWidth = GetForcedDisplayModeWidth();
    const UINT forcedHeight = GetForcedDisplayModeHeight();
    if (forcedWidth == 0 || forcedHeight == 0 || HasMode1(pDesc, originalCount, forcedWidth, forcedHeight)) {
        *pNumModes = originalCount;
        return hr == DXGI_ERROR_MORE_DATA ? hr : S_OK;
    }

    const UINT requiredCount = originalCount + 1;
    if (!pDesc) {
        *pNumModes = requiredCount;
        return S_OK;
    }

    if (capacity <= originalCount || hr == DXGI_ERROR_MORE_DATA) {
        *pNumModes = requiredCount;
        return DXGI_ERROR_MORE_DATA;
    }

    FillMode1(pDesc[originalCount], EnumFormat, forcedWidth, forcedHeight);
    *pNumModes = requiredCount;
    return S_OK;
}

void InstallOutputHook(IDXGIOutput* output) {
    if (!output) return;
    void*** objectVtable = reinterpret_cast<void***>(output);
    if (!objectVtable || !*objectVtable) return;
    void** vtable = *objectVtable;
    PatchVtableMethod(vtable, 8, reinterpret_cast<void*>(&HookedGetDisplayModeList));

    IDXGIOutput1* output1 = nullptr;
    if (SUCCEEDED(output->QueryInterface(IID_PPV_ARGS(&output1))) && output1) {
        void** vtable1 = *reinterpret_cast<void***>(output1);
        PatchVtableMethod(vtable1, 19, reinterpret_cast<void*>(&HookedGetDisplayModeList1));
        output1->Release();
    }
}

HRESULT STDMETHODCALLTYPE HookedEnumOutputs(IDXGIAdapter* adapter, UINT Output, IDXGIOutput** ppOutput) {
    void** vtable = *reinterpret_cast<void***>(adapter);
    EnumOutputsFn originalFn = GetOriginalMethod<EnumOutputsFn>(vtable, 7);

    HRESULT hr = originalFn ? originalFn(adapter, Output, ppOutput) : DXGI_ERROR_INVALID_CALL;
    if (SUCCEEDED(hr) && ppOutput && *ppOutput) {
        InstallOutputHook(*ppOutput);
    }
    return hr;
}

void InstallAdapterHook(IDXGIAdapter* adapter) {
    if (!adapter) return;
    void*** objectVtable = reinterpret_cast<void***>(adapter);
    if (!objectVtable || !*objectVtable) return;
    void** vtable = *objectVtable;
    PatchVtableMethod(vtable, 7, reinterpret_cast<void*>(&HookedEnumOutputs));
}
#endif
#endif
