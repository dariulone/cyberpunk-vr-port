//! The target access window's rules (`include/Render/NativeTargetAccess.hpp`, `target_access::detail`): the journal's
//! recording slot plan (a reset reuses the list's latest non-source slot, a new slot while there is room, else the
//! oldest closed non-source one; source generations are never discarded) and its pressure plan (at the row or marker
//! limit, the oldest closed non-source recording with rows, or with markers when only markers press); a saved receipt's
//! match against the current journal (both whole closed sources, their rows and exits, distinct lists and tokens); the
//! receipt's native submission boundary (the two adjacent source lists before the after-index, inside the exact live
//! invocation of distinct lists); an authenticated creation's comparison with the cached resource (same, upgrade or
//! conflict); and the access window check of up to four watched targets (sources, exits, proof scopes reading copy
//! sources between balanced transitions, shaders only over render-target / depth-write states, no aliasing, UAV or
//! opaque commands, exact state transitions per subresource).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions forward to `cvr_ta_*`; the records are mirrored here.
//! `tools/single_pass_tests/target_access_test` runs its cases against them in its Rust variant.

use crate::target_handoff::state_known;
use crate::texture_read::{could_alias, known_allocation, Allocation};

pub const RECORDING_LIMIT: usize = 256;
pub const ACCESS_LIMIT: u64 = 32768;
pub const MARKER_LIMIT: u64 = 8192;
pub const NO_RESOURCE: u32 = u32::MAX;
/// submission_frontier::detail::MaxSubmitLists.
const MAX_SUBMIT_LISTS: u32 = 256;
const ALL_SUBRESOURCES: u32 = u32::MAX;
const ALLOW_SIMULTANEOUS_ACCESS: u32 = 0x20;
const RENDER_TARGET: u32 = 0x4;
const DEPTH_WRITE: u32 = 0x10;
const COPY_SOURCE: u32 = 0x800;

/// target_access::Failure (the values these rules record).
pub mod failure {
    pub const NONE: u32 = 0;
    pub const CAPACITY: u32 = 2;
    pub const RECORDING: u32 = 6;
    pub const SOURCE: u32 = 7;
    pub const MISSING_EXIT: u32 = 8;
    pub const METADATA: u32 = 11;
    pub const READ: u32 = 12;
    pub const WRITE: u32 = 13;
    pub const TRANSITION: u32 = 14;
    pub const ALIAS: u32 = 15;
    pub const SHADER: u32 = 16;
    pub const UNKNOWN: u32 = 17;
    pub const PROOF: u32 = 18;
    pub const STATE: u32 = 19;
}
/// target_access::Kind.
pub mod kind {
    pub const READ: u32 = 0;
    pub const WRITE: u32 = 1;
    pub const TRANSITION: u32 = 2;
    pub const ALIAS: u32 = 3;
    pub const UAV: u32 = 4;
    pub const SHADER: u32 = 5;
    pub const UNKNOWN: u32 = 6;
    pub const SOURCE: u32 = 7;
    pub const EXIT: u32 = 8;
    pub const PROOF_BEGIN: u32 = 9;
    pub const PROOF_END: u32 = 10;
}
/// detail::CreationMatch.
pub mod creation {
    pub const CONFLICT: u32 = 0;
    pub const SAME: u32 = 1;
    pub const UPGRADE: u32 = 2;
}

