// OsInput -- the Win32 calls the game makes about the cursor and the window, answered for VR.
//
// In a headset there is no desktop cursor to speak of, but the game asks about one constantly, and its
// answers decide where it thinks the mouse is, whether it should clip the pointer to the client area,
// and how large the client area is. Left alone, those answers are about a window that is not what the
// player is looking at.
//
// HOOKED BY IAT, not by detour. These are imports in the game's own module, so rewriting the import
// entry is both cheaper and reversible, and it leaves the system DLLs untouched -- which matters
// because an overlay or a capture tool may be patching the same functions the other way.
//
// A function this file cannot find in the import table is reported and skipped, never faked. The log
// lines about MoveWindow and GetMessagePos are that path, and they are harmless: the game does not call
// what it did not import.

#include <MinHook.h>
#include "Utils/DebugGate.hpp"
#include "Hooks/SwapChainInternal.hpp"
#include "Render/DesktopMirror.hpp"
#include <algorithm>
#include <cmath>
#include <thread>
#include "Hooks/SwapChain.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Lua/Host.hpp"
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

static bool IsGameWindowForeground(HWND hwnd) {
    if (!hwnd || !IsWindow(hwnd)) return false;

    HWND foreground = GetForegroundWindow();
    if (!foreground) return false;
    if (foreground == hwnd || GetAncestor(foreground, GA_ROOT) == hwnd) {
        return true;
    }

    DWORD foregroundPid = 0;
    GetWindowThreadProcessId(foreground, &foregroundPid);
    return foregroundPid == GetCurrentProcessId();
}

static bool HookIAT(HMODULE hMod, const char* dllName, const char* funcName, void* newFunc, void** oldFunc) {
    if (!hMod) {
        Log("HookIAT: Invalid module handle\n");
        return false;
    }

    PIMAGE_DOS_HEADER dosHeader = reinterpret_cast<PIMAGE_DOS_HEADER>(hMod);
    if (dosHeader->e_magic != IMAGE_DOS_SIGNATURE) {
        Log("HookIAT: Invalid DOS signature for module %p\n", hMod);
        return false;
    }

    PIMAGE_NT_HEADERS ntHeaders = reinterpret_cast<PIMAGE_NT_HEADERS>(reinterpret_cast<BYTE*>(hMod) + dosHeader->e_lfanew);
    if (ntHeaders->Signature != IMAGE_NT_SIGNATURE) {
        Log("HookIAT: Invalid NT signature for module %p\n", hMod);
        return false;
    }

    DWORD importDirRVA = ntHeaders->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    if (!importDirRVA) {
        Log("HookIAT: No import directory for module %p\n", hMod);
        return false;
    }

    PIMAGE_IMPORT_DESCRIPTOR importDesc = reinterpret_cast<PIMAGE_IMPORT_DESCRIPTOR>(reinterpret_cast<BYTE*>(hMod) + importDirRVA);
    bool dllFound = false;

    while (importDesc->Name) {
        const char* name = reinterpret_cast<const char*>(reinterpret_cast<BYTE*>(hMod) + importDesc->Name);
        if (_stricmp(name, dllName) == 0) {
            dllFound = true;
            PIMAGE_THUNK_DATA thunk = reinterpret_cast<PIMAGE_THUNK_DATA>(reinterpret_cast<BYTE*>(hMod) + importDesc->FirstThunk);
            PIMAGE_THUNK_DATA origThunk = reinterpret_cast<PIMAGE_THUNK_DATA>(reinterpret_cast<BYTE*>(hMod) + importDesc->OriginalFirstThunk);
            
            while (origThunk->u1.AddressOfData) {
                if (!(origThunk->u1.Ordinal & IMAGE_ORDINAL_FLAG)) {
                    PIMAGE_IMPORT_BY_NAME importByName = reinterpret_cast<PIMAGE_IMPORT_BY_NAME>(reinterpret_cast<BYTE*>(hMod) + origThunk->u1.AddressOfData);
                    if (strcmp(reinterpret_cast<const char*>(importByName->Name), funcName) == 0) {
                        // Second line of defence against the self-call above: if the slot
                        // already holds our detour, capturing it as "the original" would make
                        // the detour call itself for ever. Leave both alone.
                        if (thunk->u1.Function == reinterpret_cast<uintptr_t>(newFunc)) {
                            Log("HookIAT: '%s!%s' in module %p already hooked -- left as is\n",
                                dllName, funcName, hMod);
                            return true;
                        }
                        DWORD oldProtect;
                        VirtualProtect(&thunk->u1.Function, sizeof(uintptr_t), PAGE_EXECUTE_READWRITE, &oldProtect);
                        if (oldFunc) {
                            *oldFunc = reinterpret_cast<void*>(thunk->u1.Function);
                        }
                        thunk->u1.Function = reinterpret_cast<uintptr_t>(newFunc);
                        VirtualProtect(&thunk->u1.Function, sizeof(uintptr_t), oldProtect, &oldProtect);
                        Log("HookIAT: Successfully hooked '%s!%s' in module %p (original=%p, hooked=%p)\n", dllName, funcName, hMod, oldFunc ? *oldFunc : nullptr, newFunc);
                        return true;
                    }
                }
                thunk++;
                origThunk++;
            }
        }
        importDesc++;
    }

    Log("HookIAT: Failed to find function '%s' in DLL '%s' for module %p%s\n", funcName, dllName, hMod, dllFound ? "" : " (DLL import descriptor not found)");
    return false;
}

