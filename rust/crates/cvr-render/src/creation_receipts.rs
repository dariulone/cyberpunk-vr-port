//! The API creation receipts of the registered constant uploads (src/Render/RegisteredConstantUpload.cpp,
//! `constants::RegisteredResourceCreated`, `QueryResourceCreation` and the placement the copy ledger reads).
//!
//! A created resource carries three private data entries: its placement (32 bytes: the memory span and the kind, 1
//! committed or 2 placed), the placement heap as an interface (the resource keeps its own reference to it) and a 56-byte
//! creation receipt (resource, heap, offset, bytes, kind, whether the initial state is known, that state, a version and
//! an FNV-1a checksum of the first 48 bytes). A query accepts only a receipt whose checksum and version hold, which
//! names this very resource, whose kind is 1 or 2 and whose unknown state is 0; the placement must agree with it; a
//! committed receipt names no memory, a placed one a non-empty span inside its heap, aligned and large enough for the
//! resource on the same device as the heap; both entries are read again and must be unchanged. The heap comes back
//! with the reference the interface retrieval added. `initialState` is historical, never the current state.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's RegisteredResourceCreated (past its gate) and QueryResourceCreation
//! forward to `cvr_crc_*`; the copy ledger (`registered_copies`) reads the placement here;
//! `tools/single_pass_tests/resource_creation_info_test` runs its cases against them in its Rust variant.

use core::ffi::c_void;

use windows::core::{IUnknown, Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;

use crate::constant_timeline::MemorySpan;

const PLACEMENT_KEY: GUID = GUID::from_values(0x4d3815bb, 0x3885, 0x463d, [0x81, 0xc3, 0x48, 0x32, 0x2a, 0xcb, 0x62, 0x51]);
const HEAP_OWNER_KEY: GUID = GUID::from_values(0x4d3815bb, 0x3885, 0x463d, [0x81, 0xc3, 0x48, 0x32, 0x2a, 0xcb, 0x62, 0x52]);
const CREATION_KEY: GUID = GUID::from_values(0x4d3815bb, 0x3885, 0x463d, [0x81, 0xc3, 0x48, 0x32, 0x2a, 0xcb, 0x62, 0x53]);
const CREATION_VERSION: u32 = 0x4352_4931;

/// The placement private data (the .cpp's Placement).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub memory: MemorySpan,
    pub kind: u32,
    pub _pad: u32,
}
const _: () = assert!(core::mem::size_of::<Placement>() == 32);

/// The creation receipt private data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CreationReceipt {
    resource: u64,
    heap: u64,
    offset: u64,
    bytes: u64,
    kind: u32,
    state_known: u32,
    state: u32,
    version: u32,
    checksum: u64,
}
const _: () = assert!(core::mem::size_of::<CreationReceipt>() == 56 && core::mem::offset_of!(CreationReceipt, checksum) == 48);

fn checksum(receipt: &CreationReceipt) -> u64 {
    // SAFETY: the receipt's first 48 bytes, all fields without padding.
    let bytes = unsafe { core::slice::from_raw_parts(receipt as *const CreationReceipt as *const u8, 48) };
    bytes.iter().fold(14695981039346656037u64, |hash, &b| (hash ^ b as u64).wrapping_mul(1099511628211))
}
fn read_creation(r: &ID3D12Resource) -> Option<CreationReceipt> {
    let mut receipt = CreationReceipt::default();
    let mut size = core::mem::size_of::<CreationReceipt>() as u32;
    // SAFETY: room for the receipt.
    let ok = unsafe { r.GetPrivateData(&CREATION_KEY, &mut size, Some(&mut receipt as *mut CreationReceipt as *mut c_void)) }.is_ok();
    (ok && size == 56 && receipt.version == CREATION_VERSION && receipt.checksum == checksum(&receipt)).then_some(receipt)
}

/// PlacementOf: the resource's placement private data, or the empty one.
pub fn placement_of(resource: *mut c_void) -> Placement {
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return Placement::default();
    };
    let mut value = Placement::default();
    let mut size = 32u32;
    // SAFETY: room for the placement.
    let ok = unsafe { r.GetPrivateData(&PLACEMENT_KEY, &mut size, Some(&mut value as *mut Placement as *mut c_void)) }.is_ok();
    if ok && size == 32 {
        value
    } else {
        Placement::default()
    }
}

/// RegisteredResourceCreated past its gate: the placement (with the heap's interface when the span is valid) and the
/// creation receipt.
pub fn resource_created(resource: *mut c_void, placed: bool, heap: *mut c_void, offset: u64, allocation_bytes: u64, state: Option<u32>) {
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return;
    };
    let mut value = Placement { kind: if placed { 2 } else { 1 }, ..Placement::default() };
    // SAFETY: a live heap of the caller's, or null.
    let heap_ref = unsafe { ID3D12Heap::from_raw_borrowed(&heap) };
    if placed && allocation_bytes != 0 && allocation_bytes != u64::MAX && offset <= u64::MAX - allocation_bytes {
        if let Some(h) = heap_ref {
            // SAFETY: the resource keeps its own reference to the heap.
            if unsafe { r.SetPrivateDataInterface(&HEAP_OWNER_KEY, h) }.is_ok() {
                value.memory = MemorySpan { heap: heap as u64, offset, bytes: allocation_bytes };
            }
        }
    }
    // SAFETY: 32 bytes of the placement.
    let _ = unsafe { r.SetPrivateData(&PLACEMENT_KEY, 32, Some(&value as *const Placement as *const c_void)) };
    let mut receipt = CreationReceipt {
        resource: resource as u64,
        heap: value.memory.heap,
        offset: value.memory.offset,
        bytes: value.memory.bytes,
        kind: value.kind,
        state_known: state.is_some() as u32,
        state: state.unwrap_or(0),
        version: CREATION_VERSION,
        checksum: 0,
    };
    receipt.checksum = checksum(&receipt);
    // SAFETY: 56 bytes of the receipt.
    let _ = unsafe { r.SetPrivateData(&CREATION_KEY, 56, Some(&receipt as *const CreationReceipt as *const c_void)) };
}

