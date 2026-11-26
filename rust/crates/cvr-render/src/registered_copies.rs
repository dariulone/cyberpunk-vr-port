//! The registered constant upload copy ledger (src/Render/RegisteredConstantUpload.cpp, `constants::`
//! ObserveRegisteredCopy and its siblings).
//!
//! Every observed buffer copy, whole-buffer invalidation and aliasing barrier is a positioned write or alias in the
//! constant write timeline (with its placement's memory span); every Reset, ExecuteCommandLists, fence registration,
//! queue Signal / Wait, CPU Signal and fence progress feeds the timeline's queue order. With the legacy capture enabled a
//! copy of at most 64 KiB into a default-heap, non-UAV buffer from a mapped upload source becomes an owned payload: its
//! bytes read twice (and equal to the registered upload scope's when the copy is that scope's whole destination), the
//! command resources retained, at most 16 MiB and 8192 payloads; its copy span is the latest bytes for that address
//! until a later write overlaps it. At submission a payload's source is read again (changed: invalid) and its queue,
//! batch and position recorded. Reads take the latest payload covering the address (a pending one only for its own
//! list, or with a receipt); a receipt is valid only for a consumer submitted after it on the same queue. Read positions
//! mark a consumer's point in its recording; once frozen, a position resolves through the timeline to the payload and
//! its bytes, and a still-pending position can capture the single producer recording's intended bytes. Every step is
//! traced (at most 131072 events) and the trace is published by ClearRegistered.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's ledger functions forward to `cvr_rc_*` past their gates (the intent
//! tracking, the upload registry's own reads and the command resources stay with their modules: the latter is a
//! callback); the exported counters are lent per call; `haveCopies` is the exported `cvr_rc_active`.
//! `tools/single_pass_tests/constant_capture_test` runs its cases against it in its Rust variant.

use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcess;

use crate::constant_timeline::{could_alias, detail, status, MemorySpan, Position, RelatedWrite, Resolved, Timeline};
use crate::creation_receipts::placement_of;

/// ConstantWriteTimeline::Detail past the timeline's own.
const UNSUPPORTED_DESTINATION: i32 = 4;
const UNMAPPED_SOURCE: i32 = 5;
const UNSTABLE_SOURCE: i32 = 6;
const COPY_TOO_LARGE: i32 = 7;

/// TraceOp (include/Render/ConstantTrace.hpp).
pub mod op {
    pub const MARK: u64 = 1;
    pub const WRITE: u64 = 2;
    pub const PAYLOAD: u64 = 3;
    pub const EXPLANATION: u64 = 4;
    pub const ALIAS: u64 = 5;
    pub const RESET: u64 = 6;
    pub const SUBMIT_BEGIN: u64 = 7;
    pub const SUBMIT_LIST: u64 = 8;
    pub const SUBMIT_END: u64 = 9;
    pub const FENCE: u64 = 10;
    pub const SIGNAL: u64 = 11;
    pub const WAIT: u64 = 12;
    pub const CPU_SIGNAL: u64 = 13;
    pub const PROGRESS: u64 = 14;
    pub const READ_POSITION: u64 = 15;
    pub const DRAW: u64 = 16;
    pub const BINDING: u64 = 17;
    pub const RESOLVE: u64 = 18;
    pub const FINAL_RESULT: u64 = 19;
    pub const MEMORY: u64 = 20;
}
pub const TRACE_CAPACITY: usize = 131072;
const LIMIT: usize = 16 * 1024 * 1024;
const MAX_COPY: u64 = 65536;
const HISTORY_LIMIT: usize = 8192;
const FENCE_LIMIT: usize = 4096;
const QUEUE_LIMIT: usize = 64;

/// TraceEvent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TraceEvent {
    pub op: u64,
    pub args: [u64; 7],
}
const _: () = assert!(core::mem::size_of::<TraceEvent>() == 64);

/// The .cpp's exported counters, lent per call.
#[repr(C)]
pub struct Counters {
    pub count: &'static AtomicU32,
    pub rejected: &'static AtomicU32,
    pub changed: &'static AtomicU32,
    pub unscoped: &'static AtomicU32,
    pub pending_reads: &'static AtomicU32,
    pub order_rejected: &'static AtomicU32,
    pub reads: &'static AtomicU64,
    pub timeline_results: &'static [AtomicU32; 8],
    pub timeline_stats: &'static [AtomicU32; 5],
    pub signals: &'static AtomicU32,
    pub waits: &'static AtomicU32,
    pub cpu_signals: &'static AtomicU32,
    pub trace_seq: &'static AtomicU32,
    pub trace_count: &'static AtomicU32,
    pub trace_overflow: &'static AtomicU32,
    pub trace_data: *mut u64,
    pub registered_types: &'static [AtomicU32; 6],
}
const _: () = assert!(core::mem::size_of::<Counters>() == 136);
fn bump(counter: &AtomicU32) {
    counter.fetch_add(1, Ordering::SeqCst);
}

/// `haveCopies`: any ledger entry (read by the .cpp before calling in).
#[no_mangle]
pub static cvr_rc_active: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default)]
struct Submission {
    queue: Option<ID3D12CommandQueue>,
    batch: u64,
    position: u32,
}
struct Copy {
    destination: ID3D12Resource,
    source: ID3D12Resource,
    list: u64,
    id: u64,
    address: u64,
    source_offset: u64,
    bytes: Vec<u8>,
    submitted: bool,
    invalid: bool,
    committed: bool,
    submission: Submission,
    write: u32,
    point: Position,
    memory: MemorySpan,
}
#[derive(Default)]
struct Trace {
    events: Vec<TraceEvent>,
    overflow: bool,
}
impl Trace {
    fn push(&mut self, op: u64, args: &[u64]) {
        if self.events.len() >= TRACE_CAPACITY {
            self.overflow = true;
            return;
        }
        let mut event = TraceEvent { op, args: [0; 7] };
        event.args[..args.len()].copy_from_slice(args);
        self.events.push(event);
    }
}
#[derive(Default)]
struct Ledger {
    /// The latest payload per copied address (its history id).
    copies: BTreeMap<u64, u64>,
    history: BTreeMap<u64, Copy>,
    submissions: BTreeMap<u64, Submission>,
    owned_bytes: usize,
    next_receipt: u64,
    next_batch: u64,
    timeline: Timeline,
    read_positions: BTreeMap<u64, (u64, Position)>,
    fences: BTreeMap<u64, ID3D12Fence>,
    queue_owners: BTreeMap<u64, Option<ID3D12CommandQueue>>,
    next_read_position: u64,
    frozen: bool,
    timeline_failed: bool,
    trace: Trace,
    published: Vec<TraceEvent>,
}
// SAFETY: the COM objects are free-threaded D3D12 objects; the ledger is only used under its mutex.
unsafe impl Send for Ledger {}

