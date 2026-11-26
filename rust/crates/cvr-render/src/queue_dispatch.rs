//! The command queues' ExecuteCommandLists dispatcher (formerly src/Stereo/DeviceHooks.cpp's Hook_ExecuteCommandLists,
//! patch_queue_vtable and command_queue_original_execute): each hooked queue vtable's original kept beside its table
//! (at most 16), the slot swapped to the .cpp's frame; per submission -- the streaming and video memory polls, the list
//! audit, the mirror blit detected --, under the image submission lock: the frame generation and foveation, the command
//! resources and scene profile prepared, the readers' and recordings' submissions observed, the retained and paired
//! geometry's serialization and the retained prefix boundary, the geometry snapshots and descriptor audit before the
//! submission, under the registered queue order: the registered constants and preparation lists submitted, the lists
//! executed through the camera optics (one by one around the retained boundary when serialized, the order released
//! across the retained prefix), the readers' submissions; then the consumers' submitted halves; the mirror copy after.
//! The timing counters as before.
//!
//! The .cpp keeps the hook's frame (its return address) and lends the subsystems still in C++ through the runtime; the
//! Rust halves its callbacks once only forwarded to are called here directly (`halves`).

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::Mutex;

type Queue = *mut c_void;
type Lists = *const *mut c_void;
pub type Execute = unsafe extern "system" fn(queue: Queue, count: u32, lists: Lists);
const HOOK_CAPACITY: usize = 16;
const SLOT: usize = 10;

/// What the .cpp lends.
#[repr(C)]
pub struct Runtime {
    /// CyberpunkVR_EclTiming.
    pub timing: *const [AtomicU64; 8],
    /// The .cpp's hook frame, the vtable slot's target.
    pub hook: usize,
    pub log: extern "C" fn(text: *const u8),
    /// texture/buffer reads' and target access's observers made unavailable.
    pub read_unavailable: extern "C" fn(),
    /// The constants' queue API and the diagnostic submission count (after the Rust audit halves).
    pub list_audit: extern "C" fn(queue: Queue, count: u32, lists: Lists),
    /// The mirror's pending blit list.
    pub pending_blit: extern "C" fn() -> *mut c_void,
    /// The image submission lock (true lock, false unlock).
    pub image_lock: extern "C" fn(lock: bool),
    /// The game queue's frame generation (before the foveation).
    pub before: extern "C" fn(queue: Queue, count: u32, lists: Lists),
    /// The command resources and the scene profile prepared into the two slots (constructed there).
    pub prepare: extern "C" fn(count: u32, lists: Lists, slots: *mut [*mut c_void; 2]),
    /// The target handoff and the submission frontier (after the Rust readers' and recordings' halves).
    pub before_reads: extern "C" fn(queue: Queue, count: u32, lists: Lists, caller: *mut c_void),
    /// 0 retained serialization, 1 paired serialization, 2 the retained current boundary.
    pub serialize: extern "C" fn(which: u32, queue: Queue, count: u32, lists: Lists) -> u32,
    /// The retained ordered execution and the early depth's submission.
    pub ordered: extern "C" fn(queue: Queue, count: u32, lists: Lists, serialize: bool),
    /// The geometry snapshots constructed into the slot.
    pub geometry: extern "C" fn(count: u32, lists: Lists, slot: *mut c_void),
    /// The registered queue order: 0 wanted, 1 lock, 2 unlock, 3 refresh the fences.
    pub order: extern "C" fn(which: u32) -> bool,
    /// The registered constants and the preparation lists submitted (`owns` the order).
    pub submit_prefix: extern "C" fn(queue: Queue, count: u32, lists: Lists, owns: bool),
    /// The camera optics' execution through the original.
    pub camera_execute: extern "C" fn(queue: Queue, count: u32, lists: Lists, original: Execute),
    /// The target handoff's submission (after the Rust readers' and recordings' halves).
    pub submitted_reads: extern "C" fn(queue: Queue, count: u32, lists: Lists),
    /// The retained prefix executed, submitted and its current submitted.
    pub retained_prefix: extern "C" fn(queue: Queue, boundary: u32, lists: Lists),
    /// The consumers' submitted halves, the three slots' objects consumed and destroyed (before the frame generation's
    /// inputs and the menu surface).
    pub after: extern "C" fn(queue: Queue, count: u32, lists: Lists, boundary: u32, resources: *mut c_void, profile: *mut c_void, geometry: *mut c_void),
    /// The game queue's mirror copy.
    pub mirror: extern "C" fn(queue: Queue),
}
// SAFETY: immutable; its pointers name process-lifetime objects and functions.
unsafe impl Sync for Runtime {}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}