using GetCursorPosFn = BOOL(WINAPI*)(LPPOINT);
using SetCursorPosFn = BOOL(WINAPI*)(int, int);
using SetWindowPosFn = BOOL(WINAPI*)(HWND, HWND, int, int, int, int, UINT);
using MoveWindowFn = BOOL(WINAPI*)(HWND, int, int, int, int, BOOL);
using GetClientRectFn = BOOL(WINAPI*)(HWND, LPRECT);
using GetWindowRectFn = BOOL(WINAPI*)(HWND, LPRECT);
using GetCursorInfoFn = BOOL(WINAPI*)(PCURSORINFO);
using ClipCursorFn = BOOL(WINAPI*)(const RECT*);
using GetSystemMetricsFn = int(WINAPI*)(int);
using GetMessagePosFn = DWORD(WINAPI*)(VOID);

static GetCursorPosFn g_origGetCursorPos = nullptr;
static SetCursorPosFn g_origSetCursorPos = nullptr;
static SetWindowPosFn g_origSetWindowPos = nullptr;
static MoveWindowFn g_origMoveWindow = nullptr;
static GetClientRectFn g_origGetClientRect = nullptr;
static GetWindowRectFn g_origGetWindowRect = nullptr;
static GetCursorInfoFn g_origGetCursorInfo = nullptr;
static ClipCursorFn g_origClipCursor = nullptr;
static GetSystemMetricsFn g_origGetSystemMetrics = nullptr;
static GetMessagePosFn g_origGetMessagePos = nullptr;
HWND g_gameHwnd = nullptr;

// ---- the desktop cover transform (TofuExpress fork, 2026-10-16) ---------------------------------
//
// The game lives in the VIRTUAL client: the render size, which GetClientRect reports. The window shows
// the centre crop of it at the window's aspect (Render/DesktopMirror.hpp), stretched over the physical
// client. Every position crossing between the two goes through the same crop, so the game's idea of
// the cursor and the cursor on the screen agree -- the old per-axis scale only matched while the window
// had the render's aspect, which no monitor does with a tall headset render.

// Our own cursor clip, in physical coordinates. Through ClipCursor it would hit our IAT hook (this
// module is hooked too) and be mapped a second time.
static BOOL SetPhysicalCursorClip(const RECT* rect) {
    static const auto clipCursor = reinterpret_cast<ClipCursorFn>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "ClipCursor"));
    return clipCursor ? clipCursor(rect) : FALSE;
}

struct DesktopCursorTransform {
    DesktopCoverTransform cover;
    POINT clientOrigin{};
    int clientWidth = 0;
    int clientHeight = 0;
};

