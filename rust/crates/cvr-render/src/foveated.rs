//! Foveated DLSS's policy (formerly src/Render/FoveatedDlssSettings.cpp): its settings (the mode, the full-resolution
//! centre's size and offset in percent of the view, the boundary overlay), the delayed application (an expensive
//! rebuild waits a second after the last change and while the panel is being edited), and the centre region in render
//! and output pixels.
use core::ffi::{c_char, c_void};
use std::sync::Mutex;

use cvr_crt::{cpp_clamp, format, lroundf, ptr, scan, Arg};

/// cvr::foveated::Settings.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Off, Quality, Balanced, Performance.
    pub mode: i32,
    pub width_percent: f32,
    pub height_percent: f32,
    pub show_boundary: bool,
    pub offset_x_percent: f32,
    pub offset_y_percent: f32,
    /// INI migration only; explicit axes take precedence.
    pub parsed_dimensions: u8,
}
const DEFAULTS: Settings = Settings {
    mode: 0,
    width_percent: 70.0,
    height_percent: 65.0,
    show_boundary: false,
    offset_x_percent: 0.0,
    offset_y_percent: -10.0,
    parsed_dimensions: 0,
};
impl Default for Settings {
    fn default() -> Settings {
        DEFAULTS
    }
}
impl Settings {
    /// SameReconstruction: everything that needs a rebuild (not the boundary overlay).
    pub fn same_reconstruction(&self, o: &Settings) -> bool {
        self.mode == o.mode
            && self.width_percent == o.width_percent
            && self.height_percent == o.height_percent
            && self.offset_x_percent == o.offset_x_percent
            && self.offset_y_percent == o.offset_y_percent
    }
}
const _: () = assert!(core::mem::size_of::<Settings>() == 28 && core::mem::offset_of!(Settings, offset_x_percent) == 16);

/// cvr::foveated::Region: the centre in output pixels, its input size and its source origin in render pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub input_w: u32,
    pub input_h: u32,
    pub source_x: u32,
    pub source_y: u32,
}

/// Normalize: the mode 0..3, the centre 20..100% (non-finite: the defaults), its offset within the view.
pub fn normalize(mut v: Settings) -> Settings {
    v.mode = v.mode.clamp(0, 3);
    v.width_percent = if v.width_percent.is_finite() { cpp_clamp(v.width_percent, 20.0, 100.0) } else { DEFAULTS.width_percent };
    v.height_percent = if v.height_percent.is_finite() { cpp_clamp(v.height_percent, 20.0, 100.0) } else { DEFAULTS.height_percent };
    let (max_x, max_y) = ((100.0 - v.width_percent) * 0.5, (100.0 - v.height_percent) * 0.5);
    v.offset_x_percent = cpp_clamp(if v.offset_x_percent.is_finite() { v.offset_x_percent } else { DEFAULTS.offset_x_percent }, -max_x, max_x);
    v.offset_y_percent = cpp_clamp(if v.offset_y_percent.is_finite() { v.offset_y_percent } else { DEFAULTS.offset_y_percent }, -max_y, max_y);
    v.parsed_dimensions = 0;
    v
}
pub fn enabled(v: Settings) -> bool {
    normalize(v).mode != 0
}

/// cvr::foveated::SettingsDelay.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SettingsDelay {
    pub requested: Settings,
    pub applied: Settings,
    pub apply_after: u64,
    pub editing: bool,
}
const _: () = assert!(core::mem::size_of::<SettingsDelay>() == 72 && core::mem::offset_of!(SettingsDelay, apply_after) == 56);
impl SettingsDelay {
    pub const NEW: SettingsDelay = SettingsDelay { requested: DEFAULTS, applied: DEFAULTS, apply_after: 0, editing: false };
    /// A new request: a change that needs a rebuild waits a second; the boundary overlay is applied at once, and Off
    /// never waits for a pending rebuild.
    pub fn request(&mut self, value: Settings, now: u64) {
        let value = normalize(value);
        if !value.same_reconstruction(&self.requested) {
            self.apply_after = now + 1000;
        }
        self.requested = value;
        self.applied.show_boundary = value.show_boundary;
        if !enabled(value) {
            self.applied = value;
        }
    }
    pub fn set_editing(&mut self, active: bool, now: u64) {
        if active || self.editing {
            self.apply_after = now + 1000;
        }
        self.editing = active;
    }
    pub fn update(&mut self, now: u64) {
        if !self.editing && now >= self.apply_after {
            self.applied = self.requested;
        }
    }
}