/// detail::RecordingSlot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RecordingSlot {
    pub list: u64,
    pub generation: u64,
    pub rows: u64,
    pub markers: u64,
    pub source: bool,
    pub closed: bool,
}
/// detail::RecordingPlan.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingPlan {
    pub index: u32,
    pub generation: u64,
    pub rows: u64,
    pub markers: u64,
    pub failure: u32,
}
impl Default for RecordingPlan {
    fn default() -> Self {
        RecordingPlan { index: u32::MAX, generation: 0, rows: 0, markers: 0, failure: failure::NONE }
    }
}
/// detail::PressurePlan.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PressurePlan {
    pub index: u32,
    pub rows: u64,
    pub markers: u64,
    pub failure: u32,
}
/// detail::Resource.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Resource {
    pub pointer: u64,
    pub allocation: Allocation,
    pub flags: u32,
}
/// detail::ReceiptRecord.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptRecord {
    pub list: u64,
    pub generation: u64,
    pub native_generation: u64,
    pub token: u64,
    pub context: u64,
    pub registry: u64,
    pub rows: u64,
    pub markers: u64,
    pub frame: u32,
    pub side: u32,
    pub source_row: u32,
    pub exit_row: u32,
    pub proof: u32,
    pub close_result: i32,
    pub failure: u32,
    pub whole: bool,
    pub closed: bool,
    pub om_known: bool,
    pub om: [u32; 9],
}
/// detail::ReceiptKey.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptKey {
    pub epoch: u64,
    pub metadata_version: u64,
    pub queue: u64,
    pub thread: u64,
    pub scope: u64,
    pub frame: u32,
    pub sources: [ReceiptRecord; 2],
}
/// detail::ReceiptStatus.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReceiptStatus {
    pub key: ReceiptKey,
    pub observing: bool,
    pub finished: bool,
    pub unavailable: bool,
    pub failure: u32,
}
/// submission_frontier::Boundary.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Boundary {
    pub probe_epoch: u64,
    pub batch: u64,
    pub thread: u64,
    pub queue: u64,
    pub lists: u64,
    pub consumer: u64,
    pub graph_epoch: u64,
    pub pair_generation: u64,
    pub frame_owner: u64,
    pub fingerprint: u64,
    pub invocation: u64,
    pub count: u32,
    pub after_index: u32,
    pub source_indices: [u32; 2],
}
/// queue_residency::Invocation as the face passes it: the lists borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RawInvocation {
    pub thread: u64,
    pub queue: u64,
    pub lists_address: u64,
    pub lists: *const u64,
    pub count: usize,
}
/// detail::Event.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub kind: u32,
    pub resource: u32,
    pub other: u32,
    pub subresource: u32,
    pub before: u32,
    pub after: u32,
    pub flags: u32,
    pub proof: u32,
    pub tag: u32,
}
/// detail::Watch.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Watch {
    pub resource: Resource,
    pub count: u32,
    pub states: [u32; 2],
}
/// detail::Check.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Check {
    pub rows: u64,
    pub reads: u64,
    pub writes: u64,
    pub transitions: u64,
    pub aliases: u64,
    pub shaders: u64,
    pub excluded_shaders: u64,
    pub proof_reads: u64,
    pub proof_scopes: u64,
    pub failure: u32,
    pub failure_row: u32,
    pub resource: u32,
    pub phase: u32,
    pub final_states: [[u32; 2]; 4],
}
impl Default for Check {
    fn default() -> Self {
        Check {
            rows: 0,
            reads: 0,
            writes: 0,
            transitions: 0,
            aliases: 0,
            shaders: 0,
            excluded_shaders: 0,
            proof_reads: 0,
            proof_scopes: 0,
            failure: failure::NONE,
            failure_row: u32::MAX,
            resource: NO_RESOURCE,
            phase: 0,
            final_states: [[0; 2]; 4],
        }
    }
}
impl Check {
    /// Check::Reject: the first failure wins.
    pub fn reject(&mut self, why: u32, row: u32, target: u32) -> bool {
        if self.failure == failure::NONE {
            self.failure = why;
            self.failure_row = row;
            self.resource = target;
        }
        false
    }
}
const _: () = assert!(
    core::mem::size_of::<RecordingSlot>() == 40
        && core::mem::size_of::<RecordingPlan>() == 40
        && core::mem::size_of::<PressurePlan>() == 32
        && core::mem::size_of::<Resource>() == 48
        && core::mem::size_of::<ReceiptRecord>() == 136
        && core::mem::offset_of!(ReceiptRecord, om) == 96
        && core::mem::size_of::<ReceiptKey>() == 320
        && core::mem::size_of::<ReceiptStatus>() == 328
        && core::mem::size_of::<Boundary>() == 104
        && core::mem::size_of::<Event>() == 36
        && core::mem::size_of::<Watch>() == 64
        && core::mem::size_of::<Check>() == 120
);

