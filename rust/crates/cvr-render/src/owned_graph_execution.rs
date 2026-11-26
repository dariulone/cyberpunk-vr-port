//! The owned graph execution state's rules (`include/Render/OwnedGraphExecutionState.hpp`, `owned_graph_execution`):
//! one admitted consumer's lifecycle -- the producer's preconstruct graph slot, publication recorded before its
//! readback, verified rollback, the escape at the original construct call, the verified construct, the two aggregate
//! paths (declaration and execution: queued once with a pinned terminal group, original return, completion only after
//! the producer closed), the producer's close, selection cancellation, sticky quarantine once escaped -- and the
//! ledger's admission checks (a strictly increasing consumer id consumed even on refusal, the identity's graphs, frame
//! owner and prerequisite group, the owner generation bindings: distinct owners and generations, nonzero epochs and
//! protection receipts, at most 128).
//!
//! In the Rust build (`CVR_RUST_VRIK`) `State`'s rules and the ledger's admission checks forward to `cvr_oge_*`; the
//! state's members stay in C++ (the ledger's map owns them), mirrored here. The typed expansion admission and the map
//! stay in C++. `tools/single_pass_tests/owned_graph_execution_state_test` runs its cases against these in its Rust
//! variant.

pub const MAX_OWNER_BINDINGS: usize = 128;

/// owned_graph_execution::Phase.
pub mod phase {
    pub const PREPARED: u32 = 0;
    pub const PUBLISHED: u32 = 1;
    pub const ROLLBACK_REQUIRED: u32 = 2;
    pub const ESCAPED: u32 = 3;
    pub const CANCELLED: u32 = 4;
    pub const QUARANTINED: u32 = 5;
}
/// owned_graph_execution::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const SEQUENCE: u32 = 2;
    pub const BINDINGS: u32 = 6;
    pub const IDENTITY: u32 = 7;
    pub const PRODUCER_SCOPE: u32 = 8;
    pub const ORDER: u32 = 9;
    pub const PUBLICATION: u32 = 10;
    pub const ROLLBACK: u32 = 11;
    pub const CONSTRUCT: u32 = 12;
    pub const TERMINAL_GROUP: u32 = 13;
    pub const DUPLICATE: u32 = 14;
    pub const INCOMPLETE: u32 = 15;
    pub const MISSING_PROOF: u32 = 16;
}

/// owned_graph_execution::Identity.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub consumer_id: u64,
    pub epoch: u64,
    pub pair_generation: u64,
    pub source_graph: u64,
    pub execution_graph: u64,
    pub frame_owner: u64,
    pub prerequisite_group: u64,
}
/// owned_graph_execution::OwnerGenerationBinding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OwnerGenerationBinding {
    pub owner_header: u64,
    pub generation_id: u64,
    pub born_mutation_epoch: u64,
    pub protection_receipt: u64,
}
/// State's path reader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Reader {
    pub group: u64,
    pub queued: bool,
    pub original_returned: bool,
    pub complete: bool,
}
/// State's members.
#[repr(C)]
pub struct State {
    pub identity: Identity,
    pub bindings: [OwnerGenerationBinding; MAX_OWNER_BINDINGS],
    pub binding_count: usize,
    pub producer_scope: u64,
    pub graph_slot: u64,
    pub paths: [Reader; 2],
    pub phase: u32,
    pub failure: u32,
    pub selected: bool,
    pub escaped: bool,
    pub producer_open: bool,
    pub construct_verified: bool,
}
const _: () = assert!(
    core::mem::size_of::<Identity>() == 56
        && core::mem::size_of::<OwnerGenerationBinding>() == 32
        && core::mem::size_of::<Reader>() == 16
        && core::mem::size_of::<State>() == 4224
        && core::mem::offset_of!(State, paths) == 4176
        && core::mem::offset_of!(State, phase) == 4208
);

fn pointer(address: u64, bytes: u64) -> bool {
    address != 0 && address & 7 == 0 && address <= u64::MAX - bytes
}
fn valid_path(path: u32) -> bool {
    path == 0 || path == 1
}

