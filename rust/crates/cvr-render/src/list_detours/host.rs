//! The detours' host services that only reached the Rust halves through the C++ (formerly
//! src/Stereo/ListDetourHost.cpp's cvr_lh_* functions, whose cases were one-statement forwards into Rust exports): the
//! render thread's context, the retained geometry's queries, the list's housekeeping, the hidden area's clears and
//! barriers, the queries and target notes, and the constant-buffer copies the HUD and the luma probe watch.
use core::ffi::{c_char, c_void};
use core::sync::atomic::Ordering::*;
use windows::core::Interface;

use super::{Barrier, Context, List};
use crate::render_tls;
use crate::stereo_shared as s;

extern "C" {
    fn cvr_cpp_stereo_log(text: *const c_char);
    fn cvr_nrg2_active() -> bool;
    fn cvr_nrg2_flush(list: List, reason: u32);
    fn cvr_nrg2_before_resources(list: List, source: *mut c_void, destination: *mut c_void, reason: u32);
    fn cvr_nrg2_render_pass(list: List, begin: bool);
    fn cvr_nrg2_indirect(list: List, signature: *mut c_void, count: u32, arguments: *mut c_void, offset: u64, counter: *mut c_void,
                         counter_offset: u64) -> bool;
    fn cvr_prep_draw_recorded(list: u64);
    fn cvr_framegen_capture_native_inputs(list: List);
    fn cvr_list_audit_list_close(list: List);
    fn cvr_mbind_reset(list: List);
    fn cvr_menu_surface_written(list: List, resource: *mut c_void);
    fn cvr_menu_surface_waiting() -> bool;
    fn cvr_sao_operand(list: List, resource: *mut c_void, write: bool, tag: u32, subresource: u32, offset: u64, bytes: u64);
    fn cvr_hidden_area_after_depth_clear(list: List, dsv: usize, flags: i32, depth: f32, rects: u32, side: i32, node: u32, clear: usize);
    fn cvr_hidden_area_before_barriers(list: List, count: u32, barriers: *const Barrier, side: i32, node: u32, clear: usize);
    fn cvr_hidden_area_after_barriers(list: List, count: u32, barriers: *const Barrier, side: i32, node: u32, clear: usize);
    fn cvr_hidden_area_before_enhanced_barriers(list: List, count: u32, groups: *const c_void, side: i32, node: u32, clear: usize);
    fn cvr_hidden_area_after_enhanced_barriers(list: List, count: u32, groups: *const c_void, side: i32, node: u32, clear: usize);
    fn cvr_ned_instance_alias(list: List, before: *mut c_void, after: *mut c_void);
    fn cvr_optics_probe_stage_requested() -> bool;
    fn cvr_optics_probe_stage_view(key: u64, eye_key: u64) -> bool;
    fn cvr_optics_probe_stage_target(list: List, resource: *mut c_void);
    fn cvr_pose_node_current(out: *mut c_void);
    fn cvr_image_pose_record(list: List, resource: *mut c_void, pose: *const c_void, stable: bool);
    fn cvr_upload_map_read(res: *mut c_void) -> *const u8;
    fn cvr_hud_block_plausible(p: *const u8, w: f32, h: f32) -> bool;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugCbCaptures: u64;
    static mut CyberpunkVR_DebugFrameCb: u64;
    static mut CyberpunkVR_DebugHudCb: u64;
}

fn tls() -> &'static mut render_tls::RenderTls {
    // SAFETY: this thread's block.
    unsafe { &mut *render_tls::cvr_render_tls() }
}

/// The render thread's context the detours classify by.
pub(crate) unsafe fn cvr_lh_context(out: *mut Context) {
    let t = tls();
    // SAFETY: the caller's context; g_exe_base is set once at install.
    unsafe {
        *out = Context {
            base: s::g_exe_base as usize,
            work: t.current_node_work,
            active_view_key: t.active_view_key,
            side: t.view_side,
            cascade: t.cascade_idx,
            vrcam: t.vrcam_node_active,
            active_view_known: t.active_view_known,
        }
    };
}
pub(crate) unsafe fn cvr_lh_last_dispatch(x: u32, y: u32, z: u32) {
    tls().last_disp = [x, y, z];
}
pub(crate) unsafe fn cvr_lh_log(text: *const c_char) {
    // SAFETY: a NUL-terminated line from the caller.
    unsafe { cvr_cpp_stereo_log(text) };
}

