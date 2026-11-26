//! The submission frontier's rules (`include/Render/NativeSubmissionFrontier.hpp`, `submission_frontier::detail`): a
//! slot's token capture admission (a qualified graph, the context publisher, a live unsubmitted context of kind 1 or
//! 2); the native context ids and producer slots; the bounded engine reads; a context read twice from the 2.5 registry
//! (+0xD6EDA8, 16 bytes per id: list +0x30, state +0x60, kind +0x68, residency +0x630, submitted +0x638); the range
//! snapshot before the original consumer moves contexts (the caller 0x65607B, the renderer 0x3BBCAE8 + 0x4C60 as the
//! header, its table and first index, the registry 0x3BCDCA8, the ids read twice with every header field and global
//! re-read in the C++ order); the admission epoch; the batch ledger's token observations (fresh, ambiguous, replayed,
//! rejected or missing); and the numeric boundary selection of the two adjacent source lists and their qualified suffix.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions and the methods of `AdmissionState` and `Ledger` forward to
//! `cvr_sf_*`; their members stay in C++, mirrored here. `tools/single_pass_tests/submission_frontier_test` runs its
//! cases against them in its Rust variant.

use core::ffi::c_void;

use crate::native_recording::caller;
use crate::target_access::Boundary;

pub const CALLER_RETURN: u32 = 0x65607B;
pub const MAX_SLOTS: u32 = 256;
pub const MAX_CONTEXTS: u32 = 128;
pub const MAX_ROWS: usize = 4096;
pub const MAX_SUBMIT_LISTS: u32 = 256;
pub const RENDERER_RVA: u64 = 0x3BBCAE8;
pub const REGISTRY_RVA: u64 = 0x3BCDCA8;

/// detail::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const CALLER: u32 = 2;
    pub const RENDERER: u32 = 3;
    pub const HEADER: u32 = 4;
    pub const BOUNDS: u32 = 5;
    pub const READ: u32 = 6;
    pub const CHANGED: u32 = 7;
    pub const REGISTRY: u32 = 8;
    pub const CONTEXT_ID: u32 = 9;
    pub const CONTEXT: u32 = 10;
}
/// detail::MatchKind.
pub mod matches {
    pub const UNKNOWN_LIST: u32 = 0;
    pub const MATCHED: u32 = 1;
    pub const MATCHED_UNKNOWN_GRAPH: u32 = 2;
    pub const MISSING_TOKEN: u32 = 3;
    pub const REJECTED_TOKEN: u32 = 4;
    pub const REPLAY: u32 = 5;
    pub const AMBIGUOUS: u32 = 6;
}
/// detail::BoundaryFailure.
pub mod boundary {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const BATCH: u32 = 3;
    pub const SOURCE: u32 = 6;
    pub const SOURCE_MATCH: u32 = 7;
    pub const GRAPH: u32 = 8;
    pub const SUFFIX: u32 = 9;
    pub const REPLAY: u32 = 10;
    pub const DUPLICATE: u32 = 11;
    pub const MISSING_SOURCE: u32 = 12;
    pub const NO_SUFFIX: u32 = 13;
    pub const CHANGED: u32 = 15;
}