static bool GetDesktopCursorTransform(DesktopCursorTransform& out) {
    if (!g_gameHwnd) return false;
    const UINT virtualWidth = GetForcedDisplayModeWidth();
    const UINT virtualHeight = GetForcedDisplayModeHeight();
    RECT client{};
    if (!virtualWidth || !virtualHeight ||
        !GetDesktopPhysicalClientRect(g_gameHwnd, client, out.clientOrigin)) {
        return false;
    }
    out.clientWidth = client.right - client.left;
    out.clientHeight = client.bottom - client.top;
    return out.clientWidth > 0 && out.clientHeight > 0 &&
           ComputeDesktopCoverTransform(virtualWidth, virtualHeight,
                                        static_cast<UINT>(out.clientWidth),
                                        static_cast<UINT>(out.clientHeight), out.cover);
}

// Screen point on the physical client -> screen point in the virtual client (same origin).
static POINT PhysicalToVirtualScreen(POINT point, const DesktopCursorTransform& transform) {
    const double x = transform.cover.sourceX +
        static_cast<double>(point.x - transform.clientOrigin.x) * transform.cover.sourceWidth /
            transform.clientWidth;
    const double y = transform.cover.sourceY +
        static_cast<double>(point.y - transform.clientOrigin.y) * transform.cover.sourceHeight /
            transform.clientHeight;
    return {
        transform.clientOrigin.x + static_cast<LONG>(std::lround(x)),
        transform.clientOrigin.y + static_cast<LONG>(std::lround(y))
    };
}

// And back, clamped into the client: a virtual point outside the crop has no place on the screen.
static POINT VirtualToPhysicalScreen(POINT point, const DesktopCursorTransform& transform) {
    double x = (static_cast<double>(point.x - transform.clientOrigin.x) - transform.cover.sourceX) *
        transform.clientWidth / transform.cover.sourceWidth;
    double y = (static_cast<double>(point.y - transform.clientOrigin.y) - transform.cover.sourceY) *
        transform.clientHeight / transform.cover.sourceHeight;
    x = (std::max)(0.0, (std::min)(static_cast<double>(transform.clientWidth - 1), x));
    y = (std::max)(0.0, (std::min)(static_cast<double>(transform.clientHeight - 1), y));
    return {
        transform.clientOrigin.x + static_cast<LONG>(std::lround(x)),
        transform.clientOrigin.y + static_cast<LONG>(std::lround(y))
    };
}

// A clip-rectangle edge: like VirtualToPhysicalScreen, but the right/bottom edge may be the client's
// own (exclusive) edge.
static LONG VirtualEdgeToPhysical(LONG value, bool horizontal, const DesktopCursorTransform& transform) {
    const double origin = horizontal ? transform.clientOrigin.x : transform.clientOrigin.y;
    const double cropOrigin = horizontal ? transform.cover.sourceX : transform.cover.sourceY;
    const double cropSize = horizontal ? transform.cover.sourceWidth : transform.cover.sourceHeight;
    const double destinationSize = horizontal ? transform.clientWidth : transform.clientHeight;
    double mapped = (static_cast<double>(value) - origin - cropOrigin) * destinationSize / cropSize;
    mapped = (std::max)(0.0, (std::min)(destinationSize, mapped));
    return static_cast<LONG>(std::lround(origin + mapped));
}

// The last GetCursorPos answer, diagnostics only: physical screen x,y, then the virtual x,y returned.
extern "C" __declspec(dllexport) volatile LONG CyberpunkVR_DebugCursorMap[4] = {};

static BOOL WINAPI HookedGetCursorPos(LPPOINT lpPoint) {
    BOOL res = g_origGetCursorPos ? g_origGetCursorPos(lpPoint) : FALSE;
    if(IsLauncherOpen())return res;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 1000 == 0) {
        Log("HookedGetCursorPos: called %d times. lpPoint=%p, pos=(%ld,%ld) g_gameHwnd=%p\n",
            callCount, lpPoint, lpPoint ? lpPoint->x : 0, lpPoint ? lpPoint->y : 0, g_gameHwnd);
    }
    if (res && lpPoint && g_gameHwnd) {
        DesktopCursorTransform transform{};
        if (GetDesktopCursorTransform(transform)) {
            const POINT physical = *lpPoint;
            *lpPoint = PhysicalToVirtualScreen(physical, transform);
            CVR_DIAGNOSTIC(CyberpunkVR_DebugCursorMap[0] = physical.x; CyberpunkVR_DebugCursorMap[1] = physical.y;
                           CyberpunkVR_DebugCursorMap[2] = lpPoint->x; CyberpunkVR_DebugCursorMap[3] = lpPoint->y);
        }
    }
    return res;
}