// ---- the hooked queue tables
static TABLES: [AtomicUsize; HOOK_CAPACITY] = [const { AtomicUsize::new(0) }; HOOK_CAPACITY];
static ORIGINALS: [AtomicUsize; HOOK_CAPACITY] = [const { AtomicUsize::new(0) }; HOOK_CAPACITY];
static COUNT: AtomicU32 = AtomicU32::new(0);
static INSTALL: Mutex<()> = Mutex::new(());

/// command_queue_original_execute: the queue's vtable original.
pub fn original_execute(queue: Queue) -> Option<Execute> {
    if queue.is_null() {
        return None;
    }
    // SAFETY: a live queue's vtable pointer.
    let table = unsafe { *queue.cast::<usize>() };
    let n = (COUNT.load(Acquire) as usize).min(HOOK_CAPACITY);
    (0..n).find(|&i| TABLES[i].load(Relaxed) == table).and_then(|i| {
        let original = ORIGINALS[i].load(Relaxed);
        // SAFETY: the slot's original ExecuteCommandLists.
        (original != 0).then(|| unsafe { core::mem::transmute::<usize, Execute>(original) })
    })
}
/// patch_queue_vtable: the queue's ExecuteCommandLists slot pointed at the hook, its original kept.
pub fn patch(queue: Queue) {
    let Some(r) = rt() else { return };
    if queue.is_null() {
        return;
    }
    let _g = INSTALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if original_execute(queue).is_some() {
        return;
    }
    let count = COUNT.load(Relaxed) as usize;
    if count >= HOOK_CAPACITY {
        (r.read_unavailable)();
        return;
    }
    // SAFETY: the queue's vtable; the slot made writable for the swap.
    let table = unsafe { *queue.cast::<*mut usize>() };
    let slot = unsafe { table.add(SLOT) };
    let mut old = 0u32;
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};
    if unsafe { VirtualProtect(slot.cast(), 8, PAGE_READWRITE, &mut old) } == 0 {
        (r.read_unavailable)();
        return;
    }
    let original = unsafe { *slot };
    TABLES[count].store(table as usize, Relaxed);
    ORIGINALS[count].store(original, Relaxed);
    COUNT.store(count as u32 + 1, Release);
    // SAFETY: the writable slot.
    unsafe { (*slot.cast::<AtomicUsize>()).swap(r.hook, SeqCst) };
    let mut junk = 0u32;
    unsafe { VirtualProtect(slot.cast(), 8, old, &mut junk) };
    let text = format!("[exec] ExecuteCommandLists hooked (queue={queue:p} orig={:p})\0", original as *const c_void);
    (r.log)(text.as_ptr());
}

