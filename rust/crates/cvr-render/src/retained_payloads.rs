//! The retained geometry's payload checks (src/Render/NativeRetainedGeometry.cpp, `Poll`): in the readback mode each
//! eye's plane compares its two copies (layouts 0 and 1) per pixel of the target's width at the plane's stride and
//! component, counting the differing pixels at the eye's plane output (colors 0..3, depth 4, stencil 5, six per eye)
//! and the compared bytes per eye; and each full t5 row of an eye compares its original readback range (offset 12)
//! with its private copy (offset 8, both `row[9]` bytes) once the row is pending (14 = 1): the differing bytes and the
//! first one's index (15), the row's verdict (2 equal, 3 different) and the stats' compared rows (9) and differences
//! (10); a row outside either readback, or not pending, fails the eye.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_plane_differences` (bounds-checked against the
//! readback; a plane outside it fails the map, 91) and `cvr_nrg_full_t5_rows` (the plane mirrored); the unit tests
//! below cover each output, the bounds and each row verdict.

use windows::Win32::Graphics::Direct3D12::D3D12_PLACED_SUBRESOURCE_FOOTPRINT;

/// The .cpp's Plane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub layouts: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 2],
    pub rows: u32,
    pub stride: u32,
    pub component: u32,
    pub bytes: u32,
    pub slot: u32,
    pub plane: u32,
}
const _: () = assert!(core::mem::size_of::<Plane>() == 88 && core::mem::offset_of!(Plane, rows) == 64);

/// One eye's plane differences; false when the plane leaves the readback (the counts then hold the rows before it).
pub fn plane_differences(image: &[u8], side: u32, plane: &Plane, width: u64, different: &mut [u64; 12], compared: &mut [u64; 2]) -> bool {
    let output = side as usize * 6 + if plane.slot == 4 { 4 + plane.plane as usize } else { plane.slot as usize };
    if side > 1 || output >= 12 {
        return false;
    }
    let bytes = plane.bytes as usize;
    let span = if width == 0 { Some(0) } else { (width - 1).checked_mul(u64::from(plane.stride)).and_then(|v| v.checked_add(u64::from(plane.bytes))) };
    let Some(span) = span else { return false };
    let start = |layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT, row: u32| -> Option<usize> {
        let at = layout.Offset.checked_add(u64::from(row).checked_mul(u64::from(layout.Footprint.RowPitch))?)?.checked_add(u64::from(plane.component))?;
        (at.checked_add(span)? <= image.len() as u64).then_some(at as usize)
    };
    for row in 0..plane.rows {
        let (Some(a), Some(b)) = (start(&plane.layouts[0], row), start(&plane.layouts[1], row)) else { return false };
        for pixel in 0..width as usize {
            let offset = pixel * plane.stride as usize;
            let differs = image[a + offset..a + offset + bytes] != image[b + offset..b + offset + bytes];
            different[output] = different[output].wrapping_add(u64::from(differs));
            compared[side as usize] = compared[side as usize].wrapping_add(u64::from(plane.bytes));
        }
    }
    true
}
/// One eye's full t5 rows; returns the eye's validity (`valid` its readbacks' mapping).
pub fn full_t5_rows(rows: &mut [[u64; 16]], side: u32, valid: bool, original: &[u8], copied: &[u8], stats: &mut [u64; 16]) -> bool {
    let (original_end, private_end) = (original.len() as u64, copied.len() as u64);
    for row in rows.iter_mut().filter(|row| row[1] == u64::from(side)) {
        if !valid || row[8] > private_end || row[9] > private_end - row[8] || row[12] > original_end || row[9] > original_end - row[12] || row[14] != 1 {
            return false;
        }
        let (a, b) = (&original[row[12] as usize..][..row[9] as usize], &copied[row[8] as usize..][..row[9] as usize]);
        let mut differences = 0u64;
        if a != b {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                if x != y {
                    if differences == 0 {
                        row[15] = i as u64;
                    }
                    differences += 1;
                }
            }
        }
        row[14] = if differences != 0 { 3 } else { 2 };
        stats[10] = stats[10].wrapping_add(differences);
        stats[9] = stats[9].wrapping_add(1);
    }
    valid
}

// ---- the C ABI

