#include "Render/DesktopWindow.hpp"

#include "Core/CoreInternal.hpp"
#include "Core/LiveControls.hpp"
#include "Overlay/LiveControlsUi.hpp"

#include <algorithm>
#include <atomic>
#include <cmath>

extern void Log(const char* fmt, ...);
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();

namespace cvr::desktop_window {
namespace {

// user32's own SetWindowPos, not the IAT hook (Hooks/OsInput.cpp), which ignores size and position for this window
// in this mode.
using SetWindowPosFn = BOOL(WINAPI*)(HWND, HWND, int, int, int, int, UINT);
SetWindowPosFn OriginalSetWindowPos() {
    static auto fn = reinterpret_cast<SetWindowPosFn>(
        GetProcAddress(GetModuleHandleW(L"user32.dll"), "SetWindowPos"));
    return fn;
}

constexpr UINT_PTR kSaveTimer = 0xC7D1;   // WM_TIMER id of the debounced save
constexpr UINT kSaveDelayMs = 500;
constexpr LONG_PTR kFramedStyle = WS_OVERLAPPEDWINDOW;

UINT SetupMessage() {
    static const UINT msg = RegisterWindowMessageW(L"CyberpunkVRPort.DesktopWindow.Setup");
    return msg;
}

std::atomic<HWND> g_posted{nullptr};   // the window a setup was asked for (present path)
HWND g_window = nullptr;               // the window set up (its own thread from here on)
std::atomic<bool> g_fullscreen{false}; // also read by the cursor clip, on the present path
WINDOWPLACEMENT g_windowed{sizeof(WINDOWPLACEMENT)};   // the framed window's placement, kept while fullscreen
LONG_PTR g_windowedStyle = 0;

bool Place(HWND hwnd, HWND after, const RECT& r, UINT flags) {
    const auto setWindowPos = OriginalSetWindowPos();
    return setWindowPos && setWindowPos(hwnd, after, r.left, r.top, r.right - r.left, r.bottom - r.top, flags);
}

// The framed window's first placement: the whole frame at the render's aspect, in 70% of the monitor's work area,
// centred.
bool DefaultRect(HWND hwnd, RECT& out) {
    MONITORINFO mi{sizeof(mi)};
    const HMONITOR monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    const UINT renderWidth = GetForcedDisplayModeWidth();
    const UINT renderHeight = GetForcedDisplayModeHeight();
    if (!monitor || !GetMonitorInfoW(monitor, &mi) || !renderWidth || !renderHeight) return false;
    const double workWidth = mi.rcWork.right - mi.rcWork.left;
    const double workHeight = mi.rcWork.bottom - mi.rcWork.top;
    const double scale = 0.7 * (std::min)(workWidth / renderWidth, workHeight / renderHeight);
    const LONG width = (std::max)(320L, std::lround(renderWidth * scale));
    const LONG height = (std::max)(180L, std::lround(renderHeight * scale));
    RECT outer{0, 0, width, height};
    AdjustWindowRectEx(&outer, static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_STYLE)), FALSE,
                       static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_EXSTYLE)));
    const LONG outerWidth = outer.right - outer.left;
    const LONG outerHeight = outer.bottom - outer.top;
    out.left = mi.rcWork.left + (static_cast<LONG>(workWidth) - outerWidth) / 2;
    out.top = mi.rcWork.top + (static_cast<LONG>(workHeight) - outerHeight) / 2;
    out.right = out.left + outerWidth;
    out.bottom = out.top + outerHeight;
    return true;
}

// A remembered rectangle worth restoring: of a usable size, and on a monitor that is still there.
bool SavedRect(RECT& out) {
    const volatile int* r = g_liveControls.xrDesktopRect;
    out = RECT{r[0], r[1], r[0] + r[2], r[1] + r[3]};
    return r[2] >= 200 && r[3] >= 150 && MonitorFromRect(&out, MONITOR_DEFAULTTONULL) != nullptr;
}

// Into vrport.ini, only when something changed: placement in workspace coordinates (GetWindowPlacement's own, so it
// round-trips through SetWindowPlacement), maximised or not, fullscreen or not.
void Save(HWND hwnd) {
    WINDOWPLACEMENT wp{sizeof(wp)};
    if (g_fullscreen) {
        wp = g_windowed;
    } else if (!GetWindowPlacement(hwnd, &wp)) {
        return;
    }
    const RECT& n = wp.rcNormalPosition;
    const int rect[4] = {n.left, n.top, n.right - n.left, n.bottom - n.top};
    const int maximized = wp.showCmd == SW_SHOWMAXIMIZED ? 1 : 0;
    const int fullscreen = g_fullscreen ? 1 : 0;
    bool changed = g_liveControls.xrDesktopMaximized != maximized || g_liveControls.xrDesktopFullscreen != fullscreen;
    for (int i = 0; i < 4; ++i) changed = changed || g_liveControls.xrDesktopRect[i] != rect[i];
    if (!changed) return;
    for (int i = 0; i < 4; ++i) g_liveControls.xrDesktopRect[i] = rect[i];
    g_liveControls.xrDesktopMaximized = maximized;
    g_liveControls.xrDesktopFullscreen = fullscreen;
    PersistLiveControlsUiState(MakeLiveControlsUiState());
}

