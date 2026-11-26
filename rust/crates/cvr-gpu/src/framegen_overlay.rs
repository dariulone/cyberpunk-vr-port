//! The frame generation's statistics panel (formerly the drawing half of src/Framegen/StatsOverlay.cpp): a small
//! persistent bitmap in a five-by-seven pixel font -- the rates, the frame times with their graphs, the hardware's load,
//! memory and cores, the generator -- refreshed four times a second by the XR quad that shows it. No ImGui context,
//! font atlas or per-frame raster. The text goes through the C runtime's snprintf, so every number reads as the C++
//! wrote it.
use crate::framegen_core::{Settings, Statistics, GRAPH_POINTS};
use crate::hardware_telemetry::HardwareStatistics;

/// Five columns, seven rows; all text is a bounded diagnostic alphabet.
fn glyph(c: u8) -> [u8; 5] {
    const DIGITS: [[u8; 5]; 10] = [
        [62, 81, 73, 69, 62],
        [0, 66, 127, 64, 0],
        [66, 97, 81, 73, 70],
        [33, 65, 69, 75, 49],
        [24, 20, 18, 127, 16],
        [39, 69, 69, 69, 57],
        [60, 74, 73, 73, 48],
        [1, 113, 9, 5, 3],
        [54, 73, 73, 73, 54],
        [6, 73, 73, 41, 30],
    ];
    const LETTERS: [[u8; 5]; 26] = [
        [126, 17, 17, 17, 126],
        [127, 73, 73, 73, 54],
        [62, 65, 65, 65, 34],
        [127, 65, 65, 34, 28],
        [127, 73, 73, 73, 65],
        [127, 9, 9, 9, 1],
        [62, 65, 73, 73, 122],
        [127, 8, 8, 8, 127],
        [0, 65, 127, 65, 0],
        [32, 64, 65, 63, 1],
        [127, 8, 20, 34, 65],
        [127, 64, 64, 64, 64],
        [127, 2, 12, 2, 127],
        [127, 4, 8, 16, 127],
        [62, 65, 65, 65, 62],
        [127, 9, 9, 9, 6],
        [62, 65, 81, 33, 94],
        [127, 9, 25, 41, 70],
        [70, 73, 73, 73, 49],
        [1, 1, 127, 1, 1],
        [63, 64, 64, 64, 63],
        [31, 32, 64, 32, 31],
        [63, 64, 56, 64, 63],
        [99, 20, 8, 20, 99],
        [7, 8, 112, 8, 7],
        [97, 81, 73, 69, 67],
    ];
    const LOWERCASE: [[u8; 5]; 26] = [
        [32, 84, 84, 84, 120],
        [127, 72, 68, 68, 56],
        [56, 68, 68, 68, 32],
        [56, 68, 68, 72, 127],
        [56, 84, 84, 84, 24],
        [8, 126, 9, 1, 2],
        [12, 82, 82, 82, 62],
        [127, 8, 4, 4, 120],
        [0, 68, 125, 64, 0],
        [32, 64, 68, 61, 0],
        [127, 16, 40, 68, 0],
        [0, 65, 127, 64, 0],
        [124, 4, 24, 4, 120],
        [124, 8, 4, 4, 120],
        [56, 68, 68, 68, 56],
        [124, 20, 20, 20, 8],
        [8, 20, 20, 24, 124],
        [124, 8, 4, 4, 8],
        [72, 84, 84, 84, 32],
        [4, 63, 68, 64, 32],
        [60, 64, 64, 32, 124],
        [28, 32, 64, 32, 28],
        [60, 64, 48, 64, 60],
        [68, 40, 16, 40, 68],
        [12, 80, 80, 80, 60],
        [68, 100, 84, 76, 68],
    ];
    match c {
        b'0'..=b'9' => DIGITS[(c - b'0') as usize],
        b'A'..=b'Z' => LETTERS[(c - b'A') as usize],
        b'a'..=b'z' => LOWERCASE[(c - b'a') as usize],
        b'+' => [8, 8, 62, 8, 8],
        b'(' => [0, 28, 34, 65, 0],
        b')' => [0, 65, 34, 28, 0],
        b'%' => [99, 19, 8, 100, 99],
        b':' => [0, 54, 54, 0, 0],
        b'/' => [32, 16, 8, 4, 2],
        b'.' => [0, 96, 96, 0, 0],
        b'-' => [8, 8, 8, 8, 8],
        _ => [0; 5],
    }
}

