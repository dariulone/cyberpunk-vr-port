//! OnFootMoveXY (formerly src/Hooks/OnFootMoveXY.cpp): the on-foot planar movement input, after the game stores its Y
//! scalar, so locomotion follows the chosen source (HMD, either hand) rather than the game heading. "Game" (source 0)
//! means forward in the character's own frame, which the native solver already uses.
use super::body_heading::cvr_body_heading_offset;
use cvr_body::cmath::{cos, sin};
use cvr_engine::code::find_pattern;
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use core::ffi::c_void;

extern "C" {
    // src/Hooks/HookServices.cpp
    fn cvr_cpp_telemetry_move_xy(move_struct: *mut c_void);
    fn cvr_cpp_swimming_bind_native_input(move_struct: *mut c_void);
    fn cvr_cpp_ladder_active() -> bool;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_xr_hmd_yaw_rel_to_body() -> f32;
    fn cvr_xr_hand_yaw_rel_to_body(side: i32) -> f32;
}

/// movss [rsi+94h],xmm0 / addss xmm1,xmm2
const PATTERN: &[u8] = b"\xF3\x0F\x11\x86\x94\x00\x00\x00\xF3\x0F\x58\xCA";
const MASK: &[u8] = b"xxxxxxxxxxxx";
const REPLACE: usize = 8;

#[no_mangle]
pub unsafe extern "C" fn OnOnFootMoveXYCallback(move_struct: *mut c_void) {
    cvr_cpp_telemetry_move_xy(move_struct);
    cvr_cpp_swimming_bind_native_input(move_struct);
    if cvr_cpp_ladder_active() {
        return; // ladder MoveY is up/down, independent of gaze yaw
    }
    let src = super::live_reads::movement_source();
    if src <= 0 || cvr_cpp_menu_mode() != 0 || move_struct.is_null() {
        return;
    }
    let p = (move_struct as *mut u8).add(0x90) as *mut f32;
    let (x, y) = (*p, *p.add(1));
    if x == 0.0 && y == 0.0 {
        return;
    }
    let mut yaw = match src {
        1 => cvr_xr_hmd_yaw_rel_to_body(),
        2 => cvr_xr_hand_yaw_rel_to_body(0),
        3 => cvr_xr_hand_yaw_rel_to_body(1),
        _ => return,
    };
    // A tracking-relative direction into the current native body frame (BodyYawFollowOffset).
    yaw -= cvr_body_heading_offset();
    let (c, s) = (cos(yaw), sin(yaw));
    *p = x * c - y * s;
    *p.add(1) = x * s + y * c;
}

/// The original store first (the struct then has the new Y scalar), the callback with rcx = rsi and flags, GPRs and
/// xmm0-5 saved (the game's next addss xmm1,xmm2 and its registers survive), back past the site.
pub fn stub(at: usize, site: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0xF3, 0x0F, 0x11, 0x86, 0x94, 0x00, 0x00, 0x00])
        .volatile_call(6, &[0x48, 0x89, 0xF1], callback) // rcx = rsi
        .jmp(site + REPLACE);
    c
}

/// Installed from src/Hooks/OnFootMoveXY.cpp's registration (Boot, 78).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_on_foot_move_xy_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 256) else { return false };
    stub(tramp, found, OnOnFootMoveXYCallback as *const () as u64).commit();
    patch_jump(found, REPLACE, tramp);
    true
}