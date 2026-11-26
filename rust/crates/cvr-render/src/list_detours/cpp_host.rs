//! The detours' calls into the subsystems they tell (formerly src/Stereo/ListDetourHost.cpp's cvr_lh_* switches): the
//! copy windows, the targets' handoff and access, the stereo families, single pass, the reads, the registered constant
//! copies, the bound targets, the barriers and the list's reset. Where the subsystem is Rust it is called directly (the
//! targets' handoff and access among them, its immediate writes too, and the stereo families -- families_native.rs, by its
//! C names); the C++ subsystems (the native probe and the registered constant uploads) are called by their MSVC names.

use super::{Barrier, List};
use crate::copy_windows as cw;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::Ordering::Relaxed;

unsafe extern "C" {
    // cvr-rig's (the menu surface and the image poses)
    fn cvr_menu_surface_barriers(a0: *mut c_void, a1: u32, a2: *const c_void);
    fn cvr_menu_surface_reset_list(a0: *mut c_void);
    fn cvr_image_pose_reset_list(a0: *mut c_void);

    // src/Render/NativeStereoFamilies.cpp
    #[link_name = "cvr_nsf2_pipeline_bound"]
    fn families_pipeline_bound(list: List, pso: *mut c_void);
    #[link_name = "cvr_nsf2_observe_draw"]
    fn families_observe_draw(list: List, node: u32, side: i32);
    #[link_name = "cvr_nsf2_reset"]
    fn families_reset(list: List, pso: *mut c_void);
    #[link_name = "cvr_nsf2_enabled"]
    fn families_enabled() -> bool;

}

