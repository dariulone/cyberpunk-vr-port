//! THE TARGET HANDOFF'S OBSERVER (formerly src/Render/NativeTargetHandoff.cpp's anonymous namespace): the two source
//! recordings the native draws hand over, each with its four writable targets (three colours and the depth) and their
//! per-subresource ledgers, and the publication of what they hold into the CyberpunkVR_TargetHandoff* export rows.
//!
//! The sources live here (under the observer lock, with the observation window and the capture epoch); the publication
//! runs on them, as do the ledger's events (the writes, the shader draws, the barriers, the unknown commands). The rules
//! each source and target follows are target_handoff.rs'. The C++ API (src/Render/NativeTargetHandoff.cpp) forwards to
//! the entry points at the end of this file; the detours call them directly.
use windows::core::Interface as _;
use crate::native_recording::Lease;
use crate::target_handoff::{source, RecordingState, TargetState, STATES_PER_SOURCE};
use crate::texture_writable::WritableIdentity;
use cvr_gpu::scene_state::TargetInfo;

pub const SOURCE_LIMIT: usize = 2;
pub const TARGETS_PER_SOURCE: usize = 4;

/// The observer's Target (formerly the anonymous namespace's): the writable identity (owned here: its COM references and
/// its recording's token), the bound view, its descriptor, the per-subresource ledger, the binding slot, the planes and
/// the first state row.
#[repr(C)]
pub struct Target {
    pub identity: WritableIdentity,
    /// The identity was captured (the C++ WritableTargetIdentity's authenticated copy present).
    pub authenticated: bool,
    pub view: TargetInfo,
    pub handle: usize,
    pub state: TargetState,
    pub slot: u32,
    pub planes: u32,
    pub state_first: u32,
}
/// The observer's Source (formerly the anonymous namespace's): one recording's ledger, its native recording (its token
/// owned here), its targets, the frame and side, the counts and the capture's flags.
#[repr(C)]
pub struct Source {
    pub state: RecordingState,
    pub recording: Lease,
    pub targets: [Target; TARGETS_PER_SOURCE],
    pub frame: u32,
    pub side: u32,
    pub target_count: u32,
    pub state_count: u32,
    pub occupied: bool,
    pub capturing: bool,
    pub capture_touched: bool,
    pub retired_recording: bool,
}

/// The export rows Publish fills.
pub struct Rows<'a> {
    pub stats: &'a mut [u64; 16],
    pub sources: &'a mut [u64; 32],
    pub source_local: &'a mut [u64; 16],
    pub owners: &'a mut [u64; 16],
    pub targets: &'a mut [u64; 192],
    pub states: &'a mut [u64; 80],
}

fn address<T: windows::core::Interface>(value: &Option<T>) -> u64 {
    value.as_ref().map_or(0, |v| v.as_raw() as u64)
}

/// Publish: the per-publication stats (sources, targets, states, pending validations) and every row recomputed from
/// the occupied sources; the counters kept by the observer's events (the schema, begins, barriers, ...) stay.
pub fn publish(sources: &[Source; SOURCE_LIMIT], rows: Rows<'_>) {
    let stats = rows.stats;
    stats[1] = 0;
    stats[2] = 0;
    stats[3] = 0;
    stats[14] = 0;
    rows.sources.fill(0);
    rows.source_local.fill(0);
    rows.owners.fill(0);
    rows.targets.fill(0);
    rows.states.fill(0);
    for (side, source) in sources.iter().enumerate() {
        if !source.occupied {
            continue;
        }
        stats[1] += 1;
        stats[2] += u64::from(source.target_count);
        stats[3] += u64::from(source.state_count);
        let state = &source.state;
        let aggregate = state.aggregate_flags();
        if aggregate & source::PENDING_VALIDATED != 0 {
            stats[14] += 1;
        }
        let side64 = side as u64;
        let values: [u64; 16] = [
            u64::from(source.frame),
            side64,
            state.list,
            state.generation,
            source.recording.context,
            source.recording.registry,
            side64 * TARGETS_PER_SOURCE as u64,
            u64::from(source.target_count),
            side64 * STATES_PER_SOURCE as u64,
            u64::from(source.state_count),
            aggregate,
            u64::from(state.exit_reason),
            u64::from(state.exit_kind),
            u64::from(state.close_result as u32),
            state.queue,
            u64::from(state.aggregate_failure()),
        ];
        rows.sources[side * 16..side * 16 + 16].copy_from_slice(&values);
        let local: [u64; 8] = [
            state.flags,
            u64::from(state.failure),
            u64::from(state.close_result as u32),
            u64::from(state.foreign_failure),
            state.foreign_list,
            state.foreign_kind,
            state.foreign_events,
            u64::from(state.flags & source::SUBMITTED_OBSERVED != 0),
        ];
        rows.source_local[side * 8..side * 8 + 8].copy_from_slice(&local);
        for (index, target) in source.targets.iter().enumerate().take(source.target_count as usize) {
            let target_index = side * TARGETS_PER_SOURCE + index;
            let identity = &target.identity;
            let creation = &identity.creation;
            let desc = &identity.resource_desc;
            let refused = if target.state.failure != crate::target_handoff::failure::NONE { source::REFUSED } else { 0 };
            let fields: [u64; 24] = [
                side64,
                u64::from(target.slot),
                address(&creation.resource),
                target.view.identity,
                identity.native.registry,
                u64::from(identity.native.handle),
                u64::from(creation.kind),
                address(&creation.heap),
                creation.offset,
                creation.bytes,
                desc.Width,
                u64::from(desc.Height),
                u64::from(desc.Format.0 as u32),
                u64::from(target.view.view_format),
                u64::from(desc.Flags.0 as u32),
                u64::from(target.view.view_flags),
                u64::from(desc.MipLevels),
                u64::from(desc.DepthOrArraySize),
                u64::from(target.planes),
                u64::from(target.state_first),
                u64::from(target.state.count),
                source::CAPTURED | refused,
                u64::from(target.state.failure),
                u64::from(identity.expected_writable_state.0 as u32),
            ];
            if let Some(out) = rows.targets.get_mut(target_index * 24..target_index * 24 + 24) {
                out.copy_from_slice(&fields);
            }
            let owner_tag = u32::from_le_bytes(identity.native_record[8..12].try_into().unwrap_or([0; 4]));
            if let Some(out) = rows.owners.get_mut(target_index * 2..target_index * 2 + 2) {
                out.copy_from_slice(&[u64::from(owner_tag), identity.allocator_owner_observation]);
            }
            for (subresource, current) in target.state.states.iter().enumerate().take(target.state.count as usize) {
                let row = (target.state_first as usize + subresource) * 8;
                let fields: [u64; 8] = [
                    side64,
                    target_index as u64,
                    subresource as u64,
                    u64::from(current.seed),
                    u64::from(current.current),
                    u64::from(current.frozen),
                    current.flags,
                    u64::from(current.transitions),
                ];
                if let Some(out) = rows.states.get_mut(row..row + 8) {
                    out.copy_from_slice(&fields);
                }
            }
        }
    }
}

