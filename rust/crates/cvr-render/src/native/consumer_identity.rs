//! The native root's identity ticket (include/Render/NativeGraphConsumerIdentity.hpp).
//!
//! Captured immediately before the root's construct (return 0x40A4AB) from its stack: the frame owner, the graph and
//! the completion handle's group, read twice, then the owner (+8) and the group (+1C) retained and everything read
//! twice more. An execution override records the caller's future publication contract; this ticket neither publishes
//! the slot nor proves storage/definition leases. Verified once right after B56968 (the constructed payload carries the
//! identity and the source handle was consumed); from then on it matches the queued payload (queue return 0x3E90BA)
//! and the scheduler's execution callbacks without reading the expired root stack. Identity only: neither a zero
//! pending count nor cancellation proves completion (the jobs api's complete is deliberately unused). The native
//! callbacks must not unwind; the caller serializes this object.
#![allow(unsafe_op_in_unsafe_fn)]
use super::graph_jobs::{Read, QUEUE_RETURN};
use super::job_lease::JobLeaseApi;
use super::job_receipt::FrameOwnerApi;
use core::ffi::c_void;

pub const CONSTRUCT_RETURN: u64 = 0x40A4AB;
/// The scheduler's execution callback return and its aggregate variant (2.5).
pub const EXECUTION_CALLBACK_RETURN: u64 = 0x26E110;
pub const AGGREGATE_EXECUTION_CALLBACK_RETURN: u64 = 0x2735E3;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IdentityFailure {
    #[default]
    None,
    Arguments,
    Caller,
    AlreadyCaptured,
    MissingApi,
    Read,
    References,
    Changed,
    NotCaptured,
    ConstructAlreadyChecked,
    ConstructIdentity,
    SourceNotConsumed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Identity {
    owner: u64,
    graph: u64,
    group: u64,
}

fn pointer(value: u64, bytes: u64) -> bool {
    value != 0 && value & 7 == 0 && value <= u64::MAX - bytes
}
fn valid(value: &Identity) -> bool {
    pointer(value.owner, 12)
        && pointer(value.graph, 0x70)
        && pointer(value.group, 0x20)
        && value.owner != value.group
}
fn read64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn read_source(
    read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    owner: u64,
    graph: u64,
    group: u64,
) -> Option<Identity> {
    let value = Identity {
        owner: read64(read, owner)?,
        graph: read64(read, graph)?,
        group: read64(read, group)?,
    };
    valid(&value).then_some(value)
}
fn read_payload(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<Identity> {
    if !pointer(address, 0x20) {
        return None;
    }
    let value = Identity {
        owner: read64(read, address)?,
        graph: read64(read, address + 8)?,
        group: read64(read, address + 0x18)?,
    };
    valid(&value).then_some(value)
}
fn references(
    read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    address: u64,
    before_increment: bool,
) -> bool {
    let mut b = [0u8; 4];
    read(address, &mut b) && {
        let count = i32::from_le_bytes(b);
        count > 0 && (!before_increment || count < i32::MAX)
    }
}

/// The frame owner and group pins taken during a capture, released (group first) unless handed over.
struct Pins {
    jobs_release: unsafe extern "C" fn(*mut *mut c_void),
    owner_release: unsafe extern "C" fn(*mut c_void),
    frame: *mut c_void,
    group: *mut c_void,
}
impl Drop for Pins {
    fn drop(&mut self) {
        // SAFETY: the pins this capture retained.
        unsafe {
            if !self.group.is_null() {
                (self.jobs_release)(&mut self.group);
            }
            if !self.frame.is_null() {
                (self.owner_release)(self.frame);
            }
        }
    }
}

#[derive(Default)]
pub struct NativeRootTicket {
    identity: Identity,
    jobs_api: Option<JobLeaseApi>,
    owner_api: Option<FrameOwnerApi>,
    frame_pin: usize,
    group_pin: usize,
    image_base: u64,
    epoch: u64,
    generation: u64,
    id: u64,
    completion_address: u64,
    source_graph: u64,
    captured: bool,
    construct_checked: bool,
    constructed: bool,
    failure: IdentityFailure,
}
impl Drop for NativeRootTicket {
    fn drop(&mut self) {
        // SAFETY: the pins this ticket retained, the group before its frame owner.
        unsafe {
            if let (true, Some(release)) =
                (self.group_pin != 0, self.jobs_api.and_then(|a| a.release))
            {
                let mut group = self.group_pin as *mut c_void;
                release(&mut group);
            }
            if let (true, Some(release)) =
                (self.frame_pin != 0, self.owner_api.and_then(|a| a.release))
            {
                release(self.frame_pin as *mut c_void);
            }
        }
        self.group_pin = 0;
        self.frame_pin = 0;
    }
}
impl NativeRootTicket {
    /// The root's identity pinned before its construct (`expected_execution_graph`: an execution override, or 0).
    ///
    /// # Safety
    /// The read owner and group are live engine objects; the callbacks do not unwind.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn capture_before_construct(
        &mut self,
        owner_address: u64,
        graph_pointer_address: u64,
        completion_handle_address: u64,
        caller: u64,
        image_base: u64,
        epoch: u64,
        generation: u64,
        monotonic_id: u64,
        read: Option<Read<'_>>,
        jobs: JobLeaseApi,
        owner: FrameOwnerApi,
        expected_execution_graph: u64,
    ) -> bool {
        if self.captured {
            self.failure = IdentityFailure::AlreadyCaptured;
            return false;
        }
        self.failure = IdentityFailure::Arguments;
        let Some(read) = read else { return false };
        if monotonic_id == 0
            || image_base == 0
            || image_base > u64::MAX - CONSTRUCT_RETURN
            || !pointer(owner_address, 8)
            || !pointer(graph_pointer_address, 8)
            || !pointer(completion_handle_address, 8)
        {
            return false;
        }
        self.failure = IdentityFailure::Caller;
        if caller != image_base + CONSTRUCT_RETURN {
            return false;
        }
        self.failure = IdentityFailure::MissingApi;
        // complete is deliberately unused: a prerequisite group is not the graph's terminal receipt.
        let (Some(retain_group), Some(release_group), Some(retain_owner), Some(release_owner)) =
            (jobs.retain, jobs.release, owner.retain, owner.release)
        else {
            return false;
        };
        let source = |read: &mut dyn FnMut(u64, &mut [u8]) -> bool| {
            read_source(
                read,
                owner_address,
                graph_pointer_address,
                completion_handle_address,
            )
        };
        self.failure = IdentityFailure::Read;
        let Some(first) = source(read) else {
            return false;
        };
        let Some(second) = source(read) else {
            return false;
        };
        self.failure = IdentityFailure::Changed;
        if first != second {
            return false;
        }
        self.failure = IdentityFailure::Arguments;
        if expected_execution_graph != 0
            && (!pointer(expected_execution_graph, 0x70)
                || expected_execution_graph == first.owner
                || expected_execution_graph == first.group)
        {
            return false;
        }
        self.failure = IdentityFailure::References;
        if !references(read, first.owner + 8, true) || !references(read, first.group + 0x1c, true) {
            return false;
        }
        let mut pins = Pins {
            jobs_release: release_group,
            owner_release: release_owner,
            frame: core::ptr::null_mut(),
            group: core::ptr::null_mut(),
        };
        pins.frame = first.owner as usize as *mut c_void;
        retain_owner(pins.frame);
        pins.group = first.group as usize as *mut c_void;
        retain_group(pins.group);
        self.failure = IdentityFailure::Read;
        let Some(second) = source(read) else {
            return false;
        };
        let Some(third) = source(read) else {
            return false;
        };
        self.failure = IdentityFailure::Changed;
        if first != second || first != third {
            return false;
        }
        self.failure = IdentityFailure::References;
        if !references(read, first.owner + 8, false) || !references(read, first.group + 0x1c, false)
        {
            return false;
        }
        self.source_graph = first.graph;
        self.identity = first;
        if expected_execution_graph != 0 {
            self.identity.graph = expected_execution_graph;
        }
        self.image_base = image_base;
        self.epoch = epoch;
        self.generation = generation;
        self.id = monotonic_id;
        self.completion_address = completion_handle_address;
        self.jobs_api = Some(jobs);
        self.owner_api = Some(owner);
        self.frame_pin = pins.frame as usize;
        self.group_pin = pins.group as usize;
        pins.frame = core::ptr::null_mut();
        pins.group = core::ptr::null_mut();
        self.captured = true;
        self.failure = IdentityFailure::None;
        true
    }
    /// Immediately after B56968 only, while the original root's source handle is still live. No owner/graph stack
    /// addresses are retained or read by the later matches.
    pub fn verify_construct(&mut self, destination: u64, read: Option<Read<'_>>) -> bool {
        self.failure = IdentityFailure::NotCaptured;
        if !self.captured {
            return false;
        }
        self.failure = IdentityFailure::ConstructAlreadyChecked;
        if self.construct_checked {
            return false;
        }
        self.construct_checked = true;
        self.failure = IdentityFailure::Read;
        let Some(read) = read else { return false };
        let completion = self.completion_address;
        let Some(first) = read_payload(read, destination) else {
            return false;
        };
        let Some(consumed) = read64(read, completion) else {
            return false;
        };
        let Some(second) = read_payload(read, destination) else {
            return false;
        };
        let Some(again) = read64(read, completion) else {
            return false;
        };
        self.failure = IdentityFailure::ConstructIdentity;
        if first != self.identity || second != self.identity {
            return false;
        }
        self.failure = IdentityFailure::SourceNotConsumed;
        if consumed != 0 || again != 0 {
            return false;
        }
        self.completion_address = 0;
        self.constructed = true;
        self.failure = IdentityFailure::None;
        true
    }
    fn payload_matches(&self, payload: u64, read: &mut dyn FnMut(u64, &mut [u8]) -> bool) -> bool {
        read_payload(read, payload).is_some_and(|first| first == self.identity)
            && read_payload(read, payload).is_some_and(|second| second == self.identity)
    }
    /// The queued payload (queue return 0x3E90BA) carries the verified identity.
    pub fn match_queued(
        &self,
        payload: u64,
        graph: u64,
        caller: u64,
        read: Option<Read<'_>>,
    ) -> bool {
        let Some(read) = read else { return false };
        if !self.captured
            || !self.constructed
            || graph != self.identity.graph
            || self.image_base > u64::MAX - QUEUE_RETURN
            || caller != self.image_base + QUEUE_RETURN
        {
            return false;
        }
        self.payload_matches(payload, read)
    }
    /// The scheduler's execution callback (either return) carries the verified identity.
    pub fn match_execution_callback(
        &self,
        payload: u64,
        graph: u64,
        caller: u64,
        read: Option<Read<'_>>,
    ) -> bool {
        let Some(read) = read else { return false };
        if !self.captured
            || !self.constructed
            || graph != self.identity.graph
            || self.image_base > u64::MAX - EXECUTION_CALLBACK_RETURN
            || (caller != self.image_base + EXECUTION_CALLBACK_RETURN
                && caller != self.image_base + AGGREGATE_EXECUTION_CALLBACK_RETURN)
        {
            return false;
        }
        self.payload_matches(payload, read)
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    pub fn construct_verified(&self) -> bool {
        self.constructed
    }
    /// The C++ face refuses a throwing reader with Read, as the C++ rules do.
    pub(crate) fn set_rejection(&mut self, reason: IdentityFailure) {
        self.failure = reason;
    }
    pub fn rejection(&self) -> IdentityFailure {
        self.failure
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn image_base(&self) -> u64 {
        self.image_base
    }
    pub fn frame_owner(&self) -> u64 {
        self.identity.owner
    }
    pub fn source_graph_address(&self) -> u64 {
        self.source_graph
    }
    pub fn graph_address(&self) -> u64 {
        self.identity.graph
    }
    pub fn redirected(&self) -> bool {
        self.captured && self.source_graph != self.identity.graph
    }
    pub fn group(&self) -> u64 {
        self.identity.group
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/graph_consumer_identity_test.cpp's cases (a thrown read is a failed read here).
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const BASE: u64 = 0x1_4000_0000;
    const OWNER_ADDRESS: u64 = 0x10000;
    const GRAPH_ADDRESS: u64 = 0x10020;
    const COMPLETION_ADDRESS: u64 = 0x10040;
    const DESTINATION: u64 = 0x11000;
    const PAYLOAD: u64 = 0x12000;
    const FRAME: u64 = 0x18000;
    const GROUP: u64 = 0x19000;
    const GRAPH: u64 = 0x20000;
    const EPOCH: u64 = 13;
    const GENERATION: u64 = 47;
    const ID: u64 = 99;
    const CALLERS: [u64; 2] = [
        EXECUTION_CALLBACK_RETURN,
        AGGREGATE_EXECUTION_CALLBACK_RETURN,
    ];

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        reads: u64,
        fail_at: u64,
        change_at: u64,
        change_address: u64,
        change_value: u64,
        owner_retains: u32,
        owner_releases: u32,
        group_retains: u32,
        group_releases: u32,
        complete_calls: u32,
        owner_refs: i32,
        group_refs: i32,
        original_owner_refs: i32,
        original_group_refs: i32,
        change_on_owner_retain: bool,
        change_on_group_retain: bool,
        forbid_source_reads: bool,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        W.with(|w| f(&mut w.borrow_mut()))
    }
    fn put_in(w: &mut World, address: u64, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            w.memory.insert(address + i as u64, *v);
        }
    }
    fn put64(address: u64, v: u64) {
        with(|w| put_in(w, address, &v.to_le_bytes()));
    }
    fn put32(address: u64, v: i32) {
        with(|w| put_in(w, address, &v.to_le_bytes()));
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        with(|w| {
            assert!(
                !w.forbid_source_reads
                    || (address != OWNER_ADDRESS
                        && address != GRAPH_ADDRESS
                        && address != COMPLETION_ADDRESS),
                "the expired root stack was read"
            );
            // The C++ fixture throws on a read of the source graph; the ticket turns that into a failed read.
            if address == GRAPH {
                return false;
            }
            w.reads += 1;
            if w.reads == w.fail_at {
                return false;
            }
            if w.reads == w.change_at {
                let (a, v) = (w.change_address, w.change_value);
                put_in(w, a, &v.to_le_bytes());
            }
            if address > u64::MAX - out.len() as u64 {
                return false;
            }
            for (i, o) in out.iter_mut().enumerate() {
                match w.memory.get(&(address + i as u64)) {
                    Some(v) => *o = *v,
                    None => return false,
                }
            }
            true
        })
    }
    unsafe extern "C" fn retain_owner(value: *mut c_void) {
        with(|w| {
            assert!(
                value as u64 == FRAME && w.owner_refs > 0 && w.owner_refs < i32::MAX,
                "invalid owner retain"
            );
            w.owner_refs += 1;
            w.owner_retains += 1;
            let r = w.owner_refs;
            put_in(w, FRAME + 8, &r.to_le_bytes());
            if w.change_on_owner_retain {
                put_in(w, OWNER_ADDRESS, &(FRAME + 8).to_le_bytes());
            }
        });
    }
    unsafe extern "C" fn release_owner(value: *mut c_void) {
        with(|w| {
            assert!(
                value as u64 == FRAME
                    && w.owner_refs > w.original_owner_refs
                    && w.group_releases == w.group_retains,
                "invalid owner release order"
            );
            w.owner_refs -= 1;
            w.owner_releases += 1;
            let r = w.owner_refs;
            put_in(w, FRAME + 8, &r.to_le_bytes());
        });
    }
    unsafe extern "C" fn retain_group(value: *mut c_void) {
        with(|w| {
            assert!(
                value as u64 == GROUP && w.group_refs > 0 && w.group_refs < i32::MAX,
                "invalid group retain"
            );
            w.group_refs += 1;
            w.group_retains += 1;
            let r = w.group_refs;
            put_in(w, GROUP + 0x1c, &r.to_le_bytes());
            if w.change_on_group_retain {
                put_in(w, COMPLETION_ADDRESS, &(GROUP + 8).to_le_bytes());
            }
        });
    }
    unsafe extern "C" fn release_group(value: *mut *mut c_void) {
        assert!(!value.is_null());
        with(|w| {
            assert!(
                *value as u64 == GROUP && w.group_refs > w.original_group_refs,
                "invalid group release"
            );
            w.group_refs -= 1;
            w.group_releases += 1;
            let r = w.group_refs;
            put_in(w, GROUP + 0x1c, &r.to_le_bytes());
        });
        *value = core::ptr::null_mut();
    }
    unsafe extern "C" fn complete(_: *mut c_void) -> bool {
        with(|w| w.complete_calls += 1);
        true
    }
    const JOBS: JobLeaseApi = JobLeaseApi {
        retain: Some(retain_group),
        release: Some(release_group),
        complete: Some(complete),
    };
    const OWNERS: FrameOwnerApi = FrameOwnerApi {
        retain: Some(retain_owner),
        release: Some(release_owner),
    };

    fn reset() {
        with(|w| {
            *w = World {
                owner_refs: 1,
                group_refs: 1,
                original_owner_refs: 1,
                original_group_refs: 1,
                ..World::default()
            }
        });
        put64(OWNER_ADDRESS, FRAME);
        put64(GRAPH_ADDRESS, GRAPH);
        put64(COMPLETION_ADDRESS, GROUP);
        put32(FRAME + 8, 1);
        put32(GROUP + 0x1c, 1);
        put32(GROUP + 0x18, 0);
    }
    #[allow(clippy::too_many_arguments)]
    fn capture_full(
        ticket: &mut NativeRootTicket,
        owner: u64,
        graph: u64,
        completion: u64,
        caller: u64,
        base: u64,
        id: u64,
        reader: bool,
        jobs: JobLeaseApi,
        owners: FrameOwnerApi,
        execution: u64,
    ) -> bool {
        let reader: Option<Read<'_>> = if reader { Some(&mut read) } else { None };
        unsafe {
            ticket.capture_before_construct(
                owner, graph, completion, caller, base, EPOCH, GENERATION, id, reader, jobs,
                owners, execution,
            )
        }
    }
    fn capture_with(ticket: &mut NativeRootTicket, epoch: u64, generation: u64) -> bool {
        unsafe {
            ticket.capture_before_construct(
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                BASE + CONSTRUCT_RETURN,
                BASE,
                epoch,
                generation,
                ID,
                Some(&mut read),
                JOBS,
                OWNERS,
                0,
            )
        }
    }
    fn capture(ticket: &mut NativeRootTicket) -> bool {
        capture_with(ticket, EPOCH, GENERATION)
    }
    fn capture_execution(ticket: &mut NativeRootTicket, execution: u64) -> bool {
        capture_full(
            ticket,
            OWNER_ADDRESS,
            GRAPH_ADDRESS,
            COMPLETION_ADDRESS,
            BASE + CONSTRUCT_RETURN,
            BASE,
            ID,
            true,
            JOBS,
            OWNERS,
            execution,
        )
    }
    fn native_construct() {
        put64(DESTINATION, FRAME);
        put64(DESTINATION + 8, GRAPH);
        put64(DESTINATION + 0x18, GROUP);
        put64(COMPLETION_ADDRESS, 0);
    }
    fn native_queued() {
        put64(PAYLOAD, FRAME);
        put64(PAYLOAD + 8, GRAPH);
        put64(PAYLOAD + 0x18, GROUP);
        put64(DESTINATION + 0x18, 0);
    }
    fn verify(ticket: &mut NativeRootTicket) -> bool {
        ticket.verify_construct(DESTINATION, Some(&mut read))
    }
    fn queued(ticket: &NativeRootTicket, payload: u64, graph: u64, caller: u64) -> bool {
        ticket.match_queued(payload, graph, caller, Some(&mut read))
    }
    fn callback(ticket: &NativeRootTicket, payload: u64, graph: u64, caller: u64) -> bool {
        ticket.match_execution_callback(payload, graph, caller, Some(&mut read))
    }
    fn set(f: impl FnOnce(&mut World)) {
        with(f);
    }
    fn balanced() {
        with(|w| {
            assert!(
                w.owner_refs == w.original_owner_refs
                    && w.group_refs == w.original_group_refs
                    && w.owner_retains == w.owner_releases
                    && w.group_retains == w.group_releases
                    && w.complete_calls == 0,
                "unbalanced identity refs or a prerequisite used as completion"
            )
        });
    }

    #[test]
    fn lifecycle() {
        reset();
        {
            let mut ticket = NativeRootTicket::default();
            assert!(!ticket.captured() && !ticket.construct_verified());
            assert!(capture(&mut ticket) && ticket.captured() && !ticket.construct_verified());
            assert!(
                ticket.id() == ID && ticket.epoch() == EPOCH && ticket.generation() == GENERATION
            );
            assert!(
                ticket.image_base() == BASE
                    && ticket.frame_owner() == FRAME
                    && ticket.source_graph_address() == GRAPH
            );
            assert!(
                ticket.graph_address() == GRAPH && !ticket.redirected() && ticket.group() == GROUP
            );
            assert!(with(|w| w.owner_refs == 2
                && w.group_refs == 2
                && w.complete_calls == 0));
            assert!(!queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            for caller in CALLERS {
                assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + caller));
            }
            native_construct();
            assert!(verify(&mut ticket) && ticket.construct_verified());
            assert!(!verify(&mut ticket));
            native_queued();
            set(|w| {
                w.forbid_source_reads = true;
                for address in [OWNER_ADDRESS, GRAPH_ADDRESS, COMPLETION_ADDRESS] {
                    for i in 0..8 {
                        w.memory.remove(&(address + i));
                    }
                }
            });
            assert!(queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            for caller in CALLERS {
                assert!(callback(&ticket, PAYLOAD, GRAPH, BASE + caller));
            }
            assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            assert!(!callback(
                &ticket,
                PAYLOAD,
                GRAPH + 8,
                BASE + EXECUTION_CALLBACK_RETURN
            ));
            let (current_epoch, current_generation) = (EPOCH + 1, GENERATION + 1);
            assert!(current_epoch != ticket.epoch() && current_generation != ticket.generation());
            assert!(queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            assert!(
                !capture(&mut ticket) && ticket.rejection() == IdentityFailure::AlreadyCaptured
            );
            assert!(queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            for pending in [0, 7] {
                put32(GROUP + 0x18, pending);
                assert!(queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            }
            assert!(with(|w| w.owner_retains == 1
                && w.group_retains == 1
                && w.owner_releases == 0
                && w.group_releases == 0));
        }
        balanced();
        reset();
        {
            let mut ticket = NativeRootTicket::default();
            assert!(
                capture_with(&mut ticket, 0, 0) && ticket.epoch() == 0 && ticket.generation() == 0
            );
        }
        balanced();
    }
    #[test]
    fn invalid_arguments() {
        reset();
        let ok = BASE + CONSTRUCT_RETURN;
        let cases: [(u64, u64, u64, u64, u64, u64, bool); 10] = [
            (0, GRAPH_ADDRESS, COMPLETION_ADDRESS, ok, BASE, ID, true),
            (
                OWNER_ADDRESS + 1,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                BASE,
                ID,
                true,
            ),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS + 1,
                COMPLETION_ADDRESS,
                ok,
                BASE,
                ID,
                true,
            ),
            (OWNER_ADDRESS, GRAPH, COMPLETION_ADDRESS, ok, BASE, ID, true),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                u64::MAX - 7,
                ok,
                BASE,
                ID,
                true,
            ),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok + 1,
                BASE,
                ID,
                true,
            ),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                0,
                ID,
                true,
            ),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                u64::MAX,
                ID,
                true,
            ),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                BASE,
                0,
                true,
            ),
            (
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                BASE,
                ID,
                false,
            ),
        ];
        for (i, (owner, graph, completion, caller, base, id, reader)) in
            cases.into_iter().enumerate()
        {
            let mut ticket = NativeRootTicket::default();
            assert!(
                !capture_full(
                    &mut ticket,
                    owner,
                    graph,
                    completion,
                    caller,
                    base,
                    id,
                    reader,
                    JOBS,
                    OWNERS,
                    0
                ) && !ticket.captured(),
                "case {i}"
            );
            drop(ticket);
            balanced();
        }
        for missing in 0..4 {
            let (mut jobs, mut owners) = (JOBS, OWNERS);
            match missing {
                0 => jobs.retain = None,
                1 => jobs.release = None,
                2 => owners.retain = None,
                _ => owners.release = None,
            }
            let mut ticket = NativeRootTicket::default();
            assert!(!capture_full(
                &mut ticket,
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                BASE,
                ID,
                true,
                jobs,
                owners,
                0
            ));
            assert_eq!(ticket.rejection(), IdentityFailure::MissingApi);
        }
        balanced();
        // A complete callback is neither required nor called by identity-only tickets.
        {
            let mut ticket = NativeRootTicket::default();
            let jobs = JobLeaseApi {
                complete: None,
                ..JOBS
            };
            assert!(capture_full(
                &mut ticket,
                OWNER_ADDRESS,
                GRAPH_ADDRESS,
                COMPLETION_ADDRESS,
                ok,
                BASE,
                ID,
                true,
                jobs,
                OWNERS,
                0
            ));
        }
        balanced();
        for value in [0, 1, u64::MAX - 7] {
            for address in [OWNER_ADDRESS, GRAPH_ADDRESS, COMPLETION_ADDRESS] {
                reset();
                put64(address, value);
                let mut ticket = NativeRootTicket::default();
                assert!(!capture(&mut ticket));
                drop(ticket);
                balanced();
            }
        }
        reset();
        put64(OWNER_ADDRESS, GROUP);
        {
            let mut ticket = NativeRootTicket::default();
            assert!(
                !capture(&mut ticket),
                "owner/group reference domains were aliased"
            );
        }
        balanced();
        for refs in [0, -1, i32::MAX] {
            for address in [FRAME + 8, GROUP + 0x1c] {
                reset();
                put32(address, refs);
                let mut ticket = NativeRootTicket::default();
                assert!(!capture(&mut ticket) && ticket.rejection() == IdentityFailure::References);
                drop(ticket);
                balanced();
            }
        }
        reset();
        set(|w| {
            w.owner_refs = i32::MAX - 1;
            w.group_refs = i32::MAX - 1;
            w.original_owner_refs = i32::MAX - 1;
            w.original_group_refs = i32::MAX - 1;
        });
        put32(FRAME + 8, i32::MAX - 1);
        put32(GROUP + 0x1c, i32::MAX - 1);
        {
            let mut ticket = NativeRootTicket::default();
            assert!(
                capture(&mut ticket)
                    && with(|w| w.owner_refs == i32::MAX && w.group_refs == i32::MAX)
            );
        }
        balanced();
    }
    #[test]
    fn rollback_and_torn_reads() {
        reset();
        let capture_reads = {
            let mut ticket = NativeRootTicket::default();
            assert!(capture(&mut ticket));
            with(|w| w.reads)
        };
        balanced();
        for at in 1..=capture_reads {
            reset();
            set(|w| w.fail_at = at);
            let mut ticket = NativeRootTicket::default();
            assert!(!capture(&mut ticket) && !ticket.captured());
            drop(ticket);
            balanced();
        }
        for on_owner in [false, true] {
            reset();
            set(|w| {
                w.change_on_owner_retain = on_owner;
                w.change_on_group_retain = !on_owner;
            });
            {
                let mut ticket = NativeRootTicket::default();
                assert!(!capture(&mut ticket) && ticket.rejection() == IdentityFailure::Changed);
            }
            balanced();
            assert!(with(|w| w.owner_retains == 1 && w.group_retains == 1));
        }
        for address in [OWNER_ADDRESS, GRAPH_ADDRESS, COMPLETION_ADDRESS] {
            reset();
            set(|w| {
                w.change_at = 4;
                w.change_address = address;
                w.change_value = match address {
                    OWNER_ADDRESS => FRAME + 8,
                    GRAPH_ADDRESS => GRAPH + 8,
                    _ => GROUP + 8,
                };
            });
            {
                let mut ticket = NativeRootTicket::default();
                assert!(!capture(&mut ticket) && ticket.rejection() == IdentityFailure::Changed);
            }
            balanced();
            assert!(with(|w| w.owner_retains == 0 && w.group_retains == 0));
        }
    }
    #[test]
    fn construct_and_queue_failures() {
        for field in [
            DESTINATION,
            DESTINATION + 8,
            DESTINATION + 0x18,
            COMPLETION_ADDRESS,
        ] {
            reset();
            {
                let mut ticket = NativeRootTicket::default();
                assert!(capture(&mut ticket));
                native_construct();
                put64(field, 0x33330);
                assert!(!verify(&mut ticket) && !ticket.construct_verified());
                native_construct();
                assert!(
                    !verify(&mut ticket),
                    "a rejected construction retried into a valid receipt"
                );
            }
            balanced();
        }
        for at in 1..=8 {
            reset();
            {
                let mut ticket = NativeRootTicket::default();
                assert!(capture(&mut ticket));
                native_construct();
                set(|w| {
                    w.reads = 0;
                    w.fail_at = at;
                });
                assert!(!verify(&mut ticket));
            }
            balanced();
        }
        for field in [PAYLOAD, PAYLOAD + 8, PAYLOAD + 0x18] {
            reset();
            {
                let mut ticket = NativeRootTicket::default();
                assert!(capture(&mut ticket));
                native_construct();
                assert!(verify(&mut ticket));
                native_queued();
                put64(field, 0x33330);
                assert!(!queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
                native_queued();
                set(|w| {
                    w.reads = 0;
                    w.change_at = 4;
                    w.change_address = field;
                    w.change_value = 0x33330;
                });
                assert!(!queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            }
            balanced();
        }
        reset();
        {
            let mut ticket = NativeRootTicket::default();
            assert!(capture(&mut ticket));
            native_construct();
            assert!(verify(&mut ticket));
            native_queued();
            assert!(!queued(&ticket, PAYLOAD, GRAPH + 8, BASE + QUEUE_RETURN));
            assert!(!queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN + 1));
            assert!(!queued(&ticket, u64::MAX - 7, GRAPH, BASE + QUEUE_RETURN));
            assert!(!ticket.match_queued(PAYLOAD, GRAPH, BASE + QUEUE_RETURN, None));
            for at in 1..=6 {
                set(|w| {
                    w.reads = 0;
                    w.fail_at = at;
                });
                assert!(!queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            }
        }
        balanced();
        reset();
        {
            let mut ticket = NativeRootTicket::default();
            assert!(capture(&mut ticket));
            native_construct();
            set(|w| {
                w.reads = 0;
                w.change_at = 5;
                w.change_address = COMPLETION_ADDRESS;
                w.change_value = GROUP;
            });
            assert!(
                !verify(&mut ticket) && ticket.rejection() == IdentityFailure::SourceNotConsumed
            );
        }
        balanced();
    }
    #[test]
    fn execution_callback_failures() {
        for caller in CALLERS {
            reset();
            {
                let mut ticket = NativeRootTicket::default();
                assert!(capture(&mut ticket));
                native_construct();
                assert!(verify(&mut ticket));
                native_queued();
                set(|w| w.forbid_source_reads = true);
                assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + caller - 1));
                assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + caller + 1));
                assert!(!callback(&ticket, PAYLOAD, GRAPH + 8, BASE + caller));
                assert!(!callback(&ticket, u64::MAX - 7, GRAPH, BASE + caller));
                assert!(!ticket.match_execution_callback(PAYLOAD, GRAPH, BASE + caller, None));
                for field in [PAYLOAD, PAYLOAD + 8, PAYLOAD + 0x18] {
                    put64(field, 0x33330);
                    assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + caller));
                    native_queued();
                    set(|w| {
                        w.reads = 0;
                        w.change_at = 4;
                        w.change_address = field;
                        w.change_value = 0x33330;
                    });
                    assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + caller));
                    set(|w| w.change_at = 0);
                    native_queued();
                }
                for at in 1..=6 {
                    set(|w| {
                        w.reads = 0;
                        w.fail_at = at;
                    });
                    assert!(!callback(&ticket, PAYLOAD, GRAPH, BASE + caller));
                }
                set(|w| w.fail_at = 0);
                assert!(callback(&ticket, PAYLOAD, GRAPH, BASE + caller));
            }
            balanced();
        }
    }
    #[test]
    fn separate_source_and_execution() {
        const EXECUTION: u64 = 0x30000;
        reset();
        {
            let mut ticket = NativeRootTicket::default();
            assert!(capture_execution(&mut ticket, EXECUTION));
            assert!(
                ticket.source_graph_address() == GRAPH
                    && ticket.graph_address() == EXECUTION
                    && ticket.redirected()
            );
            assert!(with(|w| w.owner_retains == 1 && w.group_retains == 1));
            // Publication belongs to the transaction owner, not this read-only ticket.
            let mut original = [0u8; 8];
            assert!(read(GRAPH_ADDRESS, &mut original) && u64::from_le_bytes(original) == GRAPH);
            native_construct();
            put64(DESTINATION + 8, EXECUTION);
            assert!(verify(&mut ticket));
            native_queued();
            put64(PAYLOAD + 8, EXECUTION);
            set(|w| w.forbid_source_reads = true);
            assert!(queued(&ticket, PAYLOAD, EXECUTION, BASE + QUEUE_RETURN));
            assert!(callback(
                &ticket,
                PAYLOAD,
                EXECUTION,
                BASE + AGGREGATE_EXECUTION_CALLBACK_RETURN
            ));
            assert!(!queued(&ticket, PAYLOAD, GRAPH, BASE + QUEUE_RETURN));
            assert!(!callback(
                &ticket,
                PAYLOAD,
                GRAPH,
                BASE + EXECUTION_CALLBACK_RETURN
            ));
        }
        balanced();
        reset();
        {
            let mut ticket = NativeRootTicket::default();
            assert!(capture_execution(&mut ticket, EXECUTION));
            native_construct();
            assert!(
                !verify(&mut ticket),
                "an unpublished execution override was accepted"
            );
        }
        balanced();
        for invalid in [1, u64::MAX - 7, FRAME, GROUP] {
            reset();
            {
                let mut ticket = NativeRootTicket::default();
                assert!(!capture_execution(&mut ticket, invalid));
            }
            balanced();
        }
        reset();
        set(|w| {
            w.change_at = 4;
            w.change_address = GRAPH_ADDRESS;
            w.change_value = GRAPH + 8;
        });
        {
            let mut ticket = NativeRootTicket::default();
            assert!(
                !capture_execution(&mut ticket, EXECUTION),
                "a torn source hidden by the execution override"
            );
        }
        balanced();
    }
}