static LEDGER: Mutex<Option<Box<Ledger>>> = Mutex::new(None);
fn lock() -> MutexGuard<'static, Option<Box<Ledger>>> {
    LEDGER.lock().unwrap_or_else(|e| e.into_inner())
}
fn ledger<'a>(guard: &'a mut MutexGuard<'static, Option<Box<Ledger>>>) -> &'a mut Ledger {
    guard.get_or_insert_with(Box::default)
}

impl Ledger {
    fn mark(&mut self, list: u64) -> Position {
        let point = self.timeline.mark(list);
        self.trace.push(op::MARK, &[list, point.recording as u64, point.command]);
        point
    }
    fn record_write(&mut self, list: u64, address: u64, size: u64, committed: bool, memory: MemorySpan) -> (Position, u32) {
        let point = self.mark(list);
        let id = self.timeline.write(point, address, size, committed, 0);
        self.trace.push(op::WRITE, &[id as u64, point.recording as u64, point.command, address, size, committed as u64]);
        self.timeline.memory(id, memory);
        self.trace.push(op::MEMORY, &[id as u64, memory.heap, memory.offset, memory.bytes]);
        (point, id)
    }
    fn explain(&mut self, write: u32, detail: i32) {
        self.timeline.explanation(write, detail);
        self.trace.push(op::EXPLANATION, &[write as u64, detail as u64]);
    }
    fn invalidate_range(&mut self, address: u64, size: u64) {
        if size == 0 {
            return;
        }
        let history = &self.history;
        self.copies.retain(|&start, id| {
            let length = history.get(id).map_or(0, |c| c.bytes.len() as u64);
            !(address <= u64::MAX - size && address < start.wrapping_add(length) && start < address.wrapping_add(size))
        });
    }
}

fn read_source(source: &ID3D12Resource, offset: u64, output: &mut [u8]) -> bool {
    let bytes = output.len() as u64;
    // SAFETY (whole block): a live resource; the mapped range covers the bytes read.
    unsafe {
        let desc = source.GetDesc();
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        let mut flags = D3D12_HEAP_FLAGS::default();
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || offset > desc.Width
            || bytes > desc.Width - offset
            || source.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_err()
            || (heap.Type != D3D12_HEAP_TYPE_UPLOAD && heap.Type != D3D12_HEAP_TYPE_GPU_UPLOAD)
        {
            return false;
        }
        let range = D3D12_RANGE { Begin: offset as usize, End: (offset + bytes) as usize };
        let mut mapped: *mut c_void = core::ptr::null_mut();
        if source.Map(0, Some(&range), Some(&mut mapped)).is_err() {
            return false;
        }
        let mut read = 0usize;
        let ok = !mapped.is_null()
            && ReadProcessMemory(
                GetCurrentProcess(),
                (mapped as *const u8).add(offset as usize).cast(),
                output.as_mut_ptr().cast(),
                output.len(),
                Some(&mut read),
            )
            .is_ok()
            && read == output.len();
        let none = D3D12_RANGE::default();
        source.Unmap(0, Some(&none));
        ok
    }
}
fn buffer(resource: &ID3D12Resource) -> Option<D3D12_RESOURCE_DESC> {
    // SAFETY: a live resource.
    let desc = unsafe { resource.GetDesc() };
    (desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER).then_some(desc)
}

/// The command resources' retention of a copy's source and destination (KeepCommandResources).
pub type Keep = unsafe extern "C-unwind" fn(list: *mut c_void, source: *mut c_void, destination: *mut c_void) -> bool;

/// The registered upload scope's expectation (its destination and bytes), if any.
pub struct Expected<'a> {
    pub destination: *mut c_void,
    pub bytes: &'a [u8],
}