#[cfg(not(test))]
#[allow(non_upper_case_globals)]
extern "C" {
    static mut CyberpunkVR_TargetHandoffStats: [u64; 16];
    static mut CyberpunkVR_TargetHandoffSources: [u64; 32];
    static mut CyberpunkVR_TargetHandoffSourceLocal: [u64; 16];
    static mut CyberpunkVR_TargetHandoffOwners: [u64; 16];
    static mut CyberpunkVR_TargetHandoffTargets: [u64; 192];
    static mut CyberpunkVR_TargetHandoffStates: [u64; 80];
}
/// The C++ half's Publish, under its observer lock and inside its publication (the sequence odd).
///
/// # Safety
/// `sources` is the C++ half's array of SOURCE_LIMIT sources, the lock held.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_publish(sources: *const Source) {
    // SAFETY: the caller's sources and lock; the export rows are only written under it.
    unsafe {
        let Some(sources) = sources.cast::<[Source; SOURCE_LIMIT]>().as_ref() else { return };
        publish(
            sources,
            Rows {
                stats: &mut *(&raw mut CyberpunkVR_TargetHandoffStats),
                sources: &mut *(&raw mut CyberpunkVR_TargetHandoffSources),
                source_local: &mut *(&raw mut CyberpunkVR_TargetHandoffSourceLocal),
                owners: &mut *(&raw mut CyberpunkVR_TargetHandoffOwners),
                targets: &mut *(&raw mut CyberpunkVR_TargetHandoffTargets),
                states: &mut *(&raw mut CyberpunkVR_TargetHandoffStates),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    //! The rows from a source with two targets, the other side empty; the event counters kept.
    use super::*;
    use crate::target_handoff::{failure, state};

    fn zeroed_sources() -> Box<[Source; SOURCE_LIMIT]> {
        // SAFETY: every field is valid zeroed (integers, bools, and the identity's absent COM references).
        unsafe { Box::new(core::mem::zeroed()) }
    }

    #[test]
    fn rows_from_the_occupied_source() {
        let mut sources = zeroed_sources();
        let s = &mut sources[1];
        s.occupied = true;
        s.frame = 77;
        s.target_count = 2;
        s.state_count = 3;
        s.state = RecordingState::default();
        s.state.list = 0x1000;
        s.state.generation = 9;
        s.state.flags |= source::CAPTURED | source::SUBMITTED_OBSERVED;
        s.state.close_result = -2147024809; // E_INVALIDARG
        s.recording.context = 5;
        s.recording.registry = 6;
        s.targets[0].slot = 2;
        s.targets[0].planes = 1;
        s.targets[0].state_first = 5;
        s.targets[0].state.count = 1;
        s.targets[0].state.states[0].seed = 4;
        s.targets[0].state.states[0].flags = state::CURRENT_KNOWN;
        s.targets[0].identity.native_record[8..12].copy_from_slice(&0xABCDu32.to_le_bytes());
        s.targets[0].identity.allocator_owner_observation = 0x55;
        s.targets[0].identity.resource_desc.Width = 1864;
        s.targets[0].identity.resource_desc.Height = 2016;
        s.targets[1].slot = 4;
        s.targets[1].planes = 2;
        s.targets[1].state_first = 6;
        s.targets[1].state.count = 2;
        s.targets[1].state.failure = failure::ALIASING;
        let (mut stats, mut src, mut local, mut owners, mut targets, mut states) =
            ([7u64; 16], [1u64; 32], [1u64; 16], [1u64; 16], [1u64; 192], [1u64; 80]);
        stats[0] = 2;
        publish(
            &sources,
            Rows {
                stats: &mut stats,
                sources: &mut src,
                source_local: &mut local,
                owners: &mut owners,
                targets: &mut targets,
                states: &mut states,
            },
        );
        assert_eq!((stats[0], stats[1], stats[2], stats[3], stats[4], stats[14]), (2, 1, 2, 3, 7, 0));
        assert!(src[..16].iter().all(|&v| v == 0));
        assert_eq!(&src[16..26], &[77, 1, 0x1000, 9, 5, 6, 4, 2, 5, 3]);
        assert_eq!(src[29], 0x8007_0057);
        assert_eq!((local[8], local[15]), (source::PREFIX_UNKNOWN | source::CAPTURED | source::SUBMITTED_OBSERVED, 1));
        // Target rows 4 and 5 (side 1).
        let t0 = &targets[4 * 24..5 * 24];
        assert_eq!((t0[0], t0[1], t0[10], t0[11], t0[18], t0[19], t0[20], t0[21]), (1, 2, 1864, 2016, 1, 5, 1, source::CAPTURED));
        let t1 = &targets[5 * 24..6 * 24];
        assert_eq!((t1[1], t1[21], t1[22]), (4, source::CAPTURED | source::REFUSED, u64::from(failure::ALIASING)));
        assert_eq!((owners[8], owners[9]), (0xABCD, 0x55));
        // State rows 5 (target 4) and 6, 7 (target 5).
        assert_eq!(&states[40..48], &[1, 4, 0, 4, 0, 0, state::CURRENT_KNOWN, 0]);
        assert_eq!((states[48], states[49], states[50], states[56 + 2]), (1, 5, 0, 1));
        assert!(states[..40].iter().all(|&v| v == 0) && targets[..96].iter().all(|&v| v == 0));
    }
}

// ---- the ledger's events on the sources (slice of the C++ anonymous namespace: Reject, UnknownLocked, Touch,
// WriteLocked, ShaderDrawLocked and Barriers' loop), under the C++ half's observer lock

/// ExitKind.
pub mod exit_kind {
    pub const UNKNOWN: u32 = 0;
    pub const TRANSITION: u32 = 2;
    pub const ALIASING: u32 = 3;
    pub const UAV: u32 = 4;
    pub const CLEAR: u32 = 7;
    pub const OTHER: u32 = 9;
}
/// Whether an aliasing event over `before` (and `after`) touches a watched target's memory
/// (texture_read::AliasAffects in the plugin; tests supply their own).
pub type Affects<'a> = &'a dyn Fn(&Target, *mut core::ffi::c_void, *mut core::ffi::c_void) -> bool;

/// Count: a stat, saturating.
pub fn count(stats: &mut [u64; 16], index: usize) {
    if stats[index] != u64::MAX {
        stats[index] += 1;
    }
}
/// A target's resource as the barriers name it.
pub fn target_resource(target: &Target) -> *mut core::ffi::c_void {
    target.identity.creation.resource.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
}
fn live(source: &Source) -> usize {
    (source.target_count as usize).min(TARGETS_PER_SOURCE)
}
/// Reject: the recording and every captured target refused for one reason.
pub fn reject(source: &mut Source, why: u32) {
    source.state.reject(why);
    let n = live(source);
    for target in &mut source.targets[..n] {
        target.state.reject(why);
    }
}
fn foreign_recording(source: &Source, list: u64) -> bool {
    source.retired_recording || source.state.list != list
}
fn unknown_source(source: &mut Source, list: u64, kind: u32, why: u32) {
    if foreign_recording(source, list) {
        source.state.foreign(list, kind);
    } else {
        reject(source, why);
    }
}
/// UnknownLocked: a command this observer cannot account for, on every captured source it could concern.
pub fn unknown_locked(sources: &mut [Source; SOURCE_LIMIT], list: u64, kind: u32) {
    for source in sources.iter_mut() {
        if source.occupied
            && source.state.flags & source::CAPTURED != 0
            && (source.state.tracking() || source.retired_recording || source.state.list != list)
        {
            unknown_source(source, list, kind, crate::target_handoff::failure::UNKNOWN_COMMAND);
        }
    }
}
fn touch(source: &mut Source, list: u64, write: bool, alias: bool, kind: u32) {
    if foreign_recording(source, list) {
        source.state.foreign(list, kind);
    } else {
        source.state.touch(list, write, alias, kind);
    }
}
/// WriteLocked: a write to `destination` (null: unknown), on the captured sources it touches.
pub fn write_locked(sources: &mut [Source; SOURCE_LIMIT], list: u64, destination: *mut core::ffi::c_void, kind: u32, affects: Affects<'_>) {
    if destination.is_null() {
        unknown_locked(sources, list, kind);
        return;
    }
    for source in sources.iter_mut() {
        if !(source.occupied && source.state.flags & source::CAPTURED != 0) {
            continue;
        }
        if source.capturing {
            if foreign_recording(source, list) {
                source.state.foreign(list, kind);
            }
            continue;
        }
        if !source.state.tracking() && !source.retired_recording && source.state.list == list {
            continue;
        }
        for i in 0..live(source) {
            if !affects(&source.targets[i], destination, core::ptr::null_mut()) {
                continue;
            }
            let aliases = target_resource(&source.targets[i]) != destination;
            touch(source, list, true, aliases, kind);
            if foreign_recording(source, list) {
                continue;
            }
            if aliases {
                source.targets[i].state.reject(crate::target_handoff::failure::ALIASING);
            }
            if source.state.flags & source::REFUSED != 0 {
                let why = source.state.failure;
                reject(source, why);
            }
        }
    }
}
/// ShaderDrawLocked: a draw whose shaders may read or write the watched targets' states.
pub fn shader_draw_locked(sources: &mut [Source; SOURCE_LIMIT], list: u64) {
    for source in sources.iter_mut() {
        if !source.occupied {
            continue;
        }
        if source.capturing {
            if foreign_recording(source, list) {
                source.state.foreign(list, exit_kind::OTHER);
            }
            continue;
        }
        let n = live(source);
        let Source { state, targets, retired_recording, .. } = source;
        for target in &targets[..n] {
            state.shader_draw(list, &target.state, *retired_recording);
        }
        if source.state.flags & source::REFUSED != 0 {
            let why = source.state.failure;
            reject(source, why);
        }
    }
}
/// One resource barrier, as the observer reads it.
#[derive(Clone, Copy, Debug)]
pub enum Barrier {
    Transition { resource: *mut core::ffi::c_void, subresource: u32, before: u32, after: u32, flags: u32 },
    Aliasing { before: *mut core::ffi::c_void, after: *mut core::ffi::c_void },
    Uav { resource: *mut core::ffi::c_void },
    Other,
}
/// Barriers after the event: counted (6, aliasing 8, transitions applied 7), each against every captured source's
/// targets -- a transition of a watched target applied to its ledger, any other touch of its memory an aliasing
/// touch, a UAV barrier over it (or over all memory) a write; an unknown barrier refuses. None: the barriers could not
/// be read (no list or no array).
pub fn barriers_locked(
    sources: &mut [Source; SOURCE_LIMIT],
    stats: &mut [u64; 16],
    list: u64,
    barriers: Option<&[Barrier]>,
    affects: Affects<'_>,
) {
    use crate::target_handoff::failure::UNKNOWN_BARRIER;
    count(stats, 6);
    let Some(barriers) = barriers.filter(|_| list != 0) else {
        unknown_locked(sources, list, exit_kind::UNKNOWN);
        return;
    };
    let null = core::ptr::null_mut();
    for barrier in barriers {
        let kind = match barrier {
            Barrier::Transition { .. } => exit_kind::TRANSITION,
            Barrier::Aliasing { .. } => exit_kind::ALIASING,
            Barrier::Uav { .. } => exit_kind::UAV,
            Barrier::Other => exit_kind::UNKNOWN,
        };
        if matches!(barrier, Barrier::Aliasing { .. }) {
            count(stats, 8);
        }
        for source in sources.iter_mut() {
            if !(source.occupied && source.state.flags & source::CAPTURED != 0) {
                continue;
            }
            if source.capturing {
                if foreign_recording(source, list) {
                    source.state.foreign(list, kind);
                }
                continue;
            }
            if !source.state.tracking() && !source.retired_recording && source.state.list == list {
                continue;
            }
            for i in 0..live(source) {
                let resource = target_resource(&source.targets[i]);
                match *barrier {
                    Barrier::Transition { resource: named, subresource, before, after, flags } => {
                        if named.is_null() {
                            unknown_source(source, list, exit_kind::TRANSITION, UNKNOWN_BARRIER);
                            continue;
                        }
                        if named != resource {
                            if affects(&source.targets[i], named, null) {
                                touch(source, list, false, true, exit_kind::TRANSITION);
                                if !foreign_recording(source, list) {
                                    let why = source.state.failure;
                                    reject(source, why);
                                }
                            }
                            continue;
                        }
                        touch(source, list, false, false, exit_kind::TRANSITION);
                        if foreign_recording(source, list) {
                            continue;
                        }
                        if source.state.flags & source::REFUSED != 0 {
                            let why = source.state.failure;
                            reject(source, why);
                            continue;
                        }
                        if source.targets[i].state.transition(subresource, before, after, flags) {
                            count(stats, 7);
                        } else {
                            let why = source.targets[i].state.failure;
                            reject(source, why);
                        }
                    }
                    Barrier::Aliasing { before, after } => {
                        if affects(&source.targets[i], before, after) {
                            touch(source, list, false, true, exit_kind::ALIASING);
                            if !foreign_recording(source, list) {
                                source.targets[i].state.alias(true);
                                let why = source.state.failure;
                                reject(source, why);
                            }
                        }
                    }
                    Barrier::Uav { resource: named } => {
                        if named.is_null() || affects(&source.targets[i], named, null) {
                            touch(source, list, true, false, exit_kind::UAV);
                            if !foreign_recording(source, list) {
                                reject(source, UNKNOWN_BARRIER);
                            }
                        }
                    }
                    Barrier::Other => unknown_source(source, list, exit_kind::UNKNOWN, UNKNOWN_BARRIER),
                }
            }
        }
    }
}

// ---- their C ABI

/// The plugin's alias check over a target's resource.
#[cfg(not(test))]
fn plugin_affects(target: &Target, before: *mut core::ffi::c_void, after: *mut core::ffi::c_void) -> bool {
    crate::texture_receipts::alias_affects_resources(target_resource(target), before, after)
}
#[cfg(not(test))]
unsafe fn sources_of<'a>(sources: *mut Source) -> Option<&'a mut [Source; SOURCE_LIMIT]> {
    // SAFETY: the C++ half's array, under its lock.
    unsafe { sources.cast::<[Source; SOURCE_LIMIT]>().as_mut() }
}
/// # Safety
/// `source` is one of the C++ half's sources, under its lock.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_reject(source: *mut Source, why: u32) {
    // SAFETY: as the caller promises.
    if let Some(source) = unsafe { source.as_mut() } {
        reject(source, why);
    }
}
/// # Safety
/// `sources` is the C++ half's array, under its lock.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_unknown_locked(sources: *mut Source, list: u64, kind: u32) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        unknown_locked(sources, list, kind);
    }
}
/// # Safety
/// As cvr_th_unknown_locked; `destination` a live resource or null.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_write_locked(sources: *mut Source, list: u64, destination: *mut core::ffi::c_void, kind: u32) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        write_locked(sources, list, destination, kind, &plugin_affects);
    }
}
/// # Safety
/// As cvr_th_unknown_locked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_shader_draw_locked(sources: *mut Source, list: u64) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        shader_draw_locked(sources, list);
    }
}
/// Barriers' loop (the C++ half took the lock, accepted the event and opened the publication; it publishes after).
///
/// # Safety
/// As cvr_th_unknown_locked; `barriers` holds `count` barriers or is null.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_barriers_locked(
    sources: *mut Source,
    list: u64,
    count_of: u32,
    barriers: *const windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_BARRIER,
) {
    use windows::Win32::Graphics::Direct3D12::*;
    let Some(sources) = (unsafe { sources_of(sources) }) else { return };
    let raw = |p: &core::mem::ManuallyDrop<Option<ID3D12Resource>>| -> *mut core::ffi::c_void {
        p.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
    };
    // SAFETY: the caller's array; each barrier read by its type's union member.
    let read: Option<Vec<Barrier>> = (!barriers.is_null()).then(|| unsafe {
        core::slice::from_raw_parts(barriers, count_of as usize)
            .iter()
            .map(|b| match b.Type {
                D3D12_RESOURCE_BARRIER_TYPE_TRANSITION => {
                    let t = &b.Anonymous.Transition;
                    Barrier::Transition {
                        resource: raw(&t.pResource),
                        subresource: t.Subresource,
                        before: t.StateBefore.0 as u32,
                        after: t.StateAfter.0 as u32,
                        flags: b.Flags.0 as u32,
                    }
                }
                D3D12_RESOURCE_BARRIER_TYPE_ALIASING => {
                    let a = &b.Anonymous.Aliasing;
                    Barrier::Aliasing { before: raw(&a.pResourceBefore), after: raw(&a.pResourceAfter) }
                }
                D3D12_RESOURCE_BARRIER_TYPE_UAV => Barrier::Uav { resource: raw(&b.Anonymous.UAV.pResource) },
                _ => Barrier::Other,
            })
            .collect()
    });
    // SAFETY: the stats row, written under the lock.
    let stats = unsafe { &mut *(&raw mut CyberpunkVR_TargetHandoffStats) };
    barriers_locked(sources, stats, list, read.as_deref(), &plugin_affects);
}

