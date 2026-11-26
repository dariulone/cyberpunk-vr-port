//! The C ABI of the definition owners' generation ledger (`native::definition_generations`) for `include/Render/
//! NativeDefinitionGenerations.hpp` in the Rust build (`CVR_RUST_VRIK`): the C++ `Ledger` owns the Rust ledger and
//! forwards every method; a generation is boxed there, so GetGeneration's pointer stays valid while it is tracked, as a
//! std::map node's did. The Rust ledger does not fail on allocation (the C++ one reported Allocation from its map
//! insertions). `tools/single_pass_tests/definition_generations_test` runs its cases against these in its Rust variant.

use crate::native::definition_generations::*;

const _: () = assert!(
    core::mem::size_of::<HeaderObservation>() == 40
        && core::mem::size_of::<AggregateTerminalProof>() == 32
        && core::mem::size_of::<Generation>() == 88
        && core::mem::offset_of!(Generation, unsupported_reason) == 80
);

/// A failure value from C++ (one of the enum's; any other is Unsupported).
fn failure(value: u32) -> Failure {
    const ALL: [Failure; 19] = [
        Failure::None,
        Failure::Arguments,
        Failure::Sequence,
        Failure::Mutation,
        Failure::Generation,
        Failure::Epoch,
        Failure::Duplicate,
        Failure::Consumer,
        Failure::Capacity,
        Failure::SourceClosed,
        Failure::TerminalProof,
        Failure::Unsupported,
        Failure::Detach,
        Failure::PriorCoverage,
        Failure::Incomplete,
        Failure::Retirement,
        Failure::Allocation,
        Failure::NoSourceProof,
        Failure::Unsealed,
    ];
    ALL.get(value as usize).copied().unwrap_or(Failure::Unsupported)
}

/// A new empty ledger, owned by the C++ face.
#[no_mangle]
pub extern "C-unwind" fn cvr_dg_ledger_new() -> *mut Ledger {
    Box::into_raw(Box::default())
}
/// Frees a ledger made by `cvr_dg_ledger_new`.
///
/// # Safety
/// `ledger` came from `cvr_dg_ledger_new` and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_ledger_free(ledger: *mut Ledger) {
    if !ledger.is_null() {
        drop(unsafe { Box::from_raw(ledger) });
    }
}
/// BirthGeneration: the new generation's id (0: rejected).
///
/// # Safety
/// `ledger` is live; `observation` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_birth(ledger: *mut Ledger, observation: *const HeaderObservation, prior: bool) -> u64 {
    unsafe { (*ledger).birth_generation(&*observation, prior) }
}
/// FindGeneration.
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_find(ledger: *const Ledger, owner_header: u64, born_mutation_epoch: u64) -> u64 {
    unsafe { (*ledger).find_generation(owner_header, born_mutation_epoch) }
}
/// GetGeneration: the tracked generation's stable address, or null.
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_get(ledger: *const Ledger, generation_id: u64) -> *const Generation {
    match unsafe { (*ledger).get_generation(generation_id) } {
        Some(generation) => generation,
        None => core::ptr::null(),
    }
}
/// Consumers by `which`: RegisterConsumer (0), RegisterProvisionalConsumer (1), SealProvisionalConsumer (2),
/// HasConsumer (3), ConsumerBound (4: `a` the generation), BindConsumer (5: `a` the generation, `b` its birth epoch,
/// `value` the admission), UnsupportedConsumer (6: `value` the reason), CancelUnboundConsumer (7: `value` the native
/// no-source proof).
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_dg_consumer(
    ledger: *mut Ledger,
    which: u32,
    consumer_id: u64,
    request_epoch: u64,
    pair_generation: u64,
    a: u64,
    b: u64,
    value: u32,
) -> bool {
    match which {
        3 => return unsafe { (*ledger).has_consumer(consumer_id) },
        4 => return unsafe { (*ledger).consumer_bound(consumer_id, a) },
        _ => {}
    }
    let ledger = unsafe { &mut *ledger };
    match which {
        0 => ledger.register_consumer(consumer_id, request_epoch, pair_generation),
        1 => ledger.register_provisional_consumer(consumer_id),
        2 => ledger.seal_provisional_consumer(consumer_id, request_epoch, pair_generation),
        5 => {
            let admission = if value == 1 { Admission::Execution } else { Admission::Source };
            ledger.bind_consumer(consumer_id, request_epoch, pair_generation, a, b, admission)
        }
        6 => ledger.unsupported_consumer(consumer_id, request_epoch, pair_generation, failure(value)),
        _ => ledger.cancel_unbound_consumer(consumer_id, request_epoch, pair_generation, value != 0),
    }
}
/// CompleteConsumer.
///
/// # Safety
/// `ledger` is live; `proof` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_complete(ledger: *mut Ledger, proof: *const AggregateTerminalProof) -> bool {
    unsafe { (*ledger).complete_consumer(&*proof) }
}
/// Generations by `which`: UnsupportedGeneration (0: `value` the reason), CloseSource (1: `closing` epoch),
/// VerifyPriorConsumers (2: `value` accounted), VerifyDetach (3: `value` detached), CanAdopt (4: `value` the native
/// exclusivity proof), OwnershipEligible (5), ReadyToRetire (6), BeginRetire (7), FinishRetire (8),
/// ForgetClosedObservation (9).
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_generation(ledger: *mut Ledger, which: u32, generation_id: u64, born_mutation_epoch: u64, closing: u64, value: u32) -> bool {
    match which {
        4 => return unsafe { (*ledger).can_adopt(generation_id, value != 0) },
        5 => return unsafe { (*ledger).ownership_eligible(generation_id) },
        6 => return unsafe { (*ledger).ready_to_retire(generation_id) },
        _ => {}
    }
    let ledger = unsafe { &mut *ledger };
    match which {
        0 => ledger.unsupported_generation(generation_id, born_mutation_epoch, failure(value)),
        1 => ledger.close_source(generation_id, born_mutation_epoch, closing),
        2 => ledger.verify_prior_consumers(generation_id, born_mutation_epoch, value != 0),
        3 => ledger.verify_detach(generation_id, born_mutation_epoch, value != 0),
        7 => ledger.begin_retire(generation_id, born_mutation_epoch),
        8 => ledger.finish_retire(generation_id, born_mutation_epoch),
        _ => ledger.forget_closed_observation(generation_id, born_mutation_epoch),
    }
}
/// The counts by `which`: ActiveConsumers (0), ActiveGenerations (1), TrackedOwners (2), LastGenerationId (3),
/// LastConsumerId (4), CompletedConsumers (5), UnsupportedConsumers (6), CancelledConsumers (7), RetiredGenerations (8),
/// ForgottenObservations (9), Rejection (10).
///
/// # Safety
/// `ledger` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dg_count(ledger: *const Ledger, which: u32) -> u64 {
    let ledger = unsafe { &*ledger };
    match which {
        0 => ledger.active_consumers() as u64,
        1 => ledger.active_generations() as u64,
        2 => ledger.tracked_owners() as u64,
        3 => ledger.last_generation_id(),
        4 => ledger.last_consumer_id(),
        5 => ledger.completed_consumers(),
        6 => ledger.unsupported_consumers(),
        7 => ledger.cancelled_consumers(),
        8 => ledger.retired_generations(),
        9 => ledger.forgotten_observations(),
        _ => ledger.rejection() as u64,
    }
}
