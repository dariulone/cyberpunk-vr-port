//! Single pass's native output copy (formerly src/Render/NativeOutputCopy.cpp): the paired render's two-layer output
//! arrays (two colour formats, R8G8B8A8, depth-stencil D32S8) copied into each native side's own targets -- under the
//! caller's predicate, so a zero skips only the image writes -- then every target plane read back for diagnostics,
//! the targets left in their final states. Only the physical copy inputs are checked here (shapes, formats, planes,
//! that no target aliases a source or another target, that a placed target fits its heap); the caller separately
//! authenticates allocation lifetime, ordering, residency and states. No CPU waits, submission, mapping or
//! registration. Retain the object until its submitted fence completes.
use core::ffi::c_void;
use std::sync::Mutex;

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, E_UNEXPECTED};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SLOTS: [u32; 4] = [0, 1, 2, 4];
const FORMATS: [DXGI_FORMAT; 4] = [
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R32G8X24_TYPELESS,
];
const MAX_READBACK_BYTES: u64 = 512 * 1024 * 1024;

/// One native target, as the C++ wrapper hands it over (constants::ResourceCreationInfo's receipt, borrowed).
#[repr(C)]
pub struct TargetIn {
    pub kind: u32,
    pub resource: *mut c_void,
    pub heap: *mut c_void,
    pub heap_offset: u64,
    pub allocation_bytes: u64,
    pub plane_count: u32,
    pub final_states: [D3D12_RESOURCE_STATES; 2],
}
/// NativeOutputCopy::Plane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub side: u32,
    pub slot: u32,
    pub plane: u32,
    pub footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    pub rows: u32,
    pub stride: u32,
    pub component: u32,
    pub bytes: u32,
}
const _: () = assert!(core::mem::size_of::<Plane>() == 64);

/// One native target (owned references).
pub struct Target {
    pub kind: u32,
    pub resource: ID3D12Resource,
    pub heap: Option<ID3D12Heap>,
    pub heap_offset: u64,
    pub allocation_bytes: u64,
    pub plane_count: u32,
    pub final_states: [D3D12_RESOURCE_STATES; 2],
}
struct State {
    device: ID3D12Device,
    arrays: [ID3D12Resource; 4],
    targets: Vec<Target>, // side * 4 + slot
    predicate: ID3D12Resource,
    readback: ID3D12Resource,
    planes: [Plane; 10],
    predicate_offset: u64,
    bytes: u64,
    recorded: bool,
}
// SAFETY: free-threaded D3D12 objects under the mutex.
unsafe impl Send for State {}

/// NativeOutputCopy.
#[derive(Default)]
pub struct OutputCopy {
    state: Mutex<Option<State>>,
}

