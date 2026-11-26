//! The paired geometry probe's checks (src/Render/NativePairedGeometry.cpp, `NativeQueue`, `SameGeometry` and `Poll`'s
//! readback comparison): the game's native direct queue (the registry the image holds at 0x3BCDCA8, the lease's, and its
//! queue at +0x14104D0); the second eye's draw being the first's geometry (pipeline, root, counts, first index, base
//! vertex, input slots, index view, the 48 inline instance bytes, every bound vertex view but slot 7, viewport and
//! scissor); and each captured plane's texels compared between the native target, the paired array and the target
//! before the draw (per output: the differing and the changed texels; per side: the compared bytes).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npg_native_queue`, `cvr_npg_same_geometry` and
//! `cvr_npg_compare` (the draws as geometry_commands' DrawRecord; the planes mirrored; the readback bounds-checked
//! before anything is counted); the unit tests below cover each.

use crate::geometry_commands::DrawRecord;
use crate::instance_layout::read;
use windows::Win32::Graphics::Direct3D12::D3D12_PLACED_SUBRESOURCE_FOOTPRINT;

fn bytes_of<T>(value: &T) -> &[u8] {
    // SAFETY: plain data read as bytes.
    unsafe { core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>()) }
}

/// NativeQueue's reads (the queue's type check stays the .cpp's).
pub fn native_queue(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, registry: u64, queue: u64) -> bool {
    let at = |address: u64| {
        let mut b = [0u8; 8];
        read(address, &mut b).then(|| u64::from_le_bytes(b))
    };
    base != 0
        && queue != 0
        && registry != 0
        && base <= u64::MAX - 0x3BCDCB0
        && at(base + 0x3BCDCA8) == Some(registry)
        && registry <= u64::MAX - 0x14104D8
        && at(registry + 0x14104D0) == Some(queue)
}
/// SameGeometry.
pub fn same_geometry(a: &DrawRecord, b: &DrawRecord) -> bool {
    if a.pipeline != b.pipeline
        || a.root != b.root
        || a.indices != b.indices
        || a.instances != b.instances
        || a.first_index != b.first_index
        || a.base_vertex != b.base_vertex
        || a.input_slot_mask != b.input_slot_mask
        || bytes_of(&a.index) != bytes_of(&b.index)
        || a.instance_data_bytes != 48
        || b.instance_data_bytes != 48
        || a.instance_data[..48] != b.instance_data[..48]
    {
        return false;
    }
    if (0..16).any(|slot| slot != 7 && a.input_slot_mask & (1 << slot) != 0 && bytes_of(&a.vertices[slot]) != bytes_of(&b.vertices[slot])) {
        return false;
    }
    bytes_of(&a.viewport) == bytes_of(&b.viewport) && bytes_of(&a.scissor) == bytes_of(&b.scissor)
}

/// The .cpp's Plane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub layout: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 3],
    pub slot: u32,
    pub plane: u32,
    pub rows: u32,
    pub width: u32,
    pub stride: u32,
    pub component: u32,
    pub bytes: u32,
}
const _: () = assert!(core::mem::size_of::<Plane>() == 128);

