//! The observing detours (formerly src/Stereo/DeviceHooks.cpp's): the native probe's binding hooks (heaps, topology,
//! stencil, blend factor, graphics root signature, tables and CBVs, index buffer, predication, shading rate and
//! image), the list's state reset and bundles, its close, the discards, cascade copies, query resolves, immediate
//! writes, enhanced barriers, stream output, tile and atomic copies, render target and UAV clears, resolves, render
//! passes, meta commands, ray tracing and mesh dispatches -- each telling target handoff and access, the reads, the
//! retained geometry, VRS and the cascades what the engine's command does to their state.

use super::*;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// The node an observer reports with (0 outside a node).
fn node() -> (u32, i32) {
    let ctx = Context::read();
    (ctx.node(), ctx.side)
}

pub unsafe extern "system" fn heaps(list: List, count: u32, heaps: *const *mut c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::HEAPS], fn(List, u32, *const *mut c_void)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, count, heaps);
        if recording() {
            cvr_scene_heaps(list as usize, count, heaps.cast());
        }
        cvr_lh_probe(2, list, count as u64, heaps as u64, 0);
        cvr_lh_heaps_bound(list, count, heaps);
    }
}
pub unsafe extern "system" fn topology(list: List, value: i32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::TOPOLOGY], fn(List, i32)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, value);
        if recording() {
            cvr_scene_topology(list as usize, value);
        }
    }
}
pub unsafe extern "system" fn stencil(list: List, value: u32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::STENCIL], fn(List, u32)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, value);
        if recording() {
            cvr_scene_stencil(list as usize, value);
        }
    }
}
pub unsafe extern "system" fn blend_factor(list: List, factors: *const f32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::BLEND_FACTOR], fn(List, *const f32)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, factors);
        if recording() {
            cvr_scene_blend_factor(list as usize, factors);
        }
    }
}
pub unsafe extern "system" fn graphics_root(list: List, root: *mut c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::GRAPHICS_ROOT], fn(List, *mut c_void)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, root);
        cvr_lh_probe(3, list, root as u64, 0, 0);
    }
}
pub unsafe extern "system" fn graphics_table(list: List, index: u32, table: u64) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::GRAPHICS_TABLE], fn(List, u32, u64)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, index, table);
        cvr_lh_probe(4, list, index as u64, table, 0);
    }
}
pub unsafe extern "system" fn graphics_cbv(list: List, index: u32, address: u64) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::GRAPHICS_CBV], fn(List, u32, u64)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, index, address);
        cvr_lh_probe(5, list, index as u64, address, 0);
    }
}
pub unsafe extern "system" fn index_buffer(list: List, view: *const c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::INDEX_BUFFER], fn(List, *const c_void)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, view);
        cvr_lh_probe(6, list, view as u64, 0, 0);
    }
}
pub unsafe extern "system" fn predication(list: List, buffer: *mut c_void, offset: u64, operation: i32) {
    if !buffer.is_null() {
        flush_engine(list, 16);
    }
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::PREDICATION], fn(List, *mut c_void, u64, i32)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, buffer, offset, operation);
        cvr_lh_probe(7, list, buffer as u64, 0, 0);
    }
}
pub unsafe extern "system" fn shading_rate(list: List, rate: i32, combiners: *const i32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::SHADING_RATE], fn(List, i32, *const i32)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        cvr_vrs_restore(list);
        original(list, rate, combiners);
        if recording() {
            cvr_scene_shading_rate(list as usize, rate, combiners);
        }
        cvr_vrs_shading_rate(list, rate, combiners);
        cvr_optics_shading_rate_set(list, rate, combiners);
    }
}
pub unsafe extern "system" fn shading_image(list: List, image: *mut c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::SHADING_IMAGE], fn(List, *mut c_void)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        cvr_vrs_restore(list);
        original(list, image);
        if recording() {
            cvr_scene_shading_rate_image(list as usize, image as usize);
        }
        cvr_vrs_shading_image(list, image);
    }
}
/// VrsClearState.
pub unsafe extern "system" fn clear_state(list: List, pso: *mut c_void) {
    handoff_unknown(list, 1);
    access_unknown(list, 1);
    copy_windows_unknown(list);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::CLEAR_STATE], fn(List, *mut c_void)) else { return };
    flush_engine(list, 1);
    let rate_known = e.head[head::SHADING_RATE] != 0;
    // SAFETY: the engine's arguments.
    unsafe {
        original(list, pso);
        cvr_vrs_reset(list, rate_known, pso);
        cvr_optics_list_reset(list);
    }
    pipeline_reset(pso);
    families_reset(list, pso);
    probe_reset(list, pso, rate_known, false);
}
/// VrsExecuteBundle.
pub unsafe extern "system" fn execute_bundle(list: List, bundle: List) {
    access_unknown(list, 2);
    handoff_unknown(list, 2);
    copy_windows_unknown(list);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::BUNDLE], fn(List, List)) else { return };
    flush_engine(list, 2);
    // SAFETY: the engine's arguments.
    unsafe {
        cvr_vrs_restore(list);
        original(list, bundle);
        cvr_vrs_invalidate(list);
    }
    pipeline_reset(null_mut());
    families_reset(list, null_mut());
    probe_reset(list, null_mut(), false, false);
}
/// ProfileClose.
pub unsafe extern "system" fn close(list: List) -> i32 {
    const E_FAIL: i32 = 0x8000_4005_u32 as i32;
    let Some(e) = entry(list) else { return E_FAIL };
    let Some(original) = original!(e.tail[tail::CLOSE], fn(List) -> i32) else { return E_FAIL };
    if !internal() && retained_active() {
        // SAFETY: the engine's list.
        unsafe { cvr_lh_retained(3, list, null_mut(), null_mut(), 0) };
    }
    // SAFETY: the engine's live list.
    let copy = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.is_some_and(|l| unsafe { l.GetType() } == D3D12_COMMAND_LIST_TYPE_COPY);
    // SAFETY: the engine's list throughout.
    unsafe {
        if copy {
            let result = original(list);
            if result >= 0 {
                cvr_lh_list(2, list, null_mut());
            }
            handoff_close(list, result);
            access_close(list, result);
            return result;
        }
        cvr_lh_early_depth(2, list, null_mut(), 0);
        cvr_vrs_restore(list);
        cvr_mono_cascades_close(list);
        cvr_scene_profile_close(list);
        let result = original(list);
        if result >= 0 {
            cvr_lh_list(2, list, null_mut());
        }
        handoff_close(list, result);
        access_close(list, result);
        if result >= 0 {
            cvr_lh_list(3, list, null_mut());
        }
        result
    }
}
/// CascadeDiscard.
pub unsafe extern "system" fn discard(list: List, resource: *mut c_void, region: *const c_void) {
    resources_engine(list, null_mut(), resource, 3);
    buffer_written(null_mut(), resource);
    texture_written(null_mut(), resource);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::DISCARD], fn(List, *mut c_void, *const c_void)) else { return };
    // MonoCascades.hpp's Discard ignores the view and the clear node.
    // SAFETY: the engine's resource.
    if unsafe { cvr_mono_cascades_discard(resource) } {
        return;
    }
    handoff_written(list, resource, 3);
    access_written(list, resource, 3, ALL_SUBRESOURCES, 0, u64::MAX);
    early_instance_write(list, resource);
    constants_invalidate(list, resource);
    // SAFETY: the engine's arguments.
    unsafe { original(list, resource, region) };
}
/// CascadeCopyTexture.
pub unsafe extern "system" fn cascade_copy_texture(list: List, destination: *const CopyLocation, x: u32, y: u32, z: u32, source: *const CopyLocation, area: *const c_void) {
    // SAFETY: the engine's locations throughout.
    let (target, from) = unsafe { (CopyLocation::resource_of(destination), CopyLocation::resource_of(source)) };
    resources_engine(list, from, target, 4);
    if !destination.is_null() {
        buffer_written(list, target);
        texture_written(list, target);
    }
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.head[head::COPY_TEXTURE], fn(List, *const CopyLocation, u32, u32, u32, *const CopyLocation, *const c_void)) else { return };
    if !destination.is_null() {
        // SAFETY: the engine's resource.
        unsafe { cvr_mono_cascades_write(target) };
        constants_invalidate(list, target);
    }
    handoff_written(list, target, 4);
    // SAFETY: the engine's locations.
    unsafe {
        access_read(list, from, 4, CopyLocation::subresource_of(source), 0, u64::MAX);
        access_written(list, target, 4, CopyLocation::subresource_of(destination), 0, u64::MAX);
        original(list, destination, x, y, z, source, area);
    }
}
pub unsafe extern "system" fn resolve_query(list: List, heap: *mut c_void, kind: i32, first: u32, count: u32, destination: *mut c_void, offset: u64) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::RESOLVE_QUERY], fn(List, *mut c_void, i32, u32, u32, *mut c_void, u64)) else { return };
    buffer_written(null_mut(), destination);
    access_written(list, destination, 12, ALL_SUBRESOURCES, 0, u64::MAX);
    // SAFETY: the engine's arguments.
    unsafe { original(list, heap, kind, first, count, destination, offset) };
}
pub unsafe extern "system" fn immediate_write(list: List, count: u32, parameters: *const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER, modes: *const c_void) {
    let caller = return_address();
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::WRITE_IMMEDIATE], fn(List, u32, *const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER, *const c_void)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        if count != 0 {
            cvr_lh_access_immediate(list, count, parameters.cast(), modes, caller);
        }
        if count != 0 && parameters.is_null() {
            buffer_read_unavailable();
        } else {
            for i in 0..count as usize {
                buffer_written_address((*parameters.add(i)).Dest, 4);
            }
        }
        original(list, count, parameters, modes);
    }
}
/// The resource a barrier names (the windows type holds it as a borrowed interface).
fn barrier_resource(resource: &core::mem::ManuallyDrop<Option<ID3D12Resource>>) -> *mut c_void {
    resource.as_ref().map_or(null_mut(), |r| r.as_raw())
}
pub unsafe extern "system" fn enhanced_barrier(list: List, count: u32, groups: *const D3D12_BARRIER_GROUP) {
    if count != 0 {
        handoff_unknown(list, 5);
        access_unknown(list, 5);
        copy_windows_written_all();
    }
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::ENHANCED_BARRIER], fn(List, u32, *const D3D12_BARRIER_GROUP)) else { return };
    flush_engine(list, 13);
    if count != 0 && groups.is_null() {
        read_observation_unavailable();
    } else {
        for i in 0..count as usize {
            // SAFETY: the engine's groups.
            let group = unsafe { &*groups.add(i) };
            // SAFETY: the union member the group's type names.
            unsafe {
                if group.Type == D3D12_BARRIER_TYPE_BUFFER && !group.Anonymous.pBufferBarriers.is_null() {
                    for j in 0..group.NumBarriers as usize {
                        buffer_written(null_mut(), barrier_resource(&(*group.Anonymous.pBufferBarriers.add(j)).pResource));
                    }
                } else if group.Type == D3D12_BARRIER_TYPE_TEXTURE && !group.Anonymous.pTextureBarriers.is_null() {
                    for j in 0..group.NumBarriers as usize {
                        texture_written(null_mut(), barrier_resource(&(*group.Anonymous.pTextureBarriers.add(j)).pResource));
                    }
                } else if group.NumBarriers != 0 {
                    read_observation_unavailable();
                }
            }
        }
    }
    let (node, side) = node();
    let clear = e.head[head::CLEAR_DSV];
    // SAFETY: the engine's arguments.
    unsafe {
        if clear != 0 {
            cvr_lh_hidden_barriers(false, list, count, groups.cast(), side, node, clear);
        }
        original(list, count, groups);
        if clear != 0 {
            cvr_lh_hidden_barriers(true, list, count, groups.cast(), side, node, clear);
        }
    }
}
pub unsafe extern "system" fn stream_output(list: List, first: u32, count: u32, views: *const D3D12_STREAM_OUTPUT_BUFFER_VIEW) {
    copy_windows_unknown(list);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::STREAM_OUTPUT], fn(List, u32, u32, *const D3D12_STREAM_OUTPUT_BUFFER_VIEW)) else { return };
    if count != 0 {
        access_unknown(list, 14);
    }
    if !views.is_null() {
        for i in 0..count as usize {
            // SAFETY: the engine's views.
            let view = unsafe { &*views.add(i) };
            if view.BufferLocation != 0 {
                buffer_written_address(view.BufferLocation, view.SizeInBytes);
            }
            if view.BufferFilledSizeLocation != 0 {
                buffer_written_address(view.BufferFilledSizeLocation, 8);
            }
        }
    }
    // SAFETY: the engine's arguments.
    unsafe { original(list, first, count, views) };
}
pub unsafe extern "system" fn copy_tiles(list: List, tiled: *mut c_void, coordinate: *const c_void, size: *const c_void, linear: *mut c_void, offset: u64, flags: i32) {
    resources_engine(list, tiled, linear, 15);
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::COPY_TILES], fn(List, *mut c_void, *const c_void, *const c_void, *mut c_void, u64, i32)) else { return };
    access_unknown(list, 15);
    const TO_LINEAR: i32 = 2;
    const TO_TILED: i32 = 4;
    if flags & TO_LINEAR != 0 {
        buffer_written(null_mut(), linear);
    }
    if flags & TO_TILED != 0 {
        texture_written(null_mut(), tiled);
    }
    if flags & (TO_LINEAR | TO_TILED) == 0 {
        read_observation_unavailable();
    }
    if flags & TO_TILED != 0 {
        handoff_written(list, tiled, 6);
    }
    // SAFETY: the engine's arguments.
    unsafe { original(list, tiled, coordinate, size, linear, offset, flags) };
}
type AtomicCopy = unsafe extern "system" fn(List, *mut c_void, u64, *mut c_void, u64, u32, *const *mut c_void, *const c_void);
fn atomic_copy(list: List, wide: bool, destination: *mut c_void, args: (u64, *mut c_void, u64, u32, *const *mut c_void, *const c_void)) {
    let Some(e) = entry(list) else { return };
    let slot = if wide { tail::ATOMIC_COPY64 } else { tail::ATOMIC_COPY32 };
    let Some(original) = original!(e.tail[slot], fn(List, *mut c_void, u64, *mut c_void, u64, u32, *const *mut c_void, *const c_void)) else { return };
    let _: AtomicCopy = original;
    access_unknown(list, 16);
    buffer_written(null_mut(), destination);
    // SAFETY: the engine's arguments.
    unsafe { original(list, destination, args.0, args.1, args.2, args.3, args.4, args.5) };
}
pub unsafe extern "system" fn atomic_copy32(list: List, destination: *mut c_void, destination_offset: u64, source: *mut c_void, source_offset: u64, count: u32, dependencies: *const *mut c_void, ranges: *const c_void) {
    atomic_copy(list, false, destination, (destination_offset, source, source_offset, count, dependencies, ranges));
}
pub unsafe extern "system" fn atomic_copy64(list: List, destination: *mut c_void, destination_offset: u64, source: *mut c_void, source_offset: u64, count: u32, dependencies: *const *mut c_void, ranges: *const c_void) {
    atomic_copy(list, true, destination, (destination_offset, source, source_offset, count, dependencies, ranges));
}
/// RetainedClearRtv.
pub unsafe extern "system" fn clear_rtv(list: List, target: usize, color: *const f32, count: u32, rects: *const c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::CLEAR_RTV], fn(List, usize, *const f32, u32, *const c_void)) else { return };
    flush_engine(list, 6);
    handoff_clear_view(list, target, false);
    access_clear_view(list, target, false);
    // SAFETY: the engine's arguments.
    unsafe { original(list, target, color, count, rects) };
}
type ClearUav<T> = unsafe extern "system" fn(List, u64, usize, *mut c_void, *const T, u32, *const c_void);
fn clear_uav<T>(list: List, slot: usize, resource: *mut c_void, call: impl FnOnce(ClearUav<T>)) {
    let Some(e) = entry(list) else { return };
    let address = e.tail[slot];
    if address == 0 {
        return;
    }
    // SAFETY: the vtable's own ClearUnorderedAccessView of the slot's type.
    let original = unsafe { core::mem::transmute::<usize, ClearUav<T>>(address) };
    handoff_written(list, resource, 11);
    access_written(list, resource, 11, ALL_SUBRESOURCES, 0, u64::MAX);
    buffer_written(list, resource);
    texture_written(list, resource);
    call(original);
}
pub unsafe extern "system" fn clear_uav_uint(list: List, gpu: u64, cpu: usize, resource: *mut c_void, values: *const u32, count: u32, rects: *const c_void) {
    // SAFETY: the engine's arguments.
    clear_uav::<u32>(list, tail::CLEAR_UAV_UINT, resource, |f| unsafe { f(list, gpu, cpu, resource, values, count, rects) });
}
pub unsafe extern "system" fn clear_uav_float(list: List, gpu: u64, cpu: usize, resource: *mut c_void, values: *const f32, count: u32, rects: *const c_void) {
    // SAFETY: the engine's arguments.
    clear_uav::<f32>(list, tail::CLEAR_UAV_FLOAT, resource, |f| unsafe { f(list, gpu, cpu, resource, values, count, rects) });
}
/// RetainedResolve.
pub unsafe extern "system" fn resolve(list: List, destination: *mut c_void, destination_index: u32, source: *mut c_void, source_index: u32, format: i32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::RESOLVE], fn(List, *mut c_void, u32, *mut c_void, u32, i32)) else { return };
    resources_engine(list, source, destination, 11);
    handoff_written(list, destination, 7);
    access_read(list, source, 7, ALL_SUBRESOURCES, 0, u64::MAX);
    access_written(list, destination, 7, ALL_SUBRESOURCES, 0, u64::MAX);
    // SAFETY: the engine's arguments.
    unsafe { original(list, destination, destination_index, source, source_index, format) };
}
/// RetainedResolveRegion.
#[allow(clippy::too_many_arguments)]
pub unsafe extern "system" fn resolve_region(list: List, destination: *mut c_void, destination_index: u32, x: u32, y: u32, source: *mut c_void, source_index: u32, rect: *const c_void, format: i32, mode: i32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::RESOLVE_REGION], fn(List, *mut c_void, u32, u32, u32, *mut c_void, u32, *const c_void, i32, i32)) else { return };
    resources_engine(list, source, destination, 11);
    handoff_written(list, destination, 7);
    access_read(list, source, 7, ALL_SUBRESOURCES, 0, u64::MAX);
    access_written(list, destination, 7, ALL_SUBRESOURCES, 0, u64::MAX);
    // SAFETY: the engine's arguments.
    unsafe { original(list, destination, destination_index, x, y, source, source_index, rect, format, mode) };
}
/// RetainedBeginRenderPass.
pub unsafe extern "system" fn begin_render_pass(list: List, count: u32, targets: *const c_void, depth: *const c_void, flags: i32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::BEGIN_RENDER_PASS], fn(List, u32, *const c_void, *const c_void, i32)) else { return };
    // SAFETY: the engine's arguments.
    unsafe {
        cvr_lh_retained(4, list, null_mut(), null_mut(), 1);
        handoff_unknown(list, 8);
        access_unknown(list, 8);
        original(list, count, targets, depth, flags);
    }
}
/// RetainedEndRenderPass.
pub unsafe extern "system" fn end_render_pass(list: List) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::END_RENDER_PASS], fn(List)) else { return };
    // SAFETY: the engine's list.
    unsafe {
        original(list);
        cvr_lh_retained(4, list, null_mut(), null_mut(), 0);
    }
}
fn meta(list: List, initialize: bool, command: *mut c_void, data: *const c_void, bytes: usize) {
    let Some(e) = entry(list) else { return };
    let slot = if initialize { tail::INITIALIZE_META } else { tail::EXECUTE_META };
    let Some(original) = original!(e.tail[slot], fn(List, *mut c_void, *const c_void, usize)) else { return };
    access_unknown(list, 17);
    handoff_unknown(list, 17);
    // SAFETY: the engine's arguments.
    unsafe { original(list, command, data, bytes) };
}
pub unsafe extern "system" fn initialize_meta(list: List, command: *mut c_void, data: *const c_void, bytes: usize) {
    meta(list, true, command, data, bytes);
}
pub unsafe extern "system" fn execute_meta(list: List, command: *mut c_void, data: *const c_void, bytes: usize) {
    meta(list, false, command, data, bytes);
}
pub unsafe extern "system" fn build_ray(list: List, desc: *const c_void, count: u32, info: *const c_void) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::BUILD_RAY], fn(List, *const c_void, u32, *const c_void)) else { return };
    access_unknown(list, 18);
    handoff_unknown(list, 18);
    // SAFETY: the engine's arguments.
    unsafe { original(list, desc, count, info) };
}
pub unsafe extern "system" fn emit_ray(list: List, info: *const c_void, count: u32, addresses: *const u64) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::EMIT_RAY], fn(List, *const c_void, u32, *const u64)) else { return };
    access_unknown(list, 19);
    handoff_unknown(list, 19);
    // SAFETY: the engine's arguments.
    unsafe { original(list, info, count, addresses) };
}
pub unsafe extern "system" fn copy_ray(list: List, to: u64, from: u64, mode: i32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::COPY_RAY], fn(List, u64, u64, i32)) else { return };
    access_unknown(list, 20);
    handoff_unknown(list, 20);
    // SAFETY: the engine's arguments.
    unsafe { original(list, to, from, mode) };
}
/// ObserveRayDispatch (with the probe).
pub unsafe extern "system" fn dispatch_rays(list: List, desc: *const D3D12_DISPATCH_RAYS_DESC) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::DISPATCH_RAY], fn(List, *const D3D12_DISPATCH_RAYS_DESC)) else { return };
    draws::census_dispatch_rays(desc);
    access_unknown(list, 21);
    handoff_unknown(list, 21);
    // SAFETY: the engine's arguments.
    unsafe { original(list, desc) };
}
/// CensusRayDispatch (without the probe).
pub unsafe extern "system" fn census_rays(list: List, desc: *const D3D12_DISPATCH_RAYS_DESC) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::DISPATCH_RAY], fn(List, *const D3D12_DISPATCH_RAYS_DESC)) else { return };
    draws::census_dispatch_rays(desc);
    // SAFETY: the engine's arguments.
    unsafe { original(list, desc) };
}
pub unsafe extern "system" fn dispatch_mesh(list: List, x: u32, y: u32, z: u32) {
    let Some(e) = entry(list) else { return };
    let Some(original) = original!(e.tail[tail::DISPATCH_MESH], fn(List, u32, u32, u32)) else { return };
    access_unknown(list, 22);
    handoff_unknown(list, 22);
    // SAFETY: the engine's arguments.
    unsafe { original(list, x, y, z) };
}