/// QueryResourceCreation's answer; `heap` owns a reference (or is null for a committed resource).
#[derive(Debug, Default)]
pub struct Creation {
    pub kind: u32,
    pub state: Option<u32>,
    pub heap: Option<ID3D12Heap>,
    pub offset: u64,
    pub bytes: u64,
}

fn same_placement(p: &Placement, receipt: &CreationReceipt) -> bool {
    p.kind == receipt.kind && p.memory.heap == receipt.heap && p.memory.offset == receipt.offset && p.memory.bytes == receipt.bytes
}

/// QueryResourceCreation: the current API creation receipt, or None.
pub fn query(resource: *mut c_void) -> Option<Creation> {
    // SAFETY: a live resource of the caller's, or null.
    let r = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }?;
    let receipt = read_creation(r)?;
    if receipt.resource != resource as u64
        || (receipt.kind != 1 && receipt.kind != 2)
        || receipt.state_known > 1
        || (receipt.state_known == 0 && receipt.state != 0)
    {
        return None;
    }
    if !same_placement(&placement_of(resource), &receipt) {
        return None;
    }
    let mut resource_device: Option<ID3D12Device> = None;
    // SAFETY: a live resource.
    if unsafe { r.GetDevice(&mut resource_device) }.is_err() {
        return None;
    }
    let resource_device = resource_device?;
    let mut result = Creation { kind: receipt.kind, state: (receipt.state_known != 0).then_some(receipt.state), ..Creation::default() };
    if receipt.kind == 1 {
        if receipt.heap != 0 || receipt.offset != 0 || receipt.bytes != 0 {
            return None;
        }
    } else {
        if receipt.heap == 0 || receipt.bytes == 0 || receipt.bytes == u64::MAX || receipt.offset > u64::MAX - receipt.bytes {
            return None;
        }
        let mut raw: *mut c_void = core::ptr::null_mut();
        let mut size = 8u32;
        // SAFETY: the interface private data: an owned IUnknown pointer.
        let ok = unsafe { r.GetPrivateData(&HEAP_OWNER_KEY, &mut size, Some(&mut raw as *mut *mut c_void as *mut c_void)) }.is_ok();
        if !ok || size != 8 || raw.is_null() {
            return None;
        }
        // SAFETY: the reference the interface retrieval added.
        let owner = unsafe { IUnknown::from_raw(raw) };
        let heap = owner.cast::<ID3D12Heap>().ok()?;
        if heap.as_raw() as u64 != receipt.heap {
            return None;
        }
        let mut heap_device: Option<ID3D12Device> = None;
        // SAFETY (whole block): live objects.
        unsafe {
            if heap.GetDevice(&mut heap_device).is_err() || heap_device.as_ref().map(|d| d.as_raw()) != Some(resource_device.as_raw()) {
                return None;
            }
            let heap_desc = heap.GetDesc();
            let desc = r.GetDesc();
            let allocation = resource_device.GetResourceAllocationInfo(0, &[desc]);
            if heap_desc.SizeInBytes == 0
                || receipt.offset > heap_desc.SizeInBytes
                || receipt.bytes > heap_desc.SizeInBytes - receipt.offset
                || allocation.SizeInBytes == 0
                || allocation.SizeInBytes == u64::MAX
                || allocation.Alignment == 0
                || receipt.offset % allocation.Alignment != 0
                || receipt.bytes < allocation.SizeInBytes
            {
                return None;
            }
        }
        result.heap = Some(heap);
        result.offset = receipt.offset;
        result.bytes = receipt.bytes;
    }
    if read_creation(r)? != receipt || !same_placement(&placement_of(resource), &receipt) {
        return None;
    }
    Some(result)
}

// ---- the C ABI


/// RegisteredResourceCreated past its gate; `state_known` says whether `state` is the initial state.
///
/// # Safety
/// `resource` and `heap` are live objects or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_crc_created(
    resource: *mut c_void,
    placed: bool,
    heap: *mut c_void,
    offset: u64,
    allocation_bytes: u64,
    state_known: bool,
    state: u32,
) {
    resource_created(resource, placed, heap, offset, allocation_bytes, state_known.then_some(state));
}

/// QueryResourceCreation: false, or the kind, the historical state, the owned heap reference (null when committed),
/// the offset and the bytes.
///
/// # Safety
/// `resource` is a live resource or null; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_crc_query(
    resource: *mut c_void,
    kind: *mut u32,
    state_known: *mut bool,
    state: *mut u32,
    heap: *mut *mut c_void,
    offset: *mut u64,
    bytes: *mut u64,
) -> bool {
    let Some(value) = query(resource) else {
        return false;
    };
    unsafe {
        *kind = value.kind;
        *state_known = value.state.is_some();
        *state = value.state.unwrap_or(0);
        *offset = value.offset;
        *bytes = value.bytes;
        *heap = value.heap.map_or(core::ptr::null_mut(), |h| h.into_raw());
    }
    true
}
