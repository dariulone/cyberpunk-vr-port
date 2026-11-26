//! The XR diagnostics' clock (src/Runtimes/OpenXRFrameLoop.cpp, `XrDiagNowMs`): the performance counter in
//! milliseconds, 0 when the counter has no frequency; the frequency read once.
//!
//! The C++ half's `XrDiagNowMs` forwards to `cvr_xr_diag_now_ms`; the unit test below checks it advances.
use core::sync::atomic::{AtomicI64, Ordering::Relaxed};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

static FREQUENCY: AtomicI64 = AtomicI64::new(0);

/// The counter's value as milliseconds at its frequency.
pub fn milliseconds(count: i64, frequency: i64) -> f64 {
    if frequency == 0 { 0.0 } else { count as f64 * 1000.0 / frequency as f64 }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_xr_diag_now_ms() -> f64 {
    let mut frequency = FREQUENCY.load(Relaxed);
    if frequency == 0 {
        // SAFETY: an out-parameter.
        let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
        FREQUENCY.store(frequency, Relaxed);
    }
    if frequency == 0 {
        return 0.0;
    }
    let mut count = 0i64;
    // SAFETY: an out-parameter.
    let _ = unsafe { QueryPerformanceCounter(&mut count) };
    milliseconds(count, frequency)
}

#[cfg(test)]
mod tests {
    //! The clock advances.
    use super::*;

    #[test]
    fn clock() {
        assert_eq!(milliseconds(5_000, 10_000), 500.0);
        assert_eq!(milliseconds(5, 0), 0.0);
        let a = cvr_xr_diag_now_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = cvr_xr_diag_now_ms();
        assert!(a > 0.0 && b - a >= 4.0, "{a} {b}");
    }
}