/// ObserveRegisteredCopy past the intent tracking.
#[allow(clippy::too_many_arguments)]
pub fn observe_copy(
    counters: &Counters,
    list: *mut c_void,
    destination: *mut c_void,
    offset: u64,
    source: *mut c_void,
    source_offset: u64,
    size: u64,
    legacy: bool,
    expected: &Expected,
    keep: Keep,
) {
    if !cvr_rc_active.load(Ordering::Relaxed) && !legacy {
        return;
    }
    // SAFETY: live resources of the caller's, or null.
    let (Some(d), Some(s)) = (unsafe { ID3D12Resource::from_raw_borrowed(&destination) }, unsafe { ID3D12Resource::from_raw_borrowed(&source) })
    else {
        return;
    };
    let Some(desc) = buffer(d) else {
        return;
    };
    // SAFETY: a live resource.
    let base = unsafe { d.GetGPUVirtualAddress() };
    if base == 0 || offset > desc.Width || size > desc.Width - offset || base > u64::MAX - desc.Width {
        return;
    }
    // Queue Signal can run while the command resources hold their own mutex: never take theirs inside this one.
    // SAFETY: the caller's callback over live objects.
    let retained = legacy && size != 0 && size <= MAX_COPY && unsafe { keep(list, source, destination) };
    let mut guard = lock();
    let l = ledger(&mut guard);
    let address = base + offset;
    l.invalidate_range(address, size);
    if l.frozen {
        return;
    }
    let placement = placement_of(destination);
    let committed = placement.kind == 1;
    let (point, write) = l.record_write(list as u64, address, size, committed, placement.memory);
    cvr_rc_active.store(true, Ordering::SeqCst);
    if !legacy || size == 0 {
        return;
    }
    if size > MAX_COPY {
        l.explain(write, COPY_TOO_LARGE);
        return;
    }
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource.
    if unsafe { d.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err() {
        return;
    }
    if (heap.Type.0 as u32) < 6 {
        bump(&counters.registered_types[heap.Type.0 as usize]);
    }
    if heap.Type != D3D12_HEAP_TYPE_DEFAULT || (desc.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0) != 0 {
        l.explain(write, UNSUPPORTED_DESTINATION);
        return;
    }
    // Other native uploaders may bypass BufUpload: the copy API is authoritative for its source, destination and range.
    // This ledger describes recorded writes, not completed GPU execution.
    let scoped = expected.destination == destination && offset == 0 && size == expected.bytes.len() as u64;
    let mut actual = vec![0u8; size as usize];
    let mut repeated = vec![0u8; size as usize];
    if !read_source(s, source_offset, &mut actual) {
        l.explain(write, UNMAPPED_SOURCE);
        return;
    }
    if !read_source(s, source_offset, &mut repeated)
        || actual != repeated
        || (scoped && actual != expected.bytes)
        || actual.len() > LIMIT - l.owned_bytes
        || l.history.len() >= HISTORY_LIMIT
        || !retained
    {
        l.explain(write, UNSTABLE_SOURCE);
        bump(counters.rejected);
        return;
    }
    l.next_receipt += 1;
    let id = l.next_receipt;
    l.owned_bytes += actual.len();
    l.timeline.payload(write, id);
    l.trace.push(op::PAYLOAD, &[write as u64, id]);
    l.history.insert(
        id,
        Copy {
            destination: d.clone(),
            source: s.clone(),
            list: list as u64,
            id,
            address,
            source_offset,
            bytes: actual,
            submitted: false,
            invalid: false,
            committed,
            submission: Submission::default(),
            write,
            point,
            memory: placement.memory,
        },
    );
    l.copies.entry(address).or_insert(id);
    cvr_rc_active.store(true, Ordering::SeqCst);
    bump(counters.count);
    if !scoped {
        bump(counters.unscoped);
    }
}

/// InvalidateRegistered past the intent tracking and `haveCopies`: the whole buffer is an unknown write.
pub fn invalidate(list: *mut c_void, destination: *mut c_void) {
    // SAFETY: a live resource of the caller's, or null.
    let Some(d) = (unsafe { ID3D12Resource::from_raw_borrowed(&destination) }) else {
        return;
    };
    let Some(desc) = buffer(d) else {
        return;
    };
    let mut guard = lock();
    let l = ledger(&mut guard);
    // SAFETY: a live resource.
    let address = unsafe { d.GetGPUVirtualAddress() };
    l.invalidate_range(address, desc.Width);
    if !l.frozen {
        let p = placement_of(destination);
        l.record_write(list as u64, address, desc.Width, p.kind == 1, p.memory);
    }
}

/// RegisteredBarriers past the intent tracking and `haveCopies`: aliasing barriers alias their placed memory (an unknown
/// one: everything) and drop the uncommitted payloads it could overlap.
pub fn barriers(list: *mut c_void, items: &[D3D12_RESOURCE_BARRIER]) {
    let known = |p: &crate::creation_receipts::Placement| p.kind == 1 || (p.kind == 2 && p.memory.heap != 0 && p.memory.bytes != 0);
    for b in items {
        if b.Type != D3D12_RESOURCE_BARRIER_TYPE_ALIASING {
            continue;
        }
        // SAFETY: the union member the barrier's type names.
        let a = unsafe { &b.Anonymous.Aliasing };
        let before = a.pResourceBefore.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw());
        let after = a.pResourceAfter.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw());
        let (from, to) = (placement_of(before), placement_of(after));
        let mut areas = [from, to];
        let mut used = 2;
        if before.is_null() {
            areas[0] = to;
            used = 1;
        }
        if after.is_null() {
            areas[0] = from;
            used = 1;
        }
        if (!before.is_null() && !known(&from)) || (!after.is_null() && !known(&to)) || (before.is_null() && after.is_null()) {
            areas[0] = Default::default();
            used = 1;
        }
        let mut guard = lock();
        let l = ledger(&mut guard);
        for area in &areas[..used] {
            if area.kind == 1 {
                continue;
            }
            let memory = area.memory;
            if !l.frozen {
                let point = l.mark(list as u64);
                l.timeline.alias(point, memory);
                l.trace.push(op::ALIAS, &[point.recording as u64, point.command, memory.heap, memory.offset, memory.bytes]);
            }
            let history = &l.history;
            l.copies.retain(|_, id| history.get(id).is_none_or(|c| c.committed || !could_alias(c.memory, memory)));
        }
    }
}

/// ResetRegistered past `haveCopies`.
pub fn reset(list: *mut c_void) {
    let mut guard = lock();
    let l = ledger(&mut guard);
    let list = list as u64;
    l.submissions.remove(&list);
    l.timeline.reset(list);
    l.trace.push(op::RESET, &[list]);
    for copy in l.history.values_mut() {
        if copy.list == list && !copy.submitted {
            copy.invalid = true;
        }
    }
}

/// SubmitRegistered past `haveCopies`.
pub fn submit(counters: &Counters, queue: *mut c_void, lists: &[*mut c_void]) {
    let mut guard = lock();
    let l = ledger(&mut guard);
    l.next_batch += 1;
    let batch = l.next_batch;
    let ids: Vec<u64> = lists.iter().map(|&p| p as u64).collect();
    l.timeline.submit(queue as u64, batch, &ids);
    l.trace.push(op::SUBMIT_BEGIN, &[queue as u64, batch, lists.len() as u64]);
    for &id in &ids {
        l.trace.push(op::SUBMIT_LIST, &[id]);
    }
    l.trace.push(op::SUBMIT_END, &[]);
    // SAFETY: a live queue of the caller's.
    let owner = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.cloned();
    for (i, &id) in ids.iter().enumerate() {
        l.submissions.insert(id, Submission { queue: owner.clone(), batch, position: i as u32 });
    }
    let Ledger { history, submissions, timeline, trace, .. } = l;
    for copy in history.values_mut() {
        if copy.submitted || copy.invalid || !ids.contains(&copy.list) {
            continue;
        }
        let mut current = vec![0u8; copy.bytes.len()];
        if !read_source(&copy.source, copy.source_offset, &mut current) || current != copy.bytes {
            bump(counters.changed);
            copy.invalid = true;
            timeline.payload(copy.write, 0);
            trace.push(op::PAYLOAD, &[copy.write as u64, 0]);
        } else {
            copy.submitted = true;
            copy.submission = submissions.get(&copy.list).cloned().unwrap_or_default();
        }
    }
}

/// ReadRegistered past the upload registry: the latest payload covering the bytes.
pub fn read(counters: &Counters, list: *mut c_void, address: u64, output: &mut [u8], receipt: Option<&mut u64>) -> bool {
    if output.is_empty() || output.len() as u64 > MAX_COPY {
        return false;
    }
    let mut guard = lock();
    let l = ledger(&mut guard);
    let Some((&start, id)) = l.copies.range(..=address).next_back() else {
        return false;
    };
    let Some(copy) = l.history.get(id) else {
        return false;
    };
    let offset = address - start;
    let (length, bytes) = (copy.bytes.len() as u64, output.len() as u64);
    let list = list as u64;
    if copy.invalid || (!copy.submitted && copy.list != list && (receipt.is_none() || list == 0)) || offset > length || bytes > length - offset {
        return false;
    }
    if !copy.submitted && copy.list != list {
        bump(counters.pending_reads);
    }
    if let Some(receipt) = receipt {
        *receipt = copy.id;
    }
    output.copy_from_slice(&copy.bytes[offset as usize..(offset + bytes) as usize]);
    counters.reads.fetch_add(1, Ordering::SeqCst);
    true
}

