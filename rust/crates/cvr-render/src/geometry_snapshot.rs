//! The GPU-position geometry snapshot (src/Render/GeometrySnapshot.cpp): the draw admitted (a DIRECT list; an indexed,
//! instanced, fully described draw of a frame and an eye whose instances come from slot 7 at a 48- or 64-byte stride,
//! inside their views, with proven instance rebasing), each used IA range copied at its original draw position (its
//! lease of the list's device, DEFAULT or UPLOAD, in a known read state holding the slot's required state, the range
//! inside the lease; the instance slot trimmed to the drawn instances; one consistent state per resource; 16-aligned,
//! at most 8 MiB), the private DEFAULT and READBACK buffers, the copy commands (sources to COPY_SOURCE and back, the
//! private buffer read back and left GENERIC_READ), the draw rewritten onto the private buffer, the completed readback
//! and the two eyes' pairing (the same draw shape and every copied byte equal).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the snapshot's COM owner, the leases of the rewritten draw, the
//! command-resource retention, the internal-commands scope and the fence; the unit test below snapshots on WARP, and
//! tools/single_pass_tests/geometry_snapshot_test runs it through the .cpp in its Rust variant.

use crate::geometry_commands::DrawRecord;
use crate::readback_buffers::buffer;
use core::ffi::c_void;
use std::collections::BTreeMap;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT};

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const DXGI_ERROR_DEVICE_REMOVED: i32 = 0x887A_0005u32 as i32;
const LIMIT: u64 = 8 * 1024 * 1024;
const READ_STATES: u32 = (D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0
    | D3D12_RESOURCE_STATE_INDEX_BUFFER.0
    | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0
    | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0
    | D3D12_RESOURCE_STATE_COPY_SOURCE.0
    | D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT.0) as u32;

/// A slot's lease and read state (slot 0 the index buffer, slot n the vertex buffer n-1), lent by the .cpp.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Slot {
    pub resource: *mut c_void,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub status: u32,
    pub value: u32,
    pub _pad: u32,
}
const _: () = assert!(core::mem::size_of::<Slot>() == 40);

struct Copy {
    source: ID3D12Resource,
    from: u64,
    to: u64,
    bytes: u64,
}
/// A snapshot's copies, buffers and bytes.
pub struct Snapshot {
    device: ID3D12Device,
    copies: Vec<Copy>,
    before: BTreeMap<usize, u32>,
    offsets: [u64; 17],
    sizes: [u64; 17],
    pub bytes: u64,
    gpu: Option<ID3D12Resource>,
    readback: Option<ID3D12Resource>,
    cpu: Vec<u8>,
}

fn same_device(device: &ID3D12Device, child: &ID3D12Resource) -> bool {
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live resource; the output a local.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
fn transition(resource: *mut c_void, subresource: u32, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource) },
                Subresource: subresource,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