fn bytes<'a>(data: *const u8, length: usize) -> &'a [u8] {
    if data.is_null() || length == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, length) } }
}
/// # Safety
/// `image` holds `length` bytes; `plane` is the mirrored plane; the arrays hold 12 and 2 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_plane_differences(
    image: *const u8,
    length: usize,
    side: u32,
    plane: *const Plane,
    width: u64,
    different: *mut [u64; 12],
    compared: *mut [u64; 2],
) -> bool {
    plane_differences(bytes(image, length), side, unsafe { &*plane }, width, unsafe { &mut *different }, unsafe { &mut *compared })
}
/// # Safety
/// `rows` holds `count` rows of 16; the readbacks hold their lengths (null when unmapped); `stats` holds 16 values.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nrg_full_t5_rows(
    rows: *mut [u64; 16],
    count: usize,
    side: u32,
    valid: bool,
    original: *const u8,
    original_bytes: usize,
    copied: *const u8,
    copied_bytes: usize,
    stats: *mut [u64; 16],
) -> bool {
    let rows = if rows.is_null() || count == 0 { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(rows, count) } };
    full_t5_rows(rows, side, valid, bytes(original, original_bytes), bytes(copied, copied_bytes), unsafe { &mut *stats })
}

// ---- Proof ----------------------------------------------------------------------------------------------------------

/// Proof's targets by slot (colors, depth in 4; the imported ones in `imported`): each native target and the chain's array.
#[repr(C)]
pub struct ProofTargets {
    pub native: [*mut core::ffi::c_void; 5],
    pub array: [*mut core::ffi::c_void; 5],
    pub imported: u32,
    /// The eye's array slice (EyeIndex(side)).
    pub eye: u32,
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
    core::mem::size_of::<ProofTargets>() == 88 && core::mem::size_of::<ProofOut>() == 32 && core::mem::size_of::<ProofHost>() == 24
);
fn borrowed_resource(address: *mut core::ffi::c_void) -> Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource> {
    use windows::core::Interface;
    // SAFETY: a live resource the caller holds for the call.
    unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&address) }.cloned()
}
/// Proof's planes: per imported slot and plane the native and the array's eye-slice footprints, each after the last
/// (512-aligned), their rows the native height and their row bytes whole pixels (colors 4 bytes; depth 4 or 8, stencil
/// 1 or 8 at component 4); None on a footprint the proof refuses, a full room or more than 512 MiB.
pub fn proof_planes(device: &windows::Win32::Graphics::Direct3D12::ID3D12Device, t: &ProofTargets, out: &mut ProofOut) -> Option<u64> {
    let mut size = 0u64;
    out.count = 0;
    for slot in 0..5u32 {
        if t.imported & (1 << slot) == 0 {
            continue;
        }
        // SAFETY: live resources.
        let native = unsafe { borrowed_resource(t.native[slot as usize])?.GetDesc() };
        let array = unsafe { borrowed_resource(t.array[slot as usize])?.GetDesc() };
        for plane in 0..if slot == 4 { 2u32 } else { 1 } {
            let mut row = Plane { slot, plane, ..Plane::default() };
            let (mut rows, mut bytes, mut total) = ([0u32; 2], [0u64; 2], 0u64);
            for (version, (desc, sub)) in [(&native, plane), (&array, t.eye + plane * 2)].into_iter().enumerate() {
                // SAFETY: a live device; the outputs are locals.
                unsafe {
                    device.GetCopyableFootprints(desc, sub, 1, size, Some(&mut row.layouts[version]), Some(&mut rows[version]), Some(&mut bytes[version]), Some(&mut total))
                };
                if total == u64::MAX || size > u64::MAX - total {
                    return None;
                }
                size = (size + total + 511) & !511;
            }
            if rows[0] != rows[1] || rows[0] != native.Height || bytes[0] != bytes[1] || native.Width == 0 || bytes[0] % native.Width != 0 || bytes[0] / native.Width > u64::from(u32::MAX) {
                return None;
            }
            row.rows = rows[0];
            row.stride = (bytes[0] / native.Width) as u32;
            row.bytes = if slot == 4 && plane != 0 { 1 } else { 4 };
            row.component = if slot == 4 && plane != 0 && row.stride == 8 { 4 } else { 0 };
            if (slot == 4 && row.stride != 4 && row.stride != 8 && !(plane != 0 && row.stride == 1)) || (slot < 4 && row.stride != 4) || out.count >= out.capacity {
                return None;
            }
            // SAFETY: the C++ half's room for `capacity` planes.
            unsafe { *out.planes.add(out.count) = row };
            out.count += 1;
        }
    }
    (size != 0 && size <= 512 * 1024 * 1024).then_some(size)
}
/// Proof: the planes, the readback, and each imported slot's targets kept, to COPY_SOURCE, its planes' two copies and the
/// targets back.
///
/// # Safety
/// The targets are live in their render states (the depth DEPTH_WRITE); the list is recording.
pub unsafe fn proof(
    device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
    list: &windows::Win32::Graphics::Direct3D12::ID3D12GraphicsCommandList,
    t: &ProofTargets,
    out: &mut ProofOut,
    host: &ProofHost,
) -> bool {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::*;
    let Some(size) = proof_planes(device, t, out) else { return false };
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
    // SAFETY: the room's planes, written above.
    let planes = unsafe { core::slice::from_raw_parts(out.planes, out.count) };
    for slot in 0..5u32 {
        if t.imported & (1 << slot) == 0 {
            continue;
        }
        let (native, array) = (t.native[slot as usize], t.array[slot as usize]);
        // SAFETY: as above.
        if !unsafe { (host.keep)(host.user, list.as_raw(), [native, array].as_ptr(), 2) } {
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
        let mut subs = vec![(native, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES)];
        for plane in 0..if slot == 4 { 2u32 } else { 1 } {
            subs.push((array, t.eye + plane * 2));
        }
        let to_copy: Vec<_> = subs.iter().map(|&(r, s)| barrier(r, s, state, D3D12_RESOURCE_STATE_COPY_SOURCE)).collect();
        let back: Vec<_> = subs.iter().map(|&(r, s)| barrier(r, s, D3D12_RESOURCE_STATE_COPY_SOURCE, state)).collect();
        // SAFETY: the list recording; the resources are the caller's.
        unsafe {
            list.ResourceBarrier(&to_copy);
            for plane in planes.iter().filter(|p| p.slot == slot) {
                for version in 0..2usize {
                    let from = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: unowned(if version != 0 { array } else { native }),
                        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: if version != 0 { t.eye + plane.plane * 2 } else { plane.plane } },
                    };
                    let to = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: unowned(raw),
                        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: plane.layouts[version] },
                    };
                    list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
                }
            }
            list.ResourceBarrier(&back);
        }
    }
    true
}
/// Proof.
///
/// # Safety
/// As proof; `out` holds its room.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_proof(
    device: *mut core::ffi::c_void,
    list: *mut core::ffi::c_void,
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
    unsafe { proof(&device, &list, &*t, &mut *out, &*host) }
}

