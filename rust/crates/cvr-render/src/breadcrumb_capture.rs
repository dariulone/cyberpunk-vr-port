//! THE NATIVE BREADCRUMB WRITE'S CAPTURE (formerly src/Render/NativeBreadcrumbWrite.cpp): an actual WriteBufferImmediate
//! breadcrumb marker write, captured from its own strictly unwound native frame -- the marker's list, kind and slot, the
//! factory's breadcrumb buffer and heap read twice around the API checks (the device, the buffer's shape, its API
//! creation receipt or the native factory's placed buffer), with every stage's failure and details kept per thread --
//! and the snapshot a capture describes while its buffer still is what was seen. The rules are breadcrumb.rs'.
//!
//! The C++ API (include/Render/NativeBreadcrumbWrite.hpp) forwards here; its Lease holds a reference to the Rust state.
use core::cell::Cell;
use core::ffi::c_void;
use std::sync::OnceLock;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::breadcrumb::{self as rules, Frame, Observation};
use crate::ia_binding::cvr_iab_read;
use crate::native_recording::Code;
use crate::texture_immutable::Creation;

/// detail::Contract: the breadcrumb paths' code at their RVAs (FNV-1a of their bytes).
const CONTRACT: [Code; 16] = [
    Code { rva: 0x3E47E8, bytes: 382, hash: 0xD8695C867CC19844 },
    Code { rva: 0x3E5174, bytes: 349, hash: 0x260466FD7E09BA99 },
    Code { rva: 0x246A0B6, bytes: 138, hash: 0x1471986E0476A136 },
    Code { rva: 0x3E59DC, bytes: 156, hash: 0x7261A9D40E98E5FD },
    Code { rva: 0x246A1E6, bytes: 87, hash: 0x9F25F05CFB672FC7 },
    Code { rva: 0x688D5C, bytes: 260, hash: 0x2EB88470C17A84D9 },
    Code { rva: 0x68A1D0, bytes: 978, hash: 0x0E75B87A8C6851B5 },
    Code { rva: 0x51FDD8, bytes: 439, hash: 0x15A56074B6B4CFBE },
    Code { rva: 0x51FF90, bytes: 338, hash: 0xDE807FDE92738233 },
    Code { rva: 0x520284, bytes: 123, hash: 0x77AA09F4EE843A10 },
    Code { rva: 0x51FD40, bytes: 149, hash: 0x12D9E1DA51EB8090 },
    Code { rva: 0x520228, bytes: 58, hash: 0x5FC34FAD173BDCE7 },
    Code { rva: 0x1919E98, bytes: 93, hash: 0x187A5BC1963CF585 },
    Code { rva: 0x14F4AD0, bytes: 35, hash: 0x104BC2F46DA77B2F },
    Code { rva: 0x2FAEE24, bytes: 41, hash: 0x18A456DC7BE471B1 },
    Code { rva: 0x2427F00, bytes: 12, hash: 0xA453EAFC10D16B8C },
];
/// Provenance.
const OBSERVED_CREATION: u32 = 1;
const NATIVE_FACTORY: u32 = 2;

thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
    static DETAILS: Cell<[u64; 12]> = const { Cell::new([0; 12]) };
}
fn stage(code: u32) {
    FAILURE.set(code);
    detail(0, code as u64);
}
fn detail(index: usize, value: u64) {
    let mut d = DETAILS.get();
    d[index] = value;
    DETAILS.set(d);
}
fn base() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64)
}
/// detail::Read: `bytes` at `address` into `out`.
fn read(address: u64, out: *mut c_void, bytes: usize) -> bool {
    // SAFETY: the reader writes at most `bytes` bytes at `out`.
    address != 0 && bytes != 0 && address <= u64::MAX - bytes as u64 && unsafe { cvr_iab_read(core::ptr::null_mut(), address, out, bytes) }
}
/// Compatible: the native recording's contract and this module's sixteen code paths (the first rejected RVA kept).
fn compatible() -> bool {
    static REJECTED: OnceLock<u64> = OnceLock::new();
    let rejected = *REJECTED.get_or_init(|| {
        if !crate::native_recording::cvr_native_recording_compatible() {
            return 1;
        }
        let base = base();
        let mut bytes = [0u8; 1024];
        for code in CONTRACT {
            let (rva, size) = (code.rva as u64, code.bytes as usize);
            if base == 0
                || base > u64::MAX - rva
                || size > bytes.len()
                || !read(base + rva, bytes.as_mut_ptr().cast(), size)
                || crate::native_recording::fingerprint(&bytes[..size]) != code.hash
            {
                return rva;
            }
        }
        0
    });
    detail(1, rejected);
    rejected == 0
}
fn native_frame(base: u64, caller: u64) -> Option<Frame> {
    let mut details = DETAILS.get();
    let frame = rules::native_frame(base, caller, &mut details);
    DETAILS.set(details);
    frame
}
fn observed(value: &Observation) {
    let mut d = DETAILS.get();
    d[3] = value.holder;
    d[4] = value.marker;
    d[5] = value.entry;
    d[6] = value.resource;
    d[7] = value.heap;
    d[8] = value.gpu;
    d[9] = value.slot_gpu;
    d[10] = value.slot as u64;
    d[11] = value.kind as u64;
    DETAILS.set(d);
}
fn read_observation(base: u64, frame: &Frame, list: u64, parameter: &D3D12_WRITEBUFFERIMMEDIATE_PARAMETER, mode: u32, output: &mut Observation) -> bool {
    stage(6);
    let mut rejected = 0u32;
    // SAFETY: plain values; the reader reads the engine's memory.
    let success =
        unsafe { rules::cvr_bc_observe(Some(cvr_iab_read), core::ptr::null_mut(), base, frame, list, parameter.Dest, parameter.Value, mode, output, &mut rejected) };
    detail(1, rejected as u64);
    observed(output);
    success
}

// ---- the snapshot