fn same_device(device: &ID3D12Device, child: &impl Interface) -> bool {
    let Ok(child) = child.cast::<ID3D12DeviceChild>() else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: the live child.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
fn same_properties(a: &D3D12_HEAP_PROPERTIES, b: &D3D12_HEAP_PROPERTIES) -> bool {
    a.Type == b.Type
        && a.CPUPageProperty == b.CPUPageProperty
        && a.MemoryPoolPreference == b.MemoryPoolPreference
        && a.CreationNodeMask == b.CreationNodeMask
        && a.VisibleNodeMask == b.VisibleNodeMask
}
fn image_shape(device: &ID3D12Device, resource: &ID3D12Resource, compact: usize, layers: u16) -> Option<D3D12_RESOURCE_DESC> {
    if !same_device(device, resource) {
        return None;
    }
    // SAFETY: the live resource and device.
    let desc = unsafe { resource.GetDesc() };
    let depth = compact == 3;
    let needed = if depth {
        D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
    } else {
        D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
    };
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.Width == 0
        || desc.Width > u32::MAX as u64
        || desc.Height == 0
        || desc.DepthOrArraySize != layers
        || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1
        || desc.SampleDesc.Quality != 0
        || desc.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
        || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS).0 != 0
        || (desc.Flags & needed).0 == 0
        || (desc.Format != FORMATS[compact] && !(depth && desc.Format == DXGI_FORMAT_D32_FLOAT_S8X24_UINT))
    {
        return None;
    }
    let mut format = D3D12_FEATURE_DATA_FORMAT_INFO {
        Format: desc.Format,
        PlaneCount: 0,
    };
    // SAFETY: the live device and an out-structure of the announced size.
    let ok = unsafe {
        device.CheckFeatureSupport(
            D3D12_FEATURE_FORMAT_INFO,
            &mut format as *mut _ as *mut c_void,
            core::mem::size_of::<D3D12_FEATURE_DATA_FORMAT_INFO>() as u32,
        )
    }
    .is_ok();
    (ok && format.PlaneCount as u32 == if depth { 2 } else { 1 }).then_some(desc)
}
/// A final state the copy may leave a target in: reads, or exactly one write.
fn valid_state(state: D3D12_RESOURCE_STATES, depth: bool, flags: D3D12_RESOURCE_FLAGS) -> bool {
    let shader = (D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE).0 as u32;
    let reads = shader | D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32 | if depth { D3D12_RESOURCE_STATE_DEPTH_READ.0 as u32 } else { 0 };
    let writes = D3D12_RESOURCE_STATE_COPY_DEST.0 as u32
        | if depth {
            D3D12_RESOURCE_STATE_DEPTH_WRITE.0 as u32
        } else {
            D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32
        }
        | if !depth && (flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0 {
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32
        } else {
            0
        };
    let value = state.0 as u32;
    let write = value & writes;
    if value & !(reads | writes) != 0 || ((flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE).0 != 0 && value & shader != 0) {
        return false;
    }
    write == 0 || (write & (write - 1) == 0 && value == write)
}
/// A committed target, or a placed one that fits its heap with the alignment the device requires.
fn physical(device: &ID3D12Device, t: &Target, desc: &D3D12_RESOURCE_DESC) -> bool {
    if t.kind == 1 {
        return t.heap.is_none() && t.heap_offset == 0 && t.allocation_bytes == 0;
    }
    let Some(heap) = &t.heap else { return false };
    if t.kind != 2 || !same_device(device, heap) || t.allocation_bytes == 0 || t.allocation_bytes == u64::MAX {
        return false;
    }
    // SAFETY: the live heap, resource and device.
    unsafe {
        let hd = heap.GetDesc();
        let required = device.GetResourceAllocationInfo(0, &[*desc]);
        let (mut properties, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
        t.resource.GetHeapProperties(Some(&mut properties), Some(&mut flags)).is_ok()
            && same_properties(&properties, &hd.Properties)
            && flags == hd.Flags
            && required.SizeInBytes != 0
            && required.SizeInBytes != u64::MAX
            && required.Alignment != 0
            && t.heap_offset.is_multiple_of(required.Alignment)
            && t.allocation_bytes >= required.SizeInBytes
            && t.heap_offset <= hd.SizeInBytes
            && t.allocation_bytes <= hd.SizeInBytes - t.heap_offset
    }
}
fn overlap(a: &Target, b: &Target) -> bool {
    if a.resource.as_raw() == b.resource.as_raw() {
        return true;
    }
    let same_heap = match (&a.heap, &b.heap) {
        (Some(x), Some(y)) => x.as_raw() == y.as_raw(),
        (None, None) => true,
        _ => false,
    };
    a.kind == 2 && b.kind == 2 && same_heap && a.heap_offset < b.heap_offset + b.allocation_bytes && b.heap_offset < a.heap_offset + a.allocation_bytes
}
fn barrier(resource: &ID3D12Resource, plane: u32, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> Option<D3D12_RESOURCE_BARRIER> {
    (before != after).then(|| D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: plane,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    })
}

// The slots index the arrays, their descriptions and the targets in step.
#[allow(clippy::needless_range_loop)]
fn create(device: &ID3D12Device, arrays: [ID3D12Resource; 4], targets: Vec<Target>, predicate: &ID3D12Resource, offset: u64) -> Result<State, HRESULT> {
    // SAFETY (whole body): the live device and resources; out-structures on the stack.
    unsafe {
        if device.GetNodeCount() != 1 || !same_device(device, predicate) || !offset.is_multiple_of(8) {
            return Err(E_INVALIDARG);
        }
        let pd = predicate.GetDesc();
        if pd.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || pd.Width < 8 || offset > pd.Width - 8 {
            return Err(E_INVALIDARG);
        }
        let mut array_descs = [D3D12_RESOURCE_DESC::default(); 4];
        for slot in 0..4 {
            array_descs[slot] = image_shape(device, &arrays[slot], slot, 2).ok_or(E_INVALIDARG)?;
            if slot > 0 && (array_descs[slot].Width != array_descs[0].Width || array_descs[slot].Height != array_descs[0].Height) {
                return Err(E_INVALIDARG);
            }
            if (0..slot).any(|prior| arrays[prior].as_raw() == arrays[slot].as_raw()) {
                return Err(E_INVALIDARG);
            }
        }
        let mut target_descs = [D3D12_RESOURCE_DESC::default(); 8];
        for side in 0..2 {
            for slot in 0..4 {
                let index = side * 4 + slot;
                let t = &targets[index];
                let wanted_planes = if slot == 3 { 2 } else { 1 };
                let desc = if t.plane_count == wanted_planes {
                    image_shape(device, &t.resource, slot, 1)
                } else {
                    None
                };
                let Some(desc) = desc else { return Err(E_INVALIDARG) };
                if desc.Width != array_descs[slot].Width || desc.Height != array_descs[slot].Height || !physical(device, t, &desc) {
                    return Err(E_INVALIDARG);
                }
                target_descs[index] = desc;
                if (0..t.plane_count as usize).any(|p| !valid_state(t.final_states[p], slot == 3, desc.Flags)) {
                    return Err(E_INVALIDARG);
                }
                if arrays.iter().any(|a| a.as_raw() == t.resource.as_raw()) {
                    return Err(E_INVALIDARG);
                }
                if (0..index).any(|prior| overlap(t, &targets[prior])) {
                    return Err(E_INVALIDARG);
                }
            }
        }
        let mut planes = [Plane::default(); 10];
        let (mut index, mut total) = (0usize, 0u64);
        for side in 0..2u32 {
            for slot in 0..4usize {
                let desc = target_descs[side as usize * 4 + slot];
                for plane in 0..targets[side as usize * 4 + slot].plane_count {
                    let r = &mut planes[index];
                    index += 1;
                    r.side = side;
                    r.slot = SLOTS[slot];
                    r.plane = plane;
                    let (mut row_bytes, mut required) = (0u64, 0u64);
                    device.GetCopyableFootprints(
                        &desc,
                        plane,
                        1,
                        total,
                        Some(&mut r.footprint),
                        Some(&mut r.rows),
                        Some(&mut row_bytes),
                        Some(&mut required),
                    );
                    let f = r.footprint;
                    if required == u64::MAX
                        || r.rows == 0
                        || r.rows != desc.Height
                        || row_bytes == 0
                        || row_bytes % desc.Width != 0
                        || f.Offset < total
                        || f.Offset % 512 != 0
                        || f.Footprint.Depth != 1
                        || f.Footprint.Width as u64 != desc.Width
                        || f.Footprint.Height != desc.Height
                        || (f.Footprint.RowPitch as u64) < row_bytes
                        || row_bytes / desc.Width > u32::MAX as u64
                    {
                        return Err(E_INVALIDARG);
                    }
                    r.stride = (row_bytes / desc.Width) as u32;
                    r.bytes = if slot == 3 && plane == 1 { 1 } else { 4 };
                    r.component = if slot == 3 && plane == 1 && r.stride == 8 { 4 } else { 0 };
                    let stride_ok = if slot == 3 {
                        r.stride == 4 || r.stride == 8 || (plane == 1 && r.stride == 1)
                    } else {
                        r.stride == 4
                    };
                    if !stride_ok || r.component + r.bytes > r.stride {
                        return Err(E_INVALIDARG);
                    }
                    let payload = (r.rows as u64 - 1) * f.Footprint.RowPitch as u64 + row_bytes;
                    if f.Offset > MAX_READBACK_BYTES || payload > MAX_READBACK_BYTES - f.Offset {
                        return Err(E_OUTOFMEMORY);
                    }
                    total = (f.Offset + payload + 511) & !511;
                }
            }
        }
        if index != planes.len() || total == 0 || total > MAX_READBACK_BYTES {
            return Err(E_INVALIDARG);
        }
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: total,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let mut readback: Option<ID3D12Resource> = None;
        device
            .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback)
            .map_err(|e| e.code())?;
        Ok(State {
            device: device.clone(),
            arrays,
            targets,
            predicate: predicate.clone(),
            readback: readback.ok_or(E_INVALIDARG)?,
            planes,
            predicate_offset: offset,
            bytes: total,
            recorded: false,
        })
    }
}

fn copy_location(resource: &ID3D12Resource, sub: u32) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
        pResource: unsafe { core::mem::transmute_copy(resource) },
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub },
    }
}

