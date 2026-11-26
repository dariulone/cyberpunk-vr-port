//! The render target, barrier, viewport, scissor and reset detours (formerly src/Stereo/Capture.cpp's
//! hk_OMSetRenderTargets and hk_ResourceBarrier, src/Stereo/Dlss.cpp's hk_RSSetViewports, hk_RSSetScissorRects and
//! hk_GfxReset): the bound targets told to VRS, the optics, the menu surface, the camera's image poses, the cascades
//! and the mirror capture (which may substitute the eye's targets); the barriers told to early depth, the cascades,
//! the hidden area, target handoff and access, the reads and the capture; the list's reset told to every subsystem
//! that keeps per-list state.

use super::*;
use windows::Win32::Graphics::Direct3D12::{D3D12_VIEWPORT, D3D12_COMMAND_LIST_TYPE_COPY};

extern "C" {
    fn cvr_capture_rtv_resource_lookup(handle: usize) -> *mut c_void;
    fn cvr_capture_om_observe(count: u32, handles: *const usize, depth: *const usize);
    fn cvr_capture_om_substitute(count: u32, handles: *const usize, contiguous: i32, sub: *mut usize) -> bool;
    fn cvr_capture_om_after(list: List, count: u32, handles: *const usize, contiguous: i32, subbed: bool);
    fn cvr_capture_count_barriers(count: u32, barriers: *const Barrier);
    fn cvr_capture_barrier_clock() -> u64;
    fn cvr_capture_count_barrier_time(start: u64);
    fn cvr_capture_barriers(list: List, count: u32, barriers: *const Barrier, hooked: bool);
    fn cvr_vrs_targets(list: List, count: u32, depth: bool);
    fn cvr_vrs_viewports(list: List, count: u32, viewports: *const D3D12_VIEWPORT);
    fn cvr_optics_targets(list: List, count: u32, handles: *const usize, contiguous: i32, depth: *const usize);
    fn cvr_optics_camera_target_bound(list: List, resource: *mut c_void);
    fn cvr_optics_camera_barriers(list: List, count: u32, barriers: *const Barrier);
    fn cvr_optics_camera_list_reset(list: List);
    fn cvr_mono_cascades_targets(list: List, colors: u32, dsv: *const usize);
    fn cvr_mono_cascades_before_barriers(list: List, count: u32, barriers: *const Barrier);
    fn cvr_mono_cascades_barriers(count: u32, barriers: *const Barrier);
    fn cvr_mono_cascades_reset(list: List);
    fn cvr_scene_profile_viewport(list: List, count: u32, viewports: *const D3D12_VIEWPORT);
    fn cvr_scene_profile_reset(list: List);
    fn cvr_pair_probe_viewports(list: List, count: u32, viewports: *const D3D12_VIEWPORT);
    fn cvr_pair_probe_reset(list: List);
    fn cvr_gpu_reset_command_resources(list: List);
}