/// The counters one side's comparison adds to.
pub struct Counters<'a> {
    pub different: &'a mut [u64; 12],
    pub bytes: &'a mut [u64; 2],
    pub changed: &'a mut [u64; 12],
}
fn texel(p: &Plane, version: usize, x: u32, y: u32) -> Option<u64> {
    let l = &p.layout[version];
    l.Offset
        .checked_add(u64::from(y).checked_mul(u64::from(l.Footprint.RowPitch))?)?
        .checked_add(u64::from(x).checked_mul(u64::from(p.stride))?)?
        .checked_add(u64::from(p.component))
}
/// Poll's comparison of one side: false (nothing counted) when a plane reaches outside the readback.
pub fn compare(image: &[u8], planes: &[Plane], side: u32, counters: Counters) -> bool {
    if side > 1 {
        return false;
    }
    // Bounds first: the last texel of every version of every plane.
    for p in planes {
        if p.rows == 0 || p.width == 0 {
            continue;
        }
        if p.slot > 4 || p.plane > 1 || (p.slot == 4 && p.plane > 1) {
            return false;
        }
        for version in 0..3 {
            match texel(p, version, p.width - 1, p.rows - 1).and_then(|at| at.checked_add(u64::from(p.bytes))) {
                Some(end) if end <= image.len() as u64 => {}
                _ => return false,
            }
        }
    }
    for p in planes {
        let output = (side * 6 + if p.slot == 4 { 4 + p.plane } else { p.slot }) as usize;
        let n = p.bytes as usize;
        for y in 0..p.rows {
            for x in 0..p.width {
                let at = |version| texel(p, version, x, y).unwrap() as usize;
                let (a, b, before) = (&image[at(0)..at(0) + n], &image[at(1)..at(1) + n], &image[at(2)..at(2) + n]);
                counters.different[output] = counters.different[output].wrapping_add(u64::from(a != b));
                counters.bytes[side as usize] = counters.bytes[side as usize].wrapping_add(u64::from(p.bytes));
                counters.changed[output] = counters.changed[output].wrapping_add(u64::from(a != before));
            }
        }
    }
    true
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_npg_native_queue(base: u64, registry: u64, queue: u64) -> bool {
    native_queue(&read, base, registry, queue)
}
/// # Safety
/// Both draws are valid DrawRecords.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npg_same_geometry(a: *const DrawRecord, b: *const DrawRecord) -> bool {
    unsafe { same_geometry(&*a, &*b) }
}
/// # Safety
/// `image` holds `length` bytes, `planes` `count` planes; the counters hold 12, 2 and 12 values.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npg_compare(
    image: *const u8,
    length: usize,
    planes: *const Plane,
    count: usize,
    side: u32,
    different: *mut [u64; 12],
    bytes: *mut [u64; 2],
    changed: *mut [u64; 12],
) -> bool {
    let image = if image.is_null() || length == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(image, length) } };
    let planes = if planes.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(planes, count) } };
    unsafe { compare(image, planes, side, Counters { different: &mut *different, bytes: &mut *bytes, changed: &mut *changed }) }
}

// ---- RecordProof ----------------------------------------------------------------------------------------------------