/// Capture's admission and copy plan (no object created).
pub fn plan(list: &ID3D12GraphicsCommandList, d: &DrawRecord, slots: &[Slot; 17], rebase: bool) -> Result<Snapshot, i32> {
    // SAFETY: a live list.
    if unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || !rebase
        || d.indices == 0
        || d.instances == 0
        || (d.flags & 191) != 191
        || (d.flags & 64) != 0
        || d.frame == 0
        || d.side > 1
        || (d.input_slot_mask & 128) == 0
        || (d.input_slot_mask & !65535u32) != 0
    {
        return Err(E_INVALIDARG);
    }
    let index_bytes = match d.index.Format {
        DXGI_FORMAT_R16_UINT => 2u64,
        DXGI_FORMAT_R32_UINT => 4,
        _ => 0,
    };
    if index_bytes == 0 || (u64::from(d.first_index) + u64::from(d.indices)) * index_bytes > u64::from(d.index.SizeInBytes) {
        return Err(E_INVALIDARG);
    }
    let stride = d.vertices[7].StrideInBytes;
    if (stride != 48 && stride != 64) || (u64::from(d.first_instance) + u64::from(d.instances)) * u64::from(stride) > u64::from(d.vertices[7].SizeInBytes) {
        return Err(E_INVALIDARG);
    }
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list; the output a local.
    if let Err(error) = unsafe { list.GetDevice(&mut device) } {
        return Err(error.code().0);
    }
    let device = device.ok_or(E_FAIL)?;
    let mut s = Snapshot { device, copies: Vec::new(), before: BTreeMap::new(), offsets: [0; 17], sizes: [0; 17], bytes: 0, gpu: None, readback: None, cpu: Vec::new() };
    for slot in 0..17usize {
        if slot != 0 && d.input_slot_mask & (1u32 << (slot - 1)) == 0 {
            continue;
        }
        let lease = &slots[slot];
        let address = if slot != 0 { d.vertices[slot - 1].BufferLocation } else { d.index.BufferLocation };
        let skip = if slot == 8 { u64::from(d.first_instance) * u64::from(stride) } else { 0 };
        let bytes = match slot {
            8 => u64::from(d.instances) * u64::from(stride),
            0 => u64::from(d.index.SizeInBytes),
            _ => u64::from(d.vertices[slot - 1].SizeInBytes),
        };
        let required = if slot != 0 { D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0 } else { D3D12_RESOURCE_STATE_INDEX_BUFFER.0 } as u32;
        // SAFETY: the .cpp lends the lease's resource for the call.
        let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&lease.resource) }).cloned() else { return Err(E_INVALIDARG) };
        if !same_device(&s.device, &resource)
            || !matches!(lease.status, 1 | 2 | 8 | 9)
            || (lease.value & required) != required
            || (lease.value & !READ_STATES) != 0
            || bytes == 0
            || address < lease.gpu
            || address - lease.gpu > lease.bytes
            || skip > lease.bytes - (address - lease.gpu)
            || bytes > lease.bytes - (address - lease.gpu) - skip
        {
            return Err(E_INVALIDARG);
        }
        if lease.heap_type != D3D12_HEAP_TYPE_DEFAULT.0 as u32 && lease.heap_type != D3D12_HEAP_TYPE_UPLOAD.0 as u32 {
            return Err(E_INVALIDARG);
        }
        if lease.heap_type == D3D12_HEAP_TYPE_UPLOAD.0 as u32 && lease.value != D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32 {
            return Err(E_INVALIDARG);
        }
        if *s.before.entry(lease.resource as usize).or_insert(lease.value) != lease.value {
            return Err(E_INVALIDARG);
        }
        let offset = (s.bytes + 15) & !15u64;
        if offset > LIMIT || bytes > LIMIT - offset {
            return Err(E_OUTOFMEMORY);
        }
        s.copies.push(Copy { source: resource, from: address - lease.gpu + skip, to: offset, bytes });
        s.offsets[slot] = offset;
        s.sizes[slot] = bytes;
        s.bytes = offset + bytes;
    }
    Ok(s)
}
impl Snapshot {
    /// The private DEFAULT and READBACK buffers; the draw rewritten onto the private copy (its instances from 0).
    pub fn create(&mut self, d: &mut DrawRecord) -> Result<ID3D12Resource, i32> {
        let gpu = buffer(&self.device, None, self.bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST).map_err(|e| e.0)?;
        self.readback = Some(buffer(&self.device, None, self.bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).map_err(|e| e.0)?);
        // SAFETY: a live buffer.
        let address = unsafe { gpu.GetGPUVirtualAddress() };
        d.index.BufferLocation = address + self.offsets[0];
        for slot in 0..16usize {
            if d.input_slot_mask & (1u32 << slot) != 0 {
                d.vertices[slot].BufferLocation = address + self.offsets[slot + 1];
                d.vertices[slot].SizeInBytes = self.sizes[slot + 1] as u32;
            }
        }
        d.first_instance = 0;
        self.gpu = Some(gpu.clone());
        Ok(gpu)
    }
    /// The copy commands: the sources to COPY_SOURCE (those not already in it) and back, the private buffer read back.
    pub fn record(&mut self, list: &ID3D12GraphicsCommandList) {
        let (Some(gpu), Some(readback)) = (&self.gpu, &self.readback) else { return };
        let mut barriers: Vec<D3D12_RESOURCE_BARRIER> = self
            .before
            .iter()
            .filter(|(_, state)| **state & D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32 == 0)
            .map(|(&resource, &state)| transition(resource as *mut c_void, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_STATES(state as i32), D3D12_RESOURCE_STATE_COPY_SOURCE))
            .collect();
        // SAFETY: a live list; the barriers borrow live resources (the copies hold them).
        unsafe {
            if !barriers.is_empty() {
                list.ResourceBarrier(&barriers);
            }
            for copy in &self.copies {
                list.CopyBufferRegion(gpu, copy.to, &copy.source, copy.from, copy.bytes);
            }
            for barrier in &mut barriers {
                let t = &mut *barrier.Anonymous.Transition;
                core::mem::swap(&mut t.StateBefore, &mut t.StateAfter);
            }
            if !barriers.is_empty() {
                list.ResourceBarrier(&barriers);
            }
            list.ResourceBarrier(&[transition(gpu.as_raw(), 0, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            list.CopyResource(readback, gpu);
            list.ResourceBarrier(&[transition(gpu.as_raw(), 0, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_GENERIC_READ)]);
        }
        self.before.clear();
    }
    /// Read: S_FALSE until the sealed submission completes; then the private bytes read back (the copies released).
    pub fn read(&mut self, fence: &ID3D12Fence, value: u64) -> i32 {
        // SAFETY: a live fence.
        let completed = unsafe { fence.GetCompletedValue() };
        if completed == u64::MAX {
            return DXGI_ERROR_DEVICE_REMOVED;
        }
        if completed < value {
            return S_FALSE;
        }
        let Some(readback) = &self.readback else { return E_FAIL };
        let mut bytes = vec![0u8; self.bytes as usize];
        let mut mapped = core::ptr::null_mut();
        // SAFETY: a live READBACK buffer of `bytes`.
        if let Err(error) = unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: bytes.len() }), Some(&mut mapped)) } {
            return error.code().0;
        }
        if mapped.is_null() {
            // SAFETY: as above.
            unsafe { readback.Unmap(0, None) };
            return E_FAIL;
        }
        // SAFETY: the mapping holds `bytes` readable bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(mapped as *const u8, bytes.as_mut_ptr(), bytes.len());
            readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
        }
        self.cpu = bytes;
        self.copies.clear();
        S_OK
    }
    /// The read instance range.
    pub fn instances(&self) -> &[u8] {
        let (at, n) = (self.offsets[8] as usize, self.sizes[8] as usize);
        self.cpu.get(at..at + n).unwrap_or(&[])
    }
    fn range(&self, slot: usize) -> Option<&[u8]> {
        let (at, n) = (self.offsets[slot] as usize, self.sizes[slot] as usize);
        self.cpu.get(at..at + n)
    }
}
/// Pair: the second eye's draw (`x`, side 1) and the main eye's (`y`, side 0) of one shape, every copied byte equal.
pub fn pair(a: &Snapshot, x: &DrawRecord, b: &Snapshot, y: &DrawRecord) -> bool {
    if x.side != 1
        || y.side != 0
        || x.frame != y.frame
        || x.pipeline != y.pipeline
        || x.root != y.root
        || x.indices != y.indices
        || x.instances != y.instances
        || x.first_index != y.first_index
        || x.base_vertex != y.base_vertex
        || x.input_slot_mask != y.input_slot_mask
        || x.index.Format != y.index.Format
        || a.sizes != b.sizes
    {
        return false;
    }
    (0..17usize).filter(|&slot| a.sizes[slot] != 0).all(|slot| {
        (slot == 0 || x.vertices[slot - 1].StrideInBytes == y.vertices[slot - 1].StrideInBytes) && a.range(slot).is_some_and(|bytes| Some(bytes) == b.range(slot))
    })
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Capture's admission and plan: a new snapshot into `out`, its bytes into `bytes`.
///
/// # Safety
/// A live list; `draw` and the 17 slots lent for the call; the outputs pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gsn_plan(list: *mut c_void, draw: *const DrawRecord, slots: *const [Slot; 17], rebase: bool, out: *mut *mut Snapshot, bytes: *mut u64) -> i32 {
    // SAFETY: as documented.
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return E_INVALIDARG };
    match plan(list, unsafe { &*draw }, unsafe { &*slots }, rebase) {
        Ok(snapshot) => {
            // SAFETY: as documented.
            unsafe {
                *bytes = snapshot.bytes;
                *out = Box::into_raw(Box::new(snapshot));
            }
            S_OK
        }
        Err(hr) => hr,
    }
}
/// Releases a snapshot (null: nothing).
///
/// # Safety
/// From cvr_gsn_plan, once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gsn_free(snapshot: *mut Snapshot) {
    if !snapshot.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(snapshot) });
    }
}
/// The private buffers; `draw` rewritten onto the private copy; the DEFAULT buffer into `gpu` (borrowed).
///
/// # Safety
/// A live snapshot; `draw` writable; `gpu` a pointer.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gsn_create(snapshot: *mut Snapshot, draw: *mut DrawRecord, gpu: *mut *mut c_void) -> i32 {
    // SAFETY: as documented.
    match unsafe { (*snapshot).create(&mut *draw) } {
        Ok(resource) => {
            // SAFETY: as documented; the snapshot keeps its own reference.
            unsafe { *gpu = resource.as_raw() };
            S_OK
        }
        Err(hr) => hr,
    }
}
/// The copy commands.
///
/// # Safety
/// A live snapshot (after cvr_gsn_create) and list.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gsn_record(snapshot: *mut Snapshot, list: *mut c_void) {
    // SAFETY: as documented.
    if let Some(list) = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) } {
        unsafe { (*snapshot).record(list) }
    }
}
/// Read: S_OK with the instance range into `instances` / `count`, S_FALSE while pending, or the failure.
///
/// # Safety
/// A live snapshot and fence; the outputs pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gsn_read(snapshot: *mut Snapshot, fence: *mut c_void, value: u64, instances: *mut *const u8, count: *mut usize) -> i32 {
    // SAFETY: as documented.
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }) else { return E_INVALIDARG };
    let snapshot = unsafe { &mut *snapshot };
    let hr = snapshot.read(fence, value);
    if hr == S_OK {
        let range = snapshot.instances();
        // SAFETY: as documented; the range lives with the snapshot.
        unsafe {
            *instances = range.as_ptr();
            *count = range.len();
        }
    }
    hr
}
/// Pair's shape and bytes.
///
/// # Safety
/// Live, read snapshots; the draws lent for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gsn_pair(a: *const Snapshot, x: *const DrawRecord, b: *const Snapshot, y: *const DrawRecord) -> bool {
    // SAFETY: as documented.
    unsafe { pair(&*a, &*x, &*b, &*y) }
}

