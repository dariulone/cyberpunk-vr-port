//! The completion of a render graph's declaration jobs, pinned through the declaration (include/Render/
//! NativeGraphDeclarationCompletion.hpp).
//!
//! Captured before the construct (return 0x40A4AB) from the producer context (the owner address - 0x38): its frame
//! owner (+0x38) and its D0 group (+0x18) with a live reference and a non-negative pending count, read twice, then D0
//! retained and everything read twice more. Matched once immediately before the original A86438 (return 0x40A756),
//! while the original root still owns its context: the job is that context, the graph the frozen execution graph, the
//! identity unchanged (native 141B10 may rotate context+18 right after the dispatch). Complete only after the match,
//! the declaration's and the root's normal returns and the pinned D0's completion. This pins only D0: it neither owns
//! definitions nor waits or authorizes storage destruction. The first rejection sticks; the context address is cleared
//! on the root's return even after a failure. The caller serializes access and proves native body/dispatch coverage.
#![allow(unsafe_op_in_unsafe_fn)]
use super::job_lease::JobLeaseApi;
use core::ffi::c_void;

pub const DECLARATION_CONSTRUCT_RETURN: u64 = 0x40A4AB;
pub const DECLARATION_RETURN: u64 = 0x40A756;

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Caller,
    AlreadyAttempted,
    MissingApi,
    Read,
    FrameOwner,
    Group,
    References,
    Pending,
    Changed,
    NotCaptured,
    ProducerClosed,
    AlreadyMatched,
    DeclarationIdentity,
    NotMatched,
    AlreadyReturned,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Identity {
    frame_owner: u64,
    group: u64,
}

fn pointer(address: u64, bytes: u64) -> bool {
    address != 0 && address & 7 == 0 && address <= u64::MAX - bytes
}

pub struct DeclarationCompletionLease {
    identity: Identity,
    api: Option<JobLeaseApi>,
    group_pin: *mut c_void,
    context_address: u64,
    execution_graph: u64,
    image_base: u64,
    epoch: u64,
    generation: u64,
    consumer_id: u64,
    attempted: bool,
    captured: bool,
    matched: bool,
    declare_returned: bool,
    root_returned: bool,
    failed: bool,
    failure: Failure,
}
impl Default for DeclarationCompletionLease {
    fn default() -> Self {
        DeclarationCompletionLease {
            identity: Identity::default(),
            api: None,
            group_pin: core::ptr::null_mut(),
            context_address: 0,
            execution_graph: 0,
            image_base: 0,
            epoch: 0,
            generation: 0,
            consumer_id: 0,
            attempted: false,
            captured: false,
            matched: false,
            declare_returned: false,
            root_returned: false,
            failed: false,
            failure: Failure::None,
        }
    }
}
/// A group pin taken during a capture, released unless handed over.
struct Pin {
    release: unsafe extern "C" fn(*mut *mut c_void),
    group: *mut c_void,
}
impl Drop for Pin {
    fn drop(&mut self) {
        if !self.group.is_null() {
            // SAFETY: the group this capture retained.
            unsafe { (self.release)(&mut self.group) };
        }
    }
}

impl Drop for DeclarationCompletionLease {
    fn drop(&mut self) {
        if let (false, Some(release)) = (self.group_pin.is_null(), self.api.and_then(|a| a.release))
        {
            // SAFETY: the group this lease retained.
            unsafe { release(&mut self.group_pin) };
        }
    }
}
impl DeclarationCompletionLease {
    /// The first refusal is kept; every later call is refused (the C++ face also refuses a throwing reader with Read).
    pub(crate) fn reject(&mut self, reason: Failure) -> bool {
        if !self.failed {
            self.failure = reason;
        }
        self.failed = true;
        false
    }
    fn read_identity(
        &mut self,
        context: u64,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        before_retain: bool,
    ) -> Option<Identity> {
        self.failure = Failure::Read;
        let (mut frame, mut group) = ([0u8; 8], [0u8; 8]);
        if !read(context + 0x38, &mut frame) || !read(context + 0x18, &mut group) {
            return None;
        }
        let out = Identity {
            frame_owner: u64::from_le_bytes(frame),
            group: u64::from_le_bytes(group),
        };
        self.failure = Failure::FrameOwner;
        if !pointer(out.frame_owner, 12) {
            return None;
        }
        self.failure = Failure::Group;
        if !pointer(out.group, 0x20) || out.group == out.frame_owner {
            return None;
        }
        self.failure = Failure::Read;
        let (mut refs, mut pending) = ([0u8; 4], [0u8; 4]);
        if !read(out.group + 0x1c, &mut refs) || !read(out.group + 0x18, &mut pending) {
            return None;
        }
        let (refs, pending) = (i32::from_le_bytes(refs), i32::from_le_bytes(pending));
        self.failure = Failure::References;
        if refs <= 0 || (before_retain && refs == i32::MAX) {
            return None;
        }
        self.failure = Failure::Pending;
        if pending < 0 {
            return None;
        }
        Some(out)
    }