/// ValidateRegistered: the receipt's payload, submitted before the consumer on its queue, still holds the bytes.
pub fn validate(counters: &Counters, receipt: u64, list: *mut c_void, address: u64, expected: &[u8]) -> bool {
    if expected.is_empty() {
        return false;
    }
    let mut guard = lock();
    let l = ledger(&mut guard);
    let Some(copy) = l.history.get(&receipt) else {
        return false;
    };
    let (length, bytes) = (copy.bytes.len() as u64, expected.len() as u64);
    if copy.invalid || !copy.submitted || address < copy.address || address - copy.address > length || bytes > length - (address - copy.address) {
        return false;
    }
    let raw = |q: &Option<ID3D12CommandQueue>| q.as_ref().map_or(core::ptr::null_mut(), |q| q.as_raw());
    let ordered = l.submissions.get(&(list as u64)).is_some_and(|consumer| {
        raw(&consumer.queue) == raw(&copy.submission.queue)
            && consumer.batch >= copy.submission.batch
            && !(consumer.batch == copy.submission.batch && consumer.position < copy.submission.position)
    });
    if !ordered {
        bump(counters.order_rejected);
        return false;
    }
    let offset = (address - copy.address) as usize;
    copy.bytes[offset..offset + expected.len()] == *expected
}

/// MarkRegisteredRead past its gate: a read position in the list's recording (0 when none).
pub fn mark_read(list: *mut c_void, capacity: usize) -> u64 {
    let mut guard = lock();
    let l = ledger(&mut guard);
    if l.frozen || l.read_positions.len() >= capacity {
        return 0;
    }
    let point = l.mark(list as u64);
    if point.recording == 0 {
        return 0;
    }
    l.next_read_position += 1;
    let id = l.next_read_position;
    l.read_positions.insert(id, (list as u64, point));
    l.trace.push(op::READ_POSITION, &[id, list as u64, point.recording as u64, point.command]);
    cvr_rc_active.store(true, Ordering::SeqCst);
    id
}

/// RegisteredReadDraw / RegisteredReadBinding: one traced event.
pub fn trace(op: u64, args: &[u64]) {
    let mut guard = lock();
    ledger(&mut guard).trace.push(op, args);
}

/// ResolveRegisteredRead / ResolveRegisteredResourceRead: a frozen read position's payload bytes through the timeline
/// (`destination` non-null: only that destination's payload).
pub fn resolve(counters: &Counters, position: u64, list: *mut c_void, destination: *mut c_void, address: u64, output: &mut [u8]) -> Resolved {
    let mut guard = lock();
    let l = ledger(&mut guard);
    for (stat, value) in counters.timeline_stats.iter().zip(l.timeline.stats()) {
        stat.store(value, Ordering::SeqCst);
    }
    let missing = Resolved { status: status::MISSING, payload: 0, offset: 0, detail: detail::NONE };
    let unknown = Resolved { status: status::UNKNOWN_WRITE, ..missing };
    let bytes = output.len() as u64;
    let result = (|| {
        if l.timeline_failed {
            return Resolved { status: status::OVERFLOW, ..missing };
        }
        let read = l.read_positions.get(&position).copied();
        let Some((owner, point)) = read.filter(|_| l.frozen) else {
            return missing;
        };
        if owner != list as u64 || output.is_empty() {
            return missing;
        }
        let result = l.timeline.resolve(point, address, bytes);
        if result.status != status::PENDING {
            l.trace.push(
                op::RESOLVE,
                &[position, address, bytes, result.status as u64, result.detail as u64, result.payload, result.offset],
            );
        }
        if result.status != status::READY {
            return result;
        }
        let Some(copy) = l.history.get(&result.payload) else {
            return unknown;
        };
        if !destination.is_null() && copy.destination.as_raw() != destination {
            return unknown;
        }
        let length = copy.bytes.len() as u64;
        if copy.invalid || !copy.submitted || result.offset > length || bytes > length - result.offset {
            return Resolved { detail: UNSTABLE_SOURCE, ..unknown };
        }
        output.copy_from_slice(&copy.bytes[result.offset as usize..(result.offset + bytes) as usize]);
        result
    })();
    if let Some(counter) = counters.timeline_results.get(result.status as usize) {
        bump(counter);
    }
    if result.status != status::PENDING {
        l.trace.push(
            op::FINAL_RESULT,
            &[position, address, bytes, result.status as u64, result.detail as u64, result.payload, result.offset],
        );
    }
    result
}

/// CaptureRegisteredResourceIntent past its arguments: a still-pending read position's single producer recording's
/// payload bytes from the held destination: (its payload id, the offset in it, its recording point).
pub fn capture_intent(position: u64, list: *mut c_void, destination: *mut c_void, address: u64, output: &mut [u8]) -> Option<(u64, u64, Position)> {
    // SAFETY: a live resource of the caller's, or null.
    let d = unsafe { ID3D12Resource::from_raw_borrowed(&destination) }?;
    let desc = buffer(d)?;
    // SAFETY: a live resource.
    let gpu = unsafe { d.GetGPUVirtualAddress() };
    let bytes = output.len() as u64;
    if gpu == 0 || address < gpu || address - gpu > desc.Width || bytes > desc.Width - (address - gpu) {
        return None;
    }
    let mut guard = lock();
    let l = ledger(&mut guard);
    let &(owner, point) = l.read_positions.get(&position)?;
    if l.frozen || l.timeline_failed || owner != list as u64 || l.timeline.resolve(point, address, bytes).status != status::PENDING {
        return None;
    }
    // Only command order within a recording is known before consumer submission: competing producer recordings
    // need the final queue ledger, not CPU order.
    let mut producers: BTreeMap<u32, RelatedWrite> = BTreeMap::new();
    for write in l.timeline.related(point, address, bytes) {
        if write.point.recording == point.recording && write.point.command >= point.command {
            continue;
        }
        match producers.get(&write.point.recording) {
            Some(found) if found.point.command >= write.point.command => {}
            _ => {
                producers.insert(write.point.recording, write);
            }
        }
    }
    if producers.len() != 1 {
        return None;
    }
    let write = *producers.values().next()?;
    let copy = l.history.get(&write.payload)?;
    let length = copy.bytes.len() as u64;
    if copy.invalid
        || copy.id == 0
        || copy.write != write.id
        || copy.destination.as_raw() != destination
        || copy.point != write.point
        || address < copy.address
        || address - copy.address > length
        || bytes > length - (address - copy.address)
    {
        return None;
    }
    let offset = address - copy.address;
    output.copy_from_slice(&copy.bytes[offset as usize..(offset + bytes) as usize]);
    Some((copy.id, offset, copy.point))
}

/// FreezeRegistered: every captured consumer list has reached submission.
pub fn freeze() {
    ledger(&mut lock()).frozen = true;
}