#[cfg(test)]
mod tests {
    //! A snapshot on WARP: the refusals, the trimmed instance range, the copies read back after the source changed, and
    //! the pairing.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn warp() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn upload(device: &ID3D12Device, bytes: &[u8]) -> ID3D12Resource {
        let resource = buffer(device, None, bytes.len() as u64, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let mut mapped = core::ptr::null_mut();
        unsafe { resource.Map(0, None, Some(&mut mapped)) }.unwrap();
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped as *mut u8, bytes.len()) };
        unsafe { resource.Unmap(0, None) };
        resource
    }
    fn slot(resource: &ID3D12Resource) -> Slot {
        Slot {
            resource: resource.as_raw(),
            gpu: unsafe { resource.GetGPUVirtualAddress() },
            bytes: unsafe { resource.GetDesc() }.Width,
            heap_type: D3D12_HEAP_TYPE_UPLOAD.0 as u32,
            status: 1,
            value: D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32,
            _pad: 0,
        }
    }
    #[test]
    fn snapshots() {
        let device = warp();
        let geometry: Vec<u8> = (0..1024u32).map(|i| (i * 13) as u8).collect();
        let instances: Vec<u8> = (0..1024u32).map(|i| (i * 7 + 1) as u8).collect();
        let (ib_vb, inst) = (upload(&device, &geometry), upload(&device, &instances));
        let mut slots = [Slot { resource: core::ptr::null_mut(), gpu: 0, bytes: 0, heap_type: 0, status: 0, value: 0, _pad: 0 }; 17];
        slots[0] = slot(&ib_vb);
        slots[1] = slot(&ib_vb);
        slots[8] = slot(&inst);
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        let base = unsafe { ib_vb.GetGPUVirtualAddress() };
        d.frame = 1;
        d.side = 1;
        d.pipeline = 1;
        d.root = 2;
        d.indices = 3;
        d.instances = 2;
        d.first_instance = 3;
        d.flags = 191;
        d.input_slot_mask = 129;
        d.index = D3D12_INDEX_BUFFER_VIEW { BufferLocation: base + 64, SizeInBytes: 24, Format: DXGI_FORMAT_R16_UINT };
        d.vertices[0] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: base + 256, SizeInBytes: 256, StrideInBytes: 16 };
        d.vertices[7] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: unsafe { inst.GetGPUVirtualAddress() }, SizeInBytes: 1024, StrideInBytes: 48 };
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        assert_eq!(plan(&list, &d, &slots, false).err(), Some(E_INVALIDARG), "unproven instance rebasing");
        let mut bad = slots;
        bad[0].status = 5;
        assert_eq!(plan(&list, &d, &bad, true).err(), Some(E_INVALIDARG), "an unknown read state");
        let mut bad = slots;
        bad[1].value = D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0 as u32;
        assert_eq!(plan(&list, &d, &bad, true).err(), Some(E_INVALIDARG), "an UPLOAD lease outside GENERIC_READ");
        let mut left = plan(&list, &d, &slots, true).unwrap();
        assert_eq!((left.bytes, left.sizes[8], left.offsets[1]), (384, 96, 32), "the instance slot trimmed, ranges 16-aligned");
        let mut x = d;
        left.create(&mut x).unwrap();
        assert_eq!((x.first_instance, x.vertices[7].SizeInBytes), (0, 96), "the draw on the private copy");
        left.record(&list);
        let mut y = d;
        y.side = 0;
        let mut right = plan(&list, &y, &slots, true).unwrap();
        right.create(&mut y).unwrap();
        right.record(&list);
        let mut z = d;
        z.side = 0;
        z.first_instance = 9;
        let mut other = plan(&list, &z, &slots, true).unwrap();
        other.create(&mut z).unwrap();
        other.record(&list);
        unsafe { list.Close() }.unwrap();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        assert_eq!(left.read(&fence, 1), S_FALSE, "pending");
        unsafe { queue.ExecuteCommandLists(&[Some(list.cast().unwrap())]) };
        unsafe { queue.Signal(&fence, 1) }.unwrap();
        while unsafe { fence.GetCompletedValue() } < 1 {}
        for snapshot in [&mut left, &mut right, &mut other] {
            assert_eq!(snapshot.read(&fence, 1), S_OK);
        }
        assert_eq!(left.instances(), &instances[3 * 48..5 * 48], "the drawn instances, read back");
        assert_eq!(left.range(0).unwrap(), &geometry[64..88]);
        assert!(pair(&left, &x, &right, &y), "the same bytes and shape");
        assert!(!pair(&left, &x, &other, &z), "other instances");
        assert!(!pair(&right, &y, &left, &x), "the eyes swapped");
    }
}
