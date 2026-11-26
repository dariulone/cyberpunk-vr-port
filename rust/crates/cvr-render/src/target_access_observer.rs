//! THE TARGET ACCESS OBSERVER (formerly src/Render/NativeTargetAccess.cpp's anonymous namespace): the access window's
//! journal of command-list recordings and the resources they touch, its counters, and the publication of what it holds
//! into the CyberpunkVR_TargetAccess* export rows. The rules each recording follows are target_access.rs'
//! (cvr_ta_*), its events' target_access_events.rs' and the receipt's checks target_access_query.rs'.
//!
//! The counters are owned here (the C++ half's switches are references to them, guarded by its observer lock; the two
//! switches are atomics read outside it), and so is the journal: each C++ entry point takes the lock, opens the
//! publication and forwards here.
use core::sync::atomic::{AtomicBool, Ordering};

/// The observer's counters.
#[repr(C)]
pub struct Counters {
    /// The window is open; the observer gave up for the process (an unknown failure).
    pub observing: AtomicBool,
    pub unavailable: AtomicBool,
    /// The window was finished; its recordings and resources counted then.
    pub finished: bool,
    /// Failure (the first one wins).
    pub failure: u32,
    pub next_proof: u32,
    pub epoch: u64,
    pub generation: u64,
    pub rows: u64,
    pub markers: u64,
    pub resets: u64,
    pub closes: u64,
    pub source_count: u64,
    pub exits: u64,
    pub queries: u64,
    pub qualified: u64,
    pub finished_recordings: u64,
    pub finished_resources: u64,
    pub metadata_version: u64,
}
const _: () = assert!(
    core::mem::size_of::<Counters>() == 120
        && core::mem::offset_of!(Counters, failure) == 4
        && core::mem::offset_of!(Counters, epoch) == 16
        && core::mem::offset_of!(Counters, metadata_version) == 112,
    "mirrored by src/Render/NativeTargetAccess.cpp"
);
impl Counters {
    pub const fn new() -> Counters {
        Counters {
            observing: AtomicBool::new(false),
            unavailable: AtomicBool::new(false),
            finished: false,
            failure: 0,
            next_proof: 0,
            epoch: 0,
            generation: 0,
            rows: 0,
            markers: 0,
            resets: 0,
            closes: 0,
            source_count: 0,
            exits: 0,
            queries: 0,
            qualified: 0,
            finished_recordings: 0,
            finished_resources: 0,
            metadata_version: 0,
        }
    }
}
impl Default for Counters {
    fn default() -> Self {
        Self::new()
    }
}

/// The stats row (schema 3): the switches, the epoch, the recordings and resources -- as counted when the window
/// finished, else held now --, the rows, markers, resets, closes, sources, exits, queries, qualified receipts and the
/// failure.
pub fn stats_row(c: &Counters, recordings: u64, resources: u64) -> [u64; 16] {
    [
        3,
        u64::from(c.observing.load(Ordering::Relaxed)),
        u64::from(c.finished),
        c.epoch,
        if c.finished { c.finished_recordings } else { recordings },
        if c.finished { c.finished_resources } else { resources },
        c.rows,
        c.markers,
        c.resets,
        c.closes,
        c.source_count,
        c.exits,
        c.queries,
        c.qualified,
        u64::from(c.failure),
        0,
    ]
}

struct CountersCell(core::cell::UnsafeCell<Counters>);
// SAFETY: the plain fields are written and read under the C++ half's observer lock; the switches are atomics.
unsafe impl Sync for CountersCell {}
static COUNTERS: CountersCell = CountersCell(core::cell::UnsafeCell::new(Counters::new()));
/// The counters, for the C++ half's references.
#[no_mangle]
pub extern "C" fn cvr_ta_counters() -> *mut Counters {
    COUNTERS.0.get()
}

#[cfg(test)]
mod tests {
    //! The row's schema, its held and its finished counts, and the failure's place.
    use super::*;

