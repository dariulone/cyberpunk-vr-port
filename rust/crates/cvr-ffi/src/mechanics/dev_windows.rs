//! The CET mods' tuning windows in Rust (dev_overlay.rs opens them): each one is its module's -- the camera bridge's
//! (force_fpp.rs, formerly mods/cet/CyberpunkVRPort_ForceFPP's "VR camera bridge"), the wrist readout's
//! (wrist_hud.rs, CyberpunkVRPort_WristHud's "VR wrist readout"), the magazine tuner's (reload.rs,
//! CyberpunkVRPort_Reload's "VR magazine tuner"), the tactical belt's (belt.rs, CyberpunkVRPort_TacticalBelt's
//! "VR tactical belt") and the reload recorder's (reload_recorder.rs, CyberpunkVRPort_ReloadRecorder's "VR Reload
//! Recorder") -- placed where the mods placed theirs on the first open (fractions
//! of the frame), with the same controls; they run on the render thread and touch only their module's state.
use cvr_render::imgui;
use std::ffi::CString;

/// ImGuiCond_FirstUseEver.
const FIRST_USE_EVER: i32 = 4;

/// A window at fractions of the frame (the first time), its body drawn by `body`.
pub(super) fn window(name: &str, at: [f32; 2], size: [f32; 2], frame: [f32; 2], body: impl FnOnce()) {
    let Ok(title) = CString::new(name) else { return };
    imgui::set_next_window_pos_cond([frame[0] * at[0], frame[1] * at[1]], FIRST_USE_EVER);
    imgui::set_next_window_size_cond([frame[0] * size[0], frame[1] * size[1]], FIRST_USE_EVER);
    if imgui::begin(&title, 0) {
        body();
    }
    imgui::end();
}
/// A line of text.
pub(super) fn text(s: &str) {
    if let Ok(c) = CString::new(s.replace('\0', "")) {
        imgui::text_unformatted(&c);
    }
}
fn c(label: &str) -> CString {
    CString::new(label.replace('\0', "")).unwrap_or_default()
}
pub(super) fn checkbox(label: &str, v: &mut bool) -> bool {
    imgui::checkbox(&c(label), v)
}
pub(super) fn slider(label: &str, v: &mut f32, lo: f32, hi: f32) -> bool {
    imgui::slider_float(&c(label), v, lo, hi, c"%.3f", 0)
}
pub(super) fn slider_f64(label: &str, v: &mut f64, lo: f32, hi: f32) -> bool {
    let mut f = *v as f32;
    let changed = slider(label, &mut f, lo, hi);
    if changed {
        *v = f64::from(f);
    }
    changed
}
pub(super) fn button(label: &str) -> bool {
    imgui::button(&c(label), [0.0, 0.0])
}
pub(super) fn same_line() {
    imgui::same_line(0.0, -1.0);
}
pub(super) fn separator() {
    imgui::separator();
}
pub(super) fn tree(label: &str, body: impl FnOnce()) {
    if imgui::tree_node(&c(label)) {
        body();
        imgui::tree_pop();
    }
}

/// Every frame the overlay is open.
pub(super) fn draw(width: f32, height: f32) {
    static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
    if FIRST.swap(false, std::sync::atomic::Ordering::Relaxed) {
        cvr_host::log!("[overlay] the CET mods' windows drawn from Rust ({width:.0}x{height:.0}): VR camera bridge, VR wrist readout, VR magazine tuner, VR tactical belt, VR Reload Recorder, VR weapon grip\n");
    }
    let frame = [width, height];
    super::force_fpp::dev_window(frame);
    super::wrist_hud::dev_window(frame);
    super::reload::dev_window(frame);
    super::belt::dev_window(frame);
    super::reload_recorder::dev_window(frame);
    super::weapon_wrist::dev_window(frame);
}
/// The overlay closed (the mods' onOverlayClose): the belt's settings and the weapons' wrist turns written.
pub(super) fn closed() {
    super::belt::dev_window_closed();
    super::weapon_wrist::closed();
}
pub(super) fn drag(label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32, fmt: &str) -> bool {
    imgui::drag_float(&c(label), v, speed, lo, hi, &c(fmt))
}
pub(super) fn drag_int(label: &str, v: &mut i32, lo: i32, hi: i32) -> bool {
    imgui::drag_int(&c(label), v, 1.0, lo, hi)
}
pub(super) fn slider_int(label: &str, v: &mut i32, lo: i32, hi: i32) -> bool {
    imgui::slider_int_plain(&c(label), v, lo, hi)
}
pub(super) fn radio(label: &str, active: bool) -> bool {
    imgui::radio_button(&c(label), active)
}
pub(super) fn small(label: &str) -> bool {
    imgui::small_button(&c(label))
}
pub(super) fn header(label: &str) -> bool {
    imgui::collapsing_header(&c(label), 0)
}
pub(super) fn wrapped(s: &str) {
    imgui::text_wrapped(&c(s));
}
/// An InputText over a String (at most `max` bytes): true when it changed.
pub(super) fn input(label: &str, text: &mut String, max: usize) -> bool {
    let mut buffer = vec![0u8; max + 1];
    let n = text.len().min(max);
    buffer[..n].copy_from_slice(&text.as_bytes()[..n]);
    if imgui::input_text(&c(label), &mut buffer) {
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        *text = String::from_utf8_lossy(&buffer[..end]).into_owned();
        return true;
    }
    false
}
