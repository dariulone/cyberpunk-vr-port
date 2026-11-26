//! The native prepared pair's compared ranges (src/Render/NativePreparedPair.cpp, `PrepareMesh`, `PrepareInputs` and
//! `BuildBank`): one eye's mesh range of an input slot (slot 0 the index view, slots 1..16 the vertex views, slot 8 the
//! instance window -- the first instance's offset and the instances' bytes at the instance stride) lies inside its held
//! buffer, is not empty and has the first eye's bytes; a recorded input check is admitted with its resource and owner,
//! 1..65536 expected bytes and room in the 4 MiB expected buffer; and the two eyes' shared table is one table (the same
//! CPU heap, offset, storage scope and extent, identity and version) at the larger of their counts.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npp_mesh_eye`, `cvr_npp_input_admitted` and
//! `cvr_npp_shared_table` (the table key mirrored by prepared_pair_tables.rs); the unit tests below cover each bound.

use crate::prepared_pair_tables::TableKey;

/// The expected buffer's limit and one check's.
pub const MAX_EXPECTED_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CHECK_BYTES: usize = 65536;

/// One eye's view of a slot: its recorded address and bytes, the instance window's inputs and the held buffer.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshEye {
    pub address: u64,
    pub bytes: u64,
    pub stride: u32,
    pub first_instance: u32,
    pub instances: u32,
    pub present: bool,
    pub gpu: u64,
    pub buffer_bytes: u64,
}
/// The range's offset in the buffer and its bytes, or None.
pub fn mesh_eye(slot: u32, view: &MeshEye, eye: u32, range_bytes: u64) -> Option<(u64, u64)> {
    let (address, bytes) = if slot == 8 {
        let stride = u64::from(view.stride);
        (view.address.wrapping_add(u64::from(view.first_instance) * stride), u64::from(view.instances) * stride)
    } else {
        (view.address, view.bytes)
    };
    if !view.present || bytes == 0 || address < view.gpu {
        return None;
    }
    let offset = address - view.gpu;
    if offset > view.buffer_bytes || bytes > view.buffer_bytes - offset || (eye != 0 && bytes != range_bytes) {
        return None;
    }
    Some((offset, bytes))
}
/// PrepareInputs' admission of one check: `total` the expected bytes so far.
pub fn input_admitted(resource: bool, owner: bool, bytes: usize, total: usize) -> bool {
    resource && owner && bytes != 0 && bytes <= MAX_CHECK_BYTES && total <= MAX_EXPECTED_BYTES - bytes
}
/// BuildBank's shared table: the merged count, or None.
pub fn shared_table(a: &TableKey, b: &TableKey) -> Option<u32> {
    let same = a.heap == b.heap
        && a.offset == b.offset
        && a.storage_scope == b.storage_scope
        && a.storage_extent == b.storage_extent
        && a.identity == b.identity
        && a.version == b.version;
    same.then(|| a.count.max(b.count))
}

// ---- the C ABI

/// # Safety
/// `offset` and `bytes` are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npp_mesh_eye(
    slot: u32,
    address: u64,
    bytes: u64,
    stride: u32,
    first_instance: u32,
    instances: u32,
    present: bool,
    gpu: u64,
    buffer_bytes: u64,
    eye: u32,
    range_bytes: u64,
    offset: *mut u64,
    out_bytes: *mut u64,
) -> bool {
    let view = MeshEye { address, bytes, stride, first_instance, instances, present, gpu, buffer_bytes };
    match mesh_eye(slot, &view, eye, range_bytes) {
        Some((at, extent)) => {
            unsafe {
                *offset = at;
                *out_bytes = extent;
            }
            true
        }
        None => false,
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_npp_input_admitted(resource: bool, owner: bool, bytes: usize, total: usize) -> bool {
    input_admitted(resource, owner, bytes, total)
}
/// # Safety
/// Both keys are valid; `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_shared_table(a: *const TableKey, b: *const TableKey, count: *mut u32) -> bool {
    match shared_table(unsafe { &*a }, unsafe { &*b }) {
        Some(merged) => {
            unsafe { *count = merged };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! Each mesh bound, each input limit and each shared table field.
    use super::*;

    #[test]
    fn meshes() {
        let v = MeshEye { address: 1100, bytes: 50, stride: 16, first_instance: 2, instances: 3, present: true, gpu: 1000, buffer_bytes: 200 };
        assert_eq!(mesh_eye(0, &v, 0, 0), Some((100, 50)));
        assert_eq!(mesh_eye(3, &v, 1, 50), Some((100, 50)));
        assert_eq!(mesh_eye(3, &v, 1, 49), None);
        assert_eq!(mesh_eye(3, &v, 0, 49), Some((100, 50)));
        assert_eq!(mesh_eye(8, &v, 0, 0), Some((132, 48)));
        assert_eq!(mesh_eye(8, &MeshEye { instances: 5, ..v }, 0, 0), None);
        assert_eq!(mesh_eye(8, &MeshEye { instances: 4, ..v }, 0, 0), Some((132, 64)));
        assert_eq!(mesh_eye(0, &MeshEye { present: false, ..v }, 0, 0), None);
        assert_eq!(mesh_eye(0, &MeshEye { bytes: 0, ..v }, 0, 0), None);
        assert_eq!(mesh_eye(0, &MeshEye { address: 999, ..v }, 0, 0), None);
        assert_eq!(mesh_eye(0, &MeshEye { address: 1201, bytes: 1, ..v }, 0, 0), None);
        assert_eq!(mesh_eye(0, &MeshEye { address: 1200, bytes: 1, ..v }, 0, 0), None);
        assert_eq!(mesh_eye(0, &MeshEye { address: 1150, ..v }, 0, 0), Some((150, 50)));
        assert_eq!(mesh_eye(0, &MeshEye { address: 1151, ..v }, 0, 0), None);
    }
    #[test]
    fn inputs() {
        assert!(input_admitted(true, true, 1, 0));
        assert!(input_admitted(true, true, MAX_CHECK_BYTES, MAX_EXPECTED_BYTES - MAX_CHECK_BYTES));
        assert!(!input_admitted(true, true, MAX_CHECK_BYTES, MAX_EXPECTED_BYTES - MAX_CHECK_BYTES + 1));
        assert!(!input_admitted(true, true, MAX_CHECK_BYTES + 1, 0));
        assert!(!input_admitted(true, true, 0, 0));
        assert!(!input_admitted(false, true, 1, 0));
        assert!(!input_admitted(true, false, 1, 0));
    }
    #[test]
    fn tables() {
        let a = TableKey { heap: 1, storage_scope: 2, identity: 3, version: 4, offset: 5, count: 6, storage_extent: 8 };
        assert_eq!(shared_table(&a, &a), Some(6));
        assert_eq!(shared_table(&a, &TableKey { count: 7, ..a }), Some(7));
        assert_eq!(shared_table(&TableKey { count: 9, ..a }, &a), Some(9));
        for b in [
            TableKey { heap: 9, ..a },
            TableKey { offset: 9, ..a },
            TableKey { storage_scope: 9, ..a },
            TableKey { storage_extent: 9, ..a },
            TableKey { identity: 9, ..a },
            TableKey { version: 9, ..a },
        ] {
            assert_eq!(shared_table(&a, &b), None);
        }
    }
}