/// retained_geometry: Active, Flush, BeforeResources, BeforeClose (Flush with reason 0) and RenderPass.
pub(crate) unsafe fn cvr_lh_retained(op: u32, list: List, source: *mut c_void, destination: *mut c_void, reason: u32) -> bool {
    // SAFETY: the Rust halves' exports with the caller's arguments.
    unsafe {
        match op {
            0 => return cvr_nrg2_active(),
            1 => cvr_nrg2_flush(list, reason),
            2 => cvr_nrg2_before_resources(list, source, destination, reason),
            3 => cvr_nrg2_flush(list, 0),
            _ => cvr_nrg2_render_pass(list, reason != 0),
        }
    }
    true
}
pub(crate) unsafe fn cvr_lh_retained_indirect(list: List, signature: *mut c_void, count: u32, arguments: *mut c_void, offset: u64,
                                              counter: *mut c_void, counter_offset: u64) -> bool {
    // SAFETY: as above.
    unsafe { cvr_nrg2_indirect(list, signature, count, arguments, offset, counter, counter_offset) }
}

/// The list's notes: the preparation's recorded draw, framegen's native inputs, the audit's close, the material
/// binding's reset and the menu surface's write.
pub(crate) unsafe fn cvr_lh_list(op: u32, list: List, resource: *mut c_void) {
    // SAFETY: the Rust halves' exports with the caller's arguments.
    unsafe {
        match op {
            0 => cvr_prep_draw_recorded(list as u64),
            1 => cvr_framegen_capture_native_inputs(list),
            2 => cvr_list_audit_list_close(list),
            3 => cvr_mbind_reset(list),
            _ => cvr_menu_surface_written(list, resource),
        }
    }
}
pub(crate) unsafe fn cvr_lh_operand(list: List, resource: *mut c_void, write: bool, tag: u32, subresource: u32, offset: u64, bytes: u64) {
    // SAFETY: as above.
    unsafe { cvr_sao_operand(list, resource, write, tag, subresource, offset, bytes) };
}

/// The hidden area: the depth clear, and the enhanced and plain barriers before or after their original.
pub(crate) unsafe fn cvr_lh_hidden_clear(list: List, dsv: usize, flags: u32, depth: f32, rects: u32, side: i32, node: u32, clear: usize) {
    // SAFETY: as above; `clear` is the original ClearDepthStencilView or 0.
    unsafe { cvr_hidden_area_after_depth_clear(list, dsv, flags as i32, depth, rects, side, node, clear) };
}
pub(crate) unsafe fn cvr_lh_hidden_barriers(after: bool, list: List, count: u32, groups: *const c_void, side: i32, node: u32, clear: usize) {
    // SAFETY: as above.
    unsafe {
        if after {
            cvr_hidden_area_after_enhanced_barriers(list, count, groups, side, node, clear)
        } else {
            cvr_hidden_area_before_enhanced_barriers(list, count, groups, side, node, clear)
        }
    }
}
pub(crate) unsafe fn cvr_lh_hidden_plain_barriers(after: bool, list: List, count: u32, barriers: *const Barrier, side: i32, node: u32,
                                                  clear: usize) {
    // SAFETY: as above.
    unsafe {
        if after {
            cvr_hidden_area_after_barriers(list, count, barriers, side, node, clear)
        } else {
            cvr_hidden_area_before_barriers(list, count, barriers, side, node, clear)
        }
    }
}
pub(crate) unsafe fn cvr_lh_early_alias(list: List, before: *mut c_void, after: *mut c_void) {
    // SAFETY: as above.
    unsafe { cvr_ned_instance_alias(list, before, after) };
}

