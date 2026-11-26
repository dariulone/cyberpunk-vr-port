//! The GPU queue order and the constant write timeline (`include/Render/GpuQueueOrder.hpp`,
//! `include/Render/ConstantWriteTimeline.hpp`).
//!
//! GpuQueueOrder: a bounded graph (32768 nodes) of witnessed queue operations -- submissions, fence signals and waits
//! -- each node after its queue's previous one, a submission importing the latest signal each valid fence has completed;
//! a fence usable only while one queue signals it monotonically above the completed value it was first seen at.
//! `Reaches` walks backwards from a node: a wait follows only the exact signal of its value (a higher observed signal is
//! not proof), an unknown producer leaves it pending until the fence completes past the value.
//!
//! ConstantWriteTimeline: per command list recordings (16384) of positioned constant writes and alias barriers, the
//! actual queue submission order and the witnessed payloads; `Resolve` finds the maximal writes before a read (CPU
//! recording order deliberately absent from cross-list comparisons), refusing replayed, cross-queue, pending, partial
//! or alias-clobbered ones.
//!
//! In the Rust build (`CVR_RUST_VRIK`) both classes own these Rust states (copied by value) and forward to `cvr_gqo_*`
//! / `cvr_cwt_*`; `tools/single_pass_tests/constant_timeline_test` runs its cases against them in its Rust variant.

use std::cell::RefCell;
use std::collections::BTreeMap;

// ---- GpuQueueOrder -----------------------------------------------------------------------------------------------

/// GpuQueueOrder::Reach.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reach {
    pub before: bool,
    pub pending: bool,
}
pub const ORDER_CAPACITY: usize = 32768;

#[derive(Clone, Debug, Default)]
struct OrderNode {
    previous: u32,
    fence: u64,
    value: u64,
    completed: Vec<u32>,
}
#[derive(Clone, Debug, Default)]
struct FenceState {
    baseline: u64,
    highest: u64,
    owner: u64,
    completed: u64,
    invalid: bool,
    signals: BTreeMap<u64, u32>,
}