/// breadcrumb_write::Snapshot (the creation's resource and heap owned).
#[repr(C)]
#[derive(Clone, Default)]
pub struct Snapshot {
    pub creation: Creation,
    pub resource_desc: D3D12_RESOURCE_DESC,
    pub heap_desc: D3D12_HEAP_DESC,
    pub destination: u64,
    pub value: u32,
    pub mode: u32,
    pub provenance: u32,
}
const _: () = assert!(
    core::mem::size_of::<Creation>() == 48
        && core::mem::size_of::<Snapshot>() == 176
        && core::mem::offset_of!(Snapshot, heap_desc) == 104
        && core::mem::offset_of!(Snapshot, destination) == 152
);
/// What a capture saw.
struct Data {
    captured: Snapshot,
    native: Observation,
}
fn same_resource_description(a: &D3D12_RESOURCE_DESC, b: &D3D12_RESOURCE_DESC) -> bool {
    a.Dimension == b.Dimension
        && a.Alignment == b.Alignment
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == b.DepthOrArraySize
        && a.MipLevels == b.MipLevels
        && a.Format == b.Format
        && a.SampleDesc.Count == b.SampleDesc.Count
        && a.SampleDesc.Quality == b.SampleDesc.Quality
        && a.Layout == b.Layout
        && a.Flags == b.Flags
}
fn same_heap_properties(a: &D3D12_HEAP_PROPERTIES, b: &D3D12_HEAP_PROPERTIES) -> bool {
    a.Type == b.Type
        && a.CPUPageProperty == b.CPUPageProperty
        && a.MemoryPoolPreference == b.MemoryPoolPreference
        && a.CreationNodeMask == b.CreationNodeMask
        && a.VisibleNodeMask == b.VisibleNodeMask
}
fn same_heap_description(a: &D3D12_HEAP_DESC, b: &D3D12_HEAP_DESC) -> bool {
    a.SizeInBytes == b.SizeInBytes && a.Alignment == b.Alignment && a.Flags == b.Flags && same_heap_properties(&a.Properties, &b.Properties)
}
fn same_creation_metadata(a: &Creation, b: &Creation) -> bool {
    a.resource == b.resource
        && a.kind == b.kind
        && a.heap == b.heap
        && a.offset == b.offset
        && a.bytes == b.bytes
        && a.known == b.known
        && a.state == b.state
}
/// constants::QueryResourceCreation.
fn query_creation(resource: &ID3D12Resource) -> Option<Creation> {
    let (mut kind, mut known, mut state, mut heap, mut offset, mut bytes) = (0u32, false, 0u32, core::ptr::null_mut(), 0u64, 0u64);
    // SAFETY: a live resource; the outputs are locals (the heap an owned reference).
    if !unsafe { crate::creation_receipts::cvr_crc_query(resource.as_raw(), &mut kind, &mut known, &mut state, &mut heap, &mut offset, &mut bytes) } {
        return None;
    }
    Some(Creation {
        kind,
        known,
        state: if known { state as i32 } else { 0 },
        resource: Some(resource.clone()),
        // SAFETY: the query's owned heap reference (or null).
        heap: unsafe { (!heap.is_null()).then(|| ID3D12Heap::from_raw(heap)) },
        offset,
        bytes,
    })
}
/// SameDescription: the buffer and heap still have the descriptions (and the buffer its heap) the capture saw.
fn same_description(value: &Snapshot) -> bool {
    let (Some(resource), Some(heap)) = (&value.creation.resource, &value.creation.heap) else {
        return false;
    };
    // SAFETY: live objects the snapshot owns.
    unsafe {
        if !same_resource_description(&value.resource_desc, &resource.GetDesc()) || !same_heap_description(&value.heap_desc, &heap.GetDesc()) {
            return false;
        }
        let mut properties = D3D12_HEAP_PROPERTIES::default();
        let mut flags = D3D12_HEAP_FLAGS::default();
        resource.GetHeapProperties(Some(&mut properties), Some(&mut flags)).is_ok()
            && same_heap_properties(&properties, &value.heap_desc.Properties)
            && flags == value.heap_desc.Flags
    }
}
fn device_of<T: Interface>(object: &T) -> Option<ID3D12Device> {
    let child = object.cast::<ID3D12DeviceChild>().ok()?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child; the output is a local.
    unsafe { child.GetDevice(&mut device) }.ok().and(device)
}
fn capture(list: *mut c_void, count: u32, parameters: u64, modes: u64, caller: u64) -> Option<Box<Data>> {
    DETAILS.set([0; 12]);
    stage(1);
    // SAFETY: a live list or null.
    let list_ref = unsafe { ID3D12GraphicsCommandList2::from_raw_borrowed(&list) }?;
    if count != 1 || parameters == 0 || modes == 0 || caller == 0 {
        return None;
    }
    stage(2);
    if !compatible() {
        return None;
    }
    let base = base();
    stage(3);
    let frame = native_frame(base, caller)?;
    detail(2, frame.rip);
    let mut parameter = D3D12_WRITEBUFFERIMMEDIATE_PARAMETER::default();
    let mut mode = 0u32;
    stage(4);
    if !read(parameters, (&mut parameter as *mut D3D12_WRITEBUFFERIMMEDIATE_PARAMETER).cast(), core::mem::size_of_val(&parameter))
        || !read(modes, (&mut mode as *mut u32).cast(), 4)
        || !rules::parameters(rules::caller_mode(base, frame.rip), count, parameter.Dest, parameter.Value, mode)
    {
        return None;
    }
    let mut native = Observation::default();
    if !read_observation(base, &frame, list as u64, &parameter, mode, &mut native) {
        return None;
    }
    let (resource_raw, heap_raw) = (native.resource as *mut c_void, native.heap as *mut c_void);
    // SAFETY: the factory's live buffer and heap (read twice, unchanged); each takes its own reference.
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource_raw) }?.clone();
    let heap = unsafe { ID3D12Heap::from_raw_borrowed(&heap_raw) }?.clone();
    let mut saved = Snapshot {
        creation: Creation { resource: Some(resource.clone()), heap: Some(heap.clone()), ..Default::default() },
        destination: parameter.Dest,
        value: parameter.Value,
        mode,
        ..Default::default()
    };
    // SAFETY: live objects.
    unsafe {
        saved.resource_desc = resource.GetDesc();
        saved.heap_desc = heap.GetDesc();
    }
    stage(7);
    let device = device_of(&resource)?;
    if device_of(&heap).as_ref() != Some(&device) || device_of(list_ref).as_ref() != Some(&device) || unsafe { device.GetNodeCount() } != 1 {
        return None;
    }
    // SAFETY: a live device; one description.
    let allocation = unsafe { device.GetResourceAllocationInfo(0, &[saved.resource_desc]) };
    // SAFETY: a live resource.
    if !rules::shape(&native, &saved.resource_desc, &saved.heap_desc, &allocation, unsafe { resource.GetGPUVirtualAddress() }) || !same_description(&saved) {
        return None;
    }
    stage(8);
    match query_creation(&resource) {
        Some(observed) => {
            if !rules::observed_creation_matches(&observed, &native, allocation.SizeInBytes) {
                return None;
            }
            saved.creation = observed;
            saved.provenance = OBSERVED_CREATION;
        }
        None => {
            let c = &mut saved.creation;
            c.kind = 2;
            c.offset = 0;
            c.bytes = allocation.SizeInBytes;
            c.known = true;
            c.state = D3D12_RESOURCE_STATE_COPY_DEST.0;
            saved.provenance = NATIVE_FACTORY;
        }
    }
    stage(9);
    let again_frame = native_frame(base, caller)?;
    if again_frame != frame {
        return None;
    }
    let mut again = Observation::default();
    if !read_observation(base, &again_frame, list as u64, &parameter, mode, &mut again) {
        return None;
    }
    stage(9);
    // SAFETY: a live resource.
    if again != native || !same_description(&saved) || unsafe { resource.GetGPUVirtualAddress() } != native.gpu {
        return None;
    }
    stage(0);
    Some(Box::new(Data { captured: saved, native }))
}

