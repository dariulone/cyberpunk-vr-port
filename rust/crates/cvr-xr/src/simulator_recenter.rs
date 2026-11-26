//! The OpenXR Simulator's Reset View (formerly src/Runtimes/SimulatorRecenter.cpp): the simulator resets its raw poses
//! from its own window's menu (or Home) without a reference-space event, so the port would not know a recenter
//! happened. Its window's procedure is wrapped: a Reset View is bracketed by OpenXRManager's external pose reset, so
//! no pose located across it is published.
//!
//! The window is found once the simulator's module is loaded (polled from the frame loop at most once a second): a
//! window of this process titled "OpenXR Simulator" whose procedure lives in that module and whose menu has a
//! "Reset View" item.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::cell::Cell;
use cvr_host::log;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicU64, Ordering};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{GetLastError, SetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_HOME;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumWindows, GetMenu, GetMenuItemCount, GetMenuItemInfoW, GetWindowLongPtrW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindow, SetWindowLongPtrW, GWLP_WNDPROC, HMENU, MENUITEMINFOW, MIIM_ID, MIIM_STRING, MIIM_SUBMENU,
    WM_COMMAND, WM_KEYDOWN, WNDPROC,
};

#[cfg(not(test))]
use crate::published::{cvr_xr_begin_external_pose_reset, cvr_xr_end_external_pose_reset};

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL: AtomicIsize = AtomicIsize::new(0);
static RESET_COMMAND: AtomicU32 = AtomicU32::new(0);
static LAST_ATTEMPT: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static RESET_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// `needle` in the NUL-terminated `hay` (wcsstr).
fn contains(hay: &[u16], needle: &str) -> bool {
    let len = hay.iter().position(|&c| c == 0).unwrap_or(hay.len());
    let needle: Vec<u16> = needle.encode_utf16().collect();
    hay[..len].windows(needle.len()).any(|w| w == needle.as_slice())
}

/// The "Reset View" item's command, searched through the submenus four deep.
unsafe fn find_reset_command(menu: HMENU, depth: u32) -> u32 {
    if menu.is_null() || depth > 4 {
        return 0;
    }
    // SAFETY (whole body): the window's live menu, read item by item into local buffers.
    unsafe {
        let count = GetMenuItemCount(menu);
        for i in 0..count.max(0) {
            let mut label = [0u16; 256];
            let mut item: MENUITEMINFOW = core::mem::zeroed();
            item.cbSize = core::mem::size_of::<MENUITEMINFOW>() as u32;
            item.fMask = MIIM_STRING | MIIM_ID | MIIM_SUBMENU;
            item.dwTypeData = label.as_mut_ptr();
            item.cch = 255;
            if GetMenuItemInfoW(menu, i as u32, 1, &mut item) == 0 {
                continue;
            }
            if contains(&label, "Reset View") {
                return item.wID;
            }
            let nested = find_reset_command(item.hSubMenu, depth + 1);
            if nested != 0 {
                return nested;
            }
        }
    }
    0
}

#[cfg(not(test))]
unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY (whole body): the original procedure this one replaced, called as the window manager would.
    unsafe {
        let original = ORIGINAL.load(Ordering::Acquire);
        if original == 0 {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let reset = window as isize == WINDOW.load(Ordering::Acquire)
            && ((message == WM_COMMAND && (wparam & 0xFFFF) as u32 == RESET_COMMAND.load(Ordering::Relaxed))
                || (message == WM_KEYDOWN && wparam == VK_HOME as usize));
        if reset && RESET_DEPTH.with(|d| d.replace(d.get() + 1)) == 0 {
            cvr_xr_begin_external_pose_reset();
        }
        let original: WNDPROC = core::mem::transmute::<isize, WNDPROC>(original);
        let result = CallWindowProcW(original, window, message, wparam, lparam);
        if reset && RESET_DEPTH.with(|d| { d.set(d.get() - 1); d.get() }) == 0 {
            cvr_xr_end_external_pose_reset();
        }
        result
    }
}

#[cfg(not(test))]
unsafe extern "system" fn find_window(window: HWND, runtime_module: LPARAM) -> BOOL {
    // SAFETY (whole body): a top-level window being enumerated; its procedure replaced only when it is the simulator's.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        if pid != GetCurrentProcessId() {
            return 1;
        }
        let mut title = [0u16; 256];
        GetWindowTextW(window, title.as_mut_ptr(), 256);
        if !contains(&title, "OpenXR Simulator") {
            return 1;
        }
        let original = GetWindowLongPtrW(window, GWLP_WNDPROC);
        let mut region: MEMORY_BASIC_INFORMATION = core::mem::zeroed();
        if original == 0
            || VirtualQuery(original as *const _, &mut region, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0
            || region.AllocationBase as isize != runtime_module
        {
            return 1;
        }
        let reset = find_reset_command(GetMenu(window), 0);
        if reset == 0 {
            return 1;
        }
        RESET_COMMAND.store(reset, Ordering::Relaxed);
        ORIGINAL.store(original, Ordering::Release);
        WINDOW.store(window as isize, Ordering::Release);
        SetLastError(0);
        let replaced = SetWindowLongPtrW(window, GWLP_WNDPROC, window_proc as *const () as isize);
        if replaced == 0 && GetLastError() != 0 {
            WINDOW.store(0, Ordering::Release);
            ORIGINAL.store(0, Ordering::Release);
            return 1;
        }
        log!("Roomscale: simulator Reset View bridge installed, window={:016X} command={}.\n", window as usize, reset);
        0
    }
}

#[cfg(not(test))]
/// From the frame loop: until the bridge is in, look for the simulator's window once a second.
#[no_mangle]
pub extern "C" fn cvr_xr_poll_simulator_recenter() {
    if WINDOW.load(Ordering::Acquire) != 0 {
        return;
    }
    // SAFETY: plain Win32 queries; the enumeration callback above.
    unsafe {
        let now = GetTickCount64();
        if now.wrapping_sub(LAST_ATTEMPT.load(Ordering::Relaxed)) < 1000 {
            return;
        }
        LAST_ATTEMPT.store(now, Ordering::Relaxed);
        let name: Vec<u16> = "openxr_simulator.dll".encode_utf16().chain(Some(0)).collect();
        let module = GetModuleHandleW(name.as_ptr());
        if !module.is_null() {
            EnumWindows(Some(find_window), module as isize);
        }
    }
}

#[cfg(not(test))]
/// At shutdown: the original procedure back, when the window still has ours. The original stays recorded for any
/// call already dispatched into ours.
#[no_mangle]
pub extern "C" fn cvr_xr_remove_simulator_recenter() {
    let window = WINDOW.load(Ordering::Acquire);
    let original = ORIGINAL.load(Ordering::Acquire);
    // SAFETY: the window recorded at install, checked alive and still ours before its procedure is put back.
    unsafe {
        let hwnd = window as HWND;
        if window != 0
            && original != 0
            && IsWindow(hwnd) != 0
            && GetWindowLongPtrW(hwnd, GWLP_WNDPROC) == window_proc as *const () as isize
        {
            SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original);
        }
    }
    WINDOW.store(0, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_label_is_found_inside_its_nul() {
        let mut label = [0u16; 32];
        for (i, c) in "&Reset View\tHome".encode_utf16().enumerate() {
            label[i] = c;
        }
        assert!(contains(&label, "Reset View"));
        assert!(!contains(&label, "Reset Views"));
        assert!(!contains(&[0u16; 8], "R"));
    }
}