#[cfg(test)]
mod event_tests {
    //! The ledger's events: writes over an aliasing watched target, a transition of a watched one, a UAV barrier over
    //! all memory, foreign lists, the capture in progress and the unreadable barriers.
    use super::*;
    use crate::target_handoff::{failure, RecordingState};

    const LIST: u64 = 0x1000;
    const OTHER_LIST: u64 = 0x2000;
    const RESOURCE: usize = 0xAB00;

    /// One captured, closed-and-exited-free source on side 0 with two targets; target 0 names RESOURCE.
    fn sources() -> Box<[Source; SOURCE_LIMIT]> {
        // SAFETY: every field is valid zeroed.
        let mut all: Box<[Source; SOURCE_LIMIT]> = unsafe { Box::new(core::mem::zeroed()) };
        let s = &mut all[0];
        s.occupied = true;
        s.target_count = 2;
        s.state = RecordingState::default();
        assert!(s.state.begin(LIST, 1));
        s.state.flags |= source::CAPTURED;
        for (i, t) in s.targets.iter_mut().take(2).enumerate() {
            assert!(t.state.seed(1, 4), "target {i} seeded as a render target");
        }
        // SAFETY: a fake interface pointer, never called and never released (forgotten in `done`).
        s.targets[0].identity.creation.resource = Some(unsafe { core::mem::transmute::<usize, _>(RESOURCE) });
        all
    }
    fn done(mut all: Box<[Source; SOURCE_LIMIT]>) {
        core::mem::forget(all[0].targets[0].identity.creation.resource.take());
    }
    fn none(_: &Target, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void) -> bool {
        false
    }
    fn overlaps_target_one(t: &Target, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void) -> bool {
        t.slot == 1
    }

    #[test]
    fn a_transition_of_a_watched_target_is_applied_and_counted() {
        let mut all = sources();
        let mut stats = [0u64; 16];
        let t = Barrier::Transition { resource: RESOURCE as *mut _, subresource: u32::MAX, before: 4, after: 0x80, flags: 0 };
        barriers_locked(&mut all, &mut stats, LIST, Some(&[t]), &none);
        assert_eq!((stats[6], stats[7], stats[8]), (1, 1, 0));
        assert_eq!(all[0].targets[0].state.states[0].current, 0x80);
        assert_eq!(all[0].state.failure, failure::NONE);
        done(all);
    }

    #[test]
    fn a_write_over_an_aliasing_target_refuses_the_source() {
        let mut all = sources();
        all[0].targets[1].slot = 1;
        write_locked(&mut all, LIST, 0xCD00 as *mut _, exit_kind::OTHER, &overlaps_target_one);
        assert_eq!(all[0].targets[1].state.failure, failure::ALIASING);
        assert!(all[0].state.flags & source::REFUSED != 0, "the source refused with its targets");
        assert_eq!(all[0].targets[0].state.failure, all[0].state.failure);
        done(all);
    }

    #[test]
    fn foreign_lists_and_captures_are_uncertainty_not_refusal() {
        let mut all = sources();
        let mut stats = [0u64; 16];
        barriers_locked(&mut all, &mut stats, OTHER_LIST, Some(&[Barrier::Uav { resource: core::ptr::null_mut() }]), &none);
        assert_eq!(all[0].state.foreign_list, OTHER_LIST);
        assert_eq!(all[0].state.failure, failure::NONE);
        // The same list's UAV barrier over all memory refuses.
        barriers_locked(&mut all, &mut stats, LIST, Some(&[Barrier::Uav { resource: core::ptr::null_mut() }]), &none);
        assert_eq!(all[0].state.failure, failure::UNKNOWN_BARRIER);
        // While capturing, only a foreign list is noted; the unreadable barriers are an unknown command.
        let mut all2 = sources();
        all2[0].capturing = true;
        barriers_locked(&mut all2, &mut stats, LIST, None, &none);
        assert_eq!(stats[6], 3);
        assert_eq!(all2[0].state.failure, failure::UNKNOWN_COMMAND);
        done(all);
        done(all2);
    }

    #[test]
    fn counts_saturate() {
        let mut stats = [u64::MAX; 16];
        count(&mut stats, 6);
        assert_eq!(stats[6], u64::MAX);
    }
}

// ---- the recordings' lives (Event, MarkExit, Close, Reset, BeforeSubmit, Submitted), under the observer lock

/// Event: one more event in the observation window (none once it finished), and a capture whose own list is touched
/// marked so.
pub fn event(window: &mut crate::target_handoff::ObservationWindow, sources: &mut [Source; SOURCE_LIMIT], list: u64) -> bool {
    if !window.event() {
        return false;
    }
    for source in sources.iter_mut() {
        if source.capturing && source.state.list == list {
            source.capture_touched = true;
        }
    }
    true
}
/// MarkExit's loop: the list's tracking sources leave the native pass (once).
pub fn mark_exit_locked(sources: &mut [Source; SOURCE_LIMIT], stats: &mut [u64; 16], list: u64, reason: u32, kind: u32) {
    for source in sources.iter_mut() {
        if source.occupied && source.state.list == list && source.state.tracking() && source.state.flags & source::EXITED == 0 {
            source.state.exit(reason, kind);
            count(stats, 10);
        }
    }
}
/// Close's loop: the list's tracking sources closed (counted by the result); a successful close freezes the targets'
/// ledgers, a refused one refuses them.
pub fn close_locked(sources: &mut [Source; SOURCE_LIMIT], stats: &mut [u64; 16], list: u64, result: i32) {
    for source in sources.iter_mut() {
        if source.occupied && source.state.list == list && source.state.tracking() {
            count(stats, if result >= 0 { 11 } else { 12 });
            if source.state.close(result) {
                let n = live(source);
                for target in &mut source.targets[..n] {
                    target.state.freeze();
                }
            } else if source.state.flags & source::REFUSED != 0 {
                let why = source.state.failure;
                reject(source, why);
            }
        }
    }
}
/// Reset's loop over the sources: a submitted recording retires; one still tracking is reset before its submission
/// -- refused, counted, retired.
pub fn reset_locked(sources: &mut [Source; SOURCE_LIMIT], stats: &mut [u64; 16], list: u64) {
    for source in sources.iter_mut() {
        if source.occupied && source.state.list == list {
            if source.state.flags & source::SUBMITTED_OBSERVED != 0 {
                source.retired_recording = true;
            } else if source.state.tracking() {
                source.state.reset();
                reject(source, crate::target_handoff::failure::RESET_BEFORE_SUBMIT);
                count(stats, 13);
                source.retired_recording = true;
            }
        }
    }
}
fn mark_captures_touched(sources: &mut [Source; SOURCE_LIMIT], lists: &[u64]) {
    for source in sources.iter_mut() {
        if source.capturing && lists.iter().any(|&l| l == source.state.list) {
            source.capture_touched = true;
        }
    }
}
/// BeforeSubmit's loops: captures whose list is submitted marked touched; each captured source in the batch -- a
/// retired one foreign (counted) -- validated: its recording's submission and its targets' identities on this queue
/// (`validate`, the C++ half's, in that order), every target unrefused with all subresources frozen; the pending
/// submission recorded on the queue with its occurrences; a refusal or foreign uncertainty counted.
pub fn before_submit_locked(
    sources: &mut [Source; SOURCE_LIMIT],
    stats: &mut [u64; 16],
    queue: u64,
    lists: &[u64],
    validate: &mut dyn FnMut(&mut Source) -> bool,
) {
    use crate::target_handoff::{failure, state};
    mark_captures_touched(sources, lists);
    for source in sources.iter_mut() {
        if !source.occupied || source.capturing {
            continue;
        }
        let occurrences = lists.iter().filter(|&&l| l == source.state.list).count() as u32;
        if occurrences == 0 {
            continue;
        }
        if source.retired_recording {
            let list = source.state.list;
            source.state.foreign(list, exit_kind::OTHER);
            count(stats, 15);
            continue;
        }
        let mut valid = validate(source);
        for target in &source.targets[..live(source)] {
            valid = target.state.failure == failure::NONE && valid;
            let n = (target.state.count as usize).min(STATES_PER_SOURCE);
            for value in &target.state.states[..n] {
                valid = value.flags & state::FROZEN_KNOWN != 0 && valid;
            }
        }
        let local_valid = source.state.pending(queue, occurrences, valid);
        if !local_valid || source.state.foreign_events != 0 {
            if !local_valid {
                let why = source.state.failure;
                reject(source, why);
            }
            count(stats, 15);
        }
    }
}
/// Submitted's loops: captures whose list was submitted marked touched; each live source's recording observed
/// submitted on the queue, retired once it is.
pub fn submitted_locked(sources: &mut [Source; SOURCE_LIMIT], queue: u64, lists: &[u64]) {
    mark_captures_touched(sources, lists);
    for source in sources.iter_mut() {
        if source.occupied && !source.retired_recording && lists.iter().any(|&l| l == source.state.list) {
            source.state.submitted(queue);
            if source.state.flags & source::SUBMITTED_OBSERVED != 0 {
                source.retired_recording = true;
            }
        }
    }
}