static BOOL WINAPI HookedSetCursorPos(int X, int Y) {
    // The settings panel leaves simulation running. Do not let the game's
    // relative-mouse recenter fight desktop pointing or steal the VR cursor.
    // The Lua mods' windows run over the same simulation and point with the same mouse.
    if(IsLauncherOpen() || OverlayIsVisible() || cvr::lua::OverlayOpen())return TRUE;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 100 == 0) {
        Log("HookedSetCursorPos: called %d times, target=(%d,%d) g_gameHwnd=%p\n", callCount, X, Y, g_gameHwnd);
    }
    if (g_gameHwnd) {
        DesktopCursorTransform transform{};
        if (GetDesktopCursorTransform(transform)) {
            const POINT mapped = VirtualToPhysicalScreen({X, Y}, transform);
            X = mapped.x;
            Y = mapped.y;
        }
    }
    return g_origSetCursorPos ? g_origSetCursorPos(X, Y) : FALSE;
}

static BOOL WINAPI HookedGetCursorInfo(PCURSORINFO pci) {
    BOOL res = g_origGetCursorInfo ? g_origGetCursorInfo(pci) : FALSE;
    if(IsLauncherOpen())return res;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 1000 == 0) {
        Log("HookedGetCursorInfo: called %d times. ptScreenPos=(%ld,%ld)\n",
            callCount, (res && pci) ? pci->ptScreenPos.x : 0, (res && pci) ? pci->ptScreenPos.y : 0);
    }
    if (res && pci && g_gameHwnd) {
        DesktopCursorTransform transform{};
        if (GetDesktopCursorTransform(transform)) pci->ptScreenPos = PhysicalToVirtualScreen(pci->ptScreenPos, transform);
    }
    return res;
}

static BOOL WINAPI HookedClipCursor(const RECT* lpRect) {
    if(IsLauncherOpen())return g_origClipCursor ? g_origClipCursor(nullptr) : ClipCursor(nullptr);
    RECT scaledRect{};
    const RECT* rectToUse = lpRect;
    
    if (g_verboseLog) Log("HookedClipCursor called: rect=%p (%ld,%ld)-(%ld,%ld) g_gameHwnd=%p\n",
        lpRect, lpRect ? lpRect->left : 0, lpRect ? lpRect->top : 0, lpRect ? lpRect->right : 0, lpRect ? lpRect->bottom : 0, g_gameHwnd);

    if (lpRect && g_gameHwnd) {
        int w = lpRect->right - lpRect->left;
        int h = lpRect->bottom - lpRect->top;
        if (w <= 1 && h <= 1) {
            bool isMenuVisible = (GetMenuMode() != 0) || OverlayIsVisible() || cvr::lua::OverlayOpen();
            if (isMenuVisible) {
                if (g_verboseLog) Log("HookedClipCursor: Ignored centering clip (%ld,%ld) because menu mode is active\n", lpRect->left, lpRect->top);
                return g_origClipCursor ? g_origClipCursor(nullptr) : ClipCursor(nullptr);
            }
        }

        DesktopCursorTransform transform{};
        if (GetDesktopCursorTransform(transform)) {
            scaledRect.left = VirtualEdgeToPhysical(lpRect->left, true, transform);
            scaledRect.top = VirtualEdgeToPhysical(lpRect->top, false, transform);
            scaledRect.right = VirtualEdgeToPhysical(lpRect->right, true, transform);
            scaledRect.bottom = VirtualEdgeToPhysical(lpRect->bottom, false, transform);
            rectToUse = &scaledRect;
            if (g_verboseLog) Log("ClipCursor cover map: (%ld,%ld)-(%ld,%ld) -> (%ld,%ld)-(%ld,%ld)\n",
                lpRect->left, lpRect->top, lpRect->right, lpRect->bottom,
                scaledRect.left, scaledRect.top, scaledRect.right, scaledRect.bottom);
        }
    }

    return g_origClipCursor ? g_origClipCursor(rectToUse) : ClipCursor(rectToUse);
}