/// The queries: the menu surface waiting, the RTV descriptor size, the second view's key, the optics probe's stage.
pub(crate) unsafe fn cvr_lh_query(op: u32, a: u64, b: u64) -> u64 {
    // SAFETY: the Rust halves' exports; the game device is the plugin's, set at its creation.
    unsafe {
        match op {
            0 => cvr_menu_surface_waiting() as u64,
            1 => {
                let device = s::g_game_device;
                windows::Win32::Graphics::Direct3D12::ID3D12Device::from_raw_borrowed(&device).map_or(0, |d| {
                    d.GetDescriptorHandleIncrementSize(windows::Win32::Graphics::Direct3D12::D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as u64
                })
            }
            2 => crate::moved_globals::g_vrcam_ctx_key.load(Relaxed),
            3 => cvr_optics_probe_stage_requested() as u64,
            _ => cvr_optics_probe_stage_view(a, b) as u64,
        }
    }
}
/// A bound target: the optics probe's stage target, or the image pose of the node being drawn.
pub(crate) unsafe fn cvr_lh_target_bound(op: u32, list: List, target: *mut c_void) {
    // SAFETY: the Rust halves' exports with the caller's arguments.
    unsafe {
        if op != 0 {
            cvr_optics_probe_stage_target(list, target);
        } else {
            // cvr::camera::PoseIdentity: 136 bytes (cvr-rig's pose_identity.rs).
            let mut pose = [0u64; 17];
            cvr_pose_node_current(pose.as_mut_ptr().cast());
            cvr_image_pose_record(list, target, pose.as_ptr().cast(), false);
        }
    }
}
/// The resource's width, 0 for null or a fault.
pub(crate) unsafe fn cvr_lh_resource_width(resource: *mut c_void) -> u64 {
    let mut desc = Default::default();
    if !resource.is_null() && s::get_desc(resource, &mut desc) {
        desc.Width
    } else {
        0
    }
}

/// A CopyBufferRegion's destination watched for three constant buffers: the second view's 848-byte grading block
/// (inside its 2-RT pass, once armed), the engine's 480-byte per-frame constants (b0 of the composite) and the HUD
/// composite's own constants (b6), told apart by their content against the HUD snapshot's size.
pub(crate) unsafe fn cvr_lh_constant_buffers(dst: *mut c_void, dst_off: u64, src: *mut c_void, src_off: u64, num_bytes: u64) {
    use windows::core::IUnknown;
    let hold = |object: *mut c_void| {
        // SAFETY: a live resource from the copy being recorded.
        if let Some(u) = unsafe { IUnknown::from_raw_borrowed(&object) } {
            core::mem::forget(u.clone());
        }
    };
    let release = |object: *mut c_void| {
        if !object.is_null() {
            // SAFETY: the reference taken when it was stored.
            drop(unsafe { IUnknown::from_raw(object) });
        }
    };
    let t = tls();
    if t.in_vrcam_2rt && t.cb_armed_2rt && !dst.is_null() && num_bytes == 848 {
        t.cb_armed_2rt = false;
        let prev = s::g_cb_res.swap(dst, AcqRel);
        if prev != dst {
            hold(dst);
            release(prev);
        }
        crate::moved_globals::g_cb_off.store(dst_off, Release);
        // SAFETY: exports of the plugin.
        if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0 {
            // SAFETY: a counter the diagnostics own.
            unsafe { (*(&raw mut CyberpunkVR_DebugCbCaptures).cast::<core::sync::atomic::AtomicU64>()).fetch_add(1, SeqCst) };
        }
    }
    if !dst.is_null() && num_bytes == 480 && dst_off == 0 {
        let prev = s::g_frame_cb.load(Acquire);
        if prev != dst {
            hold(dst);
            s::g_frame_cb.store(dst, Release);
            release(prev);
            // SAFETY: an export of the plugin.
            unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_DebugFrameCb, dst as u64) };
        }
    }
    if !dst.is_null() && !src.is_null() && (272..=4096).contains(&num_bytes) && !crate::moved_globals::g_hud_cb_from_ring.load(Acquire) {
        let (w, h) = {
            let _held = s::g_hud_snap_mtx.lock();
            // SAFETY: under its lock.
            unsafe { (s::g_hud_snap_desc.Width, s::g_hud_snap_desc.Height as u64) }
        };
        // hud_cb_content_matches: a plausible block whose fourth row carries the curvature (the map is looked up
        // only for a known HUD size, as the C++ did).
        // SAFETY: the upload heap's persistent map or null.
        let base = if w != 0 && h != 0 { unsafe { cvr_upload_map_read(src) } } else { core::ptr::null() };
        let curve = if !base.is_null() {
            let at = base as usize + src_off as usize;
            // SAFETY: the block's bytes are checked before they are read; the curvature under SEH.
            (unsafe { cvr_hud_block_plausible(at as *const u8, w as f32, h as f32) })
                .then(|| cvr_engine::code::read::<[f32; 2]>(at + 3 * 16))
                .flatten()
        } else {
            None
        };
        if let Some(curve) = curve {
            let prev = s::g_hud_cb.load(Acquire);
            if prev != dst {
                hold(dst);
                s::g_hud_cb.store(dst, Release);
                release(prev);
                // SAFETY: an export of the plugin.
                unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_DebugHudCb, dst as u64) };
                let line = format!(
                    "[hud] composite constants captured: dst={:016X} bytes={} target={}x{} curvature=({:.6}, {:.6})\0",
                    dst as usize, num_bytes, w, h, curve[0], curve[1]
                );
                // SAFETY: a NUL-terminated line.
                unsafe { cvr_cpp_stereo_log(line.as_ptr().cast()) };
            }
        }
    }
}
