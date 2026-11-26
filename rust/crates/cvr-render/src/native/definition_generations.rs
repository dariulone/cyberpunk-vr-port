//! The generations of a render graph's native definition owners and the CVR consumers bound to them (include/Render/
//! NativeDefinitionGenerations.hpp).
//!
//! A generation is born from an audited, untorn owner header observation (one active generation per owner; a later
//! birth while one is active makes it unsupported), closed at a later mutation epoch, its prior consumers verified, its
//! original owner detached, and retired only when every consumer bound to it reached an aggregate terminal proof and
//! every observer admitted before its closure resolved (a delayed root may discover another generation at actual
//! execution). Consumers register before their first possible native cache/source access (provisionally, then sealed
//! once the native root fixed its request/pair identity); unsupported use is sticky and never terminal completion.
//! Only monotonic IDs bind use: header addresses are observations. The caller holds its short synchronization gate
//! and supplies the audited evidence; nothing here reads or writes native memory or owns a pin or allocation.
use std::collections::BTreeMap;

pub const MAX_CONSUMERS: usize = 256;
pub const MAX_GENERATIONS: usize = 4096;
pub const MAX_OWNERS: usize = 4096;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Sequence,
    Mutation,
    Generation,
    Epoch,
    Duplicate,
    Consumer,
    Capacity,
    SourceClosed,
    TerminalProof,
    Unsupported,
    Detach,
    PriorCoverage,
    Incomplete,
    Retirement,
    Allocation,
    NoSourceProof,
    Unsealed,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Source,
    Execution,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HeaderObservation {
    pub owner_header: u64,
    pub born_mutation_epoch: u64,
    pub sequence_before: u64,
    pub sequence_after: u64,
    pub mutation_in_flight: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AggregateTerminalProof {
    pub consumer_id: u64,
    pub request_epoch: u64,
    pub pair_generation: u64,
    pub original_returned: bool,
    pub aggregate_complete: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Generation {
    pub id: u64,
    pub owner_header: u64,
    pub born_mutation_epoch: u64,
    pub closed_mutation_epoch: u64,
    pub closed_at_consumer: u64,
    pub bound_consumers: u64,
    pub completed_consumers: u64,
    pub unsupported_consumers: u64,
    pub pending_consumers: u64,
    pub all_prior_consumers_accounted: bool,
    pub source_closed: bool,
    pub detached: bool,
    pub unsupported: bool,
    pub retiring: bool,
    pub unsupported_reason: Failure,
}

#[derive(Clone, Copy, Default)]
struct Owner {
    last_born_epoch: u64,
    last_closed_epoch: u64,
    active_generation: u64,
}
#[derive(Clone, Default)]
struct Consumer {
    request_epoch: u64,
    pair_generation: u64,
    generations: Vec<u64>,
    failed: bool,
    provisional: bool,
}
impl Consumer {
    fn matches(&self, request_epoch: u64, pair_generation: u64) -> bool {
        self.request_epoch == request_epoch && self.pair_generation == pair_generation
    }
}
fn coverage_eligible(generation: &Generation) -> bool {
    !generation.unsupported
        && !generation.retiring
        && generation.source_closed
        && generation.all_prior_consumers_accounted
}
fn make_unsupported(generation: &mut Generation, reason: Failure) {
    generation.unsupported = true;
    if generation.unsupported_reason == Failure::None {
        generation.unsupported_reason = reason;
    }
}

#[derive(Default)]
pub struct Ledger {
    owners: BTreeMap<u64, Owner>,
    // Boxed: a generation keeps its address while it is tracked, as a std::map node does (GetGeneration's pointer).
    generations: BTreeMap<u64, Box<Generation>>,
    consumers: BTreeMap<u64, Consumer>,
    next_generation: u64,
    last_consumer: u64,
    completed_consumers: u64,
    unsupported_consumers: u64,
    cancelled_consumers: u64,
    retired_generations: u64,
    forgotten_observations: u64,
    failure: Failure,
}
impl Ledger {
    /// A new generation's id (0: rejected).
    pub fn birth_generation(
        &mut self,
        observation: &HeaderObservation,
        all_prior_consumers_accounted: bool,
    ) -> u64 {
        self.failure = Failure::None;
        if observation.owner_header == 0
            || observation.owner_header & 7 != 0
            || observation.born_mutation_epoch == 0
        {
            return self.reject_id(Failure::Arguments);
        }
        if observation.sequence_before == 0
            || observation.sequence_before != observation.sequence_after
        {
            return self.reject_id(Failure::Sequence);
        }
        if observation.mutation_in_flight {
            return self.reject_id(Failure::Mutation);
        }
        let owner = self.owners.get(&observation.owner_header).copied();
        if let Some(owner) = owner {
            if observation.born_mutation_epoch <= owner.last_born_epoch {
                return self.reject_id(Failure::Duplicate);
            }
            if owner.active_generation != 0 {
                if let Some(previous) = self.generations.get_mut(&owner.active_generation) {
                    make_unsupported(previous, Failure::Mutation);
                }
                return self.reject_id(Failure::Mutation);
            }
            if owner.last_closed_epoch != 0
                && observation.born_mutation_epoch < owner.last_closed_epoch
            {
                return self.reject_id(Failure::Epoch);
            }
        }
        if self.generations.len() >= MAX_GENERATIONS
            || (owner.is_none() && self.owners.len() >= MAX_OWNERS)
            || self.next_generation == u64::MAX
        {
            return self.reject_id(Failure::Capacity);
        }
        let id = self.next_generation + 1;
        self.generations.insert(
            id,
            Box::new(Generation {
                id,
                owner_header: observation.owner_header,
                born_mutation_epoch: observation.born_mutation_epoch,
                all_prior_consumers_accounted,
                ..Generation::default()
            }),
        );
        let entry = self.owners.entry(observation.owner_header).or_default();
        entry.last_born_epoch = observation.born_mutation_epoch;
        entry.active_generation = id;
        self.next_generation = id;
        id
    }
    pub fn find_generation(&self, owner_header: u64, born_mutation_epoch: u64) -> u64 {
        match self.owners.get(&owner_header) {
            Some(owner) if owner.last_born_epoch == born_mutation_epoch => owner.active_generation,
            _ => 0,
        }
    }
    pub fn get_generation(&self, generation_id: u64) -> Option<&Generation> {
        self.generations.get(&generation_id).map(|generation| &**generation)
    }
    /// Registered before the first possible native cache/source access, not merely at a later callback constructor
    /// (this cannot prove that frontier).
    pub fn register_consumer(
        &mut self,
        consumer_id: u64,
        request_epoch: u64,
        pair_generation: u64,
    ) -> bool {
        self.failure = Failure::None;
        if consumer_id == 0 || consumer_id <= self.last_consumer {
            return self.reject(Failure::Duplicate);
        }
        // The id is consumed even if the admission fails: it cannot identify a later root.
        self.last_consumer = consumer_id;
        if self.consumers.len() >= MAX_CONSUMERS {
            self.reject_open_generations(Failure::Capacity);
            return self.reject(Failure::Capacity);
        }
        self.consumers.insert(
            consumer_id,
            Consumer {
                request_epoch,
                pair_generation,
                ..Consumer::default()
            },
        );
        true
    }
    pub fn register_provisional_consumer(&mut self, consumer_id: u64) -> bool {
        if !self.register_consumer(consumer_id, 0, 0) {
            return false;
        }
        self.consumers
            .get_mut(&consumer_id)
            .expect("the registered consumer")
            .provisional = true;
        true
    }
    /// The native root can advance its request/pair identity before construction: its admission id stays fixed and
    /// the identity becomes immutable at this seal.
    pub fn seal_provisional_consumer(
        &mut self,
        consumer_id: u64,
        request_epoch: u64,
        pair_generation: u64,
    ) -> bool {
        self.failure = Failure::None;
        let Some(consumer) = self.consumers.get(&consumer_id) else {
            return self.reject(Failure::Consumer);
        };
        if consumer.failed {
            return self.reject_consumer(consumer_id, Failure::Unsupported);
        }
        if !consumer.provisional {
            if consumer.matches(request_epoch, pair_generation) {
                return self.reject(Failure::Duplicate);
            }
            return self.reject_consumer(consumer_id, Failure::Epoch);
        }
        if !consumer.generations.is_empty() {
            return self.reject_consumer(consumer_id, Failure::Consumer);
        }
        let consumer = self.consumers.get_mut(&consumer_id).expect("the consumer");
        consumer.request_epoch = request_epoch;
        consumer.pair_generation = pair_generation;
        consumer.provisional = false;
        true
    }
    pub fn has_consumer(&self, consumer_id: u64) -> bool {
        self.consumers.contains_key(&consumer_id)
    }
    pub fn consumer_bound(&self, consumer_id: u64, generation_id: u64) -> bool {
        self.consumers
            .get(&consumer_id)
            .is_some_and(|c| c.generations.contains(&generation_id))
    }
    pub fn bind_consumer(
        &mut self,
        consumer_id: u64,
        request_epoch: u64,
        pair_generation: u64,
        generation_id: u64,
        born_mutation_epoch: u64,
        admission: Admission,
    ) -> bool {
        let exists = self.consumers.contains_key(&consumer_id);
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            if exists {
                return self.reject_consumer(consumer_id, self.failure);
            }
            return false;
        }
        let Some(consumer) = self.consumers.get(&consumer_id) else {
            return self.reject_generation(generation_id, Failure::Consumer);
        };
        let generation = *self.generations[&generation_id];
        if consumer.provisional {
            return self.reject_binding(generation_id, consumer_id, Failure::Unsealed);
        }
        if !consumer.matches(request_epoch, pair_generation) {
            make_unsupported(
                self.generations
                    .get_mut(&generation_id)
                    .expect("the generation"),
                Failure::Epoch,
            );
            return self.reject_consumer(consumer_id, Failure::Epoch);
        }
        if consumer.failed {
            return self.reject_generation(generation_id, Failure::Unsupported);
        }
        if generation.retiring {
            return self.reject_binding(generation_id, consumer_id, Failure::Retirement);
        }
        if generation.source_closed
            && (admission != Admission::Execution || consumer_id > generation.closed_at_consumer)
        {
            return self.reject_binding(generation_id, consumer_id, Failure::SourceClosed);
        }
        if consumer.generations.contains(&generation_id) {
            return self.reject(Failure::Duplicate);
        }
        if consumer.generations.len() >= MAX_GENERATIONS
            || generation.bound_consumers == u64::MAX
            || generation.pending_consumers == u64::MAX
        {
            return self.reject_binding(generation_id, consumer_id, Failure::Capacity);
        }
        self.consumers
            .get_mut(&consumer_id)
            .expect("the consumer")
            .generations
            .push(generation_id);
        let generation = self
            .generations
            .get_mut(&generation_id)
            .expect("the generation");
        generation.bound_consumers += 1;
        generation.pending_consumers += 1;
        true
    }
    pub fn complete_consumer(&mut self, proof: &AggregateTerminalProof) -> bool {
        self.failure = Failure::None;
        let Some(consumer) = self.consumers.get(&proof.consumer_id) else {
            return self.reject(Failure::Consumer);
        };
        if consumer.failed {
            return self.reject_consumer(proof.consumer_id, Failure::Unsupported);
        }
        if consumer.provisional {
            return self.reject_consumer(proof.consumer_id, Failure::Unsealed);
        }
        if !consumer.matches(proof.request_epoch, proof.pair_generation) {
            return self.reject_consumer(proof.consumer_id, Failure::Epoch);
        }
        if !proof.original_returned || !proof.aggregate_complete {
            return self.reject_consumer(proof.consumer_id, Failure::TerminalProof);
        }
        let complete = consumer.generations.iter().all(|id| {
            self.generations
                .get(id)
                .is_some_and(|g| g.pending_consumers != 0 && g.completed_consumers != u64::MAX)
        });
        if !complete {
            return self.reject_consumer(proof.consumer_id, Failure::Incomplete);
        }
        let consumer = self
            .consumers
            .remove(&proof.consumer_id)
            .expect("the consumer");
        for id in consumer.generations {
            let generation = self.generations.get_mut(&id).expect("the bound generation");
            generation.pending_consumers -= 1;
            generation.completed_consumers += 1;
        }
        self.completed_consumers += 1;
        true
    }
    /// Unresolved use stays pending on each generation after its observer row leaves the bounded live table;
    /// unsupported is never terminal completion.
    pub fn unsupported_consumer(
        &mut self,
        consumer_id: u64,
        request_epoch: u64,
        pair_generation: u64,
        reason: Failure,
    ) -> bool {
        self.failure = Failure::None;
        let Some(consumer) = self.consumers.get(&consumer_id) else {
            return self.reject(Failure::Consumer);
        };
        if !consumer.matches(request_epoch, pair_generation) {
            return self.reject_consumer(consumer_id, Failure::Epoch);
        }
        let reason = if reason == Failure::None {
            Failure::Unsupported
        } else {
            reason
        };
        let consumer = self.consumers.remove(&consumer_id).expect("the consumer");
        for id in &consumer.generations {
            if let Some(generation) = self.generations.get_mut(id) {
                make_unsupported(generation, reason);
                generation.unsupported_consumers += 1;
            }
        }
        for generation in self.generations.values_mut() {
            if !generation.source_closed || consumer_id <= generation.closed_at_consumer {
                make_unsupported(generation, reason);
            }
        }
        self.unsupported_consumers += 1;
        true
    }
    /// A guarded native early exit can prove no cache/source access occurred; a cancellation without that proof is
    /// neither terminal nor prior-use accounting.
    pub fn cancel_unbound_consumer(
        &mut self,
        consumer_id: u64,
        request_epoch: u64,
        pair_generation: u64,
        native_no_source_proved: bool,
    ) -> bool {
        self.failure = Failure::None;
        let Some(consumer) = self.consumers.get(&consumer_id) else {
            return self.reject(Failure::Consumer);
        };
        if consumer.failed {
            return self.reject_consumer(consumer_id, Failure::Unsupported);
        }
        if !consumer.matches(request_epoch, pair_generation) {
            return self.reject_consumer(consumer_id, Failure::Epoch);
        }
        if !consumer.generations.is_empty() {
            return self.reject_consumer(consumer_id, Failure::Consumer);
        }
        if !native_no_source_proved {
            return self.reject_consumer(consumer_id, Failure::NoSourceProof);
        }
        self.consumers.remove(&consumer_id);
        self.cancelled_consumers += 1;
        true
    }
    pub fn unsupported_generation(
        &mut self,
        generation_id: u64,
        born_mutation_epoch: u64,
        reason: Failure,
    ) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        let reason = if reason == Failure::None {
            Failure::Unsupported
        } else {
            reason
        };
        make_unsupported(
            self.generations
                .get_mut(&generation_id)
                .expect("the generation"),
            reason,
        );
        true
    }
    pub fn close_source(
        &mut self,
        generation_id: u64,
        born_mutation_epoch: u64,
        closing_mutation_epoch: u64,
    ) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        let last_consumer = self.last_consumer;
        let generation = self
            .generations
            .get_mut(&generation_id)
            .expect("the generation");
        if generation.source_closed {
            return self.reject(Failure::Duplicate);
        }
        if generation.retiring || closing_mutation_epoch <= born_mutation_epoch {
            return self.reject_generation(generation_id, Failure::Mutation);
        }
        generation.source_closed = true;
        generation.closed_mutation_epoch = closing_mutation_epoch;
        generation.closed_at_consumer = last_consumer;
        let owner_header = generation.owner_header;
        match self.owners.get_mut(&owner_header) {
            Some(owner) if owner.active_generation == generation_id => {
                owner.active_generation = 0;
                owner.last_closed_epoch = closing_mutation_epoch;
                true
            }
            _ => self.reject_generation(generation_id, Failure::Generation),
        }
    }
    pub fn verify_prior_consumers(
        &mut self,
        generation_id: u64,
        born_mutation_epoch: u64,
        all_prior_consumers_accounted: bool,
    ) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        let generation = self
            .generations
            .get_mut(&generation_id)
            .expect("the generation");
        if generation.retiring || !all_prior_consumers_accounted {
            return self.reject_generation(generation_id, Failure::PriorCoverage);
        }
        if generation.all_prior_consumers_accounted {
            return self.reject(Failure::Duplicate);
        }
        generation.all_prior_consumers_accounted = true;
        true
    }
    pub fn verify_detach(
        &mut self,
        generation_id: u64,
        born_mutation_epoch: u64,
        original_owner_detached: bool,
    ) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        let generation = self
            .generations
            .get_mut(&generation_id)
            .expect("the generation");
        if generation.detached {
            return self.reject(Failure::Duplicate);
        }
        if generation.retiring || !original_owner_detached {
            return self.reject_generation(generation_id, Failure::Detach);
        }
        generation.detached = true;
        true
    }
    /// Moving a source-closed owner protects still-pending users; destruction has the additional terminal gates of
    /// `ready_to_retire` (a mutex is not exclusivity).
    pub fn can_adopt(&self, generation_id: u64, native_exclusive_proved: bool) -> bool {
        native_exclusive_proved
            && self
                .get_generation(generation_id)
                .is_some_and(|g| !g.detached && coverage_eligible(g))
    }
    pub fn ownership_eligible(&self, generation_id: u64) -> bool {
        self.get_generation(generation_id)
            .is_some_and(|g| g.detached && coverage_eligible(g))
    }
    pub fn ready_to_retire(&self, generation_id: u64) -> bool {
        let Some(generation) = self.get_generation(generation_id) else {
            return false;
        };
        if !self.ownership_eligible(generation_id) || generation.pending_consumers != 0 {
            return false;
        }
        // A delayed root can discover another generation at actual execution: every pre-closure observer must
        // therefore reach a terminal resolution.
        self.consumers
            .keys()
            .next()
            .is_none_or(|&first| first > generation.closed_at_consumer)
    }
    pub fn begin_retire(&mut self, generation_id: u64, born_mutation_epoch: u64) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        if !self.ready_to_retire(generation_id) {
            return self.reject(Failure::Retirement);
        }
        self.generations
            .get_mut(&generation_id)
            .expect("the generation")
            .retiring = true;
        true
    }
    pub fn finish_retire(&mut self, generation_id: u64, born_mutation_epoch: u64) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        let generation = &self.generations[&generation_id];
        if !generation.retiring || generation.unsupported {
            return self.reject(Failure::Retirement);
        }
        self.generations.remove(&generation_id);
        self.retired_generations += 1;
        true
    }
    /// Only the observer metadata discarded: a detached ownership and unresolved users must survive; this neither
    /// retires nor destroys any native definitions.
    pub fn forget_closed_observation(
        &mut self,
        generation_id: u64,
        born_mutation_epoch: u64,
    ) -> bool {
        self.failure = Failure::None;
        if !self.check_generation(generation_id, born_mutation_epoch) {
            return false;
        }
        let generation = &self.generations[&generation_id];
        if !generation.source_closed
            || generation.detached
            || generation.retiring
            || generation.pending_consumers != 0
            || self
                .consumers
                .keys()
                .next()
                .is_some_and(|&first| first <= generation.closed_at_consumer)
        {
            return self.reject(Failure::Incomplete);
        }
        self.generations.remove(&generation_id);
        self.forgotten_observations += 1;
        true
    }
    pub fn active_consumers(&self) -> usize {
        self.consumers.len()
    }
    pub fn active_generations(&self) -> usize {
        self.generations.len()
    }
    pub fn tracked_owners(&self) -> usize {
        self.owners.len()
    }
    pub fn last_generation_id(&self) -> u64 {
        self.next_generation
    }
    pub fn last_consumer_id(&self) -> u64 {
        self.last_consumer
    }
    pub fn completed_consumers(&self) -> u64 {
        self.completed_consumers
    }
    pub fn unsupported_consumers(&self) -> u64 {
        self.unsupported_consumers
    }
    pub fn cancelled_consumers(&self) -> u64 {
        self.cancelled_consumers
    }
    pub fn retired_generations(&self) -> u64 {
        self.retired_generations
    }
    pub fn forgotten_observations(&self) -> u64 {
        self.forgotten_observations
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }

    fn reject(&mut self, reason: Failure) -> bool {
        self.failure = reason;
        false
    }
    fn reject_id(&mut self, reason: Failure) -> u64 {
        self.reject(reason);
        0
    }
    fn reject_generation(&mut self, generation_id: u64, reason: Failure) -> bool {
        if let Some(generation) = self.generations.get_mut(&generation_id) {
            make_unsupported(generation, reason);
        }
        self.reject(reason)
    }
    fn reject_consumer(&mut self, consumer_id: u64, reason: Failure) -> bool {
        if let Some(consumer) = self.consumers.get_mut(&consumer_id) {
            consumer.failed = true;
            for id in &consumer.generations {
                if let Some(generation) = self.generations.get_mut(id) {
                    make_unsupported(generation, reason);
                }
            }
        }
        self.reject(reason)
    }
    fn reject_binding(&mut self, generation_id: u64, consumer_id: u64, reason: Failure) -> bool {
        if let Some(generation) = self.generations.get_mut(&generation_id) {
            make_unsupported(generation, reason);
        }
        self.reject_consumer(consumer_id, reason)
    }
    fn reject_open_generations(&mut self, reason: Failure) {
        for generation in self.generations.values_mut() {
            if !generation.source_closed {
                make_unsupported(generation, reason);
            }
        }
    }
    fn check_generation(&mut self, generation_id: u64, born_mutation_epoch: u64) -> bool {
        let Some(generation) = self.generations.get(&generation_id) else {
            return self.reject(Failure::Generation);
        };
        if generation.born_mutation_epoch != born_mutation_epoch {
            return self.reject_generation(generation_id, Failure::Epoch);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/definition_generations_test.cpp's cases.
    use super::*;

    const HEADER: u64 = 0x10000;
    const EPOCH: u64 = 37;
    const REQUEST: u64 = 11;
    const PAIR: u64 = 4;

    fn observation(header: u64, epoch: u64) -> HeaderObservation {
        HeaderObservation {
            owner_header: header,
            born_mutation_epoch: epoch,
            sequence_before: epoch,
            sequence_after: epoch,
            mutation_in_flight: false,
        }
    }
    fn terminal(id: u64, request: u64, pair: u64) -> AggregateTerminalProof {
        AggregateTerminalProof {
            consumer_id: id,
            request_epoch: request,
            pair_generation: pair,
            original_returned: true,
            aggregate_complete: true,
        }
    }
    fn done(id: u64) -> AggregateTerminalProof {
        terminal(id, REQUEST, PAIR)
    }
    fn birth_at(ledger: &mut Ledger, header: u64, epoch: u64, prior: bool) -> u64 {
        let id = ledger.birth_generation(&observation(header, epoch), prior);
        assert_ne!(id, 0, "a birth rejected: {:?}", ledger.rejection());
        id
    }
    fn birth(ledger: &mut Ledger) -> u64 {
        birth_at(ledger, HEADER, EPOCH, false)
    }
    fn bind(ledger: &mut Ledger, consumer: u64, generation: u64, born: u64) -> bool {
        ledger.bind_consumer(consumer, REQUEST, PAIR, generation, born, Admission::Source)
    }
    fn generation(ledger: &Ledger, id: u64) -> Generation {
        *ledger.get_generation(id).expect("the generation")
    }

    #[test]
    fn source_and_retirement() {
        let mut ledger = Ledger::default();
        let g = birth(&mut ledger);
        assert!(g == 1 && ledger.find_generation(HEADER, EPOCH) == g);
        assert!(
            ledger.active_generations() == 1
                && ledger.tracked_owners() == 1
                && !ledger.can_adopt(g, true)
        );
        let state = generation(&ledger, g);
        assert!(
            !state.all_prior_consumers_accounted
                && !state.unsupported
                && !state.detached
                && !state.source_closed
        );
        assert!(!ledger.ownership_eligible(g) && !ledger.ready_to_retire(g));
        assert!(ledger.register_consumer(1, REQUEST, PAIR) && bind(&mut ledger, 1, g, EPOCH));
        assert!(ledger.consumer_bound(1, g) && generation(&ledger, g).pending_consumers == 1);
        assert!(ledger.close_source(g, EPOCH, EPOCH + 1));
        assert!(ledger.find_generation(HEADER, EPOCH) == 0 && !ledger.can_adopt(g, true));
        assert!(ledger.verify_prior_consumers(g, EPOCH, true));
        assert!(!ledger.can_adopt(g, false) && ledger.can_adopt(g, true));
        assert!(!ledger.ownership_eligible(g) && !ledger.ready_to_retire(g));
        assert!(ledger.verify_detach(g, EPOCH, true));
        assert!(
            !ledger.can_adopt(g, true)
                && ledger.ownership_eligible(g)
                && !ledger.ready_to_retire(g)
        );
        assert!(!ledger.begin_retire(g, EPOCH));
        assert!(
            ledger.complete_consumer(&done(1))
                && !ledger.has_consumer(1)
                && ledger.active_consumers() == 0
        );
        let state = generation(&ledger, g);
        assert!(state.bound_consumers == 1 && state.completed_consumers == 1);
        assert!(state.pending_consumers == 0 && !state.unsupported);
        assert!(ledger.ready_to_retire(g) && ledger.begin_retire(g, EPOCH));
        assert!(!ledger.begin_retire(g, EPOCH) && !ledger.ready_to_retire(g));
        assert!(generation(&ledger, g).retiring && ledger.finish_retire(g, EPOCH));
        assert!(ledger.get_generation(g).is_none() && ledger.active_generations() == 0);
        assert_eq!(ledger.retired_generations(), 1);
        assert!(!ledger.finish_retire(g, EPOCH) && ledger.rejection() == Failure::Generation);
        assert!(!ledger.complete_consumer(&done(1)) && ledger.completed_consumers() == 1);
    }
    #[test]
    fn delayed_execution_and_rearm() {
        let mut ledger = Ledger::default();
        let old = birth_at(&mut ledger, HEADER, EPOCH, true);
        assert!(ledger.register_consumer(1, REQUEST, PAIR) && ledger.register_consumer(2, 0, 0));
        assert!(bind(&mut ledger, 1, old, EPOCH));
        assert!(
            ledger.close_source(old, EPOCH, EPOCH + 1) && ledger.verify_detach(old, EPOCH, true)
        );
        assert!(ledger.ownership_eligible(old) && !ledger.ready_to_retire(old));
        let replacement = birth_at(&mut ledger, HEADER, EPOCH + 1, true);
        assert!(replacement > old && ledger.find_generation(HEADER, EPOCH + 1) == replacement);
        assert!(ledger.register_consumer(3, REQUEST + 1, PAIR + 1));
        assert!(ledger.bind_consumer(
            3,
            REQUEST + 1,
            PAIR + 1,
            replacement,
            EPOCH + 1,
            Admission::Source
        ));
        // A root admitted before the replacement may observe its new generation only at actual execution;
        // cancelling/rearming a selected capture changes no ids.
        assert!(ledger.bind_consumer(
            1,
            REQUEST,
            PAIR,
            replacement,
            EPOCH + 1,
            Admission::Execution
        ));
        assert!(ledger.consumer_bound(1, old) && ledger.consumer_bound(1, replacement));
        assert!(ledger.complete_consumer(&done(1)));
        assert!(!ledger.ready_to_retire(old));
        assert!(ledger.bind_consumer(2, 0, 0, old, EPOCH, Admission::Execution));
        assert!(!ledger.ready_to_retire(old) && generation(&ledger, old).pending_consumers == 1);
        assert!(ledger.complete_consumer(&terminal(2, 0, 0)) && ledger.ready_to_retire(old));
        assert_eq!(generation(&ledger, old).completed_consumers, 2);
        assert!(ledger.begin_retire(old, EPOCH) && ledger.finish_retire(old, EPOCH));
        assert!(ledger.has_consumer(3) && generation(&ledger, replacement).pending_consumers == 1);
        assert!(ledger.close_source(replacement, EPOCH + 1, EPOCH + 2));
        assert!(ledger.verify_detach(replacement, EPOCH + 1, true));
        assert!(!ledger.ready_to_retire(replacement));
        assert!(ledger.complete_consumer(&terminal(3, REQUEST + 1, PAIR + 1)));
        assert!(ledger.ready_to_retire(replacement));
    }
    #[test]
    fn bounded_long_process() {
        const COMPLETED: u64 = 12050;
        let mut ledger = Ledger::default();
        let g = birth(&mut ledger);
        for id in 1..=COMPLETED {
            assert!(ledger.register_consumer(id, REQUEST, PAIR) && bind(&mut ledger, id, g, EPOCH));
            assert!(ledger.complete_consumer(&done(id)));
            assert!(
                ledger.active_consumers() == 0
                    && ledger.active_generations() == 1
                    && ledger.tracked_owners() == 1
            );
        }
        let state = generation(&ledger, g);
        assert!(state.bound_consumers == COMPLETED && state.completed_consumers == COMPLETED);
        assert!(state.pending_consumers == 0);
        assert!(
            ledger.completed_consumers() == COMPLETED && ledger.last_consumer_id() == COMPLETED
        );
        assert!(ledger.close_source(g, EPOCH, EPOCH + 1) && ledger.verify_detach(g, EPOCH, true));
        assert!(
            !ledger.can_adopt(g, true)
                && !ledger.ownership_eligible(g)
                && !ledger.ready_to_retire(g)
        );
        assert!(ledger.verify_prior_consumers(g, EPOCH, true) && ledger.ready_to_retire(g));
        assert!(
            !ledger.register_consumer(COMPLETED, REQUEST, PAIR)
                && ledger.rejection() == Failure::Duplicate
        );
        let mut reused = Ledger::default();
        for i in 0..COMPLETED {
            let born = 100 + i * 2;
            let id = birth_at(&mut reused, HEADER, born, true);
            assert!(id == i + 1 && reused.close_source(id, born, born + 1));
            assert!(reused.can_adopt(id, true) && reused.verify_detach(id, born, true));
            assert!(
                reused.ready_to_retire(id)
                    && reused.begin_retire(id, born)
                    && reused.finish_retire(id, born)
            );
            assert!(reused.active_generations() == 0 && reused.tracked_owners() == 1);
            assert!(reused.birth_generation(&observation(HEADER, born), true) == 0);
            assert_eq!(reused.rejection(), Failure::Duplicate);
        }
        assert!(
            reused.last_generation_id() == COMPLETED && reused.retired_generations() == COMPLETED
        );
    }
    #[test]
    fn rejected_observations() {
        let mut ledger = Ledger::default();
        for (o, reason) in [
            (observation(0, EPOCH), Failure::Arguments),
            (observation(HEADER + 1, EPOCH), Failure::Arguments),
            (observation(HEADER, 0), Failure::Arguments),
            (
                HeaderObservation {
                    sequence_after: EPOCH + 1,
                    ..observation(HEADER, EPOCH)
                },
                Failure::Sequence,
            ),
            (
                HeaderObservation {
                    sequence_before: 0,
                    sequence_after: 0,
                    ..observation(HEADER, EPOCH)
                },
                Failure::Sequence,
            ),
            (
                HeaderObservation {
                    mutation_in_flight: true,
                    ..observation(HEADER, EPOCH)
                },
                Failure::Mutation,
            ),
        ] {
            assert!(ledger.birth_generation(&o, false) == 0 && ledger.rejection() == reason);
        }
        let g = birth(&mut ledger);
        assert!(ledger.birth_generation(&observation(HEADER, EPOCH), false) == 0);
        assert_eq!(ledger.rejection(), Failure::Duplicate);
        assert!(ledger.birth_generation(&observation(HEADER, EPOCH + 1), false) == 0);
        assert_eq!(ledger.rejection(), Failure::Mutation);
        assert!(
            generation(&ledger, g).unsupported
                && generation(&ledger, g).unsupported_reason == Failure::Mutation
        );
        assert!(ledger.close_source(g, EPOCH, EPOCH + 3));
        assert!(ledger.birth_generation(&observation(HEADER, EPOCH + 2), false) == 0);
        assert_eq!(ledger.rejection(), Failure::Epoch);
        let next = birth_at(&mut ledger, HEADER, EPOCH + 5, false);
        assert!(next > g && ledger.find_generation(HEADER, EPOCH + 5) == next);
        assert!(ledger.find_generation(HEADER, EPOCH) == 0 && generation(&ledger, g).source_closed);
        assert!(
            !ledger.close_source(g, EPOCH, EPOCH + 3) && ledger.rejection() == Failure::Duplicate
        );
    }
    #[test]
    fn failed_consumers_remain_unresolved() {
        let mut ledger = Ledger::default();
        let a = birth_at(&mut ledger, HEADER, EPOCH, true);
        let b = birth_at(&mut ledger, HEADER + 0x100, EPOCH + 1, true);
        assert!(ledger.register_consumer(1, REQUEST, PAIR));
        assert!(bind(&mut ledger, 1, a, EPOCH) && bind(&mut ledger, 1, b, EPOCH + 1));
        assert!(!bind(&mut ledger, 1, a, EPOCH) && ledger.rejection() == Failure::Duplicate);
        assert!(
            generation(&ledger, a).pending_consumers == 1 && !generation(&ledger, a).unsupported
        );
        let proof = AggregateTerminalProof {
            original_returned: false,
            ..done(1)
        };
        assert!(!ledger.complete_consumer(&proof) && ledger.rejection() == Failure::TerminalProof);
        assert!(generation(&ledger, a).unsupported && generation(&ledger, b).unsupported);
        assert!(!ledger.complete_consumer(&done(1)) && ledger.completed_consumers() == 0);
        assert!(
            ledger.unsupported_consumer(1, REQUEST, PAIR, Failure::Unsupported)
                && ledger.active_consumers() == 0
        );
        assert!(
            generation(&ledger, a).pending_consumers == 1
                && generation(&ledger, a).unsupported_consumers == 1
        );
        assert!(
            generation(&ledger, b).pending_consumers == 1
                && generation(&ledger, b).completed_consumers == 0
        );
        assert!(ledger.close_source(a, EPOCH, EPOCH + 2) && ledger.verify_detach(a, EPOCH, true));
        assert!(
            !ledger.can_adopt(a, true)
                && !ledger.ownership_eligible(a)
                && !ledger.ready_to_retire(a)
        );
        let mut pending = Ledger::default();
        let unresolved = birth_at(&mut pending, HEADER, EPOCH, true);
        assert!(
            pending.register_consumer(1, REQUEST, PAIR) && bind(&mut pending, 1, unresolved, EPOCH)
        );
        let proof = AggregateTerminalProof {
            aggregate_complete: false,
            ..done(1)
        };
        assert!(!pending.complete_consumer(&proof) && generation(&pending, unresolved).unsupported);
        assert!(generation(&pending, unresolved).completed_consumers == 0);
        assert_eq!(generation(&pending, unresolved).pending_consumers, 1);
        let mut unknown = Ledger::default();
        let uncovered = birth_at(&mut unknown, HEADER, EPOCH, true);
        assert!(!bind(&mut unknown, 99, uncovered, EPOCH));
        assert!(
            unknown.rejection() == Failure::Consumer && generation(&unknown, uncovered).unsupported
        );
        let mut unbound = Ledger::default();
        let delayed = birth_at(&mut unbound, HEADER, EPOCH, true);
        assert!(unbound.register_consumer(1, REQUEST, PAIR));
        assert!(
            unbound.close_source(delayed, EPOCH, EPOCH + 1)
                && unbound.verify_detach(delayed, EPOCH, true)
        );
        assert!(!unbound.ready_to_retire(delayed));
        assert!(unbound.unsupported_consumer(1, REQUEST, PAIR, Failure::Unsupported));
        assert!(unbound.active_consumers() == 0 && generation(&unbound, delayed).unsupported);
        assert!(!unbound.ready_to_retire(delayed));
    }
    #[test]
    fn wrong_epoch_and_late_admission() {
        let mut wrong = Ledger::default();
        let a = birth_at(&mut wrong, HEADER, EPOCH, true);
        assert!(wrong.register_consumer(1, REQUEST, PAIR));
        assert!(!wrong.bind_consumer(1, REQUEST + 1, PAIR, a, EPOCH, Admission::Source));
        assert_eq!(wrong.rejection(), Failure::Epoch);
        assert!(generation(&wrong, a).unsupported && generation(&wrong, a).bound_consumers == 0);
        assert!(!wrong.complete_consumer(&done(1)) && wrong.completed_consumers() == 0);
        let mut wrong_proof = Ledger::default();
        let b = birth_at(&mut wrong_proof, HEADER, EPOCH, true);
        assert!(
            wrong_proof.register_consumer(1, REQUEST, PAIR) && bind(&mut wrong_proof, 1, b, EPOCH)
        );
        assert!(!wrong_proof.complete_consumer(&terminal(1, REQUEST, PAIR + 1)));
        assert_eq!(wrong_proof.rejection(), Failure::Epoch);
        assert!(
            generation(&wrong_proof, b).pending_consumers == 1
                && generation(&wrong_proof, b).unsupported
        );
        let mut wrong_birth = Ledger::default();
        let c = birth_at(&mut wrong_birth, HEADER, EPOCH, true);
        assert!(
            !wrong_birth.close_source(c, EPOCH + 1, EPOCH + 2)
                && wrong_birth.rejection() == Failure::Epoch
        );
        assert!(
            generation(&wrong_birth, c).unsupported && !generation(&wrong_birth, c).source_closed
        );
        let mut late = Ledger::default();
        let d = birth_at(&mut late, HEADER, EPOCH, true);
        assert!(late.register_consumer(1, REQUEST, PAIR) && late.close_source(d, EPOCH, EPOCH + 1));
        assert!(!bind(&mut late, 1, d, EPOCH) && late.rejection() == Failure::SourceClosed);
        assert!(generation(&late, d).unsupported);
        assert!(!late.complete_consumer(&done(1)) && late.completed_consumers() == 0);
        let mut too_late = Ledger::default();
        let e = birth_at(&mut too_late, HEADER, EPOCH, true);
        assert!(
            too_late.close_source(e, EPOCH, EPOCH + 1)
                && too_late.register_consumer(1, REQUEST, PAIR)
        );
        assert!(!too_late.bind_consumer(1, REQUEST, PAIR, e, EPOCH, Admission::Execution));
        assert!(
            too_late.rejection() == Failure::SourceClosed && generation(&too_late, e).unsupported
        );
        assert!(!too_late.complete_consumer(&done(1)) && too_late.completed_consumers() == 0);
        let mut no_detach = Ledger::default();
        let f = birth_at(&mut no_detach, HEADER, EPOCH, true);
        assert!(no_detach.close_source(f, EPOCH, EPOCH + 1) && no_detach.can_adopt(f, true));
        assert!(
            !no_detach.verify_detach(f, EPOCH, false) && no_detach.rejection() == Failure::Detach
        );
        assert!(!no_detach.ownership_eligible(f) && !no_detach.ready_to_retire(f));
        let mut coverage = Ledger::default();
        let g = birth(&mut coverage);
        assert!(
            !coverage.verify_prior_consumers(g, EPOCH, false)
                && coverage.rejection() == Failure::PriorCoverage
        );
        assert!(generation(&coverage, g).unsupported && !coverage.can_adopt(g, true));
    }
    #[test]
    fn capacity_and_retirement_guards() {
        let mut consumers = Ledger::default();
        let g = birth_at(&mut consumers, HEADER, EPOCH, true);
        let max = MAX_CONSUMERS as u64;
        for id in 1..=max {
            assert!(consumers.register_consumer(id, REQUEST, PAIR));
        }
        assert_eq!(consumers.active_consumers(), MAX_CONSUMERS);
        assert!(
            !consumers.register_consumer(max + 1, REQUEST, PAIR)
                && consumers.rejection() == Failure::Capacity
        );
        assert!(
            consumers.active_consumers() == MAX_CONSUMERS
                && consumers.last_consumer_id() == max + 1
        );
        assert!(generation(&consumers, g).unsupported);
        for id in 1..=max {
            assert!(consumers.complete_consumer(&done(id)));
        }
        assert!(
            consumers.active_consumers() == 0
                && consumers.register_consumer(max + 2, REQUEST, PAIR)
        );
        assert!(
            !consumers.register_consumer(max + 1, REQUEST, PAIR)
                && consumers.rejection() == Failure::Duplicate
        );
        let mut generations = Ledger::default();
        for i in 0..MAX_GENERATIONS as u64 {
            birth_at(&mut generations, HEADER + i * 0x100, EPOCH + i, true);
        }
        assert!(
            generations.active_generations() == MAX_GENERATIONS
                && generations.tracked_owners() == MAX_OWNERS
        );
        let owners = MAX_OWNERS as u64;
        assert!(
            generations
                .birth_generation(&observation(HEADER + owners * 0x100, EPOCH + owners), false)
                == 0
        );
        assert_eq!(generations.rejection(), Failure::Capacity);
        let oldest = 1;
        assert!(generations.close_source(oldest, EPOCH, EPOCH + owners + 1));
        assert!(generations.verify_detach(oldest, EPOCH, true));
        assert!(
            generations.begin_retire(oldest, EPOCH) && generations.finish_retire(oldest, EPOCH)
        );
        assert!(
            generations.active_generations() == MAX_GENERATIONS - 1
                && generations.tracked_owners() == MAX_OWNERS
        );
        assert!(
            generations.birth_generation(
                &observation(HEADER + owners * 0x100, EPOCH + owners + 2),
                false
            ) == 0
        );
        assert_eq!(generations.rejection(), Failure::Capacity);
        let replacement = birth_at(&mut generations, HEADER, EPOCH + owners + 3, false);
        assert!(
            replacement == MAX_GENERATIONS as u64 + 1
                && generations.active_generations() == MAX_GENERATIONS
        );
        let mut many_same_owner = Ledger::default();
        for i in 0..MAX_GENERATIONS as u64 {
            let born = 100 + i;
            let id = birth_at(&mut many_same_owner, HEADER, born, false);
            assert!(many_same_owner.close_source(id, born, born + 1));
        }
        assert!(
            many_same_owner.active_generations() == MAX_GENERATIONS
                && many_same_owner.tracked_owners() == 1
        );
        assert!(
            many_same_owner
                .birth_generation(&observation(HEADER, 100 + MAX_GENERATIONS as u64), false)
                == 0
        );
        assert_eq!(many_same_owner.rejection(), Failure::Capacity);
        let mut retirement = Ledger::default();
        let guarded = birth_at(&mut retirement, HEADER, EPOCH, true);
        assert!(
            retirement.close_source(guarded, EPOCH, EPOCH + 1)
                && retirement.verify_detach(guarded, EPOCH, true)
        );
        assert!(
            !retirement.finish_retire(guarded, EPOCH)
                && retirement.rejection() == Failure::Retirement
        );
        assert!(retirement.begin_retire(guarded, EPOCH));
        assert!(retirement.unsupported_generation(guarded, EPOCH, Failure::Mutation));
        assert!(
            !retirement.finish_retire(guarded, EPOCH) && generation(&retirement, guarded).retiring
        );
        assert!(!retirement.begin_retire(guarded, EPOCH) && retirement.active_generations() == 1);
    }
    #[test]
    fn observation_forgetting() {
        let mut ledger = Ledger::default();
        let g = birth(&mut ledger);
        assert!(
            !ledger.forget_closed_observation(g, EPOCH)
                && ledger.rejection() == Failure::Incomplete
        );
        assert!(ledger.register_consumer(1, REQUEST, PAIR) && bind(&mut ledger, 1, g, EPOCH));
        assert!(ledger.close_source(g, EPOCH, EPOCH + 1));
        assert!(
            !ledger.forget_closed_observation(g, EPOCH)
                && generation(&ledger, g).pending_consumers == 1
        );
        assert!(ledger.complete_consumer(&done(1)));
        assert!(!ledger.ownership_eligible(g) && !ledger.ready_to_retire(g));
        assert!(ledger.forget_closed_observation(g, EPOCH) && ledger.forgotten_observations() == 1);
        assert!(
            ledger.active_generations() == 0
                && ledger.tracked_owners() == 1
                && ledger.retired_generations() == 0
        );
        assert!(
            !ledger.forget_closed_observation(g, EPOCH)
                && ledger.rejection() == Failure::Generation
        );
        assert_eq!(ledger.forgotten_observations(), 1);
        assert!(ledger.birth_generation(&observation(HEADER, EPOCH), false) == 0);
        assert_eq!(ledger.rejection(), Failure::Duplicate);
        let replacement = birth_at(&mut ledger, HEADER, EPOCH + 1, false);
        assert!(replacement > g && ledger.find_generation(HEADER, EPOCH + 1) == replacement);
        let mut delayed = Ledger::default();
        let unseen = birth(&mut delayed);
        assert!(
            delayed.register_consumer(1, REQUEST, PAIR)
                && delayed.close_source(unseen, EPOCH, EPOCH + 1)
        );
        assert!(
            generation(&delayed, unseen).pending_consumers == 0
                && !delayed.forget_closed_observation(unseen, EPOCH)
        );
        assert!(
            delayed.complete_consumer(&done(1)) && delayed.forget_closed_observation(unseen, EPOCH)
        );
        let mut detached = Ledger::default();
        let owned = birth_at(&mut detached, HEADER, EPOCH, true);
        assert!(
            detached.close_source(owned, EPOCH, EPOCH + 1)
                && detached.verify_detach(owned, EPOCH, true)
        );
        assert!(
            !detached.forget_closed_observation(owned, EPOCH) && detached.ready_to_retire(owned)
        );
        assert!(
            detached.begin_retire(owned, EPOCH)
                && !detached.forget_closed_observation(owned, EPOCH)
        );
        assert!(detached.finish_retire(owned, EPOCH) && detached.forgotten_observations() == 0);
        assert_eq!(detached.retired_generations(), 1);
        let mut unsupported = Ledger::default();
        let unresolved = birth(&mut unsupported);
        assert!(
            unsupported.register_consumer(1, REQUEST, PAIR)
                && bind(&mut unsupported, 1, unresolved, EPOCH)
        );
        assert!(unsupported.close_source(unresolved, EPOCH, EPOCH + 1));
        assert!(unsupported.unsupported_consumer(1, REQUEST, PAIR, Failure::Unsupported));
        assert!(
            unsupported.active_consumers() == 0 && generation(&unsupported, unresolved).unsupported
        );
        assert!(
            !unsupported.forget_closed_observation(unresolved, EPOCH)
                && unsupported.forgotten_observations() == 0
        );
        let mut unsupported_idle = Ledger::default();
        let idle = birth(&mut unsupported_idle);
        assert!(unsupported_idle.unsupported_generation(idle, EPOCH, Failure::Unsupported));
        assert!(unsupported_idle.close_source(idle, EPOCH, EPOCH + 1));
        assert!(unsupported_idle.forget_closed_observation(idle, EPOCH));
        assert_eq!(unsupported_idle.forgotten_observations(), 1);
        let mut wrong_epoch = Ledger::default();
        let exact = birth(&mut wrong_epoch);
        assert!(wrong_epoch.close_source(exact, EPOCH, EPOCH + 1));
        assert!(
            !wrong_epoch.forget_closed_observation(exact, EPOCH + 1)
                && wrong_epoch.rejection() == Failure::Epoch
        );
        assert!(
            wrong_epoch.get_generation(exact).is_some()
                && wrong_epoch.forgotten_observations() == 0
        );
        assert!(
            wrong_epoch.forget_closed_observation(exact, EPOCH)
                && wrong_epoch.forgotten_observations() == 1
        );
        const CYCLES: u64 = 12050;
        let mut long_observation = Ledger::default();
        for i in 0..CYCLES {
            let born = 100 + i * 2;
            let id = birth_at(&mut long_observation, HEADER, born, false);
            assert!(id == i + 1 && long_observation.close_source(id, born, born + 1));
            assert!(
                !long_observation.can_adopt(id, true) && !long_observation.ownership_eligible(id)
            );
            assert!(!long_observation.ready_to_retire(id));
            assert!(long_observation.forget_closed_observation(id, born));
            assert!(
                long_observation.active_generations() == 0
                    && long_observation.tracked_owners() == 1
            );
            assert!(long_observation.birth_generation(&observation(HEADER, born), false) == 0);
            assert_eq!(long_observation.rejection(), Failure::Duplicate);
        }
        assert!(
            long_observation.forgotten_observations() == CYCLES
                && long_observation.last_generation_id() == CYCLES
        );
        assert_eq!(long_observation.retired_generations(), 0);
    }
    #[test]
    fn proved_no_source_cancellation() {
        let mut ledger = Ledger::default();
        let g = birth_at(&mut ledger, HEADER, EPOCH, true);
        assert!(
            ledger.register_consumer(1, REQUEST, PAIR) && ledger.close_source(g, EPOCH, EPOCH + 1)
        );
        assert!(ledger.verify_detach(g, EPOCH, true) && !ledger.ready_to_retire(g));
        assert!(ledger.cancel_unbound_consumer(1, REQUEST, PAIR, true));
        assert!(
            !ledger.has_consumer(1)
                && ledger.cancelled_consumers() == 1
                && ledger.completed_consumers() == 0
        );
        assert!(!generation(&ledger, g).unsupported && ledger.ready_to_retire(g));
        assert!(
            !ledger.cancel_unbound_consumer(1, REQUEST, PAIR, true)
                && ledger.rejection() == Failure::Consumer
        );
        assert!(ledger.cancelled_consumers() == 1 && !ledger.register_consumer(1, REQUEST, PAIR));
        let mut observation_ledger = Ledger::default();
        let unseen = birth(&mut observation_ledger);
        assert!(observation_ledger.register_consumer(1, REQUEST, PAIR));
        assert!(observation_ledger.close_source(unseen, EPOCH, EPOCH + 1));
        assert!(!observation_ledger.forget_closed_observation(unseen, EPOCH));
        assert!(observation_ledger.cancel_unbound_consumer(1, REQUEST, PAIR, true));
        assert!(observation_ledger.forget_closed_observation(unseen, EPOCH));
        assert!(
            observation_ledger.cancelled_consumers() == 1
                && observation_ledger.forgotten_observations() == 1
        );
        assert_eq!(observation_ledger.completed_consumers(), 0);
        let mut false_proof = Ledger::default();
        assert!(false_proof.register_consumer(1, REQUEST, PAIR));
        assert!(!false_proof.cancel_unbound_consumer(1, REQUEST, PAIR, false));
        assert_eq!(false_proof.rejection(), Failure::NoSourceProof);
        assert!(false_proof.has_consumer(1) && false_proof.cancelled_consumers() == 0);
        assert!(!false_proof.cancel_unbound_consumer(1, REQUEST, PAIR, true));
        assert_eq!(false_proof.rejection(), Failure::Unsupported);
        assert!(!false_proof.complete_consumer(&done(1)) && false_proof.completed_consumers() == 0);
        assert!(false_proof.unsupported_consumer(1, REQUEST, PAIR, Failure::Unsupported));
        assert_eq!(false_proof.unsupported_consumers(), 1);
        let mut bound = Ledger::default();
        let owned = birth(&mut bound);
        assert!(bound.register_consumer(1, REQUEST, PAIR) && bind(&mut bound, 1, owned, EPOCH));
        assert!(
            !bound.cancel_unbound_consumer(1, REQUEST, PAIR, true)
                && bound.rejection() == Failure::Consumer
        );
        assert!(
            generation(&bound, owned).unsupported
                && generation(&bound, owned).pending_consumers == 1
        );
        assert!(
            bound.completed_consumers() == 0
                && bound.cancelled_consumers() == 0
                && bound.has_consumer(1)
        );
        assert!(!bound.complete_consumer(&done(1)));
        assert!(bound.unsupported_consumer(1, REQUEST, PAIR, Failure::Unsupported));
        assert!(
            generation(&bound, owned).pending_consumers == 1
                && generation(&bound, owned).completed_consumers == 0
        );
        let mut wrong_epoch = Ledger::default();
        assert!(wrong_epoch.register_consumer(1, REQUEST, PAIR));
        assert!(!wrong_epoch.cancel_unbound_consumer(1, REQUEST + 1, PAIR, true));
        assert_eq!(wrong_epoch.rejection(), Failure::Epoch);
        assert!(wrong_epoch.has_consumer(1) && wrong_epoch.cancelled_consumers() == 0);
        assert!(!wrong_epoch.cancel_unbound_consumer(1, REQUEST, PAIR, true));
        assert_eq!(wrong_epoch.rejection(), Failure::Unsupported);
        let mut sticky = Ledger::default();
        assert!(sticky.register_consumer(1, REQUEST, PAIR));
        assert!(!sticky.complete_consumer(&AggregateTerminalProof {
            original_returned: false,
            ..done(1)
        }));
        assert!(
            !sticky.cancel_unbound_consumer(1, REQUEST, PAIR, true)
                && sticky.rejection() == Failure::Unsupported
        );
        assert!(
            sticky.cancelled_consumers() == 0
                && sticky.completed_consumers() == 0
                && sticky.has_consumer(1)
        );
        assert!(
            !sticky.cancel_unbound_consumer(99, REQUEST, PAIR, true)
                && sticky.rejection() == Failure::Consumer
        );
    }
    #[test]
    fn provisional_admission_and_identity_seal() {
        let mut delayed = Ledger::default();
        let g = birth_at(&mut delayed, HEADER, EPOCH, true);
        assert!(delayed.register_provisional_consumer(1));
        assert!(delayed.last_consumer_id() == 1 && delayed.has_consumer(1));
        assert!(
            delayed.close_source(g, EPOCH, EPOCH + 1)
                && generation(&delayed, g).closed_at_consumer == 1
        );
        assert!(delayed.verify_detach(g, EPOCH, true) && !delayed.ready_to_retire(g));
        // The pair identity advances inside the native root, after admission but before its constructor: sealing
        // preserves the original admission id/high water.
        assert!(delayed.seal_provisional_consumer(1, REQUEST, PAIR + 1));
        assert!(delayed.last_consumer_id() == 1);
        assert!(delayed.bind_consumer(1, REQUEST, PAIR + 1, g, EPOCH, Admission::Execution));
        assert!(
            !delayed.seal_provisional_consumer(1, REQUEST, PAIR + 1)
                && delayed.rejection() == Failure::Duplicate
        );
        assert!(!generation(&delayed, g).unsupported && !delayed.ready_to_retire(g));
        assert!(
            delayed.complete_consumer(&terminal(1, REQUEST, PAIR + 1))
                && delayed.ready_to_retire(g)
        );
        assert!(
            delayed.completed_consumers() == 1
                && !delayed.seal_provisional_consumer(1, REQUEST, PAIR + 1)
        );
        let mut replacement = Ledger::default();
        let old = birth(&mut replacement);
        assert!(
            replacement.register_provisional_consumer(1)
                && replacement.close_source(old, EPOCH, EPOCH + 1)
        );
        let current = birth_at(&mut replacement, HEADER, EPOCH + 1, false);
        assert!(replacement.seal_provisional_consumer(1, REQUEST, PAIR + 1));
        assert!(replacement.bind_consumer(
            1,
            REQUEST,
            PAIR + 1,
            current,
            EPOCH + 1,
            Admission::Source
        ));
        assert!(replacement.complete_consumer(&terminal(1, REQUEST, PAIR + 1)));
        assert!(replacement.forget_closed_observation(old, EPOCH));
        let mut unsafe_bind = Ledger::default();
        let unsealed = birth(&mut unsafe_bind);
        assert!(unsafe_bind.register_provisional_consumer(1));
        assert!(!unsafe_bind.bind_consumer(1, 0, 0, unsealed, EPOCH, Admission::Source));
        assert_eq!(unsafe_bind.rejection(), Failure::Unsealed);
        assert!(
            generation(&unsafe_bind, unsealed).unsupported
                && generation(&unsafe_bind, unsealed).bound_consumers == 0
        );
        assert!(!unsafe_bind.seal_provisional_consumer(1, REQUEST, PAIR + 1));
        assert_eq!(unsafe_bind.rejection(), Failure::Unsupported);
        assert!(
            !unsafe_bind.complete_consumer(&terminal(1, 0, 0))
                && unsafe_bind.completed_consumers() == 0
        );
        assert!(unsafe_bind.unsupported_consumer(1, 0, 0, Failure::Unsupported));
        let mut unsafe_complete = Ledger::default();
        assert!(unsafe_complete.register_provisional_consumer(1));
        assert!(!unsafe_complete.complete_consumer(&terminal(1, 0, 0)));
        assert_eq!(unsafe_complete.rejection(), Failure::Unsealed);
        assert!(!unsafe_complete.seal_provisional_consumer(1, REQUEST, PAIR));
        assert_eq!(unsafe_complete.rejection(), Failure::Unsupported);
        assert!(
            !unsafe_complete.cancel_unbound_consumer(1, 0, 0, true)
                && unsafe_complete.cancelled_consumers() == 0
        );
        assert!(unsafe_complete.has_consumer(1) && unsafe_complete.completed_consumers() == 0);
        let mut changed_identity = Ledger::default();
        let immutable = birth(&mut changed_identity);
        assert!(changed_identity.register_provisional_consumer(1));
        assert!(changed_identity.seal_provisional_consumer(1, REQUEST, PAIR + 1));
        assert!(changed_identity.bind_consumer(
            1,
            REQUEST,
            PAIR + 1,
            immutable,
            EPOCH,
            Admission::Source
        ));
        assert!(!changed_identity.seal_provisional_consumer(1, REQUEST + 1, PAIR + 2));
        assert_eq!(changed_identity.rejection(), Failure::Epoch);
        assert!(generation(&changed_identity, immutable).unsupported);
        assert_eq!(
            generation(&changed_identity, immutable).pending_consumers,
            1
        );
        assert!(!changed_identity.complete_consumer(&terminal(1, REQUEST, PAIR + 1)));
        assert_eq!(changed_identity.completed_consumers(), 0);
        let mut wrong_terminal = Ledger::default();
        let wrong = birth(&mut wrong_terminal);
        assert!(wrong_terminal.register_provisional_consumer(1));
        assert!(wrong_terminal.seal_provisional_consumer(1, REQUEST, PAIR + 1));
        assert!(wrong_terminal.bind_consumer(
            1,
            REQUEST,
            PAIR + 1,
            wrong,
            EPOCH,
            Admission::Source
        ));
        assert!(
            !wrong_terminal.complete_consumer(&done(1))
                && wrong_terminal.rejection() == Failure::Epoch
        );
        assert!(
            generation(&wrong_terminal, wrong).unsupported
                && wrong_terminal.completed_consumers() == 0
        );
        let mut no_source = Ledger::default();
        let idle = birth(&mut no_source);
        assert!(
            no_source.register_provisional_consumer(1)
                && no_source.close_source(idle, EPOCH, EPOCH + 1)
        );
        assert!(!no_source.forget_closed_observation(idle, EPOCH));
        assert!(
            no_source.cancel_unbound_consumer(1, 0, 0, true)
                && no_source.cancelled_consumers() == 1
        );
        assert!(
            no_source.completed_consumers() == 0
                && no_source.forget_closed_observation(idle, EPOCH)
        );
        let mut missing_proof = Ledger::default();
        assert!(missing_proof.register_provisional_consumer(1));
        assert!(!missing_proof.cancel_unbound_consumer(1, 0, 0, false));
        assert_eq!(missing_proof.rejection(), Failure::NoSourceProof);
        assert!(!missing_proof.seal_provisional_consumer(1, REQUEST, PAIR));
        assert_eq!(missing_proof.rejection(), Failure::Unsupported);
        let mut normal = Ledger::default();
        assert!(normal.register_consumer(1, REQUEST, PAIR) && normal.complete_consumer(&done(1)));
        assert!(normal.register_consumer(2, 0, 0) && normal.complete_consumer(&terminal(2, 0, 0)));
        assert!(
            normal.register_provisional_consumer(3) && normal.seal_provisional_consumer(3, 0, 0)
        );
        assert!(normal.complete_consumer(&terminal(3, 0, 0)));
        assert!(normal.register_consumer(4, REQUEST, PAIR));
        assert!(
            !normal.seal_provisional_consumer(4, REQUEST, PAIR)
                && normal.rejection() == Failure::Duplicate
        );
        assert!(normal.complete_consumer(&done(4)) && normal.completed_consumers() == 4);
        assert!(
            !normal.register_provisional_consumer(4) && normal.rejection() == Failure::Duplicate
        );
        let mut capacity = Ledger::default();
        let max = MAX_CONSUMERS as u64;
        for id in 1..=max {
            assert!(capacity.register_provisional_consumer(id));
        }
        assert!(
            !capacity.register_provisional_consumer(max + 1)
                && capacity.rejection() == Failure::Capacity
        );
        assert!(
            capacity.active_consumers() == MAX_CONSUMERS && capacity.last_consumer_id() == max + 1
        );
        for id in 1..=max {
            assert!(capacity.seal_provisional_consumer(id, REQUEST, PAIR + 1));
            assert!(capacity.complete_consumer(&terminal(id, REQUEST, PAIR + 1)));
        }
        assert!(
            capacity.active_consumers() == 0 && capacity.register_provisional_consumer(max + 2)
        );
        assert!(
            !capacity.register_provisional_consumer(max + 1)
                && capacity.rejection() == Failure::Duplicate
        );
        assert!(
            !capacity.seal_provisional_consumer(999, REQUEST, PAIR)
                && capacity.rejection() == Failure::Consumer
        );
    }
}
