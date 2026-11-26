//! THE FRAME GENERATION'S SETTINGS, STATISTICS AND PACING (formerly src/Framegen/Framegen.cpp with
//! include/Framegen/History.hpp and FramePolicy.hpp's pacing: PresentationPeriod, CadenceLock, PhaseLockedPacing,
//! CycleClock -- main's 0.2.4; Framegen.cpp keeps the exports and the C++ face).
//!
//! The settings are sanitised as they are set; the statistics (frame rates over a two-second window, the CPU and GPU
//! frame times' history with its percentiles and graph, the hardware's) are kept only while the FPS overlay shows
//! them. The pacing holds the game's producer after its Present: while frame generation runs, two display periods a real
//! frame (three under the opt-in cadence lock while the game cannot make two), its release locked to the display cycle
//! when xrWaitFrame keeps its period; the plain refresh cap (the next Present in the middle of a display cycle)
//! otherwise.
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicI64, AtomicPtr, AtomicU32, AtomicU64, Ordering,
};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimerEx, WaitForSingleObject,
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS,
};

use crate::hardware_telemetry::{self, HardwareStatistics};

pub const GRAPH_POINTS: usize = 120;

/// cvr::framegen::Settings.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub overlay: bool,
    pub auto_pace: bool,
    /// Switches in the byte before the backend: FSR3's two (main's fsr3OpticalFlow and fsr3Show, both on by default --
    /// FSR3_NO_OPTICAL_FLOW, FSR3_HIDE: set, the bit turns its switch off) and main's cadenceLock (CADENCE_LOCK, off by
    /// default). A zeroed copy has the defaults.
    pub flags: u8,
    pub backend: i32,
    pub quality: i32,
    pub flow_scale: i32,
    pub overlay_corner: i32,
    pub overlay_layout: i32,
    pub overlay_follow: i32,
    pub history_seconds: i32,
    pub overlay_cone: f32,
    pub overlay_scale: f32,
    pub overlay_distance: f32,
    pub overlay_x: f32,
    pub overlay_y: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: false,
            overlay: false,
            auto_pace: true,
            flags: 0,
            backend: 1,
            quality: 2,
            flow_scale: 50,
            overlay_corner: 1,
            overlay_layout: 0,
            overlay_follow: 1,
            history_seconds: 30,
            overlay_cone: 60.0,
            overlay_scale: 1.0,
            overlay_distance: 1.25,
            overlay_x: 0.0,
            overlay_y: 0.0,
        }
    }
}
const _: () = assert!(core::mem::size_of::<Settings>() == 52 && core::mem::offset_of!(Settings, backend) == 4);
/// Settings::fsr3: FSR3 interpolation without FidelityFX's optical flow (the engine's vectors alone).
pub const FSR3_NO_OPTICAL_FLOW: u8 = 1;
/// Settings::fsr3: FSR3 generates, the headset gets only the real frames (what showing them costs; not saved).
pub const FSR3_HIDE: u8 = 2;
/// Settings::flags: real frames three display periods apart while the game cannot make two (CadenceLock,
/// xr_framegen_cadence_lock). Off: always two, the rate the headset's refresh was chosen for; a slow frame is absorbed
/// by the XR side instead.
pub const CADENCE_LOCK: u8 = 4;
/// The frame generators (cvr::framegen::Backend): 0 FidelityFX, 1 NVIDIA OFA + FidelityFX, 2 DLSS-G, 3 ANR FG, 4 FSR3
/// interpolation, 5 FSR3 extrapolation. ANR FG is not in this build: it runs as DLSS-G.
pub fn clamp_backend(backend: i32) -> i32 {
    // 3 is ANR FG (main's 0.2.4)
    backend.clamp(0, 5)
}
impl Settings {
    /// main's fsr3OpticalFlow.
    pub fn fsr3_optical_flow(&self) -> bool {
        self.flags & FSR3_NO_OPTICAL_FLOW == 0
    }
    /// main's fsr3Show.
    pub fn fsr3_show(&self) -> bool {
        self.flags & FSR3_HIDE == 0
    }
    /// main's cadenceLock.
    pub fn cadence_lock(&self) -> bool {
        self.flags & CADENCE_LOCK != 0
    }
    pub fn set_fsr3_optical_flow(&mut self, on: bool) {
        self.flags = if on { self.flags & !FSR3_NO_OPTICAL_FLOW } else { self.flags | FSR3_NO_OPTICAL_FLOW };
    }
    pub fn set_fsr3_show(&mut self, on: bool) {
        self.flags = if on { self.flags & !FSR3_HIDE } else { self.flags | FSR3_HIDE };
    }
    pub fn set_cadence_lock(&mut self, on: bool) {
        self.flags = if on { self.flags | CADENCE_LOCK } else { self.flags & !CADENCE_LOCK };
    }
}

/// cvr::framegen::Statistics.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Statistics {
    pub real_fps: f64,
    pub generated_fps: f64,
    pub output_fps: f64,
    pub repeat_fps: f64,
    pub cpu_ms: f64,
    pub cpu_peak_ms: f64,
    pub gpu_ms: f64,
    pub gpu_peak_ms: f64,
    pub generation_ms: f64,
    pub generation_span_ms: f64,
    pub average_fps: f64,
    pub minimum_fps: f64,
    pub maximum_fps: f64,
    pub low1_fps: f64,
    pub low01_fps: f64,
    pub cpu_average_ms: f64,
    pub gpu_average_ms: f64,
    pub cpu_p99_ms: f64,
    pub cpu_p999_ms: f64,
    pub gpu_p99_ms: f64,
    pub gpu_p999_ms: f64,
    pub elapsed_seconds: f64,
    pub budget_ms: f64,
    pub repeat_percent: f64,
    pub history_samples: u32,
    pub cpu_graph: [f32; GRAPH_POINTS],
    pub gpu_graph: [f32; GRAPH_POINTS],
    pub hardware: HardwareStatistics,
    pub real_frames: u64,
    pub generated_frames: u64,
    pub repeated_frames: u64,
    pub skipped: u64,
    pub vram_bytes: u64,
    pub real_submissions: u64,
    pub input_frames: [u64; 2],
    pub render_width: [u32; 2],
    pub render_height: [u32; 2],
    pub active: bool,
    pub gpu_valid: bool,
    pub inputs_ready: bool,
    pub status: [u8; 160],
}
const _: () = assert!(core::mem::size_of::<Statistics>() == 2232);
impl Default for Statistics {
    fn default() -> Self {
        let mut status = [0u8; 160];
        status[..8].copy_from_slice(b"Disabled");
        Statistics {
            real_fps: 0.0,
            generated_fps: 0.0,
            output_fps: 0.0,
            repeat_fps: 0.0,
            cpu_ms: 0.0,
            cpu_peak_ms: 0.0,
            gpu_ms: 0.0,
            gpu_peak_ms: 0.0,
            generation_ms: 0.0,
            generation_span_ms: 0.0,
            average_fps: 0.0,
            minimum_fps: 0.0,
            maximum_fps: 0.0,
            low1_fps: 0.0,
            low01_fps: 0.0,
            cpu_average_ms: 0.0,
            gpu_average_ms: 0.0,
            cpu_p99_ms: 0.0,
            cpu_p999_ms: 0.0,
            gpu_p99_ms: 0.0,
            gpu_p999_ms: 0.0,
            elapsed_seconds: 0.0,
            budget_ms: 0.0,
            repeat_percent: 0.0,
            history_samples: 0,
            cpu_graph: [0.0; GRAPH_POINTS],
            gpu_graph: [0.0; GRAPH_POINTS],
            hardware: HardwareStatistics::default(),
            real_frames: 0,
            generated_frames: 0,
            repeated_frames: 0,
            skipped: 0,
            vram_bytes: 0,
            real_submissions: 0,
            input_frames: [0; 2],
            render_width: [0; 2],
            render_height: [0; 2],
            active: false,
            gpu_valid: false,
            inputs_ready: false,
            status,
        }
    }
}

