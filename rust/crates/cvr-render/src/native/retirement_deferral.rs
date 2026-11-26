//! The deferred retirement of a render graph definition's native owner vector (include/Render/
//! NativeRetirementDeferral.hpp).
//!
//! A `State` tracks only the readers CVR added to one frozen owner header: admitted while acquisitions are open,
//! released only by an actual producer/declaration/execution completion proof (or a verified cancellation before any
//! native escape), never by a timeout or a zero count. The exact native retirement intent (Reset or Destroy) closes
//! acquisitions; the header is then transferred to a saved copy and destroyed exactly once through that stable copy
//! (`begin_destroy` consumes the attempt before the caller invokes original986E74 outside the gate). Anything inexact
//! quarantines the state, which keeps its readers and storage but never authorizes destruction. Unknown vanilla users
//! remain the native retirement point's responsibility; this never waits or writes native memory.
//!
//! The `Ledger` reserves each State with its fixed reader slots before any reader is admitted, keeps bounded per-owner
//! ABA watermarks, and refuses overlapping saved allocations. States are addressed by `StateId` (slot and serial), so a
//! retired state's id never reaches its slot's successor. The layout audit must establish data as the vector
//! allocation base with capacity*8 entry bytes and its 8-byte allocator trailer; the ledger must outlive its readers
//! and any begun native destroy call; nothing native is freed when it drops. This is not an adoption gate.