/// RegisterFence past its gate.
pub fn register_fence(fence: *mut c_void) {
    // SAFETY: a live fence of the caller's, or null.
    let Some(f) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }) else {
        return;
    };
    let mut guard = lock();
    let l = ledger(&mut guard);
    if l.fences.contains_key(&(fence as u64)) {
        return;
    }
    if l.fences.len() >= FENCE_LIMIT {
        l.timeline_failed = true;
        return;
    }
    // SAFETY: a live fence.
    let completed = unsafe { f.GetCompletedValue() };
    if completed == u64::MAX {
        l.timeline_failed = true;
        return;
    }
    l.fences.insert(fence as u64, f.clone());
    l.timeline.order().fence(fence as u64, completed);
    l.trace.push(op::FENCE, &[fence as u64, completed]);
}

/// RegisteredSignal (`wait` false) / RegisteredWait past their gate.
pub fn queue_fence(counters: &Counters, wait: bool, queue: *mut c_void, fence: *mut c_void, value: u64) {
    let mut guard = lock();
    let l = ledger(&mut guard);
    let key = queue as u64;
    if l.queue_owners.len() >= QUEUE_LIMIT && !l.queue_owners.contains_key(&key) {
        l.timeline_failed = true;
        return;
    }
    // SAFETY: a live queue of the caller's, or null.
    l.queue_owners.entry(key).or_insert_with(|| unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.cloned());
    if wait {
        l.timeline.order().wait(key, fence as u64, value);
        l.trace.push(op::WAIT, &[key, fence as u64, value]);
        bump(counters.waits);
    } else {
        l.timeline.order().signal(key, fence as u64, value);
        l.trace.push(op::SIGNAL, &[key, fence as u64, value]);
        bump(counters.signals);
    }
}

/// RegisteredCpuSignal past its gate: the fence no longer orders anything.
pub fn cpu_signal(counters: &Counters, fence: *mut c_void, value: u64, caller: u64, nested: u32) {
    let mut guard = lock();
    let l = ledger(&mut guard);
    l.timeline.order().invalidate(fence as u64);
    // SAFETY: the process image's handle.
    let module = unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64);
    l.trace.push(op::CPU_SIGNAL, &[fence as u64, value, caller, module, nested as u64]);
    bump(counters.cpu_signals);
}

/// RefreshRegisteredFences: every registered fence's progress.
pub fn refresh_fences() {
    let mut guard = lock();
    let l = ledger(&mut guard);
    let Ledger { fences, timeline, trace, timeline_failed, .. } = l;
    for (&key, fence) in fences.iter() {
        // SAFETY: a held fence.
        let done = unsafe { fence.GetCompletedValue() };
        if done == u64::MAX {
            *timeline_failed = true;
            return;
        }
        timeline.order().progress(key, done);
        trace.push(op::PROGRESS, &[key, done]);
    }
}

/// ClearRegistered: publishes the trace (or an empty one) and forgets everything.
pub fn clear(counters: &Counters, publish: bool) {
    let mut guard = lock();
    let l = ledger(&mut guard);
    counters.trace_seq.fetch_add(1, Ordering::SeqCst);
    l.published = if publish { core::mem::take(&mut l.trace.events) } else { Vec::new() };
    counters.trace_count.store(l.published.len() as u32, Ordering::SeqCst);
    let data = if l.published.is_empty() { 0 } else { l.published.as_ptr() as u64 };
    // SAFETY: the .cpp's exported trace pointer.
    unsafe { counters.trace_data.write_volatile(data) };
    counters.trace_overflow.store(l.trace.overflow as u32, Ordering::SeqCst);
    counters.trace_seq.fetch_add(1, Ordering::SeqCst);
    l.trace.events.clear();
    l.trace.overflow = false;
    l.copies.clear();
    l.history.clear();
    l.submissions.clear();
    l.read_positions.clear();
    l.fences.clear();
    l.queue_owners.clear();
    l.timeline.clear();
    l.frozen = false;
    l.timeline_failed = false;
    l.owned_bytes = 0;
    cvr_rc_active.store(false, Ordering::SeqCst);
}

// ---- the C ABI

/// # Safety
/// `counters` and `expected_bytes` (for `expected_len`) are valid; the objects are live or null.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_rc_observe_copy(
    counters: *const Counters,
    list: *mut c_void,
    destination: *mut c_void,
    offset: u64,
    source: *mut c_void,
    source_offset: u64,
    size: u64,
    legacy: bool,
    expected_destination: *mut c_void,
    expected_bytes: *const u8,
    expected_len: usize,
    keep: Keep,
) {
    let bytes = if expected_bytes.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(expected_bytes, expected_len) } };
    let expected = Expected { destination: expected_destination, bytes };
    observe_copy(unsafe { &*counters }, list, destination, offset, source, source_offset, size, legacy, &expected, keep);
}
/// # Safety
/// The objects are live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_invalidate(list: *mut c_void, destination: *mut c_void) {
    invalidate(list, destination);
}
/// # Safety
/// `items` holds `count` barriers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_barriers(list: *mut c_void, count: u32, items: *const D3D12_RESOURCE_BARRIER) {
    if items.is_null() || count == 0 {
        return;
    }
    barriers(list, unsafe { core::slice::from_raw_parts(items, count as usize) });
}
/// # Safety
/// None beyond the list being any value.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_reset(list: *mut c_void) {
    reset(list);
}
/// # Safety
/// `counters` is valid; `lists` holds `count` lists.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_submit(counters: *const Counters, queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    let lists = if count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(lists, count as usize) } };
    submit(unsafe { &*counters }, queue, lists);
}
/// # Safety
/// `counters` is valid; `output` has `bytes` writable bytes (or is null); `receipt` is writable or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_read(counters: *const Counters, list: *mut c_void, address: u64, output: *mut u8, bytes: usize, receipt: *mut u64) -> bool {
    if output.is_null() {
        return false;
    }
    let output = unsafe { core::slice::from_raw_parts_mut(output, bytes) };
    read(unsafe { &*counters }, list, address, output, unsafe { receipt.as_mut() })
}
/// # Safety
/// `counters` is valid; `expected` has `bytes` readable bytes (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_validate(counters: *const Counters, receipt: u64, list: *mut c_void, address: u64, expected: *const u8, bytes: usize) -> bool {
    if expected.is_null() {
        return false;
    }
    validate(unsafe { &*counters }, receipt, list, address, unsafe { core::slice::from_raw_parts(expected, bytes) })
}
/// # Safety
/// None.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_mark_read(list: *mut c_void, capacity: usize) -> u64 {
    mark_read(list, capacity)
}
/// # Safety
/// `args` holds five values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_trace(op: u64, args: *const u64) {
    trace(op, unsafe { core::slice::from_raw_parts(args, 5) });
}
/// # Safety
/// `counters` is valid; `output` has `bytes` writable bytes (or is null); `result` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_rc_resolve(
    counters: *const Counters,
    position: u64,
    list: *mut c_void,
    destination: *mut c_void,
    address: u64,
    output: *mut u8,
    bytes: usize,
    result: *mut Resolved,
) {
    let output = if output.is_null() { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(output, bytes) } };
    unsafe { *result = resolve(&*counters, position, list, destination, address, output) };
}
/// # Safety
/// `output` has `bytes` writable bytes; the outputs are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_rc_capture_intent(
    position: u64,
    list: *mut c_void,
    destination: *mut c_void,
    address: u64,
    output: *mut u8,
    bytes: usize,
    payload: *mut u64,
    offset: *mut u64,
    point: *mut Position,
) -> bool {
    let Some((id, at, producer)) = capture_intent(position, list, destination, address, unsafe { core::slice::from_raw_parts_mut(output, bytes) })
    else {
        return false;
    };
    unsafe {
        *payload = id;
        *offset = at;
        *point = producer;
    }
    true
}
/// # Safety
/// None.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_freeze() {
    freeze();
}
/// # Safety
/// The fence is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_register_fence(fence: *mut c_void) {
    register_fence(fence);
}
/// # Safety
/// `counters` is valid; the queue is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_queue_fence(counters: *const Counters, wait: bool, queue: *mut c_void, fence: *mut c_void, value: u64) {
    queue_fence(unsafe { &*counters }, wait, queue, fence, value);
}
/// # Safety
/// `counters` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_cpu_signal(counters: *const Counters, fence: *mut c_void, value: u64, caller: u64, nested: u32) {
    cpu_signal(unsafe { &*counters }, fence, value, caller, nested);
}
/// # Safety
/// None.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_refresh_fences() {
    refresh_fences();
}
/// # Safety
/// `counters` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rc_clear(counters: *const Counters, publish: bool) {
    clear(unsafe { &*counters }, publish);
}