/// CenterRegion: the full-resolution centre, at least 32 render pixels, placed by the offset and clamped inside.
pub fn center_region(settings: Settings, render_w: u32, render_h: u32, output_w: u32, output_h: u32) -> Region {
    if render_w == 0 || render_h == 0 || output_w == 0 || output_h == 0 {
        return Region::default();
    }
    let s = normalize(settings);
    let mut r = Region {
        input_w: render_w.min(32u32.max(lroundf(render_w as f32 * s.width_percent / 100.0) as u32)),
        input_h: render_h.min(32u32.max(lroundf(render_h as f32 * s.height_percent / 100.0) as u32)),
        ..Region::default()
    };
    let origin = |full: u32, size: u32, offset: f32| {
        let o = ((full - size) / 2) as i32 + lroundf(full as f32 * offset / 100.0);
        o.clamp(0, (full - size) as i32) as u32
    };
    r.source_x = origin(render_w, r.input_w, s.offset_x_percent);
    r.source_y = origin(render_h, r.input_h, s.offset_y_percent);
    r.width = (r.input_w as f64 * output_w as f64 / render_w as f64).ceil() as u32;
    r.height = (r.input_h as f64 * output_h as f64 / render_h as f64).ceil() as u32;
    r.x = (r.source_x as f64 * output_w as f64 / render_w as f64) as u32;
    r.y = (r.source_y as f64 * output_h as f64 / render_h as f64) as u32;
    r
}

/// ParseSetting: one ini line, as `sscanf_s` reads it; xr_foveated_edges (the old form) sets the axes no explicit key set.
///
/// # Safety
/// `line` is a C string.
pub unsafe fn parse_setting(line: *const c_char, v: &mut Settings) -> bool {
    let (mut number, mut amount) = (0i32, 0f32);
    // SAFETY: the caller's line; each field converted into its own type.
    unsafe {
        if scan(line, c"xr_foveated_dlss = %d", &[ptr(&mut number)]) == 1 {
            v.mode = number;
            return true;
        }
        if scan(line, c"xr_foveated_width = %f", &[ptr(&mut amount)]) == 1 {
            v.width_percent = amount;
            v.parsed_dimensions |= 1;
            return true;
        }
        if scan(line, c"xr_foveated_height = %f", &[ptr(&mut amount)]) == 1 {
            v.height_percent = amount;
            v.parsed_dimensions |= 2;
            return true;
        }
        if scan(line, c"xr_foveated_offset_x = %f", &[ptr(&mut amount)]) == 1 {
            v.offset_x_percent = amount;
            return true;
        }
        if scan(line, c"xr_foveated_offset_y = %f", &[ptr(&mut amount)]) == 1 {
            v.offset_y_percent = amount;
            return true;
        }
        if scan(line, c"xr_foveated_edges = %f", &[ptr(&mut amount)]) == 1 {
            let size = 100.0 - 2.0 * cpp_clamp(amount, 0.0, 40.0);
            if v.parsed_dimensions & 1 == 0 {
                v.width_percent = if amount.is_finite() { size } else { DEFAULTS.width_percent };
            }
            if v.parsed_dimensions & 2 == 0 {
                v.height_percent = if amount.is_finite() { size } else { DEFAULTS.height_percent };
            }
            return true;
        }
        if scan(line, c"xr_foveated_boundary = %d", &[ptr(&mut number)]) == 1 {
            v.show_boundary = number != 0;
            return true;
        }
    }
    false
}

/// WriteSettings' text.
pub fn settings_text(v: Settings) -> std::ffi::CString {
    let v = normalize(v);
    format(
        c"xr_foveated_dlss=%d\nxr_foveated_width=%.2f\nxr_foveated_height=%.2f\nxr_foveated_boundary=%d\nxr_foveated_offset_x=%.2f\nxr_foveated_offset_y=%.2f\n",
        &[
            Arg::I(v.mode),
            Arg::F(v.width_percent as f64),
            Arg::F(v.height_percent as f64),
            Arg::I(v.show_boundary as i32),
            Arg::F(v.offset_x_percent as f64),
            Arg::F(v.offset_y_percent as f64),
        ],
    )
}

// ---- the settings the passes read (GetSettings / SetSettings ...)

