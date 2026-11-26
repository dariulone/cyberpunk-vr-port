//! DlssRes (formerly src/Hooks/DlssRes.cpp): the DLSS render and target sizes as the engine decides them, logged.
//! Read-only: the override it once applied is gone, so this reports what the engine chose and nothing else.
//!
//! 2.5's render-res writer (sub_140425054) clamps every size to at least 16 and leaves through several stores, so there
//! is no single tail to patch: the stereo render-res detour (src/Stereo/Dlss.cpp) sees the same struct and calls
//! OnDLSSResCallback after the engine has written it. Each state is logged the first time it is seen (with a second
//! view the hook alternates between two), and then every 600th call when its sizes changed.
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use cvr_engine::code::read;
use std::sync::Mutex;

static HITS: AtomicU64 = AtomicU64::new(0);
/// The last state seen (g_dlssResPtr of the C++ build, which nothing reads).
static LAST_STATE: AtomicUsize = AtomicUsize::new(0);
struct Seen {
    states: [usize; 8],
    count: usize,
    last: [u32; 4],
}
static SEEN: Mutex<Seen> = Mutex::new(Seen {
    states: [0; 8],
    count: 0,
    last: [0; 4],
});

#[no_mangle]
pub unsafe extern "C" fn OnDLSSResCallback(dlss_ptr: *mut c_void) {
    let hits = HITS.fetch_add(1, Ordering::Relaxed) + 1;
    let dlss = dlss_ptr as usize;
    if dlss < 0x10000 {
        return;
    }
    LAST_STATE.store(dlss, Ordering::Relaxed);
    let mut seen = SEEN.lock().unwrap_or_else(|p| p.into_inner());
    let known = seen.states[..seen.count].contains(&dlss);
    let first_sight = !known && seen.count < 8;
    if first_sight {
        let n = seen.count;
        seen.states[n] = dlss;
        seen.count += 1;
    }
    if !(first_sight || hits % 600 == 1) {
        return;
    }
    // The render/target pair: render at +0x00/+0x04, target at +0x20/+0x24.
    let field = |at: usize| read::<u32>(dlss + at).unwrap_or(0);
    let (render_w, render_h, target_w, target_h) = (field(0x00), field(0x04), field(0x20), field(0x24));
    let sx = if target_w != 0 { render_w as f32 / target_w as f32 } else { 0.0 };
    let sy = if target_h != 0 { render_h as f32 / target_h as f32 } else { 0.0 };
    let now = [render_w, render_h, target_w, target_h];
    let changed = now != seen.last;
    seen.last = now;
    if changed {
        super::env::log(&format!(
            "DLSSRes hook: ptr={dlss:016X} render={render_w}x{render_h} target={target_w}x{target_h} scale={:.3}/{:.3}\n",
            sx as f64, sy as f64
        ));
    }
}

/// Installed from src/Hooks/DlssRes.cpp's registration (Boot, 90): nothing to patch on 2.5.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_dlss_res_install() -> bool {
    super::env::log("DLSSRes: reported from the render-res detour (no code patch on 2.5)\n");
    true
}