/// copy_windows::Global: a list-wide event, while any window is open.
fn cw_global(which: u32, a: u64) {
    if cw::cvr_cw_active_windows.load(Relaxed) != 0 {
        cw::cvr_cw_global(which, a, 0);
    }
}
/// The copy windows: 0 an unknown draw (not inside a read-only scope), 1 an unknown command, 2 every window written,
/// 3 whether any window is open.
pub(crate) unsafe fn cvr_lh_copy_windows(op: u32, list: List) -> bool {
    match op {
        0 => cw::unknown_draw(list as u64),
        1 => cw_global(2, list as u64),
        2 => cw::each(|window| window.write(0)),
        _ => return cw::cvr_cw_active_windows.load(Relaxed) != 0,
    }
    true
}
/// The targets' handoff: 0 a draw, 1 an unknown command, 2 a written resource, 3 a cleared view, 4 the list closed.
pub(crate) unsafe fn cvr_lh_handoff(op: u32, list: List, value: u64, kind: u32) {
    match op {
        0 => crate::target_handoff_observer::draw(list),
        1 => crate::target_handoff_observer::unknown(list, kind),
        2 => crate::target_handoff_observer::written(list, value as *mut c_void, kind),
        3 => crate::target_handoff_observer::clear_view(list, value as usize, kind != 0),
        _ => crate::target_handoff_observer::close(list, kind as i32),
    }
}
/// The targets' access: 0 a shader's, 1 an unknown command, 2 a read, 3 a write, 4 a cleared view, 5 the list closed,
/// else the observer unavailable.
pub(crate) unsafe fn cvr_lh_access(op: u32, list: List, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
    match op {
        0 => crate::target_access_observer::events::shader(list, kind != 0),
        1 => crate::target_access_observer::events::unknown(list, kind),
        2 => crate::target_access_observer::events::read(list, resource, kind, subresource, offset, bytes),
        3 => crate::target_access_observer::events::written(list, resource, kind, subresource, offset, bytes),
        4 => crate::target_access_observer::events::clear_view(list, offset as usize, kind != 0),
        5 => crate::target_access_observer::events::close(list, kind as i32),
        _ => crate::target_access_observer::events::observer_unavailable(),
    }
}
/// WriteBufferImmediate told to the targets' access.
pub(crate) unsafe fn cvr_lh_access_immediate(list: List, count: u32, parameters: *const c_void, modes: *const c_void, caller: usize) -> bool {
    // SAFETY: the detour's list and arrays.
    unsafe { crate::target_access_observer::events::immediate_write(list, count, parameters, modes, caller) }
}
/// The stereo families: 0 a pipeline bound, 1 a draw observed, 2 a pipeline's native geometry status, 3 the list
/// reset, else whether they are on.
pub(crate) unsafe fn cvr_lh_families(op: u32, list: List, pso: *mut c_void, node: u32, side: i32) -> u32 {
    // SAFETY: the detour's list and pipeline.
    unsafe {
        match op {
            0 => families_pipeline_bound(list, pso),
            1 => families_observe_draw(list, node, side),
            2 => return crate::families_status::native_geometry_status(pso),
            3 => families_reset(list, pso),
            _ => return families_enabled() as u32,
        }
    }
    0
}
/// Single pass: 0 vertex buffers, 1 a pipeline set, 3 viewports, 4 scissors, else a draw (`pointer` its five
/// arguments).
pub(crate) unsafe fn cvr_lh_single_pass(op: u32, list: List, a: u64, b: u64, pointer: *const c_void) -> bool {
    use crate::single_pass as s;
    // SAFETY: the detour's list and arrays.
    unsafe {
        match op {
            0 => s::cvr_single_pass_vertex_buffers(list, a as u32, b as u32, pointer.cast()),
            1 => s::cvr_single_pass_pipeline_set(list, a as *mut c_void, b as *mut c_void),
            3 => s::cvr_single_pass_viewports(list, a as u32, pointer.cast()),
            4 => s::cvr_single_pass_scissors(list, a as u32, pointer.cast()),
            _ => s::cvr_single_pass_draw(list, a as u32, b as u32, pointer.cast()),
        }
    }
}
/// The reads' observers unavailable: the targets' access, the textures' and the buffers'.
pub(crate) unsafe fn cvr_lh_reads_unavailable() {
    crate::target_access_observer::events::observer_unavailable();
    crate::texture_receipts::cvr_tr_unavailable();
    crate::buffer_read::cvr_buffer_read_unavailable();
}
/// The registered constant copies: 0 a buffer region copied, 1 a resource copied, else a destination invalidated.
pub(crate) unsafe fn cvr_lh_constants(
    op: u32,
    list: List,
    destination: *mut c_void,
    destination_offset: u64,
    source: *mut c_void,
    source_offset: u64,
    bytes: u64,
) {
    // SAFETY: the detour's list and resources.
    unsafe {
        match op {
            0 => constants_observe_copy(list, destination, destination_offset, source, source_offset, bytes),
            1 => constants_observe_resource_copy(list, destination, source),
            _ => constants_invalidate(list, destination),
        }
    }
}
/// The bound targets: 0 the retained geometry's (nothing to do), 1 the native probe's, else the targets' handoff and
/// access.
pub(crate) unsafe fn cvr_lh_targets(op: u32, list: List, count: u32, handles: *const usize, contiguous: i32, depth: *const usize) {
    // SAFETY: the detour's list and descriptors.
    unsafe {
        match op {
            0 => {}
            // native_probe::Targets (stereo_probe_state.rs).
            1 => crate::stereo_probe_state::cvr_nsps_list(9, list, 0, count, handles.cast_mut().cast(), contiguous != 0, false, depth as u64),
            _ => {
                crate::target_handoff_observer::targets(list, count, handles, contiguous != 0, depth);
                crate::target_access_observer::events::targets(list, count, handles, contiguous != 0, depth);
            }
        }
    }
}
/// The barriers: 0 the menu surface's, 1 the retained geometry's, 2 the targets' handoff and access, 3 the reads'
/// (the copy windows, the buffers, the textures), 4 the optics probe's stage, else the registered constants'.
pub(crate) unsafe fn cvr_lh_barriers(op: u32, list: List, count: u32, barriers: *const Barrier) {
    // SAFETY: the detour's list and barriers.
    unsafe {
        match op {
            0 => cvr_menu_surface_barriers(list, count, barriers.cast()),
            1 => crate::retained_geometry_native::cvr_nrg2_before_barriers(list, count, barriers.cast()),
            2 => {
                crate::target_handoff_observer::barriers(list, count, barriers.cast());
                crate::target_access_observer::events::barriers(list, count, barriers.cast());
            }
            3 => {
                if cw::cvr_cw_active_windows.load(Relaxed) != 0 {
                    cw::cvr_cw_barriers(count, barriers.cast());
                }
                crate::buffer_read::cvr_buffer_read_barriers(list, count, barriers.cast());
                crate::texture_receipts::cvr_tr_barriers(count, barriers.cast());
            }
            4 => cvr_gpu::optics_probe::cvr_optics_probe_stage_barriers(list, count, barriers.cast()),
            _ => constants_barriers(list, count, barriers),
        }
    }
}
/// The list's reset told to each subsystem that keeps per-list state (`op` the detour's phase).
pub(crate) unsafe fn cvr_lh_list_reset(op: u32, list: List, allocator: *mut c_void, pso: *mut c_void, caller: usize, flag: bool) {
    // SAFETY: the detour's list, allocator and pipeline.
    unsafe {
        match op {
            0 => {
                cvr_gpu::gpu_diag::cvr_list_audit_list_reset(list, allocator);
                crate::retained_geometry_native::cvr_nrg2_reset(list);
                crate::target_access_observer::events::reset(list);
                crate::texture_receipts::cvr_tr_reset(list);
                crate::material_observe::cvr_mbind_reset(list);
                buffer_read_reset(list);
                constants_reset_registered(list);
            }
            1 => cvr_gpu::gpu_diag::cvr_list_audit_list_reset(list, allocator),
            2 => {
                crate::retained_geometry_native::cvr_nrg2_reset(list);
                crate::native_recording::cvr_native_recording_reset(list, caller);
                crate::target_access_observer::events::reset(list);
                crate::texture_receipts::cvr_tr_reset(list);
                buffer_read_reset(list);
                crate::material_observe::cvr_mbind_reset(list);
                crate::source_cbv_observe::cvr_scbo_reset(list as u64);
                crate::source_srv_observe::cvr_ssrvo_reset(list as u64);
                families_reset(list, pso);
                crate::constant_capture_state::cvr_ncc2_reset(list);
                constants_reset_registered(list);
                crate::preparation_trace::cvr_prep_reset_list(list as u64);
            }
            // native_probe::Reset (stereo_probe_state.rs), a new recording.
            3 => crate::stereo_probe_state::cvr_nsps_list(0, list, 0, 0, pso, flag, true, 0),
            4 => {
                crate::single_pass::cvr_single_pass_reset(list);
                crate::foveated_dlss::cvr_foveated_dlss_reset_list(list);
            }
            5 => {
                cvr_image_pose_reset_list(list);
                crate::framegen_inputs::cvr_framegen_reset_list(list);
                cvr_menu_surface_reset_list(list);
            }
            _ => {
                let t = &mut *crate::render_tls::cvr_render_tls();
                t.cur_rt_w = 0;
                t.cur_rt_h = 0;
                if t.final_list == list {
                    t.final_res = null_mut();
                    t.final_list = null_mut();
                }
            }
        }
    }
}
/// buffer_read::Reset: the copy windows' list reset, then the buffers'.
unsafe fn buffer_read_reset(list: List) {
    cw_global(3, list as u64);
    // SAFETY: the detour's list.
    unsafe { crate::buffer_read::cvr_buffer_read_reset(list) };
}

// constants::* (formerly src/Render/RegisteredConstantUpload.cpp's entry points): registered_copies.rs' entries.
unsafe fn constants_observe_copy(list: List, destination: *mut c_void, offset: u64, source: *mut c_void, source_offset: u64, bytes: u64) {
    crate::registered_copies::entries::cvr_rc2_observe_copy(list, destination, offset, source, source_offset, bytes);
}
unsafe fn constants_observe_resource_copy(list: List, destination: *mut c_void, source: *mut c_void) {
    crate::registered_copies::entries::cvr_rc2_observe_resource_copy(list, destination, source);
}
unsafe fn constants_invalidate(list: List, destination: *mut c_void) {
    crate::registered_copies::entries::cvr_rc2_invalidate(list, destination);
}
unsafe fn constants_barriers(list: List, count: u32, barriers: *const Barrier) {
    // SAFETY: the detour's barriers.
    unsafe { crate::registered_copies::entries::cvr_rc2_barriers(list, count, barriers.cast()) };
}
unsafe fn constants_reset_registered(list: List) {
    crate::registered_copies::entries::cvr_rc2_reset(list);
}
