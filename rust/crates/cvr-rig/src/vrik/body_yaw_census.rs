//! BodyYawCensus (formerly src/Hooks/BodyYawCensus.cpp): the site that computes the player's body yaw and stores it
//! as a pure-yaw quaternion into its movement state at +0x1D0 (position at +0x1C0, the camera path's 17-bit fixed
//! point). Everything below it only copies the value (docs/vrik-map.md section 6).
//!
//! THE ENGINE YAW IS LOAD-BEARING: the solve converts world to model with it (model space IS the entity frame); the
//! camera heading only equalled it standing still and lagged it a frame on every turn. The census half (the write's
//! time against the animation pass) is diagnostics.
//!
//! PATCH SAFETY. The original movups runs first, then the stub saves the volatile registers AND xmm0-xmm3 (the
//! function passes floats in xmm registers) before calling in. The 17-byte pattern has one match in the exe (the
//! 8-byte write alone matches twice).
use super::env::{QueryPerformanceCounter, QueryPerformanceFrequency};
use core::ffi::c_void;
use cvr_engine::code::{find_pattern, read};
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use std::sync::atomic::{AtomicI64, Ordering};
use super::body_yaw_follow::cvr_body_yaw_follow_tick;
use super::roomscale_move::{cvr_roomscale_is_player_movement, cvr_roomscale_observe_movement};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugYawWriteMs: f64;
    static mut CyberpunkVR_EngineBodyYawZ: f32;
    static mut CyberpunkVR_EngineBodyYawW: f32;
    static mut CyberpunkVR_EngineBodyYawValid: i32;
    static mut CyberpunkVR_DebugYawWritesAll: u64;
    static mut CyberpunkVR_DebugYawWritesPlayer: u64;
}

/// movups [r15+1D0h],xmm0 / movss [r15+1E0h],xmm6
const PATTERN: &[u8] = b"\x41\x0F\x11\x87\xD0\x01\x00\x00\xF3\x41\x0F\x11\xB7\xE0\x01\x00\x00";
const MASK: &[u8] = b"xxxxxxxxxxxxxxxxx";
/// Just the movups: the jmp is 5 and the rest is NOPs.
const REPLACE: usize = 8;

static FREQ: AtomicI64 = AtomicI64::new(0);
fn now_ms() -> f64 {
    let mut f = FREQ.load(Ordering::Relaxed);
    if f == 0 {
        // SAFETY: writes one local.
        unsafe { QueryPerformanceFrequency(&mut f) };
        FREQ.store(f, Ordering::Relaxed);
    }
    if f == 0 {
        return 0.0;
    }
    let mut t = 0i64;
    // SAFETY: writes one local.
    unsafe { QueryPerformanceCounter(&mut t) };
    t as f64 * 1000.0 / f as f64
}

/// R15 is the movement component; S+0x90 is its owner, compared with GetPlayer's instance (proximity to a leaning VR
/// camera cannot identify a player, and a retained pointer can survive a reload).
#[no_mangle]
pub unsafe extern "C" fn OnBodyYawWriteCallback(state: *mut c_void) {
    let diagnostics = CyberpunkVR_RuntimeDiagnostics != 0;
    if diagnostics {
        CyberpunkVR_DebugYawWritesAll += 1;
    }
    if !cvr_roomscale_is_player_movement(state) {
        return;
    }
    cvr_roomscale_observe_movement(state);
    let s = state as usize;
    if diagnostics {
        CyberpunkVR_DebugYawWritesPlayer += 1;
    }
    // A pure-yaw quaternion by construction (x and y zero, verified live): z and w are all the solve needs, published
    // BEFORE the timestamp so a reader that sees a fresh time sees the matching angle.
    let (Some(z), Some(w)) = (read::<f32>(s + 0x1D8), read::<f32>(s + 0x1DC)) else { return };
    CyberpunkVR_EngineBodyYawZ = z;
    CyberpunkVR_EngineBodyYawW = w;
    CyberpunkVR_EngineBodyYawValid = 1;
    CyberpunkVR_DebugYawWriteMs = now_ms();
    // The follower publishes the body from here: the engine's own transform write fires only when something
    // changes, this site once per frame for the player. NOTHING IS APPLIED HERE (+0x1D0 is a copy nothing
    // propagates; the real write is earlier in the same function).
    let Some(p) = read::<[i32; 3]>(s + 0x1C0) else { return };
    let k = 1.0f32 / 131072.0;
    let pos = [p[0] as f32 * k, p[1] as f32 * k, p[2] as f32 * k];
    cvr_body_yaw_follow_tick(z, w, pos.as_ptr());
}

/// The stub at `at`: the original movups, the register save, the call with rcx = r15, the restore, back past the site.
pub fn stub(at: usize, site: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0x41, 0x0F, 0x11, 0x87, 0xD0, 0x01, 0x00, 0x00]) // movups [r15+1D0h],xmm0
        .volatile_call(4, &[0x4C, 0x89, 0xF9], callback) // rcx = r15
        .jmp(site + REPLACE);
    c
}

/// Installed from src/Hooks/BodyYawCensus.cpp's registration (Boot, 61).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_body_yaw_census_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 256) else { return false };
    let code = stub(tramp, found, OnBodyYawWriteCallback as *const () as u64);
    code.commit();
    patch_jump(found, REPLACE, tramp);
    true
}