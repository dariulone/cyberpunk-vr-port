//! BladeContactPose (main's src/Anim/BladeContactPose.cpp): the lodged or swept blade's pull on the weapon hand. The
//! blade contact script publishes a model-space offset (VRBladePoseOffset, side and offset, w 1 to accept); the hand
//! stop adds it to the weapon hand's target for 160 ms after, fading over the last 80, while a mantis blade (the
//! selected cyberware kind 5) is out in that hand on foot and no menu is up.
use core::sync::atomic::{AtomicBool, Ordering::*};
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

extern "C" {
    static g_hasWeaponEquipped: bool;
    static g_isInVehicle: bool;
    static g_menuModeValue: i32;
}

#[derive(Clone, Copy)]
struct Pose {
    delta: [f32; 3],
    at: u64,
    side: i32,
}
const NONE: Pose = Pose { delta: [0.0; 3], at: 0, side: -1 };
static PUBLISHED: Mutex<Pose> = Mutex::new(NONE);
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn tick_ms() -> u64 {
    // SAFETY: a plain OS call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// VRBladePoseOffset(side, modelOffset) -> Bool: the blade's pull on that hand (model space, at most 10.5 cm), w > 0.5
/// to accept; anything else clears it.
#[no_mangle]
pub unsafe extern "C" fn VRBladePoseOffset(_this: *mut core::ffi::c_void, frame: *mut StackFrame, out: *mut bool, _: i64) {
    let (mut side, mut d) = (-1i32, [0.0f32; 4]);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut side);
        frame.get_parameter(&mut d);
        frame.finish_params();
    }
    let accepted = publish(side, d);
    if !out.is_null() {
        *out = accepted;
    }
}

/// The pull published (the native's body, for the blade's own script in Rust): accepted for a side, w > 0.5 and at
/// most 10.5 cm; anything else clears it.
pub fn publish(side: i32, d: [f32; 4]) -> bool {
    let m2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let accepted = (0..=1).contains(&side) && d[3] > 0.5 && m2.is_finite() && m2 <= 0.011;
    let mut p = PUBLISHED.lock().unwrap_or_else(|e| e.into_inner());
    *p = if accepted { Pose { delta: [d[0], d[1], d[2]], at: tick_ms(), side } } else { NONE };
    ACTIVE.store(accepted, Release);
    accepted
}

/// ApplyPose: the blade's published pull added to the weapon hand's target, fading out 160 ms after it was given.
pub unsafe fn apply_pose(side: i32, target: *mut f32) {
    if !ACTIVE.load(Acquire)
        || !g_hasWeaponEquipped
        || g_isInVehicle
        || g_menuModeValue != 0
        || super::cyberware::cvr_cyberware_selected_kind() != 5
        || side != (if super::weapon_arm::cvr_anim_weapon_in_left_hand() { 0 } else { 1 })
    {
        return;
    }
    let p = *PUBLISHED.lock().unwrap_or_else(|e| e.into_inner());
    let age = tick_ms().wrapping_sub(p.at);
    if p.side != side || age >= 160 {
        return;
    }
    let fade = 1.0f32.min((160 - age) as f32 / 80.0);
    for i in 0..3 {
        *target.add(i) += p.delta[i] * fade;
    }
}