// ---- the C++ API

/// Capture: the captured state (released with cvr_bcw_release), or null.
///
/// # Safety
/// `list` is a live ID3D12GraphicsCommandList2 or null; `parameters` and `modes` are the call's arrays (read guarded).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bcw_capture(list: *mut c_void, count: u32, parameters: u64, modes: u64, caller: u64) -> *mut c_void {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture(list, count, parameters, modes, caller))) {
        Ok(Some(data)) => Box::into_raw(data).cast(),
        Ok(None) => core::ptr::null_mut(),
        Err(_) => {
            stage(10);
            core::ptr::null_mut()
        }
    }
}
/// The C++ lease's last reference let go of the captured state.
///
/// # Safety
/// `data` is cvr_bcw_capture's (released once) or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bcw_release(data: *mut c_void) {
    if !data.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(data.cast::<Data>()) });
    }
}
/// Describe: `*output` (empty) the captured snapshot while its buffer still is what the capture saw.
///
/// # Safety
/// `data` is a live captured state or null; `output` is the caller's empty Snapshot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bcw_describe(data: *const c_void, output: *mut Snapshot) -> bool {
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stage(1);
        // SAFETY: as documented.
        let value = unsafe { data.cast::<Data>().as_ref() }?;
        stage(9);
        let captured = &value.captured;
        let resource = captured.creation.resource.as_ref()?;
        // SAFETY: a live resource the snapshot owns.
        if !same_description(captured) || unsafe { resource.GetGPUVirtualAddress() } != value.native.gpu {
            return None;
        }
        if captured.provenance == OBSERVED_CREATION {
            let current = query_creation(resource)?;
            if !same_creation_metadata(&current, &captured.creation) {
                return None;
            }
        }
        stage(0);
        Some(captured.clone())
    }));
    match made {
        Ok(Some(snapshot)) => {
            // SAFETY: the caller's empty snapshot (null references, so nothing is overwritten).
            unsafe { output.write(snapshot) };
            true
        }
        Ok(None) => false,
        Err(_) => {
            stage(10);
            false
        }
    }
}
/// LastFailure.
#[no_mangle]
pub extern "C-unwind" fn cvr_bcw_last_failure() -> u32 {
    FAILURE.get()
}
/// LastDetails.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bcw_last_details(out: *mut [u64; 12]) {
    // SAFETY: as documented.
    unsafe { *out = DETAILS.get() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_without_a_list_and_nothing_described_without_a_capture() {
        // SAFETY: a null list is refused before anything is read.
        assert!(unsafe { cvr_bcw_capture(core::ptr::null_mut(), 1, 8, 8, 1) }.is_null());
        assert_eq!(cvr_bcw_last_failure(), 1);
        let mut snapshot = Snapshot::default();
        // SAFETY: a null state.
        assert!(!unsafe { cvr_bcw_describe(core::ptr::null(), &mut snapshot) });
        assert_eq!(cvr_bcw_last_failure(), 1);
        let mut details = [7u64; 12];
        // SAFETY: a local output.
        unsafe { cvr_bcw_last_details(&mut details) };
        assert_eq!(details[0], 1);
    }

    #[test]
    fn contract_matches_the_header_count() {
        assert_eq!(CONTRACT.len(), 16);
        assert!(CONTRACT.iter().all(|c| c.bytes as usize <= 1024));
    }
}
