//! The C ABI of the deferred retirement ledger (`native::retirement_deferral`) for `include/Render/
//! NativeRetirementDeferral.hpp` in the Rust build (`CVR_RUST_VRIK`): the C++ `Ledger` owns a Rust ledger, and each
//! C++ `State` is the stable face of one reserved Rust state (boxed, so its frozen identity, its saved header and the
//! 16-byte storage BeginDestroy hands to original986E74 keep their addresses until its metadata is retired). The face
//! is allocated where the C++ allocated its State, inside the reservation after every check and before anything is
//! committed, so an allocation failure still commits nothing. `tools/single_pass_tests/retirement_deferral_test` runs
//! its cases against these in its Rust variant.

use core::ffi::c_void;

use crate::native::retirement_deferral::*;

const _: () = assert!(
    core::mem::size_of::<NativeHeader>() == 16
        && core::mem::size_of::<HeaderIdentity>() == 56
        && core::mem::size_of::<AllocatorIdentity>() == 16
        && core::mem::size_of::<AllocatorProof>() == 24
        && core::mem::size_of::<ReservationAllocatorObservation>() == 24
        && core::mem::size_of::<ReservationFacts>() == 3
        && core::mem::size_of::<NativeIntentFacts>() == 4
        && core::mem::size_of::<HeaderTransferProof>() == 96
        && core::mem::size_of::<ExtraReaderIdentity>() == 48
        && core::mem::size_of::<OwnedGraphCompletionProof>() == 64
        && core::mem::size_of::<UnescapedCancellationProof>() == 56
);

