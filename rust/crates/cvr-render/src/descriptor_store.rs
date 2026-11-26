//! The descriptor store (`include/Render/StereoDescriptorStore.hpp`, `descriptors`): the canonical descriptor values
//! interned by content (FNV-style hash, reference counted, retired through the owner's callback when the last slot lets
//! go; streaming CBVs overwritten in place when a slot is their sole owner), every tracked heap's slots (dense per-heap
//! arrays or a sparse map, within a 4 Mi slot budget), byte-exact per-page write version intervals, heap creation /
//! removal / tracked invalidation / writes / copies (overlap is unknown), and the versioned identity of a GPU range;
//! plus the borrowed-descriptor inventory, the mutation observation mode, the borrowed range tracking (atomics over the
//! C++ object), its details row, borrowed range equality and the shader range planner.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the C++ Store owns one of these and forwards every method to `cvr_ds_*`; the free
//! rules and BorrowedRangeTracking forward too. `tools/single_pass_tests/descriptor_store_test` runs its cases against
//! them in its Rust variant.

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::collections::{BTreeMap, HashMap};

/// A fast hasher for the store's u64 keys (descriptor addresses, value hashes): one multiply, high bits folded down.
#[derive(Clone, Copy, Default)]
struct KeyHasher(u64);
impl core::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    fn write_u64(&mut self, value: u64) {
        let mixed = (self.0 ^ value).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = mixed ^ (mixed >> 32);
    }
}
type KeyMap<V> = HashMap<u64, V, core::hash::BuildHasherDefault<KeyHasher>>;

pub type Value = [u64; 20];
pub const SLOT_LIMIT: usize = 4 * 1048576;
pub const VALUE_LIMIT: usize = 1048576;
const PAGE_BYTES: u64 = 4096;
const NO_HEAP: usize = usize::MAX;

/// descriptors::RangeIdentity.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RangeIdentity {
    pub heap: u64,
    pub cpu: u64,
    pub gpu: u64,
    pub version: u64,
    pub count: u32,
    pub stride: u32,
    pub valid: bool,
}
/// descriptors::BorrowedDescriptorInventory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BorrowedDescriptorInventory {
    pub slots: u64,
    pub unknown: u64,
    pub known_null: u64,
    pub resource_srv: u64,
    pub other_kinds: u64,
    pub unique_cookies: u64,
    pub tracked_cookies: u64,
    pub untracked_cookies: u64,
    pub texture_2d: u64,
    pub texture_2d_array: u64,
    pub flags_rt_ds_uav: u64,
    pub shape_unknown: u64,
}
/// descriptors::BorrowedRootRange.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BorrowedRootRange {
    pub parameter: u32,
    pub kind: u32,
    pub space: u32,
    pub shader_register: u32,
    pub count: u32,
    pub offset: u32,
    pub visibility: u32,
}
/// descriptors::BorrowedShaderUse.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BorrowedShaderUse {
    pub kind: u32,
    pub visibility: u32,
    pub space: u32,
    pub shader_register: u32,
    pub count: u32,
}
/// descriptors::BorrowedUsedRange.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BorrowedUsedRange {
    pub parameter: u32,
    pub kind: u32,
    pub offset: u32,
    pub count: u32,
    pub shader_declared_count: u32,
    pub physical_remainder: bool,
}
/// descriptors::BorrowedRangeTracking's members (std::atomic has its value's layout).
#[repr(C)]
pub struct BorrowedRangeTracking {
    pub epoch: AtomicU64,
    pub pins: AtomicU32,
    pub mutations: AtomicU32,
    pub ready: AtomicBool,
    pub unavailable: AtomicBool,
}
pub const PIN_LIMIT: u32 = 4096;
const _: () = assert!(
    core::mem::size_of::<RangeIdentity>() == 48
        && core::mem::size_of::<BorrowedDescriptorInventory>() == 96
        && core::mem::size_of::<BorrowedRootRange>() == 28
        && core::mem::size_of::<BorrowedShaderUse>() == 20
        && core::mem::size_of::<BorrowedUsedRange>() == 24
        && core::mem::size_of::<BorrowedRangeTracking>() == 24
);