impl OutputCopy {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<State>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn create(&self, device: &ID3D12Device, arrays: [ID3D12Resource; 4], targets: Vec<Target>, predicate: &ID3D12Resource, offset: u64) -> HRESULT {
        let mut s = self.lock();
        if s.is_some() {
            return E_UNEXPECTED;
        }
        match create(device, arrays, targets, predicate, offset) {
            Ok(state) => {
                *s = Some(state);
                HRESULT(0)
            }
            Err(hr) => hr,
        }
    }
    /// Record: the one recording, on a caller-owned DIRECT list; leaves predication disabled.
    pub fn record(&self, list: &ID3D12GraphicsCommandList) -> bool {
        let mut g = self.lock();
        let Some(s) = g.as_mut() else { return false };
        // SAFETY (whole block): the recording list and live resources.
        unsafe {
            if s.recorded || list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(&s.device, list) {
                return false;
            }
            let target = |p: &Plane| &s.targets[p.side as usize * 4 + if p.slot == 4 { 3 } else { p.slot as usize }];
            let slot_of = |p: &Plane| if p.slot == 4 { 3 } else { p.slot as usize };
            let before: Vec<_> = s
                .planes
                .iter()
                .filter_map(|p| {
                    barrier(
                        &target(p).resource,
                        p.plane,
                        target(p).final_states[p.plane as usize],
                        D3D12_RESOURCE_STATE_COPY_DEST,
                    )
                })
                .collect();
            list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            if !before.is_empty() {
                list.ResourceBarrier(&before);
            }
            list.SetPredication(&s.predicate, s.predicate_offset, D3D12_PREDICATION_OP_EQUAL_ZERO);
            for p in &s.planes {
                let from = copy_location(&s.arrays[slot_of(p)], 1 - p.side + p.plane * 2);
                let to = copy_location(&target(p).resource, p.plane);
                list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            }
            list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            let to_read: Vec<_> = s
                .planes
                .iter()
                .filter_map(|p| barrier(&target(p).resource, p.plane, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE))
                .collect();
            list.ResourceBarrier(&to_read);
            for p in &s.planes {
                let from = copy_location(&target(p).resource, p.plane);
                let to = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: core::mem::transmute_copy(&s.readback),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: p.footprint },
                };
                list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            }
            let after: Vec<_> = s
                .planes
                .iter()
                .filter_map(|p| {
                    barrier(
                        &target(p).resource,
                        p.plane,
                        D3D12_RESOURCE_STATE_COPY_SOURCE,
                        target(p).final_states[p.plane as usize],
                    )
                })
                .collect();
            if !after.is_empty() {
                list.ResourceBarrier(&after);
            }
        }
        s.recorded = true;
        true
    }
}

