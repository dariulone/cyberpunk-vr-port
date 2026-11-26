//! Fixed foveated variable-rate shading's policy (formerly src/Render/FixedVrsSettings.cpp): its settings (the mode,
//! the full-rate centre's size and offset in percent of the view), the rate map over the shading-rate tiles, and the
//! pipelines the rate may apply to.
use core::ffi::{c_char, c_void};
use cvr_crt::{cpp_clamp, format, ptr, scan, Arg};
use windows::Win32::Graphics::Direct3D12::D3D12_GRAPHICS_PIPELINE_STATE_DESC;

/// cvr::vrs::Settings.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Off, 2x2, 4x4 (with a 2x2 transition).
    pub mode: i32,
    pub width: f32,
    pub height: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}
impl Default for Settings {
    fn default() -> Settings {
        Settings { mode: 0, width: 70.0, height: 65.0, offset_x: 0.0, offset_y: 0.0 }
    }
}
const _: () = assert!(core::mem::size_of::<Settings>() == 20);

/// D3D12_SHADING_RATE_1X1, _2X2, _4X4.
const RATE_1X1: u8 = 0;
const RATE_2X2: u8 = 5;
const RATE_4X4: u8 = 10;

/// Normalize: the mode 0..2, the centre 20..100% (non-finite: the defaults), its offset within the view.
pub fn normalize(mut s: Settings) -> Settings {
    let percent = |x: f32, fallback: f32| if x.is_finite() { cpp_clamp(x, 20.0, 100.0) } else { fallback };
    s.mode = s.mode.clamp(0, 2);
    s.width = percent(s.width, 70.0);
    s.height = percent(s.height, 65.0);
    let (x, y) = ((100.0 - s.width) * 0.5, (100.0 - s.height) * 0.5);
    s.offset_x = cpp_clamp(if s.offset_x.is_finite() { s.offset_x } else { 0.0 }, -x, x);
    s.offset_y = cpp_clamp(if s.offset_y.is_finite() { s.offset_y } else { 0.0 }, -y, y);
    s
}

/// ParseSetting: one ini line, as `sscanf_s` reads it.
///
/// # Safety
/// `line` is a C string.
pub unsafe fn parse_setting(line: *const c_char, s: &mut Settings) -> bool {
    // SAFETY: the caller's line; each field converted into its own type.
    unsafe {
        scan(line, c"xr_vrs = %d", &[ptr(&mut s.mode)]) == 1
            || scan(line, c"xr_vrs_width = %f", &[ptr(&mut s.width)]) == 1
            || scan(line, c"xr_vrs_height = %f", &[ptr(&mut s.height)]) == 1
            || scan(line, c"xr_vrs_offset_x = %f", &[ptr(&mut s.offset_x)]) == 1
            || scan(line, c"xr_vrs_offset_y = %f", &[ptr(&mut s.offset_y)]) == 1
    }
}

/// WriteSettings' text.
pub fn settings_text(s: Settings) -> std::ffi::CString {
    let s = normalize(s);
    format(
        c"xr_vrs=%d\nxr_vrs_width=%.2f\nxr_vrs_height=%.2f\nxr_vrs_offset_x=%.2f\nxr_vrs_offset_y=%.2f\n",
        &[Arg::I(s.mode), Arg::F(s.width as f64), Arg::F(s.height as f64), Arg::F(s.offset_x as f64), Arg::F(s.offset_y as f64)],
    )
}

/// The rate map's size for a view and a tile (0 when the view or the tile cannot have one).
pub fn rate_count(w: u32, h: u32, tile: u32) -> usize {
    if w == 0 || h == 0 || w > 16384 || h > 16384 || !(8..=32).contains(&tile) || tile & (tile - 1) != 0 {
        return 0;
    }
    (w.div_ceil(tile) as usize) * (h.div_ceil(tile) as usize)
}

/// BuildRates: full rate on every tile touching the centre, 2x2 outside it, and with mode 2 (and the additional
/// rates) 4x4 beyond a further 8% of the view.
pub fn build_rates(s: Settings, w: u32, h: u32, tile: u32, additional: bool, out: &mut [u8]) {
    let s = normalize(s);
    let columns = w.div_ceil(tile);
    let rows = h.div_ceil(tile);
    for r in out.iter_mut() {
        *r = RATE_1X1;
    }
    if s.mode == 0 {
        return;
    }
    let (wf, hf) = (w as f32, h as f32);
    let left = wf * (0.5 + (s.offset_x - s.width * 0.5) / 100.0);
    let right = left + wf * s.width / 100.0;
    let top = hf * (0.5 + (s.offset_y - s.height * 0.5) / 100.0);
    let bottom = top + hf * s.height / 100.0;
    for y in 0..rows {
        for x in 0..columns {
            let x0 = (x * tile) as f32;
            let y0 = (y * tile) as f32;
            let x1 = w.min((x + 1) * tile) as f32;
            let y1 = h.min((y + 1) * tile) as f32;
            let intersects = |mx: f32, my: f32| x1 > left - mx && x0 < right + mx && y1 > top - my && y0 < bottom + my;
            if intersects(0.0, 0.0) {
                continue;
            }
            out[(y * columns + x) as usize] = if s.mode == 2 && additional && !intersects(wf * 0.08, hf * 0.08) { RATE_4X4 } else { RATE_2X2 };
        }
    }
}