/// GpuQueueOrder.
#[derive(Clone, Debug)]
pub struct QueueOrder {
    nodes: Vec<OrderNode>,
    last: BTreeMap<u64, u32>,
    fences: BTreeMap<u64, FenceState>,
    cache: RefCell<BTreeMap<u64, Reach>>,
    failed: bool,
}
impl Default for QueueOrder {
    fn default() -> Self {
        QueueOrder {
            nodes: vec![OrderNode::default()],
            last: BTreeMap::new(),
            fences: BTreeMap::new(),
            cache: RefCell::default(),
            failed: false,
        }
    }
}
impl QueueOrder {
    pub fn submit(&mut self, queue: u64) -> u32 {
        self.append(queue, 0, 0, true)
    }
    pub fn fence(&mut self, fence: u64, completed: u64) {
        if fence == 0 || self.fences.contains_key(&fence) {
            return;
        }
        if self.fences.len() >= ORDER_CAPACITY {
            self.failed = true;
            return;
        }
        self.fences.insert(
            fence,
            FenceState {
                baseline: completed,
                highest: completed,
                owner: 0,
                completed,
                ..Default::default()
            },
        );
        self.cache.get_mut().clear();
    }
    pub fn signal(&mut self, queue: u64, fence: u64, value: u64) {
        let point = self.append(queue, 0, 0, false);
        let Some(state) = self.fences.get_mut(&fence) else {
            self.failed = true;
            return;
        };
        if point == 0 || value <= state.highest || (state.owner != 0 && state.owner != queue) {
            state.invalid = true;
            return;
        }
        state.owner = queue;
        state.highest = value;
        state.signals.entry(value).or_insert(point);
    }
    pub fn wait(&mut self, queue: u64, fence: u64, value: u64) {
        self.append(queue, fence, value, false);
    }
    pub fn invalidate(&mut self, fence: u64) {
        if let Some(state) = self.fences.get_mut(&fence) {
            state.invalid = true;
        }
        self.cache.get_mut().clear();
    }
    pub fn progress(&mut self, fence: u64, completed: u64) {
        let Some(state) = self.fences.get_mut(&fence) else {
            return;
        };
        if state.completed != completed {
            state.completed = completed;
            self.cache.get_mut().clear();
        }
    }
    pub fn reaches(&self, from: u32, target: u32) -> Reach {
        if self.failed
            || from == 0
            || target == 0
            || from as usize >= self.nodes.len()
            || target as usize >= self.nodes.len()
        {
            return Reach::default();
        }
        let key = ((from as u64) << 32) | target as u64;
        if let Some(found) = self.cache.borrow().get(&key) {
            return *found;
        }
        let mut stack = vec![from];
        let mut visited = vec![false; self.nodes.len()];
        let mut result = Reach::default();
        while let Some(id) = stack.pop() {
            if id == 0 || visited[id as usize] {
                continue;
            }
            visited[id as usize] = true;
            if id == target {
                result.before = true;
                result.pending = false;
                break;
            }
            let node = &self.nodes[id as usize];
            if node.previous != 0 {
                stack.push(node.previous);
            }
            stack.extend_from_slice(&node.completed);
            if node.fence == 0 {
                continue;
            }
            let Some(fence) = self.fences.get(&node.fence) else {
                result.pending = true;
                continue;
            };
            if fence.invalid || node.value <= fence.baseline {
                continue;
            }
            // A higher observed signal is not proof: a lower pre-observation
            // signal may already have satisfied this wait. Completion of such
            // an unknown producer ends Pending, without inventing an edge.
            match fence.signals.get(&node.value) {
                None => result.pending |= fence.completed < node.value,
                Some(&signal) => stack.push(signal),
            }
        }
        let mut cache = self.cache.borrow_mut();
        if cache.len() < 65536 {
            cache.entry(key).or_insert(result);
        }
        result
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn clear(&mut self) {
        self.nodes = vec![OrderNode::default()];
        self.last.clear();
        self.fences.clear();
        self.cache.get_mut().clear();
        self.failed = false;
    }
    fn append(&mut self, queue: u64, fence: u64, value: u64, import_completed: bool) -> u32 {
        self.cache.get_mut().clear();
        if self.failed || queue == 0 || self.nodes.len() >= ORDER_CAPACITY {
            self.failed = true;
            return 0;
        }
        let previous = *self.last.entry(queue).or_insert(0);
        let mut node = OrderNode {
            previous,
            fence,
            value,
            completed: Vec::new(),
        };
        if import_completed {
            for observed in self.fences.values() {
                if observed.invalid || observed.completed <= observed.baseline {
                    continue;
                }
                if let Some((_, &signal)) =
                    observed.signals.range(..=observed.completed).next_back()
                {
                    node.completed.push(signal);
                }
            }
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(node);
        self.last.insert(queue, id);
        id
    }
}

// ---- ConstantWriteTimeline ---------------------------------------------------------------------------------------

/// ConstantWriteTimeline::Position.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub recording: u32,
    pub command: u64,
}
/// ConstantWriteTimeline::Status.
pub mod status {
    pub const READY: i32 = 0;
    pub const MISSING: i32 = 1;
    pub const PENDING: i32 = 2;
    pub const UNKNOWN_WRITE: i32 = 3;
    pub const CROSS_QUEUE: i32 = 4;
    pub const ABANDONED: i32 = 5;
    pub const OVERFLOW: i32 = 6;
    pub const REPLAYED: i32 = 7;
}
/// ConstantWriteTimeline::Detail.
pub mod detail {
    pub const NONE: i32 = 0;
    pub const UNKNOWN_WRITE: i32 = 1;
    pub const PARTIAL_RANGE: i32 = 2;
    pub const ALIAS_BARRIER: i32 = 3;
}
/// ConstantWriteTimeline::Result.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub status: i32,
    pub payload: u64,
    pub offset: u64,
    pub detail: i32,
}
impl Resolved {
    fn status(status: i32) -> Self {
        Resolved {
            status,
            payload: 0,
            offset: 0,
            detail: detail::NONE,
        }
    }
    fn unknown(detail: i32) -> Self {
        Resolved {
            status: status::UNKNOWN_WRITE,
            payload: 0,
            offset: 0,
            detail,
        }
    }
}
/// ConstantWriteTimeline::Context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub queue: u64,
    pub batch: u64,
    pub index: u32,
    pub node: u32,
}
/// ConstantWriteTimeline::RelatedWrite.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelatedWrite {
    pub id: u32,
    pub relation: u32,
    pub point: Position,
    pub context: Context,
    pub payload: u64,
}
/// ConstantWriteTimeline::MemorySpan.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemorySpan {
    pub heap: u64,
    pub offset: u64,
    pub bytes: u64,
}
const _: () = assert!(
    core::mem::size_of::<Position>() == 16
        && core::mem::size_of::<Resolved>() == 32
        && core::mem::size_of::<Context>() == 24
        && core::mem::size_of::<RelatedWrite>() == 56
        && core::mem::size_of::<MemorySpan>() == 24
);