static BOOL WINAPI HookedGetClientRect(HWND hWnd, LPRECT lpRect) {
    BOOL res = g_origGetClientRect ? g_origGetClientRect(hWnd, lpRect) : FALSE;
    if(IsLauncherOpen())return res;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 100 == 0) {
        Log("HookedGetClientRect: called %d times. hWnd=%p g_gameHwnd=%p\n", callCount, hWnd, g_gameHwnd);
    }
    if (res && lpRect) {
        if (hWnd && (hWnd == g_gameHwnd || (g_gameHwnd == nullptr && IsGameWindowForeground(hWnd)))) {
            UINT virtualWidth = GetForcedDisplayModeWidth();
            UINT virtualHeight = GetForcedDisplayModeHeight();
            if (virtualWidth > 0 && virtualHeight > 0) {
                lpRect->left = 0;
                lpRect->top = 0;
                lpRect->right = virtualWidth;
                lpRect->bottom = virtualHeight;
            }
        }
    }
    return res;
}

static BOOL WINAPI HookedGetWindowRect(HWND hWnd, LPRECT lpRect) {
    BOOL res = g_origGetWindowRect ? g_origGetWindowRect(hWnd, lpRect) : FALSE;
    if(IsLauncherOpen())return res;
    static int callCount = 0;
    if (g_verboseLog && callCount++ % 100 == 0) {
        Log("HookedGetWindowRect: called %d times. hWnd=%p g_gameHwnd=%p\n", callCount, hWnd, g_gameHwnd);
    }
    if (res && lpRect) {
        if (hWnd && (hWnd == g_gameHwnd || (g_gameHwnd == nullptr && IsGameWindowForeground(hWnd)))) {
            UINT virtualWidth = GetForcedDisplayModeWidth();
            UINT virtualHeight = GetForcedDisplayModeHeight();
            if (virtualWidth > 0 && virtualHeight > 0) {
                lpRect->right = lpRect->left + virtualWidth;
                lpRect->bottom = lpRect->top + virtualHeight;
            }
        }
    }
    return res;
}

// The game's window in the automatic mode (no window size of its own in the launcher) goes where
// the desktop mirror wants it: on the monitor it is on, at the render size or less. Any other size is
// fitted inside the monitor at its own aspect, centred.
static bool IsAutomaticGameWindow(HWND hWnd) {
    const UINT forcedWindowWidth = GetForcedWindowWidth();
    return hWnd && hWnd == g_gameHwnd &&
        (forcedWindowWidth == 0 || forcedWindowWidth == GetForcedDisplayModeWidth());
}

static BOOL WINAPI HookedSetWindowPos(HWND hWnd, HWND hWndInsertAfter, int X, int Y, int cx, int cy, UINT uFlags) {
    if (!(uFlags & SWP_NOSIZE)) {
        HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTONEAREST);
        MONITORINFO mi = { sizeof(MONITORINFO) };
        if (cx > 0 && cy > 0 && GetMonitorInfoA(monitor, &mi)) {
            const UINT monWidth = static_cast<UINT>(mi.rcMonitor.right - mi.rcMonitor.left);
            const UINT monHeight = static_cast<UINT>(mi.rcMonitor.bottom - mi.rcMonitor.top);
            RECT windowRect{};
            if (IsAutomaticGameWindow(hWnd) && GetDesktopMonitorWindowRect(hWnd, windowRect)) {
                cx = windowRect.right - windowRect.left;
                cy = windowRect.bottom - windowRect.top;
                X = windowRect.left;
                Y = windowRect.top;
                uFlags &= ~SWP_NOMOVE;
            } else {
                UINT fitWidth = 0, fitHeight = 0;
                FitRenderSizeInsideBounds(static_cast<UINT>(cx), static_cast<UINT>(cy),
                                          monWidth, monHeight, fitWidth, fitHeight);
                if (fitWidth != static_cast<UINT>(cx) || fitHeight != static_cast<UINT>(cy)) {
                    cx = static_cast<int>(fitWidth);
                    cy = static_cast<int>(fitHeight);
                    X = mi.rcMonitor.left + (static_cast<int>(monWidth) - cx) / 2;
                    Y = mi.rcMonitor.top + (static_cast<int>(monHeight) - cy) / 2;
                    uFlags &= ~SWP_NOMOVE;
                }
            }
        }
    }
    return g_origSetWindowPos ? g_origSetWindowPos(hWnd, hWndInsertAfter, X, Y, cx, cy, uFlags) : FALSE;
}

