//! The game thread does not run render jobs while it waits (formerly src/Stereo/GameThreadQueues.cpp; the file keeps
//! the exported switch and counters and the two detours' registration). The game thread waits for its frame's job
//! counters through sub_1405D3CFC -> FlushCounter, taking jobs itself through the dequeue sub_14026DE10, which tries the
//! dispatcher's queues 3, 2 and 1 in that order; render node jobs are on 1. With
//! CyberpunkVR_GameThreadSkipRenderQueue on, the game thread's dequeue tries only 3 and 2 and gives the job token back
//! exactly as the original's empty path does.
//!
//! THE WAIT FOR THE WORLD TICK RUNS THE LUA HOST'S COLLECTOR: inside that wait (the counter wait called from the world
//! tick, told by its return address), an empty dequeue lends the time to the host's collector (src/Lua/HostGc.cpp
//! CvrLuaIdleWork, one slice of ~60 us).
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::cell::Cell;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_GameThreadSkipRenderQueue: AtomicU32;
    /// [0] dequeues, [1] taken from queue 3, [2] from queue 2, [3] found nothing on 3 and 2 while 1 had jobs.
    static CyberpunkVR_GameThreadQueueStats: [AtomicU64; 4];
    /// The Lua host's collector slice.
    fn CvrLuaIdleWork();
}
#[link(name = "kernel32")]
extern "system" {
    fn ReleaseSemaphore(semaphore: *mut c_void, count: i32, previous: *mut i32) -> i32;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

/// The trampolines MinHook writes (src/Stereo/GameThreadQueues.cpp's CVR_DETOUR lines name them).
#[no_mangle]
pub static mut cvr_stereo_gt_dequeue_original: usize = 0;
#[no_mangle]
pub static mut cvr_stereo_world_tick_wait_original: usize = 0;

/// The world tick's call of the counter wait returns here.
const WORLD_TICK_WAIT_RETURN_RVA: usize = 0x69C8CC;
const AVAILABLE: usize = 1152;
const SEMAPHORE: usize = 1160;
const RING_ENTRIES: usize = 32;
const RING_CAPACITY: usize = 40;
const RING_READ: usize = 64;
const RING_WRITE: usize = 128;

thread_local! {
    static WORLD_TICK_WAIT: Cell<bool> = const { Cell::new(false) };
}

fn exe_base() -> usize {
    static BASE: AtomicUsize = AtomicUsize::new(0);
    let mut base = BASE.load(Relaxed);
    if base == 0 {
        // SAFETY: the process image's own handle.
        base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
        BASE.store(base, Relaxed);
    }
    base
}

unsafe extern "C" fn world_tick_wait(counter: *mut i64, return_address: usize) -> u8 {
    let original: unsafe extern "C" fn(*mut i64) -> u8 = core::mem::transmute(cvr_stereo_world_tick_wait_original);
    let base = exe_base();
    if base == 0 || return_address != base + WORLD_TICK_WAIT_RETURN_RVA {
        return original(counter);
    }
    WORLD_TICK_WAIT.set(true);
    let result = original(counter);
    WORLD_TICK_WAIT.set(false);
    result
}
/// The counter wait sub_1405D3CFC's detour: its return address handed on as a second argument.
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_world_tick_wait() {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {body}", body = sym world_tick_wait);
}

fn ring(queue: usize) -> usize {
    128 + 256 * queue
}
/// One entry (40 bytes) taken from `queue`'s ring, as the engine's own dequeue takes it.
unsafe fn pop(q: *mut u8, queue: usize, out: *mut c_void) -> bool {
    let r = q.add(ring(queue));
    let read = &*(r.add(RING_READ) as *const AtomicI64);
    let at = read.load(Relaxed);
    if (at as u64) >= core::ptr::read_volatile(r.add(RING_WRITE) as *const u64) {
        return false;
    }
    let mask = core::ptr::read_unaligned(r.add(RING_CAPACITY) as *const u64).wrapping_sub(1);
    let entries = core::ptr::read_unaligned(r.add(RING_ENTRIES) as *const *const u8);
    let entry = entries.add(40 * ((at as u64) & mask) as usize);
    let mut copy = [0u8; 40];
    core::ptr::copy_nonoverlapping(entry, copy.as_mut_ptr(), 40);
    if read.compare_exchange(at, at + 1, SeqCst, SeqCst).is_err() {
        return false;
    }
    core::ptr::copy_nonoverlapping(copy.as_ptr(), out as *mut u8, 40);
    true
}
unsafe fn queued(q: *mut u8, queue: usize) -> bool {
    let r = q.add(ring(queue));
    core::ptr::read_volatile(r.add(RING_READ) as *const u64) < core::ptr::read_volatile(r.add(RING_WRITE) as *const u64)
}
unsafe fn dequeue(q: *mut u8, out: *mut c_void, queue: *mut u8) -> u8 {
    if CyberpunkVR_GameThreadSkipRenderQueue.load(Relaxed) == 0 {
        let original: unsafe extern "C" fn(*mut u8, *mut c_void, *mut u8) -> u8 = core::mem::transmute(cvr_stereo_gt_dequeue_original);
        return original(q, out, queue);
    }
    let stats = &*(&raw const CyberpunkVR_GameThreadQueueStats);
    stats[0].fetch_add(1, Relaxed);
    let available = &*(q.add(AVAILABLE) as *const AtomicI32);
    let count = available.load(Relaxed);
    if count <= 0 || available.compare_exchange(count, count - 1, SeqCst, SeqCst).is_err() {
        return 0;
    }
    if pop(q, 3, out) {
        *queue = 3;
        stats[1].fetch_add(1, Relaxed);
        return 1;
    }
    if pop(q, 2, out) {
        *queue = 2;
        stats[2].fetch_add(1, Relaxed);
        return 1;
    }
    if queued(q, 1) {
        stats[3].fetch_add(1, Relaxed);
    }
    // The token given back as the original's empty path does.
    if available.fetch_add(1, SeqCst) < 0 {
        ReleaseSemaphore(core::ptr::read_unaligned(q.add(SEMAPHORE) as *const *mut c_void), 1, core::ptr::null_mut());
    }
    0
}
/// The game thread's dequeue sub_14026DE10's detour.
///
/// # Safety
/// Called by the engine in place of its dequeue, with its arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_gt_dequeue(q: *mut u8, out: *mut c_void, queue: *mut u8) -> u8 {
    let taken = dequeue(q, out, queue);
    if taken == 0 && WORLD_TICK_WAIT.get() {
        CvrLuaIdleWork();
    }
    taken
}