// ---- the entry points (formerly src/Render/RegisteredConstantUpload.cpp's): the upload scopes' expected bytes (a
// per-thread stack), the intent switch, and the detours' calls -- the copies observed (the source intent first when
// configured), invalidated, the barriers, resets and submissions, the readers SetReader takes and the fences. What the
// program owns -- the exported counters and switches, the command resources' keep, the owned uploads' read and the
// registered uploads' source -- the .cpp lends (cvr_rc2_attach; a test variant lends its own). The C++ API forwards here
// (the Rust test variants' constant capture); the detours call these directly.

#[cfg(not(test))]
pub mod entries {
    use super::*;
    use core::cell::RefCell;
    use core::ffi::c_void;
    use core::ptr::{null, null_mut};
    use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
    use std::sync::Once;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;

    /// What the .cpp lends.
    #[repr(C)]
    pub struct Host {
        pub counters: *const Counters,
        pub capture_enable: *const AtomicU32,
        pub intent_enable: *const AtomicU32,
        pub keep: Keep,
        pub read_upload: unsafe extern "C" fn(address: u64, output: *mut c_void, bytes: usize) -> bool,
        pub registered_source: unsafe extern "C" fn(handle: u32, size: u32) -> *mut c_void,
        /// source_resource_intent::ObserveCopy, Invalidate, Clear.
        pub intent_copy: unsafe extern "C" fn(*mut c_void, *mut c_void, u64, *mut c_void, u64, u64, *const AtomicU32) -> bool,
        pub intent_invalidate: unsafe extern "C" fn(*mut c_void),
        pub intent_clear: unsafe extern "C" fn(),
    }
    static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
    /// The .cpp's host (static storage, attached at its static initialisation).
    ///
    /// # Safety
    /// `host` lives for the process.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_rc2_attach(host: *const Host) {
        HOST.store(host.cast_mut(), Ordering::Release);
    }
    /// The plugin's own host (formerly lent by src/Render/RegisteredConstantUpload.cpp): the exports Rust defines, the
    /// command resources' keep, the owned uploads' read and source, the source intent. The Rust test variants attach theirs.
    #[cfg(feature = "rust-build")]
    mod plugin {
        use super::{Counters, Host};
        use crate::constant_registry_exports::CyberpunkVR_ConstantRegisteredTypes;
        use crate::native_state_exports::*;
        use core::ffi::c_void;
        use core::sync::atomic::AtomicU32;
        unsafe extern "C-unwind" fn keep(list: *mut c_void, source: *mut c_void, destination: *mut c_void) -> bool {
            let objects = [source, destination];
            // SAFETY: the live list and resources (gpu::KeepCommandResources).
            unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, objects.as_ptr(), objects.len()) }
        }
        unsafe extern "C" fn read_upload(address: u64, output: *mut c_void, bytes: usize) -> bool {
            // SAFETY: the caller's output (constants::ReadUpload).
            unsafe { crate::upload_registry::cvr_upload_read(address, output, bytes, false) }
        }
        unsafe extern "C" fn registered_source(handle: u32, size: u32) -> *mut c_void {
            crate::upload_registry::cvr_upload_registered_source(handle, size)
        }
        unsafe extern "C" fn intent_copy(list: *mut c_void, resource: *mut c_void, offset: u64, source: *mut c_void, source_offset: u64, bytes: u64, enabled: *const AtomicU32) -> bool {
            // SAFETY: the detour's list and resources (source_resource_intent::ObserveCopy).
            unsafe { crate::source_intent::cvr_sri_observe_copy(list, resource, offset, source, source_offset, bytes, enabled) }
        }
        unsafe extern "C" fn intent_invalidate(resource: *mut c_void) {
            // SAFETY: a live resource (source_resource_intent::Invalidate).
            unsafe { crate::source_intent::cvr_sri_invalidate(resource) };
        }
        unsafe extern "C" fn intent_clear() {
            // SAFETY: no arguments (source_resource_intent::Clear).
            unsafe { crate::source_intent::cvr_sri_clear() };
        }
        // SAFETY: immutable; its references and pointers name process-lifetime exports.
        unsafe impl Sync for Counters {}
        static COUNTERS: Counters = Counters {
            count: &CyberpunkVR_RegisteredCopyCount,
            rejected: &CyberpunkVR_RegisteredCopyRejected,
            changed: &CyberpunkVR_RegisteredCopyChanged,
            unscoped: &CyberpunkVR_RegisteredCopyUnscoped,
            pending_reads: &CyberpunkVR_RegisteredCopyPendingReads,
            order_rejected: &CyberpunkVR_RegisteredCopyOrderRejected,
            reads: &CyberpunkVR_RegisteredCopyReads,
            timeline_results: &CyberpunkVR_RegisteredCopyTimelineResults,
            timeline_stats: &CyberpunkVR_RegisteredCopyTimelineStats,
            signals: &CyberpunkVR_RegisteredCopySignals,
            waits: &CyberpunkVR_RegisteredCopyWaits,
            cpu_signals: &CyberpunkVR_RegisteredCopyCpuSignals,
            trace_seq: &CyberpunkVR_RegisteredCopyTraceSeq,
            trace_count: &CyberpunkVR_RegisteredCopyTraceCount,
            trace_overflow: &CyberpunkVR_RegisteredCopyTraceOverflow,
            trace_data: &raw mut CyberpunkVR_RegisteredCopyTraceData,
            registered_types: &CyberpunkVR_ConstantRegisteredTypes,
        };
        // SAFETY: as above.
        unsafe impl Sync for Host {}
        pub static HOST: Host = Host {
            counters: &COUNTERS,
            capture_enable: &CyberpunkVR_ConstantCaptureEnable,
            intent_enable: &CyberpunkVR_RegisteredIntentEnable,
            keep,
            read_upload,
            registered_source,
            intent_copy,
            intent_invalidate,
            intent_clear,
        };
    }
    fn host() -> Option<&'static Host> {
        // SAFETY: attached once with static storage.
        let attached = unsafe { HOST.load(Ordering::Acquire).as_ref() };
        #[cfg(feature = "rust-build")]
        let attached = attached.or(Some(&plugin::HOST));
        attached
    }
    /// ClearRegistered (the native constant capture's poll): over the .cpp's counters.
    pub fn clear_registered(publish: bool) {
        if let Some(h) = host() {
            super::clear(counters(h), publish);
        }
    }
    /// constants::ClearRegistered for the .cpp (the intent poll's cleanup).
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_clear_registered(publish: bool) {
        clear_registered(publish);
    }
    fn counters(h: &Host) -> &'static Counters {
        // SAFETY: the lent table lives for the process.
        unsafe { &*h.counters }
    }
    fn internal() -> bool {
        cvr_host::probe::internal()
    }
    fn capture_enabled() -> bool {
        // SAFETY: the lent exported switch.
        host().is_some_and(|h| unsafe { &*h.capture_enable }.load(Ordering::Relaxed) != 0)
    }
    fn have_copies() -> bool {
        cvr_rc_active.load(Ordering::Relaxed)
    }
    fn tracking() -> bool {
        have_copies() || capture_enabled()
    }

    // The upload scopes: each entered scope's destination and the bytes it is about to upload, innermost last.
    thread_local! {
        static EXPECTED: RefCell<Vec<(*mut c_void, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    }
    /// RegisteredUploadScope(destination, size, source): the upload's bytes read now (a guarded read), the scope pushed.
    /// False when it does not apply (no capture, an internal command, no destination, an empty or oversized upload).
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_scope_enter(destination: *mut c_void, size: u32, source: *const c_void) -> bool {
        use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
        use windows::Win32::System::Threading::GetCurrentProcess;
        if !capture_enabled() || internal() || destination.is_null() || source.is_null() || size == 0 || size > 65536 {
            return false;
        }
        let mut bytes = vec![0u8; size as usize];
        let mut copied = 0usize;
        // SAFETY: a guarded read of this process's memory into our buffer.
        let read = unsafe { ReadProcessMemory(GetCurrentProcess(), source, bytes.as_mut_ptr().cast(), bytes.len(), Some(&mut copied)) };
        if read.is_err() || copied != bytes.len() {
            return false;
        }
        EXPECTED.with(|e| e.borrow_mut().push((destination, bytes)));
        true
    }
    /// RegisteredUploadScope(handle, bytes, source): the registered upload's destination (when capturing), then as above.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_scope_enter_handle(handle: u32, bytes: u32, source: *const c_void) -> bool {
        let destination = match host() {
            // SAFETY: the lent source of the registered uploads.
            Some(h) if capture_enabled() => unsafe { (h.registered_source)(handle, bytes) },
            _ => null_mut(),
        };
        cvr_rc2_scope_enter(destination, bytes, source)
    }
    /// ~RegisteredUploadScope of an entered scope: the innermost popped.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_scope_leave() {
        EXPECTED.with(|e| {
            e.borrow_mut().pop();
        });
    }

    static INTENT_OBSERVED: AtomicBool = AtomicBool::new(false);
    static INTENT_CONFIGURED: Once = Once::new();
    /// IntentConfigured: the source intent requested (switched on once when both the selectors and the family manifest
    /// are configured); a request is remembered for the poll's cleanup.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_intent_configured() -> bool {
        let Some(h) = host() else { return false };
        // SAFETY: the lent exported switch.
        let enable = unsafe { &*h.intent_enable };
        INTENT_CONFIGURED.call_once(|| {
            if std::env::var_os("CVR_STEREO_SOURCE_SELECTORS").is_some() && std::env::var_os("CVR_STEREO_FAMILY_MANIFEST").is_some() {
                enable.store(1, Ordering::SeqCst);
            }
        });
        let requested = enable.load(Ordering::Acquire) != 0;
        if requested {
            INTENT_OBSERVED.store(true, Ordering::SeqCst);
        }
        requested
    }
    /// PollRegisteredIntent's `intentObserved.exchange(false)`.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_intent_observed_take() -> bool {
        INTENT_OBSERVED.swap(false, Ordering::SeqCst)
    }

    fn desc(resource: *mut c_void) -> D3D12_RESOURCE_DESC {
        // SAFETY: the caller's live resource.
        unsafe { ID3D12Resource::from_raw_borrowed(&resource).map(|r| r.GetDesc()).unwrap_or_default() }
    }
    /// CopyResourceBufferBytes: two whole buffers of the same width (the width), else 0.
    fn copy_resource_buffer_bytes(destination: &D3D12_RESOURCE_DESC, source: &D3D12_RESOURCE_DESC) -> u64 {
        let buffer = |v: &D3D12_RESOURCE_DESC| {
            v.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
                && v.Width != 0
                && v.Height == 1
                && v.DepthOrArraySize == 1
                && v.MipLevels == 1
                && v.Format == DXGI_FORMAT_UNKNOWN
                && v.SampleDesc.Count == 1
                && v.SampleDesc.Quality == 0
                && v.Layout == D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        };
        if buffer(destination) && buffer(source) && destination.Width == source.Width { destination.Width } else { 0 }
    }

    /// ObserveRegisteredCopy: the source intent's copy when configured (alone unless the legacy capture is on), then the
    /// ledger's, against the innermost upload scope's expected bytes.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_observe_copy(list: *mut c_void, destination: *mut c_void, offset: u64, source: *mut c_void, source_offset: u64, size: u64) {
        let Some(h) = host() else { return };
        if internal() || destination.is_null() || source.is_null() {
            return;
        }
        let legacy = capture_enabled();
        if cvr_rc2_intent_configured() {
            // SAFETY: the copy's live objects; the lent exported switch.
            unsafe { (h.intent_copy)(list, destination, offset, source, source_offset, size, h.intent_enable) };
            if !legacy {
                return;
            }
        }
        EXPECTED.with(|e| {
            let e = e.borrow();
            let top = e.last();
            let (expected, bytes, length) = top.map_or((null_mut(), null(), 0), |(d, b)| (*d, b.as_ptr(), b.len()));
            // SAFETY: the table, the copy's live objects, the scope's bytes.
            unsafe { cvr_rc_observe_copy(counters(h), list, destination, offset, source, source_offset, size, legacy, expected, bytes, length, h.keep) };
        });
    }
    /// ObserveRegisteredResourceCopy: compatible whole buffers become one full-span observed copy; anything else
    /// invalidates the destination.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_observe_resource_copy(list: *mut c_void, destination: *mut c_void, source: *mut c_void) {
        if internal() || destination.is_null() {
            return;
        }
        let intent = cvr_rc2_intent_configured();
        if !intent && !capture_enabled() && !have_copies() {
            return;
        }
        if !source.is_null() {
            let bytes = copy_resource_buffer_bytes(&desc(destination), &desc(source));
            if bytes != 0 {
                cvr_rc2_observe_copy(list, destination, 0, source, 0, bytes);
                return;
            }
        }
        cvr_rc2_invalidate(list, destination);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_invalidate(list: *mut c_void, destination: *mut c_void) {
        let Some(h) = host() else { return };
        if internal() || destination.is_null() {
            return;
        }
        if cvr_rc2_intent_configured() {
            // SAFETY: a live resource.
            unsafe { (h.intent_invalidate)(destination) };
        }
        if !have_copies() {
            return;
        }
        // SAFETY: the live list and resource.
        unsafe { cvr_rc_invalidate(list, destination) };
    }
    /// RegisteredBarriers: an aliasing barrier invalidates the source intent's resources (both unnamed: all of it).
    ///
    /// # Safety
    /// `barriers` holds `count_of` barriers or is null.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_rc2_barriers(list: *mut c_void, count_of: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
        let Some(h) = host() else { return };
        if internal() || barriers.is_null() {
            return;
        }
        if cvr_rc2_intent_configured() {
            let raw = |p: &core::mem::ManuallyDrop<Option<ID3D12Resource>>| -> *mut c_void { p.as_ref().map_or(null_mut(), |r| r.as_raw()) };
            // SAFETY: the caller's barriers, each read by its type's union member.
            for b in unsafe { core::slice::from_raw_parts(barriers, count_of as usize) } {
                if b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING {
                    let alias = unsafe { &b.Anonymous.Aliasing };
                    let (before, after) = (raw(&alias.pResourceBefore), raw(&alias.pResourceAfter));
                    // SAFETY: live resources.
                    unsafe {
                        if !before.is_null() {
                            (h.intent_invalidate)(before);
                        }
                        if !after.is_null() {
                            (h.intent_invalidate)(after);
                        }
                        if before.is_null() && after.is_null() {
                            (h.intent_clear)();
                        }
                    }
                }
            }
        }
        if !have_copies() {
            return;
        }
        // SAFETY: the caller's barriers.
        unsafe { cvr_rc_barriers(list, count_of, barriers) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_reset(list: *mut c_void) {
        if have_copies() {
            // SAFETY: the live list.
            unsafe { cvr_rc_reset(list) };
        }
    }
    /// # Safety
    /// `lists` holds `count_of` lists or is null.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_rc2_submit(queue: *mut c_void, count_of: u32, lists: *const *mut c_void) {
        if let Some(h) = host().filter(|_| have_copies() && !queue.is_null() && !lists.is_null()) {
            // SAFETY: the table and the caller's submission.
            unsafe { cvr_rc_submit(counters(h), queue, count_of, lists) };
        }
    }
    /// ReadRegistered (one of the readers SetReader takes): an owned upload's bytes, else the ledger's.
    ///
    /// # Safety
    /// `output` has `bytes` writable bytes or is null; `receipt` writable or null.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_rc2_read(list: *mut c_void, address: u64, output: *mut c_void, bytes: usize, receipt: *mut u64) -> bool {
        if !receipt.is_null() {
            // SAFETY: as documented.
            unsafe { *receipt = 0 };
        }
        let Some(h) = host() else { return false };
        // SAFETY: as documented; the lent read of the owned uploads.
        if unsafe { (h.read_upload)(address, output, bytes) } {
            return true;
        }
        if output.is_null() || bytes == 0 || bytes > 65536 {
            return false;
        }
        // SAFETY: as documented.
        unsafe { cvr_rc_read(counters(h), list, address, output.cast(), bytes, receipt) }
    }
    /// ValidateRegistered (a reader).
    ///
    /// # Safety
    /// `expected` holds `bytes` bytes or is null.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_rc2_validate(receipt: u64, list: *mut c_void, address: u64, expected: *const c_void, bytes: usize) -> bool {
        let Some(h) = host() else { return false };
        // SAFETY: as documented.
        !expected.is_null() && bytes != 0 && unsafe { cvr_rc_validate(counters(h), receipt, list, address, expected.cast(), bytes) }
    }
    /// MarkRegisteredRead (a reader): the read's position, the probe's draw capacity its bound.
    #[no_mangle]
    pub extern "C" fn cvr_rc2_mark_read(list: *mut c_void) -> u64 {
        if list.is_null() || !capture_enabled() {
            return 0;
        }
        // SAFETY: a live list.
        unsafe { cvr_rc_mark_read(list, 4096) }
    }
    /// ResolveRegisteredRead (a reader): the timeline's result at the read's position.
    ///
    /// # Safety
    /// `output` has `bytes` writable bytes or is null.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_rc2_resolve(position: u64, list: *mut c_void, address: u64, output: *mut c_void, bytes: usize) -> Resolved {
        let mut result = Resolved { status: 0, payload: 0, offset: 0, detail: 0 };
        if let Some(h) = host() {
            // SAFETY: as documented.
            unsafe { cvr_rc_resolve(counters(h), position, list, null_mut(), address, output.cast(), bytes, &mut result) };
        }
        result
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_tracking() -> bool {
        tracking()
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_register_fence(fence: *mut c_void) {
        if !fence.is_null() && tracking() {
            // SAFETY: a live fence.
            unsafe { cvr_rc_register_fence(fence) };
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_signal(queue: *mut c_void, fence: *mut c_void, value: u64) {
        if let Some(h) = host().filter(|_| tracking()) {
            // SAFETY: the table and the live queue and fence.
            unsafe { cvr_rc_queue_fence(counters(h), false, queue, fence, value) };
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_wait(queue: *mut c_void, fence: *mut c_void, value: u64) {
        if let Some(h) = host().filter(|_| tracking()) {
            // SAFETY: as above.
            unsafe { cvr_rc_queue_fence(counters(h), true, queue, fence, value) };
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_rc2_cpu_signal(fence: *mut c_void, value: u64, caller: u64, nested: u32) {
        if let Some(h) = host().filter(|_| tracking()) {
            // SAFETY: the table and the live fence.
            unsafe { cvr_rc_cpu_signal(counters(h), fence, value, caller, nested) };
        }
    }
}