/// A new empty ledger, owned by the C++ face.
#[no_mangle]
pub extern "C-unwind" fn cvr_rd_ledger_new() -> *mut Ledger {
    Box::into_raw(Box::default())
}
/// Frees a ledger made by `cvr_rd_ledger_new` (its states with it; nothing native is freed).
///
/// # Safety
/// `ledger` came from `cvr_rd_ledger_new` and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rd_ledger_free(ledger: *mut Ledger) {
    if !ledger.is_null() {
        drop(unsafe { Box::from_raw(ledger) });
    }
}
/// The face's allocation: (context) -> allocated.
pub type Allocate = unsafe extern "C-unwind" fn(*mut c_void) -> bool;
/// Reserve: on success the state's `slot` and its address.
///
/// # Safety
/// `ledger` is live; the records are valid; `slot` and `state` are writable; `allocate` is callable with `context`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_rd_ledger_reserve(
    ledger: *mut Ledger,
    identity: *const HeaderIdentity,
    facts: *const ReservationFacts,
    allocator: *const ReservationAllocatorObservation,
    allocate: Allocate,
    context: *mut c_void,
    slot: *mut u32,
    state: *mut *mut State,
) -> bool {
    let ledger = unsafe { &mut *ledger };
    let reserved = unsafe { ledger.reserve_with(&*identity, *facts, &*allocator, || allocate(context)) };
    let Some(id) = reserved else { return false };
    let Some(reserved) = ledger.get_mut(id) else { return false };
    unsafe {
        *slot = id.slot() as u32;
        *state = reserved;
    }
    true
}
/// Find: the reserved state's slot.
///
/// # Safety
/// `ledger` is live; `slot` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rd_ledger_find(ledger: *const Ledger, owner_header: u64, generation: u64, born_epoch: u64, slot: *mut u32) -> bool {
    let Some(id) = (unsafe { &*ledger }).find(owner_header, generation, born_epoch) else { return false };
    unsafe { *slot = id.slot() as u32 };
    true
}
/// RetireMetadata (`which` 0) or AbandonUnusedReservation (1).
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rd_ledger_retire(ledger: *mut Ledger, which: u32, owner_header: u64, generation: u64, born_epoch: u64) -> bool {
    let ledger = unsafe { &mut *ledger };
    if which == 0 {
        ledger.retire_metadata(owner_header, generation, born_epoch)
    } else {
        ledger.abandon_unused_reservation(owner_header, generation, born_epoch)
    }
}
/// CancelSelection (`which` 0), Shutdown (1), Size (2), OwnerCount (3), Rejection (4).
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rd_ledger_op(ledger: *mut Ledger, which: u32) -> u64 {
    match which {
        0 => unsafe { (*ledger).cancel_selection() },
        1 => unsafe { (*ledger).shutdown() },
        2 => return unsafe { (*ledger).size() } as u64,
        3 => return unsafe { (*ledger).owner_count() } as u64,
        _ => return unsafe { (*ledger).rejection() } as u64,
    }
    0
}
fn intent_kind(value: u64) -> IntentKind {
    match value {
        1 => IntentKind::Reset,
        2 => IntentKind::Destroy,
        _ => IntentKind::None,
    }
}
/// A state's accessors and transitions, by `which`: CurrentPhase (0), AcquisitionsOpen (1), NativeIntentSeen (2),
/// NativeIntentKind (3), ExactHeaderDetached (4), DestroyAttemptConsumed (5), DestroyReturned (6), Quarantined (7),
/// Selected (8), AdmittedReaders (9), CompletedReaders (10), CanceledReaders (11), PendingReaders (12), Rejection
/// (13), CanRetireMetadata (14), CanAbandonUnusedReservation (15), HasPendingExtraReader (16: `a` the reader),
/// ReadyToDestroy (17: `a` the allocator proof); AdmitExtraReader (20: `a`), CompleteExtraReader (21: `a` the
/// completion proof), MarkExtraReaderEscaped (22: `a` the reader), CancelUnescapedExtraReader (23: `a` the cancellation
/// proof), UnsupportedExtraCompletion (24: `value` the lease), ObserveNativeIntent (25: `value` the kind, `a` the
/// source, `b` the facts, `c` the allocator proof), RecordHeaderTransfer (26: `a`), ObserveAllocator (27: `a`),
/// FinishDestroy (28: `value` normal return), CancelSelection (29), Shutdown (30). A boolean result is 0 or 1.
///
/// # Safety
/// `state` is a live reserved state; the records are valid where used.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rd_state(state: *mut State, which: u32, a: *const c_void, b: *const c_void, c: *const c_void, value: u64) -> u64 {
    if which < 20 {
        let state = unsafe { &*state };
        return match which {
            0 => state.current_phase() as u64,
            1 => state.acquisitions_open() as u64,
            2 => state.native_intent_seen() as u64,
            3 => state.native_intent_kind() as u64,
            4 => state.exact_header_detached() as u64,
            5 => state.destroy_attempt_consumed() as u64,
            6 => state.destroy_returned() as u64,
            7 => state.quarantined() as u64,
            8 => state.selected() as u64,
            9 => state.admitted_readers(),
            10 => state.completed_readers(),
            11 => state.canceled_readers(),
            12 => state.pending_readers() as u64,
            13 => state.rejection() as u64,
            14 => state.can_retire_metadata() as u64,
            15 => state.can_abandon_unused_reservation() as u64,
            16 => state.has_pending_extra_reader(unsafe { &*a.cast() }) as u64,
            _ => state.ready_to_destroy(unsafe { &*a.cast() }) as u64,
        };
    }
    let state = unsafe { &mut *state };
    unsafe {
        match which {
            20 => state.admit_extra_reader(&*a.cast()) as u64,
            21 => state.complete_extra_reader(&*a.cast()) as u64,
            22 => state.mark_extra_reader_escaped(&*a.cast()) as u64,
            23 => state.cancel_unescaped_extra_reader(&*a.cast()) as u64,
            24 => {
                state.unsupported_extra_completion(value);
                0
            }
            25 => state.observe_native_intent(intent_kind(value), &*a.cast(), *b.cast(), &*c.cast()) as u64,
            26 => state.record_header_transfer(&*a.cast()) as u64,
            27 => state.observe_allocator(&*a.cast()) as u64,
            28 => state.finish_destroy(value != 0) as u64,
            29 => {
                state.cancel_selection();
                0
            }
            _ => {
                state.shutdown();
                0
            }
        }
    }
}
/// FrozenHeader (`which` 0), FrozenAllocator (1), SavedHeader (2), BeginDestroy (3: `proof`, null when refused): the
/// stable addresses inside the state.
///
/// # Safety
/// `state` is a live reserved state; `proof` is valid for BeginDestroy.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rd_state_ptr(state: *mut State, which: u32, proof: *const AllocatorProof) -> *mut c_void {
    match which {
        0 => (unsafe { (*state).frozen_header() } as *const HeaderIdentity).cast_mut().cast(),
        1 => (unsafe { (*state).frozen_allocator() } as *const AllocatorIdentity).cast_mut().cast(),
        2 => (unsafe { (*state).saved_header() } as *const NativeHeader).cast_mut().cast(),
        _ => match unsafe { (*state).begin_destroy(&*proof) } {
            Some(header) => header.cast(),
            None => core::ptr::null_mut(),
        },
    }
}