/// The C++ half's exports and the AFW launch flag.
#[repr(C)]
pub struct Runtime {
    pub report: *mut u8,
    pub report_size: usize,
    pub report_seq: *const AtomicU32,
    pub fps_cap_counts: *mut u64,
    pub ring_count: *const [AtomicU64; 2],
    pub ring_cpu: *mut f32,
    pub ring_gpu: *mut f32,
    pub runtime_diagnostics: *const i32,
    /// cvr::afw::Launch: frame generation is off for an AFW launch.
    pub afw_launch: extern "C" fn() -> bool,
}
/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use core::sync::atomic::{AtomicU32, AtomicU64};
    unsafe extern "C" {
        static mut CyberpunkVR_FramegenReport: [u8; 2048];
        static CyberpunkVR_FramegenReportSeq: AtomicU32;
        static mut CyberpunkVR_DebugFpsCap: [u64; 2];
        static CyberpunkVR_FrameRingCount: [AtomicU64; 2];
        static mut CyberpunkVR_FrameRingCpu: [f32; 8192];
        static mut CyberpunkVR_FrameRingGpu: [f32; 8192];
        static CyberpunkVR_RuntimeDiagnostics: i32;
        /// cvr::afw::Launch (cvr-render afw.rs).
        safe fn cvr_afw_launch() -> bool;
    }
    // SAFETY: process-lifetime exports and a plain function.
    unsafe impl Sync for super::Runtime {}
    pub(super) static RT: super::Runtime = super::Runtime {
        report: (&raw mut CyberpunkVR_FramegenReport).cast(),
        report_size: 2048,
        report_seq: &raw const CyberpunkVR_FramegenReportSeq,
        fps_cap_counts: (&raw mut CyberpunkVR_DebugFpsCap).cast(),
        ring_count: &raw const CyberpunkVR_FrameRingCount,
        ring_cpu: (&raw mut CyberpunkVR_FrameRingCpu).cast(),
        ring_gpu: (&raw mut CyberpunkVR_FrameRingGpu).cast(),
        runtime_diagnostics: &raw const CyberpunkVR_RuntimeDiagnostics,
        afw_launch: cvr_afw_launch,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
#[no_mangle]
pub extern "C" fn cvr_framegen_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    runtime().is_some_and(|rt| unsafe { core::ptr::read_volatile(rt.runtime_diagnostics) } != 0)
}
fn afw_launch() -> bool {
    runtime().is_some_and(|rt| (rt.afw_launch)())
}

// ---- the clock

/// NowMs: the performance counter in milliseconds.
#[no_mangle]
pub extern "C" fn cvr_framegen_now_ms() -> f64 {
    static SCALE: AtomicU64 = AtomicU64::new(0);
    let mut scale = f64::from_bits(SCALE.load(Ordering::Relaxed));
    if scale == 0.0 {
        let mut f = 0i64;
        // SAFETY: plain Win32 calls.
        unsafe { QueryPerformanceFrequency(&mut f) };
        scale = 1000.0 / f as f64;
        SCALE.store(scale.to_bits(), Ordering::Relaxed);
    }
    let mut v = 0i64;
    // SAFETY: as above.
    unsafe { QueryPerformanceCounter(&mut v) };
    v as f64 * scale
}
fn now_ms() -> f64 {
    cvr_framegen_now_ms()
}

// ---- History.hpp

#[derive(Clone, Copy, Default)]
struct HistorySummary {
    samples: u32,
    average: f64,
    minimum: f64,
    peak: f64,
    p99: f64,
    p999: f64,
    low1: f64,
    low01: f64,
}
#[derive(Clone, Copy, Default)]
struct Sample {
    stamp: f64,
    ms: f64,
}
const HISTORY: usize = 16384;
struct FrameHistory {
    values: Vec<Sample>,
    count: usize,
    head: usize,
}
impl FrameHistory {
    fn new() -> FrameHistory {
        FrameHistory {
            values: vec![Sample::default(); HISTORY],
            count: 0,
            head: 0,
        }
    }
    fn add(&mut self, now: f64, ms: f64) {
        if !ms.is_finite() || ms <= 0.0 {
            return;
        }
        self.values[self.head] = Sample { stamp: now, ms };
        self.head = (self.head + 1) % HISTORY;
        self.count = (self.count + 1).min(HISTORY);
    }
    fn clear(&mut self) {
        self.count = 0;
        self.head = 0;
    }
    fn summarize(&self, now: f64, seconds: f64) -> HistorySummary {
        let mut result = HistorySummary::default();
        let mut samples: Vec<f64> = Vec::with_capacity(self.count);
        let mut sum = 0.0;
        for v in &self.values[..self.count] {
            if v.stamp >= now - seconds * 1000.0 && v.stamp <= now {
                samples.push(v.ms);
                sum += v.ms;
            }
        }
        if samples.is_empty() {
            return result;
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        result.samples = samples.len() as u32;
        result.average = sum / samples.len() as f64;
        result.minimum = samples[0];
        result.peak = *samples.last().unwrap();
        let n = samples.len();
        let percentile = |p: f64| samples[(n - 1).min(((p * n as f64).ceil() - 1.0) as usize)];
        let slow_mean = |fraction: f64| {
            let k = 1usize.max((fraction * n as f64).ceil() as usize);
            let s: f64 = samples[n - k..].iter().sum();
            1000.0 * k as f64 / s
        };
        if n >= 100 {
            result.p99 = percentile(0.99);
            result.low1 = slow_mean(0.01);
        }
        if n >= 1000 {
            result.p999 = percentile(0.999);
            result.low01 = slow_mean(0.001);
        }
        result
    }
    fn graph(&self, now: f64, seconds: f64) -> [f32; GRAPH_POINTS] {
        let mut graph = [0.0f32; GRAPH_POINTS];
        let start = now - seconds * 1000.0;
        for v in &self.values[..self.count] {
            if v.stamp >= start && v.stamp <= now {
                let index = (GRAPH_POINTS - 1)
                    .min(((v.stamp - start) / (seconds * 1000.0) * GRAPH_POINTS as f64) as usize);
                graph[index] = graph[index].max(v.ms as f32);
            }
        }
        graph
    }
}

// ---- the rate windows

const WINDOW: usize = 512;
struct Window {
    values: [Sample; WINDOW],
    head: usize,
    count: usize,
    started: f64,
}
impl Window {
    const fn new() -> Window {
        Window {
            values: [Sample {
                stamp: 0.0,
                ms: 0.0,
            }; WINDOW],
            head: 0,
            count: 0,
            started: 0.0,
        }
    }
    fn add(&mut self, now: f64, value: f64) {
        if self.count == 0 {
            self.started = now;
        }
        self.values[self.head] = Sample {
            stamp: now,
            ms: value,
        };
        self.head = (self.head + 1) % WINDOW;
        self.count = (self.count + 1).min(WINDOW);
    }
    fn rate(&self, now: f64) -> f64 {
        if self.count < 2 {
            return 0.0;
        }
        let mut used = 0usize;
        let (mut first, mut last) = (now, 0.0f64);
        for v in &self.values[..self.count] {
            first = first.min(v.stamp);
            last = last.max(v.stamp);
            if v.stamp <= now && now - v.stamp <= 2000.0 {
                used += 1;
            }
        }
        let interval = (last - first) / (self.count - 1) as f64;
        let mut window = 2000.0f64.min(now - self.started + interval);
        // Menu rendering can fill the ring in less than two seconds: that shorter measured window.
        if self.count == WINDOW {
            window = window.min(now - first + interval);
        }
        if window > 0.0 {
            used as f64 * 1000.0 / window
        } else {
            0.0
        }
    }
    fn peak(&self, now: f64) -> f64 {
        let mut result = 0.0f64;
        for v in &self.values[..self.count] {
            if now - v.stamp <= 2000.0 {
                result = result.max(v.ms);
            }
        }
        result
    }
}

// ---- the state

struct State {
    settings: Settings,
    stats: Statistics,
    real: Window,
    generated: Window,
    output: Window,
    repeats: Window,
    cpu: Window,
    gpu: Window,
    cpu_history: FrameHistory,
    gpu_history: FrameHistory,
    cached: Statistics,
    next_refresh: f64,
    next_summary: f64,
    started: f64,
    cpu_summary: HistorySummary,
    gpu_summary: HistorySummary,
    last_real: u64,
    last_real_time: f64,
}
impl State {
    fn clear_metrics(&mut self) {
        self.real = Window::new();
        self.generated = Window::new();
        self.output = Window::new();
        self.repeats = Window::new();
        self.cpu = Window::new();
        self.gpu = Window::new();
        self.cpu_history.clear();
        self.gpu_history.clear();
        self.cpu_summary = HistorySummary::default();
        self.gpu_summary = HistorySummary::default();
        self.next_refresh = 0.0;
        self.next_summary = 0.0;
        self.started = 0.0;
        self.last_real = 0;
        self.last_real_time = 0.0;
        let (active, status) = (self.stats.active, self.stats.status);
        self.stats = Statistics::default();
        self.stats.active = active;
        self.stats.status = status;
        self.cached = Statistics::default();
    }
}
static STATE: Mutex<Option<Box<State>>> = Mutex::new(None);
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let s = g.get_or_insert_with(|| {
        Box::new(State {
            settings: Settings::default(),
            stats: Statistics::default(),
            real: Window::new(),
            generated: Window::new(),
            output: Window::new(),
            repeats: Window::new(),
            cpu: Window::new(),
            gpu: Window::new(),
            cpu_history: FrameHistory::new(),
            gpu_history: FrameHistory::new(),
            cached: Statistics::default(),
            next_refresh: 0.0,
            next_summary: 0.0,
            started: 0.0,
            cpu_summary: HistorySummary::default(),
            gpu_summary: HistorySummary::default(),
            last_real: 0,
            last_real_time: 0.0,
        })
    });
    f(s)
}
static SETTINGS_WRITE: Mutex<()> = Mutex::new(());
static ENABLED: AtomicBool = AtomicBool::new(false);
static METRICS: AtomicBool = AtomicBool::new(false);
static DEPTH_CAPTURE: AtomicBool = AtomicBool::new(false);
static AUTO_PACE: AtomicBool = AtomicBool::new(true);
static PACING_READY: AtomicBool = AtomicBool::new(false);
static DISPLAY_PERIOD: AtomicI64 = AtomicI64::new(0);
/// When xrWaitFrame last returned (NowMs, f64 bits): the phase of the display cycle the frame cap aims at.
static DISPLAY_CYCLE_MS: AtomicU64 = AtomicU64::new(0);
static FPS_CAP: AtomicBool = AtomicBool::new(true);
static PACING_INTERVAL_MS: AtomicU64 = AtomicU64::new(0);
static RESET_PACING: AtomicBool = AtomicBool::new(true);
/// Real frames three display periods apart while the game cannot make two (Settings::cadence_lock).
static CADENCE_LOCK_ON: AtomicBool = AtomicBool::new(false);
/// Whether xrWaitFrame's returns keep the predicted period (CycleClock), so framegen's pacing may lock to them.
static CYCLE_STEADY: AtomicBool = AtomicBool::new(false);
/// Presentation cycles whose newest capture skipped a native frame (missed adjacent frames), counted (main's 0.2.4).
#[no_mangle]
pub static CyberpunkVR_FramegenPairGaps: AtomicU64 = AtomicU64::new(0);
/// 1 while CadenceLock holds real frames three display periods apart.
#[no_mangle]
pub static CyberpunkVR_FramegenThreePeriods: AtomicU32 = AtomicU32::new(0);
/// Framegen's pacing per released frame (runtime diagnostics only), a ring of 8192 rows: the end of the Present, the
/// time waiting for the schedule's lock, the game's smoothed work, the previous and the new target, the display cycle
/// (NaN when not steady), the period, the interval, the hold, the time held, the release, 0.
#[no_mangle]
pub static mut CyberpunkVR_PaceTrace: [[f64; 12]; 8192] = [[0.0; 12]; 8192];
#[no_mangle]
pub static CyberpunkVR_PaceTraceCount: AtomicU64 = AtomicU64::new(0);
static ACTIVE_BACKEND: AtomicI32 = AtomicI32::new(-1);

