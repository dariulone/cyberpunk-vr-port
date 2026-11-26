//! The stereo GPU probe's commands and views (src/Render/StereoGpuProbe.cpp): the native draw's state restored (its
//! targets, viewport, scissor and pipeline); a surface's planes copied into the readback at their footprints (to
//! COPY_SOURCE and, when asked, back); the native reference draw on its own targets (the scene's viewport or the probe's
//! square); the shared view-instanced draw -- with late visibility predicated twice (shared for a pair, the mono pipeline
//! on slice zero otherwise), else under the group's view mask --; the routed native draw kept as the predicated fallback;
//! a timed scope's timestamps and their resolve; the probe's start (its variant, viewport and scissor; each mode's targets
//! cleared and bound; the per-eye mono draws); and, at the probe's creation, each surface's copyable footprints placed
//! after the readback so far (512-aligned) and its render-target or depth views (the two-slice array and each eye's
//! slice).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the probe's state, its scene bindings, resolver, VRS snapshot and
//! surfaces' targets, the command-resource retention and the internal-commands scope; the unit test below records these
//! on WARP, and tools/single_pass_tests/scene_probe_test runs every probe mode through the .cpp in its Rust variant.

use crate::geometry_commands::DrawRecord;
use core::ffi::c_void;
use core::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT_S8X24_UINT};

fn borrowed<T: Interface>(raw: &*mut c_void) -> Option<&T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(raw) }
}
fn unowned(raw: *mut c_void) -> ManuallyDrop<Option<ID3D12Resource>> {
    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
    unsafe { core::mem::transmute::<*mut c_void, ManuallyDrop<Option<ID3D12Resource>>>(raw) }
}
fn transition(resource: *mut c_void, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: unowned(resource),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}
fn draw_indexed(list: &ID3D12GraphicsCommandList, d: &DrawRecord) {
    // SAFETY: a live list in recording.
    unsafe { list.DrawIndexedInstanced(d.indices, d.instances, d.first_index, d.base_vertex, d.first_instance) };
}
fn pipeline(list: &ID3D12GraphicsCommandList, raw: u64) {
    let raw = raw as *mut c_void;
    // SAFETY: a live list; the pipeline the draw's own (or the probe's variant).
    if let Some(state) = borrowed::<ID3D12PipelineState>(&raw) {
        unsafe { list.SetPipelineState(state) };
    }
}
/// The scene's viewport and scissor, or the probe's square.
fn area(d: &DrawRecord, scene: bool, width: u32, height: u32) -> (D3D12_VIEWPORT, RECT) {
    if scene {
        (d.viewport, d.scissor)
    } else {
        (
            D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: width as f32, Height: height as f32, MinDepth: 0.0, MaxDepth: 1.0 },
            RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 },
        )
    }
}