impl State {
    fn quarantine(&mut self, reason: u32) -> bool {
        if self.phase != phase::QUARANTINED {
            self.failure = reason;
        }
        self.phase = phase::QUARANTINED;
        self.graph_slot = 0;
        self.selected = false;
        false
    }
    fn reject(&mut self, reason: u32) -> bool {
        if self.phase == phase::QUARANTINED {
            return false;
        }
        if self.escaped {
            return self.quarantine(reason);
        }
        self.failure = reason;
        if self.phase == phase::PUBLISHED || self.phase == phase::ROLLBACK_REQUIRED {
            self.phase = phase::ROLLBACK_REQUIRED;
        } else if self.phase == phase::PREPARED {
            self.phase = phase::CANCELLED;
            self.graph_slot = 0;
            self.selected = false;
        }
        false
    }
    fn check_identity(&mut self, proof: &Identity) -> bool {
        if self.phase == phase::QUARANTINED {
            return false;
        }
        *proof == self.identity || self.reject(failure::IDENTITY)
    }
    fn check_scope(&mut self, scope: u64) -> bool {
        (self.producer_open && scope == self.producer_scope) || self.reject(failure::PRODUCER_SCOPE)
    }
    fn check_path(&mut self, path: u32, proof: &Identity, group: u64) -> bool {
        if !self.check_identity(proof) {
            return false;
        }
        if !valid_path(path) {
            return self.reject(failure::ARGUMENTS);
        }
        if self.phase != phase::ESCAPED || !self.paths[path as usize].queued {
            return self.reject(failure::ORDER);
        }
        self.paths[path as usize].group == group || self.reject(failure::TERMINAL_GROUP)
    }
    /// PreconstructGraphSlot: only the same producer, before originalConstruct.
    pub fn preconstruct_graph_slot(&self, producer_scope: u64) -> u64 {
        let live = matches!(self.phase, phase::PREPARED | phase::PUBLISHED | phase::ROLLBACK_REQUIRED);
        if self.producer_open && !self.escaped && producer_scope == self.producer_scope && live { self.graph_slot } else { 0 }
    }
    pub fn can_rollback(&self, proof: &Identity, producer_scope: u64) -> bool {
        *proof == self.identity
            && self.preconstruct_graph_slot(producer_scope) != 0
            && (self.phase == phase::PUBLISHED || self.phase == phase::ROLLBACK_REQUIRED)
    }
    /// RecordPublication: every attempt is recorded immediately, uncertain writes included.
    pub fn record_publication(&mut self, proof: &Identity, producer_scope: u64, owned_slot_verified: bool) -> bool {
        if self.phase != phase::PREPARED {
            return self.quarantine(failure::ORDER);
        }
        self.phase = phase::ROLLBACK_REQUIRED;
        if !self.check_identity(proof) || !self.check_scope(producer_scope) {
            return false;
        }
        if !owned_slot_verified {
            return self.reject(failure::PUBLICATION);
        }
        self.phase = phase::PUBLISHED;
        self.failure = failure::NONE;
        true
    }
    pub fn record_rollback(&mut self, proof: &Identity, producer_scope: u64, original_slot_verified: bool) -> bool {
        if !self.check_identity(proof) {
            return false;
        }
        if !self.can_rollback(proof, producer_scope) {
            return self.reject(if self.escaped { failure::ORDER } else { failure::PRODUCER_SCOPE });
        }
        if !original_slot_verified {
            return self.quarantine(failure::ROLLBACK);
        }
        self.phase = phase::CANCELLED;
        self.graph_slot = 0;
        self.selected = false;
        self.failure = failure::NONE;
        true
    }
    /// MarkConstructInvoked: immediately before originalConstruct; the escape cannot be undone.
    pub fn mark_construct_invoked(&mut self, proof: &Identity, producer_scope: u64) -> bool {
        if !self.check_identity(proof) || !self.check_scope(producer_scope) {
            return false;
        }
        if self.phase != phase::PUBLISHED {
            return self.reject(failure::ORDER);
        }
        self.escaped = true;
        self.phase = phase::ESCAPED;
        self.graph_slot = 0;
        self.failure = failure::NONE;
        true
    }
    pub fn record_construct(&mut self, proof: &Identity, payload_matched: bool, prerequisite_consumed: bool) -> bool {
        if !self.check_identity(proof) {
            return false;
        }
        if self.phase != phase::ESCAPED {
            return self.reject(failure::ORDER);
        }
        if self.construct_verified {
            return self.reject(failure::DUPLICATE);
        }
        if !payload_matched || !prerequisite_consumed {
            return self.reject(failure::CONSTRUCT);
        }
        self.construct_verified = true;
        self.failure = failure::NONE;
        true
    }
    /// RecordQueued: one event per aggregate path, its pinned group covering every native descendant.
    pub fn record_queued(&mut self, path: u32, proof: &Identity, terminal_group: u64, terminal_pinned: bool) -> bool {
        if !self.check_identity(proof) {
            return false;
        }
        if !valid_path(path) {
            return self.reject(failure::ARGUMENTS);
        }
        if self.phase != phase::ESCAPED || !self.construct_verified || !self.producer_open {
            return self.reject(failure::ORDER);
        }
        if self.paths[path as usize].queued {
            return self.reject(failure::DUPLICATE);
        }
        if !pointer(terminal_group, 0x20)
            || terminal_group == self.identity.prerequisite_group
            || terminal_group == self.identity.frame_owner
            || !terminal_pinned
        {
            return self.reject(failure::TERMINAL_GROUP);
        }
        let reader = &mut self.paths[path as usize];
        reader.group = terminal_group;
        reader.queued = true;
        self.failure = failure::NONE;
        true
    }
    pub fn record_original_returned(&mut self, path: u32, proof: &Identity, terminal_group: u64) -> bool {
        if !self.check_path(path, proof, terminal_group) {
            return false;
        }
        if self.paths[path as usize].original_returned {
            return self.reject(failure::DUPLICATE);
        }
        self.paths[path as usize].original_returned = true;
        self.failure = failure::NONE;
        true
    }
    /// ObserveCompletion: pending work is a normal incomplete probe, not a failure.
    pub fn observe_completion(&mut self, path: u32, proof: &Identity, terminal_group: u64, pinned_group_complete: bool) -> bool {
        if !self.check_identity(proof) {
            return false;
        }
        if !valid_path(path) {
            return self.reject(failure::ARGUMENTS);
        }
        if self.phase != phase::ESCAPED {
            return self.reject(failure::ORDER);
        }
        let reader = self.paths[path as usize];
        if reader.queued && reader.group != terminal_group {
            return self.reject(failure::TERMINAL_GROUP);
        }
        if !reader.queued || self.producer_open || !reader.original_returned || !pinned_group_complete {
            self.failure = failure::INCOMPLETE;
            return false;
        }
        self.paths[path as usize].complete = true;
        self.failure = failure::NONE;
        true
    }
    pub fn record_producer_closed(&mut self, proof: &Identity, producer_scope: u64) -> bool {
        if !self.check_identity(proof) || !self.check_scope(producer_scope) {
            return false;
        }
        self.producer_open = false;
        self.graph_slot = 0;
        if self.phase == phase::PREPARED {
            self.phase = phase::CANCELLED;
            self.selected = false;
            self.failure = failure::NONE;
            return true;
        }
        if self.phase == phase::CANCELLED {
            return true;
        }
        if self.phase != phase::ESCAPED || !self.construct_verified || !self.paths[0].queued || !self.paths[1].queued {
            return self.quarantine(failure::MISSING_PROOF);
        }
        self.failure = failure::NONE;
        true
    }
    pub fn cancel_selection(&mut self) {
        self.selected = false;
        if self.phase == phase::PREPARED {
            self.phase = phase::CANCELLED;
            self.graph_slot = 0;
        } else if self.phase == phase::PUBLISHED {
            self.phase = phase::ROLLBACK_REQUIRED;
        }
    }
    pub fn report_failure(&mut self, reason: u32) {
        self.reject(if reason == failure::NONE { failure::MISSING_PROOF } else { reason });
    }
    pub fn queued(&self, path: u32) -> bool {
        valid_path(path) && self.paths[path as usize].queued
    }
    pub fn original_returned(&self, path: u32) -> bool {
        valid_path(path) && self.paths[path as usize].original_returned
    }
    pub fn complete(&self, path: u32) -> bool {
        valid_path(path) && self.paths[path as usize].complete
    }
    pub fn terminal_group(&self, path: u32) -> u64 {
        if valid_path(path) { self.paths[path as usize].group } else { 0 }
    }
    pub fn can_release_owned_storage(&self) -> bool {
        if self.phase == phase::CANCELLED {
            return !self.escaped;
        }
        self.phase == phase::ESCAPED
            && self.escaped
            && self.construct_verified
            && !self.producer_open
            && self.paths.iter().all(|reader| reader.queued && reader.original_returned && reader.complete)
    }
}