    /// D0 pinned before the construct runs.
    ///
    /// # Safety
    /// The api's callbacks are the engine's group calls (they do not unwind).
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn capture_before_construct(
        &mut self,
        owner_address: u64,
        execution_graph: u64,
        caller: u64,
        image_base: u64,
        epoch: u64,
        generation: u64,
        consumer_id: u64,
        read: Option<Read<'_>>,
        api: JobLeaseApi,
    ) -> bool {
        if self.failed {
            return false;
        }
        if self.attempted {
            return self.reject(Failure::AlreadyAttempted);
        }
        self.attempted = true;
        let Some(read) = read else {
            return self.reject(Failure::Arguments);
        };
        if image_base == 0
            || image_base > u64::MAX - DECLARATION_RETURN
            || epoch == 0
            || generation == 0
            || consumer_id == 0
            || owner_address < 0x38
            || !pointer(owner_address - 0x38, 0x40)
            || !pointer(execution_graph, 0x70)
        {
            return self.reject(Failure::Arguments);
        }
        if caller != image_base + DECLARATION_CONSTRUCT_RETURN {
            return self.reject(Failure::Caller);
        }
        let (Some(retain), Some(release), Some(_)) = (api.retain, api.release, api.complete) else {
            return self.reject(Failure::MissingApi);
        };
        let context = owner_address - 0x38;
        let Some(first) = self.read_identity(context, read, true) else {
            return self.reject(self.failure);
        };
        let Some(second) = self.read_identity(context, read, true) else {
            return self.reject(self.failure);
        };
        if first != second {
            return self.reject(Failure::Changed);
        }
        // Released on every way out but success -- a refusal or a reader unwinding through here (the C++ face's
        // readers may throw).
        let mut pin = Pin {
            release,
            group: first.group as usize as *mut c_void,
        };
        retain(pin.group);
        let second = self.read_identity(context, read, false);
        let third = second.and_then(|_| self.read_identity(context, read, false));
        let (Some(second), Some(third)) = (second, third) else {
            return self.reject(self.failure);
        };
        if first != second || first != third {
            return self.reject(Failure::Changed);
        }
        self.identity = first;
        self.context_address = context;
        self.execution_graph = execution_graph;
        self.image_base = image_base;
        self.epoch = epoch;
        self.generation = generation;
        self.consumer_id = consumer_id;
        self.api = Some(api);
        self.group_pin = core::mem::replace(&mut pin.group, core::ptr::null_mut());
        self.captured = true;
        self.failure = Failure::None;
        true
    }
    /// Immediately before the original A86438 only, while the original root still owns its context.
    pub fn match_before_declare(
        &mut self,
        job: u64,
        graph: u64,
        caller: u64,
        read: Option<Read<'_>>,
    ) -> bool {
        if self.failed {
            return false;
        }
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if self.root_returned || self.context_address == 0 {
            return self.reject(Failure::ProducerClosed);
        }
        if self.matched {
            return self.reject(Failure::AlreadyMatched);
        }
        let Some(read) = read else {
            return self.reject(Failure::Arguments);
        };
        if self.image_base > u64::MAX - DECLARATION_RETURN {
            return self.reject(Failure::Arguments);
        }
        if caller != self.image_base + DECLARATION_RETURN {
            return self.reject(Failure::Caller);
        }
        if job != self.context_address || graph != self.execution_graph {
            return self.reject(Failure::DeclarationIdentity);
        }
        let Some(first) = self.read_identity(job, read, false) else {
            return self.reject(self.failure);
        };
        let Some(second) = self.read_identity(job, read, false) else {
            return self.reject(self.failure);
        };
        if first != self.identity || second != self.identity {
            return self.reject(Failure::Changed);
        }
        self.matched = true;
        self.failure = Failure::None;
        true
    }
    pub fn mark_original_declare_returned(&mut self) -> bool {
        if self.failed {
            return false;
        }
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if !self.matched {
            return self.reject(Failure::NotMatched);
        }
        if self.declare_returned {
            return self.reject(Failure::AlreadyReturned);
        }
        self.declare_returned = true;
        self.failure = Failure::None;
        true
    }
    /// The original 219730's normal return (not the outer AA scheduler's); the context address cleared even after an
    /// earlier failure.
    pub fn mark_original_root_returned(&mut self) -> bool {
        self.context_address = 0;
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if self.root_returned {
            return self.reject(Failure::AlreadyReturned);
        }
        self.root_returned = true;
        !self.failed
    }
    pub fn complete(&self) -> bool {
        let Some(complete) = self.api.and_then(|a| a.complete) else {
            return false;
        };
        self.captured
            && !self.failed
            && self.matched
            && self.declare_returned
            && self.root_returned
            && !self.group_pin.is_null()
            // SAFETY: the group this lease holds.
            && unsafe { complete(self.group_pin) }
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    pub fn matched(&self) -> bool {
        self.matched
    }
    pub fn original_declare_returned(&self) -> bool {
        self.declare_returned
    }
    pub fn original_root_returned(&self) -> bool {
        self.root_returned
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn context_address(&self) -> u64 {
        self.context_address
    }
    pub fn group(&self) -> u64 {
        self.group_pin as u64
    }
    pub fn frame_owner(&self) -> u64 {
        self.identity.frame_owner
    }
    pub fn execution_graph(&self) -> u64 {
        self.execution_graph
    }
    pub fn image_base(&self) -> u64 {
        self.image_base
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn id(&self) -> u64 {
        self.consumer_id
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/graph_declaration_completion_test.cpp's cases (a thrown read is a failed read here).
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const BASE: u64 = 0x1_4000_0000;
    const CONTEXT: u64 = 0x10000;
    const OWNER_ADDRESS: u64 = CONTEXT + 0x38;
    const FRAME: u64 = 0x20000;
    const GRAPH: u64 = 0x30000;
    const GROUP: u64 = 0x40000;
    const ROTATED: u64 = 0x50000;
    const EPOCH: u64 = 11;
    const GENERATION: u64 = 12;
    const CONSUMER: u64 = 13;

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        reads: u64,
        fail_at: u64,
        change_at: u64,
        change_address: u64,
        change_value: u64,
        refs: i32,
        baseline_refs: i32,
        pending: i32,
        retains: u32,
        releases: u32,
        complete_calls: u32,
        change_on_retain: bool,
        forbid_reads: bool,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        W.with(|w| f(&mut w.borrow_mut()))
    }
    fn put(w: &mut World, address: u64, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            w.memory.insert(address + i as u64, *v);
        }
    }
    fn put64(address: u64, v: u64) {
        with(|w| put(w, address, &v.to_le_bytes()));
    }
    fn put32(address: u64, v: i32) {
        with(|w| put(w, address, &v.to_le_bytes()));
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        with(|w| {
            assert!(!w.forbid_reads, "the lease reread an expired context");
            w.reads += 1;
            if w.reads == w.fail_at {
                return false;
            }
            if w.reads == w.change_at {
                let (a, v) = (w.change_address, w.change_value);
                put(w, a, &v.to_le_bytes());
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
    unsafe extern "C" fn retain(group: *mut c_void) {
        with(|w| {
            assert!(
                group as u64 == GROUP && w.refs > 0 && w.refs < i32::MAX,
                "invalid D0 retain"
            );
            w.refs += 1;
            w.retains += 1;
            let r = w.refs;
            put(w, GROUP + 0x1c, &r.to_le_bytes());
            if w.change_on_retain {
                put(w, CONTEXT + 0x18, &ROTATED.to_le_bytes());
            }
        });
    }
    unsafe extern "C" fn release(group: *mut *mut c_void) {
        with(|w| {
            assert!(
                *group as u64 == GROUP && w.refs > 0 && w.releases < w.retains,
                "invalid D0 release"
            );
            w.refs -= 1;
            w.releases += 1;
            let r = w.refs;
            put(w, GROUP + 0x1c, &r.to_le_bytes());
        });
        *group = core::ptr::null_mut();
    }
    unsafe extern "C" fn complete(group: *mut c_void) -> bool {
        with(|w| {
            assert!(group as u64 == GROUP, "polled a rotated group");
            w.complete_calls += 1;
            w.pending == 0
        })
    }
    const API: JobLeaseApi = JobLeaseApi {
        retain: Some(retain),
        release: Some(release),
        complete: Some(complete),
    };
    fn reset() {
        with(|w| {
            *w = World {
                refs: 1,
                baseline_refs: 1,
                ..World::default()
            }
        });
        put64(OWNER_ADDRESS, FRAME);
        put64(CONTEXT + 0x18, GROUP);
        put32(GROUP + 0x1c, 1);
        put32(GROUP + 0x18, 0);
        put32(ROTATED + 0x1c, 1);
        put32(ROTATED + 0x18, 0);
    }
    fn capture_with(
        l: &mut DeclarationCompletionLease,
        epoch: u64,
        generation: u64,
        consumer: u64,
        graph: u64,
    ) -> bool {
        unsafe {
            l.capture_before_construct(
                OWNER_ADDRESS,
                graph,
                BASE + DECLARATION_CONSTRUCT_RETURN,
                BASE,
                epoch,
                generation,
                consumer,
                Some(&mut read),
                API,
            )
        }
    }
    fn capture(l: &mut DeclarationCompletionLease) -> bool {
        capture_with(l, EPOCH, GENERATION, CONSUMER, GRAPH)
    }
    fn matched(l: &mut DeclarationCompletionLease) -> bool {
        let graph = l.execution_graph();
        l.match_before_declare(CONTEXT, graph, BASE + DECLARATION_RETURN, Some(&mut read))
    }
    fn balanced() {
        with(|w| {
            assert!(
                w.refs == w.baseline_refs && w.retains == w.releases,
                "D0 pin imbalance"
            )
        });
    }

    #[test]
    fn lifecycle() {
        for _ in 0..64 {
            reset();
            {
                let mut l = DeclarationCompletionLease::default();
                assert!(!l.captured() && !l.complete());
                assert!(
                    capture(&mut l)
                        && l.context_address() == CONTEXT
                        && l.group() == GROUP
                        && with(|w| w.refs) == 2
                );
                assert!(!l.complete() && with(|w| w.complete_calls) == 0);
                assert!(matched(&mut l) && l.matched());
                assert!(!l.complete() && with(|w| w.complete_calls) == 0);
                assert!(
                    l.mark_original_declare_returned()
                        && !l.complete()
                        && with(|w| w.complete_calls) == 0
                );
                put64(CONTEXT + 0x18, ROTATED);
                with(|w| w.pending = 2);
                assert!(l.mark_original_root_returned() && l.context_address() == 0);
                with(|w| {
                    w.refs -= 1;
                    w.baseline_refs = 0;
                    w.memory.clear();
                    w.forbid_reads = true;
                });
                assert!(!l.complete() && with(|w| w.complete_calls) == 1);
                with(|w| w.pending = 0);
                assert!(l.complete() && with(|w| w.complete_calls) == 2);
            }
            balanced();
        }
    }
    #[test]
    fn invalid_capture() {
        let ok = BASE + DECLARATION_CONSTRUCT_RETURN;
        #[allow(clippy::type_complexity)]
        let cases: [(u64, u64, u64, u64, u64, u64, u64, bool, JobLeaseApi); 15] = [
            (0, GRAPH, ok, BASE, EPOCH, GENERATION, CONSUMER, true, API),
            (
                0x30, GRAPH, ok, BASE, EPOCH, GENERATION, CONSUMER, true, API,
            ),
            (
                OWNER_ADDRESS + 1,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                u64::MAX - 7,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH + 1,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                u64::MAX - 7,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok + 1,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                u64::MAX,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                0,
                GENERATION,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                0,
                CONSUMER,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                0,
                true,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                false,
                API,
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                JobLeaseApi {
                    retain: None,
                    ..API
                },
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                JobLeaseApi {
                    release: None,
                    ..API
                },
            ),
            (
                OWNER_ADDRESS,
                GRAPH,
                ok,
                BASE,
                EPOCH,
                GENERATION,
                CONSUMER,
                true,
                JobLeaseApi {
                    complete: None,
                    ..API
                },
            ),
        ];
        for (i, (owner, graph, caller, base, epoch, generation, consumer, reader, api)) in
            cases.into_iter().enumerate()
        {
            reset();
            {
                let mut l = DeclarationCompletionLease::default();
                let reader: Option<Read<'_>> = if reader { Some(&mut read) } else { None };
                let accepted = unsafe {
                    l.capture_before_construct(
                        owner, graph, caller, base, epoch, generation, consumer, reader, api,
                    )
                };
                assert!(!accepted && !l.captured() && !l.complete(), "case {i}");
            }
            balanced();
        }
        for group in [0, GROUP + 1, FRAME, u64::MAX - 7] {
            reset();
            put64(CONTEXT + 0x18, group);
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && !l.complete());
            drop(l);
            balanced();
        }
        for owner in [0, FRAME + 1, u64::MAX - 7] {
            reset();
            put64(OWNER_ADDRESS, owner);
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::FrameOwner);
            drop(l);
            balanced();
        }
        for count in [0, -1, i32::MAX] {
            reset();
            put32(GROUP + 0x1c, count);
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::References);
            drop(l);
            balanced();
        }
        reset();
        put32(GROUP + 0x18, -1);
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::Pending);
        }
        balanced();
        reset();
        with(|w| {
            w.refs = i32::MAX - 1;
            w.baseline_refs = i32::MAX - 1;
        });
        put32(GROUP + 0x1c, i32::MAX - 1);
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l) && with(|w| w.refs) == i32::MAX);
        }
        balanced();
    }
    #[test]
    fn partial_reads_and_changes() {
        reset();
        let capture_reads = {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l));
            with(|w| w.reads)
        };
        balanced();
        for at in 1..=capture_reads {
            reset();
            with(|w| w.fail_at = at);
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && !l.captured() && !l.complete());
            drop(l);
            balanced();
        }
        reset();
        with(|w| w.change_on_retain = true);
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::Changed);
        }
        balanced();
        assert!(with(|w| w.retains == 1 && w.releases == 1));
        for address in [OWNER_ADDRESS, CONTEXT + 0x18] {
            reset();
            with(|w| {
                w.change_at = 5;
                w.change_address = address;
                w.change_value = if address == OWNER_ADDRESS {
                    FRAME + 0x100
                } else {
                    ROTATED
                };
            });
            let mut l = DeclarationCompletionLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::Changed);
            drop(l);
            balanced();
        }
        reset();
        let match_reads = {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l));
            with(|w| w.reads = 0);
            assert!(matched(&mut l));
            with(|w| w.reads)
        };
        balanced();
        for at in 1..=match_reads {
            reset();
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l));
            with(|w| {
                w.reads = 0;
                w.fail_at = at;
            });
            assert!(!matched(&mut l) && l.failed() && !l.complete());
            assert!(!l.mark_original_root_returned() && l.context_address() == 0);
            drop(l);
            balanced();
        }
    }
    #[test]
    fn match_failures() {
        for failure in 0..9 {
            reset();
            {
                let mut l = DeclarationCompletionLease::default();
                assert!(capture(&mut l));
                let (mut job, mut graph, mut caller, mut no_reader) =
                    (CONTEXT, GRAPH, BASE + DECLARATION_RETURN, false);
                match failure {
                    0 => job += 1,
                    1 => graph += 8,
                    2 => caller += 1,
                    3 => caller -= 1,
                    4 => no_reader = true,
                    5 => put64(CONTEXT + 0x18, ROTATED),
                    6 => put64(OWNER_ADDRESS, FRAME + 0x100),
                    7 => with(|w| {
                        w.change_at = w.reads + 5;
                        w.change_address = CONTEXT + 0x18;
                        w.change_value = ROTATED;
                    }),
                    _ => put32(GROUP + 0x1c, 0),
                }
                let reader: Option<Read<'_>> = if no_reader { None } else { Some(&mut read) };
                assert!(
                    !l.match_before_declare(job, graph, caller, reader)
                        && l.failed()
                        && !l.complete(),
                    "case {failure}"
                );
                l.mark_original_declare_returned();
                l.mark_original_root_returned();
                with(|w| w.forbid_reads = true);
                assert!(
                    !l.complete() && l.context_address() == 0 && with(|w| w.complete_calls) == 0
                );
            }
            balanced();
        }
        reset();
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l) && matched(&mut l));
            assert!(!matched(&mut l) && l.failed() && !l.complete());
        }
        balanced();
        reset();
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l) && l.mark_original_root_returned());
            with(|w| w.forbid_reads = true);
            assert!(!matched(&mut l) && l.rejection() == Failure::ProducerClosed && !l.complete());
        }
        balanced();
    }
    #[test]
    fn missing_returns_and_rearm() {
        for missing in 0..3 {
            reset();
            {
                let mut l = DeclarationCompletionLease::default();
                assert!(capture(&mut l));
                if missing != 0 {
                    assert!(matched(&mut l));
                }
                if missing != 1 && missing != 0 {
                    assert!(l.mark_original_declare_returned());
                }
                if missing != 2 {
                    assert!(l.mark_original_root_returned());
                }
                assert!(!l.complete() && with(|w| w.complete_calls) == 0);
            }
            balanced();
        }
        reset();
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l));
            assert!(!l.mark_original_declare_returned() && l.failed() && !l.complete());
        }
        balanced();
        reset();
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(!l.mark_original_root_returned());
            with(|w| w.forbid_reads = true);
            assert!(
                !capture(&mut l)
                    && !l.captured()
                    && with(|w| w.retains) == 0
                    && l.context_address() == 0
            );
        }
        balanced();
        reset();
        {
            let (mut first, mut second) = (
                DeclarationCompletionLease::default(),
                DeclarationCompletionLease::default(),
            );
            assert!(
                capture(&mut first)
                    && matched(&mut first)
                    && first.mark_original_declare_returned()
                    && first.mark_original_root_returned()
            );
            assert!(
                capture_with(
                    &mut second,
                    EPOCH + 1,
                    GENERATION + 1,
                    CONSUMER + 1,
                    GRAPH + 0x100
                ) && matched(&mut second)
            );
            assert!(
                second.mark_original_declare_returned() && second.mark_original_root_returned()
            );
            with(|w| {
                w.memory.clear();
                w.forbid_reads = true;
            });
            assert!(
                first.complete()
                    && second.complete()
                    && first.epoch() == EPOCH
                    && first.generation() == GENERATION
                    && first.id() == CONSUMER
            );
            assert!(
                first.execution_graph() == GRAPH
                    && second.execution_graph() == GRAPH + 0x100
                    && with(|w| w.refs) == 3
            );
        }
        balanced();
        reset();
        {
            let mut l = DeclarationCompletionLease::default();
            assert!(capture(&mut l));
            assert!(!capture_with(
                &mut l,
                EPOCH + 1,
                GENERATION + 1,
                CONSUMER + 1,
                GRAPH + 0x100
            ));
            assert!(
                l.id() == CONSUMER
                    && l.epoch() == EPOCH
                    && l.execution_graph() == GRAPH
                    && l.group() == GROUP
                    && l.failed()
                    && !l.complete()
            );
        }
        balanced();
        for duplicate_root in [false, true] {
            reset();
            {
                let mut l = DeclarationCompletionLease::default();
                assert!(
                    capture(&mut l)
                        && matched(&mut l)
                        && l.mark_original_declare_returned()
                        && l.mark_original_root_returned()
                        && l.complete()
                );
                let accepted = if duplicate_root {
                    l.mark_original_root_returned()
                } else {
                    l.mark_original_declare_returned()
                };
                assert!(!accepted && l.failed() && !l.complete());
            }
            balanced();
        }
    }
}