/// Restore: the native draw's targets, viewport, scissor and pipeline.
pub fn restore(list: &ID3D12GraphicsCommandList, d: &DrawRecord) {
    let targets = d.targets.map(|ptr| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: ptr as usize });
    let depth = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: d.depth as usize };
    // SAFETY: a live list; the handles locals.
    unsafe {
        list.OMSetRenderTargets(d.rt_count, Some(targets.as_ptr()), d.rt_contiguous != 0, if d.depth != 0 { Some(&depth) } else { None });
        list.RSSetViewports(&[d.viewport]);
        list.RSSetScissorRects(&[d.scissor]);
    }
    pipeline(list, d.pipeline);
}
/// A surface's planes into the readback: subresource `i` at footprint `first + i * step`, from `state` through
/// COPY_SOURCE (and back to `state` when `back`).
#[allow(clippy::too_many_arguments)]
pub fn copy_planes(list: &ID3D12GraphicsCommandList, resource: *mut c_void, state: D3D12_RESOURCE_STATES, readback: *mut c_void, footprints: &[D3D12_PLACED_SUBRESOURCE_FOOTPRINT], first: usize, step: usize, count: u32, back: bool) {
    let mut barrier = [transition(resource, state, D3D12_RESOURCE_STATE_COPY_SOURCE)];
    // SAFETY: a live list; the resource and readback live (the probe retains them).
    unsafe {
        list.ResourceBarrier(&barrier);
        for sub in 0..count {
            let src = D3D12_TEXTURE_COPY_LOCATION { pResource: unowned(resource), Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub } };
            let dst = D3D12_TEXTURE_COPY_LOCATION {
                pResource: unowned(readback),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: footprints[first + sub as usize * step] },
            };
            list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
        }
        if back {
            let t = &mut *barrier[0].Anonymous.Transition;
            core::mem::swap(&mut t.StateBefore, &mut t.StateAfter);
            list.ResourceBarrier(&barrier);
        }
    }
}
/// Reference: the native draw on the probe's targets (three colours, or depth alone).
#[allow(clippy::too_many_arguments)]
pub fn reference(list: &ID3D12GraphicsCommandList, d: &DrawRecord, scene: bool, width: u32, height: u32, targets: &[D3D12_CPU_DESCRIPTOR_HANDLE; 3], depth: D3D12_CPU_DESCRIPTOR_HANDLE, depth_only: bool) {
    let (viewport, scissor) = area(d, scene, width, height);
    pipeline(list, d.pipeline);
    // SAFETY: a live list; the handles locals.
    unsafe {
        list.RSSetViewports(&[viewport]);
        list.RSSetScissorRects(&[scissor]);
        list.OMSetRenderTargets(if depth_only { 0 } else { 3 }, if depth_only { None } else { Some(targets.as_ptr()) }, false, Some(&depth));
    }
    draw_indexed(list, d);
}
/// SharedDraw's draws: with late visibility (`predicates`) both predicated branches -- the shared draw where the
/// predicate is nonzero, the mono pipeline on slice zero where it is zero --, else one draw under `mask`.
pub fn shared(list: &ID3D12GraphicsCommandList, views: &ID3D12GraphicsCommandList1, predicates: *mut c_void, offset: u64, mask: u32, d: &DrawRecord) {
    // SAFETY: a live list; the predicate buffer live (the probe retains it).
    unsafe {
        match borrowed::<ID3D12Resource>(&predicates) {
            Some(predicate) => {
                // D3D12's predicate operation specifies when to SKIP the draw.
                list.SetPredication(predicate, offset, D3D12_PREDICATION_OP_EQUAL_ZERO);
                views.SetViewInstanceMask(3);
                draw_indexed(list, d);
                list.SetPredication(predicate, offset, D3D12_PREDICATION_OP_NOT_EQUAL_ZERO);
                pipeline(list, d.pipeline);
                views.SetViewInstanceMask(1);
                draw_indexed(list, d);
                list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            }
            None => {
                views.SetViewInstanceMask(mask);
                draw_indexed(list, d);
            }
        }
    }
}
/// RouteNative: the native draw recorded as the fallback, skipped once MAIN publishes the commit flag.
pub fn routed(list: &ID3D12GraphicsCommandList, predicates: &ID3D12Resource, offset: u64, d: &DrawRecord) {
    // SAFETY: a live list and predicate buffer.
    unsafe { list.SetPredication(predicates, offset, D3D12_PREDICATION_OP_NOT_EQUAL_ZERO) };
    draw_indexed(list, d);
    unsafe { list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO) };
}
/// A timed scope's timestamp `query`; at its end (`readback`), the pair resolved at `offset`.
pub fn timestamp(list: &ID3D12GraphicsCommandList, heap: &ID3D12QueryHeap, query: u32, readback: Option<&ID3D12Resource>, offset: u64) {
    // SAFETY: a live list, heap and readback.
    unsafe {
        list.EndQuery(heap, D3D12_QUERY_TYPE_TIMESTAMP, query);
        if let Some(readback) = readback {
            list.ResolveQueryData(heap, D3D12_QUERY_TYPE_TIMESTAMP, query - 1, 2, readback, offset);
        }
    }
}
/// The probe's start: its variant, the viewport and scissor.
pub fn begin(list: &ID3D12GraphicsCommandList, variant: *mut c_void, d: &DrawRecord, scene: bool, width: u32, height: u32) {
    let (viewport, scissor) = area(d, scene, width, height);
    pipeline(list, variant as u64);
    // SAFETY: a live list.
    unsafe {
        list.RSSetViewports(&[viewport]);
        list.RSSetScissorRects(&[scissor]);
    }
}
/// A mode's targets cleared (the colours unless depth alone; depth 0 and `stencil`) and bound.
pub fn clear(list: &ID3D12GraphicsCommandList, targets: &[D3D12_CPU_DESCRIPTOR_HANDLE; 3], depth: D3D12_CPU_DESCRIPTOR_HANDLE, depth_only: bool, stencil: u8) {
    // SAFETY: a live list; the handles the probe's own heaps.
    unsafe {
        if !depth_only {
            for target in targets {
                list.ClearRenderTargetView(*target, &[0.0; 4], None);
            }
        }
        list.ClearDepthStencilView(depth, D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL, 0.0, stencil, None);
        list.OMSetRenderTargets(if depth_only { 0 } else { 3 }, if depth_only { None } else { Some(targets.as_ptr()) }, false, Some(&depth));
    }
}
/// The per-eye mono draws (view instance masks 1 and 2).
pub fn eyes(list: &ID3D12GraphicsCommandList, views: &ID3D12GraphicsCommandList1, d: &DrawRecord) {
    for eye in 0..2 {
        // SAFETY: a live list.
        unsafe { views.SetViewInstanceMask(1 << eye) };
        draw_indexed(list, d);
    }
}
/// A surface's copyable footprints (`planes` per eye) placed after `bytes` (512-aligned): the readback's new size.
pub fn footprints(device: &ID3D12Device, resource: &ID3D12Resource, planes: u32, bytes: u64, out: &mut [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 4], rows: &mut [u32; 4], row_bytes: &mut [u64; 4]) -> u64 {
    // SAFETY: a live resource and device; the outputs hold four entries.
    let desc = unsafe { resource.GetDesc() };
    let mut total = 0u64;
    unsafe { device.GetCopyableFootprints(&desc, 0, planes * 2, 0, Some(out.as_mut_ptr()), Some(rows.as_mut_ptr()), Some(row_bytes.as_mut_ptr()), Some(&mut total)) };
    let base = (bytes + 511) & !511u64;
    for footprint in out.iter_mut().take((planes * 2) as usize) {
        footprint.Offset += base;
    }
    base + total
}
/// A surface's views: the two-slice array view at `array` (when nonzero) and each eye's slice from `first` at `eyes`
/// (depth with the D32_S8X24 view format).
pub fn views(device: &ID3D12Device, resource: &ID3D12Resource, depth: bool, format: DXGI_FORMAT, array: D3D12_CPU_DESCRIPTOR_HANDLE, eyes: &[D3D12_CPU_DESCRIPTOR_HANDLE; 2], first: u32) {
    let view = |slice: u32, size: u32, handle: D3D12_CPU_DESCRIPTOR_HANDLE| {
        // SAFETY: a live device and resource; the handle the probe's own heap.
        unsafe {
            if depth {
                let desc = D3D12_DEPTH_STENCIL_VIEW_DESC {
                    Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
                    ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
                    Flags: D3D12_DSV_FLAG_NONE,
                    Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_DSV { MipSlice: 0, FirstArraySlice: slice, ArraySize: size } },
                };
                device.CreateDepthStencilView(resource, Some(&desc), handle);
            } else {
                let desc = D3D12_RENDER_TARGET_VIEW_DESC {
                    Format: format,
                    ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
                    Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: slice, ArraySize: size, PlaneSlice: 0 } },
                };
                device.CreateRenderTargetView(resource, Some(&desc), handle);
            }
        }
    };
    if array.ptr != 0 {
        view(0, 2, array);
    }
    for eye in first..2 {
        view(eye, 1, eyes[eye as usize]);
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

fn list<'a>(raw: &'a *mut c_void) -> Option<&'a ID3D12GraphicsCommandList> {
    borrowed::<ID3D12GraphicsCommandList>(raw)
}
/// Restore.
///
/// # Safety
/// A live list; `draw` lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_restore(raw: *mut c_void, draw: *const DrawRecord) {
    if let Some(l) = list(&raw) {
        restore(l, unsafe { &*draw })
    }
}
/// A surface's planes into the readback.
///
/// # Safety
/// A live list, resource and readback; `footprints` holds the footprints named.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_copy(raw: *mut c_void, resource: *mut c_void, state: i32, readback: *mut c_void, footprints: *const D3D12_PLACED_SUBRESOURCE_FOOTPRINT, first: u32, step: u32, count: u32, back: bool) {
    if let Some(l) = list(&raw) {
        let all = unsafe { core::slice::from_raw_parts(footprints, 4) };
        copy_planes(l, resource, D3D12_RESOURCE_STATES(state), readback, all, first as usize, step as usize, count, back)
    }
}
/// Reference.
///
/// # Safety
/// A live list; `draw` and `targets` (three) lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_reference(raw: *mut c_void, draw: *const DrawRecord, scene: bool, width: u32, height: u32, targets: *const [D3D12_CPU_DESCRIPTOR_HANDLE; 3], depth: usize, depth_only: bool) {
    if let Some(l) = list(&raw) {
        reference(l, unsafe { &*draw }, scene, width, height, unsafe { &*targets }, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: depth }, depth_only)
    }
}
/// SharedDraw's draws (`predicates` null: one draw under `mask`).
///
/// # Safety
/// A live list and its ID3D12GraphicsCommandList1; `predicates` null or live; `draw` lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_shared(raw: *mut c_void, views: *mut c_void, predicates: *mut c_void, offset: u64, mask: u32, draw: *const DrawRecord) {
    if let (Some(l), Some(v)) = (list(&raw), borrowed::<ID3D12GraphicsCommandList1>(&views)) {
        shared(l, v, predicates, offset, mask, unsafe { &*draw })
    }
}
/// RouteNative's predicated fallback draw.
///
/// # Safety
/// A live list and predicate buffer; `draw` lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_routed(raw: *mut c_void, predicates: *mut c_void, offset: u64, draw: *const DrawRecord) {
    if let (Some(l), Some(p)) = (list(&raw), borrowed::<ID3D12Resource>(&predicates)) {
        routed(l, p, offset, unsafe { &*draw })
    }
}
/// A timed scope's timestamp (and, with `readback`, its pair's resolve).
///
/// # Safety
/// A live list and heap; `readback` null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_timestamp(raw: *mut c_void, heap: *mut c_void, query: u32, readback: *mut c_void, offset: u64) {
    if let (Some(l), Some(h)) = (list(&raw), borrowed::<ID3D12QueryHeap>(&heap)) {
        timestamp(l, h, query, borrowed::<ID3D12Resource>(&readback), offset)
    }
}
/// The probe's start: variant, viewport, scissor.
///
/// # Safety
/// A live list and variant; `draw` lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_begin(raw: *mut c_void, variant: *mut c_void, draw: *const DrawRecord, scene: bool, width: u32, height: u32) {
    if let Some(l) = list(&raw) {
        begin(l, variant, unsafe { &*draw }, scene, width, height)
    }
}
/// A mode's targets cleared and bound.
///
/// # Safety
/// A live list; `targets` (three) lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_clear(raw: *mut c_void, targets: *const [D3D12_CPU_DESCRIPTOR_HANDLE; 3], depth: usize, depth_only: bool, stencil: u8) {
    if let Some(l) = list(&raw) {
        clear(l, unsafe { &*targets }, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: depth }, depth_only, stencil)
    }
}
/// The per-eye mono draws.
///
/// # Safety
/// A live list and its ID3D12GraphicsCommandList1; `draw` lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_eyes(raw: *mut c_void, views: *mut c_void, draw: *const DrawRecord) {
    if let (Some(l), Some(v)) = (list(&raw), borrowed::<ID3D12GraphicsCommandList1>(&views)) {
        eyes(l, v, unsafe { &*draw })
    }
}
/// A surface's footprints after `bytes`: the readback's new size.
///
/// # Safety
/// A live device and resource; the outputs hold four entries.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_footprints(device: *mut c_void, resource: *mut c_void, planes: u32, bytes: u64, out: *mut [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 4], rows: *mut [u32; 4], row_bytes: *mut [u64; 4]) -> u64 {
    match (borrowed::<ID3D12Device>(&device), borrowed::<ID3D12Resource>(&resource)) {
        (Some(d), Some(r)) => footprints(d, r, planes, bytes, unsafe { &mut *out }, unsafe { &mut *rows }, unsafe { &mut *row_bytes }),
        _ => bytes,
    }
}
/// A surface's views.
///
/// # Safety
/// A live device and resource; `eyes` (two) lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpr_views(device: *mut c_void, resource: *mut c_void, depth: bool, format: i32, array: usize, eyes: *const [D3D12_CPU_DESCRIPTOR_HANDLE; 2], first: u32) {
    if let (Some(d), Some(r)) = (borrowed::<ID3D12Device>(&device), borrowed::<ID3D12Resource>(&resource)) {
        views(d, r, depth, DXGI_FORMAT(format), D3D12_CPU_DESCRIPTOR_HANDLE { ptr: array }, unsafe { &*eyes }, first)
    }
}