static BOOL WINAPI HookedMoveWindow(HWND hWnd, int X, int Y, int nWidth, int nHeight, BOOL bRepaint) {
    HMONITOR monitor = MonitorFromWindow(hWnd, MONITOR_DEFAULTTONEAREST);
    MONITORINFO mi = { sizeof(MONITORINFO) };
    if (nWidth > 0 && nHeight > 0 && GetMonitorInfoA(monitor, &mi)) {
        const UINT monWidth = static_cast<UINT>(mi.rcMonitor.right - mi.rcMonitor.left);
        const UINT monHeight = static_cast<UINT>(mi.rcMonitor.bottom - mi.rcMonitor.top);
        RECT windowRect{};
        if (IsAutomaticGameWindow(hWnd) && GetDesktopMonitorWindowRect(hWnd, windowRect)) {
            nWidth = windowRect.right - windowRect.left;
            nHeight = windowRect.bottom - windowRect.top;
            X = windowRect.left;
            Y = windowRect.top;
        } else {
            UINT fitWidth = 0, fitHeight = 0;
            FitRenderSizeInsideBounds(static_cast<UINT>(nWidth), static_cast<UINT>(nHeight),
                                      monWidth, monHeight, fitWidth, fitHeight);
            if (fitWidth != static_cast<UINT>(nWidth) || fitHeight != static_cast<UINT>(nHeight)) {
                nWidth = static_cast<int>(fitWidth);
                nHeight = static_cast<int>(fitHeight);
                X = mi.rcMonitor.left + (static_cast<int>(monWidth) - nWidth) / 2;
                Y = mi.rcMonitor.top + (static_cast<int>(monHeight) - nHeight) / 2;
            }
        }
    }
    return g_origMoveWindow ? g_origMoveWindow(hWnd, X, Y, nWidth, nHeight, bRepaint) : FALSE;
}

static int WINAPI HookedGetSystemMetrics(int nIndex) {
    int res = g_origGetSystemMetrics ? g_origGetSystemMetrics(nIndex) : 0;
    if(IsLauncherOpen())return res;
    
    UINT virtualWidth = GetForcedDisplayModeWidth();
    UINT virtualHeight = GetForcedDisplayModeHeight();
    
    if (virtualWidth > 0 && virtualHeight > 0) {
        // SM_CXSCREEN = 0, SM_CYSCREEN = 1, SM_CXFULLSCREEN = 16, SM_CYFULLSCREEN = 17, SM_CXVIRTUALSCREEN = 78, SM_CYVIRTUALSCREEN = 79
        if (nIndex == SM_CXSCREEN || nIndex == SM_CXFULLSCREEN || nIndex == SM_CXVIRTUALSCREEN) {
            if (g_verboseLog) Log("GetSystemMetrics(%d) override: %d -> %d\n", nIndex, res, virtualWidth);
            return virtualWidth;
        }
        if (nIndex == SM_CYSCREEN || nIndex == SM_CYFULLSCREEN || nIndex == SM_CYVIRTUALSCREEN) {
            if (g_verboseLog) Log("GetSystemMetrics(%d) override: %d -> %d\n", nIndex, res, virtualHeight);
            return virtualHeight;
        }
    }
    return res;
}

static DWORD WINAPI HookedGetMessagePos(VOID) {
    DWORD res = g_origGetMessagePos ? g_origGetMessagePos() : 0;
    if(IsLauncherOpen())return res;
    if (g_gameHwnd) {
        DesktopCursorTransform transform{};
        if (GetDesktopCursorTransform(transform)) {
            const POINT source{static_cast<short>(LOWORD(res)), static_cast<short>(HIWORD(res))};
            const POINT mapped = PhysicalToVirtualScreen(source, transform);
            return MAKELONG(static_cast<WORD>(mapped.x), static_cast<WORD>(mapped.y));
        }
    }
    return res;
}

