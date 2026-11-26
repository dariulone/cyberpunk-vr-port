//! The C ABI of the upload reservations' rules (native::upload_reservation) for
//! include/Render/NativeUploadReservation.hpp: the evidence of the engine's upload-ring allocator -- a witness of its
//! successful CAS taken by a CALL patched at the success branch --, the ledger of those rows per ring generation (an
//! owned handle here), a reservation's lookup and a sub-range's validation against its backing, and the externally
//! installed patch's plan and receipt. tools/single_pass_tests/upload_reservation_test and
//! upload_install_prepare_test run their cases against these in their Rust variants.
use core::ffi::c_void;

use crate::native::upload_reservation::{
    external_plan_checksum, external_receipt_checksum, fnv, matches_external_patched_body,
    valid_external_plan, valid_external_receipt, valid_witness, valid_xstate, ExternalInstallPlan,
    ExternalInstallReceipt, Ledger, Range, Reservation, Row, Witness, ALLOCATOR_BYTES,
};

#[no_mangle]
pub extern "C" fn cvr_ur_valid_xstate(enabled: u64, supported: u64, bytes: u64) -> bool {
    valid_xstate(enabled, supported, bytes)
}
/// # Safety
/// `bytes` valid for `count`.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_fnv(bytes: *const u8, count: usize) -> u64 {
    // SAFETY: as the caller promises.
    fnv(if count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(bytes, count) }
    })
}
/// # Safety
/// `value` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_external_plan_checksum(value: *const ExternalInstallPlan) -> u64 {
    // SAFETY: as the caller promises.
    external_plan_checksum(unsafe { &*value })
}
/// # Safety
/// `value` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_external_receipt_checksum(
    value: *const ExternalInstallReceipt,
) -> u64 {
    // SAFETY: as the caller promises.
    external_receipt_checksum(unsafe { &*value })
}
/// # Safety
/// `value` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_valid_external_plan(value: *const ExternalInstallPlan) -> bool {
    // SAFETY: as the caller promises.
    valid_external_plan(unsafe { &*value })
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_matches_external_patched_body(
    value: *const ExternalInstallPlan,
    current: *const [u8; ALLOCATOR_BYTES],
) -> bool {
    // SAFETY: as the caller promises.
    matches_external_patched_body(unsafe { &*value }, unsafe { &*current })
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_valid_external_receipt(
    plan: *const ExternalInstallPlan,
    receipt: *const ExternalInstallReceipt,
) -> bool {
    // SAFETY: as the caller promises.
    valid_external_receipt(unsafe { &*plan }, unsafe { &*receipt })
}
/// # Safety
/// `value` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_valid_witness(value: *const Witness) -> bool {
    // SAFETY: as the caller promises.
    valid_witness(unsafe { &*value })
}
#[no_mangle]
pub extern "C" fn cvr_ur_ledger_new(rows: usize, rings: usize, observer_epoch: u64) -> *mut c_void {
    Box::into_raw(Box::new(Ledger::new(rows, rings, observer_epoch))) as *mut c_void
}
/// # Safety
/// A handle cvr_ur_ledger_new returned.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_free(ledger: *mut c_void) {
    if !ledger.is_null() {
        // SAFETY: as the caller promises.
        drop(unsafe { Box::from_raw(ledger as *mut Ledger) });
    }
}
/// # Safety
/// A live handle (the caller serializes access).
unsafe fn ledger<'a>(handle: *mut c_void) -> &'a mut Ledger {
    // SAFETY: as the caller promises.
    unsafe { &mut *(handle as *mut Ledger) }
}
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_reset(handle: *mut c_void, observer_epoch: u64) {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.reset(observer_epoch);
}
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_begin_generation(
    handle: *mut c_void,
    ring: u64,
    generation: u64,
    capacity: u64,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.begin_generation(ring, generation, capacity)
}
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_end_generation(handle: *mut c_void, ring: u64) {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.end_generation(ring);
}
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_quarantine_all(handle: *mut c_void) {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.quarantine_all();
}
/// # Safety
/// A live handle; `value` and `output` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_observe(
    handle: *mut c_void,
    value: *const Witness,
    thread: u64,
    output: *mut Row,
    capacity_stable: bool,
) -> u64 {
    // SAFETY: as the caller promises.
    unsafe {
        let (status, row) = ledger(handle).observe(&*value, thread, capacity_stable);
        *output = row;
        status as u64
    }
}
/// # Safety
/// A live handle; `output` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_find(
    handle: *mut c_void,
    sequence: u64,
    output: *mut Reservation,
) -> u64 {
    // SAFETY: as the caller promises.
    unsafe {
        let (status, reservation) = ledger(handle).find(sequence);
        *output = reservation;
        status as u64
    }
}
/// # Safety
/// A live handle; the records valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_ur_ledger_validate_range(
    handle: *mut c_void,
    reservation: *const Reservation,
    local_offset: u64,
    physical_offset: u64,
    padded_bytes: u64,
    logical_bytes: u64,
    backing_bytes: u64,
    output: *mut Range,
) -> u64 {
    // SAFETY: as the caller promises.
    unsafe {
        let (status, range) = ledger(handle).validate_range(
            &*reservation,
            local_offset,
            physical_offset,
            padded_bytes,
            logical_bytes,
            backing_bytes,
        );
        *output = range;
        status as u64
    }
}
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_count(handle: *mut c_void) -> usize {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.count()
}
/// The rows' storage (all of them, default beyond the count; stable while the ledger lives).
///
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_rows(handle: *mut c_void) -> *const Row {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.rows().as_ptr()
}
/// # Safety
/// A live handle.
#[no_mangle]
pub unsafe extern "C" fn cvr_ur_ledger_exhausted(handle: *mut c_void) -> bool {
    // SAFETY: as the caller promises.
    unsafe { ledger(handle) }.exhausted()
}
