//! The HUD panels' two script inputs (formerly src/Natives/HudClearance.cpp and src/Natives/InteractionDepth.cpp): the
//! script modules publish, every ~33 ms, the nearest surface along the camera's forward axis (SetVRHudClearance; 99 =
//! open space) and the depth of the interaction prompt's object (SetVRInteractionDepth; 0 = no target). A value older
//! than half a second reads as none: the module stopped. The panels (`hud_quad`) read them here.
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use red4ext_abi::stack::StackFrame;

/// Published every ~33 ms; a gap this long means the module stopped.
const FRESH_MS: u64 = 500;

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

struct Published {
    value: AtomicU32,
    stamp: AtomicU64,
}
impl Published {
    const fn new() -> Published {
        Published {
            value: AtomicU32::new(0),
            stamp: AtomicU64::new(0),
        }
    }
    fn get(&self, now_ms: u64) -> f32 {
        let stamp = self.stamp.load(Ordering::Acquire);
        if stamp == 0 || now_ms.wrapping_sub(stamp) > FRESH_MS {
            return 0.0;
        }
        f32::from_bits(self.value.load(Ordering::Relaxed))
    }
    /// A script's value: finite and not negative, else none.
    fn set(&self, value: f32) {
        let value = if !value.is_finite() || value < 0.0 { 0.0 } else { value };
        self.value.store(value.to_bits(), Ordering::Relaxed);
        // SAFETY: a plain clock read.
        self.stamp.store(unsafe { GetTickCount64() }, Ordering::Release);
    }
}
static CLEARANCE: Published = Published::new();
/// The clearance measured by the plugin itself (the Rust build's port of the Stereo mod's hud_clearance module), as
/// SetVRHudClearance publishes a script's.
pub fn set_clearance(value: f32) {
    CLEARANCE.set(value);
}
static INTERACTION_DEPTH: Published = Published::new();
/// The interaction prompt's depth measured by the plugin itself (the Rust build's port of the Stereo mod's
/// interaction_depth module), as SetVRInteractionDepth publishes a script's.
pub fn set_interaction_depth(value: f32) {
    INTERACTION_DEPTH.set(value);
}

/// clearance::Published: the nearest depth in metres, 99 for open space, 0 when none is current.
pub fn clearance_published(now_ms: u64) -> f32 {
    CLEARANCE.get(now_ms)
}
/// interaction::TargetDepth: the prompt's object depth in metres, 0 when none is current.
pub fn interaction_target_depth(now_ms: u64) -> f32 {
    INTERACTION_DEPTH.get(now_ms)
}

/// One Float argument, read from the script VM.
///
/// # Safety
/// The frame the VM passed to the running native, positioned at a Float parameter.
unsafe fn float_argument(frame: *mut StackFrame) -> f32 {
    let mut value = 0.0f32;
    // SAFETY: as the caller vouches.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut value);
            frame.finish_params();
        }
    }
    value
}

/// SetVRHudClearance(depth: Float).
#[no_mangle]
pub unsafe extern "C" fn SetVRHudClearance(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the VM's frame for this native.
    CLEARANCE.set(unsafe { float_argument(frame) });
}
/// SetVRInteractionDepth(depth: Float).
#[no_mangle]
pub unsafe extern "C" fn SetVRInteractionDepth(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the VM's frame for this native.
    INTERACTION_DEPTH.set(unsafe { float_argument(frame) });
}

// The headers' forwarders (Runtimes/HudClearance.hpp, InteractionDepth.hpp in the Rust build).

#[no_mangle]
pub extern "C" fn cvr_hud_clearance_published(now_ms: u64) -> f32 {
    clearance_published(now_ms)
}
#[no_mangle]
pub extern "C" fn cvr_hud_clearance_grow_factor(configured: f32, distance: f32, grow: f32) -> f32 {
    crate::hud_quad::clearance::grow_factor(configured, distance, grow)
}
#[no_mangle]
pub extern "C" fn cvr_interaction_target_depth(now_ms: u64) -> f32 {
    interaction_target_depth(now_ms)
}
