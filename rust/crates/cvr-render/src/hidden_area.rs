//! The hidden-area mask's geometry (formerly src/Render/HiddenAreaSettings.cpp; docs/hidden-area-mask-20261009.md):
//! the settings (on, the visible ellipse's radius in percent of each half-FOV), whether an eye-space direction is
//! hidden (an ellipse normalised per quadrant to the eye's FOV; everything outside the panel's rectangle is hidden),
//! the rectangles of hidden cells a render target can skip (with a band of shaded cells kept around the visible area,
//! rows with identical runs merged), the ellipse the patched shaders test, and the share a set of rectangles covers.
// `!(a > b)` is the C++ form on purpose: a NaN tangent counts as hidden, as it did there.
#![allow(clippy::neg_cmp_op_on_partial_ord)]
use core::ffi::{c_char, c_void};
use cvr_crt::{cpp_clamp, format, ptr, scan, Arg};

/// cvr::hidden_area::Settings.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub enabled: i32,
    /// The visible ellipse's radius, percent of each half-FOV.
    pub radius: f32,
}
pub const DEFAULT_RADIUS: f32 = 115.0;
pub const MIN_RADIUS: f32 = 70.0;
pub const MAX_RADIUS: f32 = 145.0;

/// The eye's FOV as tangents: left/down negative, right/up positive.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fov {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub fn normalize(mut s: Settings) -> Settings {
    s.enabled = (s.enabled != 0) as i32;
    s.radius = if s.radius.is_finite() { cpp_clamp(s.radius, MIN_RADIUS, MAX_RADIUS) } else { DEFAULT_RADIUS };
    s
}

/// # Safety
/// `line` is a C string.
pub unsafe fn parse_setting(line: *const c_char, s: &mut Settings) -> bool {
    // SAFETY: the caller's line; each field converted into its own type.
    unsafe { scan(line, c"xr_hidden_area_radius = %f", &[ptr(&mut s.radius)]) == 1 || scan(line, c"xr_hidden_area = %d", &[ptr(&mut s.enabled)]) == 1 }
}

pub fn settings_text(s: Settings) -> std::ffi::CString {
    let s = normalize(s);
    format(c"xr_hidden_area=%d\nxr_hidden_area_radius=%.1f\n", &[Arg::I(s.enabled), Arg::F(s.radius as f64)])
}

/// Whether the direction with tangents (x right, y up) is outside the visible area.
pub fn hidden(f: &Fov, radius_percent: f32, x: f32, y: f32) -> bool {
    if !(x >= f.left && x <= f.right && y >= f.down && y <= f.up) {
        return true;
    }
    // Normalised per quadrant: an asymmetric panel gets an ellipse touching each of its own edges at 100%.
    let extent_x = if x < 0.0 { -f.left } else { f.right };
    let extent_y = if y < 0.0 { -f.down } else { f.up };
    if !(extent_x > 0.0) || !(extent_y > 0.0) {
        return true;
    }
    let (nx, ny, r) = (x / extent_x, y / extent_y, radius_percent * 0.01);
    nx * nx + ny * ny > r * r
}

/// The axis-aligned ellipse about NDC 0 the patched shaders test (left, right, up, down scales), grown until it holds
/// every visible pixel of the window `rendered`; none when the centre itself is hidden.
pub fn shader_ellipse_scales(f: &Fov, radius_percent: f32, rendered: &Fov) -> Option<[f32; 4]> {
    let sx = rendered.right - rendered.left;
    let sy = rendered.up - rendered.down;
    if !(sx > 0.0) || !(sy > 0.0) {
        return None;
    }
    // NDC (y up) -> tangent of the rendered window.
    let visible = |nx: f32, ny: f32| !hidden(f, radius_percent, rendered.left + (nx + 1.0) * 0.5 * sx, rendered.down + (ny + 1.0) * 0.5 * sy);
    if !visible(0.0, 0.0) {
        return None;
    }
    const STEPS: i32 = 512;
    let dir: [[f32; 2]; 4] = [[-1.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, -1.0]];
    let mut scales = [0.0f32; 4];
    for i in 0..4 {
        let mut last = 0;
        for k in 1..=STEPS {
            if visible(dir[i][0] * k as f32 / STEPS as f32, dir[i][1] * k as f32 / STEPS as f32) {
                last = k;
            }
        }
        let extent = 1.0f32.min((last + 1) as f32 / STEPS as f32);
        scales[i] = 1.0 / (extent * extent);
    }
    // Off-centre the visible area is not an ellipse about NDC 0: grow the fit until it holds all of it.
    const GRID: i32 = 160;
    let mut worst = 1.0f32;
    for y in 0..=GRID {
        for x in 0..=GRID {
            let nx = 2.0 * x as f32 / GRID as f32 - 1.0;
            let ny = 2.0 * y as f32 / GRID as f32 - 1.0;
            if !visible(nx, ny) {
                continue;
            }
            // One grid cell of slack: the test samples, the shader sees every pixel.
            let ax = nx.abs() + 1.0 / GRID as f32;
            let ay = ny.abs() + 1.0 / GRID as f32;
            worst = worst.max(scales[if nx < 0.0 { 0 } else { 1 }] * ax * ax + scales[if ny > 0.0 { 2 } else { 3 }] * ay * ay);
        }
    }
    for s in scales.iter_mut() {
        *s /= worst;
    }
    Some(scales)
}