/// detail::Context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub address: u64,
    pub list: u64,
    pub state: u64,
    pub residency: u64,
    pub submitted: u64,
    pub kind: u32,
}
/// detail::Slot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Slot {
    pub index: u32,
    pub id: u32,
    pub context: Context,
    pub failure: u32,
}
/// detail::Range without its slots (they cross in the caller's buffer).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawRange {
    pub header: u64,
    pub table: u64,
    pub registry: u64,
    pub begin: u32,
    pub end: u32,
    pub mode: u32,
}
/// detail::Key.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Key {
    pub list: u64,
    pub context: u64,
    pub generation: u64,
    pub token: u64,
    pub invocation: u64,
    pub row: u32,
    pub graph_known: bool,
}
/// detail::Admission.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Admission {
    pub epoch: u64,
    pub active: bool,
}
/// detail::AdmissionState's members.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AdmissionState {
    pub epoch: u64,
    pub in_flight: u32,
}
impl Default for AdmissionState {
    fn default() -> Self {
        AdmissionState { epoch: 1, in_flight: 0 }
    }
}
/// detail::Match.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub kind: u32,
    pub entry: u32,
    pub failure: u32,
}
impl Default for Match {
    fn default() -> Self {
        Match { kind: matches::UNKNOWN_LIST, entry: u32::MAX, failure: 0 }
    }
}
/// detail::Ledger's entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LedgerEntry {
    pub key: Key,
    pub used: bool,
}
/// detail::Ledger's members.
#[repr(C)]
pub struct Ledger {
    pub entries: [LedgerEntry; MAX_ROWS],
    pub count: usize,
}
/// detail::BoundaryBatch.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundaryBatch {
    pub probe_epoch: u64,
    pub batch: u64,
    pub thread: u64,
    pub queue: u64,
    pub lists: u64,
    pub count: u32,
}
/// detail::BoundaryGraph.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundaryGraph {
    pub invocation: u64,
    pub consumer: u64,
    pub epoch: u64,
    pub pair_generation: u64,
    pub frame_owner: u64,
    pub fingerprint: u64,
    pub range: u32,
}
/// detail::BoundaryList.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundaryList {
    pub list: u64,
    pub context: u64,
    pub generation: u64,
    pub token: u64,
    pub registry: u64,
    pub work: u64,
    pub name: u64,
    pub graph: BoundaryGraph,
    pub slot: u32,
    pub node: u32,
    pub view: u32,
    pub kind: u32,
    pub qualified: bool,
}
/// detail::BoundarySource.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundarySource {
    pub list: u64,
    pub context: u64,
    pub generation: u64,
    pub token: u64,
    pub registry: u64,
    pub fingerprint: u64,
    pub work: u64,
    pub name: u64,
    pub slot: u32,
    pub node: u32,
    pub view: u32,
    pub status: u32,
}
const _: () = assert!(
    core::mem::size_of::<Context>() == 48
        && core::mem::size_of::<Slot>() == 64
        && core::mem::size_of::<RawRange>() == 40
        && core::mem::size_of::<Key>() == 48
        && core::mem::size_of::<Admission>() == 16
        && core::mem::size_of::<AdmissionState>() == 16
        && core::mem::size_of::<Match>() == 12
        && core::mem::size_of::<LedgerEntry>() == 56
        && core::mem::size_of::<Ledger>() == MAX_ROWS * 56 + 8
        && core::mem::size_of::<BoundaryBatch>() == 48
        && core::mem::size_of::<BoundaryGraph>() == 56
        && core::mem::size_of::<BoundaryList>() == 136
        && core::mem::size_of::<BoundarySource>() == 80
);

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

