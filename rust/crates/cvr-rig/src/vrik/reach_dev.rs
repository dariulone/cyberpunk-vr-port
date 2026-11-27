//! A development aid (CVR_RELOAD_REACH=1): the left hand's view frame as the solve built its target this frame (the view
//! in model space, the hand's scale and offset, the wrist turn), so a test can say which controller pose puts the hand
//! on a point of a weapon -- the inverse of VRIK_BuildViewHandTarget (cache.rs). Nothing is kept without the switch.
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// The left hand's view frame: target = vp + rv * map(local * scale) + off, rotation = rv * map(local) * wrist.
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub vp: [f32; 3],
    pub rv: [f32; 4],
    pub scale: f32,
    pub off: [f32; 3],
    pub wrist: [f32; 4],
}

static LEFT: Mutex<Option<View>> = Mutex::new(None);

/// 0 not read yet, 1 off, 2 on.
static ON: AtomicU8 = AtomicU8::new(0);
/// The switch: CVR_RELOAD_REACH=1 at the start, or a test's "reach" command since.
pub fn on() -> bool {
    match ON.load(Ordering::Relaxed) {
        0 => {
            let on = std::env::var("CVR_RELOAD_REACH").is_ok_and(|v| v == "1");
            ON.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        v => v == 2,
    }
}
/// A test's command turns it on or off.
pub fn set(on: bool) {
    ON.store(if on { 2 } else { 1 }, Ordering::Relaxed);
}

/// The frame the left hand's target was just built in (the anim thread).
pub fn note_left_view(vp: &[f32; 3], rv: &[f32; 4], scale: f32, off: &[f32; 3], wrist: &[f32; 4]) {
    if !on() {
        return;
    }
    *LEFT.lock().unwrap_or_else(|p| p.into_inner()) = Some(View { vp: *vp, rv: *rv, scale, off: *off, wrist: *wrist });
}

/// The last one noted.
pub fn left_view() -> Option<View> {
    *LEFT.lock().unwrap_or_else(|p| p.into_inner())
}