    #[test]
    fn the_stats_row() {
        let c = Counters::default();
        assert_eq!(stats_row(&c, 4, 9), [3, 0, 0, 0, 4, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut c = Counters::default();
        c.observing.store(true, Ordering::Relaxed);
        c.epoch = 7;
        c.rows = 100;
        c.markers = 12;
        c.resets = 1;
        c.closes = 2;
        c.source_count = 3;
        c.exits = 4;
        c.queries = 5;
        c.qualified = 6;
        c.failure = 9;
        assert_eq!(stats_row(&c, 4, 9), [3, 1, 0, 7, 4, 9, 100, 12, 1, 2, 3, 4, 5, 6, 9, 0]);
        c.finished = true;
        c.finished_recordings = 40;
        c.finished_resources = 90;
        let row = stats_row(&c, 4, 9);
        assert_eq!((row[2], row[4], row[5]), (1, 40, 90), "the counts the window finished with");
    }
}

// ---- the journal (formerly the anonymous namespace's recordings and held resources), owned here

use crate::target_access::{self as ta, kind, Check, Event, Resource, Watch, NO_RESOURCE};
use windows::core::{IUnknown, Interface as _};

pub const RESOURCE_LIMIT: usize = 2048;
const E_PENDING: i32 = 0x8000_000Au32 as i32;
/// target_access::Failure, the values the journal records.
pub mod why {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const CAPACITY: u32 = 2;
    pub const MISSING_RESET: u32 = 3;
    pub const CLOSED: u32 = 4;
    pub const CLOSE_FAILED: u32 = 5;
    pub const RECORDING: u32 = 6;
    pub const SOURCE: u32 = 7;
    pub const MISSING_EXIT: u32 = 8;
    pub const FRONTIER: u32 = 9;
    pub const IDENTITY: u32 = 10;
    pub const UNKNOWN: u32 = 17;
    pub const PROOF: u32 = 18;
    pub const RESET: u32 = 20;
}

/// native_recording::Lease, holding its token.
#[derive(Default, Clone)]
pub struct OwnedLease {
    pub token: Option<IUnknown>,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
impl OwnedLease {
    pub fn token_address(&self) -> u64 {
        self.token.as_ref().map_or(0, |t| t.as_raw() as u64)
    }
    /// The lease as the native recording's C ABI reads it (the token borrowed).
    pub fn view(&self) -> crate::native_recording::Lease {
        crate::native_recording::Lease {
            token: self.token_address() as *mut core::ffi::c_void,
            generation: self.generation,
            context: self.context,
            list: self.list,
            registry: self.registry,
        }
    }
}
/// One list's recording.
pub struct Recording {
    pub list: u64,
    _list_ref: Option<IUnknown>,
    pub generation: u64,
    pub whole: bool,
    pub closed: bool,
    pub om_known: bool,
    pub close_result: i32,
    pub failure: u32,
    pub events: Vec<Event>,
    pub markers: u64,
    pub om: [u32; 9],
    pub source: OwnedLease,
    pub frame: u32,
    pub side: u32,
    pub source_row: u32,
    pub exit_row: u32,
    pub proof: u32,
}
impl Recording {
    fn new(list: u64, list_ref: Option<IUnknown>, generation: u64, whole: bool) -> Recording {
        Recording {
            list,
            _list_ref: list_ref,
            generation,
            whole,
            closed: false,
            om_known: true,
            close_result: E_PENDING,
            failure: why::NONE,
            events: Vec::new(),
            markers: 0,
            om: [NO_RESOURCE; 9],
            source: OwnedLease::default(),
            frame: 0,
            side: u32::MAX,
            source_row: u32::MAX,
            exit_row: u32::MAX,
            proof: 0,
        }
    }
}
/// A resource the journal's events name, with its creation as observed.
pub struct Held {
    pub pointer: u64,
    pub resource: Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource>,
    pub creation: Option<crate::texture_immutable::Creation>,
    pub observed: Resource,
}
/// What the journal learns of a resource it holds the first time.
pub struct HeldInfo {
    pub resource: Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource>,
    pub creation: Option<crate::texture_immutable::Creation>,
    pub observed: Resource,
}
#[derive(Default)]
pub struct Journal {
    pub recordings: Vec<Recording>,
    pub resources: Vec<Held>,
}

pub fn event_of(k: u32) -> Event {
    Event { kind: k, resource: NO_RESOURCE, other: NO_RESOURCE, subresource: u32::MAX, before: 0, after: 0, flags: 0, proof: 0, tag: 0 }
}
pub fn fail(c: &mut Counters, value: u32) {
    if c.failure == why::NONE {
        c.failure = value;
    }
}
pub fn reject(r: &mut Recording, value: u32) {
    if r.failure == why::NONE {
        r.failure = value;
    }
}
impl Journal {
    /// Find: the list's latest generation.
    pub fn find(&self, list: u64) -> Option<usize> {
        let mut latest: Option<usize> = None;
        for (i, r) in self.recordings.iter().enumerate() {
            if r.list == list && latest.is_none_or(|l| r.generation > self.recordings[l].generation) {
                latest = Some(i);
            }
        }
        latest
    }
    fn slots(&self) -> Vec<ta::RecordingSlot> {
        self.recordings
            .iter()
            .map(|r| ta::RecordingSlot {
                list: r.list,
                generation: r.generation,
                rows: r.events.len() as u64,
                markers: r.markers,
                source: r.source.token.is_some(),
                closed: r.closed,
            })
            .collect()
    }
    /// StartRecording: the plan's slot (a reset reuses the list's latest non-source one, a new one while there is room,
    /// else the oldest closed non-source one), a fresh recording there.
    pub fn start_recording(&mut self, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, whole: bool) -> Option<usize> {
        let plan = ta::plan_recording(&self.slots(), list, c.generation, c.rows, c.markers);
        if plan.failure != why::NONE {
            fail(c, plan.failure);
            return None;
        }
        let index = plan.index as usize;
        let next = Recording::new(list, list_ref(), plan.generation, whole);
        if index == self.recordings.len() {
            self.recordings.push(next);
        } else {
            self.recordings[index] = next;
        }
        c.generation = plan.generation;
        c.rows = plan.rows;
        c.markers = plan.markers;
        Some(index)
    }
    /// Get: the list's recording, started (not whole) when there is none.
    pub fn get(&mut self, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>) -> Option<usize> {
        if list == 0 {
            fail(c, why::ARGUMENTS);
            return None;
        }
        self.find(list).or_else(|| self.start_recording(c, list, list_ref, false))
    }
    /// Append: one event (a closed recording refuses), discarding the oldest closed non-source recordings' events
    /// while the rows or the markers press.
    pub fn append(&mut self, c: &mut Counters, index: usize, event: Event) {
        if self.recordings[index].closed {
            reject(&mut self.recordings[index], why::CLOSED);
            return;
        }
        let marker = crate::target_access_events::is_marker(event.kind);
        while c.rows == ta::ACCESS_LIMIT || (marker && c.markers == ta::MARKER_LIMIT) {
            let plan = ta::plan_pressure(&self.slots(), index as u32, c.rows, c.markers, marker);
            if plan.failure != why::NONE {
                fail(c, plan.failure);
                return;
            }
            if plan.index == u32::MAX {
                fail(c, why::RECORDING);
                return;
            }
            let discarded = &mut self.recordings[plan.index as usize];
            discarded.events = Vec::new();
            discarded.markers = 0;
            discarded.whole = false;
            c.rows = plan.rows;
            c.markers = plan.markers;
        }
        let r = &mut self.recordings[index];
        r.events.push(event);
        c.rows += 1;
        if marker {
            c.markers += 1;
            r.markers += 1;
        }
    }
    /// Hold: the resource's index, held (with its creation as observed) the first time; none for null or past the limit.
    pub fn hold(&mut self, c: &mut Counters, resource: u64, info: &dyn Fn(u64) -> HeldInfo) -> u32 {
        if resource == 0 {
            return NO_RESOURCE;
        }
        if let Some(i) = self.resources.iter().position(|h| h.pointer == resource) {
            return i as u32;
        }
        if self.resources.len() == RESOURCE_LIMIT {
            fail(c, why::CAPACITY);
            return NO_RESOURCE;
        }
        let HeldInfo { resource: held, creation, observed } = info(resource);
        self.resources.push(Held { pointer: resource, resource: held, creation, observed });
        (self.resources.len() - 1) as u32
    }
    /// HoldBreadcrumb: the breadcrumb destination's index by its authenticated creation (a known allocation; a cached
    /// resource's creation upgraded, a conflicting one refused -- counted in the breadcrumb stats' [4]).
    pub fn hold_breadcrumb(&mut self, c: &mut Counters, stats: &mut [u64; 8], observed: Resource, info: &dyn Fn() -> HeldInfo) -> u32 {
        if observed.pointer == 0 || !crate::texture_read::known_allocation(&observed.allocation) {
            stats[4] += 1;
            return NO_RESOURCE;
        }
        for (i, cached) in self.resources.iter_mut().enumerate() {
            if cached.pointer != observed.pointer {
                continue;
            }
            let matched = ta::compare_authenticated_creation(&cached.observed, &observed);
            if matched == ta::creation::CONFLICT {
                stats[4] += 1;
                return NO_RESOURCE;
            }
            if matched == ta::creation::UPGRADE {
                cached.creation = info().creation;
                cached.observed = observed;
                c.metadata_version += 1;
            }
            return i as u32;
        }
        if self.resources.len() == RESOURCE_LIMIT {
            fail(c, why::CAPACITY);
            return NO_RESOURCE;
        }
        let HeldInfo { resource, creation, .. } = info();
        self.resources.push(Held { pointer: observed.pointer, resource, creation, observed });
        (self.resources.len() - 1) as u32
    }
}

// ---- the entry points' locked bodies (the C++ half holds the observer lock and has opened the publication)

/// Observe: an access to `resource` (null refuses the recording).
pub fn observe(j: &mut Journal, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, resource: u64, k: u32, tag: u32, subresource: u32, info: &dyn Fn(u64) -> HeldInfo) {
    if let Some(i) = j.get(c, list, list_ref) {
        let mut event = event_of(k);
        event.resource = j.hold(c, resource, info);
        event.tag = tag;
        event.subresource = subresource;
        if resource == 0 {
            reject(&mut j.recordings[i], why::ARGUMENTS);
        }
        j.append(c, i, event);
    }
}
/// ObserveUnknown: a command the window cannot account for.
pub fn observe_unknown(j: &mut Journal, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, tag: u32) {
    if let Some(i) = j.get(c, list, list_ref) {
        let mut event = event_of(kind::UNKNOWN);
        event.tag = tag;
        j.append(c, i, event);
    }
}
/// EndProof: the proof scope's end on its own recording generation (an incomplete one refuses).
pub fn end_proof(j: &mut Journal, c: &mut Counters, list: u64, serial: u64, proof: u32, complete: bool) {
    let Some(i) = j.find(list).filter(|&i| j.recordings[i].generation == serial && j.recordings[i].proof == proof) else {
        fail(c, why::PROOF);
        return;
    };
    let mut event = event_of(kind::PROOF_END);
    event.proof = proof;
    event.flags = u32::from(complete);
    j.append(c, i, event);
    j.recordings[i].proof = 0;
    if !complete {
        reject(&mut j.recordings[i], why::PROOF);
    }
}
/// Reset: counted; the list's source recording refused; a fresh whole recording.
pub fn reset(j: &mut Journal, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>) {
    c.resets += 1;
    if list == 0 {
        fail(c, why::ARGUMENTS);
        return;
    }
    if let Some(old) = j.find(list) {
        if j.recordings[old].source.token.is_some() {
            reject(&mut j.recordings[old], why::RESET);
        }
    }
    j.start_recording(c, list, list_ref, true);
}
/// Close: counted; a second close, a failed one or an open proof refuses.
pub fn close(j: &mut Journal, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, result: i32) {
    if let Some(i) = j.get(c, list, list_ref) {
        c.closes += 1;
        let r = &mut j.recordings[i];
        if r.closed {
            reject(r, why::CLOSED);
        }
        r.closed = true;
        r.close_result = result;
        if result < 0 {
            reject(r, why::CLOSE_FAILED);
        }
        if r.proof != 0 {
            reject(r, why::PROOF);
        }
    }
}
/// Shader: a shader event; a graphics one also writes the bound targets (unknown when they could not be read).
pub fn shader(j: &mut Journal, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, graphics: bool) {
    if let Some(i) = j.get(c, list, list_ref) {
        let mut event = event_of(kind::SHADER);
        j.append(c, i, event);
        if graphics {
            if !j.recordings[i].om_known {
                event.kind = kind::UNKNOWN;
                j.append(c, i, event);
            } else {
                let om = j.recordings[i].om;
                for index in om.into_iter().filter(|&x| x != NO_RESOURCE) {
                    event.kind = kind::WRITE;
                    event.resource = index;
                    j.append(c, i, event);
                }
            }
        }
    }
}
/// MarkSource: the recording becomes its side's source (closed, bad arguments, a held source or the side already
/// taken refuse); its source row and the source event.
#[allow(clippy::too_many_arguments)]
pub fn mark_source(j: &mut Journal, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, lease: OwnedLease, frame: u32, side: u32) -> bool {
    let Some(i) = j.get(c, list, list_ref) else { return false };
    if j.recordings[i].closed {
        reject(&mut j.recordings[i], why::CLOSED);
        return false;
    }
    let held_token = j.recordings[i].source.token_address();
    if !crate::target_access_events::source_arguments(side, frame, lease.token_address(), lease.list, list, lease.generation, held_token) {
        reject(&mut j.recordings[i], why::SOURCE);
        return false;
    }
    if j.recordings.iter().any(|old| old.source.token.is_some() && old.side == side) {
        reject(&mut j.recordings[i], why::SOURCE);
        return false;
    }
    let r = &mut j.recordings[i];
    r.source = lease;
    r.frame = frame;
    r.side = side;
    r.source_row = r.events.len() as u32;
    j.append(c, i, event_of(kind::SOURCE));
    c.source_count += 1;
    j.recordings[i].failure == why::NONE
}
/// MarkExit: a source recording's exit row and event, once.
pub fn mark_exit(j: &mut Journal, c: &mut Counters, list: u64) {
    let Some(i) = j.find(list) else { return };
    if j.recordings[i].source.token.is_none() || j.recordings[i].exit_row != u32::MAX {
        return;
    }
    j.recordings[i].exit_row = j.recordings[i].events.len() as u32;
    j.append(c, i, event_of(kind::EXIT));
    c.exits += 1;
}
/// DiagnosticProofScope: on an exited source recording without a proof, exactly four distinct resources begin a new
/// proof; its number and the recording's generation.
pub fn proof_begin(j: &mut Journal, c: &mut Counters, list: u64, held: &[*mut core::ffi::c_void], info: &dyn Fn(u64) -> HeldInfo) -> Option<(u32, u64)> {
    let i = j.find(list)?;
    let r = &j.recordings[i];
    if r.source.token.is_none() || r.exit_row == u32::MAX || r.proof != 0 || c.next_proof == u32::MAX {
        reject(&mut j.recordings[i], why::PROOF);
        return None;
    }
    let Some(unique) = crate::target_access_events::proof_resources(held) else {
        reject(&mut j.recordings[i], why::PROOF);
        return None;
    };
    c.next_proof += 1;
    let proof = c.next_proof;
    let generation = j.recordings[i].generation;
    j.recordings[i].proof = proof;
    for resource in unique {
        let mut event = event_of(kind::PROOF_BEGIN);
        event.resource = j.hold(c, resource as u64, info);
        event.proof = proof;
        j.append(c, i, event);
    }
    Some((proof, generation))
}
/// ReceiptStatusLocked: the switches, the failure, the key's epoch, metadata version, queue, thread, scope and frame,
/// and each side's recording as found by its source list (a side without one keeps the defaults).
pub fn receipt_status(j: &Journal, c: &Counters, queue: u64, thread: u64, scope: u64, frame: u32, lists: [u64; 2]) -> ta::ReceiptStatus {
    let mut status = ta::ReceiptStatus::default();
    status.observing = c.observing.load(Ordering::Relaxed);
    status.finished = c.finished;
    status.unavailable = c.unavailable.load(Ordering::Relaxed);
    status.failure = c.failure;
    status.key.epoch = c.epoch;
    status.key.metadata_version = c.metadata_version;
    status.key.queue = queue;
    status.key.thread = thread;
    status.key.scope = scope;
    status.key.frame = frame;
    for side in 0..2 {
        let saved = &mut status.key.sources[side];
        saved.close_result = E_PENDING;
        let Some(i) = j.find(lists[side]) else { continue };
        let r = &j.recordings[i];
        *saved = ta::ReceiptRecord {
            list: r.list,
            generation: r.generation,
            native_generation: r.source.generation,
            token: r.source.token_address(),
            context: r.source.context,
            registry: r.source.registry,
            rows: r.events.len() as u64,
            markers: r.markers,
            frame: r.frame,
            side: r.side,
            source_row: r.source_row,
            exit_row: r.exit_row,
            proof: r.proof,
            close_result: r.close_result,
            failure: r.failure,
            whole: r.whole,
            closed: r.closed,
            om_known: r.om_known,
            om: r.om,
        };
    }
    status
}
/// ReportFailureEvent: the first failing row's event, its resources' pointers.
pub fn failure_event(j: &Journal, r: &Recording, check: &Check) -> [u64; 8] {
    let mut data = [u64::from(r.side), r.list, r.generation, u64::from(check.failure_row), u64::MAX, 0, 0, 0];
    if let Some(event) = r.events.get(check.failure_row as usize) {
        data[4] = u64::from(event.kind);
        data[5] = u64::from(event.tag);
        if let Some(h) = j.resources.get(event.resource as usize) {
            data[6] = h.observed.pointer;
        }
        if let Some(h) = j.resources.get(event.other as usize) {
            data[7] = h.observed.pointer;
        }
    }
    data
}

/// One side of the query as the C++ half's snapshot gives it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueryTarget {
    pub observed: Resource,
    pub planes: u32,
    pub expected: u32,
    pub finals: [u32; 2],
    pub token: u64,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct QuerySource {
    pub token: u64,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
    pub frame: u32,
    pub side: u32,
    pub targets: [QueryTarget; 4],
}
const _: () = assert!(
    core::mem::size_of::<QueryTarget>() == 104 && core::mem::size_of::<QuerySource>() == 464 && core::mem::offset_of!(QuerySource, targets) == 48,
    "mirrored by src/Render/NativeTargetAccess.cpp"
);
fn same_lease(token: u64, generation: u64, context: u64, list: u64, registry: u64, b: &OwnedLease) -> bool {
    token != 0 && token == b.token_address() && generation == b.generation && context == b.context && list == b.list && registry == b.registry
}
/// The query's outputs: the two source rows, the check rows and the failure event (each written when the C++ half's
/// did).
pub struct QueryRows<'a> {
    pub sources: &'a mut [u64; 32],
    pub checks: &'a mut [u64; 32],
    pub failure_event: &'a mut [u64; 8],
}
/// Query's inner check: counted; each side's recording found and written to its row; the frontier, each side's whole
/// closed source recording on this frame and side with its exit and no open proof, its submission and its targets'
/// identities validated on the queue (`validate_submission`, `validate_identity` at the C++ half's points), the two
/// windows checked against their final states, then the later side's window against the earlier's; qualified.
#[allow(clippy::too_many_arguments)]
pub fn query(
    j: &Journal,
    c: &mut Counters,
    queue: u64,
    sources: &[QuerySource; 2],
    boundary: &ta::Boundary,
    rows: QueryRows<'_>,
    validate_submission: &mut dyn FnMut(&OwnedLease) -> bool,
    validate_identity: &mut dyn FnMut(usize, usize) -> bool,
) -> bool {
    c.queries += 1;
    rows.sources.fill(0);
    rows.checks.fill(0);
    let refuse = |c: &mut Counters, value: u32| {
        fail(c, value);
        false
    };
    let mut selected = [None::<usize>; 2];
    for side in 0..2 {
        let s = &sources[side];
        let found = j.find(s.list);
        selected[side] = found;
        let mut record = found;
        if let Some(k) = j.recordings.iter().position(|cand| same_lease(s.token, s.generation, s.context, s.list, s.registry, &cand.source)) {
            record = Some(k);
        }
        let r = record.map(|k| &j.recordings[k]);
        let values: [u64; 16] = [
            u64::from(s.frame),
            side as u64,
            s.list,
            r.map_or(0, |r| r.generation),
            s.generation,
            s.token,
            u64::from(r.is_some_and(|r| r.whole)),
            u64::from(r.is_some_and(|r| r.closed)),
            u64::from(r.map_or(E_PENDING, |r| r.close_result) as u32),
            0,
            r.map_or(0, |r| r.events.len() as u64),
            u64::from(r.map_or(u32::MAX, |r| r.source_row)),
            u64::from(r.map_or(u32::MAX, |r| r.exit_row)),
            u64::from(r.map_or(why::MISSING_RESET, |r| r.failure)),
            u64::from(boundary.source_indices[side]),
            0,
        ];
        rows.sources[side * 16..side * 16 + 16].copy_from_slice(&values);
    }
    if c.unavailable.load(Ordering::Relaxed) {
        c.failure = why::UNKNOWN;
        return false;
    }
    if c.failure != why::NONE {
        return false;
    }
    if !crate::target_access_query::frontier(boundary, queue) {
        return refuse(c, why::FRONTIER);
    }
    let mut watched = [[Watch::default(); 4]; 2];
    for side in 0..2 {
        let s = &sources[side];
        let Some(i) = selected[side] else { return refuse(c, why::MISSING_RESET) };
        let r = &j.recordings[i];
        if !r.whole {
            return refuse(c, why::MISSING_RESET);
        }
        if r.failure != why::NONE {
            return refuse(c, r.failure);
        }
        if !r.closed || r.close_result < 0 {
            return refuse(c, why::CLOSE_FAILED);
        }
        if s.side != side as u32
            || s.frame == 0
            || s.frame != sources[1 - side].frame
            || s.frame != r.frame
            || r.side != side as u32
            || !same_lease(s.token, s.generation, s.context, s.list, s.registry, &r.source)
            || !validate_submission(&r.source)
        {
            return refuse(c, why::RECORDING);
        }
        if r.source_row == u32::MAX || r.exit_row == u32::MAX || r.proof != 0 {
            return refuse(c, why::MISSING_EXIT);
        }
        for t in 0..4 {
            let target = &s.targets[t];
            if !validate_identity(side, t)
                || !(s.token != 0
                    && s.token == target.token
                    && s.generation == target.generation
                    && s.context == target.context
                    && s.list == target.list
                    && s.registry == target.registry)
                || target.planes == 0
                || target.planes > 2
            {
                return refuse(c, why::IDENTITY);
            }
            let watch = &mut watched[side][t];
            watch.resource = target.observed;
            watch.count = target.planes;
            for p in 0..target.planes as usize {
                watch.states[p] = target.expected;
            }
        }
    }
    let (Some(first), Some(second)) = (selected[0], selected[1]) else { return refuse(c, why::RECORDING) };
    if first == second {
        return refuse(c, why::RECORDING);
    }
    let metadata: Vec<Resource> = j.resources.iter().map(|h| h.observed).collect();
    let finals = |side: usize| -> [[u32; 2]; 4] { core::array::from_fn(|t| sources[side].targets[t].finals) };
    let mut checks = [Check::default(); 2];
    for side in 0..2 {
        let r = &j.recordings[selected[side].unwrap_or(0)];
        let mut success = ta::check_window(&r.events, &metadata, &watched[side], true, &mut checks[side]);
        success = crate::target_access_query::final_states(&mut checks[side], &watched[side], &finals(side), r.events.len() as u32, success);
        rows.checks[side * 16..side * 16 + 16].copy_from_slice(&crate::target_access_query::row(side as u32, &checks[side], success));
        if !success {
            if rows.failure_event[1] == 0 {
                *rows.failure_event = failure_event(j, r, &checks[side]);
            }
            return refuse(c, checks[side].failure);
        }
    }
    let earlier = if boundary.source_indices[0] < boundary.source_indices[1] { 0 } else { 1 };
    let later = 1 - earlier;
    crate::target_access_query::seed(&mut watched[earlier], &finals(earlier));
    let mut cross = Check::default();
    let later_r = &j.recordings[selected[later].unwrap_or(0)];
    let success = ta::check_window(&later_r.events, &metadata, &watched[earlier], false, &mut cross);
    crate::target_access_query::merge(&mut checks[earlier], &cross, success);
    rows.checks[earlier * 16..earlier * 16 + 16].copy_from_slice(&crate::target_access_query::row(earlier as u32, &checks[earlier], success));
    if !success {
        if rows.failure_event[1] == 0 {
            *rows.failure_event = failure_event(j, later_r, &cross);
        }
        return refuse(c, cross.failure);
    }
    for side in 0..2 {
        rows.sources[side * 16 + 15] = 1;
    }
    c.qualified += 1;
    true
}

// ---- the journal's C ABI (src/Render/NativeTargetAccess.cpp's entry points, under its observer lock)

#[cfg(not(test))]
struct JournalCell(core::cell::UnsafeCell<Journal>);
// SAFETY: read and written only under the C++ half's observer lock.
#[cfg(not(test))]
unsafe impl Sync for JournalCell {}
#[cfg(not(test))]
static JOURNAL: JournalCell = JournalCell(core::cell::UnsafeCell::new(Journal { recordings: Vec::new(), resources: Vec::new() }));

#[cfg(not(test))]
pub(crate) mod abi {
    use super::*;
    use core::ffi::c_void;
    use windows::Win32::Graphics::Direct3D12::{ID3D12Heap, ID3D12Resource, D3D12_RESOURCE_BARRIER};

    #[allow(non_upper_case_globals)]
    extern "C" {
        static mut CyberpunkVR_TargetAccessStats: [u64; 16];
        static mut CyberpunkVR_TargetAccessSources: [u64; 32];
        static mut CyberpunkVR_TargetAccessChecks: [u64; 32];
        static mut CyberpunkVR_TargetAccessFailureEvent: [u64; 8];
        static mut CyberpunkVR_TargetAccessBreadcrumbStats: [u64; 8];
    }
    /// The journal and the counters (the caller holds the observer lock).
    fn state() -> (&'static mut Journal, &'static mut Counters) {
        // SAFETY: only under the C++ half's observer lock.
        unsafe { (&mut *JOURNAL.0.get(), &mut *COUNTERS.0.get()) }
    }
    fn breadcrumb_stats() -> &'static mut [u64; 8] {
        // SAFETY: as state().
        unsafe { &mut *(&raw mut CyberpunkVR_TargetAccessBreadcrumbStats) }
    }
    fn list_ref(list: u64) -> impl Fn() -> Option<IUnknown> {
        move || {
            let raw = list as *mut c_void;
            // SAFETY: the caller's live command list; a reference taken for the recording.
            unsafe { IUnknown::from_raw_borrowed(&raw).cloned() }
        }
    }
    /// What the journal keeps of a resource it holds: a reference, its creation (registered, else as the receipts
    /// observed its allocation) and its description's flags.
    fn plugin_info(resource: u64) -> HeldInfo {
        let raw = resource as *mut c_void;
        // SAFETY: the caller's live resource.
        let held = unsafe { ID3D12Resource::from_raw_borrowed(&raw).cloned() };
        let mut creation = crate::texture_immutable::query_creation(raw);
        if creation.is_none() {
            let (mut kind, mut heap, mut offset, mut bytes) = (0u32, core::ptr::null_mut(), 0u64, 0u64);
            // SAFETY: the resource and the outputs; the heap comes with its reference.
            if unsafe { crate::texture_receipts::cvr_tr_query_observed_allocation(raw, &mut kind, &mut heap, &mut offset, &mut bytes) } {
                creation = Some(crate::texture_immutable::Creation {
                    kind,
                    resource: held.clone(),
                    // SAFETY: the reference handed over.
                    heap: (!heap.is_null()).then(|| unsafe { ID3D12Heap::from_raw(heap) }),
                    offset,
                    bytes,
                    ..Default::default()
                });
            }
        }
        let (heap, offset, bytes, kind) = creation.as_ref().map_or((0, 0, 0, 0), |c| {
            (c.heap.as_ref().map_or(0, |h| h.as_raw() as u64), c.offset, c.bytes, c.kind)
        });
        // SAFETY: the live resource's description.
        let flags = held.as_ref().map_or(0, |r| unsafe { r.GetDesc() }.Flags.0 as u32);
        HeldInfo {
            resource: held,
            creation,
            observed: Resource { pointer: resource, allocation: crate::texture_read::Allocation { heap, offset, bytes, kind }, flags },
        }
    }

    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_publish_stats() {
        let (j, c) = state();
        // SAFETY: the stats row, written under the lock.
        unsafe { *(&raw mut CyberpunkVR_TargetAccessStats) = stats_row(c, j.recordings.len() as u64, j.resources.len() as u64) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_observe(list: u64, resource: u64, k: u32, tag: u32, subresource: u32) {
        let (j, c) = state();
        observe(j, c, list, &list_ref(list), resource, k, tag, subresource, &plugin_info);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_observe_unknown(list: u64, tag: u32) {
        let (j, c) = state();
        observe_unknown(j, c, list, &list_ref(list), tag);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_end_proof(list: u64, serial: u64, proof: u32, complete: bool) {
        let (j, c) = state();
        end_proof(j, c, list, serial, proof, complete);
    }
    /// # Safety
    /// `out` is a ReceiptStatus.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta_receipt_status(queue: u64, thread: u64, scope: u64, frame: u32, list0: u64, list1: u64, out: *mut ta::ReceiptStatus) {
        let (j, c) = state();
        // SAFETY: the caller's output.
        unsafe { *out = receipt_status(j, c, queue, thread, scope, frame, [list0, list1]) };
    }
    /// Clear: the window closed and emptied, its counts and rows zeroed (the epoch and generation kept); an unavailable
    /// observer stays failed.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_clear() {
        let (j, c) = state();
        c.observing.store(false, Ordering::Release);
        j.recordings.clear();
        j.resources.clear();
        c.rows = 0;
        c.markers = 0;
        c.resets = 0;
        c.closes = 0;
        c.source_count = 0;
        c.exits = 0;
        c.queries = 0;
        c.qualified = 0;
        c.failure = if c.unavailable.load(Ordering::Relaxed) { why::UNKNOWN } else { why::NONE };
        c.finished = false;
        c.finished_recordings = 0;
        c.finished_resources = 0;
        c.metadata_version = 0;
        c.next_proof = 0;
        // SAFETY: the rows, under the lock.
        unsafe {
            (*(&raw mut CyberpunkVR_TargetAccessSources)).fill(0);
            (*(&raw mut CyberpunkVR_TargetAccessChecks)).fill(0);
            (*(&raw mut CyberpunkVR_TargetAccessFailureEvent)).fill(0);
            (*(&raw mut CyberpunkVR_TargetAccessBreadcrumbStats)).fill(0);
        }
    }
    /// Arm: a new epoch's window opens (an unavailable observer fails; an open, finished or non-empty one refuses).
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_arm() {
        let (j, c) = state();
        if c.unavailable.load(Ordering::Relaxed) {
            c.failure = why::UNKNOWN;
            return;
        }
        if c.observing.load(Ordering::Relaxed) || c.finished || !j.recordings.is_empty() || c.epoch == u64::MAX {
            fail(c, why::ARGUMENTS);
            return;
        }
        j.recordings.reserve(ta::RECORDING_LIMIT);
        j.resources.reserve(RESOURCE_LIMIT);
        c.epoch += 1;
        c.observing.store(true, Ordering::Release);
    }
    /// Finish: the window closes, its recordings and resources counted and dropped.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_finish() {
        let (j, c) = state();
        c.observing.store(false, Ordering::Release);
        c.finished_recordings = j.recordings.len() as u64;
        c.finished_resources = j.resources.len() as u64;
        c.finished = true;
        j.recordings.clear();
        j.resources.clear();
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_reset(list: u64) {
        let (j, c) = state();
        reset(j, c, list, &list_ref(list));
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_close(list: u64, result: i32) {
        let (j, c) = state();
        close(j, c, list, &list_ref(list), result);
    }
    /// ImmediateWrite's first part: the list's recording counted, its epoch and generation captured.
    ///
    /// # Safety
    /// The outputs are valid.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta_immediate_begin(list: u64, epoch: *mut u64, generation: *mut u64) -> bool {
        let (j, c) = state();
        let Some(i) = j.get(c, list, &list_ref(list)) else { return false };
        breadcrumb_stats()[0] += 1;
        // SAFETY: the caller's outputs.
        unsafe {
            *epoch = c.epoch;
            *generation = j.recordings[i].generation;
        }
        true
    }
    /// ImmediateWrite's second part, after the resolver: the same recording generation, the resolver's outcome, the
    /// destination written when its creation is authenticated (else an unknown write).
    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_immediate_end(
        list: u64,
        generation: u64,
        resolved: bool,
        resolver_failure: u32,
        destination: u64,
        resource: u64,
        heap: u64,
        offset: u64,
        bytes: u64,
        kind_of: u32,
        flags: u32,
    ) -> bool {
        let (j, c) = state();
        let stats = breadcrumb_stats();
        let Some(i) = j.find(list).filter(|&i| j.recordings[i].generation == generation) else {
            stats[3] += 1;
            fail(c, why::RECORDING);
            return false;
        };
        stats[5] = u64::from(resolver_failure);
        stats[6] = destination;
        stats[7] = resource;
        let mut event = event_of(kind::UNKNOWN);
        event.tag = 13;
        if resolved {
            let observed = Resource { pointer: resource, allocation: crate::texture_read::Allocation { heap, offset, bytes, kind: kind_of }, flags };
            let info = || {
                let (r, h) = (resource as *mut c_void, heap as *mut c_void);
                // SAFETY: the snapshot's live resource and heap; references taken for the journal.
                let (r, h) = unsafe { (ID3D12Resource::from_raw_borrowed(&r).cloned(), ID3D12Heap::from_raw_borrowed(&h).cloned()) };
                HeldInfo {
                    resource: r.clone(),
                    creation: Some(crate::texture_immutable::Creation { kind: kind_of, resource: r, heap: h, offset, bytes, ..Default::default() }),
                    observed,
                }
            };
            event.resource = j.hold_breadcrumb(c, stats, observed, &info);
            if event.resource != NO_RESOURCE {
                event.kind = kind::WRITE;
                stats[1] += 1;
            }
        }
        if event.kind == kind::UNKNOWN {
            stats[2] += 1;
        }
        j.append(c, i, event);
        event.kind == kind::WRITE
    }
    /// # Safety
    /// `barriers` holds `count` barriers or is null.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta_barriers(list: u64, count: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
        let (j, c) = state();
        let Some(i) = j.get(c, list, &list_ref(list)) else { return };
        if count != 0 && barriers.is_null() {
            reject(&mut j.recordings[i], why::ARGUMENTS);
            return;
        }
        for n in 0..count as usize {
            let mut event = event_of(kind::READ);
            let mut held = [core::ptr::null_mut(); 2];
            // SAFETY: the caller's barrier; the event and held pointers ours.
            let holds = unsafe { crate::target_access_events::cvr_tae_barrier(barriers.add(n), &mut event, &mut held) };
            if holds > 0 {
                event.resource = j.hold(c, held[0] as u64, &plugin_info);
            }
            if holds > 1 {
                event.other = j.hold(c, held[1] as u64, &plugin_info);
            }
            j.append(c, i, event);
        }
    }
    /// Targets' first part: the list's recording, its bound targets forgotten before the capture.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_targets_begin(list: u64) -> bool {
        let (j, c) = state();
        let Some(i) = j.get(c, list, &list_ref(list)) else { return false };
        j.recordings[i].om = [NO_RESOURCE; 9];
        true
    }
    /// Targets' second part: the capture's outcome, its resources held.
    ///
    /// # Safety
    /// `resources` holds nine resources or nulls.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta_targets_end(list: u64, known: bool, resources: *const *mut c_void) {
        let (j, c) = state();
        let Some(i) = j.find(list) else { return };
        j.recordings[i].om_known = known;
        if known {
            for n in 0..9 {
                // SAFETY: the caller's nine.
                let resource = unsafe { *resources.add(n) } as u64;
                let index = j.hold(c, resource, &plugin_info);
                j.recordings[i].om[n] = index;
            }
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_shader(list: u64, graphics: bool) {
        let (j, c) = state();
        shader(j, c, list, &list_ref(list), graphics);
    }
    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_mark_source(list: u64, token: u64, generation: u64, context: u64, lease_list: u64, registry: u64, frame: u32, side: u32) -> bool {
        let (j, c) = state();
        let raw = token as *mut c_void;
        // SAFETY: the lease's live token; a reference taken for the recording.
        let held = unsafe { IUnknown::from_raw_borrowed(&raw).cloned() };
        let lease = OwnedLease { token: held, generation, context, list: lease_list, registry };
        mark_source(j, c, list, &list_ref(list), lease, frame, side)
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_mark_exit(list: u64) {
        let (j, c) = state();
        mark_exit(j, c, list);
    }
    /// # Safety
    /// `held` holds `count` resources or nulls; the outputs are valid.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta_proof_begin(list: u64, held: *const *mut c_void, count: usize, proof: *mut u32, generation: *mut u64) -> bool {
        let (j, c) = state();
        // SAFETY: the caller's span.
        let held = if held.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(held, count) } };
        match proof_begin(j, c, list, held, &plugin_info) {
            Some((p, g)) => {
                // SAFETY: the caller's outputs.
                unsafe {
                    *proof = p;
                    *generation = g;
                }
                true
            }
            None => false,
        }
    }
    /// Query's inner check; `validate_identity` the C++ half's identity check of one side's target on the queue.
    ///
    /// # Safety
    /// `sources` holds two, `boundary` is valid; `queue` live.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta_query(
        queue: *mut c_void,
        sources: *const [QuerySource; 2],
        boundary: *const ta::Boundary,
        validate_identity: unsafe extern "C-unwind" fn(ctx: *mut c_void, side: u32, target: u32) -> bool,
        ctx: *mut c_void,
    ) -> bool {
        let (j, c) = state();
        // SAFETY: the caller's inputs and the rows, under the lock.
        unsafe {
            let rows = QueryRows {
                sources: &mut *(&raw mut CyberpunkVR_TargetAccessSources),
                checks: &mut *(&raw mut CyberpunkVR_TargetAccessChecks),
                failure_event: &mut *(&raw mut CyberpunkVR_TargetAccessFailureEvent),
            };
            let mut submission = |lease: &OwnedLease| {
                let view = lease.view();
                crate::native_recording::cvr_native_recording_validate_submission(&view, queue)
            };
            let mut identity = |side: usize, t: usize| validate_identity(ctx, side as u32, t as u32);
            query(j, c, queue as u64, &*sources, &*boundary, rows, &mut submission, &mut identity)
        }
    }
    /// The journal's emptiness, for the C++ half's Arm check order (unused when Arm runs here).
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_journal_sizes(recordings: *mut u64, resources: *mut u64) {
        let (j, _) = state();
        // SAFETY: the caller's outputs.
        unsafe {
            *recordings = j.recordings.len() as u64;
            *resources = j.resources.len() as u64;
        }
    }
}

