//! The descriptor audit's root signature layouts (src/Render/StereoDescriptorAudit.cpp, `descriptors::roots`): each
//! created root's version 1.1 description parsed into its table layout (root_layout) and kept by root, at most 128 roots
//! (an already known root is replaced); the export rows (root, parameters, supported, ranges, reasons) of all of them.
//!
//! In the Rust build (`CVR_RUST_VRIK`) RootCreated, the .cpp's root lookups and Publish's root rows forward to
//! `cvr_sda_root_*` (a lookup borrows the layout's ranges under the .cpp's metadata mutex);
//! `tools/single_pass_tests/descriptor_audit_test` runs its cases against it in its Rust variant.

use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

use crate::root_layout::{root_layout, RootLayout};
use crate::source_layout::Binding;

const ROOT_LIMIT: usize = 128;

/// RootInfo (the export row).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RootInfo {
    pub root: u64,
    pub parameters: u32,
    pub supported: u32,
    pub ranges: u32,
    pub reasons: u32,
}
const _: () = assert!(core::mem::size_of::<RootInfo>() == 24);

static ROOTS: Mutex<BTreeMap<usize, RootLayout>> = Mutex::new(BTreeMap::new());
fn roots() -> MutexGuard<'static, BTreeMap<usize, RootLayout>> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// RootCreated: the root's layout from its serialized bytes (nothing when they do not deserialize).
pub fn root_created(root: usize, bytes: &[u8]) {
    let mut raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: the root's bytes; an out-interface.
    if unsafe { D3D12CreateVersionedRootSignatureDeserializer(bytes.as_ptr() as *const c_void, bytes.len(), &ID3D12VersionedRootSignatureDeserializer::IID, &mut raw) }.is_err()
        || raw.is_null()
    {
        return;
    }
    // SAFETY: the reference the creation added.
    let reader = unsafe { ID3D12VersionedRootSignatureDeserializer::from_raw(raw) };
    // SAFETY: a live deserializer; its 1.1 description alive with it.
    let Ok(desc) = (unsafe { reader.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) }) else {
        return;
    };
    // SAFETY: a 1.1 description, as asked for.
    let layout = unsafe { root_layout(&(*desc).Anonymous.Desc_1_1) };
    let mut map = roots();
    if map.len() < ROOT_LIMIT || map.contains_key(&root) {
        map.insert(root, layout);
    }
}

/// The root's layout, when known, seen by `f` under the registry's lock.
pub fn with_layout<R>(root: usize, f: impl FnOnce(&RootLayout) -> R) -> Option<R> {
    roots().get(&root).map(f)
}

// ---- the C ABI

/// # Safety
/// `bytes` holds `size` bytes (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_root_created(root: *mut c_void, bytes: *const u8, size: usize) {
    if root.is_null() || bytes.is_null() || size == 0 {
        return;
    }
    root_created(root as usize, unsafe { core::slice::from_raw_parts(bytes, size) });
}
/// A root's layout as a lookup lends it: its ranges stay in the registry.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RootView {
    pub ranges: *const Binding,
    pub count: usize,
    pub supported: bool,
    pub parameters: u32,
    pub reasons: u32,
    pub flags: u32,
}
const _: () = assert!(core::mem::size_of::<RootView>() == 32);

/// The root's layout borrowed into `view`: false when unknown. The ranges stay valid until the root's layout is
/// replaced (only by RootCreated, which the .cpp calls under the same metadata mutex as its lookups).
///
/// # Safety
/// `view` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_root_find(root: *const c_void, view: *mut RootView) -> bool {
    let map = roots();
    let Some(layout) = map.get(&(root as usize)) else {
        return false;
    };
    unsafe {
        *view = RootView {
            ranges: layout.ranges.as_ptr(),
            count: layout.ranges.len(),
            supported: layout.supported,
            parameters: layout.parameters,
            reasons: layout.reasons,
            flags: layout.flags,
        };
    }
    true
}
/// Publish's root rows into `rows` (room for `capacity`); their count.
///
/// # Safety
/// `rows` has room for `capacity` rows.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_root_rows(rows: *mut RootInfo, capacity: usize) -> usize {
    let map = roots();
    let mut n = 0;
    for (&root, layout) in map.iter().take(capacity) {
        unsafe {
            *rows.add(n) = RootInfo {
                root: root as u64,
                parameters: layout.parameters,
                supported: layout.supported as u32,
                ranges: layout.ranges.len() as u32,
                reasons: layout.reasons,
            };
        }
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    //! A serialized root's registration and the registry's root limit (an already known root is still replaced).
    use super::*;
    use windows::Win32::Graphics::Direct3D::ID3DBlob;

    fn serialized(count: u32) -> Vec<u8> {
        let range = D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV, NumDescriptors: count, ..Default::default() };
        let parameter = D3D12_ROOT_PARAMETER1 {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER1_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 1, pDescriptorRanges: &range } },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let desc = D3D12_VERSIONED_ROOT_SIGNATURE_DESC {
            Version: D3D_ROOT_SIGNATURE_VERSION_1_1,
            Anonymous: D3D12_VERSIONED_ROOT_SIGNATURE_DESC_0 { Desc_1_1: D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: 1, pParameters: &parameter, ..Default::default() } },
        };
        let mut blob: Option<ID3DBlob> = None;
        unsafe { D3D12SerializeVersionedRootSignature(&desc, &mut blob, None) }.unwrap();
        let blob = blob.unwrap();
        unsafe { core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()) }.to_vec()
    }
    fn find(root: usize) -> Option<(u32, RootView)> {
        let mut view = RootView { ranges: core::ptr::null(), count: 0, supported: false, parameters: 0, reasons: 0, flags: 0 };
        unsafe { cvr_sda_root_find(root as *const c_void, &mut view) }.then(|| (unsafe { (*view.ranges).count }, view))
    }

    #[test]
    fn registers_parses_and_limits_roots() {
        roots().clear();
        let (four, five) = (serialized(4), serialized(5));
        unsafe { cvr_sda_root_created(0x1000 as *mut c_void, four.as_ptr(), four.len()) };
        let (count, view) = find(0x1000).unwrap();
        assert!(count == 4 && view.count == 1 && view.supported && view.parameters == 1 && view.reasons == 0);
        unsafe { cvr_sda_root_created(0x2000 as *mut c_void, [1u8, 2, 3].as_ptr(), 3) };
        assert!(find(0x2000).is_none());
        for i in 1..ROOT_LIMIT {
            root_created(0x1000 + i * 16, &four);
        }
        root_created(0x9000_0000, &four);
        assert!(find(0x9000_0000).is_none());
        root_created(0x1000, &five);
        assert_eq!(find(0x1000).unwrap().0, 5);
        let mut rows = [RootInfo::default(); 130];
        assert_eq!(unsafe { cvr_sda_root_rows(rows.as_mut_ptr(), rows.len()) }, ROOT_LIMIT);
        assert_eq!(rows[0], RootInfo { root: 0x1000, parameters: 1, supported: 1, ranges: 1, reasons: 0 });
    }
}