/// detail::TokenCaptureAllowed.
pub fn token_capture_allowed(graph_status: u32, publisher: bool, slot: &Slot) -> bool {
    let context = &slot.context;
    graph_status == 0
        && publisher
        && slot.failure == failure::NONE
        && context.address != 0
        && context.list != 0
        && context.state != 0
        && context.submitted == 0
        && (context.kind == 1 || context.kind == 2)
}
/// detail::NativeId.
pub fn native_id(id: u32) -> bool {
    id != 0 && id < 0x8000_0000
}
/// detail::ProducerSlot.
pub fn producer_slot(table_index: u32) -> u32 {
    if table_index != 0 { table_index - 1 } else { u32::MAX }
}
/// detail::Read.
pub fn read(reader: &mut Option<Read<'_>>, address: u64, out: &mut [u8]) -> bool {
    match reader {
        Some(reader) => address != 0 && !out.is_empty() && address <= u64::MAX - out.len() as u64 && reader(address, out),
        None => false,
    }
}
/// detail::Field.
pub fn field(reader: &mut Option<Read<'_>>, address: u64, offset: u64, out: &mut [u8]) -> bool {
    address != 0 && address <= u64::MAX - offset && read(reader, address + offset, out)
}
fn field_u64(reader: &mut Option<Read<'_>>, address: u64, offset: u64) -> Option<u64> {
    let mut bytes = [0u8; 8];
    field(reader, address, offset, &mut bytes).then(|| u64::from_le_bytes(bytes))
}
fn field_u32(reader: &mut Option<Read<'_>>, address: u64, offset: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    field(reader, address, offset, &mut bytes).then(|| u32::from_le_bytes(bytes))
}
fn context_once(reader: &mut Option<Read<'_>>, registry: u64, offset: u64) -> Option<Context> {
    let address = field_u64(reader, registry, offset).filter(|&address| address != 0)?;
    let list = field_u64(reader, address, 0x30)?;
    let state = field_u64(reader, address, 0x60)?;
    let kind = field_u32(reader, address, 0x68)?;
    let residency = field_u64(reader, address, 0x630)?;
    let submitted = field_u64(reader, address, 0x638)?;
    Some(Context { address, list, state, residency, submitted, kind })
}
/// detail::ReadContext: the registry's context of `id`, read twice and equal.
pub fn read_context(reader: &mut Option<Read<'_>>, registry: u64, id: u32) -> Option<Context> {
    if registry == 0 || id == 0 || id > MAX_CONTEXTS {
        return None;
    }
    let offset = 0xD6EDA8 + (id - 1) as u64 * 16;
    let a = context_once(reader, registry, offset)?;
    let b = context_once(reader, registry, offset)?;
    (a == b).then_some(a)
}
fn ids(reader: &mut Option<Read<'_>>, address: u64, count: usize) -> Option<[u32; MAX_SLOTS as usize]> {
    let mut bytes = [0u8; MAX_SLOTS as usize * 4];
    if !read(reader, address, &mut bytes[..count * 4]) {
        return None;
    }
    let mut ids = [0u32; MAX_SLOTS as usize];
    for (id, chunk) in ids.iter_mut().zip(bytes.chunks_exact(4)).take(count) {
        *id = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    Some(ids)
}
/// detail::CaptureRange: the range and its slots, or the failing stage.
pub fn capture_range(
    mut reader: Option<Read<'_>>,
    base: u64,
    caller_address: u64,
    header: u64,
    end: u32,
    mode: u32,
) -> Result<(RawRange, Vec<Slot>), u32> {
    let reader = &mut reader;
    if base == 0 || header == 0 || mode > 2 || end >= MAX_SLOTS {
        return Err(failure::ARGUMENTS);
    }
    if !caller(base, caller_address, CALLER_RETURN) {
        return Err(failure::CALLER);
    }
    let renderer = match field_u64(reader, base, RENDERER_RVA) {
        Some(renderer) if renderer != 0 && renderer <= u64::MAX - 0x4C60 && header == renderer + 0x4C60 => renderer,
        _ => return Err(failure::RENDERER),
    };
    let mut next = RawRange { header, end, mode, ..RawRange::default() };
    next.table = field_u64(reader, header, 0).filter(|&table| table != 0).ok_or(failure::HEADER)?;
    next.begin = field_u32(reader, header, 0x10).ok_or(failure::HEADER)?;
    if next.begin > end + 1 || next.begin > MAX_SLOTS {
        return Err(failure::BOUNDS);
    }
    next.registry = field_u64(reader, base, REGISTRY_RVA).filter(|&registry| registry != 0).ok_or(failure::REGISTRY)?;
    let count = (end + 1 - next.begin) as usize;
    let first = next.begin as u64 * 4;
    let mut slot_ids = [0u32; MAX_SLOTS as usize];
    if count != 0 {
        if next.table > u64::MAX - first {
            return Err(failure::READ);
        }
        slot_ids = ids(reader, next.table + first, count).ok_or(failure::READ)?;
    }
    let mut slots = Vec::with_capacity(count);
    for (i, &id) in slot_ids.iter().enumerate().take(count) {
        let mut slot = Slot { index: next.begin + i as u32, id, ..Slot::default() };
        if native_id(id) {
            if id > MAX_CONTEXTS {
                slot.failure = failure::CONTEXT_ID;
            } else {
                match read_context(reader, next.registry, id) {
                    Some(context) => slot.context = context,
                    None => slot.failure = failure::CONTEXT,
                }
            }
        }
        slots.push(slot);
    }
    let unchanged = field_u64(reader, header, 0) == Some(next.table)
        && field_u32(reader, header, 0x10) == Some(next.begin)
        && field_u64(reader, base, REGISTRY_RVA) == Some(next.registry)
        && field_u64(reader, base, RENDERER_RVA) == Some(renderer)
        && (count == 0 || ids(reader, next.table + first, count).is_some_and(|again| again[..count] == slot_ids[..count]));
    if !unchanged {
        return Err(failure::CHANGED);
    }
    Ok((next, slots))
}

impl AdmissionState {
    pub fn admit(&mut self, enabled: bool, ticket: &mut Admission) -> bool {
        if !enabled || ticket.active || self.in_flight == u32::MAX {
            return false;
        }
        *ticket = Admission { epoch: self.epoch, active: true };
        self.in_flight += 1;
        true
    }
    pub fn matches(&self, epoch: u64) -> bool {
        self.in_flight != 0 && epoch != 0 && epoch == self.epoch
    }
    pub fn release(&mut self, ticket: &mut Admission) -> bool {
        if !ticket.active || !self.matches(ticket.epoch) {
            return false;
        }
        self.in_flight -= 1;
        ticket.active = false;
        true
    }
    pub fn reset(&mut self, enabled: bool) -> bool {
        if enabled || self.in_flight != 0 || self.epoch == u64::MAX {
            return false;
        }
        self.epoch += 1;
        true
    }
}

impl Ledger {
    fn live(&self) -> &[LedgerEntry] {
        &self.entries[..self.count.min(MAX_ROWS)]
    }
    pub fn add(&mut self, key: &Key) -> bool {
        if key.list == 0 || key.context == 0 || self.count == MAX_ROWS {
            return false;
        }
        self.entries[self.count] = LedgerEntry { key: *key, used: false };
        self.count += 1;
        true
    }
    pub fn clear(&mut self) {
        self.count = 0;
    }
    pub fn mentions(&self, list: u64) -> bool {
        self.live().iter().any(|entry| entry.key.list == list)
    }
    /// Observe: `validate` inspects held tokens (entry index, reason out), never the recycled id bank.
    pub fn observe(&mut self, list: u64, validate: &mut dyn FnMut(u32, &mut u32) -> bool) -> Match {
        let count = self.count;
        let ledger: *mut Ledger = self;
        // SAFETY: `ledger` is this live ledger; each access is a short copy or store.
        unsafe { observe_entries(count, list, |i| (*ledger).entries[i], validate, |i| (*ledger).entries[i].used = true) }
    }
}
/// Ledger::Observe over the entries as `entry` reads them; `mark` marks the single fresh match used. The ledger is not
/// borrowed across `validate`, which may read it.
pub fn observe_entries(
    count: usize,
    list: u64,
    entry: impl Fn(usize) -> LedgerEntry,
    validate: &mut dyn FnMut(u32, &mut u32) -> bool,
    mut mark: impl FnMut(usize),
) -> Match {
    {
        let (mut fresh, mut missing, mut rejected, mut replayed) = (Match::default(), Match::default(), Match::default(), Match::default());
        let mut valid_count = 0u32;
        for i in 0..count.min(MAX_ROWS) {
            let entry = entry(i);
            let key = &entry.key;
            if key.list != list {
                continue;
            }
            let index = i as u32;
            if key.token == 0 || key.generation == 0 {
                if missing.entry == u32::MAX {
                    missing = Match { kind: matches::MISSING_TOKEN, entry: index, failure: 0 };
                }
                continue;
            }
            let mut reason = 0u32;
            let valid = validate(index, &mut reason);
            if valid && !entry.used {
                let kind = if key.graph_known { matches::MATCHED } else { matches::MATCHED_UNKNOWN_GRAPH };
                fresh = Match { kind, entry: index, failure: 0 };
                valid_count += 1;
            } else if entry.used && (valid || reason == 9 || reason == 12) {
                if replayed.entry == u32::MAX {
                    replayed = Match { kind: matches::REPLAY, entry: index, failure: reason };
                }
            } else if rejected.entry == u32::MAX {
                rejected = Match { kind: matches::REJECTED_TOKEN, entry: index, failure: reason };
            }
        }
        if valid_count > 1 {
            return Match { kind: matches::AMBIGUOUS, entry: u32::MAX, failure: 0 };
        }
        if valid_count == 1 {
            mark(fresh.entry as usize);
            return fresh;
        }
        [replayed, rejected, missing].into_iter().find(|candidate| candidate.entry != u32::MAX).unwrap_or_default()
    }
}

/// detail::SelectBoundary: numeric selection only; the caller validates held native tokens.
pub fn select_boundary(
    observed: &BoundaryBatch,
    current: &BoundaryBatch,
    rows: &[BoundaryList],
    lists: &[u64],
    sources: &[BoundarySource; 2],
) -> Result<Boundary, u32> {
    if observed.probe_epoch == 0
        || observed.batch == 0
        || observed.thread == 0
        || observed.queue == 0
        || observed.lists == 0
        || observed != current
    {
        return Err(boundary::BATCH);
    }
    if observed.count < 3 || observed.count > MAX_SUBMIT_LISTS || rows.len() != observed.count as usize || lists.len() != rows.len() {
        return Err(boundary::ARGUMENTS);
    }
    let mut indices = [u32::MAX; 2];
    for (i, row) in rows.iter().enumerate() {
        if lists[i] == 0 || row.list != lists[i] {
            return Err(boundary::CHANGED);
        }
        if row.kind != matches::UNKNOWN_LIST && row.kind != matches::MATCHED {
            return Err(boundary::REPLAY);
        }
        if lists[..i].contains(&lists[i]) {
            return Err(boundary::DUPLICATE);
        }
        for side in 0..2 {
            if row.list == sources[side].list {
                indices[side] = i as u32;
            }
        }
    }
    for (side, source) in sources.iter().enumerate() {
        if indices[side] == u32::MAX {
            return Err(boundary::MISSING_SOURCE);
        }
        if source.status != 0
            || source.view != side as u32
            || source.list == 0
            || source.context == 0
            || source.generation == 0
            || source.token == 0
            || source.registry == 0
            || source.fingerprint == 0
            || source.work == 0
            || source.slot >= MAX_SLOTS
        {
            return Err(boundary::SOURCE);
        }
        let row = &rows[indices[side] as usize];
        if row.kind != matches::MATCHED
            || !row.qualified
            || row.context != source.context
            || row.generation != source.generation
            || row.token != source.token
            || row.registry != source.registry
            || row.graph.fingerprint != source.fingerprint
            || row.slot != source.slot
            || row.node != source.node
            || row.view != source.view
            || row.work != source.work
            || row.name != source.name
        {
            return Err(boundary::SOURCE_MATCH);
        }
    }
    if indices[0] == indices[1]
        || sources[0].slot == sources[1].slot
        || sources[0].token == sources[1].token
        || sources[0].context == sources[1].context
        || sources[0].node == sources[1].node
    {
        return Err(boundary::DUPLICATE);
    }
    if sources[0].registry != sources[1].registry || sources[0].work != sources[1].work {
        return Err(boundary::SOURCE);
    }
    let graph = rows[indices[0] as usize].graph;
    if graph.invocation == 0
        || graph.consumer == 0
        || graph.epoch == 0
        || graph.pair_generation == 0
        || graph.frame_owner == 0
        || graph.fingerprint == 0
        || rows[indices[1] as usize].graph != graph
    {
        return Err(boundary::GRAPH);
    }
    let after = indices[0].max(indices[1]) + 1;
    let highest_slot = sources[0].slot.max(sources[1].slot);
    if after as usize >= rows.len() {
        return Err(boundary::NO_SUFFIX);
    }
    for i in after as usize..rows.len() {
        let row = &rows[i];
        if row.kind != matches::MATCHED
            || !row.qualified
            || row.context == 0
            || row.generation == 0
            || row.token == 0
            || row.registry != sources[0].registry
            || row.graph != graph
            || row.slot <= highest_slot
            || row.slot >= MAX_SLOTS
            || row.view > 1
            || row.work != sources[0].work
        {
            return Err(boundary::SUFFIX);
        }
        if rows[..i].iter().any(|earlier| earlier.qualified && (row.token == earlier.token || row.slot == earlier.slot)) {
            return Err(boundary::DUPLICATE);
        }
    }
    Ok(Boundary {
        probe_epoch: observed.probe_epoch,
        batch: observed.batch,
        thread: observed.thread,
        queue: observed.queue,
        lists: observed.lists,
        consumer: graph.consumer,
        graph_epoch: graph.epoch,
        pair_generation: graph.pair_generation,
        frame_owner: graph.frame_owner,
        fingerprint: graph.fingerprint,
        invocation: graph.invocation,
        count: observed.count,
        after_index: after,
        source_indices: indices,
    })
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// detail::ReadMemory.
pub type ReadMemory = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
fn reader(read: Option<ReadMemory>, user: *mut c_void) -> Option<impl FnMut(u64, &mut [u8]) -> bool> {
    read.map(move |read| move |at: u64, out: &mut [u8]| unsafe { read(user, at, out.as_mut_ptr().cast(), out.len()) })
}
unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// TokenCaptureAllowed.
///
/// # Safety
/// `slot` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sf_token_capture_allowed(graph_status: u32, publisher: bool, slot: *const Slot) -> bool {
    token_capture_allowed(graph_status, publisher, unsafe { &*slot })
}
/// NativeId (`which` 0, a boolean) or ProducerSlot (1).
#[no_mangle]
pub extern "C-unwind" fn cvr_sf_id(which: u32, value: u32) -> u32 {
    if which == 0 { native_id(value) as u32 } else { producer_slot(value) }
}
/// Read (`offset` none) or Field (with `offset`).
///
/// # Safety
/// `out` holds `bytes` writable bytes; `read` follows detail::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sf_read(
    read_memory: Option<ReadMemory>,
    user: *mut c_void,
    address: u64,
    field_read: bool,
    offset: u64,
    out: *mut c_void,
    bytes: usize,
) -> bool {
    let mut adapter = reader(read_memory, user);
    let mut reader = adapter.as_mut().map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool);
    let mut empty = [0u8; 0];
    let out = if out.is_null() || bytes == 0 { &mut empty[..] } else { unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) } };
    if field_read { field(&mut reader, address, offset, out) } else { read(&mut reader, address, out) }
}
/// ReadContext (`output` zeroed, written on success).
///
/// # Safety
/// `output` is writable; `read` follows detail::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sf_read_context(read_memory: Option<ReadMemory>, user: *mut c_void, registry: u64, id: u32, output: *mut Context) -> bool {
    let mut adapter = reader(read_memory, user);
    let mut reader = adapter.as_mut().map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool);
    let context = read_context(&mut reader, registry, id);
    unsafe { *output = context.unwrap_or_default() };
    context.is_some()
}
/// CaptureRange: `range` and up to MAX_SLOTS `slots` with their `count` written on success; `failure` always.
///
/// # Safety
/// `range`, `slots` (room for MAX_SLOTS), `count` and `failure` are writable; `read` follows detail::ReadMemory.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sf_capture_range(
    read_memory: Option<ReadMemory>,
    user: *mut c_void,
    base: u64,
    caller_address: u64,
    header: u64,
    end: u32,
    mode: u32,
    range: *mut RawRange,
    slots: *mut Slot,
    count: *mut usize,
    failure: *mut u32,
) -> bool {
    let mut adapter = reader(read_memory, user);
    let reader = adapter.as_mut().map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool);
    match capture_range(reader, base, caller_address, header, end, mode) {
        Ok((next, captured)) => unsafe {
            *range = next;
            core::ptr::copy_nonoverlapping(captured.as_ptr(), slots, captured.len());
            *count = captured.len();
            *failure = failure::NONE;
            true
        },
        Err(stage) => {
            unsafe { *failure = stage };
            false
        }
    }
}
/// AdmissionState: Epoch (`which` 0), InFlight (1), Admit (2: `enabled`, `ticket`), Matches (3: `epoch`), Release
/// (4: `ticket`), Reset (5: `enabled`). A boolean result is 0 or 1.
///
/// # Safety
/// `state` is valid; `ticket` is valid for Admit and Release.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sf_admission(state: *mut AdmissionState, which: u32, enabled: bool, ticket: *mut Admission, epoch: u64) -> u64 {
    match which {
        0 => return unsafe { (*state).epoch },
        1 => return unsafe { (*state).in_flight } as u64,
        3 => return unsafe { (*state).matches(epoch) } as u64,
        _ => {}
    }
    let state = unsafe { &mut *state };
    (match which {
        2 => state.admit(enabled, unsafe { &mut *ticket }),
        4 => state.release(unsafe { &mut *ticket }),
        _ => state.reset(enabled),
    }) as u64
}
/// Ledger: Add (`which` 0: `key`), Clear (1), Mentions (2: `list`).
///
/// # Safety
/// `ledger` is valid; `key` is valid for Add.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sf_ledger(ledger: *mut Ledger, which: u32, key: *const Key, list: u64) -> bool {
    if which == 2 {
        return unsafe { (*ledger).mentions(list) };
    }
    let ledger = unsafe { &mut *ledger };
    if which == 0 {
        ledger.add(unsafe { &*key })
    } else {
        ledger.clear();
        true
    }
}
/// The ledger's validation callback: (context, entry index, reason out) -> valid.
pub type Validate = unsafe extern "C-unwind" fn(*mut c_void, u32, *mut u32) -> bool;
/// Ledger::Observe.
///
/// # Safety
/// `ledger` is valid; `output` is writable; `validate` is callable with `context`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sf_ledger_observe(ledger: *mut Ledger, list: u64, validate: Validate, context: *mut c_void, output: *mut Match) {
    let mut call = |index: u32, reason: &mut u32| unsafe { validate(context, index, reason) };
    // SAFETY: `ledger` is valid; the C++ callback may read it, so no Rust reference spans the call.
    let result = unsafe {
        observe_entries((*ledger).count, list, |i| (*ledger).entries[i], &mut call, |i| (*ledger).entries[i].used = true)
    };
    unsafe { *output = result };
}
/// SelectBoundary (`output` zeroed, written on success; `failure`, when not null, always written).
///
/// # Safety
/// The batches, rows (`row_count`), lists (`list_count`) and sources are valid; `output` is writable; `failure` is null
/// or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sf_select_boundary(
    observed: *const BoundaryBatch,
    current: *const BoundaryBatch,
    rows: *const BoundaryList,
    row_count: usize,
    lists: *const u64,
    list_count: usize,
    sources: *const [BoundarySource; 2],
    output: *mut Boundary,
    failure: *mut u32,
) -> bool {
    let result = unsafe { select_boundary(&*observed, &*current, slice(rows, row_count), slice(lists, list_count), &*sources) };
    let (selected, reason) = match result {
        Ok(selected) => (selected, boundary::NONE),
        Err(reason) => (Boundary::default(), reason),
    };
    unsafe {
        *output = selected;
        if !failure.is_null() {
            *failure = reason;
        }
    }
    reason == boundary::NONE
}