/// ConstantWriteTimeline::CouldAlias.
pub fn could_alias(a: MemorySpan, b: MemorySpan) -> bool {
    if a.heap == 0
        || b.heap == 0
        || a.bytes == 0
        || b.bytes == 0
        || a.offset > u64::MAX - a.bytes
        || b.offset > u64::MAX - b.bytes
    {
        return true;
    }
    a.heap == b.heap && a.offset < b.offset + b.bytes && b.offset < a.offset + a.bytes
}
pub const TIMELINE_CAPACITY: usize = 16384;

#[derive(Clone, Copy, Debug, Default)]
struct Recording {
    commands: u64,
    queue: u64,
    batch: u64,
    index: u32,
    node: u32,
    submitted: bool,
    abandoned: bool,
    replayed: bool,
}
#[derive(Clone, Copy, Debug)]
struct WriteEvent {
    point: Position,
    address: u64,
    size: u64,
    payload: u64,
    committed: bool,
    alias: bool,
    detail: i32,
    memory: MemorySpan,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    Before,
    After,
    Pending,
    Unordered,
    Abandoned,
    Replayed,
}

/// ConstantWriteTimeline.
#[derive(Clone, Debug)]
pub struct Timeline {
    current: BTreeMap<u64, u32>,
    recordings: Vec<Recording>,
    writes: Vec<WriteEvent>,
    failed: bool,
    queues: BTreeMap<u64, bool>,
    submits: u32,
    replays: u32,
    order: QueueOrder,
}
impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            current: BTreeMap::new(),
            recordings: vec![Recording::default()],
            writes: Vec::new(),
            failed: false,
            queues: BTreeMap::new(),
            submits: 0,
            replays: 0,
            order: QueueOrder::default(),
        }
    }
}
impl Timeline {
    pub fn mark(&mut self, list: u64) -> Position {
        if self.failed || list == 0 {
            return Position::default();
        }
        let index = match self.current.get(&list) {
            Some(&index) => index,
            None => {
                if self.recordings.len() >= TIMELINE_CAPACITY {
                    self.failed = true;
                    return Position::default();
                }
                let index = self.recordings.len() as u32;
                self.current.insert(list, index);
                self.recordings.push(Recording::default());
                index
            }
        };
        let recording = &mut self.recordings[index as usize];
        if recording.submitted {
            self.failed = true;
            return Position::default();
        }
        recording.commands += 1;
        Position {
            recording: index,
            command: recording.commands,
        }
    }
    pub fn write(
        &mut self,
        point: Position,
        address: u64,
        size: u64,
        committed: bool,
        payload: u64,
    ) -> u32 {
        if self.failed
            || !self.valid(point)
            || address == 0
            || size == 0
            || address > u64::MAX - size
        {
            return 0;
        }
        if self.writes.len() >= TIMELINE_CAPACITY {
            self.failed = true;
            return 0;
        }
        self.writes.push(WriteEvent {
            point,
            address,
            size,
            payload,
            committed,
            alias: false,
            detail: detail::UNKNOWN_WRITE,
            memory: MemorySpan::default(),
        });
        self.writes.len() as u32
    }
    pub fn alias(&mut self, point: Position, memory: MemorySpan) {
        if self.failed || !self.valid(point) {
            return;
        }
        if self.writes.len() >= TIMELINE_CAPACITY {
            self.failed = true;
            return;
        }
        self.writes.push(WriteEvent {
            point,
            address: 0,
            size: 0,
            payload: 0,
            committed: false,
            alias: true,
            detail: detail::UNKNOWN_WRITE,
            memory,
        });
    }
    fn event(&mut self, write: u32) -> Option<&mut WriteEvent> {
        if write == 0 {
            None
        } else {
            self.writes.get_mut(write as usize - 1)
        }
    }
    pub fn payload(&mut self, write: u32, payload: u64) {
        if let Some(event) = self.event(write) {
            event.payload = payload;
        }
    }
    pub fn explanation(&mut self, write: u32, detail: i32) {
        if let Some(event) = self.event(write) {
            event.detail = detail;
        }
    }
    pub fn memory(&mut self, write: u32, memory: MemorySpan) {
        if let Some(event) = self.event(write) {
            event.memory = memory;
        }
    }
    pub fn reset(&mut self, list: u64) {
        let Some(index) = self.current.remove(&list) else {
            return;
        };
        let old = &mut self.recordings[index as usize];
        if !old.submitted {
            old.abandoned = true;
        }
    }
    pub fn submit(&mut self, queue: u64, batch: u64, lists: &[u64]) {
        if self.failed || queue == 0 || batch == 0 {
            return;
        }
        self.submits += 1;
        self.queues.insert(queue, true);
        let node = self.order.submit(queue);
        for (i, list) in lists.iter().enumerate() {
            let Some(&index) = self.current.get(list) else {
                continue;
            };
            let value = &mut self.recordings[index as usize];
            if value.submitted {
                value.replayed = true;
                self.replays += 1;
                continue;
            }
            value.queue = queue;
            value.batch = batch;
            value.index = i as u32;
            value.submitted = true;
            value.node = node;
        }
    }
    pub fn resolve(&self, read: Position, address: u64, bytes: u64) -> Resolved {
        if self.failed || self.order.failed() {
            return Resolved::status(status::OVERFLOW);
        }
        if !self.valid(read) || address == 0 || bytes == 0 || address > u64::MAX - bytes {
            return Resolved::status(status::MISSING);
        }
        let consumer = &self.recordings[read.recording as usize];
        if consumer.abandoned || consumer.replayed {
            return Resolved::status(status::ABANDONED);
        }
        if !consumer.submitted {
            return Resolved::status(status::PENDING);
        }
        let mut maximal: Vec<&WriteEvent> = Vec::new();
        let (mut pending, mut cross_queue, mut replayed) = (false, false, false);
        for write in &self.writes {
            if write.alias || !overlap(write, address, bytes) {
                continue;
            }
            match self.before(write.point, read) {
                Relation::Pending => {
                    pending = true;
                    continue;
                }
                Relation::Unordered => {
                    cross_queue = true;
                    continue;
                }
                Relation::Replayed => {
                    replayed = true;
                    continue;
                }
                Relation::Before => {}
                _ => continue,
            }
            // Old incomparable writes no longer matter after a complete write
            // proven later than both. Keep only maximal predecessors of read.
            if maximal
                .iter()
                .any(|other| self.before(write.point, other.point) == Relation::Before)
            {
                continue;
            }
            maximal.retain(|other| self.before(other.point, write.point) != Relation::Before);
            maximal.push(write);
        }
        for i in 0..maximal.len() {
            for j in i + 1..maximal.len() {
                let relation = self.before(maximal[i].point, maximal[j].point);
                pending |= relation == Relation::Pending;
                cross_queue |= relation != Relation::Pending;
            }
        }
        if replayed {
            return Resolved::status(status::REPLAYED);
        }
        if cross_queue {
            return Resolved::status(status::CROSS_QUEUE);
        }
        if pending {
            return Resolved::status(status::PENDING);
        }
        let Some(latest) = maximal.first() else {
            return Resolved::status(status::MISSING);
        };
        // Partial writes are not reconstructed from unrelated historical bytes.
        if latest.payload == 0 {
            return Resolved::unknown(latest.detail);
        }
        if address < latest.address
            || address - latest.address > latest.size
            || bytes > latest.size - (address - latest.address)
        {
            return Resolved::unknown(detail::PARTIAL_RANGE);
        }
        if !latest.committed {
            for write in self.writes.iter().filter(|write| write.alias) {
                if !could_alias(latest.memory, write.memory) {
                    continue;
                }
                match self.before(write.point, read) {
                    Relation::Pending => return Resolved::status(status::PENDING),
                    Relation::Unordered => return Resolved::status(status::CROSS_QUEUE),
                    Relation::Replayed => return Resolved::status(status::REPLAYED),
                    Relation::Before => match self.before(latest.point, write.point) {
                        Relation::Before => return Resolved::unknown(detail::ALIAS_BARRIER),
                        Relation::Pending => return Resolved::status(status::PENDING),
                        Relation::After => {}
                        _ => return Resolved::status(status::CROSS_QUEUE),
                    },
                    _ => {}
                }
            }
        }
        Resolved {
            status: status::READY,
            payload: latest.payload,
            offset: address - latest.address,
            detail: detail::NONE,
        }
    }
    pub fn stats(&self) -> [u32; 5] {
        [
            (self.recordings.len() - 1) as u32,
            self.writes.len() as u32,
            self.submits,
            self.replays,
            self.queues.len() as u32,
        ]
    }
    pub fn describe(&self, position: Position) -> Context {
        if !self.valid(position) {
            return Context::default();
        }
        let r = &self.recordings[position.recording as usize];
        Context {
            queue: r.queue,
            batch: r.batch,
            index: r.index,
            node: r.node,
        }
    }
    pub fn related(&self, read: Position, address: u64, bytes: u64) -> Vec<RelatedWrite> {
        if !self.valid(read) || address > u64::MAX - bytes {
            return Vec::new();
        }
        self.writes
            .iter()
            .enumerate()
            .filter(|(_, write)| !write.alias && overlap(write, address, bytes))
            .map(|(i, write)| RelatedWrite {
                id: i as u32 + 1,
                relation: self.before(write.point, read) as u32,
                point: write.point,
                context: self.describe(write.point),
                payload: write.payload,
            })
            .collect()
    }
    pub fn order(&mut self) -> &mut QueueOrder {
        &mut self.order
    }
    pub fn clear(&mut self) {
        self.current.clear();
        self.recordings = vec![Recording::default()];
        self.writes.clear();
        self.failed = false;
        self.queues.clear();
        self.submits = 0;
        self.replays = 0;
        self.order.clear();
    }
    fn valid(&self, point: Position) -> bool {
        point.recording != 0
            && (point.recording as usize) < self.recordings.len()
            && point.command != 0
            && point.command <= self.recordings[point.recording as usize].commands
    }
    fn less(&self, a: Position, b: Position) -> bool {
        let (x, y) = (
            &self.recordings[a.recording as usize],
            &self.recordings[b.recording as usize],
        );
        (x.batch, x.index, a.command) < (y.batch, y.index, b.command)
    }
    fn before(&self, a: Position, b: Position) -> Relation {
        let producer = &self.recordings[a.recording as usize];
        let consumer = &self.recordings[b.recording as usize];
        if producer.abandoned {
            return Relation::Abandoned;
        }
        if producer.replayed {
            return Relation::Replayed;
        }
        if !producer.submitted {
            return Relation::Pending;
        }
        if producer.queue != consumer.queue {
            let before = self.order.reaches(consumer.node, producer.node);
            if before.before {
                return Relation::Before;
            }
            let after = self.order.reaches(producer.node, consumer.node);
            if after.before {
                return Relation::After;
            }
            return if before.pending || after.pending {
                Relation::Pending
            } else {
                Relation::Unordered
            };
        }
        if self.less(a, b) {
            Relation::Before
        } else {
            Relation::After
        }
    }
}
fn overlap(write: &WriteEvent, address: u64, bytes: u64) -> bool {
    write.address < address.wrapping_add(bytes) && address < write.address.wrapping_add(write.size)
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// A new queue order (released by `cvr_gqo_free`), or a copy of `from` when non-null.
///
/// # Safety
/// `from` is null or valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gqo_new(from: *const QueueOrder) -> *mut QueueOrder {
    Box::into_raw(Box::new(if from.is_null() {
        QueueOrder::default()
    } else {
        unsafe { (*from).clone() }
    }))
}
/// # Safety
/// `order` came from `cvr_gqo_new` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gqo_free(order: *mut QueueOrder) {
    if !order.is_null() {
        drop(unsafe { Box::from_raw(order) });
    }
}
/// Submit (`which` 0, returns the node), Fence (1), Signal (2), Wait (3), Invalidate (4), Progress (5), Clear (6),
/// Failed (7, returns 1 or 0).
///
/// # Safety
/// `order` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gqo_op(
    order: *mut QueueOrder,
    which: u32,
    a: u64,
    b: u64,
    c: u64,
) -> u32 {
    let order = unsafe { &mut *order };
    match which {
        0 => return order.submit(a),
        1 => order.fence(a, b),
        2 => order.signal(a, b, c),
        3 => order.wait(a, b, c),
        4 => order.invalidate(a),
        5 => order.progress(a, b),
        6 => order.clear(),
        _ => return order.failed() as u32,
    }
    0
}
/// GpuQueueOrder::Reaches.
///
/// # Safety
/// `order` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gqo_reaches(
    order: *const QueueOrder,
    from: u32,
    target: u32,
    out: *mut Reach,
) {
    unsafe { *out = (*order).reaches(from, target) }
}