// ---- the C ABI (include/Render/NativeOutputCopy.hpp)

#[no_mangle]
pub extern "C" fn cvr_output_copy_new() -> *mut OutputCopy {
    Box::into_raw(Box::default())
}
/// # Safety
/// `c` came from cvr_output_copy_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_output_copy_free(c: *mut OutputCopy) {
    if !c.is_null() {
        // SAFETY: the caller's object, freed once.
        drop(unsafe { Box::from_raw(c) });
    }
}
/// # Safety
/// `c` is live; `device`/`predicate` D3D12 objects or null; `arrays` four resource pointers; `targets` eight rows
/// (side-major).
#[no_mangle]
pub unsafe extern "C" fn cvr_output_copy_create(
    c: *const OutputCopy,
    device: *mut c_void,
    arrays: *const [*mut c_void; 4],
    targets: *const [TargetIn; 8],
    predicate: *mut c_void,
    offset: u64,
) -> HRESULT {
    // SAFETY: the caller's objects.
    unsafe {
        let Some(c) = c.as_ref() else { return E_INVALIDARG };
        if c.lock().is_some() {
            return E_UNEXPECTED;
        }
        let (Some(device), Some(predicate), Some(arrays), Some(targets)) = (
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12Resource::from_raw_borrowed(&predicate),
            arrays.as_ref(),
            targets.as_ref(),
        ) else {
            return E_INVALIDARG;
        };
        let mut owned_arrays = Vec::with_capacity(4);
        for a in arrays {
            match ID3D12Resource::from_raw_borrowed(a) {
                Some(r) => owned_arrays.push(r.clone()),
                None => return E_INVALIDARG,
            }
        }
        let mut owned_targets = Vec::with_capacity(8);
        for t in targets {
            let Some(resource) = ID3D12Resource::from_raw_borrowed(&t.resource) else {
                return E_INVALIDARG;
            };
            owned_targets.push(Target {
                kind: t.kind,
                resource: resource.clone(),
                heap: ID3D12Heap::from_raw_borrowed(&t.heap).cloned(),
                heap_offset: t.heap_offset,
                allocation_bytes: t.allocation_bytes,
                plane_count: t.plane_count,
                final_states: t.final_states,
            });
        }
        let Ok(arrays) = <[ID3D12Resource; 4]>::try_from(owned_arrays) else {
            return E_INVALIDARG;
        };
        c.create(device, arrays, owned_targets, predicate, offset)
    }
}
/// # Safety
/// `c` is live; `list` an ID3D12GraphicsCommandList or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_output_copy_record(c: *const OutputCopy, list: *mut c_void) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (c.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list)) {
            (Some(c), Some(l)) => c.record(l),
            _ => false,
        }
    }
}
/// The readback (borrowed, null before Create), its bytes, whether it was recorded, and the ten planes.
///
/// # Safety
/// `c` is live; the out-pointers writable or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_output_copy_info(
    c: *const OutputCopy,
    readback: *mut *mut c_void,
    bytes: *mut u64,
    recorded: *mut bool,
    planes: *mut [Plane; 10],
) {
    // SAFETY: the caller's object and out-pointers.
    unsafe {
        let Some(c) = c.as_ref() else { return };
        let g = c.lock();
        if let Some(o) = readback.as_mut() {
            *o = g.as_ref().map_or(core::ptr::null_mut(), |s| s.readback.as_raw());
        }
        if let Some(o) = bytes.as_mut() {
            *o = g.as_ref().map_or(0, |s| s.bytes);
        }
        if let Some(o) = recorded.as_mut() {
            *o = g.as_ref().is_some_and(|s| s.recorded);
        }
        if let Some(o) = planes.as_mut() {
            *o = g.as_ref().map_or([Plane::default(); 10], |s| s.planes);
        }
    }
}