/// The rectangles of `cell`-pixel cells hidden in full whose neighbours within `margin` cells are hidden too, for a
/// width x height target rendering the window `rendered`; rows with identical runs merged.
pub fn build_rects(f: &Fov, radius_percent: f32, w: u32, h: u32, rendered: &Fov, cell: u32, margin: u32) -> Vec<Rect> {
    let span_x = rendered.right - rendered.left;
    let span_y = rendered.up - rendered.down;
    if w == 0 || h == 0 || w > 16384 || h > 16384 || !(span_x > 0.0) || !(span_y > 0.0) || cell == 0 || cell > 64 || margin > 8 {
        return Vec::new();
    }
    let columns = w.div_ceil(cell);
    let rows = h.div_ceil(cell);
    let (wf, hf) = (w as f32, h as f32);
    // Pixel position -> tangent of the rendered window (y up).
    let hid = |px: f32, py: f32| hidden(f, radius_percent, rendered.left + px / wf * span_x, rendered.up - py / hf * span_y);
    let mut full = vec![false; (columns * rows) as usize];
    for r in 0..rows {
        for c in 0..columns {
            let x0 = (c * cell) as f32;
            let y0 = (r * cell) as f32;
            let x1 = w.min((c + 1) * cell) as f32;
            let y1 = h.min((r + 1) * cell) as f32;
            let (xm, ym) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
            full[(r * columns + c) as usize] = hid(x0, y0)
                && hid(x1, y0)
                && hid(x0, y1)
                && hid(x1, y1)
                && hid(xm, y0)
                && hid(xm, y1)
                && hid(x0, ym)
                && hid(x1, ym)
                && hid(xm, ym);
        }
    }
    // A band of shaded cells around the visible area: filters reading neighbours never reach a masked pixel from a
    // visible one. Off-target counts as hidden.
    let m = margin as i32;
    let mut masked = vec![false; full.len()];
    for r in 0..rows as i32 {
        for c in 0..columns as i32 {
            let mut keep = full[(r as u32 * columns + c as u32) as usize];
            let mut dy = -m;
            while keep && dy <= m {
                let mut dx = -m;
                while keep && dx <= m {
                    let (rr, cc) = (r + dy, c + dx);
                    if !(rr < 0 || cc < 0 || rr >= rows as i32 || cc >= columns as i32) {
                        keep = full[(rr as u32 * columns + cc as u32) as usize];
                    }
                    dx += 1;
                }
                dy += 1;
            }
            masked[(r as u32 * columns + c as u32) as usize] = keep;
        }
    }
    let mut out: Vec<Rect> = Vec::new();
    // A row's runs of masked cells, [first, end).
    let mut previous: Vec<(u32, u32)> = Vec::new();
    let mut current: Vec<(u32, u32)> = Vec::new();
    let (mut previous_index, mut current_index): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    for r in 0..rows {
        current.clear();
        current_index.clear();
        let mut c = 0;
        while c < columns {
            if !masked[(r * columns + c) as usize] {
                c += 1;
                continue;
            }
            let mut end = c;
            while end < columns && masked[(r * columns + end) as usize] {
                end += 1;
            }
            current.push((c, end));
            c = end;
        }
        for run in &current {
            if let Some(i) = previous.iter().position(|p| p == run) {
                let index = previous_index[i];
                out[index].bottom = h.min((r + 1) * cell) as i32;
                current_index.push(index);
            } else {
                out.push(Rect { left: (run.0 * cell) as i32, top: (r * cell) as i32, right: w.min(run.1 * cell) as i32, bottom: h.min((r + 1) * cell) as i32 });
                current_index.push(out.len() - 1);
            }
        }
        core::mem::swap(&mut previous, &mut current);
        core::mem::swap(&mut previous_index, &mut current_index);
    }
    out
}