fn f64_load(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::Relaxed))
}
fn f64_store(a: &AtomicU64, v: f64) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

// ---- the report (CyberpunkVR_FramegenReport)

fn write_report(text: &str) {
    let Some(rt) = runtime() else { return };
    // SAFETY: the C++ half's export: a buffer of report_size bytes and its seqlock.
    unsafe {
        let seq = &*rt.report_seq;
        seq.fetch_add(1, Ordering::AcqRel);
        let n = text.len().min(rt.report_size - 1);
        core::ptr::copy_nonoverlapping(text.as_ptr(), rt.report, n);
        *rt.report.add(n) = 0;
        seq.fetch_add(1, Ordering::Release);
    }
}
fn report_empty() -> bool {
    // SAFETY: as above.
    runtime().is_none_or(|rt| unsafe { *rt.report } == 0)
}

// ---- the C ABI (include/Framegen/Framegen.hpp)

#[no_mangle]
pub extern "C" fn cvr_framegen_get_settings(out: *mut Settings) {
    let s = with(|s| s.settings);
    // SAFETY: the caller's out pointer.
    if let Some(out) = unsafe { out.as_mut() } {
        *out = s;
    }
}
#[no_mangle]
pub extern "C" fn cvr_framegen_active_backend() -> i32 {
    let v = ACTIVE_BACKEND.load(Ordering::Relaxed);
    if v < 0 {
        with(|s| s.settings.backend)
    } else {
        v
    }
}
#[no_mangle]
pub extern "C" fn cvr_framegen_publish_active_backend(value: i32) {
    ACTIVE_BACKEND.store(value, Ordering::Relaxed);
}
/// Enabled: off for an AFW launch (its pairs are one drawn eye and one warped).
#[no_mangle]
pub extern "C" fn cvr_framegen_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) && !afw_launch()
}
#[no_mangle]
pub extern "C" fn cvr_framegen_depth_capture_enabled() -> bool {
    DEPTH_CAPTURE.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_framegen_capture_inputs_enabled() -> bool {
    cvr_framegen_enabled() || cvr_framegen_depth_capture_enabled() || afw_launch()
}
#[no_mangle]
pub extern "C" fn cvr_framegen_motion_wanted() -> bool {
    cvr_framegen_enabled() || afw_launch()
}
#[no_mangle]
pub extern "C" fn cvr_framegen_set_depth_capture_enabled(value: bool) {
    DEPTH_CAPTURE.store(value, Ordering::Relaxed);
}
#[no_mangle]
pub extern "C" fn cvr_framegen_metrics_enabled() -> bool {
    METRICS.load(Ordering::Relaxed)
}
unsafe extern "C" fn metrics_enabled() -> bool {
    cvr_framegen_metrics_enabled()
}

fn bounded(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        cvr_body::cmath::clamp(v, lo, hi)
    } else {
        fallback
    }
}
/// SetSettings: sanitised, the metrics cleared when the overlay comes or goes.
///
/// # Safety
/// `value` is readable.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_set_settings(value: *const Settings) {
    // SAFETY: as the caller vouches.
    let Some(&value) = (unsafe { value.as_ref() }) else {
        return;
    };
    let mut value = value;
    let _settings_lock = SETTINGS_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    value.backend = clamp_backend(value.backend);
    value.quality = value.quality.clamp(0, 2);
    // 0 = no optical flow: frame interpolation on the engine's vectors alone.
    value.flow_scale = if matches!(value.flow_scale, 100 | 75 | 0) {
        value.flow_scale
    } else {
        50
    };
    value.overlay_corner = value.overlay_corner.clamp(0, 8);
    value.overlay_layout = value.overlay_layout.clamp(0, 2);
    value.overlay_follow = i32::from(value.overlay_follow != 0);
    value.history_seconds = if matches!(value.history_seconds, 10 | 60 | 120) {
        value.history_seconds
    } else {
        30
    };
    value.overlay_cone = bounded(value.overlay_cone, 5.0, 90.0, 60.0);
    value.overlay_scale = bounded(value.overlay_scale, 0.5, 2.0, 1.0);
    value.overlay_distance = bounded(value.overlay_distance, 0.5, 5.0, 1.25);
    value.overlay_x = bounded(value.overlay_x, -60.0, 60.0, 0.0);
    value.overlay_y = bounded(value.overlay_y, -45.0, 45.0, 0.0);
    let metrics_changed = with(|s| {
        let metrics_changed = value.overlay != s.settings.overlay;
        let disabled_report_changed = metrics_changed
            || value.enabled != s.settings.enabled
            || value.backend != s.settings.backend
            || report_empty();
        if metrics_changed {
            s.clear_metrics();
        }
        if value.history_seconds != s.settings.history_seconds {
            s.next_refresh = 0.0;
            s.next_summary = 0.0;
        }
        s.settings = value;
        ENABLED.store(value.enabled, Ordering::Relaxed);
        METRICS.store(value.overlay, Ordering::Relaxed);
        AUTO_PACE.store(value.auto_pace, Ordering::Relaxed);
        CADENCE_LOCK_ON.store(value.cadence_lock(), Ordering::Relaxed);
        if !value.enabled || !value.auto_pace {
            f64_store(&PACING_INTERVAL_MS, 0.0);
            RESET_PACING.store(true, Ordering::Relaxed);
        }
        if !value.enabled {
            PACING_READY.store(false, Ordering::Relaxed);
            s.stats.generation_ms = 0.0;
        }
        if diagnostics() && !value.overlay && disabled_report_changed {
            write_report(&format!(
                "{{\"enabled\":{},\"backend\":{},\"metrics\":0,\"status\":\"FPS telemetry disabled\"}}",
                i32::from(value.enabled),
                value.backend
            ));
        }
        metrics_changed
    });
    if value.overlay {
        // SAFETY: the telemetry's own entry, with this module's metrics query.
        unsafe { hardware_telemetry::cvr_gpu_hw_telemetry_wake(metrics_enabled) };
    } else if metrics_changed {
        hardware_telemetry::cvr_gpu_hw_telemetry_stop();
    }
}