/// A new timeline (released by `cvr_cwt_free`), or a copy of `from` when non-null.
///
/// # Safety
/// `from` is null or valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_new(from: *const Timeline) -> *mut Timeline {
    Box::into_raw(Box::new(if from.is_null() {
        Timeline::default()
    } else {
        unsafe { (*from).clone() }
    }))
}
/// # Safety
/// `timeline` came from `cvr_cwt_new` (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_free(timeline: *mut Timeline) {
    if !timeline.is_null() {
        drop(unsafe { Box::from_raw(timeline) });
    }
}
/// ConstantWriteTimeline::Mark.
///
/// # Safety
/// `timeline` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_mark(
    timeline: *mut Timeline,
    list: u64,
    out: *mut Position,
) {
    unsafe { *out = (*timeline).mark(list) }
}
/// ConstantWriteTimeline::Write.
///
/// # Safety
/// `timeline` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_write(
    timeline: *mut Timeline,
    point: *const Position,
    address: u64,
    size: u64,
    committed: bool,
    payload: u64,
) -> u32 {
    unsafe { (*timeline).write(*point, address, size, committed, payload) }
}
/// ConstantWriteTimeline::Alias.
///
/// # Safety
/// `timeline` and `memory` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_alias(
    timeline: *mut Timeline,
    point: *const Position,
    memory: *const MemorySpan,
) {
    unsafe { (*timeline).alias(*point, *memory) }
}
/// Payload (`which` 0, `value`), Explanation (1, `value` as the detail), Memory (2, `memory`) of one write.
///
/// # Safety
/// `timeline` is valid; `memory` is valid for `which` 2.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_set(
    timeline: *mut Timeline,
    which: u32,
    write: u32,
    value: u64,
    memory: *const MemorySpan,
) {
    let timeline = unsafe { &mut *timeline };
    match which {
        0 => timeline.payload(write, value),
        1 => timeline.explanation(write, value as i32),
        _ => timeline.memory(write, unsafe { *memory }),
    }
}
/// ConstantWriteTimeline::Reset (`which` 0, `a` the list), Clear (1), Fence (2), Signal (3), Wait (4),
/// InvalidateFence (5), FenceProgress (6).
///
/// # Safety
/// `timeline` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_op(
    timeline: *mut Timeline,
    which: u32,
    a: u64,
    b: u64,
    c: u64,
) {
    let timeline = unsafe { &mut *timeline };
    match which {
        0 => timeline.reset(a),
        1 => timeline.clear(),
        2 => timeline.order().fence(a, b),
        3 => timeline.order().signal(a, b, c),
        4 => timeline.order().wait(a, b, c),
        5 => timeline.order().invalidate(a),
        _ => timeline.order().progress(a, b),
    }
}
/// ConstantWriteTimeline::Submit.
///
/// # Safety
/// `timeline` is valid; `lists` holds `count` values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_submit(
    timeline: *mut Timeline,
    queue: u64,
    batch: u64,
    lists: *const u64,
    count: usize,
) {
    unsafe {
        let lists = if lists.is_null() || count == 0 {
            &[][..]
        } else {
            core::slice::from_raw_parts(lists, count)
        };
        (*timeline).submit(queue, batch, lists)
    }
}
/// ConstantWriteTimeline::Resolve.
///
/// # Safety
/// `timeline` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_resolve(
    timeline: *const Timeline,
    read: *const Position,
    address: u64,
    bytes: u64,
    out: *mut Resolved,
) {
    unsafe { *out = (*timeline).resolve(*read, address, bytes) }
}
/// ConstantWriteTimeline::Stats.
///
/// # Safety
/// `timeline` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_stats(timeline: *const Timeline, out: *mut [u32; 5]) {
    unsafe { *out = (*timeline).stats() }
}
/// ConstantWriteTimeline::Describe.
///
/// # Safety
/// `timeline` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_describe(
    timeline: *const Timeline,
    position: *const Position,
    out: *mut Context,
) {
    unsafe { *out = (*timeline).describe(*position) }
}
/// ConstantWriteTimeline::Related: the count; up to `capacity` of them into `out`.
///
/// # Safety
/// `timeline` is valid; `out` has room for `capacity` writes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_related(
    timeline: *const Timeline,
    read: *const Position,
    address: u64,
    bytes: u64,
    out: *mut RelatedWrite,
    capacity: usize,
) -> usize {
    unsafe {
        let related = (*timeline).related(*read, address, bytes);
        for (i, write) in related.iter().take(capacity).enumerate() {
            *out.add(i) = *write;
        }
        related.len()
    }
}
/// The failed flag of the timeline (`which` 0) or of its queue order (1).
///
/// # Safety
/// `timeline` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_failed(timeline: *const Timeline, which: u32) -> bool {
    unsafe {
        if which == 0 {
            (*timeline).failed
        } else {
            (*timeline).order.failed()
        }
    }
}

/// ConstantWriteTimeline::CouldAlias.
///
/// # Safety
/// `a` and `b` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cwt_could_alias(
    a: *const MemorySpan,
    b: *const MemorySpan,
) -> bool {
    unsafe { could_alias(*a, *b) }
}
