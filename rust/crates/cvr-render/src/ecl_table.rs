//! The ExecuteCommandLists runtime (formerly src/Stereo/DeviceHooks.cpp's kEclRuntime): what queue_dispatch.rs's
//! dispatch calls around a submission -- the constants' queue API and the diagnostic submission count, the mirror's
//! pending blit, the image submission lock, the game queue's frame generation, the command resources and the scene
//! profile (Rust handles written into the dispatch's slots), the target handoff and the submission frontier, the
//! retained and paired geometry's serialization and boundary, the ordered execution and early depth, the geometry
//! snapshots, the registered queue order, the registered constants and the preparation lists, the camera optics'
//! execution, the consumers' submitted halves and the game queue's mirror copy.
//!
//! The Render subsystems still in C++ (target handoff, paired geometry, the retained geometry's boundary, the
//! registered constants, the frame timer) are called by their MSVC names; DeviceHooks.cpp keeps the hook's trampoline
//! (`_ReturnAddress`) and the geometry snapshots' C++ submission. The registered queue order's recursive lock is
//! registered_order below (formerly src/Render/ConstantQueueObserver.cpp's RegisteredQueueOrderMutex).

use crate::queue_dispatch::{Execute, Runtime};
use core::ffi::{c_char, c_void};
use core::sync::atomic::Ordering::*;
use std::sync::{LazyLock, Once};

type Queue = *mut c_void;
type Lists = *const *mut c_void;

#[allow(non_upper_case_globals)]
unsafe extern "C" {
    /// The hook's entry (below), the vtable slot's target.
    fn cvr_ecl_hook_entry();
    /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
    fn cvr_cpp_stereo_log(text: *const c_char);
    static CyberpunkVR_RuntimeDiagnostics: i32;

}

fn internal() -> bool {
    cvr_host::probe::internal()
}

/// The registered queue order (formerly a std::recursive_mutex of src/Render/ConstantQueueObserver.cpp): the dispatch
/// locks and unlocks it in pairs on one thread, and a submission inside a submission nests.
mod registered_order {
    use std::cell::{Cell, RefCell};
    use std::sync::{Mutex, MutexGuard};

    static ORDER: Mutex<()> = Mutex::new(());
    thread_local! {
        static DEPTH: Cell<u32> = const { Cell::new(0) };
        static HELD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
    }
    pub(super) fn lock() {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                let guard = ORDER.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                HELD.with(|held| *held.borrow_mut() = Some(guard));
            }
            depth.set(depth.get() + 1);
        });
    }
    pub(super) fn unlock() {
        DEPTH.with(|depth| match depth.get() {
            0 => {}
            1 => {
                depth.set(0);
                HELD.with(|held| drop(held.borrow_mut().take()));
            }
            n => depth.set(n - 1),
        });
    }
}
/// The registered queue order under its C name, for the Rust that names it so (queue_api_observer.rs,
/// prepared_pair_native.rs); formerly src/Stereo/DeviceHooks.cpp's.
#[no_mangle]
pub extern "C" fn cvr_cpp_ecl_registered_order(lock: bool) {
    if lock {
        registered_order::lock();
    } else {
        registered_order::unlock();
    }
}
fn game_queue() -> Queue {
    crate::device_creation::game_objects::CyberpunkVR_GetGameQueue()
}