// ---- their C ABI

#[cfg(not(test))]
unsafe fn lists_of<'a>(count_of: u32, lists: *const *mut core::ffi::c_void) -> Vec<u64> {
    // SAFETY: the caller's array of `count_of` lists.
    unsafe { core::slice::from_raw_parts(lists, count_of as usize).iter().map(|&l| l as u64).collect() }
}
#[cfg(not(test))]
fn stats_row() -> &'static mut [u64; 16] {
    // SAFETY: the stats row, written only under the observer lock the caller holds.
    unsafe { &mut *(&raw mut CyberpunkVR_TargetHandoffStats) }
}
/// # Safety
/// `window` and `sources` are the C++ half's, under its lock.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_event(window: *mut crate::target_handoff::ObservationWindow, sources: *mut Source, list: u64) -> bool {
    // SAFETY: as the caller promises.
    match unsafe { (window.as_mut(), sources_of(sources)) } {
        (Some(window), Some(sources)) => event(window, sources, list),
        _ => false,
    }
}
/// # Safety
/// As cvr_th_unknown_locked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_mark_exit_locked(sources: *mut Source, list: u64, reason: u32, kind: u32) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        mark_exit_locked(sources, stats_row(), list, reason, kind);
    }
}
/// # Safety
/// As cvr_th_unknown_locked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_close_locked(sources: *mut Source, list: u64, result: i32) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        close_locked(sources, stats_row(), list, result);
    }
}
/// # Safety
/// As cvr_th_unknown_locked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_reset_locked(sources: *mut Source, list: u64) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        reset_locked(sources, stats_row(), list);
    }
}
/// # Safety
/// As cvr_th_unknown_locked; `lists` holds `count_of` lists; `validate` the C++ half's check of one source on `queue`.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_before_submit_locked(
    sources: *mut Source,
    queue: *mut core::ffi::c_void,
    count_of: u32,
    lists: *const *mut core::ffi::c_void,
    validate: unsafe extern "C-unwind" fn(source: *mut Source, queue: *mut core::ffi::c_void) -> bool,
) {
    let Some(sources) = (unsafe { sources_of(sources) }) else { return };
    let lists = unsafe { lists_of(count_of, lists) };
    // SAFETY: the C++ half's check, on its own source.
    let mut check = |source: &mut Source| unsafe { validate(source, queue) };
    before_submit_locked(sources, stats_row(), queue as u64, &lists, &mut check);
}
/// # Safety
/// As cvr_th_unknown_locked; `lists` holds `count_of` lists.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_submitted_locked(sources: *mut Source, queue: u64, count_of: u32, lists: *const *mut core::ffi::c_void) {
    let Some(sources) = (unsafe { sources_of(sources) }) else { return };
    let lists = unsafe { lists_of(count_of, lists) };
    submitted_locked(sources, queue, &lists);
}

#[cfg(test)]
mod life_tests {
    //! A recording's life: exit, close (frozen), the submission validated pending, observed submitted and retired; a
    //! reset before the submission; a retired recording resubmitted.
    use super::*;
    use crate::target_handoff::{failure, state, ObservationWindow, RecordingState};

    const LIST: u64 = 0x1000;
    const QUEUE: u64 = 0x7000;

    fn sources() -> Box<[Source; SOURCE_LIMIT]> {
        // SAFETY: every field is valid zeroed.
        let mut all: Box<[Source; SOURCE_LIMIT]> = unsafe { Box::new(core::mem::zeroed()) };
        let s = &mut all[1];
        s.occupied = true;
        s.target_count = 1;
        s.state = RecordingState::default();
        assert!(s.state.begin(LIST, 3));
        assert!(s.targets[0].state.seed(1, 4));
        all
    }

    #[test]
    fn exit_close_pending_submitted() {
        let mut all = sources();
        let mut stats = [0u64; 16];
        mark_exit_locked(&mut all, &mut stats, LIST, 7, exit_kind::OTHER);
        mark_exit_locked(&mut all, &mut stats, LIST, 8, exit_kind::OTHER);
        assert_eq!((stats[10], all[1].state.exit_reason), (1, 7), "one exit, the first");
        close_locked(&mut all, &mut stats, LIST, 0);
        assert_eq!(stats[11], 1);
        assert!(all[1].targets[0].state.states[0].flags & state::FROZEN_KNOWN != 0, "frozen at the close");
        let mut calls = 0;
        before_submit_locked(&mut all, &mut stats, QUEUE, &[LIST, 0x9999], &mut |_| {
            calls += 1;
            true
        });
        assert_eq!((calls, all[1].state.queue, stats[15]), (1, QUEUE, 0));
        assert!(all[1].state.flags & source::PENDING_VALIDATED != 0);
        submitted_locked(&mut all, QUEUE, &[LIST]);
        assert!(all[1].retired_recording && all[1].state.flags & source::SUBMITTED_OBSERVED != 0);
        // Resubmitted after retiring: foreign, counted, not validated again.
        before_submit_locked(&mut all, &mut stats, QUEUE, &[LIST], &mut |_| panic!("not validated"));
        assert_eq!((stats[15], all[1].state.foreign_list), (1, LIST));
    }

    #[test]
    fn a_reset_before_the_submission_refuses_and_retires() {
        let mut all = sources();
        let mut stats = [0u64; 16];
        reset_locked(&mut all, &mut stats, LIST);
        assert_eq!((stats[13], all[1].state.failure), (1, failure::RESET_BEFORE_SUBMIT));
        assert!(all[1].retired_recording && all[1].targets[0].state.failure == failure::RESET_BEFORE_SUBMIT);
    }

    #[test]
    fn an_unfrozen_target_fails_the_pending_validation() {
        let mut all = sources();
        let mut stats = [0u64; 16];
        // Exited and closed, but one subresource's frozen state is not known.
        mark_exit_locked(&mut all, &mut stats, LIST, 1, exit_kind::OTHER);
        close_locked(&mut all, &mut stats, LIST, 0);
        all[1].targets[0].state.states[0].flags &= !state::FROZEN_KNOWN;
        before_submit_locked(&mut all, &mut stats, QUEUE, &[LIST], &mut |_| true);
        assert_eq!(stats[15], 1);
        assert!(all[1].state.flags & source::REFUSED != 0);
    }

    #[test]
    fn events_mark_a_touched_capture() {
        let mut all = sources();
        all[1].capturing = true;
        let mut window = ObservationWindow::default();
        assert!(!event(&mut window, &mut all, LIST), "no event before the window opens");
        assert!(window.begin());
        assert!(event(&mut window, &mut all, LIST));
        assert!(all[1].capture_touched);
        submitted_locked(&mut all, QUEUE, &[LIST]);
        assert!(!all[1].retired_recording, "a capture is not submitted");
    }
}

// ---- the target bindings, owned here (formerly the anonymous namespace's BoundTargets array): the resources each
// list last bound for writing (eight colours and the depth) and whether they are known; 64 lists at most -- one more
// is an overflow, and every later draw is then unknown.

