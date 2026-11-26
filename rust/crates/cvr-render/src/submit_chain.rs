//! The submission chain and the deferred Present (formerly src/Stereo/SubmitChain.cpp; the file keeps the exported
//! switches and counters and the three detours' registration). Each render submission point (sub_1422EF830) is run
//! through a builder of the port's own whose first stage waits for the previous point's last stage (and the caller's),
//! so the points submit in order without the render thread waiting on each; a job of the port's marks each point
//! submitted. The presenting view's Present node (sub_140BF60F0), once the presenting output has been steady for 600
//! frames and nothing else presents, is deferred: the Present runs as an engine job after the chain's tail, the node
//! returning at once. Whatever must not overtake them waits: the next point, the next Present node, the next view job
//! builder (sub_140B603D8), and the packet code (CyberpunkVR_WaitDeferredPresentForPackets).
#![allow(unsafe_op_in_unsafe_fn)]
use crate::stereo_profiler::cvr_stereo_prof_now as prof_now;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::cell::Cell;
use std::sync::{Mutex, OnceLock};

#[allow(non_upper_case_globals)]
extern "C" {
    /// 0 off, 1 the chain (the view job builder waits too), 2 the chain without that wait.
    static CyberpunkVR_SubmitChain: AtomicU32;
    static CyberpunkVR_SubmitChainStats: [AtomicU64; 8];
    static CyberpunkVR_SubmitChainPriority: AtomicI32;
    static CyberpunkVR_SubmitChainDeferPresent: AtomicU32;
    static CyberpunkVR_DeferredPresentStats: [AtomicU64; 8];
    static CyberpunkVR_DeferredPresentGate: [AtomicU64; 4];
    static CyberpunkVR_DeferredPresentPacketWaits: [AtomicU64; 2];
    fn cvr_cpp_stereo_log(text: *const core::ffi::c_char);
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
    fn SwitchToThread() -> i32;
}
#[link(name = "user32")]
extern "system" {
    fn InvalidateRect(window: *mut c_void, rect: *const c_void, erase: i32) -> i32;
}

/// (builder, prio, prio2, tag): fresh after/done counters
const BUILDER_INIT_RVA: usize = 0x409858;
/// (builder, &last): hands over the last stage's counter
const BUILDER_FINISH_RVA: usize = 0x40993C;
/// (src, dst): dst stays pending until src completes
const COUNTER_LINK_RVA: usize = 0x272E1C;
/// (&counter): drops one reference
const COUNTER_RELEASE_RVA: usize = 0x2744CC;
/// (dispatcher, job, prio, after, done)
const JOB_SCHEDULE_RVA: usize = 0x272B40;
const DISPATCHER_RVA: usize = 0x3BCDBF8;
/// (unused, tag): a counter with one reference
const COUNTER_NEW_RVA: usize = 0x273008;
/// the render device the Present node calls
const RENDER_DEVICE_RVA: usize = 0x3BBCAE8;
/// (device+18048): XeSS frame generation present id
const XEFG_PRESENT_ID_RVA: usize = 0x229D9C0;
/// (&swapchain id, -, sync interval)
const SWAPCHAIN_PRESENT_RVA: usize = 0x5D95B8;
const BUILDER_TAG: usize = 8;
const BUILDER_AFTER: usize = 16;
const BUILDER_PRIO: usize = 40;
const BUILDER_PRIO2: usize = 41;
const COUNTER_PENDING: usize = 24;
/// Frames the presenting output must have been the same before its Present is deferred.
const STEADY_FRAMES: u64 = 600;

/// The engine's job record (what the schedule copies).
#[repr(C)]
struct EngineJob {
    f: unsafe extern "C" fn(*mut c_void, *mut c_void),
    data: *mut c_void,
    name: *const c_void,
    misc: u64,
}

/// The trampolines MinHook writes (src/Stereo/SubmitChain.cpp's CVR_DETOUR lines name them).
#[no_mangle]
pub static mut cvr_submit_point_original: usize = 0;
#[no_mangle]
pub static mut cvr_present_node_original: usize = 0;
#[no_mangle]
pub static mut cvr_view_builder_original: usize = 0;

