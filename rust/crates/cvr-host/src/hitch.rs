//! HITCH TRACE (CVR_HITCH_TRACE=1, tests only): a span of work on a frame's path that took longer than a bound is logged
//! with the QPC clock in seconds, where it ran and how long it took -- to tell which part of the port or the game
//! froze a frame, lined up with SteamVR's frame timings and the port's display-cycle trace on the same clock. Off, the
//! cost is one cached flag read per span.
use std::sync::OnceLock;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// The trace is on (CVR_HITCH_TRACE set to anything but "0").
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("CVR_HITCH_TRACE").is_ok_and(|v| v != "0"))
}
/// Log entries headed by the QPC clock (CVR_LOG_QPC set to anything but "0", or the hitch trace on).
pub fn log_qpc() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| on() || std::env::var("CVR_LOG_QPC").is_ok_and(|v| v != "0"))
}
fn frequency() -> f64 {
    static F: OnceLock<f64> = OnceLock::new();
    *F.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: a plain counter read.
        unsafe { QueryPerformanceFrequency(&mut f) };
        f.max(1) as f64
    })
}
/// Now on the QPC clock, in seconds.
pub fn now_s() -> f64 {
    let mut t = 0i64;
    // SAFETY: a plain counter read.
    unsafe { QueryPerformanceCounter(&mut t) };
    t as f64 / frequency()
}
/// A span's start: now in seconds while the trace is on, else 0.
pub fn start() -> f64 {
    if on() {
        now_s()
    } else {
        0.0
    }
}
/// The span from `start` logged as `what` when it took at least `bound_ms`; its length in ms (0 with the trace off).
pub fn end(start: f64, bound_ms: f64, what: impl FnOnce() -> String) -> f64 {
    if start == 0.0 {
        return 0.0;
    }
    let now = now_s();
    let ms = (now - start) * 1000.0;
    if ms >= bound_ms {
        crate::log::write(&format!("[hitch] q={start:.4} {:.1} ms {} (thread {})\n", ms, what(), thread_id()));
    }
    ms
}
fn thread_id() -> u32 {
    // SAFETY: plain Win32 call.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}