#[cfg(test)]
mod journal_tests {
    //! The journal: recordings found, started, reset; events appended under pressure; resources held; a source's life
    //! with its exit and proof; the shader's writes; the receipt status's defaults; the failure event.
    use super::*;

    const LIST: u64 = 0x1000;
    fn none() -> Option<IUnknown> {
        None
    }
    fn info(resource: u64) -> HeldInfo {
        HeldInfo { resource: None, creation: None, observed: Resource { pointer: resource, ..Default::default() } }
    }

    #[test]
    fn recordings_found_started_and_reset() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        let i = j.get(&mut c, LIST, &none).unwrap();
        let first = j.recordings[i].generation;
        assert!(!j.recordings[i].whole && j.recordings[i].om_known && j.recordings[i].close_result == E_PENDING);
        assert_eq!(j.get(&mut c, LIST, &none), Some(i), "found again");
        assert!(j.get(&mut c, 0, &none).is_none());
        assert_eq!(c.failure, why::ARGUMENTS);
        reset(&mut j, &mut c, LIST, &none);
        let latest = j.find(LIST).unwrap();
        assert!(j.recordings[latest].whole && j.recordings[latest].generation > first);
        assert_eq!(c.resets, 1);
    }

    #[test]
    fn events_append_counting_markers_and_refusing_a_closed_recording() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        let i = j.get(&mut c, LIST, &none).unwrap();
        observe(&mut j, &mut c, LIST, &none, 0xA0, kind::READ, 3, 0, &info);
        observe_unknown(&mut j, &mut c, LIST, &none, 9);
        assert_eq!((c.rows, c.markers, j.recordings[i].markers), (2, 1, 1));
        assert_eq!((j.recordings[i].events[0].resource, j.recordings[i].events[1].tag), (0, 9));
        observe(&mut j, &mut c, LIST, &none, 0, kind::WRITE, 0, 0, &info);
        assert_eq!(j.recordings[i].failure, why::ARGUMENTS, "a null resource refuses");
        close(&mut j, &mut c, LIST, &none, 0);
        observe_unknown(&mut j, &mut c, LIST, &none, 1);
        assert_eq!(j.recordings[i].events.len(), 3, "a closed recording takes no more");
    }

    #[test]
    fn pressure_discards_a_closed_recordings_events() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        let old = j.get(&mut c, 0x2000, &none).unwrap();
        observe_unknown(&mut j, &mut c, 0x2000, &none, 1);
        close(&mut j, &mut c, 0x2000, &none, 0);
        let i = j.get(&mut c, LIST, &none).unwrap();
        c.rows = ta::ACCESS_LIMIT;
        observe_unknown(&mut j, &mut c, LIST, &none, 2);
        assert!(j.recordings[old].events.is_empty() && !j.recordings[old].whole);
        assert_eq!(j.recordings[i].events.len(), 1);
    }

    #[test]
    fn resources_held_once_up_to_the_limit() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        assert_eq!(j.hold(&mut c, 0xB0, &info), 0);
        assert_eq!(j.hold(&mut c, 0xB0, &info), 0);
        assert_eq!(j.hold(&mut c, 0, &info), NO_RESOURCE);
        for n in 1..RESOURCE_LIMIT as u64 {
            j.hold(&mut c, 0xB0 + n * 8, &info);
        }
        assert_eq!(j.hold(&mut c, 0xFFFF_0000, &info), NO_RESOURCE);
        assert_eq!(c.failure, why::CAPACITY);
    }

    #[test]
    fn a_sources_life_exit_and_proof() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        let token: IUnknown = unsafe { core::mem::transmute::<usize, IUnknown>(0x7000) };
        let lease = OwnedLease { token: Some(token), generation: 5, context: 1, list: LIST, registry: 2 };
        assert!(mark_source(&mut j, &mut c, LIST, &none, lease, 40, 1));
        let i = j.find(LIST).unwrap();
        assert_eq!((j.recordings[i].side, j.recordings[i].source_row, c.source_count), (1, 0, 1));
        assert!(proof_begin(&mut j, &mut c, LIST, &[], &info).is_none(), "no exit yet");
        let i = j.find(LIST).unwrap();
        j.recordings[i].failure = why::NONE;
        mark_exit(&mut j, &mut c, LIST);
        mark_exit(&mut j, &mut c, LIST);
        assert_eq!((j.recordings[i].exit_row, c.exits), (1, 1), "one exit");
        let held: Vec<*mut core::ffi::c_void> = (1..=4).map(|n| (0xC000 + n * 8) as *mut _).collect();
        let (proof, generation) = proof_begin(&mut j, &mut c, LIST, &held, &info).unwrap();
        assert_eq!((proof, j.recordings[i].proof, generation), (1, 1, j.recordings[i].generation));
        end_proof(&mut j, &mut c, LIST, generation, proof, true);
        assert_eq!((j.recordings[i].proof, j.recordings[i].failure), (0, why::NONE));
        end_proof(&mut j, &mut c, LIST, generation, proof, true);
        assert_eq!(c.failure, why::PROOF, "the proof already ended");
        // The token is a fake: forgotten, never released.
        core::mem::forget(j.recordings[i].source.token.take());
    }

    #[test]
    fn a_graphics_shader_writes_the_bound_targets() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        let i = j.get(&mut c, LIST, &none).unwrap();
        j.recordings[i].om[0] = j.hold(&mut c, 0xD0, &info);
        j.recordings[i].om[8] = j.hold(&mut c, 0xD8, &info);
        shader(&mut j, &mut c, LIST, &none, true);
        let kinds: Vec<(u32, u32)> = j.recordings[i].events.iter().map(|e| (e.kind, e.resource)).collect();
        assert_eq!(kinds, [(kind::SHADER, NO_RESOURCE), (kind::WRITE, 0), (kind::WRITE, 1)]);
        j.recordings[i].om_known = false;
        shader(&mut j, &mut c, LIST, &none, true);
        assert_eq!(j.recordings[i].events.last().unwrap().kind, kind::UNKNOWN);
    }

    #[test]
    fn the_receipt_status_and_the_failure_event() {
        let (mut j, mut c) = (Journal::default(), Counters::default());
        c.epoch = 3;
        let s = receipt_status(&j, &c, 0x9000, 7, 0, 40, [LIST, 0x2000]);
        assert_eq!((s.key.epoch, s.key.queue, s.key.sources[0].close_result), (3, 0x9000, E_PENDING));
        let i = j.get(&mut c, LIST, &none).unwrap();
        observe(&mut j, &mut c, LIST, &none, 0xE0, kind::WRITE, 4, 0, &info);
        let s = receipt_status(&j, &c, 0x9000, 7, 0, 40, [LIST, 0x2000]);
        assert_eq!((s.key.sources[0].list, s.key.sources[0].rows, s.key.sources[1].close_result), (LIST, 1, E_PENDING));
        let check = Check { failure_row: 0, ..Check::default() };
        let row = failure_event(&j, &j.recordings[i], &check);
        assert_eq!(row[4..7], [u64::from(kind::WRITE), 4, 0xE0]);
    }
}