/// One reference to the last stage of the last chained submission point (or the last deferred Present's counter).
struct Tail(usize);
static LOCK: Mutex<Tail> = Mutex::new(Tail(0));
static ISSUED: AtomicU64 = AtomicU64::new(0);
static SUBMITTED: AtomicU64 = AtomicU64::new(0);
static TIMEOUT_LOGGED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy)]
struct DeferredPresent {
    /// render data +3976
    output: *mut u8,
    /// render data +416
    handle: u64,
    seq: u64,
}
static mut DEFERRED: [DeferredPresent; 4] = [DeferredPresent {
    output: core::ptr::null_mut(),
    handle: 0,
    seq: 0,
}; 4];
static PRESENT_ISSUED: AtomicU64 = AtomicU64::new(0);
static PRESENT_DONE: AtomicU64 = AtomicU64::new(0);
/// The view object of the presenting view's graph.
static PRESENT_VIEW: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
thread_local! {
    static IN_DEFERRED_PRESENT: Cell<bool> = const { Cell::new(false) };
}

fn base() -> usize {
    static BASE: OnceLock<usize> = OnceLock::new();
    // SAFETY: the process image's own handle.
    *BASE.get_or_init(|| unsafe { GetModuleHandleW(core::ptr::null()) } as usize)
}
unsafe fn engine<F: Copy>(rva: usize) -> F {
    let address = base() + rva;
    core::mem::transmute_copy(&address)
}
fn log(text: String) {
    if let Ok(text) = std::ffi::CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
/// Half a second of QPC ticks: how long a wait holds on before it gives up.
fn give_up() -> i64 {
    static TICKS: OnceLock<i64> = OnceLock::new();
    *TICKS.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: a plain system query into a local.
        unsafe { QueryPerformanceFrequency(&mut f) };
        f / 2
    })
}

unsafe fn pending(counter: usize) -> bool {
    counter != 0 && core::ptr::read_volatile((counter + COUNTER_PENDING) as *const i32) != 0
}
fn chain_pending() -> bool {
    SUBMITTED.load(Acquire) < ISSUED.load(Acquire)
}
/// The job after a submission point's last stage: that point (its sequence number in `data`) has submitted.
unsafe extern "C" fn mark_submitted(data: *mut c_void, _unused: *mut c_void) {
    let seq = data as u64;
    let mut seen = SUBMITTED.load(Relaxed);
    while seen < seq {
        match SUBMITTED.compare_exchange_weak(seen, seq, Release, Relaxed) {
            Ok(_) => break,
            Err(now) => seen = now,
        }
    }
    CyberpunkVR_SubmitChainStats[1].fetch_add(1, Relaxed);
}
/// Until every issued point has submitted (or half a second); the ticks waited.
unsafe fn wait_submitted() -> i64 {
    let target = ISSUED.load(Acquire);
    if SUBMITTED.load(Acquire) >= target {
        return 0;
    }
    let t0 = prof_now();
    let mut i = 0u32;
    while SUBMITTED.load(Acquire) < target {
        if i < 64 {
            core::hint::spin_loop();
        } else {
            SwitchToThread();
        }
        if i & 255 == 255 && prof_now() - t0 > give_up() {
            CyberpunkVR_SubmitChainStats[6].fetch_add(1, Relaxed);
            if !TIMEOUT_LOGGED.swap(true, SeqCst) {
                log(format!(
                    "[submit-chain] chain stalled: {} of {} submission points submitted after 500 ms",
                    SUBMITTED.load(SeqCst),
                    target
                ));
            }
            break;
        }
        i = i.wrapping_add(1);
    }
    prof_now() - t0
}
fn present_pending() -> bool {
    PRESENT_DONE.load(Acquire) < PRESENT_ISSUED.load(Acquire)
}
/// Until the deferred Present has run (or half a second), except inside it; the ticks waited.
unsafe fn wait_presented() -> i64 {
    if !present_pending() || IN_DEFERRED_PRESENT.get() {
        return 0;
    }
    let t0 = prof_now();
    let mut i = 0u32;
    while present_pending() {
        if i < 64 {
            core::hint::spin_loop();
        } else {
            SwitchToThread();
        }
        if i & 255 == 255 && prof_now() - t0 > give_up() {
            CyberpunkVR_DeferredPresentStats[7].fetch_add(1, Relaxed);
            log(format!("[submit-chain] deferred present {} not done after 500 ms", PRESENT_ISSUED.load(SeqCst)));
            break;
        }
        i = i.wrapping_add(1);
    }
    prof_now() - t0
}