/// RecordProof's targets by slot (colors 0..2, depth 4): the native target after the draw, the paired array, the
/// target as it was before.
#[repr(C)]
pub struct ProofTargets {
    pub native: [*mut core::ffi::c_void; 5],
    pub array: [*mut core::ffi::c_void; 5],
    pub before: [*mut core::ffi::c_void; 5],
}
/// The planes (into the C++ half's room) and the readback created (with its reference).
#[repr(C)]
pub struct ProofOut {
    pub planes: *mut Plane,
    pub capacity: usize,
    pub count: usize,
    pub readback: *mut core::ffi::c_void,
}
/// The list's keeping of objects, and the internal-commands scope from the readback on.
#[repr(C)]
pub struct ProofHost {
    pub user: *mut core::ffi::c_void,
    pub keep: unsafe extern "C-unwind" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, *const *mut core::ffi::c_void, usize) -> bool,
    pub internal: unsafe extern "C-unwind" fn(*mut core::ffi::c_void),
}
const _: () = assert!(
    core::mem::size_of::<ProofTargets>() == 120 && core::mem::size_of::<ProofOut>() == 32 && core::mem::size_of::<ProofHost>() == 24
);
const PROOF_SLOTS: [u32; 4] = [0, 1, 2, 4];
fn borrowed_resource(address: *mut core::ffi::c_void) -> Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource> {
    use windows::core::Interface;
    // SAFETY: a live resource the caller holds for the call.
    unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&address) }.cloned()
}
/// The proof's planes: per slot and plane the native, the array's eye slice and the before footprints, each 512-aligned
/// after the last, their rows the native height and their row bytes whole pixels (colors 4 bytes; depth 4, stencil 1 at
/// component 4 of an 8-byte stride); None on a footprint the proof refuses, a full room or more than 512 MiB.
pub fn proof_planes(device: &windows::Win32::Graphics::Direct3D12::ID3D12Device, side: u32, t: &ProofTargets, out: &mut ProofOut) -> Option<u64> {
    use windows::Win32::Graphics::Direct3D12::*;
    let footprint = |desc: &D3D12_RESOURCE_DESC, sub: u32, at: u64, layout: &mut D3D12_PLACED_SUBRESOURCE_FOOTPRINT| {
        let (mut rows, mut row_bytes, mut total) = (0u32, 0u64, 0u64);
        // SAFETY: a live device; the outputs are locals.
        unsafe { device.GetCopyableFootprints(desc, sub, 1, at, Some(layout), Some(&mut rows), Some(&mut row_bytes), Some(&mut total)) };
        (rows, row_bytes, total)
    };
    let after = |layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT, rows: u32| (layout.Offset + u64::from(layout.Footprint.RowPitch) * u64::from(rows) + 511) & !511;
    let (mut size, side) = (0u64, side & 1);
    out.count = 0;
    for slot in PROOF_SLOTS {
        // SAFETY: live resources.
        let native = unsafe { borrowed_resource(t.native[slot as usize])?.GetDesc() };
        let array = unsafe { borrowed_resource(t.array[slot as usize])?.GetDesc() };
        let before = unsafe { borrowed_resource(t.before[slot as usize])?.GetDesc() };
        for plane in 0..if slot == 4 { 2u32 } else { 1 } {
            let mut item = Plane { slot, plane, width: native.Width as u32, ..Plane::default() };
            let (rows, row_bytes, total) = footprint(&native, plane, size, &mut item.layout[0]);
            if total == u64::MAX || rows == 0 {
                return None;
            }
            size = after(&item.layout[0], rows);
            let (array_rows, array_bytes, total) = footprint(&array, 1 - side + plane * 2, size, &mut item.layout[1]);
            if total == u64::MAX || rows != array_rows || rows != native.Height || row_bytes != array_bytes || native.Width == 0 || row_bytes % native.Width != 0 {
                return None;
            }
            size = after(&item.layout[1], array_rows);
            let (before_rows, before_bytes, total) = footprint(&before, plane, size, &mut item.layout[2]);
            if total == u64::MAX || before_rows != rows || before_bytes != row_bytes {
                return None;
            }
            size = after(&item.layout[2], before_rows);
            item.rows = rows;
            item.stride = (row_bytes / native.Width) as u32;
            item.bytes = if slot == 4 && plane != 0 { 1 } else { 4 };
            item.component = if slot == 4 && plane != 0 && item.stride == 8 { 4 } else { 0 };
            if item.component + item.bytes > item.stride || (slot < 4 && item.stride != 4) || out.count >= out.capacity {
                return None;
            }
            // SAFETY: the C++ half's room for `capacity` planes.
            unsafe { *out.planes.add(out.count) = item };
            out.count += 1;
        }
    }
    (size != 0 && size <= 512 * 1024 * 1024).then_some(size)
}
/// RecordProof: the planes, the readback, the targets kept, and each slot's targets to COPY_SOURCE, its planes' three
/// copies and the targets back.
///
/// # Safety
/// The targets are live in their render states (the depth DEPTH_WRITE); the list is recording.
pub unsafe fn record_proof(
    device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
    list: &windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList,
    side: u32,
    t: &ProofTargets,
    out: &mut ProofOut,
    host: &ProofHost,
) -> bool {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::*;
    let Some(size) = proof_planes(device, side, t, out) else { return false };
    // SAFETY: the C++ half's callbacks.
    unsafe { (host.internal)(host.user) };
    let Some(readback) = crate::prepared_pair_record::readback_buffer(device, size) else { return false };
    let raw = readback.into_raw();
    out.readback = raw;
    // SAFETY: as above.
    if !unsafe { (host.keep)(host.user, list.as_raw(), [raw].as_ptr(), 1) } {
        return false;
    }
    let unowned = |resource: *mut core::ffi::c_void| {
        // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
        unsafe { core::mem::transmute::<*mut core::ffi::c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource) }
    };
    let side = side & 1;
    // SAFETY: the room's planes, written above.
    let planes = unsafe { core::slice::from_raw_parts(out.planes, out.count) };
    for slot in PROOF_SLOTS {
        let (native, array, before) = (t.native[slot as usize], t.array[slot as usize], t.before[slot as usize]);
        // SAFETY: as above.
        if !unsafe { (host.keep)(host.user, list.as_raw(), [native, array, before].as_ptr(), 3) } {
            return false;
        }
        let state = if slot == 4 { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
        let barrier = |resource: *mut core::ffi::c_void, sub: u32, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES| D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER { pResource: unowned(resource), Subresource: sub, StateBefore: before, StateAfter: after }),
            },
        };
        let mut subs = vec![(native, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES), (before, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES)];
        for plane in 0..if slot == 4 { 2u32 } else { 1 } {
            subs.push((array, 1 - side + plane * 2));
        }
        let to_copy: Vec<_> = subs.iter().map(|&(r, s)| barrier(r, s, state, D3D12_RESOURCE_STATE_COPY_SOURCE)).collect();
        let back: Vec<_> = subs.iter().map(|&(r, s)| barrier(r, s, D3D12_RESOURCE_STATE_COPY_SOURCE, state)).collect();
        // SAFETY: the list recording; the resources are the caller's.
        unsafe {
            list.ResourceBarrier(&to_copy);
            for plane in planes.iter().filter(|p| p.slot == slot) {
                for version in 0..3usize {
                    let from = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: unowned(match version {
                            1 => array,
                            2 => before,
                            _ => native,
                        }),
                        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: if version == 1 { 1 - side + plane.plane * 2 } else { plane.plane } },
                    };
                    let to = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: unowned(raw),
                        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: plane.layout[version] },
                    };
                    list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
                }
            }
            list.ResourceBarrier(&back);
        }
    }
    true
}
/// RecordProof.
///
/// # Safety
/// As record_proof; `out` holds its room.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npg_record_proof(
    device: *mut core::ffi::c_void,
    list: *mut core::ffi::c_void,
    side: u32,
    t: *const ProofTargets,
    out: *mut ProofOut,
    host: *const ProofHost,
) -> bool {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12GraphicsCommandList};
    // SAFETY: live objects the caller holds.
    let (Some(device), Some(list)) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }.cloned(), unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.cloned())
    else {
        return false;
    };
    // SAFETY: as documented.
    unsafe { record_proof(&device, &list, side, &*t, &mut *out, &*host) }
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn proof_on_warp() {
        use core::ffi::c_void;
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::Common::*;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut created: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut created) }.unwrap();
        let device = created.unwrap();
        let texture = |format: DXGI_FORMAT, slices: u16, depth: bool, height: u32| {
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: 8,
                Height: height,
                DepthOrArraySize: slices,
                MipLevels: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Flags: if depth { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET },
                ..Default::default()
            };
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let state = if depth { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
            let mut out: Option<ID3D12Resource> = None;
            unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.unwrap();
            out.unwrap()
        };
        let set = |slices: u16, height: u32| -> Vec<ID3D12Resource> {
            (0..5).map(|slot| if slot == 4 { texture(DXGI_FORMAT_D32_FLOAT_S8X24_UINT, slices, true, height) } else { texture(DXGI_FORMAT_R8G8B8A8_UNORM, slices, false, height) }).collect()
        };
        let (native, array, before) = (set(1, 4), set(2, 4), set(1, 4));
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let heap: ID3D12DescriptorHeap =
            unsafe { device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 3, ..Default::default() }) }.unwrap();
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
        let start = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        for (i, (resource, color)) in [(&native[0], [1.0f32, 0.0, 0.0, 1.0]), (&array[0], [0.0, 1.0, 0.0, 1.0]), (&before[0], [0.0, 0.0, 1.0, 1.0])].iter().enumerate() {
            let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + i * stride };
            unsafe {
                // The array: only the left eye's slice (slice 1 for side 0) green.
                let slice = D3D12_RENDER_TARGET_VIEW_DESC {
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
                    Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: 1, ArraySize: 1, PlaneSlice: 0 } },
                };
                device.CreateRenderTargetView(*resource, if i == 1 { Some(&slice) } else { None }, handle);
                list.ClearRenderTargetView(handle, color, None);
            }
        }
        let raw = |v: &[ID3D12Resource]| -> [*mut c_void; 5] { core::array::from_fn(|i| v[i].as_raw()) };
        let targets = ProofTargets { native: raw(&native), array: raw(&array), before: raw(&before) };
        unsafe extern "C-unwind" fn keep(_: *mut c_void, _: *mut c_void, _: *const *mut c_void, _: usize) -> bool {
            true
        }
        unsafe extern "C-unwind" fn internal(_: *mut c_void) {}
        let host = ProofHost { user: core::ptr::null_mut(), keep, internal };
        let mut room = [Plane::default(); 8];
        let mut out = ProofOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut() };
        assert!(unsafe { record_proof(&device, &list, 0, &targets, &mut out, &host) });
        let readback = unsafe { ID3D12Resource::from_raw(out.readback) };
        assert_eq!(out.count, 5, "three colors, the depth and the stencil");
        let planes = &room[..5];
        assert_eq!((planes[0].slot, planes[0].stride, planes[0].bytes, planes[0].width, planes[0].rows), (0, 4, 4, 8, 4));
        assert_eq!((planes[3].slot, planes[3].plane, planes[4].plane, planes[4].bytes), (4, 0, 1, 1));
        unsafe { list.Close() }.unwrap();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe {
            queue.ExecuteCommandLists(&[Some(list.cast().unwrap())]);
            queue.Signal(&fence, 1).unwrap();
        }
        while unsafe { fence.GetCompletedValue() } < 1 {
            std::thread::yield_now();
        }
        let bytes = unsafe { readback.GetDesc() }.Width as usize;
        let mut mapped = core::ptr::null_mut();
        unsafe { readback.Map(0, None, Some(&mut mapped)) }.unwrap();
        let image = unsafe { core::slice::from_raw_parts(mapped as *const u8, bytes) };
        let pixel = |version: usize| {
            let at = (planes[0].layout[version].Offset + 2 * u64::from(planes[0].layout[version].Footprint.RowPitch) + 12) as usize;
            [image[at], image[at + 1], image[at + 2], image[at + 3]]
        };
        assert_eq!((pixel(0), pixel(1), pixel(2)), ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]), "the native target, the array's eye slice, the target before");
        unsafe { readback.Unmap(0, None) };
        let short = set(1, 2);
        let mut wrong = ProofOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut() };
        assert!(proof_planes(&device, 0, &ProofTargets { before: raw(&short), ..targets }, &mut wrong).is_none(), "a before target of other rows");
        let mut small = ProofOut { planes: room.as_mut_ptr(), capacity: 4, count: 0, readback: core::ptr::null_mut() };
        assert!(proof_planes(&device, 1, &targets, &mut small).is_none(), "a full room");
    }

    #[test]
    fn queues() {
        let mut words = BTreeMap::new();
        let (base, registry, queue) = (0x1000_0000u64, 0x2000_0000u64, 0x77u64);
        words.insert(base + 0x3BCDCA8, registry);
        words.insert(registry + 0x14104D0, queue);
        let read = |address: u64, out: &mut [u8]| match words.get(&address) {
            Some(v) if out.len() == 8 => {
                out.copy_from_slice(&v.to_le_bytes());
                true
            }
            _ => false,
        };
        assert!(native_queue(&read, base, registry, queue));
        assert!(!native_queue(&read, base, registry, 0x78));
        assert!(!native_queue(&read, base, registry + 1, queue));
        assert!(!native_queue(&read, 0, registry, queue));
        assert!(!native_queue(&read, base, registry, 0));
    }
    fn draw() -> DrawRecord {
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        (d.pipeline, d.root, d.indices, d.instances, d.input_slot_mask, d.instance_data_bytes) = (1, 2, 30, 4, 0b1000_0001, 48);
        d.vertices[0].SizeInBytes = 64;
        d.instance_data[3] = 9;
        d
    }
    #[test]
    fn geometry() {
        assert!(same_geometry(&draw(), &draw()));
        let changes: [fn(&mut DrawRecord); 10] = [
            |d| d.pipeline = 5,
            |d| d.base_vertex = -1,
            |d| d.index.SizeInBytes = 4,
            |d| d.instance_data_bytes = 64,
            |d| d.instance_data[47] = 1,
            |d| d.vertices[0].StrideInBytes = 4,
            |d| d.viewport.Width = 2.0,
            |d| d.scissor.right = 3,
            |d| d.input_slot_mask = 1,
            |d| d.first_index = 1,
        ];
        for change in changes {
            let mut b = draw();
            change(&mut b);
            assert!(!same_geometry(&draw(), &b));
        }
        // Slot 7 and unbound slots are not compared, nor instance bytes past 48.
        let mut b = draw();
        b.vertices[7].SizeInBytes = 1;
        b.vertices[3].SizeInBytes = 1;
        b.instance_data[50] = 1;
        assert!(same_geometry(&draw(), &b));
    }
    fn plane(slot: u32, index: u32, rows: u32, width: u32, offsets: [u64; 3]) -> Plane {
        let mut p = Plane { slot, plane: index, rows, width, stride: 4, component: 0, bytes: 4, ..Default::default() };
        for (l, o) in p.layout.iter_mut().zip(offsets) {
            l.Offset = o;
            l.Footprint.RowPitch = 16;
        }
        p
    }
    #[test]
    fn comparisons() {
        // A 2x2 plane in three versions at 0, 32 and 64: one texel differs from the array, two from before.
        let mut image = vec![0u8; 96];
        image[32 + 16 + 4] = 1;
        image[64] = 2;
        image[64 + 4] = 2;
        let (mut different, mut bytes, mut changed) = ([0u64; 12], [0u64; 2], [0u64; 12]);
        let planes = [plane(1, 0, 2, 2, [0, 32, 64])];
        assert!(compare(&image, &planes, 1, Counters { different: &mut different, bytes: &mut bytes, changed: &mut changed }));
        assert_eq!((different[7], bytes[1], changed[7]), (1, 16, 2));
        // The depth's second plane counts at output 5 of its side.
        let (mut different, mut bytes, mut changed) = ([0u64; 12], [0u64; 2], [0u64; 12]);
        assert!(compare(&image, &[plane(4, 1, 2, 2, [0, 32, 64])], 0, Counters { different: &mut different, bytes: &mut bytes, changed: &mut changed }));
        assert_eq!((different[5], changed[5], bytes[0]), (1, 2, 16));
        // A plane reaching past the readback counts nothing.
        let (mut different, mut bytes, mut changed) = ([0u64; 12], [0u64; 2], [0u64; 12]);
        assert!(!compare(&image, &[planes[0], plane(0, 0, 2, 2, [0, 32, 80])], 0, Counters { different: &mut different, bytes: &mut bytes, changed: &mut changed }));
        assert_eq!((different, bytes, changed), ([0; 12], [0; 2], [0; 12]));
    }
}
