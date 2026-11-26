//! The geometry write receipt (`include/Render/GeometryWriteReceipt.hpp`): whether a geometry range the main view read
//! is unchanged when the peer view reads it. Recordings per command list (16384) from an observed Reset, positioned
//! writes, alias barriers and unknown writes; actual queue submissions (one ExecuteCommandLists call proving no order
//! between its lists), fence signals and waits mirrored into the GPU queue order. `Check` admits a receipt only for
//! known, submitted, unreplayed recordings whose queue ancestry is closed (no future signal, no cycle), the main read
//! before the peer read, and no possibly intersecting or aliasing write ordered between them. The caller serializes
//! recording and queue evidence, supplies stable lifetime IDs and retains the resources, heaps and fences; any failed
//! observation faults the receipt until `Clear` starts a new epoch.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `GeometryWriteReceipt` owns this Rust state (copies copy it) and forwards to
//! `cvr_gwr_*`; `tools/single_pass_tests/geometry_write_receipt_test` runs its cases against it in its Rust variant.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::constant_timeline::QueueOrder;

pub const CAPACITY: usize = 16384;

/// GeometryWriteReceipt::Status.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unchanged,
    MissingProvenance,
    Pending,
    Abandoned,
    Replayed,
    Incomparable,
    IntersectingWrite,
    Aliased,
    Overflow,
}
/// GeometryWriteReceipt::Position.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub epoch: u64,
    pub recording: u32,
    pub command: u64,
}
/// GeometryWriteReceipt::MemorySpan.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemorySpan {
    pub heap: u64,
    pub offset: u64,
    pub bytes: u64,
}
/// GeometryWriteReceipt::Range (for placed buffers the allocation begins at resource byte zero).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub resource: u64,
    pub offset: u64,
    pub bytes: u64,
    pub allocation: MemorySpan,
    pub committed: bool,
}
/// GeometryWriteReceipt::Receipt.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Receipt {
    pub main: Position,
    pub range: Range,
    pub valid: bool,
}
const _: () = assert!(
    core::mem::size_of::<Position>() == 24
        && core::mem::size_of::<MemorySpan>() == 24
        && core::mem::size_of::<Range>() == 56
        && core::mem::size_of::<Receipt>() == 88
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Write,
    Alias,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    Before,
    After,
    Equal,
    Pending,
    Abandoned,
    Replayed,
    Incomparable,
}
#[derive(Clone, Copy, Debug, Default)]
struct Recording {
    commands: u64,
    queue: u64,
    batch: u64,
    index: u32,
    node: u32,
    order_node: u32,
    known: bool,
    submitted: bool,
    abandoned: bool,
    replayed: bool,
}
#[derive(Clone, Copy, Debug)]
struct Event {
    point: Position,
    range: Range,
    memory: MemorySpan,
    kind: Kind,
}
#[derive(Clone, Copy, Debug, Default)]
struct QueueNode {
    previous: u32,
    fence: u64,
    value: u64,
}
#[derive(Clone, Debug, Default)]
struct FenceState {
    baseline: u64,
    completed: u64,
    owner: u64,
    highest: u64,
    invalid: bool,
    signals: BTreeMap<u64, u32>,
}

/// GeometryWriteReceipt.
#[derive(Clone, Debug)]
pub struct WriteReceipt {
    epoch: u64,
    batch: u64,
    fault: Status,
    current: BTreeMap<u64, u32>,
    queues: BTreeMap<u64, u32>,
    recordings: Vec<Recording>,
    events: Vec<Event>,
    nodes: Vec<QueueNode>,
    fences: BTreeMap<u64, FenceState>,
    closed: RefCell<BTreeMap<u32, Status>>,
    order: QueueOrder,
}
impl Default for WriteReceipt {
    fn default() -> Self {
        WriteReceipt {
            epoch: 1,
            batch: 0,
            fault: Status::Unchanged,
            current: BTreeMap::new(),
            queues: BTreeMap::new(),
            recordings: vec![Recording::default()],
            events: Vec::new(),
            nodes: vec![QueueNode::default()],
            fences: BTreeMap::new(),
            closed: RefCell::default(),
            order: QueueOrder::default(),
        }
    }
}

