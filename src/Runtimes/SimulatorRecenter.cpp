#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Runtimes/SimulatorRecenter.hpp"
#include "Runtimes/OpenXRManager.hpp"

#if CVR_RUST_VRIK
// The bridge is rust/crates/cvr-xr/src/simulator_recenter.rs, with the external pose reset's gate.
#else
#include <windows.h>
#include <atomic>
#include <cwchar>

extern void Log(const char* fmt, ...);
namespace {
std::atomic<HWND> s_window{};
std::atomic<WNDPROC> s_original{};
UINT s_resetCommand{};
thread_local unsigned s_resetDepth{};

UINT FindResetCommand(HMENU menu, unsigned depth = 0) {
    if (!menu || depth > 4) return 0;
    const int count = GetMenuItemCount(menu);
    for (int i = 0; i < count; ++i) {
        wchar_t label[256]{};
        MENUITEMINFOW item{}; item.cbSize = sizeof(item);
        item.fMask = MIIM_STRING | MIIM_ID | MIIM_SUBMENU;
        item.dwTypeData = label; item.cch = 255;
        if (!GetMenuItemInfoW(menu, i, TRUE, &item)) continue;
        if (std::wcsstr(label, L"Reset View")) return item.wID;
        if (const auto nested = FindResetCommand(item.hSubMenu, depth + 1)) return nested;
    }
    return 0;
}

LRESULT CALLBACK WindowProc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    const auto original = s_original.load(std::memory_order_acquire);
    if (!original) return DefWindowProcW(window, message, wparam, lparam);
    const bool reset = window == s_window.load(std::memory_order_acquire) &&
        ((message == WM_COMMAND && LOWORD(wparam) == s_resetCommand) ||
         (message == WM_KEYDOWN && wparam == VK_HOME));
    if (reset && s_resetDepth++ == 0) OpenXRManager::Get().BeginExternalPoseReset();
    const auto result = CallWindowProcW(original, window, message, wparam, lparam);
    if (reset && --s_resetDepth == 0) OpenXRManager::Get().EndExternalPoseReset();
    return result;
}

BOOL CALLBACK FindWindow(HWND window, LPARAM runtimeModule) {
    DWORD pid{}; GetWindowThreadProcessId(window, &pid);
    if (pid != GetCurrentProcessId()) return TRUE;
    wchar_t title[256]{}; GetWindowTextW(window, title, 256);
    if (!std::wcsstr(title, L"OpenXR Simulator")) return TRUE;
    const auto original = reinterpret_cast<WNDPROC>(GetWindowLongPtrW(window, GWLP_WNDPROC));
    MEMORY_BASIC_INFORMATION region{};
    if (!original || !VirtualQuery(reinterpret_cast<void*>(original), &region, sizeof(region)) ||
        region.AllocationBase != reinterpret_cast<void*>(runtimeModule)) return TRUE;
    const UINT reset = FindResetCommand(GetMenu(window));
    if (!reset) return TRUE;
    s_resetCommand = reset;
    s_original.store(original, std::memory_order_release);
    s_window.store(window, std::memory_order_release);
    SetLastError(0);
    const auto replaced = SetWindowLongPtrW(window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(&WindowProc));
    if (!replaced && GetLastError()) {
        s_window.store(nullptr, std::memory_order_release);
        s_original.store(nullptr, std::memory_order_release);
        return TRUE;
    }
    Log("Roomscale: simulator Reset View bridge installed, window=%p command=%u.\n", window, reset);
    return FALSE;
}
}

void PollSimulatorRecenterHook() {
    if (s_window.load(std::memory_order_acquire)) return;
    static ULONGLONG lastAttempt{};
    const auto now = GetTickCount64();
    if (now - lastAttempt < 1000) return;
    lastAttempt = now;
    if (const auto module = GetModuleHandleW(L"openxr_simulator.dll"))
        EnumWindows(FindWindow, reinterpret_cast<LPARAM>(module));
}

void RemoveSimulatorRecenterHook() {
    const auto window = s_window.load(std::memory_order_acquire);
    const auto original = s_original.load(std::memory_order_acquire);
    if (window && original && IsWindow(window) &&
        GetWindowLongPtrW(window, GWLP_WNDPROC) == reinterpret_cast<LONG_PTR>(&WindowProc))
        SetWindowLongPtrW(window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(original));
    s_window.store(nullptr, std::memory_order_release);
    // Keep the chain target valid for any callback that was already dispatched.
}
#endif
#endif
