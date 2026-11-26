//! THE XR FRAME LOOP'S TRACES (RuntimeDiagnostics only; main's 0.2.4 OpenXRFrameLoop.cpp, OpenXRCapture.cpp and
//! OpenXRPresent.cpp): what each display cycle showed and when its stages ran, when each capture was done on the GPU,
//! and where the game's Present waited -- read by tools/render_runtime_audit to tell a frame the game delivered late
//! from one the GPU finished late from one the XR thread did not take.
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering::*};
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::ID3D12Fence;

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
}
#[cfg(not(test))]
fn diagnostics() -> bool {
    // SAFETY: the exported switch.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
#[cfg(test)]
fn diagnostics() -> bool {
    true
}

/// WHAT EACH DISPLAY CYCLE SHOWED: per projection submit -- now ms, predicted display time ns, serial, synthetic, cadence
/// kind (-1 without the presenter; a resent cycle: synthetic -1, kind -2), capture ms, xrEndFrame ok, predicted period
/// ns, native frame, xrWaitFrame ms, xrEndFrame ms, the presenter's selection and generation ms.
#[no_mangle]
pub static mut CyberpunkVR_XrCycleTrace: [[f64; 12]; 8192] = [[0.0; 12]; 8192];
#[no_mangle]
pub static CyberpunkVR_XrCycleTraceCount: AtomicU64 = AtomicU64::new(0);
/// The same cycle's stage instants (ms, same clock, 0 = not reached): xrWaitFrame returned, xrBeginFrame returned, views
/// located, input begins, selection begins, eye image acquired, copies submitted, xrEndFrame called.
#[no_mangle]
pub static mut CyberpunkVR_XrStageTrace: [[f64; 8]; 8192] = [[0.0; 8]; 8192];

pub const STAGE_WAITED: usize = 0;
pub const STAGE_BEGUN: usize = 1;
pub const STAGE_LOCATED: usize = 2;
pub const STAGE_INPUT: usize = 3;
pub const STAGE_SELECTION: usize = 4;
pub const STAGE_IMAGE: usize = 5;
pub const STAGE_COPIES: usize = 6;
pub const STAGE_END: usize = 7;

/// The cycle being traced (the XR thread's).
static STAGES: Mutex<[f64; 8]> = Mutex::new([0.0; 8]);

/// XrStage: a stage of this cycle reached now (the first clears the others).
pub fn stage(index: usize) {
    if !diagnostics() || index >= 8 {
        return;
    }
    let mut s = STAGES.lock().unwrap_or_else(|e| e.into_inner());
    if index == STAGE_WAITED {
        *s = [0.0; 8];
    }
    s[index] = crate::tracking::now_ms();
}
/// The cycle's row and its stages, appended to the ring.
pub fn cycle(row: [f64; 12]) {
    if !diagnostics() {
        return;
    }
    let stages = *STAGES.lock().unwrap_or_else(|e| e.into_inner());
    let n = CyberpunkVR_XrCycleTraceCount.load(Relaxed);
    let i = (n % 8192) as usize;
    // SAFETY: the rings' one writer (the XR thread); readers follow the count.
    unsafe {
        (*(&raw mut CyberpunkVR_XrCycleTrace))[i] = row;
        (*(&raw mut CyberpunkVR_XrStageTrace))[i] = stages;
    }
    CyberpunkVR_XrCycleTraceCount.store(n + 1, Release);
}

// ---- when each capture is done on the GPU

/// Serial, capture ms, GPU-complete ms, fence value.
#[no_mangle]
pub static mut CyberpunkVR_CaptureDoneTrace: [[f64; 4]; 4096] = [[0.0; 4]; 4096];
#[no_mangle]
pub static CyberpunkVR_CaptureDoneCount: AtomicU64 = AtomicU64::new(0);

struct Item {
    fence: ID3D12Fence,
    row: [f64; 4],
}
// SAFETY: a fence is free-threaded.
unsafe impl Send for Item {}
struct Watch {
    pending: VecDeque<Item>,
    started: bool,
}
static WATCH: Mutex<Watch> = Mutex::new(Watch { pending: VecDeque::new(), started: false });
static WAKE: Condvar = Condvar::new();

/// WatchCapture: one watcher thread waits on the capture fence in submission order and stamps when each capture was
/// done (64 pending at most).
///
/// # Safety
/// `fence` is a live ID3D12Fence or null.
pub unsafe fn watch_capture(fence: *mut c_void, serial: u64, capture_ms: f64, value: u64) {
    if !diagnostics() {
        return;
    }
    // SAFETY: as the caller vouches; one reference of our own, held while pending (a session restart may release the
    // port's).
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }).cloned() else { return };
    let mut w = WATCH.lock().unwrap_or_else(|e| e.into_inner());
    if !w.started {
        w.started = true;
        let _ = std::thread::Builder::new().name("CVR capture completion".into()).spawn(watcher);
    }
    if w.pending.len() < 64 {
        w.pending.push_back(Item { fence, row: [serial as f64, capture_ms, 0.0, value as f64] });
    }
    WAKE.notify_one();
}
fn watcher() {
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    // SAFETY: an auto-reset event of the thread's own.
    let Ok(event) = (unsafe { CreateEventW(None, false, false, None) }) else { return };
    loop {
        let mut item = {
            let mut w = WATCH.lock().unwrap_or_else(|e| e.into_inner());
            while w.pending.is_empty() {
                w = WAKE.wait(w).unwrap_or_else(|e| e.into_inner());
            }
            w.pending.pop_front().expect("not empty")
        };
        let value = item.row[3] as u64;
        let mut done = false;
        for _ in 0..10 {
            // SAFETY: the held fence; our event.
            let completed = unsafe { item.fence.GetCompletedValue() };
            if completed == u64::MAX {
                break; // device removed
            }
            done = completed >= value;
            if done {
                break;
            }
            // SAFETY: as above.
            if unsafe { item.fence.SetEventOnCompletion(value, event) }.is_ok() {
                // SAFETY: our event.
                unsafe { WaitForSingleObject(event, 100) };
            }
        }
        if !done {
            continue;
        }
        item.row[2] = crate::tracking::now_ms();
        let n = CyberpunkVR_CaptureDoneCount.load(Relaxed);
        // SAFETY: the ring's one writer (this thread); readers follow the count.
        unsafe { (*(&raw mut CyberpunkVR_CaptureDoneTrace))[(n % 4096) as usize] = item.row };
        CyberpunkVR_CaptureDoneCount.store(n + 1, Release);
    }
}

