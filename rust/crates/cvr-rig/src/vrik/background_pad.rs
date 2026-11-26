//! BackgroundPad (formerly src/Hooks/BackgroundPad.cpp): the pad's events reach the game while the headset has the
//! input, whichever window is in front.
//!
//! The game polls the pad in the background but hands its events on (2.5 sub_1420849A0) only while
//! GetForegroundWindow() is the input system's own window ([this+0x1D0]); otherwise it drops them. The VR controllers
//! reach the game as that pad (vrik::xinput), so with the monitor's window behind anything -- the runtime's desktop
//! window, a streaming app, a click on another screen, a window the game minimised itself -- the sticks stopped
//! walking and steering and the cyberware chord stopped, while the port's own VR menu, which reads OpenXR directly,
//! still worked.
//!
//! THE PATCH IS AT THAT ONE CALL: `call [GetForegroundWindow]; cmp rax, [rbx+window]; jne skip` goes to a stub that
//! asks cvr_pad_foreground_window instead -- the input system's window while the OpenXR session is FOCUSED (the
//! headset is on and this game has its input), the real foreground window otherwise -- and keeps the game's own
//! compare and jumps. Nothing else in the game asks this question through here: its focus, pause and cursor logic
//! still see the real foreground window.
use core::ffi::c_void;
use cvr_engine::code::{find_pattern, read};
use cvr_engine::trampoline::{allocate, patch_jump, Code};

extern "C" {
}
#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
}

/// cmp byte [rcx+flag],0 / mov rdi,rdx / mov rbx,rcx / jne / call [GetForegroundWindow] / cmp rax,[rbx+window] / jne
const PATTERN: &[u8] = b"\x80\xB9\x00\x00\x00\x00\x00\x48\x8B\xFA\x48\x8B\xD9\x75\x00\xFF\x15\x00\x00\x00\x00\x48\x3B\x83\x00\x00\x00\x00\x75\x00";
const MASK: &[u8] = b"xx??xxxxxxxxxx?xx????xxx??xxx?";
/// The call (6), the compare (7) and its jne (2).
const REPLACE: usize = 15;

/// The window the game compares with its own: its own while the headset is focused.
#[no_mangle]
pub unsafe extern "C" fn cvr_pad_foreground_window(input_window: *mut c_void) -> *mut c_void {
    // SAFETY: C++ service and a plain Win32 query.
    unsafe {
        if !input_window.is_null() && super::xr_calls::session_focused() {
            return input_window;
        }
        GetForegroundWindow()
    }
}

/// The stub at `at`: rcx = [rbx+window], the call, then the game's own compare and both of its jumps.
pub fn stub(at: usize, window: i32, skip: usize, drain: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0x48, 0x8B, 0x8B]).bytes(&window.to_le_bytes()); // mov rcx,[rbx+window]
    // at the original call's stack: aligned, the shadow space reserved
    c.mov_rax(callback).bytes(&[0xFF, 0xD0]);
    c.bytes(&[0x48, 0x3B, 0x83]).bytes(&window.to_le_bytes()); // cmp rax,[rbx+window]
    let rel = (skip as i64 - (c.here() as i64 + 6)) as i32;
    c.bytes(&[0x0F, 0x85]).bytes(&rel.to_le_bytes()); // jne skip
    c.jmp(drain);
    c
}

/// Installed from src/Hooks/BackgroundPad.cpp's registration (Boot, 86).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_background_pad_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let site = found + 15;
    let (Some(window), Some(jne)) = (read::<i32>(found + 24), read::<i8>(found + 29)) else { return false };
    let drain = found + 30; // after the jne: the events are handed on
    let skip = (drain as i64 + jne as i64) as usize;
    let Some(tramp) = allocate(site, 64) else { return false };
    let code = stub(tramp, window, skip, drain, cvr_pad_foreground_window as *const () as u64);
    // SAFETY: fresh arena memory, then a site whose 15 bytes the stub reproduces.
    unsafe {
        code.commit();
        patch_jump(site, REPLACE, tramp);
    }
    super::env::log(&format!(
        "BackgroundPad: pad events reach the game while the headset is focused (site {:#x}, window +0x{:X})\n",
        site, window as u32
    ));
    true
}