pub const BINDING_LIMIT: usize = 64;
pub const BOUND_RESOURCES: usize = 9;
#[derive(Default)]
pub struct Bound {
    pub list: u64,
    pub resources: [Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource>; BOUND_RESOURCES],
    pub known: bool,
}
pub struct Bindings {
    slots: Vec<Bound>,
    pub overflow: bool,
}
impl Default for Bindings {
    fn default() -> Self {
        Bindings { slots: (0..BINDING_LIMIT).map(|_| Bound::default()).collect(), overflow: false }
    }
}
impl Bindings {
    /// Binding(list, create): the list's slot, or (creating) the first free one taken for it; none free is the
    /// overflow.
    pub fn slot(&mut self, list: u64, create: bool) -> Option<&mut Bound> {
        if list == 0 {
            return None;
        }
        let mut empty = None;
        for (i, bound) in self.slots.iter().enumerate() {
            if bound.list == list {
                return Some(&mut self.slots[i]);
            }
            if bound.list == 0 && empty.is_none() {
                empty = Some(i);
            }
        }
        if !create {
            return None;
        }
        let Some(i) = empty else {
            self.overflow = true;
            return None;
        };
        self.slots[i].list = list;
        Some(&mut self.slots[i])
    }
    /// The bindings forgotten (the observation finished or cleared).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
fn bound_resource(resource: &Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource>) -> *mut core::ffi::c_void {
    resource.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
}
/// Draw's locked part: the shader draw, then the list's bound targets written -- unknown without a known binding or
/// after an overflow.
pub fn draw_locked(sources: &mut [Source; SOURCE_LIMIT], bindings: &mut Bindings, list: u64, affects: Affects<'_>) {
    shader_draw_locked(sources, list);
    let overflow = bindings.overflow;
    match bindings.slot(list, false) {
        Some(bound) if bound.known && !overflow => {
            for resource in bound.resources.iter().filter(|r| r.is_some()) {
                write_locked(sources, list, bound_resource(resource), exit_kind::OTHER, affects);
            }
        }
        _ => unknown_locked(sources, list, exit_kind::OTHER),
    }
}
/// ClearView's locked part: the cleared resources written (unknown when they could not be read, or when none were).
pub fn writes_locked(sources: &mut [Source; SOURCE_LIMIT], list: u64, known: bool, resources: &[*mut core::ffi::c_void], kind: u32, affects: Affects<'_>) {
    if !known {
        unknown_locked(sources, list, kind);
        return;
    }
    let mut any = false;
    for &resource in resources.iter().filter(|r| !r.is_null()) {
        any = true;
        write_locked(sources, list, resource, kind, affects);
    }
    if !any {
        unknown_locked(sources, list, kind);
    }
}
/// DrawTargets' locked part: the shader draw, then the draw's colour targets and its depth written (more than four
/// colours, or none at all, is unknown).
pub fn draw_targets_locked(sources: &mut [Source; SOURCE_LIMIT], list: u64, colors: u32, resources: &[*mut core::ffi::c_void; 5], affects: Affects<'_>) {
    shader_draw_locked(sources, list);
    if colors > 4 {
        unknown_locked(sources, list, exit_kind::OTHER);
        return;
    }
    let mut any = false;
    for (slot, &resource) in resources.iter().enumerate() {
        if (slot as u32) < colors || (slot == 4 && !resource.is_null()) {
            any = true;
            write_locked(sources, list, resource, exit_kind::OTHER, affects);
        }
    }
    if !any {
        unknown_locked(sources, list, exit_kind::OTHER);
    }
}

// ---- their C ABI

#[cfg(not(test))]
static BINDINGS: std::sync::LazyLock<std::sync::Mutex<Bindings>> = std::sync::LazyLock::new(Default::default);
#[cfg(not(test))]
fn bindings() -> std::sync::MutexGuard<'static, Bindings> {
    BINDINGS.lock().unwrap_or_else(|p| p.into_inner())
}
/// A list's bound resources kept (a reference taken on each): Targets, or Begin's three colours and depth. False when
/// no slot was free (the overflow).
///
/// # Safety
/// `resources` holds nine live resources or nulls; under the observer lock.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_bind_targets(list: u64, resources: *const *mut core::ffi::c_void, known: bool) -> bool {
    let mut all = bindings();
    let Some(bound) = all.slot(list, true) else { return false };
    for i in 0..BOUND_RESOURCES {
        // SAFETY: the caller's array; a reference taken on each live resource.
        bound.resources[i] = unsafe {
            let raw = *resources.add(i);
            windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&raw).cloned()
        };
    }
    bound.known = known;
    true
}
/// Reset: the list's binding forgotten.
#[cfg(not(test))]
#[no_mangle]
pub extern "C-unwind" fn cvr_th_unbind(list: u64) {
    if let Some(bound) = bindings().slot(list, false) {
        *bound = Bound::default();
    }
}
#[cfg(not(test))]
#[no_mangle]
pub extern "C-unwind" fn cvr_th_binding_overflow() -> bool {
    bindings().overflow
}
#[cfg(not(test))]
#[no_mangle]
pub extern "C-unwind" fn cvr_th_clear_bindings() {
    bindings().clear();
}
/// # Safety
/// As cvr_th_unknown_locked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_draw_locked(sources: *mut Source, list: u64) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        count(stats_row(), 9);
        draw_locked(sources, &mut bindings(), list, &plugin_affects);
    }
}
/// # Safety
/// As cvr_th_unknown_locked; `resources` holds nine resources or nulls.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_clear_locked(sources: *mut Source, list: u64, known: bool, resources: *const *mut core::ffi::c_void) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        count(stats_row(), 9);
        // SAFETY: the caller's array of nine.
        let resources = unsafe { core::slice::from_raw_parts(resources, BOUND_RESOURCES) };
        writes_locked(sources, list, known, resources, exit_kind::CLEAR, &plugin_affects);
    }
}
/// # Safety
/// As cvr_th_unknown_locked; `resources` holds five resources or nulls.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_draw_targets_locked(sources: *mut Source, list: u64, colors: u32, resources: *const [*mut core::ffi::c_void; 5]) {
    if let (Some(sources), Some(resources)) = unsafe { (sources_of(sources), resources.as_ref()) } {
        count(stats_row(), 9);
        draw_targets_locked(sources, list, colors, resources, &plugin_affects);
    }
}
/// # Safety
/// As cvr_th_write_locked.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_written_locked(sources: *mut Source, list: u64, destination: *mut core::ffi::c_void, kind: u32) {
    if let Some(sources) = unsafe { sources_of(sources) } {
        count(stats_row(), 9);
        write_locked(sources, list, destination, kind, &plugin_affects);
    }
}

#[cfg(test)]
mod binding_tests {
    //! The slots, their overflow, and the draws over them.
    use super::*;
    use crate::target_handoff::{failure, RecordingState};

    const LIST: u64 = 0x1000;

    fn sources() -> Box<[Source; SOURCE_LIMIT]> {
        // SAFETY: every field is valid zeroed.
        let mut all: Box<[Source; SOURCE_LIMIT]> = unsafe { Box::new(core::mem::zeroed()) };
        let s = &mut all[0];
        s.occupied = true;
        s.target_count = 1;
        s.state = RecordingState::default();
        assert!(s.state.begin(LIST, 1));
        assert!(s.targets[0].state.seed(1, 4));
        all
    }
    fn every(_: &Target, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void) -> bool {
        true
    }

    #[test]
    fn slots_found_taken_and_overflowing() {
        let mut b = Bindings::default();
        assert!(b.slot(0, true).is_none(), "no list, no slot");
        assert!(b.slot(LIST, false).is_none());
        b.slot(LIST, true).unwrap().known = true;
        assert!(b.slot(LIST, false).unwrap().known);
        for list in 1..BINDING_LIMIT as u64 {
            assert!(b.slot(0x10_0000 + list, true).is_some());
        }
        assert!(!b.overflow);
        assert!(b.slot(0x20_0000, true).is_none());
        assert!(b.overflow);
        // An existing list is still found after the overflow.
        assert!(b.slot(LIST, false).is_some());
        b.clear();
        assert!(!b.overflow && b.slot(LIST, false).is_none());
    }

    #[test]
    fn a_draw_without_a_known_binding_is_unknown() {
        let mut all = sources();
        let mut b = Bindings::default();
        draw_locked(&mut all, &mut b, LIST, &every);
        assert_eq!(all[0].state.failure, failure::UNKNOWN_COMMAND);
        // A binding the list has, but whose targets could not be read: unknown too.
        let mut all = sources();
        assert!(!b.slot(LIST, true).unwrap().known);
        draw_locked(&mut all, &mut b, LIST, &every);
        assert_eq!(all[0].state.failure, failure::UNKNOWN_COMMAND);
    }

    #[test]
    fn a_draw_writes_its_bound_targets() {
        let mut all = sources();
        let mut b = Bindings::default();
        let bound = b.slot(LIST, true).unwrap();
        // SAFETY: a fake interface pointer, never called and never released (forgotten below).
        bound.resources[8] = Some(unsafe { core::mem::transmute::<usize, _>(0xD000usize) });
        bound.known = true;
        draw_locked(&mut all, &mut b, LIST, &every);
        // The bound depth aliases the watched target (a different resource over its memory).
        assert_eq!(all[0].targets[0].state.failure, failure::ALIASING);
        core::mem::forget(b.slot(LIST, false).unwrap().resources[8].take());
    }

    #[test]
    fn draw_targets_beyond_four_colours_or_none_are_unknown() {
        let mut all = sources();
        draw_targets_locked(&mut all, LIST, 5, &[core::ptr::null_mut(); 5], &every);
        assert_eq!(all[0].state.failure, failure::UNKNOWN_COMMAND);
        let mut all = sources();
        draw_targets_locked(&mut all, LIST, 0, &[core::ptr::null_mut(); 5], &every);
        assert_eq!(all[0].state.failure, failure::UNKNOWN_COMMAND);
        // A depth only: written.
        let mut all = sources();
        let mut r = [core::ptr::null_mut(); 5];
        r[4] = 0xE000 as *mut _;
        draw_targets_locked(&mut all, LIST, 0, &r, &every);
        assert_eq!(all[0].state.failure, failure::ALIASING);
    }
}

// ---- the capture's merge into its side (Begin) and the sources' snapshot check (SnapshotSources)

/// native_recording::Lease equality as the observer compares recordings (a live token).
pub fn same_recording(a: &Lease, b: &Lease) -> bool {
    !a.token.is_null()
        && a.token == b.token
        && a.generation == b.generation
        && a.context == b.context
        && a.list == b.list
        && a.registry == b.registry
}
const CAPTURE_RACE: u32 = 6;
/// Begin's merge, before the captured source replaces its side's: a capture the side's own list touched meanwhile
/// races, a binding overflow is an unknown command, the side's refusal carries over, and so does its foreign
/// uncertainty; anything but four targets with five states is the wrong shape.
pub fn begin_merge(current: &Source, captured: &mut Source, binding_overflow: bool) {
    use crate::target_handoff::failure;
    if current.capture_touched {
        reject(captured, CAPTURE_RACE);
    }
    if binding_overflow {
        reject(captured, failure::UNKNOWN_COMMAND);
    }
    if current.state.flags & source::REFUSED != 0 {
        reject(captured, current.state.failure);
    }
    captured.state.foreign_failure = current.state.foreign_failure;
    captured.state.foreign_list = current.state.foreign_list;
    captured.state.foreign_kind = current.state.foreign_kind;
    captured.state.foreign_events = current.state.foreign_events;
    if captured.target_count as usize != TARGETS_PER_SOURCE || captured.state_count as usize != STATES_PER_SOURCE {
        reject(captured, failure::SHAPE);
    }
}
/// SnapshotSources' check of one side: captured on this frame and side, not capturing, retired, refused, reset or
/// already submitted; exited, closed and pending-validated on this queue with no failure; the full four targets with
/// five states, its lease the recording's; each target unrefused with one or two planes all current and frozen (none
/// refused) and its identity's recording the source's. The frozen states per target and plane when it passes.
pub fn snapshot_check(source: &Source, side: u32, frame: u32, queue: u64) -> Option<[[u32; 2]; TARGETS_PER_SOURCE]> {
    use crate::target_handoff::{failure, state};
    let s = &source.state;
    let required = source::CAPTURED | source::EXITED | source::CLOSE_SUCCEEDED | source::PENDING_VALIDATED;
    if !source.occupied
        || source.capturing
        || source.retired_recording
        || source.frame != frame
        || source.side != side
        || s.failure != failure::NONE
        || s.flags & required != required
        || s.flags & (source::REFUSED | source::RESET_BEFORE_SUBMIT | source::SUBMITTED_OBSERVED) != 0
        || s.queue != queue
        || source.target_count as usize != TARGETS_PER_SOURCE
        || source.state_count as usize != STATES_PER_SOURCE
        || source.recording.list != s.list
        || source.recording.generation != s.generation
    {
        return None;
    }
    let mut finals = [[0u32; 2]; TARGETS_PER_SOURCE];
    for (i, target) in source.targets.iter().enumerate() {
        if target.state.failure != failure::NONE || target.planes != target.state.count || target.planes == 0 || target.planes > 2 {
            return None;
        }
        if !same_recording(&source.recording, &target.identity.recording) {
            return None;
        }
        for plane in 0..target.planes as usize {
            let value = &target.state.states[plane];
            let known = state::CURRENT_KNOWN | state::FROZEN_KNOWN;
            if value.flags & (known | state::STATE_REFUSED) != known {
                return None;
            }
            finals[i][plane] = value.frozen;
        }
    }
    Some(finals)
}