// Runs from every swapchain creation, so it must be idempotent -- and it was not.
//
// A second pass re-walks the same IATs, finds our own detour already in the slot, and stores
// THAT as "the original". The detour then calls itself, for ever: enabling the VRCAM mirror
// (which creates the process's second swapchain) died on the spot with
// EXCEPTION_STACK_OVERFLOW. Nothing here needs repeating -- the imports are patched once and
// stay patched -- so the whole function runs exactly once. HookIAT carries the same check
// again, because this is a failure mode that must not come back by another route.
void InstallOSHooks() {
    static std::atomic<bool> s_installed{false};
    bool expected = false;
    if (!s_installed.compare_exchange_strong(expected, true)) return;

    if (g_verboseLog) Log("InstallOSHooks: Installing IAT hooks for user32.dll functions...\n");
    
    HMODULE hUser32 = GetModuleHandleA("user32.dll");
    if (hUser32) {
        g_origGetCursorPos = reinterpret_cast<GetCursorPosFn>(GetProcAddress(hUser32, "GetCursorPos"));
        g_origSetCursorPos = reinterpret_cast<SetCursorPosFn>(GetProcAddress(hUser32, "SetCursorPos"));
        g_origSetWindowPos = reinterpret_cast<SetWindowPosFn>(GetProcAddress(hUser32, "SetWindowPos"));
        g_origMoveWindow = reinterpret_cast<MoveWindowFn>(GetProcAddress(hUser32, "MoveWindow"));
        g_origGetClientRect = reinterpret_cast<GetClientRectFn>(GetProcAddress(hUser32, "GetClientRect"));
        g_origGetWindowRect = reinterpret_cast<GetWindowRectFn>(GetProcAddress(hUser32, "GetWindowRect"));
        g_origGetCursorInfo = reinterpret_cast<GetCursorInfoFn>(GetProcAddress(hUser32, "GetCursorInfo"));
        g_origClipCursor = reinterpret_cast<ClipCursorFn>(GetProcAddress(hUser32, "ClipCursor"));
        g_origGetSystemMetrics = reinterpret_cast<GetSystemMetricsFn>(GetProcAddress(hUser32, "GetSystemMetrics"));
        g_origGetMessagePos = reinterpret_cast<GetMessagePosFn>(GetProcAddress(hUser32, "GetMessagePos"));
        if (g_verboseLog) Log("InstallOSHooks: Dynamically resolved all user32 original functions.\n");
    } else {
        Log("InstallOSHooks: Warning: user32.dll not found in process!\n");
    }
    
    // BOTH modules, deliberately: the game's, and our own. Our overlay asks user32 for the
    // client rect and cursor too, and it needs the same VIRTUAL answers the game gets --
    // without the self-hook the panel's mouse mapping and the mirror window break.
    HMODULE hMainExe = GetModuleHandleA(nullptr);
    HMODULE hCurrentDll = nullptr;
    GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       reinterpret_cast<LPCSTR>(InstallOSHooks), &hCurrentDll);

    HMODULE modules[] = { hMainExe, hCurrentDll };

    for (HMODULE hMod : modules) {
        if (!hMod) continue;
        if (g_verboseLog) Log("InstallOSHooks: Hooking module %p...\n", hMod);
        HookIAT(hMod, "user32.dll", "GetCursorPos", reinterpret_cast<void*>(HookedGetCursorPos), reinterpret_cast<void**>(&g_origGetCursorPos));
        HookIAT(hMod, "user32.dll", "SetCursorPos", reinterpret_cast<void*>(HookedSetCursorPos), reinterpret_cast<void**>(&g_origSetCursorPos));
        HookIAT(hMod, "user32.dll", "SetWindowPos", reinterpret_cast<void*>(HookedSetWindowPos), reinterpret_cast<void**>(&g_origSetWindowPos));
        HookIAT(hMod, "user32.dll", "MoveWindow", reinterpret_cast<void*>(HookedMoveWindow), reinterpret_cast<void**>(&g_origMoveWindow));
        HookIAT(hMod, "user32.dll", "GetClientRect", reinterpret_cast<void*>(HookedGetClientRect), reinterpret_cast<void**>(&g_origGetClientRect));
        HookIAT(hMod, "user32.dll", "GetWindowRect", reinterpret_cast<void*>(HookedGetWindowRect), reinterpret_cast<void**>(&g_origGetWindowRect));
        HookIAT(hMod, "user32.dll", "GetCursorInfo", reinterpret_cast<void*>(HookedGetCursorInfo), reinterpret_cast<void**>(&g_origGetCursorInfo));
        HookIAT(hMod, "user32.dll", "ClipCursor", reinterpret_cast<void*>(HookedClipCursor), reinterpret_cast<void**>(&g_origClipCursor));
        HookIAT(hMod, "user32.dll", "GetSystemMetrics", reinterpret_cast<void*>(HookedGetSystemMetrics), reinterpret_cast<void**>(&g_origGetSystemMetrics));
        HookIAT(hMod, "user32.dll", "GetMessagePos", reinterpret_cast<void*>(HookedGetMessagePos), reinterpret_cast<void**>(&g_origGetMessagePos));
    }

    // CET has its own Win32 ImGui backend. It gets the same virtual client and cursor as the game;
    // its mouse messages are already mapped by OverlayWndProc. (TofuExpress fork, 2026-10-16.)
    if (HMODULE hCet = GetModuleHandleA("cyber_engine_tweaks.asi")) {
        HookIAT(hCet, "user32.dll", "GetClientRect", reinterpret_cast<void*>(HookedGetClientRect), reinterpret_cast<void**>(&g_origGetClientRect));
        HookIAT(hCet, "user32.dll", "GetCursorPos", reinterpret_cast<void*>(HookedGetCursorPos), reinterpret_cast<void**>(&g_origGetCursorPos));
        HookIAT(hCet, "user32.dll", "SetCursorPos", reinterpret_cast<void*>(HookedSetCursorPos), reinterpret_cast<void**>(&g_origSetCursorPos));
    } else {
        Log("InstallOSHooks: CET module not loaded; CET cursor hooks unavailable\n");
    }
}