/// The panel's pixels (0xAABBGGRR as the upload copies them).
pub struct Canvas<'a> {
    pub pixels: &'a mut [u32],
    pub width: i32,
    pub height: i32,
}

impl Canvas<'_> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        for row in y.max(0)..self.height.min(y + h) {
            if x < self.width && x + w > 0 {
                let from = (row * self.width + x.max(0)) as usize;
                let to = (row * self.width + self.width.min(x + w)) as usize;
                self.pixels[from..to].fill(color);
            }
        }
    }
    fn text_scaled(&mut self, mut x: i32, y: i32, text: &str, color: u32, scale: i32) {
        for &c in text.as_bytes() {
            if x + 5 * scale > self.width {
                break;
            }
            let g = glyph(c);
            for (col, bits) in g.iter().enumerate() {
                for row in 0..7 {
                    if bits & (1u8 << row) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let (py, px) = (y + row * scale + dy, x + col as i32 * scale + dx);
                            if py >= 0 && py < self.height && px >= 0 {
                                self.pixels[(py * self.width + px) as usize] = color;
                            }
                        }
                    }
                }
            }
            x += 6 * scale;
        }
    }
    fn text(&mut self, x: i32, y: i32, text: &str) {
        self.text_scaled(x, y, text, 0xFFF2_F2F2, 2);
    }
    fn text_fit(&mut self, y: i32, text: &str, scale: i32) {
        let scale = if text.len() * 6 * scale as usize > (self.width - 24) as usize {
            1
        } else {
            scale
        };
        self.text_scaled(12, y, text, 0xFFF2_F2F2, scale);
    }
    fn panel(&mut self) {
        let (w, h) = (self.width, self.height);
        for y in 0..h {
            for x in 0..w {
                let dx = (12 - x).max(x - (w - 13)).max(0);
                let dy = (12 - y).max(y - (h - 13)).max(0);
                let corner = dx != 0 || dy != 0;
                let radius = dx * dx + dy * dy;
                self.pixels[(y * w + x) as usize] = if radius > 144 {
                    0
                } else if (corner && radius > 100) || x < 2 || y < 2 || x >= w - 2 || y >= h - 2 {
                    0xFFA4_A4A4
                } else {
                    0xDA21_170F
                };
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn graph(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        values: &[f32; GRAPH_POINTS],
        budget: f64,
        color: u32,
    ) {
        self.rect(x - 1, y - 1, w + 2, h + 2, 0xFF68_6868);
        self.rect(x, y, w, h, 0xFF21_170F);
        let ceiling = 33.34f64.max(budget * 2.0).clamp(20.0, 100.0);
        let line = y + h - 1 - ((budget / ceiling).clamp(0.0, 1.0) * (h - 1) as f64) as i32;
        self.rect(x, line, w, 1, 0xFF66_6666);
        let n = GRAPH_POINTS as i32;
        for (i, &v) in values.iter().enumerate() {
            let i = i as i32;
            let (left, right) = (x + i * w / n, x + (i + 1) * w / n);
            let bar = ((v as f64 / ceiling * (h - 1) as f64) as i32).clamp(0, h - 1);
            let c = if v as f64 > budget * 1.2 {
                0xFF52_6CFF
            } else {
                color
            };
            self.rect(left, y + h - bar - 1, (right - left).max(1), bar + 1, c);
        }
    }
    fn cores(&mut self, x: i32, y: i32, w: i32, h: i32, hardware: &HardwareStatistics) {
        if hardware.core_count == 0 || hardware.core_maximum < 0.0 {
            self.text(x, y, "--");
            return;
        }
        let n = hardware.core_count as i32;
        for i in 0..n {
            let (left, right) = (x + i * w / n, x + (i + 1) * w / n);
            self.rect(left, y, (right - left - 2).max(1), h, 0xFF44_382B);
            let load = hardware.cores[i as usize];
            let used = (load.clamp(0.0, 100.0) * h as f32 / 100.0) as i32;
            let c = if load > 90.0 {
                0xFF52_6CFF
            } else {
                0xFF98_EEA5
            };
            self.rect(left, y + h - used, (right - left - 2).max(1), used, c);
        }
    }
}

/// snprintf(format, ...) as the C runtime formats it.
fn f(format: &core::ffi::CStr, args: &[cvr_crt::Arg]) -> String {
    cvr_crt::format(format, args).to_string_lossy().into_owned()
}
use cvr_crt::Arg::{F, I};

/// Number: "--" when unknown, negative or not finite; one decimal, or none.
fn number(value: f64, known: bool, precision: u32) -> String {
    if !known || value < 0.0 || !value.is_finite() {
        return "--".into();
    }
    f(if precision != 0 { c"%.1f" } else { c"%.0f" }, &[F(value)])
}
fn n(value: f64) -> String {
    number(value, true, 1)
}
fn c_text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// BackendName.
pub fn backend_name(backend: i32) -> &'static str {
    match backend {
        1 => "NVIDIA OFA + FidelityFX",
        2 => "NVIDIA DLSS-G",
        4 => "FSR3 interpolation",
        5 => "FSR3 extrapolation",
        _ => "FidelityFX",
    }
}

/// The panel's size for a layout: detailed, compact, minimal.
pub fn panel_size(layout: i32) -> (u32, u32) {
    match layout {
        0 => (720, 452),
        1 => (420, 404),
        _ => (360, 222),
    }
}

/// Draw: the panel for these settings and statistics; `clock` is the local time (hour, minute) the detailed layout
/// shows and `backend` the active generator's.
pub fn draw(
    c: &mut Canvas,
    settings: &Settings,
    stats: &Statistics,
    clock: (u32, u32),
    backend: i32,
) {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let hw = &stats.hardware;
    c.panel();
    let low1 = number(stats.low1_fps, stats.low1_fps > 0.0, 1);
    let low01 = number(stats.low01_fps, stats.low01_fps > 0.0, 1);
    let (gpu, cpu, temperature) = (
        n(hw.gpu_usage),
        n(hw.cpu_usage),
        number(hw.gpu_temperature, true, 0),
    );
    let ram = number(hw.ram_used as f64 / GB, hw.ram_total != 0, 1);
    let ram_total = number(hw.ram_total as f64 / GB, hw.ram_total != 0, 1);
    let vram = number(hw.gpu_memory_used as f64 / GB, hw.gpu_memory_valid, 1);
    let vram_total = number(hw.gpu_memory_total as f64 / GB, hw.gpu_memory_total != 0, 1);
    let s = |format: &core::ffi::CStr, args: &[cvr_crt::Arg]| f(format, args);
    // %s arguments go through CStrings kept alive for the call.
    let cs = |text: &str| std::ffi::CString::new(text).unwrap_or_default();
    let sp = |text: &std::ffi::CString| cvr_crt::Arg::S(text.as_ptr());
    let (low1_c, low01_c, gpu_c, cpu_c, temp_c) =
        (cs(&low1), cs(&low01), cs(&gpu), cs(&cpu), cs(&temperature));
    let (ram_c, ram_total_c, vram_c, vram_total_c) =
        (cs(&ram), cs(&ram_total), cs(&vram), cs(&vram_total));
    match settings.overlay_layout {
        0 => {
            c.text(
                12,
                10,
                &s(
                    c"cyberpunk-vr-port  %.1f MIN",
                    &[F(stats.elapsed_seconds / 60.0)],
                ),
            );
            c.text(
                c.width - 76,
                10,
                &s(c"%02u:%02u", &[I(clock.0 as i32), I(clock.1 as i32)]),
            );
            c.text(
                12,
                36,
                &s(
                    c"REAL %.1f  FG %.1f  OUT %.1f",
                    &[
                        F(stats.real_fps),
                        F(stats.generated_fps),
                        F(stats.output_fps),
                    ],
                ),
            );
            c.text_fit(
                60,
                &s(
                    c"AVG %.1f FPS   MIN %.1f   MAX %.1f",
                    &[
                        F(stats.average_fps),
                        F(stats.minimum_fps),
                        F(stats.maximum_fps),
                    ],
                ),
                2,
            );
            c.text(
                12,
                84,
                &s(
                    c"1%% LOW %s  0.1%% LOW %s  %dS",
                    &[sp(&low1_c), sp(&low01_c), I(settings.history_seconds)],
                ),
            );
            c.text(
                12,
                108,
                &s(
                    c"GPU %.1fMS AVG %.1f",
                    &[F(stats.gpu_ms), F(stats.gpu_average_ms)],
                ),
            );
            c.text(
                372,
                108,
                &s(
                    c"CPU %.1fMS AVG %.1f",
                    &[F(stats.cpu_ms), F(stats.cpu_average_ms)],
                ),
            );
            c.graph(
                12,
                130,
                330,
                76,
                &stats.gpu_graph,
                stats.budget_ms,
                0xFF98_EEA5,
            );
            c.graph(
                372,
                130,
                330,
                76,
                &stats.cpu_graph,
                stats.budget_ms,
                0xFF98_ECFF,
            );
            let (g99, g999) = (
                cs(&number(stats.gpu_p99_ms, stats.gpu_p99_ms > 0.0, 1)),
                cs(&number(stats.gpu_p999_ms, stats.gpu_p999_ms > 0.0, 1)),
            );
            c.text(12, 214, &s(c"P99 %s  P99.9 %s", &[sp(&g99), sp(&g999)]));
            let (c99, c999) = (
                cs(&number(stats.cpu_p99_ms, stats.cpu_p99_ms > 0.0, 1)),
                cs(&number(stats.cpu_p999_ms, stats.cpu_p999_ms > 0.0, 1)),
            );
            c.text(372, 214, &s(c"P99 %s  P99.9 %s", &[sp(&c99), sp(&c999)]));
            c.text(12, 238, &s(c"GPU %s%%  %sC", &[sp(&gpu_c), sp(&temp_c)]));
            let core_max = cs(&number(hw.core_maximum, true, 0));
            c.text(
                372,
                238,
                &s(c"CPU %s%% CORE MAX %s%%", &[sp(&cpu_c), sp(&core_max)]),
            );
            c.text(
                12,
                262,
                &s(c"VRAM %s/%s GB", &[sp(&vram_c), sp(&vram_total_c)]),
            );
            c.text(
                372,
                262,
                &s(c"RAM %s/%s GB", &[sp(&ram_c), sp(&ram_total_c)]),
            );
            let app_vram = cs(&number(
                hw.gpu_process_used as f64 / GB,
                hw.gpu_process_memory_valid,
                1,
            ));
            c.text(12, 286, &s(c"APP VRAM %s GB", &[sp(&app_vram)]));
            c.text(
                372,
                286,
                &s(c"APP RAM %.1f GB", &[F(hw.app_working_set as f64 / GB)]),
            );
            c.text(12, 312, &s(c"CORES %u", &[I(hw.core_count as i32)]));
            c.cores(126, 310, 576, 20, hw);
            c.text(
                12,
                340,
                &s(
                    c"FG MEM %.0fM GEN %.1fMS",
                    &[
                        F(stats.vram_bytes as f64 / (1024.0 * 1024.0)),
                        F(stats.generation_ms),
                    ],
                ),
            );
            c.text(
                372,
                340,
                &s(
                    c"REPEAT %.1f%% SKIP %llu",
                    &[F(stats.repeat_percent), cvr_crt::Arg::U(stats.skipped)],
                ),
            );
        }
        1 => {
            c.text(
                12,
                10,
                &s(
                    c"REAL %.1f FG %.1f OUT %.1f",
                    &[
                        F(stats.real_fps),
                        F(stats.generated_fps),
                        F(stats.output_fps),
                    ],
                ),
            );
            c.text_fit(
                34,
                &s(
                    c"AVG %.1f MIN %.1f MAX %.1f",
                    &[
                        F(stats.average_fps),
                        F(stats.minimum_fps),
                        F(stats.maximum_fps),
                    ],
                ),
                2,
            );
            c.text_fit(
                58,
                &s(
                    c"1%% LOW %s 0.1%% %s %dS",
                    &[sp(&low1_c), sp(&low01_c), I(settings.history_seconds)],
                ),
                2,
            );
            c.text(
                12,
                84,
                &s(
                    c"GPU %s%% %sC %.1fMS",
                    &[sp(&gpu_c), sp(&temp_c), F(stats.gpu_ms)],
                ),
            );
            c.graph(
                12,
                106,
                396,
                58,
                &stats.gpu_graph,
                stats.budget_ms,
                0xFF98_EEA5,
            );
            c.text(
                12,
                174,
                &s(c"CPU %s%% %.1fMS", &[sp(&cpu_c), F(stats.cpu_ms)]),
            );
            c.graph(
                12,
                196,
                396,
                58,
                &stats.cpu_graph,
                stats.budget_ms,
                0xFF98_ECFF,
            );
            c.text(
                12,
                266,
                &s(c"VRAM %s/%s GB", &[sp(&vram_c), sp(&vram_total_c)]),
            );
            c.text(
                12,
                290,
                &s(c"RAM %s/%s GB", &[sp(&ram_c), sp(&ram_total_c)]),
            );
            c.cores(12, 312, 396, 12, hw);
        }
        _ => {
            c.text(
                12,
                10,
                &s(
                    c"REAL %.1f FG %.1f",
                    &[F(stats.real_fps), F(stats.generated_fps)],
                ),
            );
            c.text_fit(
                34,
                &s(
                    c"AVG %.1f MIN %.1f MAX %.1f",
                    &[
                        F(stats.average_fps),
                        F(stats.minimum_fps),
                        F(stats.maximum_fps),
                    ],
                ),
                2,
            );
            c.text(12, 58, &s(c"OUT %.1f FPS", &[F(stats.output_fps)]));
            c.text(
                12,
                82,
                &s(c"1%% %s  0.1%% %s", &[sp(&low1_c), sp(&low01_c)]),
            );
            c.text(12, 106, &s(c"GPU %s%% CPU %s%%", &[sp(&gpu_c), sp(&cpu_c)]));
            c.text(
                12,
                130,
                &s(c"G %.1fMS C %.1fMS", &[F(stats.gpu_ms), F(stats.cpu_ms)]),
            );
        }
    }
    let detailed = settings.overlay_layout == 0;
    let footer = if detailed {
        378
    } else if settings.overlay_layout == 1 {
        344
    } else {
        164
    };
    c.rect(12, footer - 10, c.width - 24, 1, 0xFF68_6868);
    let small = if detailed { 2 } else { 1 };
    let cpu_name = c_text(&hw.cpu_name);
    let gpu_name = c_text(&hw.gpu_name);
    c.text_fit(
        footer,
        &format!(
            "CPU: {}",
            if cpu_name.is_empty() { "--" } else { &cpu_name }
        ),
        small,
    );
    c.text_fit(
        footer + if detailed { 24 } else { 14 },
        &format!(
            "GPU: {}",
            if gpu_name.is_empty() { "--" } else { &gpu_name }
        ),
        small,
    );
    let generator = if settings.enabled {
        backend_name(backend)
    } else {
        "Disabled"
    };
    c.text_fit(
        footer + if detailed { 48 } else { 32 },
        &format!("FG: {generator}"),
        2,
    );
}

/// cvr_framegen_overlay_draw: the panel into `pixels` (width x height), the C ABI the overlay's XR quad and
/// tools/framegen_overlay_tests call.
///
/// # Safety
/// `pixels` holds width x height words; `settings` and `stats` are live.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_framegen_overlay_draw(
    pixels: *mut u32,
    width: i32,
    height: i32,
    settings: *const Settings,
    stats: *const Statistics,
    hour: u32,
    minute: u32,
    backend: i32,
) {
    if pixels.is_null() || settings.is_null() || stats.is_null() || width <= 0 || height <= 0 {
        return;
    }
    // SAFETY: the caller's.
    let pixels = unsafe { core::slice::from_raw_parts_mut(pixels, (width * height) as usize) };
    let mut canvas = Canvas {
        pixels,
        width,
        height,
    };
    draw(
        &mut canvas,
        unsafe { &*settings },
        unsafe { &*stats },
        (hour, minute),
        backend,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_cover_the_alphabet() {
        assert_eq!(glyph(b'0'), [62, 81, 73, 69, 62]);
        assert_eq!(glyph(b'Z'), [97, 81, 73, 69, 67]);
        assert_eq!(glyph(b'z'), [68, 100, 84, 76, 68]);
        assert_eq!(glyph(b'#'), [0; 5]);
    }

    #[test]
    fn the_panel_draws_every_layout() {
        for layout in 0..3 {
            let (w, h) = panel_size(layout);
            let mut pixels = vec![0u32; (w * h) as usize];
            let mut canvas = Canvas {
                pixels: &mut pixels,
                width: w as i32,
                height: h as i32,
            };
            let settings = Settings {
                overlay_layout: layout,
                ..Settings::default()
            };
            let stats = Statistics::default();
            draw(&mut canvas, &settings, &stats, (12, 34), 1);
            // The rounded corner is clear, the border grey, the body the panel's colour, and text was drawn.
            assert_eq!(pixels[0], 0);
            assert_eq!(pixels[(5 * w + 0) as usize], 0);
            assert_eq!(pixels[(100 * w + 1) as usize], 0xFFA4_A4A4);
            assert!(pixels.iter().any(|&p| p == 0xFFF2_F2F2));
        }
    }

    #[test]
    fn unknown_numbers_read_as_dashes() {
        assert_eq!(number(-1.0, true, 1), "--");
        assert_eq!(number(f64::NAN, true, 1), "--");
        assert_eq!(number(2.25, false, 1), "--");
        assert_eq!(number(12.34, true, 1), "12.3");
        assert_eq!(number(12.6, true, 0), "13");
    }
}