/// detail::PlanRecording: a reset discards only this list's previous commands; source generations stay intact.
pub fn plan_recording(slots: &[RecordingSlot], list: u64, generation: u64, mut rows: u64, mut markers: u64) -> RecordingPlan {
    let reject = |reason: u32| RecordingPlan { failure: reason, ..RecordingPlan::default() };
    if list == 0 || slots.len() > RECORDING_LIMIT || markers > rows {
        return reject(failure::RECORDING);
    }
    if generation == u64::MAX {
        return reject(failure::CAPACITY);
    }
    let mut latest = u32::MAX;
    for (i, slot) in slots.iter().enumerate() {
        if slot.list == 0 || slot.generation == 0 || slot.generation > generation || slot.markers > slot.rows {
            return reject(failure::RECORDING);
        }
        if slot.list == list && (latest == u32::MAX || slot.generation > slots[latest as usize].generation) {
            latest = i as u32;
        }
    }
    let mut index = if latest != u32::MAX && !slots[latest as usize].source { latest } else { u32::MAX };
    if index == u32::MAX {
        if slots.len() < RECORDING_LIMIT {
            index = slots.len() as u32;
        } else {
            for (i, slot) in slots.iter().enumerate() {
                if !slot.source && slot.closed && (index == u32::MAX || slot.generation < slots[index as usize].generation) {
                    index = i as u32;
                }
            }
        }
    }
    if index == u32::MAX {
        return reject(failure::CAPACITY);
    }
    if (index as usize) < slots.len() {
        let slot = &slots[index as usize];
        if slot.source || slot.rows > rows || slot.markers > markers {
            return reject(failure::RECORDING);
        }
        rows -= slot.rows;
        markers -= slot.markers;
    }
    RecordingPlan { index, generation: generation + 1, rows, markers, failure: failure::NONE }
}
/// detail::PlanPressure.
pub fn plan_pressure(slots: &[RecordingSlot], current: u32, rows: u64, markers: u64, append_marker: bool) -> PressurePlan {
    let mut output = PressurePlan { index: u32::MAX, rows, markers, failure: failure::NONE };
    if slots.len() > RECORDING_LIMIT || current as usize >= slots.len() || markers > rows || rows > ACCESS_LIMIT || markers > MARKER_LIMIT {
        output.failure = failure::RECORDING;
        return output;
    }
    let row_pressure = rows == ACCESS_LIMIT;
    let marker_pressure = append_marker && markers == MARKER_LIMIT;
    if !row_pressure && !marker_pressure {
        return output;
    }
    let mut candidate = u32::MAX;
    for (i, slot) in slots.iter().enumerate() {
        if i as u32 == current || slot.source || !slot.closed || slot.rows == 0 || (!row_pressure && slot.markers == 0) {
            continue;
        }
        if slot.generation == 0 || slot.markers > slot.rows || slot.rows > rows || slot.markers > markers {
            output.failure = failure::RECORDING;
            return output;
        }
        if candidate == u32::MAX || slot.generation < slots[candidate as usize].generation {
            candidate = i as u32;
        }
    }
    if candidate == u32::MAX {
        output.failure = failure::CAPACITY;
        return output;
    }
    output.index = candidate;
    output.rows -= slots[candidate as usize].rows;
    output.markers -= slots[candidate as usize].markers;
    if output.markers > output.rows {
        output.failure = failure::RECORDING;
    }
    output
}
/// detail::ReceiptMatches.
pub fn receipt_matches(saved: &ReceiptKey, current: &ReceiptStatus) -> bool {
    if !current.observing
        || current.finished
        || current.unavailable
        || current.failure != failure::NONE
        || saved.epoch == 0
        || saved.queue == 0
        || saved.thread == 0
        || saved.scope == 0
        || saved.frame == 0
        || *saved != current.key
    {
        return false;
    }
    for (side, source) in saved.sources.iter().enumerate() {
        if source.list == 0
            || source.generation == 0
            || source.native_generation == 0
            || source.token == 0
            || source.context == 0
            || source.registry == 0
            || !source.whole
            || !source.closed
            || source.close_result < 0
            || source.failure != failure::NONE
            || source.proof != 0
            || source.frame != saved.frame
            || source.side != side as u32
            || source.rows == 0
            || source.rows > ACCESS_LIMIT
            || source.markers > MARKER_LIMIT
            || source.markers > source.rows
            || source.source_row >= source.exit_row
            || source.exit_row as u64 >= source.rows
        {
            return false;
        }
    }
    saved.sources[0].list != saved.sources[1].list && saved.sources[0].token != saved.sources[1].token
}
/// detail::ReceiptBoundary.
pub fn receipt_boundary(boundary: &Boundary, thread: u64, queue: u64, lists_address: u64, lists: &[u64], sources: &[u64; 2]) -> bool {
    if boundary.probe_epoch == 0
        || boundary.batch == 0
        || boundary.invocation == 0
        || boundary.consumer == 0
        || boundary.graph_epoch == 0
        || boundary.pair_generation == 0
        || boundary.frame_owner == 0
        || boundary.fingerprint == 0
        || boundary.thread == 0
        || boundary.queue == 0
        || boundary.lists == 0
        || boundary.thread != thread
        || boundary.queue != queue
        || boundary.lists != lists_address
        || boundary.count as usize != lists.len()
        || boundary.count < 3
        || boundary.count > MAX_SUBMIT_LISTS
        || sources[0] == 0
        || sources[1] == 0
        || sources[0] == sources[1]
    {
        return false;
    }
    let first = boundary.source_indices[0].min(boundary.source_indices[1]);
    let last = boundary.source_indices[0].max(boundary.source_indices[1]);
    if last >= boundary.count
        || first.wrapping_add(1) != last
        || boundary.after_index != last.wrapping_add(1)
        || boundary.after_index >= boundary.count
    {
        return false;
    }
    for side in 0..2 {
        if lists[boundary.source_indices[side] as usize] != sources[side] {
            return false;
        }
    }
    for (i, &list) in lists.iter().enumerate() {
        if list == 0 || lists[..i].contains(&list) {
            return false;
        }
    }
    true
}
/// detail::CompareAuthenticatedCreation: only an authenticated breadcrumb lease supplies the observed metadata.
pub fn compare_authenticated_creation(cached: &Resource, authenticated: &Resource) -> u32 {
    if cached.pointer == 0 || cached.pointer != authenticated.pointer || cached.flags != authenticated.flags || !known_allocation(&authenticated.allocation) {
        return creation::CONFLICT;
    }
    if cached.allocation.kind == 0 {
        return creation::UPGRADE;
    }
    let (a, b) = (&cached.allocation, &authenticated.allocation);
    if known_allocation(a) && a.kind == b.kind && a.heap == b.heap && a.offset == b.offset && a.bytes == b.bytes {
        creation::SAME
    } else {
        creation::CONFLICT
    }
}
fn span(subresource: u32, count: u32) -> core::ops::Range<usize> {
    if subresource == ALL_SUBRESOURCES { 0..count as usize } else { subresource as usize..subresource as usize + 1 }
}
/// detail::CheckWindow.
pub fn check_window(events: &[Event], resources: &[Resource], watched: &[Watch; 4], own_source: bool, out: &mut Check) -> bool {
    *out = Check::default();
    let (mut source, mut exit) = (!own_source, !own_source);
    let mut proof: u32 = 0;
    let mut proof_touched = false;
    let mut allowed = [false; 4];
    let mut proof_states = [[0u32; 2]; 4];
    for (t, watch) in watched.iter().enumerate() {
        if watch.resource.pointer == 0
            || !known_allocation(&watch.resource.allocation)
            || watch.count == 0
            || watch.count > 2
            || watch.resource.flags & ALLOW_SIMULTANEOUS_ACCESS != 0
        {
            return out.reject(failure::METADATA, 0, t as u32);
        }
        for p in 0..watch.count as usize {
            if !state_known(watch.states[p]) {
                return out.reject(failure::STATE, 0, t as u32);
            }
            out.final_states[t][p] = watch.states[p];
        }
    }
    let resource = |index: u32| resources.get(index as usize);
    let overlaps = |t: usize, value: Option<&Resource>| {
        value.is_some_and(|value| value.pointer == watched[t].resource.pointer || could_alias(&watched[t].resource.allocation, &value.allocation))
    };
    for (row, event) in events.iter().enumerate() {
        let row = row as u32;
        if event.kind == kind::SOURCE {
            if !own_source {
                continue;
            }
            if source {
                return out.reject(failure::SOURCE, row, NO_RESOURCE);
            }
            source = true;
            continue;
        }
        if !source {
            continue;
        }
        if event.kind == kind::EXIT {
            if !own_source {
                continue;
            }
            if exit || proof != 0 {
                return out.reject(failure::MISSING_EXIT, row, NO_RESOURCE);
            }
            exit = true;
            continue;
        }
        let checking = exit;
        if checking {
            out.rows += 1;
        }
        let a = resource(event.resource);
        let b = resource(event.other);
        if (event.resource != NO_RESOURCE && a.is_none()) || (event.other != NO_RESOURCE && b.is_none()) {
            return out.reject(failure::METADATA, row, NO_RESOURCE);
        }
        if event.kind == kind::PROOF_BEGIN {
            if !checking {
                continue;
            }
            if event.proof == 0 || a.is_none() || (proof != 0 && (proof != event.proof || proof_touched)) {
                return out.reject(failure::PROOF, row, NO_RESOURCE);
            }
            if proof == 0 {
                proof = event.proof;
                proof_states = out.final_states;
                allowed = [false; 4];
                out.proof_scopes += 1;
            }
            let a = a.unwrap();
            for t in 0..4 {
                if overlaps(t, Some(a)) {
                    if a.pointer != watched[t].resource.pointer {
                        return out.reject(failure::ALIAS, row, t as u32);
                    }
                    allowed[t] = true;
                }
            }
            continue;
        }
        if event.kind == kind::PROOF_END {
            if !checking {
                continue;
            }
            if proof == 0 || event.proof != proof || event.flags == 0 || out.final_states != proof_states {
                return out.reject(failure::PROOF, row, NO_RESOURCE);
            }
            proof = 0;
            proof_touched = false;
            continue;
        }
        if proof != 0 {
            proof_touched = true;
        }
        if event.kind == kind::UNKNOWN {
            if checking {
                return out.reject(if proof != 0 { failure::PROOF } else { failure::UNKNOWN }, row, NO_RESOURCE);
            }
            continue;
        }
        if event.kind == kind::SHADER {
            if !checking {
                continue;
            }
            out.shaders += 1;
            if proof != 0 {
                return out.reject(failure::PROOF, row, NO_RESOURCE);
            }
            for (t, watch) in watched.iter().enumerate() {
                for p in 0..watch.count as usize {
                    let state = out.final_states[t][p];
                    if state != RENDER_TARGET && state != DEPTH_WRITE {
                        return out.reject(failure::SHADER, row, t as u32);
                    }
                }
            }
            out.excluded_shaders += 1;
            continue;
        }
        if event.kind == kind::ALIAS {
            if checking {
                out.aliases += 1;
            }
            if proof != 0 {
                return out.reject(failure::PROOF, row, NO_RESOURCE);
            }
            for t in 0..4 {
                if (a.is_none() && b.is_none() && watched[t].resource.allocation.kind != 1) || overlaps(t, a) || overlaps(t, b) {
                    return out.reject(failure::ALIAS, row, t as u32);
                }
            }
            continue;
        }
        if event.kind == kind::UAV {
            if proof != 0 {
                return out.reject(failure::PROOF, row, NO_RESOURCE);
            }
            if checking
                && a.is_none_or(|a| {
                    watched.iter().any(|watch| a.pointer == watch.resource.pointer || could_alias(&watch.resource.allocation, &a.allocation))
                })
            {
                return out.reject(failure::UNKNOWN, row, NO_RESOURCE);
            }
            continue;
        }
        let Some(a) = a else {
            return out.reject(failure::METADATA, row, NO_RESOURCE);
        };
        if checking {
            match event.kind {
                kind::READ => out.reads += 1,
                kind::WRITE => out.writes += 1,
                kind::TRANSITION => out.transitions += 1,
                _ => {}
            }
        }
        for t in 0..4 {
            if !overlaps(t, Some(a)) {
                continue;
            }
            let target = t as u32;
            if a.pointer != watched[t].resource.pointer {
                return out.reject(failure::ALIAS, row, target);
            }
            if event.kind == kind::TRANSITION {
                if checking && !own_source && proof == 0 {
                    return out.reject(failure::TRANSITION, row, target);
                }
                if event.flags != 0
                    || !state_known(event.before)
                    || !state_known(event.after)
                    || (event.subresource != ALL_SUBRESOURCES && event.subresource >= watched[t].count)
                {
                    return out.reject(failure::STATE, row, target);
                }
                if proof != 0 && (!allowed[t] || (event.before != COPY_SOURCE && event.after != COPY_SOURCE)) {
                    return out.reject(failure::PROOF, row, target);
                }
                for p in span(event.subresource, watched[t].count) {
                    if out.final_states[t][p] != event.before {
                        return out.reject(failure::STATE, row, target);
                    }
                    out.final_states[t][p] = event.after;
                }
            } else if checking && event.kind == kind::WRITE {
                return out.reject(if proof != 0 { failure::PROOF } else { failure::WRITE }, row, target);
            } else if checking && event.kind == kind::READ {
                if proof == 0 || !allowed[t] {
                    return out.reject(failure::READ, row, target);
                }
                if event.subresource != ALL_SUBRESOURCES && event.subresource >= watched[t].count {
                    return out.reject(failure::STATE, row, target);
                }
                if span(event.subresource, watched[t].count).any(|p| out.final_states[t][p] != COPY_SOURCE) {
                    return out.reject(failure::STATE, row, target);
                }
                out.proof_reads += 1;
            }
        }
    }
    let end = events.len() as u32;
    if !source {
        return out.reject(failure::SOURCE, end, NO_RESOURCE);
    }
    if !exit {
        return out.reject(failure::MISSING_EXIT, end, NO_RESOURCE);
    }
    if proof != 0 {
        return out.reject(failure::PROOF, end, NO_RESOURCE);
    }
    true
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// PlanRecording (`output` always written).
///
/// # Safety
/// `slots` holds `count` slots (or is null with none); `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ta_plan_recording(
    slots: *const RecordingSlot,
    count: usize,
    list: u64,
    generation: u64,
    rows: u64,
    markers: u64,
    output: *mut RecordingPlan,
) -> bool {
    let plan = plan_recording(unsafe { slice(slots, count) }, list, generation, rows, markers);
    unsafe { *output = plan };
    plan.failure == failure::NONE
}
/// PlanPressure (`output` always written).
///
/// # Safety
/// `slots` holds `count` slots (or is null with none); `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ta_plan_pressure(
    slots: *const RecordingSlot,
    count: usize,
    current: u32,
    rows: u64,
    markers: u64,
    append_marker: bool,
    output: *mut PressurePlan,
) -> bool {
    let plan = plan_pressure(unsafe { slice(slots, count) }, current, rows, markers, append_marker);
    unsafe { *output = plan };
    plan.failure == failure::NONE
}
/// ReceiptMatches.
///
/// # Safety
/// `saved` and `current` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ta_receipt_matches(saved: *const ReceiptKey, current: *const ReceiptStatus) -> bool {
    unsafe { receipt_matches(&*saved, &*current) }
}
/// ReceiptBoundary.
///
/// # Safety
/// `boundary`, `invocation` (with its lists) and `sources` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ta_receipt_boundary(boundary: *const Boundary, invocation: *const RawInvocation, sources: *const [u64; 2]) -> bool {
    unsafe {
        let invocation = &*invocation;
        receipt_boundary(&*boundary, invocation.thread, invocation.queue, invocation.lists_address, slice(invocation.lists, invocation.count), &*sources)
    }
}
/// CompareAuthenticatedCreation: the CreationMatch.
///
/// # Safety
/// `cached` and `authenticated` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ta_compare_creation(cached: *const Resource, authenticated: *const Resource) -> u32 {
    unsafe { compare_authenticated_creation(&*cached, &*authenticated) }
}
/// CheckWindow (`out` always written).
///
/// # Safety
/// `events` and `resources` hold their counts (or are null with none); `watched` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ta_check_window(
    events: *const Event,
    event_count: usize,
    resources: *const Resource,
    resource_count: usize,
    watched: *const [Watch; 4],
    own_source: bool,
    out: *mut Check,
) -> bool {
    let mut check = Check::default();
    let result = unsafe { check_window(slice(events, event_count), slice(resources, resource_count), &*watched, own_source, &mut check) };
    unsafe { *out = check };
    result
}