fn valid_memory(m: MemorySpan) -> bool {
    m.heap != 0 && m.bytes != 0 && m.offset <= u64::MAX - m.bytes
}
fn empty_memory(m: MemorySpan) -> bool {
    m.heap == 0 && m.offset == 0 && m.bytes == 0
}
fn valid_range(r: &Range, require_allocation: bool) -> bool {
    if r.resource == 0 || r.bytes == 0 || r.offset > u64::MAX - r.bytes {
        return false;
    }
    if r.committed {
        return empty_memory(r.allocation);
    }
    if empty_memory(r.allocation) {
        return !require_allocation;
    }
    valid_memory(r.allocation)
        && r.offset <= r.allocation.bytes
        && r.bytes <= r.allocation.bytes - r.offset
}
fn intersects(a: u64, na: u64, b: u64, nb: u64) -> bool {
    a < b.wrapping_add(nb) && b < a.wrapping_add(na)
}
/// GeometryWriteReceipt::Overlap.
fn overlap(read: &Range, event: &Event) -> Status {
    match event.kind {
        Kind::Unknown => Status::MissingProvenance,
        Kind::Alias => {
            if !valid_memory(event.memory) {
                return Status::Aliased;
            }
            if read.committed || read.allocation.heap != event.memory.heap {
                return Status::Unchanged;
            }
            if intersects(
                read.allocation.offset.wrapping_add(read.offset),
                read.bytes,
                event.memory.offset,
                event.memory.bytes,
            ) {
                Status::Aliased
            } else {
                Status::Unchanged
            }
        }
        Kind::Write => {
            let write = &event.range;
            if read.resource == write.resource {
                return if intersects(read.offset, read.bytes, write.offset, write.bytes) {
                    Status::IntersectingWrite
                } else {
                    Status::Unchanged
                };
            }
            if read.committed || write.committed {
                return Status::Unchanged;
            }
            if !valid_memory(write.allocation) {
                return Status::MissingProvenance;
            }
            if read.allocation.heap != write.allocation.heap {
                return Status::Unchanged;
            }
            if intersects(
                read.allocation.offset.wrapping_add(read.offset),
                read.bytes,
                write.allocation.offset.wrapping_add(write.offset),
                write.bytes,
            ) {
                Status::Aliased
            } else {
                Status::Unchanged
            }
        }
    }
}

impl WriteReceipt {
    fn mutate(&mut self, action: impl FnOnce(&mut Self)) {
        if self.fault != Status::Unchanged {
            return;
        }
        self.closed.get_mut().clear();
        action(self);
        if self.order.failed() {
            self.fault = Status::Overflow;
        }
    }
    fn new_recording(&mut self, list: u64, known: bool) -> u32 {
        if self.recordings.len() > CAPACITY {
            self.fault = Status::Overflow;
            return 0;
        }
        let id = self.recordings.len() as u32;
        self.recordings.push(Recording {
            known,
            ..Default::default()
        });
        self.current.insert(list, id);
        id
    }
    fn valid(&self, p: Position) -> bool {
        p.epoch == self.epoch
            && p.recording != 0
            && (p.recording as usize) < self.recordings.len()
            && p.command != 0
            && p.command <= self.recordings[p.recording as usize].commands
    }
    fn add(&mut self, event: Event) -> bool {
        let mut result = false;
        self.mutate(|s| {
            let recording = event.point.recording as usize;
            if !s.valid(event.point)
                || s.recordings[recording].submitted
                || s.recordings[recording].abandoned
            {
                s.fault = Status::MissingProvenance;
                return;
            }
            if s.events.len() >= CAPACITY {
                s.fault = Status::Overflow;
                return;
            }
            s.events.push(event);
            result = true;
        });
        result
    }
    fn append(&mut self, queue: u64, fence: u64, value: u64) -> u32 {
        if queue == 0 {
            self.fault = Status::MissingProvenance;
            return 0;
        }
        if self.nodes.len() > CAPACITY {
            self.fault = Status::Overflow;
            return 0;
        }
        let id = self.nodes.len() as u32;
        let previous = *self.queues.entry(queue).or_insert(0);
        self.nodes.push(QueueNode {
            previous,
            fence,
            value,
        });
        self.queues.insert(queue, id);
        id
    }