void EnterFullscreen(HWND hwnd) {
    MONITORINFO mi{sizeof(mi)};
    const HMONITOR monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if (!monitor || !GetMonitorInfoW(monitor, &mi)) return;
    g_windowed.length = sizeof(g_windowed);
    GetWindowPlacement(hwnd, &g_windowed);
    g_windowedStyle = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(hwnd, GWL_STYLE, (g_windowedStyle & ~(kFramedStyle | WS_MAXIMIZE)) | WS_POPUP);
    g_fullscreen = true;
    Place(hwnd, HWND_TOP, mi.rcMonitor, SWP_FRAMECHANGED | SWP_NOOWNERZORDER);
    Log("DesktopWindow: borderless fullscreen on monitor (%ld,%ld)-(%ld,%ld)\n", mi.rcMonitor.left, mi.rcMonitor.top,
        mi.rcMonitor.right, mi.rcMonitor.bottom);
}

void ExitFullscreen(HWND hwnd) {
    const LONG_PTR style = g_windowedStyle ? g_windowedStyle : GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(hwnd, GWL_STYLE, (style & ~(WS_POPUP | WS_MAXIMIZE)) | kFramedStyle);
    g_fullscreen = false;
    SetWindowPlacement(hwnd, &g_windowed);
    Place(hwnd, nullptr, RECT{}, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER);
    Log("DesktopWindow: back to the window\n");
}

void Setup(HWND hwnd) {
    g_window = hwnd;
    const LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(hwnd, GWL_STYLE, (style & ~WS_POPUP) | kFramedStyle);

    RECT rect{};
    if (SavedRect(rect)) {
        WINDOWPLACEMENT wp{sizeof(wp)};
        GetWindowPlacement(hwnd, &wp);
        wp.flags = 0;
        wp.rcNormalPosition = rect;
        wp.showCmd = g_liveControls.xrDesktopMaximized ? SW_SHOWMAXIMIZED : SW_SHOWNORMAL;
        SetWindowPlacement(hwnd, &wp);
        Place(hwnd, nullptr, RECT{}, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER);
        Log("DesktopWindow: free window, restored at (%ld,%ld) %ldx%ld%s\n", rect.left, rect.top,
            rect.right - rect.left, rect.bottom - rect.top, g_liveControls.xrDesktopMaximized ? ", maximised" : "");
    } else if (DefaultRect(hwnd, rect)) {
        Place(hwnd, nullptr, rect, SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOOWNERZORDER);
        Log("DesktopWindow: free window, placed at (%ld,%ld) %ldx%ld\n", rect.left, rect.top, rect.right - rect.left,
            rect.bottom - rect.top);
    }
    if (g_liveControls.xrDesktopFullscreen) EnterFullscreen(hwnd);
}

} // namespace

bool Free() {
    return g_liveControls.xrDesktopWindow != 0;
}

bool ReleasesCursor() {
    return Free() && !g_fullscreen.load(std::memory_order_relaxed);
}

void OnPresent(HWND hwnd) {
    if (!hwnd || !Free() || g_posted.exchange(hwnd) == hwnd) return;
    PostMessageW(hwnd, SetupMessage(), 0, 0);
}

bool OnMessage(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam, LRESULT* result) {
    if (!Free()) return false;
    if (msg == SetupMessage()) {
        Setup(hwnd);
        *result = 0;
        return true;
    }
    if (hwnd != g_window) return false;

    switch (msg) {
    case WM_SYSKEYDOWN:
        // Alt+Enter (bit 29: Alt held; bit 30: auto-repeat, ignored), before DXGI or the game can make anything of it.
        if (wParam == VK_RETURN && (lParam & (1 << 29)) && !(lParam & (1 << 30))) {
            if (g_fullscreen) ExitFullscreen(hwnd); else EnterFullscreen(hwnd);
            Save(hwnd);
            *result = 0;
            return true;
        }
        break;
    case WM_SYSCHAR:
        // The Alt+Enter character, which DefWindowProc would answer with a beep.
        if (wParam == VK_RETURN) {
            *result = 0;
            return true;
        }
        break;
    case WM_SIZE:
    case WM_MOVE:
        // Saved half a second after the last of them: a drag through the window manager is a stream of these, with
        // no WM_EXITSIZEMOVE under Wine.
        if (!(msg == WM_SIZE && wParam == SIZE_MINIMIZED)) SetTimer(hwnd, kSaveTimer, kSaveDelayMs, nullptr);
        break;
    case WM_TIMER:
        if (wParam == kSaveTimer) {
            KillTimer(hwnd, kSaveTimer);
            Save(hwnd);
            *result = 0;
            return true;
        }
        break;
    default:
        break;
    }
    return false;
}

} // namespace cvr::desktop_window