/// The share of a width x height target the rectangles cover.
pub fn coverage(rects: &[Rect], w: u32, h: u32) -> f64 {
    if w == 0 || h == 0 {
        return 0.0;
    }
    // Summed from +0.0 as the C++ loop did (Iterator::sum starts at -0.0: an empty set would read -0.0).
    let area = rects.iter().fold(0.0f64, |a, r| a + (r.right - r.left) as f64 * (r.bottom - r.top) as f64);
    area / (w as f64 * h as f64)
}

// ---- the C ABI (include/Render/HiddenArea.hpp)

#[no_mangle]
pub extern "C" fn cvr_hidden_area_normalize(s: Settings) -> Settings {
    normalize(s)
}
/// # Safety
/// `line` is a C string, `s` a Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_parse_setting(line: *const c_char, s: *mut Settings) -> bool {
    // SAFETY: the caller's line and settings.
    match unsafe { s.as_mut() } {
        Some(s) if !line.is_null() => unsafe { parse_setting(line, s) },
        _ => false,
    }
}
/// # Safety
/// `file` is an open C runtime FILE.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_write_settings(file: *mut c_void, s: Settings) {
    // SAFETY: the caller's FILE.
    unsafe { cvr_crt::fputs_to(file, &settings_text(s)) };
}
/// # Safety
/// `f` is a Fov.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_hidden(f: *const Fov, radius_percent: f32, x: f32, y: f32) -> bool {
    // SAFETY: the caller's FOV.
    unsafe { f.as_ref() }.is_none_or(|f| hidden(f, radius_percent, x, y))
}
/// # Safety
/// `f` and `rendered` are Fov; `scales` holds four floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_shader_ellipse_scales(f: *const Fov, radius_percent: f32, rendered: *const Fov, scales: *mut f32) -> bool {
    // SAFETY: the caller's FOVs and scales.
    unsafe {
        let (Some(f), Some(rendered)) = (f.as_ref(), rendered.as_ref()) else { return false };
        if scales.is_null() {
            return false;
        }
        match shader_ellipse_scales(f, radius_percent, rendered) {
            Some(s) => {
                core::ptr::copy_nonoverlapping(s.as_ptr(), scales, 4);
                true
            }
            None => false,
        }
    }
}
/// The rectangles into `out` (at most `capacity`; ceil(w/cell) * ceil(h/cell) always suffices); their count.
///
/// # Safety
/// `f` and `rendered` are Fov; `out` holds `capacity` Rects.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_build_rects(
    f: *const Fov,
    radius_percent: f32,
    w: u32,
    h: u32,
    rendered: *const Fov,
    cell: u32,
    margin: u32,
    out: *mut Rect,
    capacity: usize,
) -> usize {
    // SAFETY: the caller's FOVs and buffer.
    unsafe {
        let (Some(f), Some(rendered)) = (f.as_ref(), rendered.as_ref()) else { return 0 };
        let rects = build_rects(f, radius_percent, w, h, rendered, cell, margin);
        let n = rects.len().min(capacity);
        if n > 0 && !out.is_null() {
            core::ptr::copy_nonoverlapping(rects.as_ptr(), out, n);
        }
        rects.len()
    }
}
/// # Safety
/// `rects` holds `count` Rects.
#[no_mangle]
pub unsafe extern "C" fn cvr_hidden_area_coverage(rects: *const Rect, count: usize, w: u32, h: u32) -> f64 {
    let rects = if rects.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(rects, count) } };
    coverage(rects, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_as_the_bench() {
        let square = Fov { left: -1.0, right: 1.0, up: 1.0, down: -1.0 };
        assert!(!hidden(&square, 100.0, 0.0, 0.0));
        assert!(hidden(&square, 100.0, 0.9, 0.9));
        assert!(!hidden(&square, 142.0, 0.99, 0.99));
        assert!(hidden(&square, 142.0, 1.01, 0.0));
    }

    #[test]
    fn rects_cover_the_corners() {
        let square = Fov { left: -1.0, right: 1.0, up: 1.0, down: -1.0 };
        let r = build_rects(&square, 100.0, 256, 256, &square, 8, 0);
        let c = coverage(&r, 256, 256);
        assert!(c > 0.1 && c < 0.25, "{c}");
        assert!(shader_ellipse_scales(&square, 100.0, &square).is_some());
    }
}