/// EligiblePipeline: a scene pass the rate may apply to -- 3 or 4 targets, vertex and pixel shaders and nothing else,
/// depth on, one sample, no alpha-to-coverage, no forced samples, no blending or logic ops.
pub fn eligible_pipeline(d: &D3D12_GRAPHICS_PIPELINE_STATE_DESC) -> bool {
    if d.NumRenderTargets < 3
        || d.NumRenderTargets > 4
        || d.PS.BytecodeLength == 0
        || d.VS.BytecodeLength == 0
        || !d.DepthStencilState.DepthEnable.as_bool()
        || d.SampleDesc.Count != 1
        || d.SampleDesc.Quality != 0
        || d.BlendState.AlphaToCoverageEnable.as_bool()
        || d.RasterizerState.ForcedSampleCount != 0
        || d.GS.BytecodeLength != 0
        || d.HS.BytecodeLength != 0
        || d.DS.BytecodeLength != 0
    {
        return false;
    }
    (0..d.NumRenderTargets as usize).all(|i| {
        let b = &d.BlendState.RenderTarget[if d.BlendState.IndependentBlendEnable.as_bool() { i } else { 0 }];
        !b.BlendEnable.as_bool() && !b.LogicOpEnable.as_bool()
    })
}

// ---- the C ABI (include/Render/FixedVrs.hpp)

#[no_mangle]
pub extern "C" fn cvr_vrs_normalize(s: Settings) -> Settings {
    normalize(s)
}
/// # Safety
/// `line` is a C string, `s` a Settings.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_parse_setting(line: *const c_char, s: *mut Settings) -> bool {
    // SAFETY: the caller's line and settings.
    match unsafe { s.as_mut() } {
        Some(s) if !line.is_null() => unsafe { parse_setting(line, s) },
        _ => false,
    }
}
/// # Safety
/// `file` is an open C runtime FILE.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_write_settings(file: *mut c_void, s: Settings) {
    // SAFETY: the caller's FILE.
    unsafe { cvr_crt::fputs_to(file, &settings_text(s)) };
}
#[no_mangle]
pub extern "C" fn cvr_vrs_rate_count(w: u32, h: u32, tile: u32) -> usize {
    rate_count(w, h, tile)
}
/// # Safety
/// `out` holds `count` bytes, cvr_vrs_rate_count's.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_build_rates(s: Settings, w: u32, h: u32, tile: u32, additional: bool, out: *mut u8, count: usize) {
    if out.is_null() || count == 0 || count != rate_count(w, h, tile) {
        return;
    }
    // SAFETY: the caller's buffer of `count` bytes.
    build_rates(s, w, h, tile, additional, unsafe { core::slice::from_raw_parts_mut(out, count) });
}
/// # Safety
/// `d` is a D3D12_GRAPHICS_PIPELINE_STATE_DESC.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_eligible_pipeline(d: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC) -> bool {
    // SAFETY: the caller's description.
    unsafe { d.as_ref() }.is_some_and(eligible_pipeline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_as_the_cpp() {
        let s = normalize(Settings { mode: 9, width: f32::NAN, height: 150.0, offset_x: 100.0, offset_y: f32::NAN });
        assert_eq!(s, Settings { mode: 2, width: 70.0, height: 100.0, offset_x: 15.0, offset_y: 0.0 });
    }

    #[test]
    fn rate_map_shapes() {
        assert_eq!(rate_count(256, 256, 0), 0);
        assert_eq!(rate_count(256, 256, 12), 0);
        assert_eq!(rate_count(100, 50, 16), 7 * 4);
        let s = Settings { mode: 2, width: 50.0, height: 50.0, offset_x: 0.0, offset_y: 0.0 };
        let mut map = vec![0u8; rate_count(256, 256, 16)];
        build_rates(s, 256, 256, 16, true, &mut map);
        assert_eq!(map[8 * 16 + 8], RATE_1X1);
        assert_eq!(map[0], RATE_4X4);
        assert!(map.contains(&RATE_2X2));
    }

    #[test]
    fn parses_and_writes() {
        let mut s = Settings::default();
        unsafe {
            assert!(parse_setting(c"xr_vrs = 2".as_ptr(), &mut s));
            assert!(parse_setting(c"xr_vrs_width=80".as_ptr(), &mut s));
            assert!(!parse_setting(c"xr_vrs_extra=1".as_ptr(), &mut s));
        }
        assert_eq!((s.mode, s.width), (2, 80.0));
        assert_eq!(
            settings_text(Settings { mode: 1, width: 70.0, height: 65.0, offset_x: -5.0, offset_y: 2.5 }).to_str().unwrap(),
            "xr_vrs=1\nxr_vrs_width=70.00\nxr_vrs_height=65.00\nxr_vrs_offset_x=-5.00\nxr_vrs_offset_y=2.50\n"
        );
    }
}