// ---- the observer's lock and the detours' events (formerly src/Render/NativeTargetAccess.cpp's mutex and its entry
// points): each event tells the scene access trace first, then -- while the window is open -- runs under the lock with
// the publication around it (CyberpunkVR_TargetAccessSeq odd while it lasts, the stats row published at its end). The
// C++ half keeps the receipts, the queries, the proofs and the immediate writes, under this same lock (cvr_ta_lock).

#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod events {
    use super::*;
    use super::abi::*;
    use crate::scene_access_native as trace;
    use core::cell::UnsafeCell;
    use core::ffi::c_void;
    use core::ptr::null;
    use core::sync::atomic::{AtomicU32, Ordering::*};
    use windows::Win32::Graphics::Direct3D12::{ID3D12Resource, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_RESOURCE_BARRIER};
    use windows::Win32::System::Threading::{AcquireSRWLockExclusive, ReleaseSRWLockExclusive, SRWLOCK};

    #[allow(non_upper_case_globals)]
    extern "C" {
        static CyberpunkVR_TargetAccessSeq: AtomicU32;
    }
    /// Kind::Read, Kind::Write; Failure::Unknown; D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES.
    const READ: u32 = 0;
    const WRITE: u32 = 1;
    const UNKNOWN_FAILURE: u32 = 17;
    const ALL_SUBRESOURCES: u32 = u32::MAX;

    struct Lock(UnsafeCell<SRWLOCK>);
    // SAFETY: an SRW lock, used only through its acquire and release.
    unsafe impl Sync for Lock {}
    static LOCK: Lock = Lock(UnsafeCell::new(SRWLOCK { Ptr: core::ptr::null_mut() }));
    fn raw_lock(on: bool) {
        // SAFETY: the static lock; acquire and release pair up on the calling thread.
        unsafe {
            if on {
                AcquireSRWLockExclusive(LOCK.0.get());
            } else {
                ReleaseSRWLockExclusive(LOCK.0.get());
            }
        }
    }
    /// The observer lock (the C++ half's mutex): `true` acquires, `false` releases.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta_lock(on: bool) {
        raw_lock(on);
    }
    struct Locked;
    impl Drop for Locked {
        fn drop(&mut self) {
            raw_lock(false);
        }
    }
    fn lock() -> Locked {
        raw_lock(true);
        Locked
    }
    /// One publication: the sequence odd from here to its drop, the stats row published before it turns even.
    struct Publish;
    impl Publish {
        fn open() -> Self {
            // SAFETY: the C++ half's exported sequence (it lives for the process).
            unsafe { CyberpunkVR_TargetAccessSeq.fetch_add(1, AcqRel) };
            Publish
        }
    }
    impl Drop for Publish {
        fn drop(&mut self) {
            cvr_ta_publish_stats();
            // SAFETY: as open's.
            unsafe { CyberpunkVR_TargetAccessSeq.fetch_add(1, Release) };
        }
    }
    fn counters() -> &'static Counters {
        // SAFETY: the counters live for the process; their switches are atomics, the rest read under the lock.
        unsafe { &*cvr_ta_counters() }
    }
    fn observing() -> bool {
        counters().observing.load(Acquire)
    }
    /// ImmediateWrite (formerly src/Render/NativeTargetAccess.cpp's): the scene access trace told, then -- while the
    /// window is open -- the list's recording counted under the lock, the breadcrumb's destination captured and described
    /// outside it (breadcrumb_capture.rs) and the outcome recorded under it again (the same epoch required).
    ///
    /// # Safety
    /// `list` is the detour's live list; `parameters` and `modes` its arrays (`count` entries, or null).
    pub unsafe fn immediate_write(list: *mut c_void, count: u32, parameters: *const c_void, modes: *const c_void, caller: usize) -> bool {
        // SAFETY: the detour's arguments.
        unsafe { trace::cvr_sao_immediate(list, count, parameters as u64, modes as u64, caller as u64) };
        if count == 0 || !observing() {
            return false;
        }
        let (mut epoch, mut generation) = (0u64, 0u64);
        {
            let _locked = lock();
            if !observing() {
                return false;
            }
            let _publish = Publish::open();
            // SAFETY: writable outputs.
            if !unsafe { cvr_ta_immediate_begin(list as u64, &mut epoch, &mut generation) } {
                return false;
            }
        }
        // breadcrumb_write::Capture and Describe: the lease (the capture's state) released after the outcome.
        // SAFETY: the detour's arguments.
        let lease = unsafe { crate::breadcrumb_capture::cvr_bcw_capture(list, count, parameters as u64, modes as u64, caller as u64) };
        let mut snapshot = crate::breadcrumb_capture::Snapshot::default();
        // SAFETY: the live capture; an empty snapshot.
        let resolved = !lease.is_null() && unsafe { crate::breadcrumb_capture::cvr_bcw_describe(lease, &mut snapshot) };
        let failure = if resolved { 0 } else { crate::breadcrumb_capture::cvr_bcw_last_failure() };
        let recorded = {
            let _locked = lock();
            if !observing() || counters().epoch != epoch {
                false
            } else {
                let _publish = Publish::open();
                let c = &snapshot.creation;
                cvr_ta_immediate_end(
                    list as u64,
                    generation,
                    resolved,
                    failure,
                    snapshot.destination,
                    c.resource.as_ref().map_or(0, |r| r.as_raw() as u64),
                    c.heap.as_ref().map_or(0, |h| h.as_raw() as u64),
                    c.offset,
                    c.bytes,
                    c.kind,
                    snapshot.resource_desc.Flags.0 as u32,
                )
            }
        };
        drop(snapshot);
        // SAFETY: the capture's state, released once (null tolerated).
        unsafe { crate::breadcrumb_capture::cvr_bcw_release(lease) };
        recorded
    }
    /// An event under the lock while the window is open (checked again under it), the publication around `f`.
    fn locked(f: impl FnOnce()) {
        if !observing() {
            return;
        }
        let _locked = lock();
        if !counters().observing.load(Relaxed) {
            return;
        }
        let _publish = Publish::open();
        f();
    }
    fn observe(list: *mut c_void, resource: *mut c_void, kind: u32, tag: u32, subresource: u32) {
        locked(|| cvr_ta_observe(list as u64, resource as u64, kind, tag, subresource));
    }
    fn observe_unknown(list: *mut c_void, kind: u32) {
        locked(|| cvr_ta_observe_unknown(list as u64, kind));
    }
    /// The bound resources of an OM call or a clear (cvr-gpu's scene state), owned while the event runs.
    fn bound_resources(count_of: u32, handles: *const usize, contiguous: bool, depth: *const usize) -> (bool, [Option<ID3D12Resource>; 9]) {
        let mut raw = [0usize; 9];
        // SAFETY: the caller's descriptors; nine owned references (or nulls) out.
        let known = unsafe { cvr_gpu::scene_state::cvr_scene_capture_bound_resources(count_of, handles, contiguous, depth, &mut raw) };
        // SAFETY: the references are ours.
        (known, raw.map(|r| unsafe { (r != 0).then(|| ID3D12Resource::from_raw(r as *mut c_void)) }))
    }

    pub fn observer_unavailable() {
        trace::cvr_sao_unavailable();
        let c = counters();
        if c.unavailable.load(Acquire) {
            return;
        }
        let _locked = lock();
        if c.unavailable.load(Relaxed) {
            return;
        }
        let _publish = Publish::open();
        c.unavailable.store(true, Release);
        // SAFETY: the plain field, under the lock.
        unsafe { (*cvr_ta_counters()).failure = UNKNOWN_FAILURE };
    }
    pub fn reset(list: *mut c_void) {
        trace::cvr_sao_reset(list);
        locked(|| cvr_ta_reset(list as u64));
    }
    /// Clear (formerly NativeTargetAccess.cpp's): the window's state cleared.
    pub fn clear() {
        let _locked = lock();
        let _publish = Publish::open();
        cvr_ta_clear();
    }
    /// Arm: the window opened.
    pub fn arm() {
        let _locked = lock();
        let _publish = Publish::open();
        cvr_ta_arm();
    }
    /// Finish: the window closed once.
    pub fn finish() {
        let _locked = lock();
        if counters().finished {
            return;
        }
        let _publish = Publish::open();
        cvr_ta_finish();
    }
    /// MarkExit: the list leaves the window's sources.
    pub fn mark_exit(list: *mut c_void) {
        locked(|| cvr_ta_mark_exit(list as u64));
    }
    /// DiagnosticProofScope's opening over the held targets: (proof, generation), the proof 0 when the window is closed
    /// or the proof was refused.
    pub fn proof_begin(list: *mut c_void, held: &[*mut c_void]) -> (u32, u64) {
        let mut out = (0, 0);
        locked(|| {
            let (mut proof, mut generation) = (0u32, 0u64);
            // SAFETY: the caller's held targets.
            if unsafe { cvr_ta_proof_begin(list as u64, held.as_ptr(), held.len(), &mut proof, &mut generation) } {
                out = (proof, generation);
            }
        });
        out
    }
    /// EndProof: the proof's end (complete or abandoned) while the window is open.
    pub fn end_proof(list: *mut c_void, generation: u64, proof: u32, complete: bool) {
        if proof == 0 {
            return;
        }
        let _locked = lock();
        if !counters().observing.load(Relaxed) {
            return;
        }
        let _publish = Publish::open();
        cvr_ta_end_proof(list as u64, generation, proof, complete);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_clear() {
        clear();
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_arm() {
        arm();
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_finish() {
        finish();
    }
    pub fn close(list: *mut c_void, result: i32) {
        trace::cvr_sao_close(list, result);
        locked(|| cvr_ta_close(list as u64, result));
    }
    pub fn read(list: *mut c_void, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
        trace::cvr_sao_operand(list, resource, false, kind, subresource, offset, bytes);
        observe(list, resource, READ, kind, subresource);
    }
    pub fn written(list: *mut c_void, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
        trace::cvr_sao_operand(list, resource, true, kind, subresource, offset, bytes);
        observe(list, resource, WRITE, kind, ALL_SUBRESOURCES);
    }
    /// # Safety
    /// `barriers` holds `count_of` barriers.
    pub unsafe fn barriers(list: *mut c_void, count_of: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
        // SAFETY: the caller's barriers.
        unsafe { trace::cvr_sao_barriers(list, count_of, barriers) };
        // SAFETY: as above, under the lock.
        locked(|| unsafe { cvr_ta_barriers(list as u64, count_of, barriers) });
    }
    /// # Safety
    /// `colors` holds `count_of` descriptors (one when contiguous); `depth` null or valid.
    pub unsafe fn targets(list: *mut c_void, count_of: u32, colors: *const usize, contiguous: bool, depth: *const usize) {
        // SAFETY: the caller's descriptors.
        unsafe {
            trace::cvr_sao_targets(list, count_of, colors.cast::<D3D12_CPU_DESCRIPTOR_HANDLE>(), i32::from(contiguous), depth.cast())
        };
        locked(|| {
            if !cvr_ta_targets_begin(list as u64) {
                return;
            }
            let (known, held) = bound_resources(count_of, colors, contiguous, depth);
            let raw: [*mut c_void; 9] = core::array::from_fn(|i| held[i].as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()));
            // SAFETY: nine resources or nulls, held until the end of the event.
            unsafe { cvr_ta_targets_end(list as u64, known, raw.as_ptr()) };
        });
    }
    pub fn shader(list: *mut c_void, graphics: bool) {
        trace::cvr_sao_shader(list, graphics);
        locked(|| cvr_ta_shader(list as u64, graphics));
    }
    pub fn clear_view(list: *mut c_void, handle: usize, depth: bool) {
        trace::cvr_sao_clear_view(list, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: handle }, depth);
        if !observing() {
            return;
        }
        let (known, held) = if depth { bound_resources(0, null(), false, &handle) } else { bound_resources(1, &handle, false, null()) };
        if !known {
            observe_unknown(list, 0);
            return;
        }
        for resource in held.iter().flatten() {
            observe(list, resource.as_raw(), WRITE, 0, ALL_SUBRESOURCES);
        }
    }
    pub fn unknown(list: *mut c_void, kind: u32) {
        trace::cvr_sao_unknown(list, kind);
        observe_unknown(list, kind);
    }

    // ---- the C ABI the C++ API forwards to (the Rust test variants' target access)

    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_observer_unavailable() {
        observer_unavailable();
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_reset(list: *mut c_void) {
        reset(list);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_close(list: *mut c_void, result: i32) {
        close(list, result);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_read(list: *mut c_void, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
        read(list, resource, kind, subresource, offset, bytes);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_written(list: *mut c_void, resource: *mut c_void, kind: u32, subresource: u32, offset: u64, bytes: u64) {
        written(list, resource, kind, subresource, offset, bytes);
    }
    /// # Safety
    /// As barriers'.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta2_barriers(list: *mut c_void, count_of: u32, value: *const D3D12_RESOURCE_BARRIER) {
        unsafe { barriers(list, count_of, value) };
    }
    /// # Safety
    /// As targets'.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ta2_targets(list: *mut c_void, count_of: u32, colors: *const usize, contiguous: bool, depth: *const usize) {
        unsafe { targets(list, count_of, colors, contiguous, depth) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_shader(list: *mut c_void, graphics: bool) {
        shader(list, graphics);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_clear_view(list: *mut c_void, handle: usize, depth: bool) {
        clear_view(list, handle, depth);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ta2_unknown(list: *mut c_void, kind: u32) {
        unknown(list, kind);
    }
}