/// InventoryBorrowedDescriptor: the counts, and the resource cookie of a resource SRV (a weak numeric label).
pub fn inventory_borrowed_descriptor(value: Option<&Value>, canonical_srv_valid: bool, out: &mut BorrowedDescriptorInventory) -> u64 {
    out.slots += 1;
    let Some(v) = value.filter(|v| v[0] != 0 && v[0] <= 4) else {
        out.unknown += 1;
        return 0;
    };
    if v[0] != 2 {
        out.other_kinds += 1;
        return 0;
    }
    if !canonical_srv_valid || v[1] & !15 != 0 {
        out.unknown += 1;
        return 0;
    }
    if v[1] & 1 == 0 {
        // An acceleration-structure SRV can have a null resource but a nonnull GPU address; not a null texture.
        if v[5] == 11 {
            out.other_kinds += 1;
            return 0;
        }
        if v[1] != 0 || v[2] != 0 || v[5] == 0 || v[5] > 10 {
            out.unknown += 1;
            return 0;
        }
        out.known_null += 1;
        return 0;
    }
    if v[2] == 0 {
        out.unknown += 1;
        return 0;
    }
    out.resource_srv += 1;
    let (dimension, height) = (v[13] as u32, v[15] as u32);
    let (layers, mips) = ((v[15] >> 32) as u16, (v[15] >> 48) as u16);
    let described = v[1] & 4 != 0 && (1..=4).contains(&dimension) && v[14] != 0 && height != 0 && layers != 0 && mips != 0 && v[17] as u32 != 0;
    if !described {
        out.shape_unknown += 1;
    } else {
        if dimension == 3 && v[5] == 4 {
            out.texture_2d += 1;
        }
        if dimension == 3 && v[5] == 5 {
            out.texture_2d_array += 1;
        }
        if v[16] as u32 & 7 != 0 {
            out.flags_rt_ds_uav += 1;
        }
    }
    v[2]
}
/// DescriptorMutationObservation: Ignore 0, Canonical 1, InvalidateTracked 2.
pub fn descriptor_mutation_observation(internal: bool, enabled: bool, borrowed: bool) -> u32 {
    if internal {
        return if borrowed { 2 } else { 0 };
    }
    if enabled || borrowed { 1 } else { 0 }
}
/// BorrowedTrackingDetails.
#[allow(clippy::too_many_arguments)]
pub fn borrowed_tracking_details(
    pinned: bool,
    ready: bool,
    saved_epoch: u64,
    current_epoch: u64,
    pins: u32,
    mutations: u32,
    versions: bool,
    overflow: u32,
    failure: u32,
    empty: bool,
) -> [u64; 4] {
    let reasons = (!pinned as u64)
        | ((!ready as u64) << 1)
        | (((saved_epoch != current_epoch) as u64) << 2)
        | (((mutations != 0) as u64) << 3)
        | ((!versions as u64) << 4)
        | (((overflow != 0) as u64) << 5)
        | (((failure != 0) as u64) << 6)
        | ((empty as u64) << 7);
    [u64::MAX, reasons, ((pins as u64) << 32) | mutations as u64, ((overflow as u64) << 32) | failure as u64]
}
/// SameBorrowedRange.
pub fn same_borrowed_range(a: &RangeIdentity, b: &RangeIdentity) -> bool {
    a.valid
        && b.valid
        && a.heap != 0
        && a.cpu != 0
        && a.gpu != 0
        && a.count != 0
        && a.stride != 0
        && a.heap == b.heap
        && a.cpu == b.cpu
        && a.gpu == b.gpu
        && a.version == b.version
        && a.count == b.count
        && a.stride == b.stride
}
/// PlanBorrowedShaderRanges: the merged used ranges (at most 256), or None.
pub fn plan_borrowed_shader_ranges(
    parameters: u32,
    supported: bool,
    direct_heap_indexing: bool,
    declarations: &[BorrowedRootRange],
    uses: &[BorrowedShaderUse],
) -> Option<Vec<BorrowedUsedRange>> {
    const LIMIT: u64 = u32::MAX as u64 + 1;
    if !supported || direct_heap_indexing || parameters == 0 || parameters > 64 || declarations.is_empty() || declarations.len() > 256
        || uses.is_empty() || uses.len() > 4096
    {
        return None;
    }
    let end = |range: &BorrowedRootRange| if range.count == u32::MAX { LIMIT } else { range.shader_register as u64 + range.count as u64 };
    for (i, range) in declarations.iter().enumerate() {
        if range.parameter >= parameters
            || range.kind > 3
            || range.visibility > 7
            || range.count == 0
            || (range.count != u32::MAX
                && (range.offset as u64 + range.count as u64 > LIMIT || range.shader_register as u64 + range.count as u64 > LIMIT))
        {
            return None;
        }
        for prior in &declarations[..i] {
            if prior.parameter == range.parameter && ((prior.kind == 3) != (range.kind == 3) || prior.count == u32::MAX) {
                return None;
            }
            let visible = prior.visibility == 0 || range.visibility == 0 || prior.visibility == range.visibility;
            if visible
                && prior.kind == range.kind
                && prior.space == range.space
                && (prior.shader_register as u64) < end(range)
                && (range.shader_register as u64) < end(prior)
            {
                return None;
            }
        }
    }
    let mut result = Vec::with_capacity(uses.len().min(256));
    for usage in uses {
        if usage.kind > 3 || (usage.visibility != 1 && usage.visibility != 5) || usage.count == 0
            || usage.shader_register as u64 + usage.count as u64 > LIMIT
        {
            return None;
        }
        let mut found: Option<&BorrowedRootRange> = None;
        for range in declarations {
            if range.kind != usage.kind
                || range.space != usage.space
                || (range.visibility != 0 && range.visibility != usage.visibility)
                || usage.shader_register < range.shader_register
            {
                continue;
            }
            let relative = usage.shader_register as u64 - range.shader_register as u64;
            if range.count != u32::MAX && (relative >= range.count as u64 || usage.count as u64 > range.count as u64 - relative) {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(range);
        }
        let matched = found?;
        let unbounded = matched.count == u32::MAX;
        let first = if unbounded {
            matched.offset as u64
        } else {
            matched.offset as u64 + usage.shader_register as u64 - matched.shader_register as u64
        };
        if first > u32::MAX as u64 {
            return None;
        }
        result.push(BorrowedUsedRange {
            parameter: matched.parameter,
            kind: usage.kind,
            offset: first as u32,
            count: if unbounded { u32::MAX } else { usage.count },
            shader_declared_count: usage.count,
            physical_remainder: unbounded,
        });
    }
    // std::sort is unstable; equal keys (parameter, offset, kind) merge identically either way.
    result.sort_by(|a, b| (a.parameter, a.offset, a.kind).cmp(&(b.parameter, b.offset, b.kind)));
    let mut merged: Vec<BorrowedUsedRange> = Vec::with_capacity(result.len().min(256));
    for range in result {
        if let Some(prior) = merged.last_mut().filter(|prior| prior.parameter == range.parameter) {
            let prior_end = if prior.physical_remainder { u64::MAX } else { prior.offset as u64 + prior.count as u64 };
            if (range.offset as u64) < prior_end && range.kind != prior.kind {
                return None;
            }
            if range.offset as u64 <= prior_end && range.kind == prior.kind {
                prior.physical_remainder = prior.physical_remainder || range.physical_remainder;
                prior.shader_declared_count = prior.shader_declared_count.max(range.shader_declared_count);
                if prior.physical_remainder {
                    prior.count = u32::MAX;
                } else {
                    let merged_end = prior_end.max(range.offset as u64 + range.count as u64);
                    if merged_end - prior.offset as u64 > u32::MAX as u64 {
                        return None;
                    }
                    prior.count = (merged_end - prior.offset as u64) as u32;
                }
                continue;
            }
        }
        if merged.len() == 256 {
            return None;
        }
        merged.push(range);
    }
    if merged.is_empty() { None } else { Some(merged) }
}

impl BorrowedRangeTracking {
    pub fn set_ready(&self, value: bool) {
        if value {
            self.ready.store(true, Ordering::Release);
        } else {
            self.unavailable.store(true, Ordering::Release);
        }
    }
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) && !self.unavailable.load(Ordering::Acquire)
    }
    pub fn pin(&self) -> bool {
        if !self.ready() {
            return false;
        }
        let mut value = self.pins.load(Ordering::Relaxed);
        while value < PIN_LIMIT {
            match self.pins.compare_exchange_weak(value, value + 1, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return true,
                Err(current) => value = current,
            }
        }
        false
    }
    pub fn release(&self) {
        self.pins.fetch_sub(1, Ordering::AcqRel);
    }
    pub fn mutation_begin(&self) -> bool {
        if !self.ready() {
            return false;
        }
        let mut value = self.mutations.load(Ordering::Relaxed);
        while value < u32::MAX {
            match self.mutations.compare_exchange_weak(value, value + 1, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return true,
                Err(current) => value = current,
            }
        }
        self.set_ready(false);
        false
    }
    pub fn mutation_end(&self) {
        self.mutations.fetch_sub(1, Ordering::AcqRel);
    }
    pub fn stable(&self, epoch: u64) -> bool {
        epoch != 0 && epoch == self.epoch.load(Ordering::Acquire) && self.ready() && self.mutations.load(Ordering::Acquire) == 0
    }
    pub fn versions_required(&self, live_metadata: bool) -> bool {
        !live_metadata || self.pins.load(Ordering::Acquire) != 0
    }
    pub fn invalidate(&self) {
        let value = self.epoch.load(Ordering::Relaxed);
        if value == u64::MAX {
            self.set_ready(false);
            return;
        }
        self.epoch.store(value + 1, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Interval {
    begin: u16,
    end: u16,
    version: u64,
}
#[derive(Clone, Debug, Default)]
struct VersionPage {
    intervals: Vec<Interval>,
    latest: u64,
}
impl VersionPage {
    fn bounds(&self, begin: u16, end: u16) -> (usize, usize) {
        let first = self.intervals.partition_point(|i| i.end <= begin);
        let last = first + self.intervals[first..].partition_point(|i| i.begin < end);
        (first, last)
    }
    fn stamp(&mut self, begin: u16, end: u16, version: u64) {
        let first = self.intervals.partition_point(|i| i.end <= begin);
        self.latest = version;
        if let Some(interval) = self.intervals.get_mut(first) {
            if interval.begin == begin && interval.end == end {
                interval.version = version;
                return;
            }
        }
        let (first, last) = self.bounds(begin, end);
        let mut replacement = Vec::with_capacity(3);
        if first != last && self.intervals[first].begin < begin {
            replacement.push(Interval { begin: self.intervals[first].begin, end: begin, version: self.intervals[first].version });
        }
        replacement.push(Interval { begin, end, version });
        if first != last && self.intervals[last - 1].end > end {
            replacement.push(Interval { begin: end, end: self.intervals[last - 1].end, version: self.intervals[last - 1].version });
        }
        self.intervals.splice(first..last, replacement);
    }
    fn version(&self, begin: u16, end: u16) -> u64 {
        if begin == 0 && end as u64 == PAGE_BYTES {
            return self.latest;
        }
        let first = self.intervals.partition_point(|i| i.end <= begin);
        self.intervals[first..].iter().take_while(|i| i.begin < end).map(|i| i.version).max().unwrap_or(0)
    }
    fn erase(&mut self, begin: u16, end: u16) {
        let (first, last) = self.bounds(begin, end);
        let mut tail = Vec::with_capacity(2);
        if first != last && self.intervals[first].begin < begin {
            tail.push(Interval { begin: self.intervals[first].begin, end: begin, version: self.intervals[first].version });
        }
        if first != last && self.intervals[last - 1].end > end {
            tail.push(Interval { begin: end, end: self.intervals[last - 1].end, version: self.intervals[last - 1].version });
        }
        self.intervals.splice(first..last, tail);
        self.latest = self.intervals.iter().map(|i| i.version).max().unwrap_or(0);
    }
}
fn value_hash(value: &Value) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &word in value {
        hash ^= word;
        hash = hash.wrapping_mul(0x100000001b3);
        hash ^= hash >> 32;
    }
    hash
}
fn range_end(begin: u64, count: u64, stride: u64) -> Option<u64> {
    if begin == 0 || count == 0 || stride == 0 || count > (u64::MAX - begin) / stride {
        return None;
    }
    Some(begin + count * stride)
}

#[derive(Clone, Debug, Default)]
struct Heap {
    cpu: u64,
    gpu: u64,
    cookie: u64,
    count: u32,
    stride: u32,
    slots: Vec<u32>,
}
impl Heap {
    fn bytes(&self) -> u64 {
        (self.count as u64).wrapping_mul(self.stride as u64)
    }
}

/// The value retirement callback (descriptors::Value by reference).
pub type Retire = unsafe extern "C-unwind" fn(*const Value);

/// descriptors::Store.
pub struct Store {
    heaps: Vec<Heap>,
    slots: KeyMap<u32>,
    dense: bool,
    dense_slots: usize,
    dense_allocated: usize,
    heap_hints: Cell<[usize; 4]>,
    // Boxed pages keep their addresses, so the last page is cached as the C++ lastPage was.
    changes: BTreeMap<u64, Box<VersionPage>>,
    last_page_key: u64,
    last_page: *mut VersionPage,
    ids: KeyMap<Vec<u32>>,
    id_count: usize,
    values: Vec<Value>,
    references: Vec<u32>,
    free_ids: Vec<u32>,
    interned: Vec<u8>,
    version: u64,
    version_floor: u64,
    track_versions: bool,
    intervals: usize,
    overflow: u32,
    large_ranges: RefCell<BTreeMap<(u64, u32), RangeIdentity>>,
    retire_value: Option<Retire>,
}
impl Store {
    pub fn new(dense: bool) -> Self {
        Store {
            heaps: Vec::new(),
            slots: KeyMap::default(),
            dense,
            dense_slots: 0,
            dense_allocated: 0,
            heap_hints: Cell::new([NO_HEAP; 4]),
            changes: BTreeMap::new(),
            last_page_key: u64::MAX,
            last_page: core::ptr::null_mut(),
            ids: KeyMap::default(),
            id_count: 0,
            values: vec![[0; 20]],
            references: vec![0],
            free_ids: Vec::new(),
            interned: vec![0],
            version: 0,
            version_floor: 0,
            track_versions: true,
            intervals: 0,
            overflow: 0,
            large_ranges: RefCell::new(BTreeMap::new()),
            retire_value: None,
        }
    }
    fn clear_heap_hints(&self) {
        self.heap_hints.set([NO_HEAP; 4]);
    }
    fn dense_heap(&self, cpu: u64) -> usize {
        let contains = |i: usize| {
            self.heaps.get(i).is_some_and(|heap| {
                !heap.slots.is_empty() && cpu >= heap.cpu && cpu - heap.cpu < heap.bytes() && (cpu - heap.cpu) % heap.stride as u64 == 0
            })
        };
        let mut hints = self.heap_hints.get();
        if let Some(&hint) = hints.iter().find(|&&hint| contains(hint)) {
            return hint;
        }
        for i in 0..self.heaps.len() {
            if contains(i) {
                hints.copy_within(0..3, 1);
                hints[0] = i;
                self.heap_hints.set(hints);
                return i;
            }
        }
        NO_HEAP
    }
    fn retire(&self, id: u32) {
        if let Some(retire) = self.retire_value {
            unsafe { retire(&self.values[id as usize]) };
        }
    }
    fn release(&mut self, id: u32) {
        let index = id as usize;
        if id == 0 || self.references[index] == 0 {
            return;
        }
        self.references[index] -= 1;
        if self.references[index] != 0 {
            return;
        }
        self.retire(id);
        if self.interned[index] != 0 {
            let hash = value_hash(&self.values[index]);
            if let Some(candidates) = self.ids.get_mut(&hash) {
                if let Some(position) = candidates.iter().position(|&candidate| candidate == id) {
                    candidates.remove(position);
                    self.id_count -= 1;
                    if candidates.is_empty() {
                        self.ids.remove(&hash);
                    }
                }
            }
        }
        self.values[index] = [0; 20];
        self.interned[index] = 0;
        self.free_ids.push(id);
    }
    fn set(&mut self, cpu: u64, id: u32) {
        if self.dense {
            let index = self.dense_heap(cpu);
            if index != NO_HEAP {
                let heap = &self.heaps[index];
                let slot = ((cpu - heap.cpu) / heap.stride as u64) as usize;
                let old = heap.slots[slot];
                if old == id {
                    return;
                }
                if id != 0 {
                    if old == 0 && self.dense_slots + self.slots.len() >= SLOT_LIMIT {
                        self.overflow |= 4;
                        return;
                    }
                    self.references[id as usize] += 1;
                    if old == 0 {
                        self.dense_slots += 1;
                    }
                } else if old != 0 {
                    self.dense_slots -= 1;
                }
                self.heaps[index].slots[slot] = id;
                self.release(old);
                return;
            }
        }
        let found = self.slots.get(&cpu).copied();
        let old = found.unwrap_or(0);
        if old == id {
            return;
        }
        if id != 0 {
            if found.is_none() && self.dense_slots + self.slots.len() >= SLOT_LIMIT {
                self.overflow |= 4;
                return;
            }
            self.slots.insert(cpu, id);
            self.references[id as usize] += 1;
        } else if found.is_some() {
            self.slots.remove(&cpu);
        }
        self.release(old);
    }
    fn erase_slots(&mut self, begin: u64, end: u64) {
        for h in 0..self.heaps.len() {
            let heap = &self.heaps[h];
            if heap.slots.is_empty() || begin >= heap.cpu.wrapping_add(heap.bytes()) || heap.cpu >= end {
                continue;
            }
            let (base, count, stride) = (heap.cpu, heap.count as u64, heap.stride as u64);
            let index = |address: u64| {
                if address <= base {
                    return 0;
                }
                let delta = address - base;
                count.min(delta / stride + (delta % stride != 0) as u64)
            };
            let (first, last) = (index(begin), index(end));
            for i in first..last {
                let slot = self.heaps[h].slots[i as usize];
                if slot != 0 {
                    self.release(slot);
                    self.heaps[h].slots[i as usize] = 0;
                    self.dense_slots -= 1;
                }
            }
        }
        let mut inside: Vec<u64> = self.slots.keys().copied().filter(|&cpu| cpu >= begin && cpu < end).collect();
        inside.sort_unstable();
        for cpu in inside {
            if let Some(id) = self.slots.remove(&cpu) {
                self.release(id);
            }
        }
    }
    fn forget_large_ranges(&self, begin: u64, end: u64) {
        self.large_ranges.borrow_mut().retain(|_, r| {
            !(begin < r.cpu.wrapping_add((r.count as u64).wrapping_mul(r.stride as u64)) && r.cpu < end)
        });
    }
    fn stamp(&mut self, mut begin: u64, end: u64) {
        if !self.track_versions {
            return;
        }
        self.forget_large_ranges(begin, end);
        self.version += 1;
        let version = self.version;
        while begin < end {
            let key = begin / PAGE_BYTES;
            if self.last_page.is_null() || key != self.last_page_key {
                self.last_page = &mut **self.changes.entry(key).or_default();
                self.last_page_key = key;
            }
            // SAFETY: the cached page is boxed in changes and dropped only by forget / set_track_versions, which reset it.
            let page = unsafe { &mut *self.last_page };
            let first = (begin % PAGE_BYTES) as u16;
            let count = (end - begin).min(PAGE_BYTES - first as u64);
            let previous = page.intervals.len();
            page.stamp(first, (first as u64 + count) as u16, version);
            self.intervals = self.intervals - previous + page.intervals.len();
            begin += count;
            if self.intervals > SLOT_LIMIT {
                self.overflow |= 8;
                return;
            }
        }
    }
    fn forget(&mut self, begin: u64, end: u64) {
        self.forget_large_ranges(begin, end);
        let keys: Vec<u64> = self.changes.range(begin / PAGE_BYTES..=(end - 1) / PAGE_BYTES).map(|(&key, _)| key).collect();
        for key in keys {
            let base = key * PAGE_BYTES;
            let first = if begin > base { (begin - base) as u16 } else { 0 };
            let last = (end - base).min(PAGE_BYTES) as u16;
            let page = self.changes.get_mut(&key).expect("a listed page");
            let before = page.intervals.len();
            page.erase(first, last);
            self.intervals = self.intervals - before + page.intervals.len();
            if page.intervals.is_empty() {
                if key == self.last_page_key {
                    self.last_page = core::ptr::null_mut();
                    self.last_page_key = u64::MAX;
                }
                self.changes.remove(&key);
            }
        }
    }

    pub fn set_value_retirement(&mut self, callback: Option<Retire>) {
        self.retire_value = callback;
    }
    pub fn stats(&self) -> [u64; 6] {
        [
            self.overflow as u64,
            self.heaps.len() as u64,
            (self.slots.len() + self.dense_slots) as u64,
            self.id_count as u64,
            self.intervals as u64,
            self.large_ranges.borrow().len() as u64,
        ]
    }
    pub fn set_track_versions(&mut self, enabled: bool) {
        if enabled == self.track_versions {
            return;
        }
        self.changes.clear();
        self.last_page = core::ptr::null_mut();
        self.last_page_key = u64::MAX;
        self.large_ranges.borrow_mut().clear();
        self.intervals = 0;
        self.version += 1;
        self.version_floor = self.version;
        self.track_versions = enabled;
    }
    /// Dense: a representation-only change; ids, reference counts, heap cookies and range versions stay intact.
    pub fn set_dense(&mut self, enabled: bool) {
        if enabled == self.dense {
            return;
        }
        let mut pending = self.slots.clone();
        let mut heaps: Vec<Vec<u32>> = vec![Vec::new(); self.heaps.len()];
        let (mut allocated, mut occupied) = (0usize, 0usize);
        if enabled {
            for (i, h) in self.heaps.iter().enumerate() {
                if h.count as usize > SLOT_LIMIT.wrapping_sub(allocated) {
                    continue;
                }
                heaps[i].resize(h.count as usize, 0);
                allocated += h.count as usize;
                pending.retain(|&address, &mut id| {
                    if address >= h.cpu && address - h.cpu < h.bytes() && (address - h.cpu) % h.stride as u64 == 0 {
                        heaps[i][((address - h.cpu) / h.stride as u64) as usize] = id;
                        occupied += 1;
                        false
                    } else {
                        true
                    }
                });
            }
        } else {
            pending.reserve(self.dense_slots);
            for h in &self.heaps {
                for (i, &id) in h.slots.iter().enumerate() {
                    if id != 0 {
                        pending.entry(h.cpu + i as u64 * h.stride as u64).or_insert(id);
                    }
                }
            }
        }
        for (heap, slots) in self.heaps.iter_mut().zip(heaps) {
            heap.slots = slots;
        }
        self.slots = pending;
        self.dense_allocated = allocated;
        self.dense_slots = occupied;
        self.dense = enabled;
        self.clear_heap_hints();
    }
    /// Overflow: the store dropped metadata past a budget (sticky).
    pub fn overflow(&self) -> bool {
        self.overflow != 0
    }
    pub fn heap_slots(&self) -> u64 {
        self.heaps.iter().map(|heap| heap.count as u64).sum()
    }
    pub fn get(&self, id: u32) -> Option<&Value> {
        self.values.get(id as usize).filter(|value| id != 0 && value[0] != 0)
    }
    fn allocate(&mut self, value: &Value) -> Option<u32> {
        if let Some(id) = self.free_ids.pop() {
            self.values[id as usize] = *value;
            return Some(id);
        }
        if self.values.len() >= VALUE_LIMIT {
            self.overflow |= 1;
            return None;
        }
        let id = self.values.len() as u32;
        self.values.push(*value);
        self.references.push(0);
        self.interned.push(0);
        Some(id)
    }
    pub fn intern(&mut self, value: &Value) -> u32 {
        if value[0] == 0 || self.overflow != 0 {
            return 0;
        }
        let hash = value_hash(value);
        if let Some(&id) = self.ids.get(&hash).and_then(|candidates| candidates.iter().find(|&&id| self.values[id as usize] == *value)) {
            return id;
        }
        let Some(id) = self.allocate(value) else { return 0 };
        self.interned[id as usize] = 1;
        self.ids.entry(hash).or_default().push(id);
        self.id_count += 1;
        id
    }
    pub fn heap_created(&mut self, cpu: u64, gpu: u64, count: u32, stride: u32, cookie: u64) -> bool {
        if self.overflow != 0 {
            return false;
        }
        let Some(end) = range_end(cpu, count as u64, stride as u64) else { return false };
        if cookie == 0 {
            return false;
        }
        let gpu_end = if gpu != 0 {
            match range_end(gpu, count as u64, stride as u64) {
                Some(end) => end,
                None => return false,
            }
        } else {
            0
        };
        let mut i = 0;
        while i < self.heaps.len() {
            let old = &self.heaps[i];
            let old_end = old.gpu.wrapping_add(old.bytes());
            let old_cpu_end = old.cpu.wrapping_add(old.bytes());
            if (gpu != 0 && old.gpu != 0 && gpu < old_end && old.gpu < gpu_end) || (cpu < old_cpu_end && old.cpu < end) {
                if old.cpu == cpu && old.gpu == gpu && old.cookie == cookie && old.count == count && old.stride == stride {
                    return true;
                }
                let old_cpu = old.cpu;
                self.erase_slots(old_cpu, old_cpu_end);
                self.stamp(old_cpu, old_cpu_end);
                self.large_ranges.borrow_mut().clear();
                self.dense_allocated -= self.heaps[i].slots.len();
                self.clear_heap_hints();
                self.heaps.remove(i);
            } else {
                i += 1;
            }
        }
        if self.heaps.len() >= 1024 {
            self.overflow |= 2;
            return false;
        }
        // A new heap at an old CPU address must not inherit any old descriptors.
        self.erase_slots(cpu, end);
        self.stamp(cpu, end);
        let mut heap = Heap { cpu, gpu, cookie, count, stride, slots: Vec::new() };
        if self.dense && count as usize <= SLOT_LIMIT.wrapping_sub(self.dense_allocated) {
            heap.slots.resize(count as usize, 0);
        }
        let allocated = heap.slots.len();
        self.heaps.push(heap);
        self.dense_allocated += allocated;
        self.clear_heap_hints();
        true
    }
    pub fn heap_removed(&mut self, cookie: u64) {
        let Some(i) = self.heaps.iter().position(|heap| heap.cookie == cookie) else { return };
        let (cpu, count, stride) = (self.heaps[i].cpu, self.heaps[i].count, self.heaps[i].stride as u64);
        // Known native heap slots are exactly base + index * stride.
        for index in 0..count {
            self.set(cpu + index as u64 * stride, 0);
        }
        self.forget(cpu, cpu + count as u64 * stride);
        self.dense_allocated -= self.heaps[i].slots.len();
        self.heaps.remove(i);
        self.clear_heap_hints();
    }
    /// InvalidateTracked: Ignored 0, Invalidated 1, Rejected 2.
    pub fn invalidate_tracked(&mut self, cpu: u64, count: u32, stride: u32) -> u32 {
        if self.overflow != 0 {
            return 2;
        }
        if count == 0 {
            return 0;
        }
        let Some(end) = range_end(cpu, count as u64, stride as u64) else {
            self.overflow |= 16;
            return 2;
        };
        let (mut touched, mut malformed) = (false, false);
        for i in 0..self.heaps.len() {
            let heap = &self.heaps[i];
            let heap_end = heap.cpu.wrapping_add(heap.bytes());
            if cpu >= heap_end || heap.cpu >= end {
                continue;
            }
            touched = true;
            let heap_cpu = heap.cpu;
            if cpu < heap.cpu || end > heap_end || stride != heap.stride || (cpu - heap.cpu) % heap.stride as u64 != 0 || count as usize > SLOT_LIMIT {
                self.erase_slots(heap_cpu, heap_end);
                self.stamp(heap_cpu, heap_end);
                malformed = true;
            } else {
                self.erase_slots(cpu, end);
                self.stamp(cpu, end);
            }
        }
        if malformed {
            self.overflow |= 16;
            return 2;
        }
        if self.overflow != 0 {
            return 2;
        }
        if touched { 1 } else { 0 }
    }
    pub fn write(&mut self, cpu: u64, stride: u32, id: u32) {
        if self.overflow != 0 {
            return;
        }
        let Some(end) = range_end(cpu, 1, stride as u64) else {
            self.overflow |= 16;
            return;
        };
        let id = if self.get(id).is_some() { id } else { 0 };
        self.set(cpu, id);
        self.stamp(cpu, end);
    }
    /// WriteValue: streaming CBVs overwrite in place when a slot is their sole owner; origin-bearing values intern.
    pub fn write_value(&mut self, cpu: u64, stride: u32, value: &Value) {
        if value[0] != 1 || value[3] != 0 {
            let id = self.intern(value);
            self.write(cpu, stride, id);
            return;
        }
        if self.overflow != 0 {
            return;
        }
        let Some(end) = range_end(cpu, 1, stride as u64) else {
            self.overflow |= 16;
            return;
        };
        let old = self.at(cpu);
        if old != 0 && self.values[old as usize] == *value {
            self.stamp(cpu, end);
            return;
        }
        if old != 0 && self.references[old as usize] == 1 && self.interned[old as usize] == 0 {
            self.retire(old);
            self.values[old as usize] = *value;
            self.stamp(cpu, end);
            return;
        }
        let Some(id) = self.allocate(value) else { return };
        self.set(cpu, id);
        self.stamp(cpu, end);
    }
    pub fn at(&self, cpu: u64) -> u32 {
        if self.dense {
            let index = self.dense_heap(cpu);
            if index != NO_HEAP {
                let heap = &self.heaps[index];
                return heap.slots[((cpu - heap.cpu) / heap.stride as u64) as usize];
            }
        }
        self.slots.get(&cpu).copied().unwrap_or(0)
    }
    pub fn copy(&mut self, destination: u64, source: u64, count: u32, stride: u32) {
        if self.overflow != 0 {
            return;
        }
        let (end, source_end) = match (range_end(destination, count as u64, stride as u64), range_end(source, count as u64, stride as u64)) {
            (Some(end), Some(source_end)) if count as usize <= SLOT_LIMIT => (end, source_end),
            _ => {
                self.overflow |= 16;
                return;
            }
        };
        // D3D12 forbids overlap: treat it as unknown rather than guessing an order.
        if destination < source_end && source < end {
            self.erase_slots(destination, end);
            self.stamp(destination, end);
            return;
        }
        for i in 0..count as u64 {
            let value = self.at(source + i * stride as u64);
            self.set(destination + i * stride as u64, value);
        }
        self.stamp(destination, end);
    }
    pub fn range(&self, gpu: u64, count: u32) -> RangeIdentity {
        if self.overflow != 0 || count == 0 {
            return RangeIdentity::default();
        }
        if count > 128 {
            if let Some(cached) = self.large_ranges.borrow().get(&(gpu, count)) {
                return *cached;
            }
        }
        for heap in &self.heaps {
            if heap.gpu == 0 {
                continue;
            }
            let stride = heap.stride as u64;
            let end = heap.gpu.wrapping_add(heap.bytes());
            if gpu < heap.gpu || gpu >= end || (gpu - heap.gpu) % stride != 0 {
                continue;
            }
            let actual = if count == u32::MAX { ((end - gpu) / stride) as u32 } else { count };
            if actual as u64 * stride > end - gpu {
                continue;
            }
            let cpu = heap.cpu.wrapping_add(gpu - heap.gpu);
            let last = cpu.wrapping_add(actual as u64 * stride);
            let mut version = if self.track_versions { self.version_floor } else { 0 };
            for (&key, page) in self.changes.range(cpu / PAGE_BYTES..) {
                if key > (last - 1) / PAGE_BYTES {
                    break;
                }
                let base = key * PAGE_BYTES;
                let first = if cpu > base { (cpu - base) as u16 } else { 0 };
                let page_end = (last - base).min(PAGE_BYTES) as u16;
                version = version.max(page.version(first, page_end));
            }
            let result = RangeIdentity { heap: heap.cookie, cpu, gpu, version, count: actual, stride: heap.stride, valid: true };
            let mut large = self.large_ranges.borrow_mut();
            if count > 128 && large.len() < 128 {
                large.insert((gpu, count), result);
            }
            return result;
        }
        RangeIdentity::default()
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// InventoryBorrowedDescriptor.
///
/// # Safety
/// `value` is null or valid; `out` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_inventory(value: *const Value, canonical_srv_valid: bool, out: *mut BorrowedDescriptorInventory) -> u64 {
    unsafe { inventory_borrowed_descriptor(value.as_ref(), canonical_srv_valid, &mut *out) }
}
/// DescriptorMutationObservation.
#[no_mangle]
pub extern "C-unwind" fn cvr_ds_mutation_mode(internal: bool, enabled: bool, borrowed: bool) -> u32 {
    descriptor_mutation_observation(internal, enabled, borrowed)
}
/// BorrowedTrackingDetails.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ds_tracking_details(
    pinned: bool,
    ready: bool,
    saved_epoch: u64,
    current_epoch: u64,
    pins: u32,
    mutations: u32,
    versions: bool,
    overflow: u32,
    failure: u32,
    empty: bool,
    out: *mut [u64; 4],
) {
    unsafe { *out = borrowed_tracking_details(pinned, ready, saved_epoch, current_epoch, pins, mutations, versions, overflow, failure, empty) };
}
/// SameBorrowedRange.
///
/// # Safety
/// `a` and `b` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_same_range(a: *const RangeIdentity, b: *const RangeIdentity) -> bool {
    unsafe { same_borrowed_range(&*a, &*b) }
}
/// PlanBorrowedShaderRanges: up to 256 ranges into `output` with their `count` (0 on refusal).
///
/// # Safety
/// The declarations and uses hold their counts; `output` has room for 256; `count` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ds_plan(
    parameters: u32,
    supported: bool,
    direct_heap_indexing: bool,
    declarations: *const BorrowedRootRange,
    declaration_count: usize,
    uses: *const BorrowedShaderUse,
    use_count: usize,
    output: *mut BorrowedUsedRange,
    count: *mut usize,
) -> bool {
    let declarations: &[BorrowedRootRange] =
        if declarations.is_null() || declaration_count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(declarations, declaration_count) } };
    let uses: &[BorrowedShaderUse] = if uses.is_null() || use_count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(uses, use_count) } };
    let plan = plan_borrowed_shader_ranges(parameters, supported, direct_heap_indexing, declarations, uses);
    let ranges = plan.as_deref().unwrap_or(&[]);
    unsafe {
        core::ptr::copy_nonoverlapping(ranges.as_ptr(), output, ranges.len());
        *count = ranges.len();
    }
    plan.is_some()
}
/// BorrowedRangeTracking by `which`: Ready(value) (0), Ready() (1), Pin (2), Release (3), MutationBegin (4),
/// MutationEnd (5), Stable (6: `value` the epoch), VersionsRequired (7: `value` live metadata), Invalidate (8).
///
/// # Safety
/// `tracking` is a live C++ BorrowedRangeTracking.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_tracking(tracking: *const BorrowedRangeTracking, which: u32, value: u64) -> bool {
    let tracking = unsafe { &*tracking };
    match which {
        0 => {
            tracking.set_ready(value != 0);
            true
        }
        1 => tracking.ready(),
        2 => tracking.pin(),
        3 => {
            tracking.release();
            true
        }
        4 => tracking.mutation_begin(),
        5 => {
            tracking.mutation_end();
            true
        }
        6 => tracking.stable(value),
        7 => tracking.versions_required(value != 0),
        _ => {
            tracking.invalidate();
            true
        }
    }
}
/// A new store, owned by the C++ face.
#[no_mangle]
pub extern "C-unwind" fn cvr_ds_new(dense: bool) -> *mut Store {
    Box::into_raw(Box::new(Store::new(dense)))
}
/// Frees a store made by `cvr_ds_new`.
///
/// # Safety
/// `store` came from `cvr_ds_new` and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_free(store: *mut Store) {
    if !store.is_null() {
        drop(unsafe { Box::from_raw(store) });
    }
}
/// SetValueRetirement.
///
/// # Safety
/// `store` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_set_retirement(store: *mut Store, callback: Option<Retire>) {
    unsafe { (*store).set_value_retirement(callback) };
}
/// Store queries by `which`: Overflow (0), Dense (1), TrackVersions (2), Values (3), HeapSlots (4), At (5: `a` cpu).
///
/// # Safety
/// `store` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_query(store: *const Store, which: u32, a: u64) -> u64 {
    let store = unsafe { &*store };
    match which {
        0 => store.overflow as u64,
        1 => store.dense as u64,
        2 => store.track_versions as u64,
        3 => store.values.len() as u64,
        4 => store.heap_slots(),
        _ => store.at(a) as u64,
    }
}
/// Stats.
///
/// # Safety
/// `store` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_stats(store: *const Store, out: *mut [u64; 6]) {
    unsafe { *out = (*store).stats() };
}
/// Get: the value's address (valid until the store's next change), or null.
///
/// # Safety
/// `store` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_get(store: *const Store, id: u32) -> *const Value {
    match unsafe { (*store).get(id) } {
        Some(value) => value,
        None => core::ptr::null(),
    }
}
/// Store changes by `which`: TrackVersions(enabled) (0: `a`), Dense(enabled) (1: `a`), HeapRemoved (2: `a` cookie),
/// Write (3: `a` cpu, `stride`, `count` the id), Copy (4: `a` destination, `b` source, `count`, `stride`),
/// InvalidateTracked (5: `a` cpu, `count`, `stride`; the TrackedDescriptorWrite), HeapCreated (6: `a` cpu, `b` gpu,
/// `count`, `stride`, `c` cookie; 1 or 0).
///
/// # Safety
/// `store` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_change(store: *mut Store, which: u32, a: u64, b: u64, c: u64, count: u32, stride: u32) -> u32 {
    let store = unsafe { &mut *store };
    match which {
        0 => store.set_track_versions(a != 0),
        1 => store.set_dense(a != 0),
        2 => store.heap_removed(a),
        3 => store.write(a, stride, count),
        4 => store.copy(a, b, count, stride),
        5 => return store.invalidate_tracked(a, count, stride),
        _ => return store.heap_created(a, b, count, stride, c) as u32,
    }
    0
}
/// Intern (`which` 0: the id) or WriteValue (1: `cpu`, `stride`).
///
/// # Safety
/// `store` is live; `value` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_value(store: *mut Store, which: u32, value: *const Value, cpu: u64, stride: u32) -> u32 {
    let (store, value) = unsafe { (&mut *store, &*value) };
    if which == 0 {
        return store.intern(value);
    }
    store.write_value(cpu, stride, value);
    0
}
/// Range.
///
/// # Safety
/// `store` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ds_range(store: *const Store, gpu: u64, count: u32, out: *mut RangeIdentity) {
    unsafe { *out = (*store).range(gpu, count) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_page_splits_and_erases() {
        let mut page = VersionPage::default();
        page.stamp(0, 64, 1);
        page.stamp(16, 32, 2);
        assert_eq!(page.intervals.len(), 3);
        assert_eq!(page.version(0, 16), 1);
        assert_eq!(page.version(16, 32), 2);
        page.erase(0, 64);
        assert!(page.intervals.is_empty());
        assert_eq!(page.latest, 0);
    }
}
