//! The native instance observer's backing checks (src/Render/NativeInstanceObserver.cpp, `instance_observer::Physical`
//! and `Capture`): a ring header still the observed ring's (handle, mapped data, capacity, stride), the cursors in order
//! and within the capacity, the creation receipt still the captured one (kind, resource, heap, offset, bytes; the initial
//! state not compared), a ring record of the instance buffer class (type 0x51 at +6, no alias words, its mapped data at
//! +0x40), and the record naming the buffer (GPU address at +8, width at +0).
//!
//! In the Rust build (`CVR_RUST_VRIK`) those checks forward to `cvr_nib_*`; the unit tests below cover each field.

use crate::ia_snapshot_same::CreationInfo;
use crate::instance_layout::RingFields;
use crate::native::instance_ring::RingObservation;

fn field_u64(raw: &[u8; 0xB0], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}
fn field_u32(raw: &[u8; 0xB0], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}

/// Physical's header check.
pub fn ring_matches(fields: &RingFields, observed: &RingObservation) -> bool {
    fields.handle == observed.handle && fields.mapped == observed.mapped && fields.capacity as u64 == observed.capacity && fields.stride == observed.stride
}
/// Physical's cursors.
pub fn cursors_valid(observed: &RingObservation) -> bool {
    observed.head >= observed.tail && observed.head - observed.tail <= observed.capacity
}
/// Physical's creation check.
pub fn same_creation(current: &CreationInfo, saved: &CreationInfo) -> bool {
    current.kind == saved.kind && current.resource == saved.resource && current.heap == saved.heap && current.heap_offset == saved.heap_offset && current.allocation_bytes == saved.allocation_bytes
}
/// Capture's record class.
pub fn record_shape(raw: &[u8; 0xB0], mapped: u64) -> bool {
    raw[6] == 0x51 && field_u64(raw, 0x20) == 0 && field_u64(raw, 0x28) == 0 && field_u64(raw, 0x40) == mapped
}
/// Capture's record buffer.
pub fn record_buffer(raw: &[u8; 0xB0], gpu: u64, width: u64) -> bool {
    field_u64(raw, 8) == gpu && field_u32(raw, 0) as u64 == width
}

// ---- the C ABI

/// # Safety
/// Both pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nib_ring_matches(fields: *const RingFields, observed: *const RingObservation) -> bool {
    ring_matches(unsafe { &*fields }, unsafe { &*observed })
}
/// # Safety
/// `observed` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nib_cursors_valid(observed: *const RingObservation) -> bool {
    cursors_valid(unsafe { &*observed })
}
/// # Safety
/// Both receipts are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nib_same_creation(current: *const CreationInfo, saved: *const CreationInfo) -> bool {
    same_creation(unsafe { &*current }, unsafe { &*saved })
}
/// # Safety
/// `raw` holds 0xB0 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nib_record_shape(raw: *const [u8; 0xB0], mapped: u64) -> bool {
    record_shape(unsafe { &*raw }, mapped)
}
/// # Safety
/// `raw` holds 0xB0 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nib_record_buffer(raw: *const [u8; 0xB0], gpu: u64, width: u64) -> bool {
    record_buffer(unsafe { &*raw }, gpu, width)
}

#[cfg(test)]
mod tests {
    //! Each header, cursor, creation and record field.
    use super::*;
    use core::ffi::c_void;

    #[test]
    fn headers_and_cursors() {
        let observed = RingObservation { handle: 3, mapped: 0x100, capacity: 0x8000, stride: 64, head: 10, tail: 4, ..Default::default() };
        let f = RingFields { handle: 3, capacity: 0x8000, stride: 64, mapped: 0x100, head: 0, tail: 0 };
        assert!(ring_matches(&f, &observed));
        assert!(!ring_matches(&RingFields { stride: 48, ..f }, &observed));
        assert!(!ring_matches(&RingFields { mapped: 0x200, ..f }, &observed));
        assert!(!ring_matches(&RingFields { capacity: 0x20000, ..f }, &observed));
        assert!(cursors_valid(&observed));
        assert!(!cursors_valid(&RingObservation { head: 3, ..observed }));
        assert!(!cursors_valid(&RingObservation { head: 0x8005, ..observed }));
        assert!(cursors_valid(&RingObservation { head: 0x8004, ..observed }));
    }
    #[test]
    fn creations_and_records() {
        let a = CreationInfo { kind: 2, initial_state_known: true, initial_state: 1, resource: 1 as *mut c_void, heap: 2 as *mut c_void, heap_offset: 3, allocation_bytes: 4 };
        assert!(same_creation(&a, &CreationInfo { initial_state: 7, initial_state_known: false, ..a }));
        assert!(!same_creation(&a, &CreationInfo { heap_offset: 9, ..a }));
        let mut raw = [0u8; 0xB0];
        raw[6] = 0x51;
        raw[0x40..0x48].copy_from_slice(&0x500u64.to_le_bytes());
        raw[8..16].copy_from_slice(&0x9000u64.to_le_bytes());
        raw[0..4].copy_from_slice(&0x2000u32.to_le_bytes());
        assert!(record_shape(&raw, 0x500));
        assert!(!record_shape(&raw, 0x501));
        assert!(record_buffer(&raw, 0x9000, 0x2000));
        assert!(!record_buffer(&raw, 0x9000, 0x1_0000_2000));
        raw[0x28] = 1;
        assert!(!record_shape(&raw, 0x500));
    }
}