fn status_text(status: &[u8; 160]) -> String {
    let end = status.iter().position(|&b| b == 0).unwrap_or(status.len());
    String::from_utf8_lossy(&status[..end]).into_owned()
}
/// GetRuntimeStatus: whether generation runs, and its status line (160 bytes).
#[no_mangle]
pub extern "C" fn cvr_framegen_runtime_status(text: *mut u8) -> bool {
    with(|s| {
        if !text.is_null() {
            // SAFETY: the caller's 160 bytes.
            unsafe { core::ptr::copy_nonoverlapping(s.stats.status.as_ptr(), text, 160) };
        }
        s.stats.active
    })
}
/// GetStatistics: refreshed at most every 250 ms while the overlay shows them (the history's summary every second).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_statistics(out: *mut Statistics) {
    let result = with(|s| {
        if !s.settings.overlay {
            return s.stats;
        }
        let now = now_ms();
        if now < s.next_refresh {
            return s.cached;
        }
        s.next_refresh = now + 250.0;
        let mut result = s.stats;
        result.real_fps = s.real.rate(now);
        result.generated_fps = s.generated.rate(now);
        result.output_fps = s.output.rate(now);
        result.repeat_fps = s.repeats.rate(now);
        result.cpu_peak_ms = s.cpu.peak(now);
        result.gpu_peak_ms = s.gpu.peak(now);
        let seconds = s.settings.history_seconds as f64;
        if now >= s.next_summary {
            s.cpu_summary = s.cpu_history.summarize(now, seconds);
            s.gpu_summary = s.gpu_history.summarize(now, seconds);
            s.next_summary = now + 1000.0;
        }
        let (c, g) = (s.cpu_summary, s.gpu_summary);
        result.history_samples = c.samples;
        result.cpu_average_ms = c.average;
        result.gpu_average_ms = g.average;
        result.average_fps = if c.average > 0.0 {
            1000.0 / c.average
        } else {
            0.0
        };
        result.low1_fps = c.low1;
        result.low01_fps = c.low01;
        result.minimum_fps = if c.peak > 0.0 { 1000.0 / c.peak } else { 0.0 };
        result.maximum_fps = if c.minimum > 0.0 {
            1000.0 / c.minimum
        } else {
            0.0
        };
        result.cpu_p99_ms = c.p99;
        result.cpu_p999_ms = c.p999;
        result.gpu_p99_ms = g.p99;
        result.gpu_p999_ms = g.p999;
        result.cpu_graph = s.cpu_history.graph(now, seconds);
        result.gpu_graph = s.gpu_history.graph(now, seconds);
        // SAFETY: the telemetry's own read.
        unsafe { hardware_telemetry::cvr_gpu_hw_telemetry_read(&mut result.hardware) };
        result.elapsed_seconds = if s.started != 0.0 {
            (now - s.started) / 1000.0
        } else {
            0.0
        };
        result.budget_ms = DISPLAY_PERIOD.load(Ordering::Relaxed) as f64
            * 1e-6
            * if s.settings.enabled { 2.0 } else { 1.0 };
        if s.settings.enabled && s.settings.auto_pace {
            result.budget_ms = result.budget_ms.max(f64_load(&PACING_INTERVAL_MS));
        }
        if result.budget_ms <= 0.0 {
            result.budget_ms = 1000.0 / 90.0;
        }
        let submissions =
            result.real_submissions + result.generated_frames + result.repeated_frames;
        result.repeat_percent = if submissions != 0 {
            100.0 * result.repeated_frames as f64 / submissions as f64
        } else {
            0.0
        };
        if diagnostics() {
            let h = &result.hardware;
            write_report(&format!(
                "{{\"enabled\":{},\"active\":{},\"backend\":{},\"metrics\":1,\"real_fps\":{:.3},\"fg_fps\":{:.3},\"output_fps\":{:.3},\"repeats_fps\":{:.3},\
                 \"cpu_ms\":{:.3},\"gpu_ms\":{:.3},\"generation_ms\":{:.3},\"generation_span_ms\":{:.3},\"pace_ms\":{:.3},\"real\":{},\"generated\":{},\"real_submitted\":{},\"repeated\":{},\"skipped\":{},\"vram_bytes\":{},\
                 \"inputs\":[{},{}],\"average_fps\":{:.3},\"minimum_fps\":{:.3},\"maximum_fps\":{:.3},\"low1\":{:.3},\"low01\":{:.3},\"cpu_usage\":{:.2},\"gpu_usage\":{:.2},\"gpu_temp\":{:.1},\
                 \"gpu_memory_used\":{},\"gpu_memory_total\":{},\"ram_used\":{},\"ram_total\":{},\"status\":\"{}\"}}",
                i32::from(s.settings.enabled),
                i32::from(result.active),
                s.settings.backend,
                result.real_fps,
                result.generated_fps,
                result.output_fps,
                result.repeat_fps,
                result.cpu_ms,
                result.gpu_ms,
                result.generation_ms,
                result.generation_span_ms,
                result.budget_ms,
                result.real_frames,
                result.generated_frames,
                result.real_submissions,
                result.repeated_frames,
                result.skipped,
                result.vram_bytes,
                result.input_frames[0],
                result.input_frames[1],
                result.average_fps,
                result.minimum_fps,
                result.maximum_fps,
                result.low1_fps,
                result.low01_fps,
                h.cpu_usage,
                h.gpu_usage,
                h.gpu_temperature,
                h.gpu_memory_used,
                h.gpu_memory_total,
                h.ram_used,
                h.ram_total,
                status_text(&result.status)
            ));
        }
        s.cached = result;
        result
    });
    // SAFETY: as the caller vouches.
    if let Some(out) = unsafe { out.as_mut() } {
        *out = result;
    }
}
#[no_mangle]
pub extern "C" fn cvr_framegen_reset_statistics() {
    with(State::clear_metrics);
}
/// Status: the status line (snprintf "%s" into 160 bytes) and whether generation runs.
///
/// # Safety
/// `text` is NUL-terminated or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_status(text: *const core::ffi::c_char, active: bool) {
    // SAFETY: as the caller vouches.
    let bytes = if text.is_null() {
        &b""[..]
    } else {
        unsafe { core::ffi::CStr::from_ptr(text) }.to_bytes()
    };
    with(|s| {
        s.stats.active = active;
        let n = bytes.len().min(159);
        s.stats.status = [0; 160];
        s.stats.status[..n].copy_from_slice(&bytes[..n]);
    });
}
fn ring(index: usize, value: f32) {
    let Some(rt) = runtime() else { return };
    // SAFETY: the C++ half's ring exports (8192 entries each).
    unsafe {
        let count = &(*rt.ring_count)[index];
        let n = count.load(Ordering::Relaxed);
        let ring = if index == 0 { rt.ring_cpu } else { rt.ring_gpu };
        *ring.add((n % 8192) as usize) = value;
        count.store(n + 1, Ordering::Release);
    }
}
#[no_mangle]
pub extern "C" fn cvr_framegen_on_real_frame(serial: u64, now: f64) {
    if !cvr_framegen_metrics_enabled() {
        return;
    }
    with(|s| {
        if !s.settings.overlay || serial == 0 || serial == s.last_real {
            return;
        }
        if s.started == 0.0 {
            s.started = now;
        }
        if s.last_real_time > 0.0 && now > s.last_real_time {
            s.stats.cpu_ms = now - s.last_real_time;
            s.cpu.add(now, s.stats.cpu_ms);
            s.cpu_history.add(now, s.stats.cpu_ms);
            ring(0, s.stats.cpu_ms as f32);
        }
        s.last_real = serial;
        s.last_real_time = now;
        s.real.add(now, 0.0);
        s.stats.real_frames += 1;
    });
}
#[no_mangle]
pub extern "C" fn cvr_framegen_on_submitted(_serial: u64, synth: bool, repeat: bool, now: f64) {
    if !cvr_framegen_metrics_enabled() {
        return;
    }
    with(|s| {
        if !s.settings.overlay {
            return;
        }
        if repeat {
            s.repeats.add(now, 0.0);
            s.stats.repeated_frames += 1;
            return;
        }
        s.output.add(now, 0.0);
        if synth {
            s.generated.add(now, 0.0);
            s.stats.generated_frames += 1;
        } else {
            s.stats.real_submissions += 1;
        }
    });
}
#[no_mangle]
pub extern "C" fn cvr_framegen_on_gpu_frame(ms: f64) {
    if !cvr_framegen_metrics_enabled() || !ms.is_finite() || ms < 0.0 || ms > 10000.0 {
        return;
    }
    with(|s| {
        if !s.settings.overlay {
            return;
        }
        s.stats.gpu_valid = true;
        s.stats.gpu_ms = ms;
        let now = now_ms();
        s.gpu.add(now, ms);
        s.gpu_history.add(now, ms);
        ring(1, ms as f32);
    });
}
#[no_mangle]
pub extern "C" fn cvr_framegen_on_generation(ms: f64, span_ms: f64) {
    if !cvr_framegen_metrics_enabled() {
        return;
    }
    with(|s| {
        if s.settings.overlay {
            s.stats.generation_ms = ms;
            s.stats.generation_span_ms = span_ms;
        }
    });
}
#[no_mangle]
pub extern "C" fn cvr_framegen_on_input(eye: u32, w: u32, h: u32) {
    if eye > 1 || !cvr_framegen_metrics_enabled() {
        return;
    }
    with(|s| {
        if !s.settings.overlay {
            return;
        }
        s.stats.input_frames[eye as usize] += 1;
        s.stats.render_width[eye as usize] = w;
        s.stats.render_height[eye as usize] = h;
        s.stats.inputs_ready = s.stats.input_frames[0] != 0 && s.stats.input_frames[1] != 0;
    });
}
#[no_mangle]
pub extern "C" fn cvr_framegen_on_skip() {
    if !cvr_framegen_metrics_enabled() {
        return;
    }
    with(|s| {
        if s.settings.overlay {
            s.stats.skipped += 1;
        }
    });
}
#[no_mangle]
pub extern "C" fn cvr_framegen_set_vram(bytes: u64) {
    if !cvr_framegen_metrics_enabled() {
        return;
    }
    with(|s| {
        if s.settings.overlay {
            s.stats.vram_bytes = bytes;
        }
    });
}
/// DisplayPeriod: the predicted display period at xrWaitFrame's return -- under SteamVR filtered (PresentationPeriod),
/// the return timed into the cycle clock (CycleClock), its steadiness published for the pacing. One XR-loop owner
/// writes it.
#[no_mangle]
pub extern "C" fn cvr_framegen_display_period(ns: i64, steam_vr: bool) {
    if !(4_000_000..=40_000_000).contains(&ns) {
        return;
    }
    static CLOCK: Mutex<(CycleClock, PresentationPeriod)> = Mutex::new((CycleClock::new(), PresentationPeriod::new()));
    let mut g = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (clock, period) = &mut *g;
    let now = now_ms();
    let ns = if steam_vr {
        period.observe(now, ns)
    } else {
        period.reset();
        ns
    };
    clock.tick(now, ns as f64 * 1e-6);
    DISPLAY_PERIOD.store(ns, Ordering::Relaxed);
    f64_store(&DISPLAY_CYCLE_MS, now);
    CYCLE_STEADY.store(clock.steady(), Ordering::Relaxed);
}
/// The headset's refresh rate from the last predicted display period (0 before one).
pub fn display_hz() -> f64 {
    let ns = DISPLAY_PERIOD.load(Ordering::Relaxed);
    if ns > 0 {
        1.0e9 / ns as f64
    } else {
        0.0
    }
}
/// The rate the game should render at: the headset's, half of it while frame generation makes every other frame.
pub fn target_game_hz() -> f64 {
    let hz = display_hz();
    if cvr_framegen_enabled() {
        hz * 0.5
    } else {
        hz
    }
}
#[no_mangle]
pub extern "C" fn cvr_framegen_fps_cap() -> bool {
    FPS_CAP.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_framegen_set_fps_cap(on: bool) {
    FPS_CAP.store(on, Ordering::Relaxed);
}

// ---- the pacing

fn wait(timer: &mut HANDLE, ms: f64) {
    if timer.is_null() {
        // SAFETY: a plain Win32 call.
        *timer = unsafe {
            CreateWaitableTimerExW(
                core::ptr::null(),
                core::ptr::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
    }
    if !timer.is_null() {
        let due = -((ms * 10000.0) as i64);
        // SAFETY: our timer.
        unsafe {
            if SetWaitableTimerEx(
                *timer,
                &due,
                0,
                None,
                core::ptr::null(),
                core::ptr::null(),
                0,
            ) != 0
            {
                WaitForSingleObject(*timer, 100);
            }
        }
    }
}
struct Cap {
    timer: HANDLE,
    released: f64,
    work: f64,
}
// SAFETY: a waitable timer handle, used under its mutex only.
unsafe impl Send for Cap {}
static CAP: Mutex<Cap> = Mutex::new(Cap {
    timer: core::ptr::null_mut(),
    released: 0.0,
    work: 0.0,
});

/// THE GAME RUNS AT THE HEADSET'S RATE AT MOST (xr_fps_cap): the producer held after its Present until its NEXT Present
/// falls in the middle of a display cycle. A game that cannot reach the refresh is never held. Caller holds the pacing
/// lock.
fn cap_to_display(period: i64) {
    let mut s = CAP.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_ms();
    let cycle = f64_load(&DISPLAY_CYCLE_MS);
    let per = period as f64 * 1e-6;
    // The XR phase gives the best placement when it is live; with the runtime idle or its cycle callback absent for a
    // while the producer is kept one headset period from its previous release, so framegen-off still follows the
    // headset's rate (main's 87693549b).
    if !FPS_CAP.load(Ordering::Relaxed) || period == 0 {
        s.released = 0.0;
        s.work = 0.0;
        return;
    }
    let have_cycle = cycle > 0.0 && now - cycle <= 100.0;
    // The game's own time from release to the end of its Present, smoothed over about eight frames.
    if s.released > 0.0 {
        let w = now - s.released;
        if w > 0.0 && w < 100.0 {
            s.work = if s.work > 0.0 {
                s.work + (w - s.work) * 0.125
            } else {
                w
            };
        }
    }
    let mut wait_ms = 0.0;
    if s.work > 0.0 && s.work < per * 0.9 {
        // The first mid-cycle instant at least half a period away: one period on from a Present already there.
        if have_cycle {
            let next = cycle + per * (0.5 + ((now - cycle) / per).ceil());
            wait_ms = (next - s.work - now).min(per * 2.0);
        } else {
            wait_ms = (per - (now - s.released)).min(per * 2.0);
        }
    }
    let rt = runtime();
    if wait_ms > 0.2 {
        wait(&mut s.timer, wait_ms);
        // SAFETY: the C++ half's export.
        if let Some(rt) = rt {
            unsafe { *rt.fps_cap_counts += 1 };
        }
    } else if let Some(rt) = rt {
        // SAFETY: as above.
        unsafe { *rt.fps_cap_counts.add(1) += 1 };
    }
    s.released = now_ms();
}
#[no_mangle]
pub extern "C" fn cvr_framegen_pacing_ready(ready: bool) {
    PACING_READY.store(ready, Ordering::Relaxed);
    if !ready {
        f64_store(&PACING_INTERVAL_MS, 0.0);
        RESET_PACING.store(true, Ordering::Relaxed);
    }
}

/// FramePolicy.hpp's PresentationPeriod (main's 0.2.4): SteamVR's predicted period wanders a little above the real one
/// on some frames; the period kept is the stable one -- a shorter one at once, a longer one only once it has held for a
/// second.
#[derive(Clone, Copy, Default)]
struct PresentationPeriod {
    stable: i64,
    candidate: i64,
    candidate_since: f64,
    last: f64,
}
impl PresentationPeriod {
    const fn new() -> Self {
        PresentationPeriod { stable: 0, candidate: 0, candidate_since: 0.0, last: 0.0 }
    }
    fn reset(&mut self) {
        *self = Self::new();
    }
    fn observe(&mut self, now: f64, reported: i64) -> i64 {
        if !now.is_finite() || !(4_000_000..=40_000_000).contains(&reported) {
            return self.stable;
        }
        if self.stable == 0 || now < self.last || now - self.last > 1000.0 {
            self.stable = reported;
            self.candidate = 0;
            self.last = now;
            return self.stable;
        }
        self.last = now;
        if (reported - self.stable).abs() <= self.stable / 1000 {
            self.candidate = 0;
            return self.stable;
        }
        if reported < self.stable {
            self.stable = reported;
            self.candidate = 0;
            return self.stable;
        }
        if self.candidate == 0 || (reported - self.candidate).abs() > self.candidate / 1000 {
            self.candidate = reported;
            self.candidate_since = now;
        } else if now - self.candidate_since >= 1000.0 {
            self.stable = self.candidate;
            self.candidate = 0;
        }
        self.stable
    }
}

/// FramePolicy.hpp's CadenceLock (main's 0.2.4): THE REAL-FRAME CADENCE OF FRAME GENERATION -- two display periods a
/// real frame (45 of 90 Hz), or three (30) while the game cannot make two: a whole-period lock, as SteamVR motion
/// smoothing and Meta's Application SpaceWarp hold an application at an exact fraction of the refresh (an interval in
/// between mixes two- and three-period frames on the display grid). The decision is the game's own frame time against
/// the two-period budget, per one-second window; a teleport, a load or a skipped capture misses neighbours for reasons a
/// slower cadence does not cure.
#[derive(Clone, Copy, Default)]
struct CadenceLock {
    window_start: f64,
    period: f64,
    frames: u32,
    over: u32,
    tight: u32,
    three: bool,
}
impl CadenceLock {
    fn reset(&mut self) {
        *self = CadenceLock::default();
    }
    fn three(&self) -> bool {
        self.three
    }
    /// `now` and `work` in ms; `work` is the game's own time for this frame, from its release to the end of its Present.
    fn observe(&mut self, now: f64, display_period: f64, work: f64) -> f64 {
        if !now.is_finite() || !display_period.is_finite() || display_period <= 0.0 {
            self.reset();
            return 0.0;
        }
        if display_period != self.period || now < self.window_start || now - self.window_start > 2000.0 {
            *self = CadenceLock { period: display_period, window_start: now, ..CadenceLock::default() };
        }
        // a load or a hitch (100 ms and more) is not the game's steady cost
        if work.is_finite() && work > 0.0 && work < 100.0 {
            let budget = self.period * 2.0;
            self.frames += 1;
            // this frame could not have made a two-period slot
            if work > budget - 1.0 {
                self.over += 1;
            }
            // too close to it to return safely
            if work > budget - 3.0 {
                self.tight += 1;
            }
        }
        if now - self.window_start >= 1000.0 {
            if self.frames >= 10 {
                if !self.three && self.over * 4 >= self.frames {
                    self.three = true;
                } else if self.three && self.tight * 20 <= self.frames {
                    self.three = false;
                }
            }
            self.window_start = now;
            self.frames = 0;
            self.over = 0;
            self.tight = 0;
        }
        self.period * if self.three { 3.0 } else { 2.0 }
    }
}

/// FramePolicy.hpp's PhaseLockedPacing (main's 0.2.4): THE REAL FRAMES OF FRAME GENERATION, LOCKED TO THE DISPLAY CYCLE
/// (xr_framegen_pace). The XR thread takes the newest real frame at each xrWaitFrame return; with a midpoint between two
/// real frames a real frame is due every other cycle. A free-running release kept whatever phase it started with and
/// drifted: a Present landing near the instant the XR thread takes its frame missed it on a fixed rhythm. Like the plain
/// refresh cap, the producer is held so its NEXT Present lands mid-cycle, the farthest point from that instant, and real
/// frames stay whole display periods apart. A game slower than that is never held.
#[derive(Clone, Copy, Default)]
struct PhaseLockedPacing {
    target: f64,
    released: f64,
    work: f64,
}
impl PhaseLockedPacing {
    fn reset(&mut self) {
        *self = PhaseLockedPacing::default();
    }
    fn target(&self) -> f64 {
        self.target
    }
    fn work(&self) -> f64 {
        self.work
    }
    /// The game's own time for the frame ending now, unsmoothed; NaN before the first release.
    fn elapsed(&self, now: f64) -> f64 {
        if self.released > 0.0 && now >= self.released {
            now - self.released
        } else {
            f64::NAN
        }
    }
    /// `now`: the end of this Present; `cycle`: the latest xrWaitFrame return; all in ms. How long to hold the producer
    /// before releasing it (released() with the actual release time afterwards).
    fn hold(&mut self, now: f64, cycle: f64, period: f64, interval: f64) -> f64 {
        if !now.is_finite() || !period.is_finite() || period <= 0.0 || !interval.is_finite() {
            self.reset();
            return 0.0;
        }
        let interval = interval.max(period * 2.0);
        // the game's own time from its release to the end of its Present, smoothed over about eight frames
        if self.released > 0.0 {
            let w = now - self.released;
            if w > 0.0 && w < 100.0 {
                self.work = if self.work > 0.0 { self.work + (w - self.work) * 0.125 } else { w };
            }
        }
        let earliest = now + self.work;
        // a schedule that fell more than two real frames behind (a load, a menu) starts again from now
        let desired = if self.target > 0.0 && earliest - self.target < interval * 2.0 {
            (self.target + interval).max(earliest)
        } else {
            earliest
        };
        let mut next = desired;
        if cycle.is_finite() && now >= cycle && now - cycle <= 500.0 {
            // the first mid-cycle instant at or after the wanted one, a quarter period of slack for a cycle measured
            // again since the previous frame
            let start = cycle + period * 0.5;
            next = start + period * ((desired - start) / period - 0.25).ceil();
        }
        self.target = next;
        let hold = next - self.work - now;
        if hold > 0.2 {
            hold.min((interval + period).min(80.0))
        } else {
            0.0
        }
    }
    fn released(&mut self, now: f64) {
        self.released = now;
    }
}

/// FramePolicy.hpp's CycleClock (main's 0.2.4): WHETHER xrWaitFrame's RETURNS ARE A DISPLAY CLOCK. The phase lock needs
/// them a whole number of predicted periods apart; the SteamVR null driver returns 136 times a second against a 90 Hz
/// period, and a grid rebuilt from each return moved by a fraction of a period every cycle. A runtime whose loop does not
/// keep its period falls back to the plain interval.
#[derive(Clone, Copy, Default)]
struct CycleClock {
    last: f64,
    error: f64,
    count: u32,
}
impl CycleClock {
    const fn new() -> Self {
        CycleClock { last: 0.0, error: 0.0, count: 0 }
    }
    fn tick(&mut self, now: f64, period: f64) {
        if !now.is_finite() || !period.is_finite() || period <= 0.0 {
            *self = Self::new();
            return;
        }
        if self.last > 0.0 && now > self.last && now - self.last < 500.0 {
            let dt = now - self.last;
            // a cycle the loop skipped still lands on the grid: measured against the nearest whole number of periods
            let cycles = (dt / period).round().max(1.0);
            let e = ((dt - cycles * period).abs() / period).min(1.0);
            self.error = if self.count != 0 { self.error + (e - self.error) * 0.05 } else { e };
            self.count += 1;
        } else if self.last > 0.0 {
            self.error = 0.0;
            self.count = 0;
        }
        self.last = now;
    }
    fn steady(&self) -> bool {
        self.count >= 30 && self.error < 0.12
    }
}

/// OnPresentationCycle: a cycle whose newest capture skipped native frames, counted (the cadence lock decides on the
/// game's own frame time instead: main's 0.2.4).
#[no_mangle]
pub extern "C" fn cvr_framegen_on_presentation_cycle(missed_adjacent_frame: bool) {
    if missed_adjacent_frame {
        CyberpunkVR_FramegenPairGaps.fetch_add(1, Ordering::Relaxed);
    }
}
struct Pace {
    handle: HANDLE,
    phase: PhaseLockedPacing,
    cadence: CadenceLock,
}
// SAFETY: a waitable timer handle, used under its mutex only.
unsafe impl Send for Pace {}
static PACE: Mutex<Pace> = Mutex::new(Pace {
    handle: core::ptr::null_mut(),
    phase: PhaseLockedPacing { target: 0.0, released: 0.0, work: 0.0 },
    cadence: CadenceLock { window_start: 0.0, period: 0.0, frames: 0, over: 0, tight: 0, three: false },
});
/// PaceRenderedFrame: framegen's pacing when it runs -- two display periods a real frame (three under the cadence
/// lock), the release locked to the display cycle when xrWaitFrame keeps its period -- the plain refresh cap
/// otherwise; one schedule for every worker the game's Present jobs run on.
#[no_mangle]
pub extern "C" fn cvr_framegen_pace_rendered_frame() {
    let entered = if diagnostics() { now_ms() } else { 0.0 };
    let mut guard = PACE.lock().unwrap_or_else(|e| e.into_inner());
    let timer = &mut *guard;
    let period = DISPLAY_PERIOD.load(Ordering::Relaxed);
    if !cvr_framegen_enabled() || !AUTO_PACE.load(Ordering::Relaxed) || !PACING_READY.load(Ordering::Relaxed) || period == 0 {
        timer.phase.reset();
        timer.cadence.reset();
        f64_store(&PACING_INTERVAL_MS, 0.0);
        CyberpunkVR_FramegenThreePeriods.store(0, Ordering::Relaxed);
        cap_to_display(period);
        return;
    }
    if RESET_PACING.swap(false, Ordering::Relaxed) {
        timer.cadence.reset();
    }
    let now = now_ms();
    let per = period as f64 * 1e-6;
    // ALWAYS TWO DISPLAY PERIODS unless the cadence lock is asked for (xr_framegen_cadence_lock): the lock moved to three
    // on one heavy second and stayed there seven seconds while the game made 45 -- a frame over the budget now costs that
    // frame, and the XR side keeps one finished frame in hand.
    let lock_cadence = CADENCE_LOCK_ON.load(Ordering::Relaxed);
    if !lock_cadence {
        timer.cadence.reset();
    }
    let interval = if lock_cadence { timer.cadence.observe(now, per, timer.phase.elapsed(now)) } else { per * 2.0 };
    f64_store(&PACING_INTERVAL_MS, interval);
    CyberpunkVR_FramegenThreePeriods.store(u32::from(timer.cadence.three()), Ordering::Relaxed);
    // no grid from a loop that does not keep its period (CycleClock): the plain interval then
    let cycle = if CYCLE_STEADY.load(Ordering::Relaxed) { f64_load(&DISPLAY_CYCLE_MS) } else { f64::NAN };
    let previous_target = timer.phase.target();
    let hold = timer.phase.hold(now, cycle, per, interval);
    let wait_start = if entered != 0.0 { now_ms() } else { 0.0 };
    if hold > 0.0 {
        wait(&mut timer.handle, hold);
    }
    let released = now_ms();
    timer.phase.released(released);
    if entered != 0.0 {
        let n = CyberpunkVR_PaceTraceCount.load(Ordering::Relaxed);
        let row = [now, now - entered, timer.phase.work(), previous_target, timer.phase.target(), cycle, per, interval, hold,
            released - wait_start, released, 0.0];
        // SAFETY: the export's own ring, written under the pacing lock; readers follow the count.
        unsafe { (*(&raw mut CyberpunkVR_PaceTrace))[(n % 8192) as usize] = row };
        CyberpunkVR_PaceTraceCount.store(n + 1, Ordering::Release);
    }
}

#[cfg(test)]
mod pacing_tests {
    use super::*;

    #[test]
    fn the_cadence_moves_to_three_on_a_slow_second_and_back() {
        let mut c = CadenceLock::default();
        let p = 11.1;
        // a second of frames over the two-period budget (22.2 ms): three periods after the window
        let mut t = 0.0;
        // a second and a bit: the window closes after 1000 ms from its first frame
        for _ in 0..45 {
            t += 25.0;
            c.observe(t, p, 23.0);
        }
        assert!(c.three());
        assert!((c.observe(t, p, 23.0) - p * 3.0).abs() < 1e-9);
        // two seconds of easy frames: back to two
        for _ in 0..90 {
            t += 22.2;
            c.observe(t, p, 10.0);
        }
        assert!(!c.three());
    }

    #[test]
    fn the_release_lands_mid_cycle() {
        let mut ph = PhaseLockedPacing::default();
        let p = 10.0;
        let cycle = 1000.0;
        // the first frame: no work known, released at once (a hold from the grid only)
        let h = ph.hold(1003.0, cycle, p, 2.0 * p);
        ph.released(1003.0 + h);
        // the next Present ends 6 ms after its release: the target is a mid-cycle instant
        let now = 1003.0 + h + 6.0;
        let h2 = ph.hold(now, cycle, p, 2.0 * p);
        let target = ph.target();
        assert!(((target - (cycle + p * 0.5)) / p).fract().abs() < 1e-9, "{target}");
        assert!(h2 >= 0.0 && h2 <= 3.0 * p);
    }

    #[test]
    fn the_cycle_clock_and_the_presentation_period() {
        let mut c = CycleClock::new();
        for i in 0..40 {
            c.tick(1000.0 + f64::from(i) * 11.111, 11.111);
        }
        assert!(c.steady());
        // the SteamVR null driver's 136 returns a second against a 90 Hz period
        let mut d = CycleClock::new();
        for i in 0..60 {
            d.tick(1000.0 + f64::from(i) * 7.35, 11.111);
        }
        assert!(!d.steady());
        let mut pp = PresentationPeriod::new();
        assert_eq!(pp.observe(0.0, 11_111_111), 11_111_111);
        // a longer period is taken only once it has held a second
        assert_eq!(pp.observe(10.0, 11_500_000), 11_111_111);
        assert_eq!(pp.observe(1020.0, 11_500_000), 11_500_000);
        // a shorter one at once
        assert_eq!(pp.observe(1030.0, 11_111_111), 11_111_111);
    }
}

// ---- the live controls' keys

/// sscanf's "key = %d" / "key = %f": the key at the line's start, blanks, '=', blanks, the number.
fn value_after<'a>(line: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let rest = line.strip_prefix(key)?;
    let rest = &rest[rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(rest.len())..];
    let rest = rest.strip_prefix(b"=")?;
    Some(
        &rest[rest
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(rest.len())..],
    )
}
fn scan_int(v: &[u8]) -> Option<i32> {
    let mut end = usize::from(v.first().is_some_and(|&b| b == b'+' || b == b'-'));
    let digits = v[end..].iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    end += digits;
    let n: i64 = core::str::from_utf8(&v[..end]).ok()?.parse().ok()?;
    Some(n as i32)
}
fn scan_float(v: &[u8]) -> Option<f32> {
    let mut i = usize::from(v.first().is_some_and(|&b| b == b'+' || b == b'-'));
    let lower: Vec<u8> = v[i..]
        .iter()
        .take(8)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if lower.starts_with(b"inf") || lower.starts_with(b"nan") {
        let negative = v[0] == b'-';
        let x = if lower.starts_with(b"inf") {
            f32::INFINITY
        } else {
            f32::NAN
        };
        return Some(if negative { -x } else { x });
    }
    let whole = v[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    i += whole;
    let mut frac = 0;
    if v.get(i) == Some(&b'.') {
        frac = v[i + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        i += 1 + frac;
    }
    if whole + frac == 0 {
        return None;
    }
    if matches!(v.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(v.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp = v[j..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exp > 0 {
            i = j + exp;
        }
    }
    core::str::from_utf8(&v[..i]).ok()?.parse::<f32>().ok()
}
/// ParseSetting: one xr_framegen* line into `value`; false for any other line.
pub fn parse_setting(line: &[u8], value: &mut Settings) -> bool {
    let int = |key: &[u8]| value_after(line, key).and_then(scan_int);
    let float = |key: &[u8]| value_after(line, key).and_then(scan_float);
    if let Some(n) = int(b"xr_framegen") {
        value.enabled = n != 0;
    } else if let Some(n) = int(b"xr_framegen_overlay") {
        value.overlay = n != 0;
    } else if let Some(n) = int(b"xr_framegen_pace") {
        value.auto_pace = n != 0;
    } else if let Some(n) = int(b"xr_framegen_backend") {
        value.backend = clamp_backend(n);
    } else if let Some(n) = int(b"xr_fsr3_optical_flow") {
        value.set_fsr3_optical_flow(n != 0);
    } else if let Some(n) = int(b"xr_framegen_cadence_lock") {
        value.set_cadence_lock(n != 0);
    } else if let Some(n) = int(b"xr_framegen_quality") {
        value.quality = n;
    } else if let Some(n) = int(b"xr_framegen_flow_scale") {
        value.flow_scale = n;
    } else if let Some(n) = int(b"xr_framegen_overlay_corner") {
        value.overlay_corner = n;
    } else if let Some(n) = int(b"xr_framegen_overlay_layout") {
        value.overlay_layout = n;
    } else if let Some(n) = int(b"xr_framegen_overlay_follow") {
        value.overlay_follow = n;
    } else if let Some(n) = int(b"xr_framegen_history") {
        value.history_seconds = n;
    } else if let Some(f) = float(b"xr_framegen_overlay_cone") {
        value.overlay_cone = f;
    } else if let Some(f) = float(b"xr_framegen_overlay_scale") {
        value.overlay_scale = f;
    } else if let Some(f) = float(b"xr_framegen_overlay_distance") {
        value.overlay_distance = f;
    } else if let Some(f) = float(b"xr_framegen_overlay_x") {
        value.overlay_x = f;
    } else if let Some(f) = float(b"xr_framegen_overlay_y") {
        value.overlay_y = f;
    } else {
        return false;
    }
    true
}
/// # Safety
/// `line` is NUL-terminated; `value` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_parse_setting(
    line: *const core::ffi::c_char,
    value: *mut Settings,
) -> bool {
    // SAFETY: as the caller vouches.
    let (Some(value), false) = (unsafe { value.as_mut() }, line.is_null()) else {
        return false;
    };
    parse_setting(unsafe { core::ffi::CStr::from_ptr(line) }.to_bytes(), value)
}
/// WriteSettings' text.
pub fn settings_text(s: &Settings) -> String {
    format!(
        "xr_fsr3_optical_flow={}\nxr_framegen={}\nxr_framegen_overlay={}\nxr_framegen_pace={}\nxr_framegen_backend={}\nxr_framegen_quality={}\nxr_framegen_flow_scale={}\nxr_framegen_overlay_corner={}\n\
         xr_framegen_cadence_lock={}\nxr_framegen_overlay_layout={}\nxr_framegen_overlay_follow={}\nxr_framegen_history={}\nxr_framegen_overlay_cone={:.2}\nxr_framegen_overlay_scale={:.3}\nxr_framegen_overlay_distance={:.3}\nxr_framegen_overlay_x={:.2}\nxr_framegen_overlay_y={:.2}\n",
        i32::from(s.fsr3_optical_flow()),
        i32::from(s.enabled),
        i32::from(s.overlay),
        i32::from(s.auto_pace),
        s.backend,
        s.quality,
        s.flow_scale,
        s.overlay_corner,
        i32::from(s.cadence_lock()),
        s.overlay_layout,
        s.overlay_follow,
        s.history_seconds,
        s.overlay_cone as f64,
        s.overlay_scale as f64,
        s.overlay_distance as f64,
        s.overlay_x as f64,
        s.overlay_y as f64
    )
}
/// # Safety
/// `s` is readable; `out` holds `size` bytes. Returns the text's length (written NUL-terminated when it fits).
#[no_mangle]
pub unsafe extern "C" fn cvr_framegen_settings_text(
    s: *const Settings,
    out: *mut u8,
    size: usize,
) -> usize {
    // SAFETY: as the caller vouches.
    let Some(s) = (unsafe { s.as_ref() }) else {
        return 0;
    };
    let text = settings_text(s);
    if !out.is_null() && size > text.len() {
        // SAFETY: as the caller vouches.
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), out, text.len());
            *out.add(text.len()) = 0;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_lines_parse_as_sscanf_did() {
        let mut s = Settings::default();
        assert!(parse_setting(b"xr_framegen = 1", &mut s) && s.enabled);
        assert!(parse_setting(b"xr_framegen_overlay=1", &mut s) && s.overlay);
        assert!(parse_setting(b"xr_framegen_backend = 7", &mut s) && s.backend == 5);
        assert!(parse_setting(b"xr_framegen_backend = 3", &mut s) && s.backend == 3);
        assert!(parse_setting(b"xr_framegen_backend = 4", &mut s) && s.backend == 4);
        assert!(parse_setting(b"xr_fsr3_optical_flow = 0", &mut s) && !s.fsr3_optical_flow() && s.fsr3_show());
        assert!(parse_setting(b"xr_fsr3_optical_flow = 1", &mut s) && s.fsr3_optical_flow());
        assert!(
            parse_setting(b"xr_framegen_overlay_cone = 45.5", &mut s) && s.overlay_cone == 45.5
        );
        assert!(parse_setting(b"xr_framegen_overlay_x=-12", &mut s) && s.overlay_x == -12.0);
        assert!(!parse_setting(b"xr_framegen_unknown = 1", &mut s));
        assert!(!parse_setting(b" xr_framegen = 1", &mut s));
        assert!(!parse_setting(b"xr_framegen = x", &mut s));
    }

    #[test]
    fn settings_round_trip_through_their_text() {
        let s = Settings {
            enabled: true,
            overlay: true,
            overlay_cone: 33.25,
            overlay_x: -5.5,
            ..Settings::default()
        };
        let text = settings_text(&s);
        let mut back = Settings::default();
        for line in text.lines() {
            assert!(parse_setting(line.as_bytes(), &mut back), "{line}");
        }
        assert_eq!(back, s);
    }

    #[test]
    fn history_percentiles_and_lows() {
        let mut h = FrameHistory::new();
        for i in 0..1000 {
            h.add(i as f64, if i % 100 == 0 { 50.0 } else { 10.0 });
        }
        let s = h.summarize(999.0, 10.0);
        assert_eq!(s.samples, 1000);
        assert_eq!(s.peak, 50.0);
        assert_eq!(s.p99, 10.0);
        assert_eq!(s.p999, 50.0);
        assert!((s.low1 - 20.0).abs() < 1e-9);
    }
}