/// # Safety
/// `current` is its side's source and `captured` the C++ half's capture of it, under the observer lock.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_begin_merge(current: *const Source, captured: *mut Source) {
    // SAFETY: as the caller promises.
    if let (Some(current), Some(captured)) = unsafe { (current.as_ref(), captured.as_mut()) } {
        let overflow = bindings().overflow;
        begin_merge(current, captured, overflow);
    }
}
/// # Safety
/// `source` is one of the C++ half's sources, under its lock; `finals` holds four pairs.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_snapshot_check(
    source: *const Source,
    side: u32,
    frame: u32,
    queue: u64,
    finals: *mut [[u32; 2]; TARGETS_PER_SOURCE],
) -> bool {
    // SAFETY: as the caller promises.
    let (Some(source), Some(out)) = (unsafe { source.as_ref() }, unsafe { finals.as_mut() }) else { return false };
    match snapshot_check(source, side, frame, queue) {
        Some(states) => {
            *out = states;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod snapshot_tests {
    //! A source through its whole life is snapshotted with its frozen states; each missing step refuses; the merge.
    use super::*;
    use crate::target_handoff::{failure, RecordingState};

    const LIST: u64 = 0x1000;
    const QUEUE: u64 = 0x7000;
    const TOKEN: usize = 0x5000;

    fn submitted_source() -> Box<[Source; SOURCE_LIMIT]> {
        // SAFETY: every field is valid zeroed.
        let mut all: Box<[Source; SOURCE_LIMIT]> = unsafe { Box::new(core::mem::zeroed()) };
        let s = &mut all[1];
        s.occupied = true;
        s.frame = 40;
        s.side = 1;
        s.target_count = 4;
        s.state_count = 5;
        s.state = RecordingState::default();
        assert!(s.state.begin(LIST, 2));
        s.recording.token = TOKEN as *mut _;
        s.recording.list = LIST;
        s.recording.generation = 2;
        for (i, t) in s.targets.iter_mut().enumerate() {
            let planes = if i == 3 { 2 } else { 1 };
            assert!(t.state.seed(planes, if i == 3 { 0x10 } else { 4 }));
            t.planes = planes;
            t.identity.recording.token = TOKEN as *mut _;
            t.identity.recording.list = LIST;
            t.identity.recording.generation = 2;
        }
        let mut stats = [0u64; 16];
        mark_exit_locked(&mut all, &mut stats, LIST, 1, exit_kind::OTHER);
        close_locked(&mut all, &mut stats, LIST, 0);
        before_submit_locked(&mut all, &mut stats, QUEUE, &[LIST], &mut |_| true);
        all
    }

    #[test]
    fn a_closed_pending_source_is_snapshotted() {
        let all = submitted_source();
        let finals = snapshot_check(&all[1], 1, 40, QUEUE).expect("snapshotted");
        assert_eq!(finals, [[4, 0], [4, 0], [4, 0], [0x10, 0x10]]);
        assert!(snapshot_check(&all[1], 0, 40, QUEUE).is_none(), "the wrong side");
        assert!(snapshot_check(&all[1], 1, 41, QUEUE).is_none(), "another frame");
        assert!(snapshot_check(&all[1], 1, 40, QUEUE + 8).is_none(), "another queue");
        assert!(snapshot_check(&all[0], 0, 40, QUEUE).is_none(), "an empty side");
    }

    #[test]
    fn a_target_of_another_recording_or_unfrozen_is_not() {
        let mut all = submitted_source();
        all[1].targets[2].identity.recording.generation = 3;
        assert!(snapshot_check(&all[1], 1, 40, QUEUE).is_none());
        let mut all = submitted_source();
        all[1].targets[3].state.states[1].flags &= !crate::target_handoff::state::FROZEN_KNOWN;
        assert!(snapshot_check(&all[1], 1, 40, QUEUE).is_none());
        let mut all = submitted_source();
        submitted_locked(&mut all, QUEUE, &[LIST]);
        assert!(snapshot_check(&all[1], 1, 40, QUEUE).is_none(), "already observed submitted");
    }

    #[test]
    fn the_merge_carries_the_sides_refusal_and_uncertainty() {
        // SAFETY: valid zeroed.
        let mut sides: Box<[Source; 2]> = unsafe { Box::new(core::mem::zeroed()) };
        let (current, captured) = sides.split_at_mut(1);
        let (current, captured) = (&mut current[0], &mut captured[0]);
        current.state = RecordingState::default();
        assert!(current.state.begin(LIST, 2));
        current.state.foreign(0x2000, exit_kind::OTHER);
        current.capture_touched = true;
        captured.state = RecordingState::default();
        assert!(captured.state.begin(LIST, 2));
        captured.target_count = 4;
        captured.state_count = 5;
        begin_merge(current, captured, false);
        assert_eq!((captured.state.failure, captured.state.foreign_list), (CAPTURE_RACE, 0x2000));
        current.capture_touched = false;
        captured.state = RecordingState::default();
        assert!(captured.state.begin(LIST, 2));
        captured.state_count = 4;
        begin_merge(current, captured, true);
        assert_eq!(captured.state.failure, failure::UNKNOWN_COMMAND, "the overflow first, then the shape");
        assert!(captured.state.flags & source::REFUSED != 0);
    }
}

// ---- the observer itself (formerly src/Render/NativeTargetHandoff.cpp's anonymous namespace and its entry points): the
// two sources, the observation window and the capture epoch under one lock, the observing flag, and every publication
// bracketed by the CyberpunkVR_TargetHandoffSeq sequence (odd while it lasts).

#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub use owned::*;

#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
mod owned {
    use super::*;
    use crate::target_handoff::{failure, ObservationWindow};
    use crate::texture_writable::{cvr_tr_capture_writable_identity, cvr_tr_validate_writable_identity};
    use core::ffi::c_void;
    use core::ptr::null_mut;
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::*};
    use std::sync::{LazyLock, Mutex, MutexGuard};
    use windows::core::IUnknown;
    use windows::Win32::Graphics::Direct3D12::*;

    #[allow(non_upper_case_globals)]
    extern "C" {
        static CyberpunkVR_TargetHandoffSeq: AtomicU32;
    }

    /// Failure::Identity, Failure::Recording, Failure::DuplicateBegin.
    const IDENTITY: u32 = 3;
    const RECORDING: u32 = 4;
    const DUPLICATE_BEGIN: u32 = 7;

    struct Observer {
        sources: [Source; SOURCE_LIMIT],
        window: ObservationWindow,
        epoch: u64,
    }
    struct Held(Box<Observer>);
    // SAFETY: the sources' COM references and tokens are only touched under the lock.
    unsafe impl Send for Held {}
    static OBSERVER: LazyLock<Mutex<Held>> =
        // SAFETY: every field is valid zeroed (integers, bools, absent COM references and tokens).
        LazyLock::new(|| Mutex::new(Held(unsafe { Box::new(core::mem::zeroed()) })));
    static OBSERVING: AtomicBool = AtomicBool::new(false);

    fn lock() -> MutexGuard<'static, Held> {
        OBSERVER.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// One publication: the sequence odd from here to its drop.
    struct Publication;
    impl Publication {
        fn open() -> Self {
            // SAFETY: the C++ half's exported sequence (it lives for the process).
            unsafe { CyberpunkVR_TargetHandoffSeq.fetch_add(1, AcqRel) };
            Publication
        }
    }
    impl Drop for Publication {
        fn drop(&mut self) {
            // SAFETY: as open's.
            unsafe { CyberpunkVR_TargetHandoffSeq.fetch_add(1, Release) };
        }
    }
    fn publish(o: &Observer) {
        // SAFETY: our sources, under the lock.
        unsafe { cvr_th_publish(o.sources.as_ptr()) };
    }

    /// A reference taken on a token (an IUnknown) / one released.
    fn add_ref(token: *mut c_void) {
        // SAFETY: a live IUnknown or null; the clone's reference is kept.
        if let Some(unknown) = unsafe { IUnknown::from_raw_borrowed(&token) } {
            core::mem::forget(unknown.clone());
        }
    }
    fn release(token: *mut c_void) {
        if !token.is_null() {
            // SAFETY: a reference this side holds.
            drop(unsafe { IUnknown::from_raw(token) });
        }
    }
    /// A Lease copied (ComPtr<IUnknown>'s copy: a reference taken on the token).
    fn lease_copy(lease: &Lease) -> Lease {
        add_ref(lease.token);
        Lease { token: lease.token, generation: lease.generation, context: lease.context, list: lease.list, registry: lease.registry }
    }
    /// A source destroyed (`sources = {}`, a replaced side): its recording's token, each target identity's COM references
    /// and its recording's token released; the source zeroed.
    fn drop_source(s: &mut Source) {
        release(s.recording.token);
        for target in s.targets.iter_mut() {
            release(target.identity.recording.token);
            // SAFETY: the identity's creation dropped once (its references), then the whole source zeroed below.
            unsafe { core::ptr::drop_in_place(&mut target.identity.creation) };
        }
        // SAFETY: every field is valid zeroed.
        unsafe { core::ptr::write_bytes(s as *mut Source, 0, 1) };
    }
    /// The identity cloned (WritableTargetIdentity's copy: its COM references and its recording's token referenced again).
    fn identity_clone(value: &WritableIdentity) -> WritableIdentity {
        WritableIdentity {
            creation: value.creation.clone(),
            native: value.native,
            recording: lease_copy(&value.recording),
            original_context: value.original_context,
            resource_desc: value.resource_desc,
            heap_desc: value.heap_desc,
            resource_heap_properties: value.resource_heap_properties,
            resource_heap_flags: value.resource_heap_flags,
            expected_writable_state: value.expected_writable_state,
            native_record: value.native_record,
            allocator_owner_observation: value.allocator_owner_observation,
        }
    }
    fn identity_drop(value: WritableIdentity) {
        release(value.recording.token);
        drop(value);
    }
    /// texture_read::ValidateWritableIdentity: against its authenticated copy (the identity itself, never modified after
    /// its capture), none when it was not captured.
    fn validate_identity(value: &WritableIdentity, authenticated: bool, queue: *mut c_void) -> bool {
        let captured: *const WritableIdentity = if authenticated { value } else { core::ptr::null() };
        // SAFETY: a valid identity, its copy or null.
        unsafe { cvr_tr_validate_writable_identity(value, captured, queue) }
    }

    /// scene_state::GeometryTargets (include/Render/StereoSceneState.hpp) as the C++ lends it: the five resources, their
    /// views and descriptors, the colour count.
    #[repr(C)]
    pub struct GeometryTargets {
        pub resources: [*mut c_void; 5],
        pub views: [TargetInfo; 5],
        pub handles: [usize; 5],
        pub colors: u32,
    }
    const _: () = assert!(core::mem::size_of::<GeometryTargets>() == 328);

    /// TargetShape: the target's descriptor and view (handoff_target_shape.rs' rules), the format's planes from the
    /// device; the target's slot, planes, view and descriptor recorded and its ledger seeded.
    fn target_shape(device: Option<&ID3D12Device>, target: &mut Target, slot: usize, targets: &GeometryTargets) -> bool {
        let desc = target.identity.resource_desc;
        let view = targets.views[slot];
        let Some(device) = device else { return false };
        // SAFETY: local records.
        if !unsafe { crate::handoff_target_shape::cvr_nth_view_shape(&desc, &view, target_resource(target) as u64, targets.handles[slot]) } {
            return false;
        }
        let mut format = D3D12_FEATURE_DATA_FORMAT_INFO { Format: desc.Format, PlaneCount: 0 };
        // SAFETY: a live device; the record and its size.
        let supported = unsafe {
            device.CheckFeatureSupport(
                D3D12_FEATURE_FORMAT_INFO,
                (&raw mut format).cast(),
                core::mem::size_of::<D3D12_FEATURE_DATA_FORMAT_INFO>() as u32,
            )
        };
        let planes = u32::from(format.PlaneCount);
        // SAFETY: local records.
        if supported.is_err() || !unsafe { crate::handoff_target_shape::cvr_nth_format_shape(slot as u32, &desc, view.view_format, planes) } {
            return false;
        }
        target.slot = slot as u32;
        target.planes = planes;
        target.view = view;
        target.handle = targets.handles[slot];
        target.state.seed(planes, target.identity.expected_writable_state.0 as u32)
    }

    /// Begin: the side's source admitted (its recording, the window opened, the list's targets bound), then its four
    /// targets captured off the lock (identity, recording, shape, size, aliasing) and merged in if the window and the
    /// epoch held.
    ///
    /// # Safety
    /// `list` a live command list or null; `recording` and `targets` the caller's records.
    pub unsafe fn begin(list: *mut c_void, recording: &Lease, frame: u32, side: u32, targets: &GeometryTargets) -> bool {
        let epoch;
        {
            let mut held = lock();
            let o = &mut *held.0;
            if o.window.finished {
                return false;
            }
            let _publication = Publication::open();
            count(stats_row(), 4);
            if list.is_null() || frame == 0 || side as usize >= SOURCE_LIMIT || targets.colors != 3 || recording.token.is_null()
                || recording.list != list as u64
            {
                count(stats_row(), 5);
                return false;
            }
            let source = &mut o.sources[side as usize];
            if source.occupied {
                reject(source, DUPLICATE_BEGIN);
                count(stats_row(), 5);
                publish(o);
                return false;
            }
            source.occupied = true;
            source.capturing = true;
            source.frame = frame;
            source.side = side;
            let old = core::mem::replace(&mut source.recording, lease_copy(recording));
            release(old.token);
            o.window.begin();
            OBSERVING.store(true, Release);
            o.sources[side as usize].state.begin(list as u64, recording.generation);
            epoch = o.epoch;
            let r = &targets.resources;
            let bound: [*mut c_void; BOUND_RESOURCES] = [r[0], r[1], r[2], null_mut(), null_mut(), null_mut(), null_mut(), null_mut(), r[4]];
            // SAFETY: nine live resources or nulls, under the lock.
            unsafe { cvr_th_bind_targets(list as u64, bound.as_ptr(), true) };
        }
        // SAFETY: every field is valid zeroed.
        let mut captured: Box<Source> = unsafe { Box::new(core::mem::zeroed()) };
        captured.occupied = true;
        captured.frame = frame;
        captured.side = side;
        captured.recording = lease_copy(recording);
        captured.state.begin(list as u64, recording.generation);
        // SAFETY: a live command list (checked above).
        let device: Option<ID3D12Device> = unsafe {
            ID3D12GraphicsCommandList::from_raw_borrowed(&list).and_then(|l| {
                let mut device = None;
                l.GetDevice(&mut device).ok().and(device)
            })
        };
        if device.is_none() {
            reject(&mut captured, failure::SHAPE);
        }
        for slot in [0usize, 1, 2, 4] {
            if captured.state.flags & source::REFUSED != 0 {
                break;
            }
            let index = captured.target_count as usize;
            let expected = if slot == 4 { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
            // SAFETY: the live list, the caller's resource or null, a zeroed identity of ours.
            if !unsafe { cvr_tr_capture_writable_identity(list, targets.resources[slot], expected.0 as u32, &mut captured.targets[index].identity) } {
                reject(&mut captured, IDENTITY);
                break;
            }
            captured.targets[index].authenticated = true;
            if !same_recording(recording, &captured.targets[index].identity.recording) {
                reject(&mut captured, RECORDING);
                break;
            }
            if !target_shape(device.as_ref(), &mut captured.targets[index], slot, targets) {
                reject(&mut captured, failure::SHAPE);
                break;
            }
            let (first, this) = (&captured.targets[0].identity.resource_desc, &captured.targets[index].identity.resource_desc);
            if index != 0 && (this.Width != first.Width || this.Height != first.Height) {
                reject(&mut captured, failure::SHAPE);
                break;
            }
            captured.targets[index].state_first = side * STATES_PER_SOURCE as u32 + captured.state_count;
            captured.state_count += captured.targets[index].state.count;
            captured.target_count += 1;
            let watched = target_resource(&captured.targets[index]);
            for prior in 0..index {
                if plugin_affects(&captured.targets[prior], watched, null_mut()) {
                    reject(&mut captured, failure::ALIASING);
                    break;
                }
            }
        }
        let mut held = lock();
        let o = &mut *held.0;
        if epoch != o.epoch || !o.window.open {
            drop_source(&mut captured);
            return false;
        }
        let _publication = Publication::open();
        begin_merge(&o.sources[side as usize], &mut captured, cvr_th_binding_overflow());
        let slot = &mut o.sources[side as usize];
        drop_source(slot);
        // SAFETY: the zeroed slot takes the capture's fields (moved out of its box, which is freed without dropping them).
        unsafe { core::ptr::write(slot, *captured) };
        let valid = o.sources[side as usize].state.aggregate_flags() & source::REFUSED == 0;
        if !valid {
            count(stats_row(), 5);
        }
        publish(o);
        valid
    }

    pub fn observing() -> bool {
        OBSERVING.load(Acquire)
    }
    /// An event under the lock: accepted by the window (a capture the list touches marked), the publication around `f`.
    fn on_event(list: *mut c_void, f: impl FnOnce(&mut Observer)) {
        let mut held = lock();
        let o = &mut *held.0;
        if !event(&mut o.window, &mut o.sources, list as u64) {
            return;
        }
        let _publication = Publication::open();
        f(o);
        publish(o);
    }
    pub fn mark_exit(list: *mut c_void, reason: u32, kind: u32) {
        if !observing() {
            return;
        }
        on_event(list, |o| mark_exit_locked(&mut o.sources, stats_row(), list as u64, reason, kind));
    }
    /// # Safety
    /// `barriers` holds `count_of` barriers.
    pub unsafe fn barriers(list: *mut c_void, count_of: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
        if count_of == 0 || !observing() {
            return;
        }
        // SAFETY: our sources under the lock; the caller's barriers.
        on_event(list, |o| unsafe { cvr_th_barriers_locked(o.sources.as_mut_ptr(), list as u64, count_of, barriers) });
    }
    pub fn written(list: *mut c_void, destination: *mut c_void, kind: u32) {
        if !observing() {
            return;
        }
        // SAFETY: our sources under the lock; a live resource or null.
        on_event(list, |o| unsafe { cvr_th_written_locked(o.sources.as_mut_ptr(), list as u64, destination, kind) });
    }
    /// The bound resources of an OM call or a clear (cvr-gpu's scene state), owned while the event runs.
    fn bound_resources(count_of: u32, handles: *const usize, contiguous: bool, depth: *const usize) -> (bool, [Option<ID3D12Resource>; 9]) {
        let mut raw = [0usize; 9];
        // SAFETY: the caller's descriptors; nine owned references (or nulls) out.
        let known = unsafe { cvr_gpu::scene_state::cvr_scene_capture_bound_resources(count_of, handles, contiguous, depth, &mut raw) };
        // SAFETY: the references are ours.
        (known, raw.map(|r| unsafe { (r != 0).then(|| ID3D12Resource::from_raw(r as *mut c_void)) }))
    }
    fn raw_of(resources: &[Option<ID3D12Resource>; 9]) -> [*mut c_void; 9] {
        core::array::from_fn(|i| resources[i].as_ref().map_or(null_mut(), |r| r.as_raw()))
    }
    pub fn clear_view(list: *mut c_void, handle: usize, depth: bool) {
        if !observing() {
            return;
        }
        let epoch = {
            let held = lock();
            if !held.0.window.open {
                return;
            }
            held.0.epoch
        };
        let (known, resources) = if depth {
            bound_resources(0, core::ptr::null(), false, &handle)
        } else {
            bound_resources(1, &handle, false, core::ptr::null())
        };
        let raw = raw_of(&resources);
        let mut held = lock();
        let o = &mut *held.0;
        if epoch != o.epoch || !event(&mut o.window, &mut o.sources, list as u64) {
            return;
        }
        let _publication = Publication::open();
        // SAFETY: our sources under the lock; nine resources or nulls.
        unsafe { cvr_th_clear_locked(o.sources.as_mut_ptr(), list as u64, known, raw.as_ptr()) };
        publish(o);
    }
    /// # Safety
    /// `handles` holds `count_of` descriptors (when nonzero); `depth` null or valid.
    pub unsafe fn targets(list: *mut c_void, count_of: u32, handles: *const usize, contiguous: bool, depth: *const usize) {
        if !observing() {
            return;
        }
        let epoch = {
            let held = lock();
            if !held.0.window.open || !held.0.sources.iter().any(|s| s.occupied) {
                return;
            }
            held.0.epoch
        };
        let (known, resources) = bound_resources(count_of, handles, contiguous, depth);
        let raw = raw_of(&resources);
        let mut held = lock();
        let o = &mut *held.0;
        if epoch != o.epoch || !event(&mut o.window, &mut o.sources, list as u64) {
            return;
        }
        let _publication = Publication::open();
        // SAFETY: nine resources or nulls, under the lock.
        if !unsafe { cvr_th_bind_targets(list as u64, raw.as_ptr(), known) } {
            unknown_locked(&mut o.sources, list as u64, exit_kind::OTHER);
        }
        publish(o);
    }
    pub fn draw(list: *mut c_void) {
        if !observing() {
            return;
        }
        // SAFETY: our sources under the lock.
        on_event(list, |o| unsafe { cvr_th_draw_locked(o.sources.as_mut_ptr(), list as u64) });
    }
    pub fn unknown(list: *mut c_void, kind: u32) {
        if !observing() {
            return;
        }
        on_event(list, |o| unknown_locked(&mut o.sources, list as u64, kind));
    }
    pub fn close(list: *mut c_void, result: i32) {
        if !observing() {
            return;
        }
        on_event(list, |o| close_locked(&mut o.sources, stats_row(), list as u64, result));
    }
    pub fn reset(list: *mut c_void) {
        if !observing() {
            return;
        }
        on_event(list, |o| {
            cvr_th_unbind(list as u64);
            reset_locked(&mut o.sources, stats_row(), list as u64);
        });
    }
    /// ValidateSource: the source's recording and each captured target's identity, validated on `queue`.
    unsafe extern "C-unwind" fn validate_source(source: *mut Source, queue: *mut c_void) -> bool {
        // SAFETY: one of our sources, under the lock.
        let s = unsafe { &*source };
        // SAFETY: a valid lease.
        let mut valid = unsafe { crate::native_recording::cvr_native_recording_validate_submission(&s.recording, queue) };
        for target in &s.targets[..(s.target_count as usize).min(TARGETS_PER_SOURCE)] {
            valid = validate_identity(&target.identity, target.authenticated, queue) && valid;
        }
        valid
    }
    /// # Safety
    /// `lists` holds `count_of` lists.
    pub unsafe fn before_submit(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) {
        if queue.is_null() || count_of == 0 || lists.is_null() || !observing() {
            return;
        }
        let mut held = lock();
        let o = &mut *held.0;
        if !o.window.event() {
            return;
        }
        let _publication = Publication::open();
        // SAFETY: our sources under the lock; the caller's lists.
        unsafe { cvr_th_before_submit_locked(o.sources.as_mut_ptr(), queue, count_of, lists, validate_source) };
        publish(o);
    }
    /// # Safety
    /// `lists` holds `count_of` lists.
    pub unsafe fn submitted(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) {
        if queue.is_null() || count_of == 0 || lists.is_null() || !observing() {
            return;
        }
        let mut held = lock();
        let o = &mut *held.0;
        if !o.window.event() {
            return;
        }
        let _publication = Publication::open();
        // SAFETY: our sources under the lock; the caller's lists.
        unsafe { cvr_th_submitted_locked(o.sources.as_mut_ptr(), queue as u64, count_of, lists) };
        publish(o);
    }

    /// target_handoff::TargetSnapshot / SourceSnapshot (include/Render/NativeTargetHandoff.hpp): the identity (its
    /// authenticated copy, the trailing shared_ptr, set by the C++ after), the view, slot, planes and frozen states.
    #[repr(C)]
    pub struct TargetSnapshot {
        pub identity: WritableIdentity,
        identity_data: [usize; 2],
        pub view: TargetInfo,
        pub slot: u32,
        pub planes: u32,
        pub final_states: [u32; 2],
    }
    #[repr(C)]
    pub struct SourceSnapshot {
        pub recording: Lease,
        pub frame: u32,
        pub side: u32,
        pub targets: [TargetSnapshot; TARGETS_PER_SOURCE],
    }
    const _: () = assert!(core::mem::size_of::<TargetSnapshot>() == 448 && core::mem::size_of::<SourceSnapshot>() == 1840);

    struct Taken {
        recording: Lease,
        targets: [(WritableIdentity, TargetInfo, u32, u32, [u32; 2]); TARGETS_PER_SOURCE],
    }
    fn taken_drop(taken: Vec<Taken>) {
        for t in taken {
            release(t.recording.token);
            for (identity, ..) in t.targets {
                identity_drop(identity);
            }
        }
    }
    /// SnapshotSources: both sides checked and copied under the lock, their recordings and identities validated on
    /// `queue` off it, the sides confirmed unchanged under it again; then written into the caller's two zeroed snapshots
    /// (their references handed over).
    ///
    /// # Safety
    /// `queue` a live queue or null; `out` two default SourceSnapshots the C++ then authenticates.
    pub unsafe fn snapshot(queue: *mut c_void, frame: u32, out: *mut [SourceSnapshot; SOURCE_LIMIT]) -> bool {
        if queue.is_null() || frame == 0 || !observing() {
            return false;
        }
        let epoch;
        let mut taken = Vec::with_capacity(SOURCE_LIMIT);
        {
            let held = lock();
            let o = &*held.0;
            if !o.window.open {
                return false;
            }
            epoch = o.epoch;
            for (side, source) in o.sources.iter().enumerate() {
                let Some(finals) = snapshot_check(source, side as u32, frame, queue as u64) else {
                    taken_drop(taken);
                    return false;
                };
                taken.push(Taken {
                    recording: lease_copy(&source.recording),
                    targets: core::array::from_fn(|i| {
                        let t = &source.targets[i];
                        (identity_clone(&t.identity), t.view, t.slot, t.planes, finals[i])
                    }),
                });
            }
        }
        let valid = taken.iter().all(|t| {
            // SAFETY: a valid lease.
            let recording = unsafe { crate::native_recording::cvr_native_recording_validate_submission(&t.recording, queue) };
            recording
                && t.targets.iter().all(|(identity, ..)| validate_identity(identity, true, queue))
        });
        let confirmed = valid && {
            let held = lock();
            let o = &*held.0;
            epoch == o.epoch
                && o.window.open
                && o.sources.iter().zip(&taken).all(|(s, t)| {
                    same_recording(&s.recording, &t.recording) && s.state.failure == failure::NONE && !s.retired_recording
                })
        };
        if !confirmed {
            taken_drop(taken);
            return false;
        }
        // SAFETY: the caller's zeroed snapshots take our references (written, never dropped here).
        let out = unsafe { &mut *out };
        for (side, t) in taken.into_iter().enumerate() {
            let saved = &mut out[side];
            // SAFETY: plain writes over the zeroed records.
            unsafe { core::ptr::write(&mut saved.recording, t.recording) };
            saved.frame = frame;
            saved.side = side as u32;
            for (i, (identity, view, slot, planes, finals)) in t.targets.into_iter().enumerate() {
                let copy = &mut saved.targets[i];
                // SAFETY: as above.
                unsafe { core::ptr::write(&mut copy.identity, identity) };
                copy.view = view;
                copy.slot = slot;
                copy.planes = planes;
                for plane in 0..(planes as usize).min(2) {
                    copy.final_states[plane] = finals[plane];
                }
            }
        }
        true
    }
    pub fn finish_observation() {
        let mut held = lock();
        let o = &mut *held.0;
        if o.window.finished {
            return;
        }
        let _publication = Publication::open();
        o.window.finish();
        o.epoch += 1;
        OBSERVING.store(false, Release);
        cvr_th_clear_bindings();
        publish(o);
    }
    pub fn clear() {
        let mut held = lock();
        let o = &mut *held.0;
        let _publication = Publication::open();
        for s in o.sources.iter_mut() {
            drop_source(s);
        }
        cvr_th_clear_bindings();
        o.epoch += 1;
        o.window.clear();
        OBSERVING.store(false, Release);
        let stats = stats_row();
        stats.fill(0);
        stats[0] = 2;
        publish(o);
    }

    // ---- the C ABI the C++ API forwards to (src/Render/NativeTargetHandoff.cpp in the Rust build)

    /// # Safety
    /// As begin's.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_th2_begin(list: *mut c_void, recording: *const Lease, frame: u32, side: u32, targets: *const GeometryTargets) -> bool {
        // SAFETY: the caller's records.
        match unsafe { (recording.as_ref(), targets.as_ref()) } {
            (Some(recording), Some(targets)) => unsafe { begin(list, recording, frame, side, targets) },
            _ => false,
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_observing() -> bool {
        observing()
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_mark_exit(list: *mut c_void, reason: u32, kind: u32) {
        mark_exit(list, reason, kind);
    }
    /// # Safety
    /// As barriers'.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_th2_barriers(list: *mut c_void, count_of: u32, value: *const D3D12_RESOURCE_BARRIER) {
        unsafe { barriers(list, count_of, value) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_written(list: *mut c_void, destination: *mut c_void, kind: u32) {
        written(list, destination, kind);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_clear_view(list: *mut c_void, handle: usize, depth: bool) {
        clear_view(list, handle, depth);
    }
    /// # Safety
    /// As targets'.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_th2_targets(list: *mut c_void, count_of: u32, handles: *const usize, contiguous: bool, depth: *const usize) {
        unsafe { targets(list, count_of, handles, contiguous, depth) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_draw(list: *mut c_void) {
        draw(list);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_unknown(list: *mut c_void, kind: u32) {
        unknown(list, kind);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_close(list: *mut c_void, result: i32) {
        close(list, result);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_reset(list: *mut c_void) {
        reset(list);
    }
    /// # Safety
    /// As before_submit's.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_th2_before_submit(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) {
        unsafe { before_submit(queue, count_of, lists) };
    }
    /// # Safety
    /// As submitted's.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_th2_submitted(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) {
        unsafe { submitted(queue, count_of, lists) };
    }
    /// # Safety
    /// As snapshot's.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_th2_snapshot(queue: *mut c_void, frame: u32, out: *mut [SourceSnapshot; SOURCE_LIMIT]) -> bool {
        !out.is_null() && unsafe { snapshot(queue, frame, out) }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_finish_observation() {
        finish_observation();
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_th2_clear() {
        clear();
    }
}