static DELAY: Mutex<SettingsDelay> = Mutex::new(SettingsDelay::NEW);
fn delay() -> std::sync::MutexGuard<'static, SettingsDelay> {
    DELAY.lock().unwrap_or_else(|p| p.into_inner())
}
fn tick_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

// ---- the C ABI (include/Render/FoveatedDlss.hpp)

#[no_mangle]
pub extern "C" fn cvr_foveated_normalize(v: Settings) -> Settings {
    normalize(v)
}
#[no_mangle]
pub extern "C" fn cvr_foveated_enabled(v: Settings) -> bool {
    enabled(v)
}
#[no_mangle]
pub extern "C" fn cvr_foveated_get_settings() -> Settings {
    delay().requested
}
#[no_mangle]
pub extern "C" fn cvr_foveated_get_applied_settings() -> Settings {
    delay().applied
}
#[no_mangle]
pub extern "C" fn cvr_foveated_set_settings(v: Settings) {
    delay().request(v, tick_ms());
}
#[no_mangle]
pub extern "C" fn cvr_foveated_set_ui_editing(active: bool) {
    delay().set_editing(active, tick_ms());
}
#[no_mangle]
pub extern "C" fn cvr_foveated_update_settings() {
    delay().update(tick_ms());
}
#[no_mangle]
pub extern "C" fn cvr_foveated_center_region(v: Settings, render_w: u32, render_h: u32, output_w: u32, output_h: u32) -> Region {
    center_region(v, render_w, render_h, output_w, output_h)
}
/// # Safety
/// `line` is a C string, `v` a Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_parse_setting(line: *const c_char, v: *mut Settings) -> bool {
    // SAFETY: the caller's line and settings.
    match unsafe { v.as_mut() } {
        Some(v) if !line.is_null() => unsafe { parse_setting(line, v) },
        _ => false,
    }
}
/// # Safety
/// `file` is an open C runtime FILE.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_write_settings(file: *mut c_void, v: Settings) {
    // SAFETY: the caller's FILE.
    unsafe { cvr_crt::fputs_to(file, &settings_text(v)) };
}
/// SettingsDelay::Request on the caller's delay.
///
/// # Safety
/// `d` is a SettingsDelay.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_delay_request(d: *mut SettingsDelay, v: Settings, now: u64) {
    // SAFETY: the caller's delay.
    if let Some(d) = unsafe { d.as_mut() } {
        d.request(v, now);
    }
}
/// # Safety
/// `d` is a SettingsDelay.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_delay_set_editing(d: *mut SettingsDelay, active: bool, now: u64) {
    // SAFETY: the caller's delay.
    if let Some(d) = unsafe { d.as_mut() } {
        d.set_editing(active, now);
    }
}
/// # Safety
/// `d` is a SettingsDelay.
#[no_mangle]
pub unsafe extern "C" fn cvr_foveated_delay_update(d: *mut SettingsDelay, now: u64) {
    // SAFETY: the caller's delay.
    if let Some(d) = unsafe { d.as_mut() } {
        d.update(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_region_as_the_bench() {
        let s = Settings { mode: 1, width_percent: 60.0, height_percent: 60.0, show_boundary: false, offset_x_percent: 0.0, offset_y_percent: 0.0, parsed_dimensions: 0 };
        let r = center_region(s, 1485, 1485, 2560, 2560);
        assert_eq!((r.input_w, r.input_h), (891, 891));
        assert_eq!(r.source_x, (1485 - 891) / 2);
        assert!(r.width >= 1535 && r.x + r.width <= 2560);
    }

    #[test]
    fn the_delay_holds_a_rebuild_a_second() {
        let mut d = SettingsDelay::NEW;
        let mut s = DEFAULTS;
        s.mode = 1;
        d.request(s, 0);
        d.update(500);
        assert_eq!(d.applied.mode, 0);
        d.update(1000);
        assert_eq!(d.applied.mode, 1);
        s.mode = 0;
        d.request(s, 2000);
        assert_eq!(d.applied.mode, 0);
    }

    #[test]
    fn edges_fill_only_the_axes_not_set() {
        let mut v = DEFAULTS;
        unsafe {
            assert!(parse_setting(c"xr_foveated_width = 50".as_ptr(), &mut v));
            assert!(parse_setting(c"xr_foveated_edges = 10".as_ptr(), &mut v));
        }
        assert_eq!((v.width_percent, v.height_percent), (50.0, 80.0));
    }
}