/// The handle of the bound target `i` (a contiguous range steps by the RTV descriptor size).
///
/// # Safety
/// `handles` holds the call's handles.
unsafe fn target_handle(handles: *const usize, contiguous: i32, step: u64, i: u32) -> usize {
    // SAFETY: the caller's handles.
    unsafe {
        if contiguous != 0 {
            (*handles).wrapping_add((step * i as u64) as usize)
        } else {
            *handles.add(i as usize)
        }
    }
}
/// hk_OMSetRenderTargets.
pub unsafe extern "system" fn set_targets(list: List, count: u32, handles: *const usize, contiguous: i32, depth: *const usize) {
    let Some(original) = entry(list).and_then(|e| original!(e.head[head::OM], fn(List, u32, *const usize, i32, *const usize))) else { return };
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if internal() {
            original(list, count, handles, contiguous, depth);
            cvr_lh_targets(2, list, count, handles, contiguous, depth);
            return;
        }
        if retained_active() {
            cvr_lh_targets(0, list, count, handles, contiguous, depth);
        }
        cvr_vrs_targets(list, if handles.is_null() { 0 } else { count }, !depth.is_null() && *depth != 0);
        cvr_optics_targets(list, count, handles, contiguous, depth);
        if !handles.is_null() && cvr_lh_query(0, 0, 0) != 0 {
            let step = cvr_lh_query(1, 0, 0);
            for i in 0..count {
                menu_surface_written(list, cvr_capture_rtv_resource_lookup(target_handle(handles, contiguous, step, i)));
            }
        }
        if !handles.is_null() && cvr_optics_camera_node_active() {
            let step = cvr_lh_query(1, 0, 0);
            for i in 0..count {
                cvr_optics_camera_target_bound(list, cvr_capture_rtv_resource_lookup(target_handle(handles, contiguous, step, i)));
            }
        }
        cvr_mono_cascades_targets(list, count, depth);
        if probe_state_on() {
            cvr_lh_targets(1, list, count, handles, contiguous, depth);
        }
        let ctx = Context::read();
        if count == 1 && !handles.is_null() && ctx.active_view_known && (ctx.active_view_key == 0 || ctx.active_view_key == cvr_lh_query(2, 0, 0)) {
            let target = cvr_capture_rtv_resource_lookup(*handles);
            if !target.is_null() {
                cvr_lh_target_bound(0, list, target);
            }
        }
        cvr_capture_om_observe(count, handles, depth);
        if !handles.is_null()
            && count != 0
            && ctx.active_view_known
            && (ctx.active_view_key != 0 || ctx.side == 0)
            && cvr_lh_query(4, ctx.active_view_key, cvr_lh_query(2, 0, 0)) != 0
            && ctx.work == ctx.base.wrapping_add(0x41_6600)
            && cvr_lh_query(3, 0, 0) != 0
        {
            cvr_lh_target_bound(1, list, cvr_capture_rtv_resource_lookup(*handles));
        }
        let mut sub = [0usize; 8];
        let subbed = cvr_capture_om_substitute(count, handles, contiguous, sub.as_mut_ptr());
        let (bound, bound_contiguous) = if subbed { (sub.as_ptr(), 0) } else { (handles, contiguous) };
        original(list, count, bound, bound_contiguous, depth);
        cvr_lh_targets(2, list, count, bound, bound_contiguous, depth);
        cvr_capture_om_after(list, count, handles, contiguous, subbed);
    }
}
const BARRIER_TRANSITION: u32 = 0;
const BARRIER_ALIASING: u32 = 1;
/// D3D12_RESOURCE_STATE_COPY_DEST | UNORDERED_ACCESS | STREAM_OUT.
const WRITE_STATES: u32 = 0x400 | 0x8 | 0x100;
/// hk_ResourceBarrier.
pub unsafe extern "system" fn resource_barrier(list: List, count: u32, barriers: *const Barrier) {
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if !barriers.is_null() && !internal() {
            cvr_capture_count_barriers(count, barriers);
        }
        cvr_lh_barriers(0, list, count, barriers);
        let e = entry(list);
        let original = e.and_then(|e| original!(e.head[head::BARRIER_ORIGINAL], fn(List, u32, *const Barrier)));
        if !internal() && retained_active() {
            cvr_lh_barriers(1, list, count, barriers);
        }
        if !barriers.is_null() && !internal() && cvr_lh_early_depth(4, null_mut(), null_mut(), 0) {
            for i in 0..count as usize {
                let b = &*barriers.add(i);
                if b.kind == BARRIER_ALIASING {
                    // Aliasing: the resource before at 8, the one after at 16.
                    let after = *(b as *const Barrier as *const u8).add(16).cast::<*mut c_void>();
                    cvr_lh_early_depth(5, list, null_mut(), 0);
                    cvr_lh_early_alias(list, b.resource, after);
                } else if b.kind == BARRIER_TRANSITION {
                    cvr_lh_early_depth(5, list, b.resource, 0);
                    if b.after & WRITE_STATES != 0 {
                        early_instance_write(list, b.resource);
                    }
                }
            }
        }
        cvr_mono_cascades_before_barriers(list, count, barriers);
        let ctx = Context::read();
        let node = ctx.node();
        let clear = e.map_or(0, |e| e.head[head::CLEAR_DSV]);
        if clear != 0 {
            cvr_lh_hidden_plain_barriers(false, list, count, barriers, ctx.side, node, clear);
        }
        if let Some(original) = original {
            let start = cvr_capture_barrier_clock();
            original(list, count, barriers);
            if !internal() {
                cvr_capture_count_barrier_time(start);
            }
            if clear != 0 {
                cvr_lh_hidden_plain_barriers(true, list, count, barriers, ctx.side, node, clear);
            }
            cvr_lh_barriers(2, list, count, barriers);
        }
        cvr_lh_barriers(3, list, count, barriers);
        cvr_lh_barriers(4, list, count, barriers);
        if internal() {
            return;
        }
        cvr_optics_camera_barriers(list, count, barriers);
        cvr_mono_cascades_barriers(count, barriers);
        cvr_lh_barriers(5, list, count, barriers);
        cvr_capture_barriers(list, count, barriers, e.is_some());
    }
}
/// hk_RSSetViewports.
pub unsafe extern "system" fn viewports(list: List, count: u32, viewports: *const D3D12_VIEWPORT) {
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if let Some(original) = entry(list).and_then(|e| original!(e.head[head::VIEWPORTS], fn(List, u32, *const D3D12_VIEWPORT))) {
            if !cvr_lh_single_pass(3, list, count as u64, 0, viewports.cast()) {
                original(list, count, viewports);
            }
        }
        cvr_scene_profile_viewport(list, count, viewports);
        draws::CyberpunkVR_GeometryCostTrackViewport(list, count, viewports);
        cvr_pair_probe_viewports(list, count, viewports);
        cvr_vrs_viewports(list, count, viewports);
        if probe_state_on() {
            cvr_lh_probe(9, list, count as u64, viewports as u64, 0);
        }
    }
}
/// hk_RSSetScissorRects.
pub unsafe extern "system" fn scissors(list: List, count: u32, rects: *const [i32; 4]) {
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if let Some(original) = entry(list).and_then(|e| original!(e.head[head::SCISSOR], fn(List, u32, *const [i32; 4]))) {
            if !cvr_lh_single_pass(4, list, count as u64, 0, rects.cast()) {
                original(list, count, rects);
            }
        }
        draws::CyberpunkVR_GeometryCostTrackScissor(list, count, rects);
        if probe_state_on() {
            cvr_lh_probe(10, list, count as u64, rects as u64, 0);
        }
    }
}
/// hk_GfxReset.
pub unsafe extern "system" fn reset(list: List, allocator: *mut c_void, pso: *mut c_void) -> i32 {
    const E_FAIL: i32 = 0x8000_4005_u32 as i32;
    let caller = return_address();
    // SAFETY: the engine's live list.
    let copy = unsafe { windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList::from_raw_borrowed(&list) }
        .is_some_and(|l| unsafe { l.GetType() } == D3D12_COMMAND_LIST_TYPE_COPY);
    // SAFETY: the engine's arguments throughout.
    unsafe {
        if copy {
            let Some(original) = entry(list).and_then(|e| original!(e.head[head::RESET], fn(List, *mut c_void, *mut c_void) -> i32)) else { return E_FAIL };
            let result = original(list, allocator, pso);
            if result >= 0 {
                cvr_lh_list_reset(0, list, allocator, pso, caller, false);
                cvr_gpu_reset_command_resources(list);
            }
            return result;
        }
        cvr_lh_list_reset(6, list, allocator, pso, caller, false);
        cvr_ld_vision_swap_reset();
        let e = entry(list);
        let original = e.and_then(|e| original!(e.head[head::RESET], fn(List, *mut c_void, *mut c_void) -> i32));
        let result = original.map_or(0, |f| f(list, allocator, pso));
        if let (Some(e), true) = (e, original.is_some() && result >= 0) {
            cvr_lh_list_reset(1, list, allocator, pso, caller, false);
            pipeline_reset(pso);
            cvr_lh_list_reset(2, list, allocator, pso, caller, false);
            if probe_state_on() {
                cvr_lh_list_reset(3, list, allocator, pso, caller, e.shading_state_known);
            }
            cvr_gpu_reset_command_resources(list);
            cvr_mono_cascades_reset(list);
            cvr_scene_profile_reset(list);
            cvr_pair_probe_reset(list);
            cvr_lh_list_reset(4, list, allocator, pso, caller, false);
            cvr_vrs_reset(list, e.head[head::SHADING_RATE] != 0, pso);
            cvr_optics_list_reset(list);
            cvr_optics_source_list_reset(list);
            cvr_optics_camera_list_reset(list);
            cvr_lh_list_reset(5, list, allocator, pso, caller, false);
        }
        result
    }
}