/// The submission point sub_1422EF830's detour.
///
/// # Safety
/// Called by the engine in place of its submission point, with its arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_submit_point(name: usize, range: *mut c_void, builder: *mut u8, fences: *mut c_void) {
    type Point = unsafe extern "C" fn(usize, *mut c_void, *mut u8, *mut c_void);
    let original: Point = core::mem::transmute(cvr_submit_point_original);
    if CyberpunkVR_SubmitChain.load(Relaxed) == 0 || builder.is_null() {
        if chain_pending() {
            CyberpunkVR_SubmitChainStats[7].fetch_add(1, Relaxed);
            wait_submitted();
        }
        wait_presented();
        original(name, range, builder, fences);
        return;
    }
    #[repr(C, align(16))]
    struct Own([u8; 64]);
    let mut own = Own([0u8; 64]);
    let forced = CyberpunkVR_SubmitChainPriority.load(Relaxed);
    let priority = if (0..=3).contains(&forced) {
        forced as u8
    } else {
        *builder.add(BUILDER_PRIO)
    };
    let mut tail = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let init: unsafe extern "C" fn(*mut u8, u8, u8, usize) -> *mut c_void = engine(BUILDER_INIT_RVA);
    init(
        own.0.as_mut_ptr(),
        priority,
        *builder.add(BUILDER_PRIO2),
        core::ptr::read_unaligned(builder.add(BUILDER_TAG) as *const usize),
    );
    let first = core::ptr::read_unaligned(own.0.as_ptr().add(BUILDER_AFTER) as *const usize);
    let caller_after = core::ptr::read_unaligned(builder.add(BUILDER_AFTER) as *const usize);
    let link: unsafe extern "C" fn(usize, usize) = engine(COUNTER_LINK_RVA);
    if pending(tail.0) {
        link(tail.0, first);
    }
    if pending(caller_after) {
        link(caller_after, first);
    }
    original(name, range, own.0.as_mut_ptr(), fences);
    let mut last = 0usize;
    let finish: unsafe extern "C" fn(*mut u8, *mut usize) -> *mut c_void = engine(BUILDER_FINISH_RVA);
    finish(own.0.as_mut_ptr(), &mut last);
    let seq = ISSUED.load(Relaxed) + 1;
    if last != 0 {
        let job = EngineJob {
            f: mark_submitted,
            data: seq as *mut c_void,
            name: core::ptr::null(),
            misc: 0,
        };
        let schedule: unsafe extern "C" fn(usize, *const EngineJob, u8, usize, usize) -> i32 = engine(JOB_SCHEDULE_RVA);
        schedule(
            core::ptr::read_volatile((base() + DISPATCHER_RVA) as *const usize),
            &job,
            own.0[BUILDER_PRIO],
            last,
            0,
        );
    }
    ISSUED.store(seq, Release);
    if last == 0 {
        mark_submitted(seq as *mut c_void, core::ptr::null_mut());
    }
    if tail.0 != 0 {
        let release: unsafe extern "C" fn(*mut usize) = engine(COUNTER_RELEASE_RVA);
        release(&mut tail.0);
    }
    tail.0 = last;
    drop(tail);
    CyberpunkVR_SubmitChainStats[0].fetch_add(1, Relaxed);
}