#[cfg(test)]
mod tests {
    //! On WARP: a two-slice target's footprints and views, cleared and drawn into, its slices copied into the readback.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn warp() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    #[test]
    fn commands() {
        let device = warp();
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 8,
            Height: 4,
            DepthOrArraySize: 2,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let mut target: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_RENDER_TARGET, None, &mut target) }.unwrap();
        let target = target.unwrap();
        let (mut fp, mut rows, mut row_bytes) = ([D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); 4], [0u32; 4], [0u64; 4]);
        let bytes = footprints(&device, &target, 1, 16, &mut fp, &mut rows, &mut row_bytes);
        assert_eq!((fp[0].Offset, rows[0], row_bytes[0]), (512, 4, 32), "after the readback so far, 512-aligned");
        assert!(fp[1].Offset > fp[0].Offset && bytes > fp[1].Offset);
        let rtv: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 3, ..Default::default() }) }.unwrap();
        let dsv: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV, NumDescriptors: 1, ..Default::default() }) }.unwrap();
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
        let start = unsafe { rtv.GetCPUDescriptorHandleForHeapStart() };
        let at = |i: usize| D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + i * stride };
        views(&device, &target, false, DXGI_FORMAT_R8G8B8A8_UNORM, at(0), &[at(1), at(2)], 0);
        let readback = crate::readback_buffers::buffer(&device, None, bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).unwrap();
        let depth = D3D12_RESOURCE_DESC { Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT, Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, DepthOrArraySize: 1, ..desc };
        let mut ds: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &depth, D3D12_RESOURCE_STATE_DEPTH_WRITE, None, &mut ds) }.unwrap();
        let depth_handle = unsafe { dsv.GetCPUDescriptorHandleForHeapStart() };
        let dsv_desc = D3D12_DEPTH_STENCIL_VIEW_DESC { Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT, ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D, ..Default::default() };
        unsafe { device.CreateDepthStencilView(ds.as_ref().unwrap(), Some(&dsv_desc), depth_handle) };
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        clear(&list, &[at(1), at(2), at(0)], depth_handle, false, 0);
        // A colour written to eye 1's slice alone.
        unsafe { list.ClearRenderTargetView(at(2), &[1.0, 0.0, 0.0, 1.0], None) };
        copy_planes(&list, target.as_raw(), D3D12_RESOURCE_STATE_RENDER_TARGET, readback.as_raw(), &fp, 0, 1, 2, true);
        unsafe { list.Close() }.unwrap();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe { queue.ExecuteCommandLists(&[Some(list.cast().unwrap())]) };
        unsafe { queue.Signal(&fence, 1) }.unwrap();
        while unsafe { fence.GetCompletedValue() } < 1 {}
        let mut mapped = core::ptr::null_mut();
        unsafe { readback.Map(0, None, Some(&mut mapped)) }.unwrap();
        let pixel = |footprint: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT| unsafe { *(mapped as *const u8).add(footprint.Offset as usize) as u32 | (*(mapped as *const u8).add(footprint.Offset as usize + 3) as u32) << 24 };
        assert_eq!(pixel(&fp[0]), 0, "eye 0 cleared to zero");
        assert_eq!(pixel(&fp[1]), 0xFF00_00FF, "eye 1's slice written through its own view");
        unsafe { readback.Unmap(0, None) };
    }
}