fn ticks() -> u64 {
    let mut t = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    t as u64
}
/// The image submission lock, released however the dispatch leaves.
struct ImageLock(&'static Runtime);
impl ImageLock {
    fn new(r: &'static Runtime) -> Self {
        (r.image_lock)(true);
        ImageLock(r)
    }
}
impl Drop for ImageLock {
    fn drop(&mut self) {
        (self.0.image_lock)(false);
    }
}
/// The registered queue order (a deferred lock).
struct Order {
    r: &'static Runtime,
    owns: bool,
}
impl Order {
    fn lock(&mut self) {
        (self.r.order)(1);
        self.owns = true;
    }
    fn unlock(&mut self) {
        (self.r.order)(2);
        self.owns = false;
    }
}
impl Drop for Order {
    fn drop(&mut self) {
        if self.owns {
            self.unlock();
        }
    }
}
/// A slot on the dispatch's stack for one of the .cpp's move-only objects (no heap per submission).
#[repr(C, align(16))]
struct Slot([u8; SLOT_BYTES]);
pub const SLOT_BYTES: usize = 256;
fn offset(lists: Lists, by: u32) -> Lists {
    if lists.is_null() {
        lists
    } else {
        // SAFETY: within the call's lists.
        unsafe { lists.add(by as usize) }
    }
}

/// The Rust halves the runtime's callbacks once only forwarded to (the .cpp's list audit, foveation, readers', recordings',
/// frame generation inputs' and menu surface's facades), in the callbacks' order. Outside the plugin build (the tests)
/// they are absent: an unattached dispatch returns before them.
#[cfg(feature = "rust-build")]
mod halves {
    use super::{Lists, Queue};
    use core::ffi::c_void;
    extern "C" {
        /// cvr-rig's menu surface.
        fn cvr_menu_surface_submitted(queue: Queue, count: u32, lists: Lists);
    }
    /// The streaming placement poll, the submission's video memory and the list audit.
    pub fn audit(queue: Queue, n: u32, lists: Lists) {
        crate::device_creation::cvr_poll_streaming_placed();
        // SAFETY: the hooked call's queue and lists.
        unsafe {
            crate::device_creation::cvr_note_submit_video_memory(queue);
            cvr_gpu::gpu_diag::cvr_list_audit_execute(queue, n, lists);
        }
    }
    /// The foveation.
    pub fn before(queue: Queue, n: u32, lists: Lists) {
        // SAFETY: as audit.
        unsafe { crate::foveated_dlss::cvr_foveated_dlss_before_submit(queue, n, lists) };
    }
    /// The GPU probe, the texture and buffer reads and the native recordings (`caller` the hook's return address).
    pub fn before_reads(queue: Queue, n: u32, lists: Lists, caller: *mut c_void) {
        // SAFETY: as audit.
        unsafe {
            crate::gpu_probe_runtime::cvr_sgpw_submit(false, queue, n, lists);
            crate::texture_receipts::cvr_tr_before_submit(queue, n, lists);
            crate::buffer_read::cvr_buffer_read_before_submit(queue, n, lists);
            crate::native_recording::cvr_native_recording_before_submit(queue, n, lists, caller as usize);
        }
    }
    /// The texture and buffer reads' and the native recordings' submissions.
    pub fn submitted_reads(queue: Queue, n: u32, lists: Lists) {
        // SAFETY: as audit.
        unsafe {
            crate::texture_receipts::cvr_tr_submitted(queue, n, lists);
            crate::buffer_read::cvr_buffer_read_submitted(queue, n, lists);
            crate::native_recording::cvr_native_recording_submitted(queue, n, lists);
        }
    }
    /// The frame generation's inputs and the menu surface told of the submission.
    pub fn after(queue: Queue, n: u32, lists: Lists) {
        // SAFETY: as audit.
        unsafe {
            crate::framegen_inputs::cvr_framegen_inputs_submitted(queue, n, lists);
            cvr_menu_surface_submitted(queue, n, lists);
        }
    }
}
#[cfg(not(feature = "rust-build"))]
mod halves {
    use super::{Lists, Queue};
    use core::ffi::c_void;
    pub fn audit(_: Queue, _: u32, _: Lists) {}
    pub fn before(_: Queue, _: u32, _: Lists) {}
    pub fn before_reads(_: Queue, _: u32, _: Lists, _: *mut c_void) {}
    pub fn submitted_reads(_: Queue, _: u32, _: Lists) {}
    pub fn after(_: Queue, _: u32, _: Lists) {}
}

/// Hook_ExecuteCommandLists' body (`caller` the hook's return address).
pub fn execute(queue: Queue, n: u32, lists: Lists, caller: *mut c_void) {
    let Some(r) = rt() else { return };
    // SAFETY: the export.
    let timing = unsafe { &*r.timing };
    let start = ticks();
    timing[0].fetch_add(1, Relaxed);
    timing[1].fetch_add(n.into(), Relaxed);
    let Some(original) = original_execute(queue) else { return };
    halves::audit(queue, n, lists);
    (r.list_audit)(queue, n, lists);
    let pending = (r.pending_blit)();
    // SAFETY: the call's lists.
    let submits_blit = !pending.is_null() && !lists.is_null() && (0..n as usize).any(|i| unsafe { *lists.add(i) } == pending);
    {
        let _image = ImageLock::new(r);
        (r.before)(queue, n, lists);
        halves::before(queue, n, lists);
        let mut slots = [Slot([0; SLOT_BYTES]), Slot([0; SLOT_BYTES]), Slot([0; SLOT_BYTES])];
        let [resources, profile, geometry] = &mut slots;
        let mut handles = [(resources as *mut Slot).cast::<c_void>(), (profile as *mut Slot).cast::<c_void>()];
        let geometry = (geometry as *mut Slot).cast::<c_void>();
        (r.prepare)(n, lists, &mut handles);
        halves::before_reads(queue, n, lists, caller);
        (r.before_reads)(queue, n, lists, caller);
        let retained = (r.serialize)(0, queue, n, lists) != 0;
        let paired = (r.serialize)(1, queue, n, lists) != 0;
        let serialize = retained || paired;
        let boundary = if retained && serialize { (r.serialize)(2, queue, n, lists) } else { 0 };
        (r.ordered)(queue, n, lists, serialize);
        (r.geometry)(n, lists, geometry);
        if !lists.is_null() {
            // SAFETY: the call's lists, as addresses.
            crate::descriptor_audit::before_submit(unsafe { core::slice::from_raw_parts(lists.cast::<u64>(), n as usize) });
        }
        {
            let mut order = Order { r, owns: false };
            if (r.order)(0) {
                order.lock();
            }
            if order.owns {
                (r.order)(3);
            }
            let prefix = if boundary != 0 { boundary } else { n };
            (r.submit_prefix)(queue, prefix, lists, order.owns);
            if serialize && !lists.is_null() {
                for i in 0..n {
                    (r.camera_execute)(queue, 1, offset(lists, i), original);
                    if boundary != 0 && i + 1 == boundary {
                        halves::submitted_reads(queue, boundary, lists);
                        (r.submitted_reads)(queue, boundary, lists);
                        let relock = order.owns;
                        if relock {
                            order.unlock();
                        }
                        (r.retained_prefix)(queue, boundary, lists);
                        if relock {
                            order.lock();
                        }
                        (r.submit_prefix)(queue, n - boundary, offset(lists, boundary), order.owns);
                    }
                }
            } else {
                let driver_start = ticks();
                timing[3].fetch_add(driver_start - start, Relaxed);
                (r.camera_execute)(queue, n, lists, original);
                let driver_end = ticks();
                timing[4].fetch_add(driver_end - driver_start, Relaxed);
                timing[5].fetch_sub(driver_end, Relaxed);
            }
            halves::submitted_reads(queue, n - boundary, offset(lists, boundary));
            (r.submitted_reads)(queue, n - boundary, offset(lists, boundary));
        }
        (r.after)(queue, n, lists, boundary, handles[0], handles[1], geometry);
        halves::after(queue, n, lists);
        if !serialize {
            timing[5].fetch_add(ticks(), Relaxed);
        }
    }
    if submits_blit {
        (r.mirror)(queue);
    }
}

// ---- the C ABI
/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ecl_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
/// # Safety
/// The hooked call's arguments.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ecl_execute(queue: Queue, n: u32, lists: Lists, caller: *mut c_void) {
    execute(queue, n, lists, caller);
}
/// # Safety
/// A live queue.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ecl_patch(queue: Queue) {
    patch(queue);
}
/// # Safety
/// A live queue (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ecl_original(queue: Queue) -> usize {
    original_execute(queue).map_or(0, |f| f as *const () as usize)
}

#[cfg(test)]
mod tests {
    //! The table lookup: an unhooked or null queue has no original; a recorded table answers its original.
    use super::*;

    unsafe extern "system" fn native(_: Queue, _: u32, _: Lists) {}

    #[test]
    fn originals_by_table() {
        assert!(original_execute(core::ptr::null_mut()).is_none());
        let table = [0usize; 16];
        let mut queue = table.as_ptr() as usize;
        let q = (&mut queue as *mut usize).cast::<c_void>();
        assert!(original_execute(q).is_none());
        let n = COUNT.load(SeqCst) as usize;
        TABLES[n].store(table.as_ptr() as usize, SeqCst);
        ORIGINALS[n].store(native as *const () as usize, SeqCst);
        COUNT.store(n as u32 + 1, SeqCst);
        assert_eq!(original_execute(q).map(|f| f as *const () as usize), Some(native as *const () as usize));
        assert_eq!(unsafe { cvr_ecl_original(q) }, native as *const () as usize);
        // Unattached: the dispatch and the patch do nothing.
        execute(q, 0, core::ptr::null(), core::ptr::null_mut());
        patch(q);
    }
}