/// Whether the node's view presents: its render data's output and present flag (under SEH; a fault says yes).
unsafe extern "C" fn presents_raw(context: *mut u8, out: *mut bool, _unused: *mut c_void) -> bool {
    let view = *(context as *const *mut u8);
    let table = *(view as *const *const usize);
    let data_of: unsafe extern "C" fn(*mut c_void) -> *mut u8 = core::mem::transmute(*table.add(4));
    let data = data_of(view as *mut c_void);
    *out = !data.is_null() && !(*(data.add(0xF98) as *const *mut c_void)).is_null() && *data.add(336) != 0;
    true
}
unsafe fn presents(context: *mut u8) -> bool {
    let mut answer = false;
    match cvr_engine::code::guarded_call3(
        presents_raw as *const () as usize,
        context as *mut c_void,
        &mut answer as *mut bool as *mut c_void,
        core::ptr::null_mut(),
    ) {
        Some(_) => answer,
        None => true,
    }
}
/// What the engine's Present node does for the output: the device's frame end, the XeFG present id, the swapchain's
/// present and the window repainted.
unsafe fn run_present(output: *mut u8, handle: u64) {
    let device = core::ptr::read_volatile((base() + RENDER_DEVICE_RVA) as *const usize);
    let table = *(device as *const *const usize);
    type DeviceOutput = unsafe extern "C" fn(usize, u64);
    for slot in [56 / 8, 0x80 / 8, 0xA8 / 8] {
        let f: DeviceOutput = core::mem::transmute(*table.add(slot));
        f(device, handle);
    }
    let present_id: unsafe extern "C" fn(usize) = engine(XEFG_PRESENT_ID_RVA);
    present_id(*((device + 0x4678) as *const usize));
    if *output.add(0x17C) == 0 {
        let present: unsafe extern "C" fn(*const u32, usize, u32) -> usize = engine(SWAPCHAIN_PRESENT_RVA);
        present(output.add(56) as *const u32, 0, *(output.add(0x78) as *const u32));
        let window = *((*(output.add(48) as *const usize) + 8) as *const *mut c_void);
        InvalidateRect(window, core::ptr::null(), 1);
    }
}
/// The deferred Present's job.
unsafe extern "C" fn deferred_present_job(data: *mut c_void, _unused: *mut c_void) {
    let d = &*(data as *const DeferredPresent);
    IN_DEFERRED_PRESENT.set(true);
    run_present(d.output, d.handle);
    IN_DEFERRED_PRESENT.set(false);
    PRESENT_DONE.store(d.seq, Release);
    CyberpunkVR_DeferredPresentStats[1].fetch_add(1, Relaxed);
}
/// The presenting view's output and handle (under SEH; a fault says no).
unsafe extern "C" fn present_target_raw(context: *mut u8, output: *mut *mut u8, handle: *mut u64) -> bool {
    let view = *(context as *const *mut u8);
    let table = *(view as *const *const usize);
    let data_of: unsafe extern "C" fn(*mut c_void) -> *mut u8 = core::mem::transmute(*table.add(4));
    let data = data_of(view as *mut c_void);
    if data.is_null() || (*(data.add(0xF98) as *const *mut u8)).is_null() || *data.add(336) == 0 {
        return false;
    }
    *output = *(data.add(0xF98) as *const *mut u8);
    *handle = *(data.add(416) as *const u64);
    PRESENT_VIEW.store(view as *mut c_void, Relaxed);
    true
}
unsafe fn present_target(context: *mut u8, output: &mut *mut u8, handle: &mut u64) -> bool {
    cvr_engine::code::guarded_call3(
        present_target_raw as *const () as usize,
        context as *mut c_void,
        output as *mut *mut u8 as *mut c_void,
        handle as *mut u64 as *mut c_void,
    )
    .unwrap_or(false)
}
unsafe fn defer_present(output: *mut u8, handle: u64) {
    if present_pending() {
        let ticks = wait_presented();
        CyberpunkVR_DeferredPresentStats[2].fetch_add(1, Relaxed);
        CyberpunkVR_DeferredPresentStats[3].fetch_add(ticks as u64, Relaxed);
    }
    // The node's own clearing of the output's busy byte, done here as it would have been before the Present.
    let device = core::ptr::read_volatile((base() + RENDER_DEVICE_RVA) as *const usize);
    *((*((device + 18008) as *const usize) + 498) as *mut u8) = 0;
    let seq = PRESENT_ISSUED.load(Relaxed) + 1;
    let slot = &mut (*(&raw mut DEFERRED))[(seq & 3) as usize];
    *slot = DeferredPresent { output, handle, seq };
    let mut tail = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let counter_new: unsafe extern "C" fn(*mut c_void, usize) -> usize = engine(COUNTER_NEW_RVA);
    let counter = counter_new(core::ptr::null_mut(), 0);
    let job = EngineJob {
        f: deferred_present_job,
        data: slot as *mut DeferredPresent as *mut c_void,
        name: core::ptr::null(),
        misc: 0,
    };
    PRESENT_ISSUED.store(seq, Release);
    let schedule: unsafe extern "C" fn(usize, *const EngineJob, u8, usize, usize) -> i32 = engine(JOB_SCHEDULE_RVA);
    schedule(core::ptr::read_volatile((base() + DISPATCHER_RVA) as *const usize), &job, 3, tail.0, counter);
    if tail.0 != 0 {
        let release: unsafe extern "C" fn(*mut usize) = engine(COUNTER_RELEASE_RVA);
        release(&mut tail.0);
    }
    tail.0 = counter;
    drop(tail);
    CyberpunkVR_DeferredPresentStats[0].fetch_add(1, Relaxed);
}

/// The presenting view's node only, one frame at a time.
struct Steady {
    last_output: usize,
    last_handle: u64,
    steady: u64,
}
static STEADY: Mutex<Steady> = Mutex::new(Steady {
    last_output: 0,
    last_handle: 0,
    steady: 0,
});
/// Presenting nodes run; SECONDARY is PRESENTING when a graph last ran the node without presenting.
static PRESENTING: AtomicU64 = AtomicU64::new(0);
static SECONDARY: AtomicU64 = AtomicU64::new(0);

