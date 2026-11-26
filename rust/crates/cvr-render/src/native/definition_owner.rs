//! The native owner vector of a render graph's definitions, and the gates of their single retirement
//! (include/Render/NativeDefinitionOwner.hpp).
//!
//! `MovePlan` is a move proposal only: the graph's owner vector at graph+0x70 ({data, capacity, count}, then its
//! allocator in the 8 bytes after the allocation) read twice and compared, entries unique, aligned and outside the
//! vector and the owner, the allocator a pointer outside the allocation. The caller proves the native graph/template
//! type and serializes mutation before writing the empty header to the actual native owner; the saved header holds
//! native-allocated memory, and the owner itself is never the allocation to free. Synthetic graph nodes keep a null
//! slot in the vector.
//!
//! `RetireState` is the exactly-once retirement: every consumer added before the source closes is completed, the
//! original owner was detached, source uses closed (future users of this definition generation, native cache hits
//! included, excluded), then one begin and one finish. The first rejection sticks; no method waits or frees.
use std::collections::BTreeSet;

pub const MAX_DEFINITIONS: u32 = 8192;
pub const MAX_CONSUMERS: u32 = 8192;

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeVector {
    pub data: u64,
    pub capacity: u32,
    pub count: u32,
}
impl NativeVector {
    fn from_bytes(b: &[u8; 16]) -> Self {
        NativeVector {
            data: u64::from_le_bytes(b[0..8].try_into().expect("eight")),
            capacity: u32::from_le_bytes(b[8..12].try_into().expect("four")),
            count: u32::from_le_bytes(b[12..16].try_into().expect("four")),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Header,
    Bounds,
    Entries,
    Allocator,
    Changed,
    Allocation,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Snapshot {
    header: NativeVector,
    allocator: u64,
    trailer: u64,
    entries: Vec<u64>,
}

fn pointer(value: u64) -> bool {
    value != 0 && value & 7 == 0
}
fn read_u64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}

#[derive(Clone, Debug, Default)]
pub struct MovePlan {
    saved: NativeVector,
    empty: NativeVector,
    definitions: Vec<u64>,
    address: u64,
    trailer: u64,
    ready: bool,
    failure: Failure,
}
impl MovePlan {
    pub fn capture(&mut self, graph: u64, read: Option<Read<'_>>) -> bool {
        *self = MovePlan {
            failure: Failure::Arguments,
            ..MovePlan::default()
        };
        let Some(read) = read else { return false };
        if !pointer(graph) || graph > u64::MAX - 0x88 {
            return false;
        }
        self.address = graph + 0x70;
        let Some(first) = self.read_once(read, self.address) else {
            return false;
        };
        let Some(second) = self.read_once(read, self.address) else {
            return false;
        };
        self.failure = Failure::Changed;
        if first != second {
            return false;
        }
        self.saved = first.header;
        self.empty = NativeVector {
            data: first.allocator,
            capacity: 0,
            count: 0,
        };
        self.trailer = first.trailer;
        self.definitions = first.entries;
        self.ready = true;
        self.failure = Failure::None;
        true
    }
    /// A ready plan captured before (the C++ face keeps a capture's fields and hands them back to revalidate it).
    pub fn restored(
        address: u64,
        trailer: u64,
        saved: NativeVector,
        empty: NativeVector,
        definitions: Vec<u64>,
    ) -> MovePlan {
        MovePlan {
            saved,
            empty,
            definitions,
            address,
            trailer,
            ready: true,
            failure: Failure::None,
        }
    }
    pub fn matches_before(&self, read: Read<'_>) -> bool {
        if !self.ready {
            return false;
        }
        let mut current = MovePlan::default();
        current.capture(self.address - 0x70, Some(read))
            && current.address == self.address
            && current.saved == self.saved
            && current.empty == self.empty
            && current.trailer == self.trailer
            && current.definitions == self.definitions
    }
    pub fn verify_detached(&self, read: Option<Read<'_>>) -> bool {
        let Some(read) = read else { return false };
        if !self.ready {
            return false;
        }
        let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
        read(self.address, &mut a)
            && read(self.address, &mut b)
            && NativeVector::from_bytes(&a) == self.empty
            && NativeVector::from_bytes(&b) == self.empty
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn owns_allocation(&self) -> bool {
        self.ready && self.saved.capacity != 0
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
    pub fn owner_address(&self) -> u64 {
        self.address
    }
    pub fn trailer_address(&self) -> u64 {
        self.trailer
    }
    pub fn saved_header(&self) -> NativeVector {
        self.saved
    }
    pub fn empty_header(&self) -> NativeVector {
        self.empty
    }
    pub fn definitions(&self) -> &[u64] {
        &self.definitions
    }

    fn read_once(
        &mut self,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        address: u64,
    ) -> Option<Snapshot> {
        let mut out = Snapshot::default();
        self.failure = Failure::Header;
        let mut raw = [0u8; 16];
        if !read(address, &mut raw) {
            return None;
        }
        out.header = NativeVector::from_bytes(&raw);
        let h = out.header;
        self.failure = Failure::Bounds;
        if !pointer(h.data) || h.capacity > MAX_DEFINITIONS || h.count > h.capacity {
            return None;
        }
        if h.capacity == 0 {
            if h.count != 0 || (h.data >= address - 0x70 && h.data < address + 16) {
                return None;
            }
            out.allocator = h.data;
            return Some(out);
        }
        let bytes = h.capacity as u64 * 8;
        let aligned = (bytes + 7) & !7u64;
        if aligned > u32::MAX as u64
            || h.data > u64::MAX - aligned
            || h.data + aligned > u64::MAX - 8
        {
            return None;
        }
        if h.data < address + 16 && address - 0x70 < h.data + aligned + 8 {
            return None;
        }
        out.trailer = h.data + aligned;
        self.failure = Failure::Entries;
        if h.count != 0 {
            let mut raw = vec![0u8; h.count as usize * 8];
            if !read(h.data, &mut raw) {
                return None;
            }
            out.entries = raw
                .chunks_exact(8)
                .map(|c| u64::from_le_bytes(c.try_into().expect("eight")))
                .collect();
        }
        let mut unique = BTreeSet::new();
        for &entry in &out.entries {
            if entry != 0
                && (!pointer(entry)
                    || !unique.insert(entry)
                    || (entry >= h.data && entry < out.trailer + 8)
                    || (entry >= address - 0x70 && entry < address + 16))
            {
                return None;
            }
        }
        self.failure = Failure::Allocator;
        let Some(allocator) = read_u64(read, out.trailer) else {
            return None;
        };
        out.allocator = allocator;
        if !pointer(out.allocator) || (out.allocator >= h.data && out.allocator < out.trailer + 8) {
            return None;
        }
        Some(out)
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RetireFailure {
    #[default]
    None,
    Epoch,
    UnaccountedPriorUse,
    Consumer,
    Capacity,
    UnclaimedMutation,
    Detach,
    SourceOpen,
    Incomplete,
    Repeated,
}

#[derive(Clone, Debug)]
pub struct RetireState {
    epoch: u64,
    consumers: BTreeSet<u64>,
    completed: BTreeSet<u64>,
    detached: bool,
    source_closed: bool,
    retiring: bool,
    retired: bool,
    failure: RetireFailure,
}
impl RetireState {
    pub fn new(epoch: u64, all_prior_consumers_accounted: bool) -> Self {
        let mut s = RetireState {
            epoch,
            consumers: BTreeSet::new(),
            completed: BTreeSet::new(),
            detached: false,
            source_closed: false,
            retiring: false,
            retired: false,
            failure: RetireFailure::None,
        };
        if epoch == 0 {
            s.reject(RetireFailure::Epoch);
        } else if !all_prior_consumers_accounted {
            s.reject(RetireFailure::UnaccountedPriorUse);
        }
        s
    }
    fn reject(&mut self, failure: RetireFailure) -> bool {
        if self.failure == RetireFailure::None {
            self.failure = failure;
        }
        false
    }
    fn check_epoch(&mut self, epoch: u64) -> bool {
        if self.failure != RetireFailure::None {
            return false;
        }
        epoch == self.epoch || self.reject(RetireFailure::Epoch)
    }
    pub fn add_consumer(&mut self, epoch: u64, consumer: u64) -> bool {
        if !self.check_epoch(epoch) {
            return false;
        }
        if consumer == 0
            || self.source_closed
            || self.retiring
            || self.retired
            || self.consumers.contains(&consumer)
        {
            return self.reject(RetireFailure::Consumer);
        }
        if self.consumers.len() >= MAX_CONSUMERS as usize {
            return self.reject(RetireFailure::Capacity);
        }
        self.consumers.insert(consumer);
        true
    }
    pub fn complete_consumer(&mut self, epoch: u64, consumer: u64) -> bool {
        if !self.check_epoch(epoch) {
            return false;
        }
        if consumer == 0
            || self.retiring
            || self.retired
            || !self.consumers.contains(&consumer)
            || self.completed.contains(&consumer)
        {
            return self.reject(RetireFailure::Consumer);
        }
        self.completed.insert(consumer);
        true
    }
    pub fn verify_detach(&mut self, epoch: u64, original_owner_detached: bool) -> bool {
        if !self.check_epoch(epoch) {
            return false;
        }
        if self.detached || self.retiring || self.retired || !original_owner_detached {
            return self.reject(RetireFailure::Detach);
        }
        self.detached = true;
        true
    }
    pub fn close_source_uses(&mut self, epoch: u64) -> bool {
        if !self.check_epoch(epoch) {
            return false;
        }
        if !self.detached || self.source_closed || self.retiring || self.retired {
            return self.reject(RetireFailure::SourceOpen);
        }
        self.source_closed = true;
        true
    }
    pub fn unclaimed_native_mutation(&mut self) {
        self.reject(RetireFailure::UnclaimedMutation);
    }
    pub fn ready_to_retire(&self) -> bool {
        self.failure == RetireFailure::None
            && self.detached
            && self.source_closed
            && !self.retiring
            && !self.retired
            && self.consumers.len() == self.completed.len()
    }
    pub fn begin_retire(&mut self, epoch: u64) -> bool {
        if !self.check_epoch(epoch) || !self.ready_to_retire() {
            return false;
        }
        self.retiring = true;
        true
    }
    pub fn finish_retire(&mut self, epoch: u64) -> bool {
        if !self.check_epoch(epoch) {
            return false;
        }
        if !self.retiring || self.retired {
            return self.reject(RetireFailure::Repeated);
        }
        self.retired = true;
        self.retiring = false;
        true
    }
    pub fn retiring(&self) -> bool {
        self.retiring
    }
    pub fn retired(&self) -> bool {
        self.retired
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn consumers(&self) -> usize {
        self.consumers.len()
    }
    pub fn pending_consumers(&self) -> usize {
        self.consumers.len() - self.completed.len()
    }
    pub fn failure_reason(&self) -> RetireFailure {
        self.failure
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/definition_owner_test.cpp's cases.
    use super::*;
    use std::collections::BTreeMap;

    const GRAPH: u64 = 0x10000;
    const DATA: u64 = 0x20000;
    const ALLOCATOR: u64 = 0x900000;
    const EPOCH: u64 = 37;
    #[derive(Default)]
    struct Mem {
        bytes: BTreeMap<u64, u8>,
        reads: u32,
        mutate_after: u32,
        mutate_address: u64,
        mutate_byte: u8,
    }
    impl Mem {
        fn put(&mut self, address: u64, b: &[u8]) {
            for (i, v) in b.iter().enumerate() {
                self.bytes.insert(address + i as u64, *v);
            }
        }
        fn put64(&mut self, address: u64, v: u64) {
            self.put(address, &v.to_le_bytes());
        }
        fn put_vec(&mut self, address: u64, v: NativeVector) {
            let mut b = [0u8; 16];
            b[0..8].copy_from_slice(&v.data.to_le_bytes());
            b[8..12].copy_from_slice(&v.capacity.to_le_bytes());
            b[12..16].copy_from_slice(&v.count.to_le_bytes());
            self.put(address, &b);
        }
        fn erase(&mut self, address: u64) {
            self.bytes.remove(&address);
        }
        fn read(&mut self, address: u64, out: &mut [u8]) -> bool {
            if address > u64::MAX - out.len() as u64 {
                return false;
            }
            for (i, o) in out.iter_mut().enumerate() {
                match self.bytes.get(&(address + i as u64)) {
                    Some(v) => *o = *v,
                    None => return false,
                }
            }
            self.reads += 1;
            if self.reads == self.mutate_after {
                self.bytes.insert(self.mutate_address, self.mutate_byte);
            }
            true
        }
    }
    fn fresh(capacity: u32, count: u32) -> Mem {
        let mut m = Mem::default();
        m.put_vec(
            GRAPH + 0x70,
            NativeVector {
                data: if capacity != 0 { DATA } else { ALLOCATOR },
                capacity,
                count,
            },
        );
        if capacity != 0 {
            for i in 0..count {
                m.put64(DATA + i as u64 * 8, 0x40000 + i as u64 * 0x100);
            }
            m.put64(DATA + capacity as u64 * 8, ALLOCATOR);
        }
        m
    }
    fn rejected(m: &mut Mem, reason: Failure) {
        let mut plan = MovePlan::default();
        assert!(
            !plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o))),
            "accepted, expected {reason:?}"
        );
        assert!(
            !plan.ready() && plan.rejection() == reason,
            "{:?} != {reason:?}",
            plan.rejection()
        );
        assert!(
            !plan.matches_before(&mut |a, o| m.read(a, o))
                && !plan.verify_detached(Some(&mut |a, o| m.read(a, o)))
        );
    }

    #[test]
    fn move_plan() {
        let mut m = fresh(4, 2);
        let mut plan = MovePlan::default();
        assert!(
            plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o)))
                && plan.ready()
                && plan.owns_allocation()
        );
        assert!(plan.owner_address() == GRAPH + 0x70 && plan.trailer_address() == DATA + 32);
        assert!(
            plan.saved_header()
                == NativeVector {
                    data: DATA,
                    capacity: 4,
                    count: 2
                }
                && plan.empty_header()
                    == NativeVector {
                        data: ALLOCATOR,
                        capacity: 0,
                        count: 0
                    }
        );
        assert!(
            plan.definitions() == [0x40000, 0x40100]
                && plan.matches_before(&mut |a, o| m.read(a, o))
                && !plan.verify_detached(Some(&mut |a, o| m.read(a, o)))
        );
        m.put_vec(GRAPH + 0x70, plan.empty_header());
        assert!(
            plan.verify_detached(Some(&mut |a, o| m.read(a, o)))
                && !plan.matches_before(&mut |a, o| m.read(a, o))
        );
        let mut m = fresh(0, 0);
        assert!(
            plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o)))
                && !plan.owns_allocation()
                && plan.trailer_address() == 0
        );
        assert!(plan.saved_header() == plan.empty_header() && plan.definitions().is_empty());
        assert!(
            plan.matches_before(&mut |a, o| m.read(a, o))
                && plan.verify_detached(Some(&mut |a, o| m.read(a, o)))
        );
        let mut m = fresh(4, 0);
        assert!(
            plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o)))
                && plan.owns_allocation()
                && plan.definitions().is_empty()
        );
        let mut m = fresh(MAX_DEFINITIONS, MAX_DEFINITIONS);
        assert!(
            plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o)))
                && plan.definitions().len() == MAX_DEFINITIONS as usize
        );
        let mut m = fresh(4, 2);
        for graph in [0, GRAPH + 1, u64::MAX - 7] {
            assert!(
                !plan.capture(graph, Some(&mut |a, o| m.read(a, o)))
                    && plan.rejection() == Failure::Arguments
            );
        }
        assert!(!plan.capture(GRAPH, None) && plan.rejection() == Failure::Arguments);
    }
    #[test]
    fn move_plan_rejections() {
        let mut m = fresh(4, 2);
        m.erase(GRAPH + 0x70);
        rejected(&mut m, Failure::Header);
        for v in [
            NativeVector {
                data: 0,
                capacity: 4,
                count: 2,
            },
            NativeVector {
                data: DATA + 1,
                capacity: 4,
                count: 2,
            },
            NativeVector {
                data: DATA,
                capacity: MAX_DEFINITIONS + 1,
                count: 0,
            },
            NativeVector {
                data: DATA,
                capacity: 1,
                count: 2,
            },
            NativeVector {
                data: ALLOCATOR,
                capacity: 0,
                count: 1,
            },
            NativeVector {
                data: u64::MAX - 7,
                capacity: 4,
                count: 0,
            },
        ] {
            let mut m = fresh(4, 2);
            m.put_vec(GRAPH + 0x70, v);
            rejected(&mut m, Failure::Bounds);
        }
        let mut m = fresh(4, 2);
        m.put64(DATA, 0);
        let mut plan = MovePlan::default();
        assert!(
            plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o)))
                && plan.definitions() == [0, 0x40100]
        );
        m.put64(DATA + 8, 0);
        assert!(
            plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o))) && plan.definitions() == [0, 0]
        );
        for (at, v) in [
            (DATA, 0x40001),
            (DATA + 8, 0x40000),
            (DATA, DATA + 8),
            (DATA, GRAPH + 0x70),
        ] {
            let mut m = fresh(4, 2);
            m.put64(at, v);
            rejected(&mut m, Failure::Entries);
        }
        let mut m = fresh(4, 2);
        m.erase(DATA + 8);
        rejected(&mut m, Failure::Entries);
        for v in [0, ALLOCATOR + 1, DATA] {
            let mut m = fresh(4, 2);
            m.put64(DATA + 32, v);
            rejected(&mut m, Failure::Allocator);
        }
        let mut m = fresh(4, 2);
        m.erase(DATA + 32);
        rejected(&mut m, Failure::Allocator);
        for (address, byte) in [
            (GRAPH + 0x70 + 12, 1u8),
            (DATA + 1, 0x80),
            (DATA + 32 + 1, 0x80),
        ] {
            let mut m = fresh(4, 2);
            m.mutate_after = 3;
            m.mutate_address = address;
            m.mutate_byte = byte;
            rejected(&mut m, Failure::Changed);
        }
        let mut m = fresh(4, 2);
        let mut plan = MovePlan::default();
        assert!(plan.capture(GRAPH, Some(&mut |a, o| m.read(a, o))));
        m.put_vec(GRAPH + 0x70, plan.empty_header());
        m.reads = 0;
        m.mutate_after = 1;
        m.mutate_address = GRAPH + 0x70 + 12;
        m.mutate_byte = 1;
        assert!(!plan.verify_detached(Some(&mut |a, o| m.read(a, o))));
    }
    #[test]
    fn retire_state() {
        let mut s = RetireState::new(EPOCH, true);
        assert!(!s.ready_to_retire() && !s.begin_retire(EPOCH));
        assert!(
            s.add_consumer(EPOCH, 10) && s.add_consumer(EPOCH, 11) && s.pending_consumers() == 2
        );
        assert!(s.verify_detach(EPOCH, true) && !s.ready_to_retire() && s.close_source_uses(EPOCH));
        assert!(!s.begin_retire(EPOCH) && s.complete_consumer(EPOCH, 10) && !s.ready_to_retire());
        assert!(s.complete_consumer(EPOCH, 11) && s.ready_to_retire());
        assert!(
            s.begin_retire(EPOCH) && s.retiring() && !s.begin_retire(EPOCH) && !s.ready_to_retire()
        );
        assert!(
            s.finish_retire(EPOCH)
                && s.retired()
                && !s.begin_retire(EPOCH)
                && !s.finish_retire(EPOCH)
        );
        assert_eq!(s.failure_reason(), RetireFailure::Repeated);
        let mut u = RetireState::new(EPOCH, false);
        assert!(
            !u.add_consumer(EPOCH, 1)
                && !u.verify_detach(EPOCH, true)
                && !u.ready_to_retire()
                && u.failure_reason() == RetireFailure::UnaccountedPriorUse
        );
        assert_eq!(
            RetireState::new(0, true).failure_reason(),
            RetireFailure::Epoch
        );
        let mut m = RetireState::new(EPOCH, true);
        assert!(!m.add_consumer(EPOCH + 1, 1) && m.failure_reason() == RetireFailure::Epoch);
        let mut d = RetireState::new(EPOCH, true);
        assert!(d.add_consumer(EPOCH, 1) && !d.add_consumer(EPOCH, 1) && !d.ready_to_retire());
        let mut c = RetireState::new(EPOCH, true);
        assert!(
            c.add_consumer(EPOCH, 1)
                && c.complete_consumer(EPOCH, 1)
                && !c.complete_consumer(EPOCH, 1)
        );
        assert!(!RetireState::new(EPOCH, true).complete_consumer(EPOCH, 1));
        let mut x = RetireState::new(EPOCH, true);
        assert!(x.add_consumer(EPOCH, 1));
        x.unclaimed_native_mutation();
        assert!(
            !x.verify_detach(EPOCH, true) && !x.complete_consumer(EPOCH, 1) && !x.ready_to_retire()
        );
        assert!(!RetireState::new(EPOCH, true).close_source_uses(EPOCH));
        let mut f = RetireState::new(EPOCH, true);
        assert!(!f.verify_detach(EPOCH, false) && !f.ready_to_retire());
        let mut l = RetireState::new(EPOCH, true);
        assert!(
            l.verify_detach(EPOCH, true)
                && l.close_source_uses(EPOCH)
                && !l.add_consumer(EPOCH, 1)
                && !l.ready_to_retire()
        );
        let mut n = RetireState::new(EPOCH, true);
        assert!(n.verify_detach(EPOCH, true) && n.close_source_uses(EPOCH) && n.ready_to_retire());
        assert!(n.begin_retire(EPOCH) && !n.begin_retire(EPOCH));
        let mut cap = RetireState::new(EPOCH, true);
        for i in 1..=MAX_CONSUMERS as u64 {
            assert!(cap.add_consumer(EPOCH, i));
        }
        assert!(!cap.add_consumer(EPOCH, MAX_CONSUMERS as u64 + 1));
    }
}