extern "C" fn log(text: *const u8) {
    // SAFETY: a terminated line.
    unsafe { cvr_cpp_stereo_log(text.cast()) };
}
extern "C" fn read_unavailable() {
    // SAFETY: plain calls.
    unsafe { crate::list_detours::cvr_lh_reads_unavailable() };
}
extern "C" fn list_audit(queue: Queue, count: u32, _lists: Lists) {
    // SAFETY: the submitting queue.
    unsafe { crate::queue_api_observer::observe_queue_api(queue) };
    // SAFETY: the C++ half's switch; the diagnostic export.
    if unsafe { CyberpunkVR_RuntimeDiagnostics } != 0 {
        let total = crate::moved_globals::g_exec_total.fetch_add(u64::from(count), Relaxed) + u64::from(count);
        // SAFETY: a diagnostic counter.
        unsafe { core::ptr::write_volatile(&raw mut crate::declared_moved_exports::CyberpunkVR_DebugExecTotal, total) };
    }
}
extern "C" fn pending_blit() -> *mut c_void {
    crate::stereo_shared::g_mirror_pending_list.load(Acquire)
}
extern "C" fn image_lock(lock: bool) {
    if lock {
        cvr_host::submission::cvr_image_submission_lock();
    } else {
        cvr_host::submission::cvr_image_submission_unlock();
    }
}
unsafe extern "C" fn frame_metrics() -> bool {
    cvr_gpu::framegen_core::cvr_framegen_metrics_enabled()
}
unsafe extern "C" fn frame_sink(ms: f64) {
    cvr_gpu::framegen_core::cvr_framegen_on_gpu_frame(ms);
}
extern "C" fn before(queue: Queue, _count: u32, _lists: Lists) {
    if queue == crate::swapchain::game_present_queue() && !internal() {
        // framegen::BeforeGameCommands: the GPU frame timer (frame_timer.rs) with framegen's metrics switch and frame sink.
        // SAFETY: the game queue.
        unsafe {
            cvr_gpu::frame_timer::cvr_gpu_frame_timer_before(queue, frame_metrics, frame_sink);
        }
    }
}
extern "C" fn prepare(count: u32, lists: Lists, slots: *mut [*mut c_void; 2]) {
    // SAFETY: the dispatch's two slots, each room for a pointer; the submission's lists.
    unsafe {
        let resources = cvr_gpu::command_resources::cvr_gpu_prepare_command_resources(count, lists.cast());
        let profile = cvr_gpu::scene_profile::cvr_scene_profile_before_submit(count, lists);
        (*slots)[0].cast::<*mut c_void>().write(resources.cast());
        (*slots)[1].cast::<*mut c_void>().write(profile.cast());
    }
}
extern "C" fn before_reads(queue: Queue, count: u32, lists: Lists, _caller: *mut c_void) {
    // SAFETY: the submission's queue and lists.
    unsafe {
        crate::target_handoff_observer::before_submit(queue, count, lists);
        crate::submission_frontier_host::observe_submit(queue, count, lists);
    }
}
extern "C" fn serialize(which: u32, queue: Queue, count: u32, lists: Lists) -> u32 {
    // SAFETY: the submission's queue and lists.
    unsafe {
        match which {
            0 => u32::from(!internal() && crate::retained_geometry_native::cvr_nrg2_active()
                && crate::retained_geometry_native::cvr_nrg2_before_submit(queue, count, lists)),
            1 => u32::from(!internal() && paired_active() && paired_before_submit(queue, count, lists)),
            _ => crate::retained_geometry_native::current_boundary(queue, count, lists),
        }
    }
}
extern "C" fn ordered(queue: Queue, count: u32, lists: Lists, serialize: bool) {
    // SAFETY: the submission's queue and lists.
    unsafe { crate::retained_geometry_native::cvr_nrg2_ordered_execution(queue, count, lists, serialize) };
    crate::early_depth_native::cvr_ned_before_submit(queue, count, lists);
}
extern "C" fn geometry(count: u32, lists: Lists, slot: *mut c_void) {
    const _: () = assert!(
        core::mem::size_of::<crate::geometry_freeze::Submission>() <= crate::queue_dispatch::SLOT_BYTES
            && core::mem::align_of::<crate::geometry_freeze::Submission>() <= 16
    );
    // SAFETY: the dispatch's slot (256 bytes, 16-aligned) takes the freeze's submission; the submission's lists.
    unsafe { slot.cast::<crate::geometry_freeze::Submission>().write(crate::geometry_freeze::before_submit(count, lists)) };
}
extern "C" fn order(which: u32) -> bool {
    match which {
        // SAFETY: a plain C++ query; the export.
        0 => (unsafe { registered_tracking() }) || crate::native_state_exports::CyberpunkVR_PreparationAllStages.load(Relaxed) != 0,
        1 | 2 => {
            if which == 1 {
                registered_order::lock();
            } else {
                registered_order::unlock();
            }
            true
        }
        _ => {
            // SAFETY: the registered copies' fences.
            unsafe { crate::registered_copies::cvr_rc_refresh_fences() };
            true
        }
    }
}
extern "C" fn submit_prefix(queue: Queue, count: u32, lists: Lists, owns: bool) {
    // SAFETY: the submission's queue and lists.
    unsafe {
        submit_registered(queue, count, lists);
        crate::constant_capture_state::cvr_ncc2_before_submit(count, lists);
        crate::preparation_trace::cvr_prep_submit_lists(queue as u64, count, lists.cast(), owns);
    }
}
extern "C" fn camera_execute(queue: Queue, count: u32, lists: Lists, original: Execute) {
    // SAFETY: the submission's queue and lists; the engine's own ExecuteCommandLists (the same signature).
    unsafe {
        let execute: cvr_gpu::optics_source::ExecuteFn = core::mem::transmute(original);
        cvr_gpu::optics_camera::cvr_optics_camera_execute(queue, count, lists, execute);
    }
}
extern "C" fn submitted_reads(queue: Queue, count: u32, lists: Lists) {
    // SAFETY: the submission's queue and lists.
    unsafe { crate::target_handoff_observer::submitted(queue, count, lists) };
}
extern "C" fn retained_prefix(queue: Queue, boundary: u32, lists: Lists) {
    crate::retained_geometry_native::cvr_nrg2_prefix_executed(boundary);
    // SAFETY: the submission's queue and its prefix.
    unsafe { crate::retained_geometry_native::cvr_nrg2_submitted(queue, boundary, lists) };
    crate::retained_geometry_native::cvr_nrg2_submit_current(queue);
}
extern "C" fn after(queue: Queue, count: u32, lists: Lists, boundary: u32, resources: *mut c_void, profile: *mut c_void, geometry: *mut c_void) {
    if boundary != 0 {
        crate::retained_geometry_native::cvr_nrg2_suffix_executed(count - boundary);
    }
    // SAFETY: the submission's queue and lists; the dispatch's slots, filled by prepare and geometry.
    unsafe {
        if !internal() && crate::retained_geometry_native::cvr_nrg2_active() {
            crate::retained_geometry_native::cvr_nrg2_submitted(queue, count, lists);
            crate::retained_geometry_native::cvr_nrg2_submit_current(queue);
        }
        if !internal() && paired_active() {
            paired_submitted(queue, count, lists);
        }
        if !lists.is_null() {
            for i in 0..count as usize {
                crate::material_observe::cvr_mbind_reset(*lists.add(i));
            }
        }
        let held_resources = resources.cast::<*mut c_void>().replace(core::ptr::null_mut());
        cvr_gpu::command_resources::cvr_gpu_submit_command_resources(queue, held_resources.cast());
        crate::geometry_freeze::submitted(queue, geometry.cast::<crate::geometry_freeze::Submission>().read());
        let held_profile = profile.cast::<*mut c_void>().replace(core::ptr::null_mut());
        cvr_gpu::scene_profile::cvr_scene_profile_submitted(queue, held_profile.cast());
        crate::gpu_probe_runtime::cvr_sgpw_submit(true, queue, count, lists);
    }
    crate::early_depth_native::cvr_ned_submitted(queue, count, lists);
    // SAFETY: the submission's queue and lists.
    unsafe { cvr_image_pose_commit(queue, count, lists.cast_mut().cast()) };
}
unsafe extern "C" {
    /// camera::CommitImagePoseLists (cvr-rig's image_pose.rs).
    fn cvr_image_pose_commit(queue: *mut c_void, count: u32, lists: *mut c_void);
}
extern "C" fn mirror(queue: Queue) {
    if queue == game_queue() {
        crate::stereo_shared::g_mirror_pending_list.store(core::ptr::null_mut(), Release);
        // SAFETY: the game queue.
        unsafe { crate::mirror::cvr_mirror_submit_copy(queue) };
    }
}