/// The Present node sub_140BF60F0's detour.
///
/// # Safety
/// Called by the engine in place of its node, with its arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_present_node(node: *mut c_void, context: *mut u8, job: *mut c_void) {
    let presenting_node = !context.is_null() && *context.add(48) & 2 != 0;
    if presenting_node && CyberpunkVR_SubmitChain.load(Relaxed) != 0 && CyberpunkVR_SubmitChainDeferPresent.load(Relaxed) != 0 {
        let (mut output, mut handle) = (core::ptr::null_mut(), 0u64);
        if present_target(context, &mut output, &mut handle) {
            let gate = &*(&raw const CyberpunkVR_DeferredPresentGate);
            let mut s = STEADY.lock().unwrap_or_else(|p| p.into_inner());
            if output as usize != s.last_output {
                gate[2].fetch_add(1, Relaxed);
            }
            if handle != s.last_handle {
                gate[3].fetch_add(1, Relaxed);
            }
            s.steady = if output as usize == s.last_output { s.steady + 1 } else { 0 };
            s.last_output = output as usize;
            s.last_handle = handle;
            let frame = PRESENTING.fetch_add(1, Relaxed) + 1;
            gate[0].store(s.steady, Relaxed);
            gate[1].store(frame.wrapping_sub(SECONDARY.load(Relaxed)), Relaxed);
            let steady = s.steady;
            drop(s);
            if steady >= STEADY_FRAMES && frame.wrapping_sub(SECONDARY.load(Relaxed)) <= 2 {
                defer_present(output, handle);
                return;
            }
            CyberpunkVR_DeferredPresentStats[6].fetch_add(1, Relaxed);
        } else {
            SECONDARY.store(PRESENTING.load(Relaxed), Relaxed);
        }
    }
    wait_presented();
    if presenting_node && chain_pending() && presents(context) {
        let ticks = wait_submitted();
        CyberpunkVR_SubmitChainStats[2].fetch_add(1, Relaxed);
        CyberpunkVR_SubmitChainStats[3].fetch_add(ticks as u64, Relaxed);
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut u8, *mut c_void) = core::mem::transmute(cvr_present_node_original);
    original(node, context, job);
}

/// The view job builder sub_140B603D8's detour: the next frame's views wait for the deferred Present, and for the
/// chain in mode 1.
///
/// # Safety
/// Called by the engine in place of its builder, with its arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_view_builder(builder: *mut c_void, job: *mut c_void, view: *mut i64) -> *mut c_void {
    if present_pending() {
        let ticks = wait_presented();
        CyberpunkVR_DeferredPresentStats[4].fetch_add(1, Relaxed);
        CyberpunkVR_DeferredPresentStats[5].fetch_add(ticks as u64, Relaxed);
    }
    if CyberpunkVR_SubmitChain.load(Relaxed) == 1 && chain_pending() {
        let ticks = wait_submitted();
        CyberpunkVR_SubmitChainStats[4].fetch_add(1, Relaxed);
        CyberpunkVR_SubmitChainStats[5].fetch_add(ticks as u64, Relaxed);
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut i64) -> *mut c_void = core::mem::transmute(cvr_view_builder_original);
    original(builder, job, view)
}

/// Until the deferred Present has run (the swapchain's own Present and the overlay call it).
#[no_mangle]
pub extern "C" fn CyberpunkVR_WaitDeferredPresent() {
    // SAFETY: the module's own waits.
    unsafe { wait_presented() };
}
/// The packet code's wait for the deferred Present, counted.
#[no_mangle]
pub extern "C" fn CyberpunkVR_WaitDeferredPresentForPackets() {
    if !present_pending() {
        return;
    }
    // SAFETY: the module's own waits and the C++ counters.
    unsafe {
        let ticks = wait_presented();
        if ticks == 0 {
            return;
        }
        CyberpunkVR_DeferredPresentPacketWaits[0].fetch_add(1, Relaxed);
        CyberpunkVR_DeferredPresentPacketWaits[1].fetch_add(ticks as u64, Relaxed);
    }
}
/// xr_submit_chain: 0 off, 1 the chain with the view builder's wait, 2 without it.
#[no_mangle]
pub extern "C" fn CyberpunkVR_SubmitChainConfigure(mode: i32) {
    // SAFETY: the C++ switch.
    unsafe { CyberpunkVR_SubmitChain.store(mode.clamp(0, 2) as u32, Relaxed) };
}