// ---- where the game's Present waits

/// Accumulated microseconds per OnPresent phase: 0 hand lock, 1 FlushHandsToShared, 2 view lock, 3 render pose lock,
/// 4 mono capture, 5 foveated/VRS ticks, 6 main HUD capture (with the optics probe), 7 the other HUD channels.
#[no_mangle]
pub static CyberpunkVR_OnPresentPhaseUs: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
pub const PHASE_FLUSH_HANDS: usize = 1;
pub const PHASE_CAPTURE: usize = 4;
pub const PHASE_TICKS: usize = 5;
pub const PHASE_HUD: usize = 6;

/// A phase's start (0 with the diagnostics off).
pub fn phase_start() -> u64 {
    if !diagnostics() {
        return 0;
    }
    let mut t = 0i64;
    // SAFETY: a plain counter read.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    t as u64
}
/// The phase's time added.
pub fn phase_end(index: usize, start: u64) {
    if start == 0 || index >= 8 {
        return;
    }
    static FREQUENCY: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let frequency = *FREQUENCY.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: a plain counter read.
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut f) };
        f.max(1) as u64
    });
    let mut t = 0i64;
    // SAFETY: as above.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    CyberpunkVR_OnPresentPhaseUs[index].fetch_add((t as u64).wrapping_sub(start) * 1_000_000 / frequency, Relaxed);
}

/// C names for the render crate.
#[no_mangle]
pub extern "C" fn cvr_xr_trace_stage(index: u32) {
    stage(index as usize);
}
/// The resource whose capture is watched is the caller's; its fence a live one.
///
/// # Safety
/// As watch_capture.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_trace_watch_capture(fence: *mut c_void, serial: u64, capture_ms: f64, value: u64) {
    // SAFETY: as the caller vouches.
    unsafe { watch_capture(fence, serial, capture_ms, value) }
}
#[allow(dead_code)]
fn _interface_used(f: &ID3D12Fence) -> *mut c_void {
    f.as_raw()
}