// ExecuteCommandLists' hook entry: the caller's return address handed to the dispatch as its fourth argument. A leaf
// jump with no stack of its own, so it needs no unwind data.
core::arch::global_asm!(
    ".globl cvr_ecl_hook_entry",
    ".def cvr_ecl_hook_entry",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_ecl_hook_entry:",
    "    mov r9, [rsp]",
    "    jmp {target}",
    target = sym crate::queue_dispatch::cvr_ecl_execute,
);
/// The runtime, built once (its pointers name process-lifetime objects).
struct Rt(Runtime);
// SAFETY: immutable once built.
unsafe impl Send for Rt {}
// SAFETY: as above.
unsafe impl Sync for Rt {}
static RT: LazyLock<Rt> = LazyLock::new(|| Rt(Runtime {
    timing: &raw const crate::used_moved_exports::CyberpunkVR_EclTiming,
    hook: cvr_ecl_hook_entry as *const () as usize,
    log,
    read_unavailable,
    list_audit,
    pending_blit,
    image_lock,
    before,
    prepare,
    before_reads,
    serialize,
    ordered,
    geometry,
    order,
    submit_prefix,
    camera_execute,
    submitted_reads,
    retained_prefix,
    after,
    mirror,
}));
/// The runtime handed to the dispatch, once (before a queue's ExecuteCommandLists is patched).
pub fn attach() {
    static ONCE: Once = Once::new();
    // SAFETY: a runtime alive for the process.
    ONCE.call_once(|| unsafe { crate::queue_dispatch::cvr_ecl_attach(&RT.0) });
}

// constants::* (formerly src/Render/RegisteredConstantUpload.cpp's entry points): registered_copies.rs' entries.
unsafe fn registered_tracking() -> bool {
    crate::registered_copies::entries::cvr_rc2_tracking()
}
unsafe fn submit_registered(queue: Queue, count: u32, lists: Lists) {
    // SAFETY: the submission's queue and lists.
    unsafe { crate::registered_copies::entries::cvr_rc2_submit(queue, count, lists) };
}

// paired_geometry::* (formerly src/Render/NativePairedGeometry.cpp): paired_geometry_native.rs.
unsafe fn paired_active() -> bool {
    crate::paired_geometry_native::active()
}
unsafe fn paired_before_submit(queue: Queue, count: u32, lists: Lists) -> bool {
    // SAFETY: the submission's queue and lists.
    unsafe { crate::paired_geometry_native::before_submit(queue, count, lists) }
}
unsafe fn paired_submitted(queue: Queue, count: u32, lists: Lists) {
    // SAFETY: as above.
    unsafe { crate::paired_geometry_native::submitted(queue, count, lists) };
}