pub const MAX_OWNERS: usize = 128;
pub const MAX_STATES: usize = 256;
pub const MAX_EXTRA_READERS: usize = 256;
pub const MAX_DEFINITION_SLOTS: u32 = 8192;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeHeader {
    pub data: u64,
    pub capacity: u32,
    pub count: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderIdentity {
    pub owner_header: u64,
    pub generation: u64,
    pub born_epoch: u64,
    pub saved_native: NativeHeader,
    pub empty_allocator: NativeHeader,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocatorIdentity {
    pub address: u64,
    pub generation: u64,
}
/// Fresh typed guarded Reset/Destroy scope evidence, including matching object/control/string allocator globals (a
/// pointer read is not liveness).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AllocatorProof {
    pub identity: AllocatorIdentity,
    pub infrastructure_matches: bool,
    pub alive: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReservationAllocatorObservation {
    pub identity: AllocatorIdentity,
    pub infrastructure_matches: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReservationFacts {
    pub retirement_path_audited: bool,
    pub layout_audited: bool,
    pub destructor_tree_safe: bool,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IntentKind {
    #[default]
    None,
    Reset,
    Destroy,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeIntentFacts {
    pub exact_native_retirement_point: bool,
    pub layout_audited: bool,
    pub unique_writer: bool,
    pub destructor_tree_safe: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HeaderTransferProof {
    pub source_before: HeaderIdentity,
    pub saved_copy: NativeHeader,
    pub source_after: NativeHeader,
    pub successful_exact_transfer: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExtraReaderIdentity {
    pub lease_id: u64,
    pub consumer_id: u64,
    pub epoch: u64,
    pub pair_generation: u64,
    pub source_graph: u64,
    pub execution_graph: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct OwnedGraphCompletionProof {
    pub reader: ExtraReaderIdentity,
    pub owned_identity_verified: bool,
    pub producer_closed: bool,
    pub original_producer_returned: bool,
    pub declaration_queued: bool,
    pub original_declaration_returned: bool,
    pub declaration_complete: bool,
    pub execution_queued: bool,
    pub original_execution_returned: bool,
    pub execution_aggregate_complete: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UnescapedCancellationProof {
    pub reader: ExtraReaderIdentity,
    pub native_original_never_invoked: bool,
    pub verified_original_slot_restored: bool,
    pub no_native_escape: bool,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Reserved,
    AcquisitionsClosed,
    IntentClosed,
    Detached,
    Destroying,
    Destroyed,
    Quarantined,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    ReservationFacts,
    Capacity,
    Allocation,
    GenerationReplay,
    UnresolvedSource,
    StorageOverlap,
    Closed,
    ReaderSequence,
    ReaderIdentity,
    ReaderMissing,
    Incomplete,
    UnsupportedCompletion,
    CancellationClosed,
    CancellationProof,
    HeaderChanged,
    IntentFacts,
    IntentKind,
    NestedIntent,
    NoIntent,
    TransferRepeated,
    TransferFailed,
    AllocatorChanged,
    Shutdown,
    DestroyRepeated,
    DestroyNotBegun,
    DestroyFailed,
}

fn pointer(value: u64, bytes: u64) -> bool {
    value != 0 && value & 7 == 0 && value <= u64::MAX - bytes
}
fn overlap(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    a < b.wrapping_add(b_bytes) && b < a.wrapping_add(a_bytes)
}
fn allocation_bytes(header: &NativeHeader) -> u64 {
    if header.capacity != 0 {
        header.capacity as u64 * 8 + 8
    } else {
        0
    }
}
fn header_valid(identity: &HeaderIdentity) -> bool {
    let (saved, empty) = (&identity.saved_native, &identity.empty_allocator);
    if !pointer(identity.owner_header, 16)
        || identity.generation == 0
        || identity.born_epoch == 0
        || !pointer(empty.data, 8)
        || empty.capacity != 0
        || empty.count != 0
        || saved.capacity > MAX_DEFINITION_SLOTS
        || saved.count > saved.capacity
        || !pointer(saved.data, 8)
        || overlap(identity.owner_header, 16, empty.data, 8)
    {
        return false;
    }
    if saved.capacity == 0 {
        return saved.data == empty.data;
    }
    let bytes = allocation_bytes(saved);
    pointer(saved.data, bytes)
        && !overlap(saved.data, bytes, empty.data, 8)
        && !overlap(saved.data, bytes, identity.owner_header, 16)
}
fn reader_valid(reader: &ExtraReaderIdentity) -> bool {
    reader.consumer_id != 0
        && reader.epoch != 0
        && reader.pair_generation != 0
        && pointer(reader.source_graph, 0x70)
        && pointer(reader.execution_graph, 0x70)
        && reader.source_graph != reader.execution_graph
}

#[derive(Clone, Copy, Default)]
struct Reader {
    identity: ExtraReaderIdentity,
    active: bool,
    native_escape_known: bool,
}

/// One frozen owner header's added readers and retirement. The caller serializes all methods under a short gate and
/// supplies audited facts.
pub struct State {
    identity: HeaderIdentity,
    allocator: AllocatorIdentity,
    destruction_header: NativeHeader,
    readers: Box<[Reader; MAX_EXTRA_READERS]>,
    last_reader_lease: u64,
    admitted_readers: u64,
    completed_readers: u64,
    canceled_readers: u64,
    pending_readers: usize,
    kind: IntentKind,
    failure: Failure,
    selected: bool,
    acquisitions_closed: bool,
    intent_attempted: bool,
    intent_seen: bool,
    transfer_attempted: bool,
    detached: bool,
    destroy_attempted: bool,
    destroy_finished: bool,
    destroyed: bool,
    quarantined: bool,
    shutdown: bool,
}
impl State {
    fn new(
        identity: HeaderIdentity,
        allocator: AllocatorIdentity,
        previous_reader_lease: u64,
    ) -> Self {
        State {
            identity,
            allocator,
            destruction_header: identity.saved_native,
            readers: Box::new([Reader::default(); MAX_EXTRA_READERS]),
            last_reader_lease: previous_reader_lease,
            admitted_readers: 0,
            completed_readers: 0,
            canceled_readers: 0,
            pending_readers: 0,
            kind: IntentKind::None,
            failure: Failure::None,
            selected: true,
            acquisitions_closed: false,
            intent_attempted: false,
            intent_seen: false,
            transfer_attempted: false,
            detached: false,
            destroy_attempted: false,
            destroy_finished: false,
            destroyed: false,
            quarantined: false,
            shutdown: false,
        }
    }
    pub fn frozen_header(&self) -> &HeaderIdentity {
        &self.identity
    }
    pub fn frozen_allocator(&self) -> &AllocatorIdentity {
        &self.allocator
    }
    pub fn saved_header(&self) -> &NativeHeader {
        &self.destruction_header
    }
    pub fn current_phase(&self) -> Phase {
        if self.quarantined {
            Phase::Quarantined
        } else if self.destroyed {
            Phase::Destroyed
        } else if self.destroy_attempted {
            Phase::Destroying
        } else if self.detached {
            Phase::Detached
        } else if self.intent_seen {
            Phase::IntentClosed
        } else if self.acquisitions_closed {
            Phase::AcquisitionsClosed
        } else {
            Phase::Reserved
        }
    }
    pub fn acquisitions_open(&self) -> bool {
        !self.acquisitions_closed && !self.quarantined
    }
    pub fn native_intent_seen(&self) -> bool {
        self.intent_seen
    }
    pub fn native_intent_kind(&self) -> IntentKind {
        self.kind
    }
    pub fn exact_header_detached(&self) -> bool {
        self.detached
    }
    pub fn destroy_attempt_consumed(&self) -> bool {
        self.destroy_attempted
    }
    pub fn destroy_returned(&self) -> bool {
        self.destroyed
    }
    pub fn quarantined(&self) -> bool {
        self.quarantined
    }
    pub fn selected(&self) -> bool {
        self.selected
    }
    pub fn admitted_readers(&self) -> u64 {
        self.admitted_readers
    }
    pub fn completed_readers(&self) -> u64 {
        self.completed_readers
    }
    pub fn canceled_readers(&self) -> u64 {
        self.canceled_readers
    }
    pub fn pending_readers(&self) -> usize {
        self.pending_readers
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
    /// A read-only exact lookup, not a new acquisition: closing the native intent (or the selection) does not
    /// invalidate a reader still protecting the saved owner.
    pub fn has_pending_extra_reader(&self, identity: &ExtraReaderIdentity) -> bool {
        if self.quarantined || self.shutdown || self.destroy_attempted || self.destroyed {
            return false;
        }
        self.readers
            .iter()
            .any(|r| r.active && r.identity == *identity)
    }
    pub fn admit_extra_reader(&mut self, reader: &ExtraReaderIdentity) -> bool {
        if reader.lease_id == 0 || reader.lease_id <= self.last_reader_lease {
            return self.reject(Failure::ReaderSequence);
        }
        self.last_reader_lease = reader.lease_id;
        if !self.acquisitions_open() {
            return self.reject(Failure::Closed);
        }
        if !reader_valid(reader) {
            return self.reject(Failure::ReaderIdentity);
        }
        if self.pending_readers >= MAX_EXTRA_READERS || self.admitted_readers == u64::MAX {
            return self.reject(Failure::Capacity);
        }
        if self
            .readers
            .iter()
            .any(|s| s.active && s.identity.consumer_id == reader.consumer_id)
        {
            return self.reject(Failure::ReaderIdentity);
        }
        let Some(slot) = self.readers.iter_mut().find(|s| !s.active) else {
            return self.reject(Failure::Capacity);
        };
        *slot = Reader {
            identity: *reader,
            active: true,
            native_escape_known: false,
        };
        self.pending_readers += 1;
        self.admitted_readers += 1;
        self.failure = Failure::None;
        true
    }
    /// An incomplete observation retains the reader: only actual producer, declaration and execution proofs release
    /// its slot (a cancellation is no such proof).
    pub fn complete_extra_reader(&mut self, proof: &OwnedGraphCompletionProof) -> bool {
        let Some(slot) = self.find_reader(proof.reader.lease_id) else {
            return self.reject(Failure::ReaderMissing);
        };
        if self.readers[slot].identity != proof.reader {
            return self.quarantine(Failure::ReaderIdentity);
        }
        if !proof.owned_identity_verified
            || !proof.producer_closed
            || !proof.original_producer_returned
            || !proof.declaration_queued
            || !proof.original_declaration_returned
            || !proof.declaration_complete
            || !proof.execution_queued
            || !proof.original_execution_returned
            || !proof.execution_aggregate_complete
        {
            return self.reject(Failure::Incomplete);
        }
        self.readers[slot] = Reader::default();
        self.pending_readers -= 1;
        self.completed_readers += 1;
        if !self.quarantined {
            self.failure = Failure::None;
        }
        true
    }
    pub fn mark_extra_reader_escaped(&mut self, identity: &ExtraReaderIdentity) -> bool {
        let Some(slot) = self.find_reader(identity.lease_id) else {
            return self.reject(Failure::ReaderMissing);
        };
        if self.readers[slot].identity != *identity {
            return self.quarantine(Failure::ReaderIdentity);
        }
        self.readers[slot].native_escape_known = true;
        true
    }
    pub fn cancel_unescaped_extra_reader(&mut self, proof: &UnescapedCancellationProof) -> bool {
        let Some(slot) = self.find_reader(proof.reader.lease_id) else {
            return self.reject(Failure::ReaderMissing);
        };
        if self.readers[slot].identity != proof.reader {
            return self.quarantine(Failure::ReaderIdentity);
        }
        if self.intent_attempted
            || self.transfer_attempted
            || self.detached
            || self.destroy_attempted
            || self.shutdown
            || self.quarantined
        {
            return self.reject(Failure::CancellationClosed);
        }
        if self.readers[slot].native_escape_known
            || !proof.native_original_never_invoked
            || !proof.no_native_escape
        {
            self.readers[slot].native_escape_known = true;
            return self.reject(Failure::CancellationProof);
        }
        if !proof.verified_original_slot_restored {
            return self.reject(Failure::CancellationProof);
        }
        self.readers[slot] = Reader::default();
        self.pending_readers -= 1;
        self.canceled_readers += 1;
        self.failure = Failure::None;
        true
    }
    pub fn unsupported_extra_completion(&mut self, lease_id: u64) {
        let reason = if self.find_reader(lease_id).is_some() {
            Failure::UnsupportedCompletion
        } else {
            Failure::ReaderMissing
        };
        self.quarantine(reason);
    }
    /// Closing and sealing the exact native intent is one transition under the caller's gate; it closes only CVR
    /// acquisitions and never claims prior coverage.
    pub fn observe_native_intent(
        &mut self,
        kind: IntentKind,
        source: &HeaderIdentity,
        facts: NativeIntentFacts,
        allocator: &AllocatorProof,
    ) -> bool {
        self.acquisitions_closed = true;
        if self.intent_attempted {
            return self.quarantine(Failure::NestedIntent);
        }
        self.intent_attempted = true;
        if kind != IntentKind::Reset && kind != IntentKind::Destroy {
            return self.quarantine(Failure::IntentKind);
        }
        if *source != self.identity {
            return self.quarantine(Failure::HeaderChanged);
        }
        if !facts.exact_native_retirement_point
            || !facts.layout_audited
            || !facts.unique_writer
            || !facts.destructor_tree_safe
        {
            return self.quarantine(Failure::IntentFacts);
        }
        if !self.allocator_matches(allocator) {
            return self.quarantine(Failure::AllocatorChanged);
        }
        self.kind = kind;
        self.intent_seen = true;
        if !self.quarantined {
            self.failure = Failure::None;
        }
        true
    }
    /// A quarantined but exact source can still be detached to preserve its readers; a successful transfer never
    /// clears the quarantine or authorizes its destruction.
    pub fn record_header_transfer(&mut self, proof: &HeaderTransferProof) -> bool {
        if !self.intent_seen {
            return self.quarantine(Failure::NoIntent);
        }
        if self.transfer_attempted {
            return self.quarantine(Failure::TransferRepeated);
        }
        self.transfer_attempted = true;
        if !proof.successful_exact_transfer
            || proof.source_before != self.identity
            || proof.saved_copy != self.identity.saved_native
            || proof.saved_copy != self.destruction_header
            || proof.source_after != self.identity.empty_allocator
        {
            return self.quarantine(Failure::TransferFailed);
        }
        self.detached = true;
        if !self.quarantined {
            self.failure = Failure::None;
        }
        true
    }
    pub fn observe_allocator(&mut self, proof: &AllocatorProof) -> bool {
        self.allocator_matches(proof) || self.quarantine(Failure::AllocatorChanged)
    }
    pub fn ready_to_destroy(&self, proof: &AllocatorProof) -> bool {
        self.intent_seen
            && self.detached
            && self.pending_readers == 0
            && !self.quarantined
            && !self.shutdown
            && !self.destroy_attempted
            && self.destruction_header == self.identity.saved_native
            && self.allocator_matches(proof)
    }
    /// The stable 16-byte storage original986E74 is called on. The attempt is consumed BEFORE the call, made outside
    /// the short CVR gate but within that typed native scope; keep its certificate valid through the call. A closing
    /// scope stops later attempts but never interrupts one already begun; an exception or an unsupported return is
    /// never retried. The pointer owns nothing else; continuing the native baseline intent is always the caller's.
    pub fn begin_destroy(&mut self, proof: &AllocatorProof) -> Option<*mut NativeHeader> {
        if self.destroy_attempted {
            self.quarantine(Failure::DestroyRepeated);
            return None;
        }
        if !self.allocator_matches(proof) {
            self.quarantine(Failure::AllocatorChanged);
            return None;
        }
        if !self.ready_to_destroy(proof) {
            self.reject(Failure::Incomplete);
            return None;
        }
        self.destroy_attempted = true;
        self.failure = Failure::None;
        Some(&mut self.destruction_header as *mut NativeHeader)
    }
    pub fn finish_destroy(&mut self, original_returned_normally: bool) -> bool {
        if !self.destroy_attempted {
            return self.quarantine(Failure::DestroyNotBegun);
        }
        if self.destroy_finished {
            return self.quarantine(Failure::DestroyRepeated);
        }
        self.destroy_finished = true;
        if !original_returned_normally {
            return self.quarantine(Failure::DestroyFailed);
        }
        self.destroyed = true;
        if !self.quarantined {
            self.failure = Failure::None;
        }
        !self.quarantined
    }
    pub fn cancel_selection(&mut self) {
        self.selected = false;
        self.acquisitions_closed = true;
    }
    pub fn shutdown(&mut self) {
        self.shutdown = true;
        self.acquisitions_closed = true;
        self.selected = false;
        self.quarantine(Failure::Shutdown);
    }
    pub fn can_retire_metadata(&self) -> bool {
        self.destroyed
            && self.detached
            && self.intent_seen
            && self.pending_readers == 0
            && !self.quarantined
            && !self.shutdown
    }
    pub fn can_abandon_unused_reservation(&self) -> bool {
        self.pending_readers == 0
            && self.completed_readers == 0
            && self.admitted_readers == self.canceled_readers
            && !self.intent_attempted
            && !self.transfer_attempted
            && !self.detached
            && !self.destroy_attempted
            && !self.quarantined
            && !self.shutdown
    }

    fn allocation_still_retained(&self) -> bool {
        self.identity.saved_native.capacity != 0 && (!self.destroyed || self.quarantined)
    }
    fn allocator_matches(&self, proof: &AllocatorProof) -> bool {
        proof.alive && proof.infrastructure_matches && proof.identity == self.allocator
    }
    fn find_reader(&self, lease_id: u64) -> Option<usize> {
        self.readers
            .iter()
            .position(|r| r.active && r.identity.lease_id == lease_id)
    }
    fn reject(&mut self, reason: Failure) -> bool {
        if !self.quarantined {
            self.failure = reason;
        }
        false
    }
    fn quarantine(&mut self, reason: Failure) -> bool {
        if !self.quarantined {
            self.failure = reason;
        }
        self.quarantined = true;
        self.acquisitions_closed = true;
        self.selected = false;
        false
    }
}

/// A reserved state's address in its ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateId {
    slot: usize,
    serial: u64,
}
impl StateId {
    /// The ledger slot (below MAX_STATES) the state occupies while it is reserved.
    pub fn slot(self) -> usize {
        self.slot
    }
}

#[derive(Clone, Copy, Default)]
struct Watermark {
    identity: HeaderIdentity,
    allocator: AllocatorIdentity,
    last_reader_lease: u64,
    unused_abandoned: bool,
}

pub struct Ledger {
    owners: Box<[Watermark; MAX_OWNERS]>,
    states: Vec<Option<(u64, Box<State>)>>,
    serial: u64,
    state_count: usize,
    owner_count: usize,
    shutdown: bool,
    failure: Failure,
}
impl Default for Ledger {
    fn default() -> Self {
        Ledger {
            owners: Box::new([Watermark::default(); MAX_OWNERS]),
            states: (0..MAX_STATES).map(|_| None).collect(),
            serial: 0,
            state_count: 0,
            owner_count: 0,
            shutdown: false,
            failure: Failure::None,
        }
    }
}
impl Ledger {
    /// A State and all its fixed reader slots reserved before any extra reader is admitted.
    pub fn reserve(
        &mut self,
        identity: &HeaderIdentity,
        facts: ReservationFacts,
        allocator: &ReservationAllocatorObservation,
    ) -> Option<StateId> {
        self.reserve_with(identity, facts, allocator, || true)
    }
    /// `reserve`, with `allocate` called where the C++ allocated its State: after every check, before anything is
    /// committed. Its refusal is an Allocation failure that commits nothing (the C++ face allocates its State there).
    pub fn reserve_with(
        &mut self,
        identity: &HeaderIdentity,
        facts: ReservationFacts,
        allocator: &ReservationAllocatorObservation,
        allocate: impl FnOnce() -> bool,
    ) -> Option<StateId> {
        if self.shutdown {
            self.failure = Failure::Shutdown;
            return None;
        }
        self.failure = Failure::Arguments;
        if !header_valid(identity)
            || !pointer(allocator.identity.address, 8)
            || allocator.identity.generation == 0
            || allocator.identity.address != identity.empty_allocator.data
        {
            return None;
        }
        self.failure = Failure::ReservationFacts;
        if !facts.retirement_path_audited
            || !facts.layout_audited
            || !facts.destructor_tree_safe
            || !allocator.infrastructure_matches
        {
            return None;
        }
        let watermark = self
            .owners
            .iter()
            .rposition(|w| w.identity.owner_header == identity.owner_header);
        let empty_owner = self
            .owners
            .iter()
            .position(|w| w.identity.owner_header == 0);
        self.failure = Failure::GenerationReplay;
        let unused_retry = watermark.is_some_and(|i| {
            let w = &self.owners[i];
            w.unused_abandoned && *identity == w.identity && allocator.identity == w.allocator
        });
        if let Some(i) = watermark {
            let w = &self.owners[i];
            if !unused_retry
                && (identity.generation <= w.identity.generation
                    || identity.born_epoch <= w.identity.born_epoch)
            {
                return None;
            }
        }
        if self
            .states
            .iter()
            .flatten()
            .any(|(_, old)| old.identity.owner_header == identity.owner_header && !old.detached)
        {
            self.failure = Failure::UnresolvedSource;
            return None;
        }
        let new_bytes = allocation_bytes(&identity.saved_native);
        for (_, old) in self.states.iter_mut().flatten() {
            let saved = old.identity.saved_native;
            let old_bytes = allocation_bytes(&saved);
            let old_allocation_conflict = old.allocation_still_retained()
                && ((new_bytes != 0
                    && overlap(saved.data, old_bytes, identity.saved_native.data, new_bytes))
                    || overlap(saved.data, old_bytes, identity.owner_header, 16)
                    || overlap(saved.data, old_bytes, identity.empty_allocator.data, 8));
            let new_allocation_conflict = new_bytes != 0
                && ((!old.detached
                    && overlap(
                        identity.saved_native.data,
                        new_bytes,
                        old.identity.owner_header,
                        16,
                    ))
                    || ((!old.destroyed || old.quarantined)
                        && overlap(
                            identity.saved_native.data,
                            new_bytes,
                            old.identity.empty_allocator.data,
                            8,
                        )));
            if old_allocation_conflict || new_allocation_conflict {
                old.quarantine(Failure::StorageOverlap);
                self.failure = Failure::StorageOverlap;
                return None;
            }
        }
        self.failure = Failure::Capacity;
        if watermark.is_none() && empty_owner.is_none() {
            return None;
        }
        let slot = self.states.iter().position(Option::is_none)?;
        if !allocate() {
            self.failure = Failure::Allocation;
            return None;
        }
        let previous_lease = match (unused_retry, watermark) {
            (true, Some(i)) => self.owners[i].last_reader_lease,
            _ => 0,
        };
        self.serial += 1;
        self.states[slot] = Some((
            self.serial,
            Box::new(State::new(*identity, allocator.identity, previous_lease)),
        ));
        let owner = match watermark {
            Some(i) => i,
            None => {
                self.owner_count += 1;
                empty_owner.expect("an empty owner")
            }
        };
        self.owners[owner] = Watermark {
            identity: *identity,
            allocator: allocator.identity,
            last_reader_lease: previous_lease,
            unused_abandoned: false,
        };
        self.state_count += 1;
        self.failure = Failure::None;
        Some(StateId {
            slot,
            serial: self.serial,
        })
    }
    pub fn get(&self, id: StateId) -> Option<&State> {
        match self.states.get(id.slot)? {
            Some((serial, state)) if *serial == id.serial => Some(state),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, id: StateId) -> Option<&mut State> {
        match self.states.get_mut(id.slot)? {
            Some((serial, state)) if *serial == id.serial => Some(state),
            _ => None,
        }
    }
    fn position(&self, owner_header: u64, generation: u64, born_epoch: u64) -> Option<usize> {
        self.states.iter().position(|s| {
            s.as_ref().is_some_and(|(_, state)| {
                state.identity.owner_header == owner_header
                    && state.identity.generation == generation
                    && state.identity.born_epoch == born_epoch
            })
        })
    }
    pub fn find(&self, owner_header: u64, generation: u64, born_epoch: u64) -> Option<StateId> {
        let slot = self.position(owner_header, generation, born_epoch)?;
        self.states[slot].as_ref().map(|(serial, _)| StateId {
            slot,
            serial: *serial,
        })
    }
    pub fn retire_metadata(&mut self, owner_header: u64, generation: u64, born_epoch: u64) -> bool {
        let Some(slot) = self.position(owner_header, generation, born_epoch) else {
            self.failure = Failure::Arguments;
            return false;
        };
        if !self.states[slot]
            .as_ref()
            .is_some_and(|(_, s)| s.can_retire_metadata())
        {
            self.failure = Failure::Incomplete;
            return false;
        }
        self.states[slot] = None;
        self.state_count -= 1;
        self.failure = Failure::None;
        true
    }
    /// Metadata only: no native intent/transfer occurred and every admitted reader was explicitly canceled before
    /// escape. Its exact retry identity and nonce are preserved; a completed escaped reader is not an unused attempt.
    pub fn abandon_unused_reservation(
        &mut self,
        owner_header: u64,
        generation: u64,
        born_epoch: u64,
    ) -> bool {
        let Some(slot) = self.position(owner_header, generation, born_epoch) else {
            self.failure = Failure::Arguments;
            return false;
        };
        let Some((_, state)) = self.states[slot].as_ref() else {
            return false;
        };
        if !state.can_abandon_unused_reservation() {
            self.failure = Failure::Incomplete;
            return false;
        }
        let (identity, last) = (state.identity, state.last_reader_lease);
        if let Some(w) = self.owners.iter_mut().find(|w| w.identity == identity) {
            w.unused_abandoned = true;
            w.last_reader_lease = last;
        }
        self.states[slot] = None;
        self.state_count -= 1;
        self.failure = Failure::None;
        true
    }
    pub fn cancel_selection(&mut self) {
        for (_, state) in self.states.iter_mut().flatten() {
            state.cancel_selection();
        }
    }
    pub fn shutdown(&mut self) {
        self.shutdown = true;
        for (_, state) in self.states.iter_mut().flatten() {
            state.shutdown();
        }
    }
    pub fn size(&self) -> usize {
        self.state_count
    }
    pub fn owner_count(&self) -> usize {
        self.owner_count
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/retirement_deferral_test.cpp's cases (its injected allocation failures have no Rust
    //! counterpart: reservation and admission do not fail on allocation here).
    use super::*;

    const RESERVATION: ReservationFacts = ReservationFacts {
        retirement_path_audited: true,
        layout_audited: true,
        destructor_tree_safe: true,
    };
    const INTENT: NativeIntentFacts = NativeIntentFacts {
        exact_native_retirement_point: true,
        layout_audited: true,
        unique_writer: true,
        destructor_tree_safe: true,
    };
    const ALLOCATOR: AllocatorProof = AllocatorProof {
        identity: AllocatorIdentity {
            address: 0x10000,
            generation: 1,
        },
        infrastructure_matches: true,
        alive: true,
    };
    const OBSERVED: ReservationAllocatorObservation = ReservationAllocatorObservation {
        identity: ALLOCATOR.identity,
        infrastructure_matches: true,
    };

    fn header_at(generation: u64, owner: u64) -> HeaderIdentity {
        HeaderIdentity {
            owner_header: owner,
            generation,
            born_epoch: generation,
            saved_native: NativeHeader {
                data: 0x100000 + owner * 0x100 + generation * 0x1000,
                capacity: 8,
                count: 2,
            },
            empty_allocator: NativeHeader {
                data: ALLOCATOR.identity.address,
                capacity: 0,
                count: 0,
            },
        }
    }
    fn header(generation: u64) -> HeaderIdentity {
        header_at(generation, 0x20000)
    }
    fn reader(lease: u64, consumer: u64) -> ExtraReaderIdentity {
        ExtraReaderIdentity {
            lease_id: lease,
            consumer_id: consumer,
            epoch: 11,
            pair_generation: 12,
            source_graph: 0x40000,
            execution_graph: 0x50000 + consumer * 0x100,
        }
    }
    fn one() -> ExtraReaderIdentity {
        reader(1, 1)
    }
    fn completed(reader: ExtraReaderIdentity) -> OwnedGraphCompletionProof {
        OwnedGraphCompletionProof {
            reader,
            owned_identity_verified: true,
            producer_closed: true,
            original_producer_returned: true,
            declaration_queued: true,
            original_declaration_returned: true,
            declaration_complete: true,
            execution_queued: true,
            original_execution_returned: true,
            execution_aggregate_complete: true,
        }
    }
    fn unescaped(reader: ExtraReaderIdentity) -> UnescapedCancellationProof {
        UnescapedCancellationProof {
            reader,
            native_original_never_invoked: true,
            verified_original_slot_restored: true,
            no_native_escape: true,
        }
    }
    fn exact_transfer(h: &HeaderIdentity) -> HeaderTransferProof {
        HeaderTransferProof {
            source_before: *h,
            saved_copy: h.saved_native,
            source_after: h.empty_allocator,
            successful_exact_transfer: true,
        }
    }
    fn reserve_with(ledger: &mut Ledger, h: &HeaderIdentity) -> StateId {
        ledger
            .reserve(h, RESERVATION, &OBSERVED)
            .expect("a valid pre-reservation rejected")
    }
    fn reserve(ledger: &mut Ledger) -> StateId {
        reserve_with(ledger, &header(1))
    }
    fn at(ledger: &mut Ledger, id: StateId) -> &mut State {
        ledger.get_mut(id).expect("a reserved state")
    }
    fn transfer(state: &mut State, kind: IntentKind) {
        let h = *state.frozen_header();
        assert!(
            state.observe_native_intent(kind, &h, INTENT, &ALLOCATOR),
            "the exact native intent rejected"
        );
        assert!(
            state.record_header_transfer(&exact_transfer(&h)),
            "the exact header transfer rejected"
        );
    }
    fn destroy_and_retire(ledger: &mut Ledger, id: StateId) {
        let state = at(ledger, id);
        let h = *state.frozen_header();
        assert!(state.ready_to_destroy(&ALLOCATOR));
        let saved = state
            .begin_destroy(&ALLOCATOR)
            .expect("the destruction attempt");
        // SAFETY: the state's stable saved header (the model of original986E74 touching only it).
        unsafe {
            assert!(*saved == h.saved_native);
            *saved = h.empty_allocator;
        }
        assert!(state.destroy_attempt_consumed());
        assert!(state.finish_destroy(true) && state.can_retire_metadata());
        assert!(
            *state.frozen_header() == h
                && ledger.retire_metadata(h.owner_header, h.generation, h.born_epoch)
        );
    }

    #[test]
    fn lifetimes() {
        let mut ledger = Ledger::default();
        for generation in 1..=12544u64 {
            let h = header(generation);
            let id = reserve_with(&mut ledger, &h);
            let (first, second) = (reader(1, generation * 2), reader(2, generation * 2 + 1));
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&first) && state.admit_extra_reader(&second));
            transfer(
                state,
                if generation & 1 != 0 {
                    IntentKind::Reset
                } else {
                    IntentKind::Destroy
                },
            );
            assert!(
                !state.acquisitions_open()
                    && state.pending_readers() == 2
                    && !state.ready_to_destroy(&ALLOCATOR)
            );
            assert!(
                !state.admit_extra_reader(&reader(3, generation * 2 + 2))
                    && state.rejection() == Failure::Closed
            );
            assert!(!state.quarantined());
            assert!(
                state.complete_extra_reader(&completed(second))
                    && !state.ready_to_destroy(&ALLOCATOR)
            );
            assert!(state.complete_extra_reader(&completed(first)) && state.pending_readers() == 0);
            assert_eq!(state.completed_readers(), 2);
            destroy_and_retire(&mut ledger, id);
            assert!(
                ledger.size() == 0 && ledger.owner_count() == 1,
                "successful cycles grew the completed history"
            );
            assert!(
                ledger.reserve(&h, RESERVATION, &OBSERVED).is_none()
                    && ledger.rejection() == Failure::GenerationReplay
            );
            assert!(ledger.get(id).is_none());
        }
    }
    #[test]
    fn reservation_failures_and_bounds() {
        let mut ledger = Ledger::default();
        for missing in 0..4 {
            let (mut facts, mut allocator) = (RESERVATION, OBSERVED);
            match missing {
                0 => facts.retirement_path_audited = false,
                1 => facts.layout_audited = false,
                2 => facts.destructor_tree_safe = false,
                _ => allocator.infrastructure_matches = false,
            }
            assert!(ledger.reserve(&header(1), facts, &allocator).is_none() && ledger.size() == 0);
            assert_eq!(ledger.owner_count(), 0);
        }
        for invalid in 0..9 {
            let mut h = header(2);
            match invalid {
                0 => h.owner_header = 0,
                1 => h.owner_header += 1,
                2 => h.generation = 0,
                3 => h.saved_native.count = 9,
                4 => h.empty_allocator.count = 1,
                5 => h.saved_native.data = u64::MAX - 7,
                6 => h.saved_native.data = h.owner_header,
                7 => h.saved_native.data = h.empty_allocator.data - 8,
                _ => h.owner_header = h.empty_allocator.data,
            }
            assert!(
                ledger.reserve(&h, RESERVATION, &OBSERVED).is_none() && ledger.size() == 0,
                "case {invalid}"
            );
        }
        let mut owners = Ledger::default();
        for i in 0..MAX_OWNERS as u64 {
            let id = reserve_with(&mut owners, &header_at(1, 0x20000 + i * 0x100));
            transfer(at(&mut owners, id), IntentKind::Reset);
            destroy_and_retire(&mut owners, id);
        }
        assert!(owners.size() == 0 && owners.owner_count() == MAX_OWNERS);
        assert!(owners
            .reserve(&header_at(1, 0x80000), RESERVATION, &OBSERVED)
            .is_none());
        assert_eq!(owners.rejection(), Failure::Capacity);
        let mut states = Ledger::default();
        for i in 0..MAX_STATES as u64 {
            let id = reserve_with(&mut states, &header(i + 1));
            transfer(at(&mut states, id), IntentKind::Reset);
        }
        assert!(states.size() == MAX_STATES);
        assert!(states
            .reserve(&header(MAX_STATES as u64 + 1), RESERVATION, &OBSERVED)
            .is_none());
        assert_eq!(states.rejection(), Failure::Capacity);
        let mut readers = Ledger::default();
        let id = reserve(&mut readers);
        let state = at(&mut readers, id);
        for i in 0..MAX_EXTRA_READERS as u64 {
            assert!(state.admit_extra_reader(&reader(i + 1, i + 1)));
        }
        let over = MAX_EXTRA_READERS as u64 + 1;
        assert!(
            !state.admit_extra_reader(&reader(over, over))
                && state.pending_readers() == MAX_EXTRA_READERS
        );
        let mut retry = Ledger::default();
        let retained = reserve(&mut retry);
        assert!(at(&mut retry, retained).admit_extra_reader(&one()));
        transfer(at(&mut retry, retained), IntentKind::Reset);
        let next = reserve_with(&mut retry, &header(2));
        assert!(
            at(&mut retry, next).acquisitions_open()
                && at(&mut retry, retained).pending_readers() == 1
        );
    }
    #[test]
    fn saved_allocation_aliasing() {
        for alias in 0..5 {
            let mut ledger = Ledger::default();
            let retained = reserve(&mut ledger);
            let state = at(&mut ledger, retained);
            assert!(state.admit_extra_reader(&one()));
            transfer(state, IntentKind::Reset);
            if alias >= 3 {
                assert!(
                    state.complete_extra_reader(&completed(one()))
                        && state.begin_destroy(&ALLOCATOR).is_some()
                );
                if alias == 4 {
                    assert!(!state.finish_destroy(false));
                }
            }
            let mut h = header_at(2, if alias == 1 { 0x21000 } else { 0x20000 });
            h.saved_native.data =
                state.frozen_header().saved_native.data + if alias == 2 { 8 } else { 0 };
            assert!(
                ledger.reserve(&h, RESERVATION, &OBSERVED).is_none()
                    && ledger.rejection() == Failure::StorageOverlap
            );
            assert!(ledger.size() == 1);
            let state = at(&mut ledger, retained);
            assert!(
                state.quarantined() && state.begin_destroy(&ALLOCATOR).is_none(),
                "alias {alias}"
            );
        }
        {
            let mut ledger = Ledger::default();
            let old = reserve(&mut ledger);
            let state = at(&mut ledger, old);
            transfer(state, IntentKind::Reset);
            assert!(state.begin_destroy(&ALLOCATOR).is_some() && state.finish_destroy(true));
            let mut h = header(2);
            h.saved_native.data = state.frozen_header().saved_native.data;
            let next = reserve_with(&mut ledger, &h);
            assert!(
                at(&mut ledger, next).acquisitions_open(),
                "a normally destroyed allocation could not be reused"
            );
        }
        for empty_old in [false, true] {
            let mut ledger = Ledger::default();
            let mut h = header(1);
            if empty_old {
                h.saved_native = h.empty_allocator;
            }
            let retained = reserve_with(&mut ledger, &h);
            assert!(at(&mut ledger, retained).admit_extra_reader(&one()));
            let mut incoming = header_at(1, 0x21000);
            incoming.saved_native.data = h.owner_header;
            assert!(ledger.reserve(&incoming, RESERVATION, &OBSERVED).is_none());
            assert_eq!(ledger.rejection(), Failure::StorageOverlap);
            let state = at(&mut ledger, retained);
            assert!(state.quarantined() && state.pending_readers() == 1);
        }
        for empty_old in [false, true] {
            let mut ledger = Ledger::default();
            let mut h = header(1);
            if empty_old {
                h.saved_native = h.empty_allocator;
            }
            let retained = reserve_with(&mut ledger, &h);
            let mut incoming = header_at(1, 0x21000);
            let allocator = AllocatorIdentity {
                address: 0x90000,
                generation: 2,
            };
            incoming.empty_allocator.data = allocator.address;
            incoming.saved_native.data = h.empty_allocator.data;
            let observed = ReservationAllocatorObservation {
                identity: allocator,
                infrastructure_matches: true,
            };
            assert!(ledger.reserve(&incoming, RESERVATION, &observed).is_none());
            assert!(
                ledger.rejection() == Failure::StorageOverlap
                    && at(&mut ledger, retained).quarantined()
            );
        }
    }
    #[test]
    fn completion_and_closure() {
        for missing in 0..9 {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            transfer(state, IntentKind::Reset);
            let mut proof = completed(one());
            match missing {
                0 => proof.owned_identity_verified = false,
                1 => proof.producer_closed = false,
                2 => proof.original_producer_returned = false,
                3 => proof.declaration_queued = false,
                4 => proof.original_declaration_returned = false,
                5 => proof.declaration_complete = false,
                6 => proof.execution_queued = false,
                7 => proof.original_execution_returned = false,
                _ => proof.execution_aggregate_complete = false,
            }
            assert!(!state.complete_extra_reader(&proof) && state.pending_readers() == 1);
            assert!(
                !state.ready_to_destroy(&ALLOCATOR) && state.begin_destroy(&ALLOCATOR).is_none()
            );
            assert!(!state.destroy_attempt_consumed());
            assert!(state.complete_extra_reader(&completed(one())));
            destroy_and_retire(&mut ledger, id);
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            transfer(state, IntentKind::Reset);
            let mut proof = completed(one());
            proof.reader.execution_graph += 1;
            assert!(
                !state.complete_extra_reader(&proof)
                    && state.quarantined()
                    && state.pending_readers() == 1
            );
            assert!(
                !state.ready_to_destroy(&ALLOCATOR),
                "a foreign graph released the source definitions"
            );
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            state.unsupported_extra_completion(1);
            assert!(state.quarantined() && state.pending_readers() == 1);
            transfer(state, IntentKind::Reset);
            assert!(
                state.exact_header_detached()
                    && state.quarantined()
                    && state.complete_extra_reader(&completed(one()))
            );
            assert!(
                !state.ready_to_destroy(&ALLOCATOR) && state.begin_destroy(&ALLOCATOR).is_none()
            );
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(
                state.admit_extra_reader(&one()) && state.complete_extra_reader(&completed(one()))
            );
            assert!(
                !state.ready_to_destroy(&ALLOCATOR) && state.begin_destroy(&ALLOCATOR).is_none()
            );
            assert!(
                !state.destroy_attempt_consumed(),
                "a reader completion synthesized a native retirement intent"
            );
            transfer(state, IntentKind::Reset);
            destroy_and_retire(&mut ledger, id);
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            transfer(at(&mut ledger, id), IntentKind::Reset);
            destroy_and_retire(&mut ledger, id);
        }
    }
    #[test]
    fn intents_and_transfer_failures() {
        for bad in 0..5 {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            let (mut facts, mut h, mut kind) = (INTENT, *state.frozen_header(), IntentKind::Reset);
            match bad {
                0 => facts.exact_native_retirement_point = false,
                1 => facts.unique_writer = false,
                2 => facts.destructor_tree_safe = false,
                3 => h.saved_native.count += 1,
                _ => kind = IntentKind::None,
            }
            assert!(
                !state.observe_native_intent(kind, &h, facts, &ALLOCATOR) && state.quarantined()
            );
            assert!(!state.acquisitions_open() && !state.ready_to_destroy(&ALLOCATOR));
            assert!(!state.admit_extra_reader(&reader(2, 2)));
        }
        for bad in 0..4 {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            let h = *state.frozen_header();
            assert!(state.observe_native_intent(IntentKind::Destroy, &h, INTENT, &ALLOCATOR));
            let mut proof = exact_transfer(&h);
            match bad {
                0 => proof.successful_exact_transfer = false,
                1 => proof.saved_copy.count += 1,
                2 => proof.source_after.capacity += 1,
                _ => proof.source_before.born_epoch += 1,
            }
            assert!(
                !state.record_header_transfer(&proof)
                    && state.quarantined()
                    && !state.exact_header_detached()
            );
            assert_eq!(state.pending_readers(), 1);
            assert!(
                !state.record_header_transfer(&exact_transfer(&h))
                    && state.begin_destroy(&ALLOCATOR).is_none()
            );
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            transfer(state, IntentKind::Reset);
            let h = *state.frozen_header();
            assert!(!state.observe_native_intent(IntentKind::Destroy, &h, INTENT, &ALLOCATOR));
            assert!(state.native_intent_kind() == IntentKind::Reset && state.quarantined());
        }
        {
            let mut ledger = Ledger::default();
            let first = reserve(&mut ledger);
            let nested = reserve_with(&mut ledger, &header_at(1, 0x21000));
            transfer(at(&mut ledger, first), IntentKind::Reset);
            transfer(at(&mut ledger, nested), IntentKind::Destroy);
            destroy_and_retire(&mut ledger, nested);
            destroy_and_retire(&mut ledger, first);
            assert_eq!(ledger.size(), 0, "independent nested intents interfered");
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            transfer(state, IntentKind::Reset);
            let h = *state.frozen_header();
            assert!(
                !state.record_header_transfer(&exact_transfer(&h)) && state.quarantined(),
                "a header transferred twice"
            );
        }
    }
    #[test]
    fn allocator_and_shutdown() {
        let no_native_use = AllocatorProof {
            alive: false,
            ..ALLOCATOR
        };
        {
            let mut metadata = Ledger::default();
            let id = reserve(&mut metadata);
            let state = at(&mut metadata, id);
            assert!(!state.native_intent_seen() && !state.ready_to_destroy(&no_native_use));
            assert!(!state.observe_native_intent(
                IntentKind::Reset,
                &header(1),
                INTENT,
                &no_native_use
            ));
            assert!(state.quarantined());
        }
        {
            let mut metadata = Ledger::default();
            let id = reserve(&mut metadata);
            let state = at(&mut metadata, id);
            transfer(state, IntentKind::Reset);
            assert!(
                state.begin_destroy(&no_native_use).is_none() && !state.destroy_attempt_consumed()
            );
            assert!(state.quarantined());
        }
        for changed in 0..4 {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            transfer(state, IntentKind::Reset);
            let mut allocator = ALLOCATOR;
            match changed {
                0 => allocator.identity.address += 0x100,
                1 => allocator.identity.generation += 1,
                2 => allocator.alive = false,
                _ => allocator.infrastructure_matches = false,
            }
            assert!(
                !state.ready_to_destroy(&allocator)
                    && !state.observe_allocator(&allocator)
                    && state.quarantined()
            );
            assert!(
                state.complete_extra_reader(&completed(one()))
                    && !state.ready_to_destroy(&ALLOCATOR)
            );
            assert!(
                state.begin_destroy(&ALLOCATOR).is_none(),
                "an allocator resurrection cleared the quarantine"
            );
        }
        let mut ledger = Ledger::default();
        let id = reserve(&mut ledger);
        assert!(at(&mut ledger, id).admit_extra_reader(&one()));
        transfer(at(&mut ledger, id), IntentKind::Reset);
        ledger.shutdown();
        let state = at(&mut ledger, id);
        assert!(state.quarantined() && state.pending_readers() == 1);
        assert!(ledger.size() == 1 && ledger.reserve(&header(2), RESERVATION, &OBSERVED).is_none());
        let state = at(&mut ledger, id);
        assert!(
            state.complete_extra_reader(&completed(one()))
                && state.begin_destroy(&ALLOCATOR).is_none()
        );
        assert!(
            !ledger.retire_metadata(0x20000, 1, 1),
            "a shutdown released the saved native header"
        );
    }
    #[test]
    fn exactly_once_cancellation_and_aba() {
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            transfer(state, IntentKind::Reset);
            assert!(state.begin_destroy(&ALLOCATOR).is_some());
            assert!(
                state.begin_destroy(&ALLOCATOR).is_none()
                    && state.destroy_attempt_consumed()
                    && state.quarantined()
            );
            assert!(!state.finish_destroy(true) && !ledger.retire_metadata(0x20000, 1, 1));
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            transfer(state, IntentKind::Reset);
            assert!(state.begin_destroy(&ALLOCATOR).is_some());
            assert!(
                !state.finish_destroy(false)
                    && state.begin_destroy(&ALLOCATOR).is_none()
                    && state.quarantined()
            );
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            transfer(at(&mut ledger, id), IntentKind::Reset);
            assert!(at(&mut ledger, id).begin_destroy(&ALLOCATOR).is_some());
            ledger.shutdown();
            let state = at(&mut ledger, id);
            assert!(
                !state.finish_destroy(true)
                    && state.destroy_returned()
                    && state.destroy_attempt_consumed()
            );
            assert!(state.begin_destroy(&ALLOCATOR).is_none() && !state.can_retire_metadata());
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(
                !state.finish_destroy(true) && state.quarantined(),
                "a return without a native attempt accepted"
            );
        }
        {
            let mut ledger = Ledger::default();
            let first = reserve(&mut ledger);
            assert!(at(&mut ledger, first).admit_extra_reader(&one()));
            ledger.cancel_selection();
            let state = at(&mut ledger, first);
            assert!(
                !state.selected() && !state.acquisitions_open() && state.pending_readers() == 1
            );
            assert!(!state.can_retire_metadata());
            transfer(state, IntentKind::Reset);
            let next = reserve_with(&mut ledger, &header(2));
            assert!(
                at(&mut ledger, next).acquisitions_open()
                    && at(&mut ledger, first).pending_readers() == 1
            );
            assert_eq!(ledger.size(), 2);
            assert!(at(&mut ledger, first).complete_extra_reader(&completed(one())));
            destroy_and_retire(&mut ledger, first);
            transfer(at(&mut ledger, next), IntentKind::Reset);
            destroy_and_retire(&mut ledger, next);
            assert!(ledger.reserve(&header(1), RESERVATION, &OBSERVED).is_none());
            assert!(ledger.reserve(&header(2), RESERVATION, &OBSERVED).is_none());
        }
        {
            let mut ledger = Ledger::default();
            let source = reserve(&mut ledger);
            assert!(at(&mut ledger, source).admit_extra_reader(&one()));
            assert!(ledger.reserve(&header(2), RESERVATION, &OBSERVED).is_none());
            assert!(
                ledger.rejection() == Failure::UnresolvedSource
                    && at(&mut ledger, source).pending_readers() == 1
            );
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(
                *state.frozen_header() == header(1)
                    && *state.saved_header() == header(1).saved_native
            );
            assert!(
                state.admit_extra_reader(&reader(1, 1)) && !state.admit_extra_reader(&reader(1, 2))
            );
            assert!(
                !state.admit_extra_reader(&reader(2, 1)),
                "a reader sequence/duplicate consumer admitted"
            );
        }
        {
            let mut ledger = Ledger::default();
            let mut h = header(1);
            h.generation = u64::MAX;
            h.born_epoch = u64::MAX;
            let id = reserve_with(&mut ledger, &h);
            transfer(at(&mut ledger, id), IntentKind::Reset);
            destroy_and_retire(&mut ledger, id);
            assert!(ledger.reserve(&h, RESERVATION, &OBSERVED).is_none());
            assert!(ledger.reserve(&header(1), RESERVATION, &OBSERVED).is_none());
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            let last = reader(u64::MAX, 1);
            assert!(
                state.admit_extra_reader(&last) && state.complete_extra_reader(&completed(last))
            );
            assert!(!state.admit_extra_reader(&last) && !state.admit_extra_reader(&one()));
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            for i in 1..=MAX_EXTRA_READERS as u64 * 2 {
                let r = reader(i, i);
                assert!(state.admit_extra_reader(&r) && state.complete_extra_reader(&completed(r)));
            }
            let n = MAX_EXTRA_READERS as u64 * 2 + 1;
            let current = reader(n, n);
            assert!(state.admit_extra_reader(&current));
            assert!(
                !state.complete_extra_reader(&completed(reader(1, 1)))
                    && state.pending_readers() == 1
            );
            let mut wrong = completed(current);
            wrong.reader.epoch += 1;
            assert!(
                !state.complete_extra_reader(&wrong)
                    && state.quarantined()
                    && state.pending_readers() == 1
            );
        }
    }
    #[test]
    fn unescaped_cancellation_and_abandonment() {
        let mut cycles = Ledger::default();
        for i in 1..=12544u64 {
            let id = reserve(&mut cycles);
            let r = reader(i, i);
            assert!(at(&mut cycles, id).admit_extra_reader(&r));
            cycles.cancel_selection();
            let state = at(&mut cycles, id);
            assert!(state.pending_readers() == 1 && !state.can_abandon_unused_reservation());
            assert!(
                state.cancel_unescaped_extra_reader(&unescaped(r)) && state.pending_readers() == 0
            );
            assert!(
                state.canceled_readers() == 1
                    && state.completed_readers() == 0
                    && !state.native_intent_seen()
            );
            assert!(!state.ready_to_destroy(&ALLOCATOR));
            assert!(
                cycles.abandon_unused_reservation(0x20000, 1, 1)
                    && cycles.size() == 0
                    && cycles.owner_count() == 1
            );
        }
        {
            let retried = reserve(&mut cycles);
            let state = at(&mut cycles, retried);
            assert!(
                !state.admit_extra_reader(&reader(1, 1))
                    && state.admit_extra_reader(&reader(12545, 12545))
            );
        }
        {
            let mut unused = Ledger::default();
            let id = reserve(&mut unused);
            assert!(at(&mut unused, id).can_abandon_unused_reservation());
            assert!(unused.abandon_unused_reservation(0x20000, 1, 1) && unused.size() == 0);
            let mut changed = header(1);
            changed.saved_native.count += 1;
            assert!(unused.reserve(&changed, RESERVATION, &OBSERVED).is_none());
            assert_eq!(unused.rejection(), Failure::GenerationReplay);
            let mut changed_allocator = OBSERVED;
            changed_allocator.identity.generation += 1;
            assert!(unused
                .reserve(&header(1), RESERVATION, &changed_allocator)
                .is_none());
            assert!(
                unused.reserve(&header(1), RESERVATION, &OBSERVED).is_some(),
                "the exact reader-free retry rejected"
            );
        }
        for denied in 0..7 {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let r = one();
            assert!(at(&mut ledger, id).admit_extra_reader(&r));
            let mut proof = unescaped(r);
            match denied {
                0 => assert!(at(&mut ledger, id).mark_extra_reader_escaped(&r)),
                1 => proof.native_original_never_invoked = false,
                2 => proof.verified_original_slot_restored = false,
                3 => proof.no_native_escape = false,
                4 => at(&mut ledger, id).unsupported_extra_completion(r.lease_id),
                5 => transfer(at(&mut ledger, id), IntentKind::Reset),
                _ => ledger.shutdown(),
            }
            let state = at(&mut ledger, id);
            assert!(!state.cancel_unescaped_extra_reader(&proof) && state.pending_readers() == 1);
            assert!(state.canceled_readers() == 0);
            if denied == 0 || denied == 1 || denied == 3 {
                assert!(
                    !state.cancel_unescaped_extra_reader(&unescaped(r))
                        && state.pending_readers() == 1
                );
            }
            assert!(
                !ledger.abandon_unused_reservation(0x20000, 1, 1),
                "denied {denied}"
            );
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(state.admit_extra_reader(&one()));
            let h = *state.frozen_header();
            assert!(state.observe_native_intent(IntentKind::Reset, &h, INTENT, &ALLOCATOR));
            assert!(!state.cancel_unescaped_extra_reader(&unescaped(one())));
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            assert!(
                state.admit_extra_reader(&one()) && state.complete_extra_reader(&completed(one()))
            );
            assert!(
                !ledger.abandon_unused_reservation(0x20000, 1, 1),
                "a completed escaped use became unused"
            );
        }
    }
    #[test]
    fn exact_pending_reader_query() {
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            let r = one();
            assert!(!state.has_pending_extra_reader(&r));
            assert!(state.admit_extra_reader(&r) && state.has_pending_extra_reader(&r));
            let (rejection, phase) = (state.rejection(), state.current_phase());
            for field in 0..6 {
                let mut other = r;
                match field {
                    0 => other.lease_id += 1,
                    1 => other.consumer_id += 1,
                    2 => other.epoch += 1,
                    3 => other.pair_generation += 1,
                    4 => other.source_graph += 1,
                    _ => other.execution_graph += 1,
                }
                assert!(
                    !state.has_pending_extra_reader(&other),
                    "a partial reader identity matched"
                );
            }
            assert!(
                state.rejection() == rejection
                    && state.current_phase() == phase
                    && state.pending_readers() == 1
            );
            assert!(state.mark_extra_reader_escaped(&r) && state.has_pending_extra_reader(&r));
            ledger.cancel_selection();
            let state = at(&mut ledger, id);
            assert!(state.has_pending_extra_reader(&r));
            let h = *state.frozen_header();
            assert!(
                state.observe_native_intent(IntentKind::Reset, &h, INTENT, &ALLOCATOR)
                    && state.has_pending_extra_reader(&r)
            );
            assert!(
                state.record_header_transfer(&exact_transfer(&h))
                    && state.has_pending_extra_reader(&r)
            );
            assert!(state.begin_destroy(&ALLOCATOR).is_none() && !state.destroy_attempt_consumed());
            assert!(state.has_pending_extra_reader(&r));
            assert!(
                state.complete_extra_reader(&completed(r)) && !state.has_pending_extra_reader(&r)
            );
            assert!(
                state.begin_destroy(&ALLOCATOR).is_some() && !state.has_pending_extra_reader(&r)
            );
            assert!(state.finish_destroy(true) && !state.has_pending_extra_reader(&r));
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            let state = at(&mut ledger, id);
            let old = one();
            assert!(
                state.admit_extra_reader(&old)
                    && state.cancel_unescaped_extra_reader(&unescaped(old))
            );
            assert!(!state.has_pending_extra_reader(&old));
            let next = reader(2, 2);
            assert!(state.admit_extra_reader(&next) && state.has_pending_extra_reader(&next));
            assert!(!state.has_pending_extra_reader(&old));
            state.unsupported_extra_completion(next.lease_id);
            assert!(state.pending_readers() == 1 && !state.has_pending_extra_reader(&next));
        }
        {
            let mut ledger = Ledger::default();
            let id = reserve(&mut ledger);
            assert!(at(&mut ledger, id).admit_extra_reader(&one()));
            ledger.shutdown();
            let state = at(&mut ledger, id);
            assert!(state.pending_readers() == 1 && !state.has_pending_extra_reader(&one()));
        }
    }
}