static bool GetClampedClientRect(HWND hwnd, RECT* outRect) {
    if (!hwnd || !outRect || !IsWindow(hwnd) || IsIconic(hwnd)) return false;

    RECT client{};
    BOOL getRectRes = g_origGetClientRect ? g_origGetClientRect(hwnd, &client) : GetClientRect(hwnd, &client);
    if (!getRectRes) return false;

    POINT topLeft{client.left, client.top};
    POINT bottomRight{client.right, client.bottom};
    if (!ClientToScreen(hwnd, &topLeft) || !ClientToScreen(hwnd, &bottomRight)) {
        return false;
    }

    RECT screenRect{topLeft.x, topLeft.y, bottomRight.x, bottomRight.y};
    HMONITOR monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    MONITORINFO monitorInfo{sizeof(MONITORINFO)};
    if (monitor && GetMonitorInfoA(monitor, &monitorInfo)) {
        RECT clipped{};
        if (IntersectRect(&clipped, &screenRect, &monitorInfo.rcMonitor)) {
            screenRect = clipped;
        } else {
            screenRect = monitorInfo.rcMonitor;
        }
    }

    if (screenRect.right <= screenRect.left || screenRect.bottom <= screenRect.top) {
        return false;
    }

    *outRect = screenRect;
    return true;
}

void UpdateCursorCapture(HWND hwnd) {
    if (IsLauncherOpen() || OverlayIsVisible()) {
        if (g_cursorClipped) {
            SetPhysicalCursorClip(nullptr);
            g_cursorClipped = false;
        }
        return;
    }

    RECT clipRect{};
    if (IsGameWindowForeground(hwnd) && GetClampedClientRect(hwnd, &clipRect)) {
        // Already physical screen coordinates: past our own ClipCursor hook, which would map them
        // as virtual ones a second time.
        SetPhysicalCursorClip(&clipRect);
        g_cursorClipped = true;
        return;
    }

    if (g_cursorClipped) {
        SetPhysicalCursorClip(nullptr);
        g_cursorClipped = false;
    }
}