/// Ledger::BeginAdmission: (failure, last consumer id, admitted); the id is consumed even when the rest is refused.
pub fn begin_admission(identity: &Identity, last_consumer: u64, producer_scope: u64, graph_slot: u64) -> (u32, u64, bool) {
    if identity.consumer_id == 0 || identity.consumer_id <= last_consumer {
        return (failure::SEQUENCE, last_consumer, false);
    }
    let admitted = identity.epoch != 0
        && identity.pair_generation != 0
        && producer_scope != 0
        && pointer(graph_slot, 8)
        && pointer(identity.source_graph, 0x70)
        && pointer(identity.execution_graph, 0x70)
        && identity.source_graph != identity.execution_graph
        && pointer(identity.frame_owner, 12)
        && pointer(identity.prerequisite_group, 0x20)
        && identity.frame_owner != identity.prerequisite_group;
    (failure::ARGUMENTS, identity.consumer_id, admitted)
}
/// AdmitVerified's binding checks.
pub fn verified_bindings(bindings: &[OwnerGenerationBinding]) -> bool {
    if bindings.len() > MAX_OWNER_BINDINGS {
        return false;
    }
    for (i, value) in bindings.iter().enumerate() {
        if !pointer(value.owner_header, 16) || value.generation_id == 0 || value.born_mutation_epoch == 0 || value.protection_receipt == 0 {
            return false;
        }
        if bindings[..i].iter().any(|earlier| earlier.owner_header == value.owner_header || earlier.generation_id == value.generation_id) {
            return false;
        }
    }
    true
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// State, by `which`: PreconstructGraphSlot (0: `scope`, the slot), CanRollback (1: `proof`, `scope`), Queued (2),
/// OriginalReturned (3), Complete (4), TerminalGroup (5, the group), CanReleaseOwnedStorage (6) -- `path` for 2..5;
/// RecordPublication (10: `proof`, `scope`, `flag` owned slot verified), RecordRollback (11: `proof`, `scope`, `flag`
/// original slot verified), MarkConstructInvoked (12: `proof`, `scope`), RecordConstruct (13: `proof`, `flag` payload
/// matched, `flag2` prerequisite consumed), RecordQueued (14: `path`, `proof`, `scope` the group, `flag` pinned),
/// RecordOriginalReturned (15: `path`, `proof`, `scope` the group), ObserveCompletion (16: `path`, `proof`, `scope` the
/// group, `flag` the pinned group complete), RecordProducerClosed (17: `proof`, `scope`), CancelSelection (18),
/// ReportFailure (19: `path` the reason). A boolean result is 0 or 1.
///
/// # Safety
/// `state` is a live state; `proof` is valid where used.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_oge_state(state: *mut State, which: u32, proof: *const Identity, path: u32, scope: u64, flag: bool, flag2: bool) -> u64 {
    if which < 10 {
        let state = unsafe { &*state };
        return match which {
            0 => state.preconstruct_graph_slot(scope),
            1 => state.can_rollback(unsafe { &*proof }, scope) as u64,
            2 => state.queued(path) as u64,
            3 => state.original_returned(path) as u64,
            4 => state.complete(path) as u64,
            5 => state.terminal_group(path),
            _ => state.can_release_owned_storage() as u64,
        };
    }
    let state = unsafe { &mut *state };
    let proof = || unsafe { &*proof };
    (match which {
        10 => state.record_publication(proof(), scope, flag),
        11 => state.record_rollback(proof(), scope, flag),
        12 => state.mark_construct_invoked(proof(), scope),
        13 => state.record_construct(proof(), flag, flag2),
        14 => state.record_queued(path, proof(), scope, flag),
        15 => state.record_original_returned(path, proof(), scope),
        16 => state.observe_completion(path, proof(), scope, flag),
        17 => state.record_producer_closed(proof(), scope),
        18 => {
            state.cancel_selection();
            false
        }
        _ => {
            state.report_failure(path);
            false
        }
    }) as u64
}
/// Ledger::BeginAdmission: `failure` and `last` always written.
///
/// # Safety
/// `identity` is valid; `failure` and `last` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_oge_begin_admission(identity: *const Identity, producer_scope: u64, graph_slot: u64, failure: *mut u32, last: *mut u64) -> bool {
    let (reason, consumed, admitted) = unsafe { begin_admission(&*identity, *last, producer_scope, graph_slot) };
    unsafe {
        *failure = reason;
        *last = consumed;
    }
    admitted
}
/// AdmitVerified's binding checks.
///
/// # Safety
/// `bindings` holds `count` bindings (or is null with none).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_oge_verified_bindings(bindings: *const OwnerGenerationBinding, count: usize) -> bool {
    let bindings = if bindings.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(bindings, count) } };
    verified_bindings(bindings)
}