#[cfg(test)]
mod tests {
    //! Each output, the bounds and each row verdict.
    use super::*;

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
        let (native, array) = (set(1, 4), set(2, 4));
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let heap: ID3D12DescriptorHeap =
            unsafe { device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 2, ..Default::default() }) }.unwrap();
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
        let start = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        // The native color red; only the array's eye slice 1 green.
        let slice = D3D12_RENDER_TARGET_VIEW_DESC {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
            Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 { Texture2DArray: D3D12_TEX2D_ARRAY_RTV { MipSlice: 0, FirstArraySlice: 1, ArraySize: 1, PlaneSlice: 0 } },
        };
        unsafe {
            device.CreateRenderTargetView(&native[0], None, start);
            list.ClearRenderTargetView(start, &[1.0, 0.0, 0.0, 1.0], None);
            let second = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + stride };
            device.CreateRenderTargetView(&array[0], Some(&slice), second);
            list.ClearRenderTargetView(second, &[0.0, 1.0, 0.0, 1.0], None);
        }
        let raw = |v: &[ID3D12Resource]| -> [*mut c_void; 5] { core::array::from_fn(|i| v[i].as_raw()) };
        // Colors 0 and 2 and the depth imported; slot 1 not.
        let targets = ProofTargets { native: raw(&native), array: raw(&array), imported: 1 | 4 | 16, eye: 1 };
        unsafe extern "C-unwind" fn keep(_: *mut c_void, _: *mut c_void, _: *const *mut c_void, _: usize) -> bool {
            true
        }
        unsafe extern "C-unwind" fn internal(_: *mut c_void) {}
        let host = ProofHost { user: core::ptr::null_mut(), keep, internal };
        let mut room = [Plane::default(); 8];
        let mut out = ProofOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut() };
        assert!(unsafe { proof(&device, &list, &targets, &mut out, &host) });
        let readback = unsafe { ID3D12Resource::from_raw(out.readback) };
        assert_eq!(out.count, 4, "colors 0 and 2, the depth and the stencil");
        let planes = &room[..4];
        assert_eq!((planes[0].slot, planes[1].slot, planes[2].slot, planes[3].plane, planes[0].stride, planes[3].bytes), (0, 2, 4, 1, 4, 1));
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
            let at = (planes[0].layouts[version].Offset + 3 * u64::from(planes[0].layouts[version].Footprint.RowPitch) + 28) as usize;
            [image[at], image[at + 1], image[at + 2], image[at + 3]]
        };
        assert_eq!((pixel(0), pixel(1)), ([255, 0, 0, 255], [0, 255, 0, 255]), "the native target and the array's eye slice");
        unsafe { readback.Unmap(0, None) };
        let short = set(2, 2);
        let mut wrong = ProofOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut() };
        assert!(proof_planes(&device, &ProofTargets { array: raw(&short), ..targets }, &mut wrong).is_none(), "an array of other rows");
        let mut small = ProofOut { planes: room.as_mut_ptr(), capacity: 3, count: 0, readback: core::ptr::null_mut() };
        assert!(proof_planes(&device, &targets, &mut small).is_none(), "a full room");
        let mut none = ProofOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut() };
        assert!(proof_planes(&device, &ProofTargets { imported: 0, ..targets }, &mut none).is_none(), "nothing imported");
    }

    fn plane(slot: u32, index: u32, stride: u32, bytes: u32, at: [u64; 2]) -> Plane {
        let mut p = Plane { rows: 2, stride, component: 1, bytes, slot, plane: index, ..Default::default() };
        for (layout, offset) in p.layouts.iter_mut().zip(at) {
            layout.Offset = offset;
            layout.Footprint.RowPitch = 16;
        }
        p
    }
    #[test]
    fn planes() {
        let mut image = vec![0u8; 64];
        image[32 + 1 + 4] = 1; // pixel 1 of row 0
        image[32 + 16 + 1 + 8] = 1; // pixel 2 of row 1
        for (slot, index, side, output) in [(0u32, 0u32, 0u32, 0usize), (2, 0, 1, 8), (4, 0, 0, 4), (4, 1, 1, 11)] {
            let (mut different, mut compared) = ([0u64; 12], [0u64; 2]);
            assert!(plane_differences(&image, side, &plane(slot, index, 4, 2, [0, 32]), 3, &mut different, &mut compared));
            assert_eq!(different[output], 2, "{slot} {index}");
            assert_eq!(different.iter().sum::<u64>(), 2);
            assert_eq!(compared[side as usize], 12);
        }
        // Two bytes from the component: byte 1 is compared, byte 3 is not.
        let mut image = vec![0u8; 64];
        image[32 + 3] = 1;
        let (mut different, mut compared) = ([0u64; 12], [0u64; 2]);
        assert!(plane_differences(&image, 0, &plane(0, 0, 4, 2, [0, 32]), 3, &mut different, &mut compared));
        assert_eq!(different[0], 0);
        // The copy's last row ends at 32 + 16 + 1 + 2 * 4 + 2 = 59; five more bytes fit, six do not.
        let image = vec![0u8; 64];
        let (mut different, mut compared) = ([0u64; 12], [0u64; 2]);
        assert!(plane_differences(&image, 0, &plane(0, 0, 4, 2, [0, 37]), 3, &mut different, &mut compared));
        assert!(!plane_differences(&image, 0, &plane(0, 0, 4, 2, [0, 38]), 3, &mut different, &mut compared));
        assert!(!plane_differences(&image, 2, &plane(0, 0, 4, 2, [0, 32]), 3, &mut different, &mut compared));
        let (mut different, mut compared) = ([0u64; 12], [0u64; 2]);
        assert!(plane_differences(&image, 0, &plane(0, 0, 4, 2, [0, 32]), 0, &mut different, &mut compared));
        assert_eq!(compared, [0, 0]);
    }
    fn row(side: u64, private: u64, original: u64, bytes: u64) -> [u64; 16] {
        let mut r = [0u64; 16];
        r[1] = side;
        r[8] = private;
        r[9] = bytes;
        r[12] = original;
        r[14] = 1;
        r[15] = u64::MAX;
        r
    }
    #[test]
    fn full_t5() {
        let original: Vec<u8> = (0..32).collect();
        let mut copied: Vec<u8> = (0..32).collect();
        copied[20] = 0;
        copied[22] = 0;
        let mut rows = [row(0, 0, 0, 16), row(1, 0, 0, 32), row(0, 16, 16, 16)];
        let mut stats = [0u64; 16];
        assert!(full_t5_rows(&mut rows, 0, true, &original, &copied, &mut stats));
        assert_eq!((rows[0][14], rows[0][15]), (2, u64::MAX));
        assert_eq!((rows[2][14], rows[2][15]), (3, 4));
        assert_eq!(rows[1][14], 1);
        assert_eq!((stats[9], stats[10]), (2, 2));
        for bad in [row(0, 17, 0, 16), row(0, 0, 17, 16), row(0, 33, 0, 0), row(0, 0, 0, 33)] {
            let mut rows = [bad];
            assert!(!full_t5_rows(&mut rows, 0, true, &original, &copied, &mut [0; 16]));
        }
        let mut done = row(0, 0, 0, 16);
        done[14] = 2;
        assert!(!full_t5_rows(&mut [done], 0, true, &original, &copied, &mut [0; 16]));
        assert!(!full_t5_rows(&mut [row(0, 0, 0, 16)], 0, false, &[], &[], &mut [0; 16]));
        assert!(!full_t5_rows(&mut [], 0, false, &[], &[], &mut [0; 16]));
        assert!(full_t5_rows(&mut [row(1, 0, 0, 16)], 0, true, &[], &[], &mut [0; 16]));
    }
}