    pub fn reset(&mut self, list: u64) {
        self.mutate(|s| {
            if list == 0 {
                s.fault = Status::MissingProvenance;
                return;
            }
            if let Some(&old) = s.current.get(&list) {
                if !s.recordings[old as usize].submitted {
                    s.recordings[old as usize].abandoned = true;
                }
            }
            s.new_recording(list, true);
        });
    }
    pub fn mark(&mut self, list: u64) -> Position {
        let mut result = Position::default();
        self.mutate(|s| {
            let Some(&found) = s.current.get(&list) else {
                return;
            };
            let epoch = s.epoch;
            let r = &mut s.recordings[found as usize];
            if !r.known || r.submitted || r.abandoned {
                return;
            }
            if r.commands == u64::MAX {
                s.fault = Status::Overflow;
                return;
            }
            r.commands += 1;
            result = Position {
                epoch,
                recording: found,
                command: r.commands,
            };
        });
        result
    }
    pub fn write(&mut self, point: Position, range: Range) -> bool {
        if self.fault != Status::Unchanged {
            return false;
        }
        if !valid_range(&range, false) {
            self.fault = Status::MissingProvenance;
            return false;
        }
        self.add(Event {
            point,
            range,
            memory: MemorySpan::default(),
            kind: Kind::Write,
        })
    }
    /// An empty or incomplete alias span cannot exclude any watched allocation.
    pub fn alias(&mut self, point: Position, memory: MemorySpan) -> bool {
        self.add(Event {
            point,
            range: Range::default(),
            memory,
            kind: Kind::Alias,
        })
    }
    pub fn unknown_write(&mut self, point: Position) -> bool {
        self.add(Event {
            point,
            range: Range::default(),
            memory: MemorySpan::default(),
            kind: Kind::Unknown,
        })
    }
    pub fn capture(&self, main: Position, range: Range) -> Receipt {
        if self.fault == Status::Unchanged && self.valid(main) && valid_range(&range, true) {
            Receipt {
                main,
                range,
                valid: true,
            }
        } else {
            Receipt::default()
        }
    }
    pub fn submit(&mut self, queue: u64, lists: &[u64]) -> bool {
        let mut result = false;
        self.mutate(|s| {
            if queue == 0 || lists.is_empty() {
                s.fault = Status::MissingProvenance;
                return;
            }
            if lists.len() > CAPACITY {
                s.fault = Status::Overflow;
                return;
            }
            if s.batch == u64::MAX {
                s.fault = Status::Overflow;
                return;
            }
            s.batch += 1;
            let batch = s.batch;
            let node = s.append(queue, 0, 0);
            let order_node = s.order.submit(queue);
            if node == 0 || order_node == 0 {
                return;
            }
            for (i, &list) in lists.iter().enumerate() {
                if list == 0 || i > u32::MAX as usize {
                    s.fault = Status::MissingProvenance;
                    return;
                }
                let found = match s.current.get(&list) {
                    Some(&found) => found,
                    None => {
                        let id = s.new_recording(list, false);
                        if id == 0 {
                            return;
                        }
                        s.recordings[id as usize].commands = 1;
                        let epoch = s.epoch;
                        if !s.add(Event {
                            point: Position {
                                epoch,
                                recording: id,
                                command: 1,
                            },
                            range: Range::default(),
                            memory: MemorySpan::default(),
                            kind: Kind::Unknown,
                        }) {
                            return;
                        }
                        s.current[&list]
                    }
                };
                let r = &mut s.recordings[found as usize];
                if r.submitted {
                    r.replayed = true;
                    continue;
                }
                r.queue = queue;
                r.batch = batch;
                r.index = i as u32;
                r.node = node;
                r.order_node = order_node;
                r.submitted = true;
            }
            result = true;
        });
        result && self.fault == Status::Unchanged
    }
    pub fn fence(&mut self, fence: u64, completed: u64) {
        self.mutate(|s| {
            if fence == 0 || completed == u64::MAX {
                s.fault = Status::MissingProvenance;
                return;
            }
            if s.fences.contains_key(&fence) {
                return;
            }
            if s.fences.len() >= CAPACITY {
                s.fault = Status::Overflow;
                return;
            }
            s.fences.insert(
                fence,
                FenceState {
                    baseline: completed,
                    completed,
                    owner: 0,
                    highest: completed,
                    ..Default::default()
                },
            );
            s.order.fence(fence, completed);
        });
    }
    pub fn signal(&mut self, queue: u64, fence: u64, value: u64) {
        self.mutate(|s| {
            if !s.fences.contains_key(&fence) {
                s.fault = Status::MissingProvenance;
                return;
            }
            let node = s.append(queue, 0, 0);
            if node == 0 {
                return;
            }
            let f = s.fences.get_mut(&fence).unwrap();
            if value <= f.highest || (f.owner != 0 && f.owner != queue) {
                f.invalid = true;
            } else {
                f.owner = queue;
                f.highest = value;
                f.signals.entry(value).or_insert(node);
            }
            s.order.signal(queue, fence, value);
        });
    }
    pub fn wait(&mut self, queue: u64, fence: u64, value: u64) {
        self.mutate(|s| {
            if !s.fences.contains_key(&fence) {
                s.fault = Status::MissingProvenance;
                return;
            }
            if s.append(queue, fence, value) != 0 {
                s.order.wait(queue, fence, value);
            }
        });
    }
    pub fn progress(&mut self, fence: u64, completed: u64) {
        self.mutate(|s| {
            let Some(f) = s.fences.get_mut(&fence) else {
                s.fault = Status::MissingProvenance;
                return;
            };
            if completed == u64::MAX || completed < f.completed {
                f.invalid = true;
                s.order.invalidate(fence);
                return;
            }
            f.completed = completed;
            s.order.progress(fence, completed);
        });
    }
    pub fn invalidate_fence(&mut self, fence: u64) {
        self.mutate(|s| {
            let Some(f) = s.fences.get_mut(&fence) else {
                s.fault = Status::MissingProvenance;
                return;
            };
            f.invalid = true;
            s.order.invalidate(fence);
        });
    }
    pub fn check(&self, receipt: &Receipt, peer: Position) -> Status {
        if self.fault != Status::Unchanged {
            return self.fault;
        }
        if self.order.failed() {
            return Status::Overflow;
        }
        if !receipt.valid
            || !self.valid(receipt.main)
            || !self.valid(peer)
            || !valid_range(&receipt.range, true)
        {
            return Status::MissingProvenance;
        }
        for point in [receipt.main, peer] {
            let r = &self.recordings[point.recording as usize];
            if !r.known {
                return Status::MissingProvenance;
            }
            if r.abandoned {
                return Status::Abandoned;
            }
            if r.replayed {
                return Status::Replayed;
            }
            if !r.submitted {
                return Status::Pending;
            }
            let closed = self.closed(r.node);
            if closed != Status::Unchanged {
                return closed;
            }
        }
        let reads = self.before(receipt.main, peer);
        if reads != Relation::Before {
            return if reads == Relation::Pending {
                Status::Pending
            } else {
                Status::Incomparable
            };
        }
        for event in &self.events {
            let conflict = overlap(&receipt.range, event);
            if conflict == Status::Unchanged {
                continue;
            }
            let before_main = self.before(event.point, receipt.main);
            if before_main == Relation::Before || before_main == Relation::Abandoned {
                continue;
            }
            if before_main == Relation::Replayed {
                return Status::Replayed;
            }
            let before_peer = self.before(event.point, peer);
            if before_peer == Relation::After {
                continue;
            }
            if before_main == Relation::Pending || before_peer == Relation::Pending {
                return Status::Pending;
            }
            if before_main == Relation::Incomparable || before_peer == Relation::Incomparable {
                return Status::Incomparable;
            }
            return conflict;
        }
        Status::Unchanged
    }
    /// Invalidates all receipts: a new epoch with nothing observed yet.
    pub fn clear(&mut self) {
        if self.epoch == u64::MAX {
            self.fault = Status::Overflow;
            return;
        }
        self.epoch += 1;
        self.batch = 0;
        self.current.clear();
        self.recordings = vec![Recording::default()];
        self.events.clear();
        self.queues.clear();
        self.nodes = vec![QueueNode::default()];
        self.fences.clear();
        self.closed.get_mut().clear();
        self.order.clear();
        self.fault = Status::Unchanged;
    }
    fn before(&self, a: Position, b: Position) -> Relation {
        let (x, y) = (
            &self.recordings[a.recording as usize],
            &self.recordings[b.recording as usize],
        );
        if x.abandoned {
            return Relation::Abandoned;
        }
        if x.replayed || y.replayed {
            return Relation::Replayed;
        }
        if !x.submitted || !y.submitted {
            return Relation::Pending;
        }
        if x.queue == y.queue {
            // One ExecuteCommandLists call can merge/overlap different lists.
            // Without barrier evidence, their array indices prove no ordering.
            if x.batch == y.batch && a.recording != b.recording {
                return Relation::Incomparable;
            }
            let (left, right) = ((x.batch, x.index, a.command), (y.batch, y.index, b.command));
            return if left < right {
                Relation::Before
            } else if left > right {
                Relation::After
            } else {
                Relation::Equal
            };
        }
        let before = self.order.reaches(y.order_node, x.order_node);
        let after = self.order.reaches(x.order_node, y.order_node);
        if before.before && after.before {
            return Relation::Incomparable;
        }
        if before.before {
            return Relation::Before;
        }
        if after.before {
            return Relation::After;
        }
        if before.pending || after.pending {
            Relation::Pending
        } else {
            Relation::Incomparable
        }
    }
    /// A known MAIN predecessor does not close another pending wait branch:
    /// every ancestor is walked, rejecting future signals and dependency cycles.
    fn closed(&self, node: u32) -> Status {
        if let Some(&found) = self.closed.borrow().get(&node) {
            return found;
        }
        let result = self.walk_closed(node);
        let mut closed = self.closed.borrow_mut();
        if closed.len() < CAPACITY {
            closed.entry(node).or_insert(result);
        }
        result
    }
    fn walk_closed(&self, node: u32) -> Status {
        let mut visited = vec![0u8; self.nodes.len()];
        let mut stack = vec![(node, false)];
        while let Some((id, leaving)) = stack.pop() {
            if id == 0 {
                continue;
            }
            if leaving {
                visited[id as usize] = 2;
                continue;
            }
            if visited[id as usize] == 2 {
                continue;
            }
            if visited[id as usize] == 1 {
                return Status::Incomparable;
            }
            visited[id as usize] = 1;
            stack.push((id, true));
            let n = &self.nodes[id as usize];
            if n.previous != 0 {
                stack.push((n.previous, false));
            }
            if n.fence == 0 {
                continue;
            }
            let Some(fence) = self.fences.get(&n.fence).filter(|fence| !fence.invalid) else {
                return Status::MissingProvenance;
            };
            if n.value <= fence.baseline || n.value <= fence.completed {
                continue;
            }
            let Some(&signal) = fence.signals.get(&n.value) else {
                return Status::Pending;
            };
            stack.push((signal, false));
        }
        Status::Unchanged
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// A new receipt state (released by `cvr_gwr_free`), or a copy of `from` when non-null.
///
/// # Safety
/// `from` is null or valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_new(from: *const WriteReceipt) -> *mut WriteReceipt {
    Box::into_raw(Box::new(if from.is_null() {
        WriteReceipt::default()
    } else {
        unsafe { (*from).clone() }
    }))
}
/// # Safety
/// `receipt` came from `cvr_gwr_new` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_free(receipt: *mut WriteReceipt) {
    if !receipt.is_null() {
        drop(unsafe { Box::from_raw(receipt) });
    }
}
/// Reset (`which` 0, `a` the list), Fence (1), Signal (2), Wait (3), Progress (4), InvalidateFence (5), Clear (6).
///
/// # Safety
/// `receipt` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_op(
    receipt: *mut WriteReceipt,
    which: u32,
    a: u64,
    b: u64,
    c: u64,
) {
    let receipt = unsafe { &mut *receipt };
    match which {
        0 => receipt.reset(a),
        1 => receipt.fence(a, b),
        2 => receipt.signal(a, b, c),
        3 => receipt.wait(a, b, c),
        4 => receipt.progress(a, b),
        5 => receipt.invalidate_fence(a),
        _ => receipt.clear(),
    }
}
/// GeometryWriteReceipt::Mark.
///
/// # Safety
/// `receipt` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_mark(
    receipt: *mut WriteReceipt,
    list: u64,
    out: *mut Position,
) {
    unsafe { *out = (*receipt).mark(list) }
}
/// Write (`which` 0, `range`), Alias (1, `memory`), UnknownWrite (2).
///
/// # Safety
/// `receipt` and `point` are valid; `range` / `memory` are valid where used.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_add(
    receipt: *mut WriteReceipt,
    which: u32,
    point: *const Position,
    range: *const Range,
    memory: *const MemorySpan,
) -> bool {
    unsafe {
        let receipt = &mut *receipt;
        match which {
            0 => receipt.write(*point, *range),
            1 => receipt.alias(*point, *memory),
            _ => receipt.unknown_write(*point),
        }
    }
}
/// GeometryWriteReceipt::Capture.
///
/// # Safety
/// The pointers are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_capture(
    receipt: *const WriteReceipt,
    main: *const Position,
    range: *const Range,
    out: *mut Receipt,
) {
    unsafe { *out = (*receipt).capture(*main, *range) }
}
/// GeometryWriteReceipt::Submit.
///
/// # Safety
/// `receipt` is valid; `lists` holds `count` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_submit(
    receipt: *mut WriteReceipt,
    queue: u64,
    lists: *const u64,
    count: usize,
) -> bool {
    unsafe {
        let lists = if lists.is_null() || count == 0 {
            &[][..]
        } else {
            core::slice::from_raw_parts(lists, count)
        };
        (*receipt).submit(queue, lists)
    }
}
/// GeometryWriteReceipt::Check.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gwr_check(
    receipt: *const WriteReceipt,
    value: *const Receipt,
    peer: *const Position,
) -> u32 {
    unsafe { (*receipt).check(&*value, *peer) as u32 }
}
