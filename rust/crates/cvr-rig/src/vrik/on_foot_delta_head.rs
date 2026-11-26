//! OnFootDeltaHead (formerly src/Hooks/OnFootDeltaHead.cpp): the per-frame heading delta on foot, where snap turn and
//! stick turning are injected. Only deliberate view turning enters here; the physical body's following is consumed
//! separately by the movement adapter (body_heading.rs).
use super::body_yaw_follow::cvr_body_yaw_follow_release;
use super::env::{g_isInVehicle, log, GetTickCount64};
use super::recoil::RecoilLastShotMs;
use super::roomscale_move::cvr_roomscale_is_player_provider;
use cvr_body::policy::native_owns_turn;
use cvr_engine::code::find_pattern;
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use core::ffi::c_void;

extern "C" {
    // Peak |heading delta| within 150 ms of a shot, degrees (src/Hooks/OnFootDeltaHead.cpp, both builds).
    static mut CyberpunkVR_DebugShotHeadingKickDeg: f32;
    // src/Hooks/HookServices.cpp
    fn cvr_cpp_telemetry_delta_head(delta_head: *mut f32);
    fn cvr_cpp_take_pending_snap_yaw() -> i32;
    fn cvr_cpp_shot_shared() -> *mut f32;
    fn cvr_cpp_pose_vehicle_state() -> i32;
    fn cvr_cpp_ladder_active() -> bool;
}

/// movss xmm0,[rcx+9Ch] / lea rdx,[rsp+30h]
const PATTERN: &[u8] = b"\xF3\x0F\x10\x81\x9C\x00\x00\x00\x48\x8D\x54\x24\x30";
const MASK: &[u8] = b"xxxxxxxxxxxxx";
const REPLACE: usize = 8;

fn now_ms() -> f64 {
    let (t, f) = super::env::qpc();
    if f != 0 {
        t as f64 * 1000.0 / f as f64
    } else {
        0.0
    }
}

/// The trampoline passes P+0x9C: [0] the DEGREE delta consumed below; [1] and [2] the accumulator and last delta.
#[no_mangle]
pub unsafe extern "C" fn OnOnFootDeltaHeadCallback(delta_head: *mut f32) {
    cvr_cpp_telemetry_delta_head(delta_head);
    if delta_head.is_null() || !cvr_roomscale_is_player_provider((delta_head as *const u8).wrapping_sub(0x9C) as *const c_void) {
        return;
    }
    if native_owns_turn(g_isInVehicle, cvr_cpp_pose_vehicle_state(), cvr_cpp_ladder_active()) {
        cvr_cpp_take_pending_snap_yaw();
        // Native vehicle/ladder heading must not inherit a pending on-foot snap or the follower's previous offset.
        cvr_body_yaw_follow_release();
        return;
    }
    // MEASUREMENT ONLY: the game's own camera kick, read before anything of ours is added (the kick is removed at
    // its source, the weapons' recoil stats; cancelling it here left the character applying it).
    let shot = RecoilLastShotMs();
    let now = now_ms();
    if shot > 0.0 && now - shot >= 0.0 && now - shot < 150.0 {
        for k in 0..4 {
            let v = *delta_head.add(k);
            let a = if v < 0.0 { -v } else { v };
            if a > CyberpunkVR_DebugShotHeadingKickDeg {
                CyberpunkVR_DebugShotHeadingKickDeg = a;
            }
        }
    }
    let bits = cvr_cpp_take_pending_snap_yaw();
    let mut snap = 0.0f32;
    if bits != 0 {
        snap = f32::from_bits(bits as u32);
        // SNAP EVENT PUBLISH: this runs at TICK, before the same tick's animation pass. The yaw delta (radians,
        // [146]) and a bump counter ([147]) make the packet latch rotate the view packet by exactly this delta once;
        // [148] is the pre-snap heading (the double-apply guard), [150] the tick stamp (deferred past this tick).
        let sh = cvr_cpp_shot_shared();
        if !sh.is_null() {
            *sh.add(146) = snap * 0.017_453_292_52;
            *sh.add(148) = *sh.add(141);
            *sh.add(150) = *sh.add(99);
            *sh.add(147) += 1.0;
            log(&format!(
                "[snap-pub] ms={} ctr={:.0} delta={:.2}deg preHeading={:.4} ack={:.0} tick={:.0}\n",
                GetTickCount64(),
                *sh.add(147),
                snap,
                *sh.add(148),
                *sh.add(149),
                *sh.add(150)
            ));
        }
    }
    if snap != 0.0 {
        *delta_head += snap;
        // FSR3 treats the capture that shows it as a cut (cvr-render fsr3.rs)
        cvr_host::snap_turn::note(snap);
    }
}

/// The callback first with rcx = rcx+9Ch (flags, GPRs and xmm0-3 saved), then the original movss, back past the site.
pub fn stub(at: usize, site: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    c.volatile_call(4, &[0x48, 0x8D, 0x89, 0x9C, 0x00, 0x00, 0x00], callback) // lea rcx,[rcx+9Ch]
        .bytes(&[0xF3, 0x0F, 0x10, 0x81, 0x9C, 0x00, 0x00, 0x00]) // the original movss xmm0,[rcx+9Ch]
        .jmp(site + REPLACE);
    c
}

/// Installed from src/Hooks/OnFootDeltaHead.cpp's registration (Boot, 76).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_on_foot_delta_head_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 512) else { return false };
    stub(tramp, found, OnOnFootDeltaHeadCallback as *const () as u64).commit();
    patch_jump(found, REPLACE, tramp);
    true
}