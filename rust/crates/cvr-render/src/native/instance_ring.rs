//! The native instance upload ring's numeric observations and the owned CPU copies of its completed writers
//! (include/Render/NativeInstanceRing.hpp, src/Render/NativeInstanceRing.cpp).
//!
//! 2.5: the ring constructor/destructor only destroy their callbacks through sub_142004BA0 (the same ring fields and
//! constants); the stamp pair takes the registry spinlock (+0x13BA9C0 -> +0x140E9D0) through a guard object and reads
//! the 24-byte slot records at +0x1408950 (2.31 +0x13B4940), version at +0x10. Guarded as whole functions.
//!
//! Numeric observations never acquire a native pointer, pin an allocation, or establish producer publication,
//! no-overwrite, submission or GPU copy permission; head/tail/capacity are INSTANCE counts, not bytes (native ring
//! offsets: handle 0, mapped 8, capacity 0x10, stride 0x14, retired tail 0x38, head 0x40). `complete` accepts only
//! bytes sampled from the actual mapped destination AFTER the genuine native writer returned: it copies them, never
//! reads native memory, and cannot certify that sampling fact. Genuine backing ownership, the producer/source joins
//! and the original GPU read epoch remain external caller proofs; nothing grants copy permission. The caller
//! serializes ledger operations.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code {
    pub rva: u32,
    pub bytes: u32,
    pub fnv: u64,
}
pub const NATIVE_CODE: [Code; 8] = [
    Code {
        rva: 0x80D5A0,
        bytes: 0x162,
        fnv: 0xE8E0_1449_FD6B_CC96,
    },
    Code {
        rva: 0x80D704,
        bytes: 0x7D,
        fnv: 0xF852_EC5B_6174_D66E,
    },
    Code {
        rva: 0x337144,
        bytes: 0xDB,
        fnv: 0x5AE0_D2CD_64C9_FC88,
    },
    Code {
        rva: 0x939ACC,
        bytes: 0x51,
        fnv: 0x4251_259E_7ABD_2CB6,
    },
    Code {
        rva: 0x4F3498,
        bytes: 0x64,
        fnv: 0xCBEF_2DC8_DE9C_716F,
    },
    Code {
        rva: 0x4F34FC,
        bytes: 0xDC,
        fnv: 0x5DD2_A8E3_24AC_5BFA,
    },
    Code {
        rva: 0x3E6A48,
        bytes: 0x77,
        fnv: 0xFB12_05A8_FC3A_5C47,
    },
    Code {
        rva: 0x3E6504,
        bytes: 0xA9,
        fnv: 0x3516_42C3_D6C0_138C,
    },
];
pub const NORMAL_WRITER_RETURN: u32 = 0x335991;
pub const SKIN_WRITER_RETURN: u32 = 0x333CDF;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RingObservation {
    pub ring: u64,
    pub generation: u64,
    pub resource: u64,
    pub gpu: u64,
    pub mapped: u64,
    pub registry: u64,
    pub allocation: u64,
    pub allocator: u64,
    pub heap: u64,
    pub width: u64,
    pub capacity: u64,
    pub head: u64,
    pub tail: u64,
    pub handle: u32,
    pub stride: u32,
    pub kind: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub thread: u64,
    pub context: u64,
    pub registry: u64,
    pub stamp: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriterTicket {
    pub id: u64,
    pub observer_epoch: u64,
    pub ring: u64,
    pub ring_generation: u64,
    pub scope: Scope,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompletedWriter {
    pub writer: WriterTicket,
    pub absolute_begin: u64,
    pub first_instance: u32,
    pub instances: u32,
}
/// Exactly one native 16-byte {submission stamp, absolute head} publication; these labels are no
/// submission/completion certificate.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Publication {
    pub scope: Scope,
    pub head: u64,
}
/// A read of a completed writer's range. Its source labels' association with the exact actual IA resource/range and
/// recording is a native caller proof, not this ledger's.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadRequest {
    pub writer: WriterTicket,
    pub source: Scope,
    pub absolute_begin: u64,
    pub first_instance: u32,
    pub instances: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub absolute_end: u64,
    pub byte_offset: u64,
    pub bytes: u64,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Observed,
    Completed,
    Published,
    Recreated,
    UnknownRing,
    InvalidObservation,
    Changed,
    InvalidScope,
    UnknownWriter,
    Pending,
    RangeMismatch,
    Overlap,
    Retired,
    UnknownWrite,
    Capacity,
    Exception,
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub rings: usize,
    pub pending_writers: usize,
    pub ranges: usize,
    pub publications: usize,
    pub writer_bytes: usize,
    pub live_cpu_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Limits {
            rings: 64,
            pending_writers: 256,
            ranges: 4096,
            publications: 4096,
            writer_bytes: 65536,
            live_cpu_bytes: 16 * 1024 * 1024,
        }
    }
}

fn valid(a: &RingObservation) -> bool {
    a.ring != 0
        && a.ring & 7 == 0
        && a.ring <= u64::MAX - 0x60
        && a.generation != 0
        && a.resource != 0
        && a.gpu != 0
        && a.mapped != 0
        && a.registry != 0
        && a.allocation != 0
        && a.allocator != 0
        && a.heap != 0
        && a.handle != 0
        && a.handle <= 0x100000
        && a.kind == 5
        && ((a.capacity == 0x20000 && a.stride == 48) || (a.capacity == 0x8000 && a.stride == 64))
        && a.width == a.capacity * a.stride as u64
        && a.gpu <= u64::MAX - a.width
        && a.mapped <= u64::MAX - a.width
        && a.tail <= a.head
        && a.head - a.tail <= a.capacity
}
fn identity(a: &RingObservation, b: &RingObservation) -> bool {
    RingObservation {
        head: 0,
        tail: 0,
        ..*a
    } == RingObservation {
        head: 0,
        tail: 0,
        ..*b
    }
}
fn valid_scope(a: &Scope) -> bool {
    a.list != 0
        && a.generation != 0
        && a.occurrence != 0
        && a.thread != 0
        && a.context != 0
        && a.registry != 0
        && a.stamp != 0
}
fn success(status: Status) -> bool {
    status == Status::Observed || status == Status::Recreated
}
fn intersects(begin: u64, bytes: u64, other: u64, size: u64) -> bool {
    begin < other + size && other < begin + bytes
}
/// A writer's instances as absolute and physical byte ranges of the observed ring.
pub fn instance_range(
    a: &RingObservation,
    absolute: u64,
    first: u32,
    instances: u32,
) -> Option<Range> {
    let (first64, instances64) = (first as u64, instances as u64);
    if !valid(a)
        || instances == 0
        || first64 >= a.capacity
        || absolute < a.tail
        || absolute > a.head
        || instances64 > a.head - absolute
        || absolute % a.capacity != first64
        || instances64 > a.capacity - first64
    {
        return None;
    }
    Some(Range {
        absolute_end: absolute + instances64,
        byte_offset: first64 * a.stride as u64,
        bytes: instances64 * a.stride as u64,
    })
}

/// A completed writer's owned CPU bytes, charged to the ledger's budget while anyone holds them.
pub struct Payload {
    producer: CompletedWriter,
    bytes: Vec<u8>,
    budget: Arc<AtomicUsize>,
}
impl Payload {
    pub fn producer(&self) -> CompletedWriter {
        self.producer
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes.len(), Ordering::Relaxed);
    }
}

/// A completed writer's bytes for one read. Remains the immutable observed CPU version after reuse/recreation (query
/// again to test the current provenance); held bytes grant no native access.
#[derive(Clone, Default)]
pub struct CpuCandidate {
    data: Option<Arc<Payload>>,
    source: ReadRequest,
    publication: Publication,
    offset: usize,
    bytes: usize,
}
impl CpuCandidate {
    pub fn is_some(&self) -> bool {
        self.data.is_some() && self.bytes != 0
    }
    pub fn bytes(&self) -> &[u8] {
        self.data
            .as_ref()
            .map_or(&[], |d| &d.bytes[self.offset..self.offset + self.bytes])
    }
    pub fn producer(&self) -> CompletedWriter {
        self.data
            .as_ref()
            .map_or_else(CompletedWriter::default, |d| d.producer)
    }
    pub fn source(&self) -> &ReadRequest {
        &self.source
    }
    pub fn published(&self) -> &Publication {
        &self.publication
    }
    /// The shared payload with this read's byte offset and length in it.
    pub fn into_payload(self) -> (Option<Arc<Payload>>, usize, usize) {
        (self.data, self.offset, self.bytes)
    }
}

/// An extra token a completed writer carries while the ledger keeps its version (the C++ face's shared_ptr presence),
/// dropped with it.
pub type PresenceToken = Box<dyn core::any::Any + Send>;
pub type PresenceHook = fn(&WriterTicket) -> Option<PresenceToken>;

struct RingState {
    observed: RingObservation,
    current: bool,
    publications: Vec<Publication>,
}
struct Writer {
    ticket: WriterTicket,
    data: Option<Arc<Payload>>,
    publication: Publication,
    pending: Option<CompletedWriter>,
    conflicted: bool,
    presence: Option<Arc<WriterTicket>>,
    token: Option<PresenceToken>,
}
impl Writer {
    fn new(ticket: WriterTicket) -> Self {
        Writer {
            ticket,
            data: None,
            publication: Publication::default(),
            pending: None,
            conflicted: false,
            presence: None,
            token: None,
        }
    }
    fn pending_matches(&self, absolute: u64, first: u32, instances: u32) -> bool {
        self.pending.is_none_or(|p| {
            p.absolute_begin == absolute && p.first_instance == first && p.instances == instances
        })
    }
}

pub struct Ledger {
    limits: Limits,
    epoch: u64,
    next: u64,
    exhausted: bool,
    rings: Vec<RingState>,
    writers: Vec<Writer>,
    budget: Arc<AtomicUsize>,
    presence_hook: Option<PresenceHook>,
}
impl Default for Ledger {
    fn default() -> Self {
        Ledger::new(Limits::default())
    }
}
impl Ledger {
    pub fn new(limits: Limits) -> Self {
        Ledger {
            limits,
            epoch: 1,
            next: 0,
            exhausted: false,
            rings: Vec::new(),
            writers: Vec::new(),
            budget: Arc::new(AtomicUsize::new(0)),
            presence_hook: None,
        }
    }
    /// Every later completed version also carries the hook's token.
    pub fn set_presence_hook(&mut self, hook: PresenceHook) {
        self.presence_hook = Some(hook);
    }
    fn ring_index(&self, address: u64) -> Option<usize> {
        self.rings.iter().position(|r| r.observed.ring == address)
    }
    fn writer_index(&self, ticket: &WriterTicket) -> Option<usize> {
        self.writers.iter().position(|w| w.ticket == *ticket)
    }
    fn erase(&mut self, ring: u64) {
        self.writers.retain(|w| w.ticket.ring != ring);
    }
    fn reject(&mut self, ring: u64) {
        if let Some(i) = self.ring_index(ring) {
            self.rings[i].current = false;
            self.rings[i].publications.clear();
        }
        self.erase(ring);
    }
    fn reject_all(&mut self) {
        self.exhausted = true;
        for ring in &mut self.rings {
            ring.current = false;
            ring.publications.clear();
        }
        self.writers.clear();
    }
    fn pending(&self, ring: u64) -> bool {
        self.writers
            .iter()
            .any(|w| w.ticket.ring == ring && w.data.is_none())
    }
    fn pending_range(&self, ring: u64, range: &Range, stride: u32, skip: u64) -> bool {
        self.writers.iter().any(|w| {
            w.ticket.ring == ring
                && w.ticket.id != skip
                && w.data.is_none()
                && w.pending.is_none_or(|p| {
                    intersects(
                        range.byte_offset,
                        range.bytes,
                        p.first_instance as u64 * stride as u64,
                        p.instances as u64 * stride as u64,
                    )
                })
        })
    }
    fn known_pending_overlap(&self, ring: u64, range: &Range, stride: u32, skip: u64) -> bool {
        self.writers.iter().any(|w| {
            w.ticket.ring == ring
                && w.ticket.id != skip
                && w.data.is_none()
                && w.pending.is_some_and(|p| {
                    intersects(
                        range.byte_offset,
                        range.bytes,
                        p.first_instance as u64 * stride as u64,
                        p.instances as u64 * stride as u64,
                    )
                })
        })
    }
    /// Pending writers intersecting the range conflicted, completed versions intersecting it dropped (true: any).
    fn overwrite(&mut self, a: &RingObservation, range: &Range, skip: u64) -> bool {
        let stride = a.stride as u64;
        for w in &mut self.writers {
            if w.ticket.ring == a.ring && w.ticket.id != skip && w.data.is_none() {
                if let Some(p) = w.pending {
                    if intersects(
                        range.byte_offset,
                        range.bytes,
                        p.first_instance as u64 * stride,
                        p.instances as u64 * stride,
                    ) {
                        w.conflicted = true;
                    }
                }
            }
        }
        let mut overlap = false;
        self.writers.retain(|w| {
            let Some(data) = w.data.as_ref() else {
                return true;
            };
            if w.ticket.ring != a.ring || w.ticket.id == skip {
                return true;
            }
            let p = data.producer;
            let hit = intersects(
                range.byte_offset,
                range.bytes,
                p.first_instance as u64 * stride,
                p.instances as u64 * stride,
            );
            overlap |= hit;
            !hit
        });
        overlap
    }

    pub fn observe(&mut self, a: &RingObservation) -> Status {
        if self.exhausted {
            return Status::Capacity;
        }
        if !valid(a) {
            self.reject(a.ring);
            return Status::InvalidObservation;
        }
        let Some(i) = self.ring_index(a.ring) else {
            if self.rings.len() >= self.limits.rings {
                self.reject_all();
                return Status::Capacity;
            }
            self.rings.push(RingState {
                observed: *a,
                current: true,
                publications: Vec::new(),
            });
            return Status::Observed;
        };
        if a.generation > self.rings[i].observed.generation {
            self.erase(a.ring);
            let ring = &mut self.rings[i];
            ring.observed = *a;
            ring.current = true;
            ring.publications.clear();
            return Status::Recreated;
        }
        let ring = &self.rings[i];
        if !ring.current
            || !identity(a, &ring.observed)
            || a.head < ring.observed.head
            || a.tail < ring.observed.tail
        {
            self.reject(a.ring);
            return Status::Changed;
        }
        self.rings[i].observed = *a;
        self.writers.retain(|w| {
            !(w.ticket.ring == a.ring
                && w.data
                    .as_ref()
                    .is_some_and(|d| d.producer.absolute_begin < a.tail))
        });
        self.rings[i].publications.retain(|p| p.head > a.tail);
        Status::Observed
    }
    pub fn begin(&mut self, a: &RingObservation, scope: &Scope) -> (Status, WriterTicket) {
        let observed = self.observe(a);
        if !success(observed) {
            return (observed, WriterTicket::default());
        }
        if !valid_scope(scope) || scope.registry != a.registry {
            self.reject(a.ring);
            return (Status::InvalidScope, WriterTicket::default());
        }
        if self
            .writers
            .iter()
            .any(|w| w.data.is_none() && w.ticket.scope == *scope)
        {
            self.reject(a.ring);
            return (Status::InvalidScope, WriterTicket::default());
        }
        let count = self.writers.iter().filter(|w| w.data.is_none()).count();
        if count >= self.limits.pending_writers || self.next == u64::MAX {
            self.reject_all();
            return (Status::Capacity, WriterTicket::default());
        }
        self.next += 1;
        let ticket = WriterTicket {
            id: self.next,
            observer_epoch: self.epoch,
            ring: a.ring,
            ring_generation: a.generation,
            scope: *scope,
        };
        self.writers.push(Writer::new(ticket));
        (Status::Observed, ticket)
    }
    /// The writer's actual absolute range, known before it writes: intersecting completed versions are dropped.
    pub fn qualify_write(
        &mut self,
        a: &RingObservation,
        ticket: &WriterTicket,
        absolute: u64,
        first: u32,
        instances: u32,
    ) -> Status {
        let observed = self.observe(a);
        if !success(observed) {
            return observed;
        }
        let Some(i) = self
            .writer_index(ticket)
            .filter(|&i| self.writers[i].data.is_none())
        else {
            return Status::UnknownWriter;
        };
        let Some(range) = instance_range(a, absolute, first, instances) else {
            return Status::RangeMismatch;
        };
        if !self.writers[i].pending_matches(absolute, first, instances) {
            return Status::Changed;
        }
        let overlap = self.known_pending_overlap(a.ring, &range, a.stride, ticket.id);
        self.overwrite(a, &range, ticket.id);
        let i = self.writer_index(ticket).expect("the qualified writer");
        let writer = &mut self.writers[i];
        writer.pending = Some(CompletedWriter {
            writer: *ticket,
            absolute_begin: absolute,
            first_instance: first,
            instances,
        });
        writer.conflicted |= overlap;
        if writer.conflicted {
            Status::Overlap
        } else {
            Status::Observed
        }
    }
    pub fn validate_write(&self, ticket: &WriterTicket) -> Status {
        let Some(writer) = self.writers.iter().find(|w| w.ticket == *ticket) else {
            return Status::UnknownWriter;
        };
        let (None, Some(pending)) = (writer.data.as_ref(), writer.pending) else {
            return Status::UnknownWriter;
        };
        if writer.conflicted {
            return Status::Overlap;
        }
        let Some(ring) = self
            .ring_index(ticket.ring)
            .map(|i| &self.rings[i])
            .filter(|r| r.current)
        else {
            return Status::Changed;
        };
        let Some(range) = instance_range(
            &ring.observed,
            pending.absolute_begin,
            pending.first_instance,
            pending.instances,
        ) else {
            return Status::RangeMismatch;
        };
        if self.pending_range(ticket.ring, &range, ring.observed.stride, ticket.id) {
            Status::Pending
        } else {
            Status::Observed
        }
    }
    /// Only after this exact native writer has returned: its known range was invalidated before writing, and dropping
    /// its sample preserves the other writers.
    pub fn discard_completed(&mut self, ticket: &WriterTicket) -> Status {
        let Some(i) = self.writer_index(ticket) else {
            return Status::UnknownWriter;
        };
        if self.writers[i].data.is_some() || self.writers[i].pending.is_none() {
            return Status::Pending;
        }
        self.writers.remove(i);
        Status::Observed
    }
    /// A guarded native cached-write branch supplies a physical range even if its earlier absolute reservation was
    /// not observed: it invalidates only.
    pub fn invalidate_physical(
        &mut self,
        a: &RingObservation,
        first: u32,
        instances: u32,
    ) -> Status {
        let observed = self.observe(a);
        if !success(observed) {
            return observed;
        }
        if instances == 0
            || first as u64 >= a.capacity
            || instances as u64 > a.capacity - first as u64
        {
            return Status::RangeMismatch;
        }
        let range = Range {
            absolute_end: 0,
            byte_offset: first as u64 * a.stride as u64,
            bytes: instances as u64 * a.stride as u64,
        };
        if self.overwrite(a, &range, 0) {
            Status::Overlap
        } else {
            Status::Observed
        }
    }
    pub fn finish_unobserved(
        &mut self,
        a: &RingObservation,
        ticket: &WriterTicket,
        first: u32,
        instances: u32,
    ) -> Status {
        let known = self
            .writer_index(ticket)
            .is_some_and(|i| self.writers[i].data.is_none() && self.writers[i].pending.is_none());
        if !known || ticket.ring != a.ring {
            return Status::UnknownWriter;
        }
        let status = self.invalidate_physical(a, first, instances);
        if status != Status::Observed && status != Status::Overlap {
            return status;
        }
        self.writers.retain(|w| w.ticket != *ticket);
        status
    }
    /// The returned writer's bytes sampled from the mapped destination, owned.
    pub fn complete(
        &mut self,
        a: &RingObservation,
        completed: &CompletedWriter,
        bytes: &[u8],
    ) -> Status {
        let observed = self.observe(a);
        if !success(observed) {
            return observed;
        }
        let index = self.writer_index(&completed.writer);
        let Some(i) = index.filter(|_| {
            completed.writer.ring == a.ring && completed.writer.ring_generation == a.generation
        }) else {
            self.reject(a.ring);
            return Status::UnknownWriter;
        };
        if self.writers[i].data.is_some() {
            self.reject(a.ring);
            return Status::Changed;
        }
        let range = instance_range(
            a,
            completed.absolute_begin,
            completed.first_instance,
            completed.instances,
        );
        let Some(range) = range.filter(|r| bytes.len() as u64 == r.bytes) else {
            self.reject(a.ring);
            return Status::RangeMismatch;
        };
        if !self.writers[i].pending_matches(
            completed.absolute_begin,
            completed.first_instance,
            completed.instances,
        ) {
            return Status::RangeMismatch;
        }
        if self.writers[i].conflicted
            || self.pending_range(a.ring, &range, a.stride, completed.writer.id)
        {
            return Status::Pending;
        }
        // This writer has returned: its actual overwritten bytes stay invalid even when the bounded CPU cache cannot
        // retain the new sample.
        self.overwrite(a, &range, completed.writer.id);
        let drop_sample = |ledger: &mut Ledger| {
            ledger.writers.retain(|w| w.ticket != completed.writer);
            Status::Capacity
        };
        if self.limits.ranges == 0
            || bytes.len() > self.limits.writer_bytes
            || bytes.len() > self.limits.live_cpu_bytes
        {
            return drop_sample(self);
        }
        let mut count = self.writers.iter().filter(|w| w.data.is_some()).count();
        while count >= self.limits.ranges
            || self.budget.load(Ordering::Relaxed) > self.limits.live_cpu_bytes - bytes.len()
        {
            let Some(oldest) = self.writers.iter().position(|w| w.data.is_some()) else {
                return drop_sample(self);
            };
            self.writers.remove(oldest);
            count -= 1;
        }
        self.budget.fetch_add(bytes.len(), Ordering::Relaxed);
        let payload = Arc::new(Payload {
            producer: *completed,
            bytes: bytes.to_vec(),
            budget: self.budget.clone(),
        });
        let i = self
            .writer_index(&completed.writer)
            .expect("the completed writer");
        self.writers[i].data = Some(payload);
        self.writers[i].presence = Some(Arc::new(completed.writer));
        self.writers[i].token = self.presence_hook.and_then(|hook| hook(&completed.writer));
        Status::Completed
    }
    pub fn publish(&mut self, a: &RingObservation, publication: &Publication) -> Status {
        let observed = self.observe(a);
        if !success(observed) {
            return observed;
        }
        if !valid_scope(&publication.scope)
            || publication.scope.registry != a.registry
            || publication.head != a.head
        {
            self.reject(a.ring);
            return Status::InvalidScope;
        }
        if self.pending(a.ring) {
            return Status::Pending;
        }
        let i = self.ring_index(a.ring).expect("the observed ring");
        if self.rings[i]
            .publications
            .iter()
            .any(|p| p.scope.stamp == publication.scope.stamp)
        {
            self.reject(a.ring);
            return Status::Changed;
        }
        let count: usize = self.rings.iter().map(|r| r.publications.len()).sum();
        if count >= self.limits.publications {
            self.reject_all();
            return Status::Capacity;
        }
        self.rings[i].publications.push(*publication);
        for w in &mut self.writers {
            if w.ticket.ring == a.ring
                && w.data.as_ref().is_some_and(|d| {
                    d.producer.absolute_begin + d.producer.instances as u64 <= publication.head
                })
            {
                w.publication = *publication;
            }
        }
        Status::Published
    }
    /// Clears the local CPU provenance after a genuine NEW native reservation, keeping the same native lifecycle
    /// generation (a numeric observation cannot do it).
    pub fn restart_observation(&mut self, a: &RingObservation) -> Status {
        if self.exhausted {
            return Status::Capacity;
        }
        let Some(i) = self.ring_index(a.ring) else {
            return Status::UnknownRing;
        };
        let observed = self.rings[i].observed;
        if !valid(a) || !identity(a, &observed) || a.head < observed.head || a.tail < observed.tail
        {
            self.reject(a.ring);
            return Status::Changed;
        }
        self.erase(a.ring);
        let ring = &mut self.rings[i];
        ring.observed = *a;
        ring.current = true;
        ring.publications.clear();
        Status::Observed
    }
    /// An actual reservation/reuse observation invalidates intersecting prior versions; the absolute range comes from
    /// the native reservation, never an inference from a later head or a matching physical address.
    pub fn reuse(
        &mut self,
        a: &RingObservation,
        absolute: u64,
        instances: u32,
        active_writer: Option<&WriterTicket>,
    ) -> Status {
        let observed = self.observe(a);
        if !success(observed) {
            return observed;
        }
        if let Some(active) = active_writer {
            let live = self
                .writer_index(active)
                .is_some_and(|i| self.writers[i].data.is_none());
            if !live || active.ring != a.ring {
                self.reject(a.ring);
                return Status::UnknownWriter;
            }
        }
        let Some(range) = instance_range(a, absolute, (absolute % a.capacity) as u32, instances)
        else {
            self.reject(a.ring);
            return Status::RangeMismatch;
        };
        if self.overwrite(a, &range, 0) {
            Status::Overlap
        } else {
            Status::Observed
        }
    }
    pub fn unknown(&mut self, ring: u64) {
        if ring != 0 {
            self.reject(ring);
        } else {
            self.reject_all();
        }
    }
    pub fn destroy(&mut self, ring: u64) {
        self.reject(ring);
    }
    pub fn reset(&mut self) {
        self.writers.clear();
        self.rings.clear();
        if self.epoch == u64::MAX {
            self.exhausted = true;
        } else {
            self.epoch += 1;
            self.exhausted = false;
        }
    }
    pub fn query(&mut self, a: &RingObservation, request: &ReadRequest) -> (Status, CpuCandidate) {
        let observed = self.observe(a);
        if !success(observed) {
            return (observed, CpuCandidate::default());
        }
        let writer = self.writers.iter().find(|w| w.ticket == request.writer);
        let Some(writer) = writer.filter(|_| {
            request.writer.ring == a.ring && request.writer.ring_generation == a.generation
        }) else {
            return (Status::UnknownWriter, CpuCandidate::default());
        };
        let Some(data) = writer.data.clone() else {
            return (Status::Pending, CpuCandidate::default());
        };
        let publication = writer.publication;
        if !valid_scope(&request.source) || request.source.registry != a.registry {
            return (Status::InvalidScope, CpuCandidate::default());
        }
        let Some(range) = instance_range(
            a,
            request.absolute_begin,
            request.first_instance,
            request.instances,
        ) else {
            return (Status::RangeMismatch, CpuCandidate::default());
        };
        if self.pending_range(a.ring, &range, a.stride, 0) {
            return (Status::Pending, CpuCandidate::default());
        }
        let producer = data.producer;
        if request.absolute_begin < producer.absolute_begin
            || request.absolute_begin - producer.absolute_begin > producer.instances as u64
            || request.instances as u64
                > producer.instances as u64 - (request.absolute_begin - producer.absolute_begin)
        {
            return (Status::RangeMismatch, CpuCandidate::default());
        }
        let mut candidate = CpuCandidate {
            data: Some(data),
            source: *request,
            publication: Publication::default(),
            offset: ((request.absolute_begin - producer.absolute_begin) * a.stride as u64) as usize,
            bytes: range.bytes as usize,
        };
        if publication.scope.stamp != 0 && range.absolute_end <= publication.head {
            candidate.publication = publication;
            return (Status::Published, candidate);
        }
        (Status::Completed, candidate)
    }
    /// Cache membership only, not permission to read native memory: expires on eviction/retirement even while the
    /// immutable CPU bytes are held.
    pub fn completed_presence(&self, ticket: &WriterTicket) -> Weak<WriterTicket> {
        self.writers
            .iter()
            .find(|w| w.ticket == *ticket)
            .and_then(|w| w.presence.as_ref().map(Arc::downgrade))
            .unwrap_or_default()
    }
    /// The completed version's token from the presence hook, if any.
    pub fn presence_token(&self, ticket: &WriterTicket) -> Option<&PresenceToken> {
        self.writers
            .iter()
            .find(|w| w.ticket == *ticket)
            .and_then(|w| w.token.as_ref())
    }
    pub fn live_cpu_bytes(&self) -> usize {
        self.budget.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/native_instance_ring_test.cpp's and instance_ring_concurrent_test.cpp's cases.
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Barrier, Condvar, Mutex};

    fn native(head: u64, tail: u64, generation: u64) -> RingObservation {
        RingObservation {
            ring: 0x1000,
            generation,
            resource: 0x2000,
            gpu: 0x1000_0000,
            mapped: 0x2000_0000,
            registry: 0x3000,
            allocation: 0x4000,
            allocator: 0x5000,
            heap: 0x6000,
            width: 0x600000,
            capacity: 0x20000,
            head,
            tail,
            handle: 17,
            stride: 48,
            kind: 5,
        }
    }
    fn scope(occurrence: u64) -> Scope {
        Scope {
            list: 0x7000,
            generation: 9,
            occurrence,
            thread: 3,
            context: 0x8000,
            registry: 0x3000,
            stamp: 0x2401,
        }
    }
    fn completed(
        writer: WriterTicket,
        absolute: u64,
        first: u32,
        instances: u32,
    ) -> CompletedWriter {
        CompletedWriter {
            writer,
            absolute_begin: absolute,
            first_instance: first,
            instances,
        }
    }
    fn write(
        ledger: &mut Ledger,
        n: &RingObservation,
        absolute: u64,
        instances: u32,
        occurrence: u64,
        value: u8,
    ) -> WriterTicket {
        let (status, ticket) = ledger.begin(n, &scope(occurrence));
        assert_eq!(status, Status::Observed, "a live writer begin");
        let bytes = vec![value; instances as usize * n.stride as usize];
        let c = completed(ticket, absolute, (absolute % n.capacity) as u32, instances);
        assert_eq!(
            ledger.complete(n, &c, &bytes),
            Status::Completed,
            "the completed mapped writer bytes"
        );
        ticket
    }
    fn read(writer: WriterTicket, absolute: u64, instances: u32, first: u32) -> ReadRequest {
        ReadRequest {
            writer,
            source: scope(99),
            absolute_begin: absolute,
            first_instance: first,
            instances,
        }
    }

    #[test]
    fn native_code_and_owned_subranges() {
        assert!(
            NATIVE_CODE[2]
                == Code {
                    rva: 0x337144,
                    bytes: 0xDB,
                    fnv: 0x5AE0_D2CD_64C9_FC88
                }
        );
        assert!(NORMAL_WRITER_RETURN == 0x335991 && SKIN_WRITER_RETURN == 0x333CDF);
        let mut ledger = Ledger::default();
        let n = native(2, 0, 1);
        let writer = write(&mut ledger, &n, 0, 2, 1, 0x3A);
        let r = read(writer, 1, 1, 1);
        let (status, candidate) = ledger.query(&n, &r);
        assert!(
            status == Status::Completed
                && candidate.bytes().len() == 48
                && candidate.bytes().iter().all(|&b| b == 0x3A)
        );
        assert!(
            candidate.producer().writer == writer && candidate.source().source.occurrence == 99
        );
        assert_eq!(
            candidate.published().scope.stamp,
            0,
            "unpublished CPU data invented a submission"
        );
        let publication = Publication {
            scope: scope(100),
            head: 2,
        };
        assert_eq!(ledger.publish(&n, &publication), Status::Published);
        let (status, candidate) = ledger.query(&n, &r);
        assert!(status == Status::Published && *candidate.published() == publication);
        let mut separate = r;
        separate.source.generation += 1;
        separate.source.list += 0x100;
        separate.source.thread = 7;
        let (status, candidate) = ledger.query(&n, &separate);
        assert!(status == Status::Published && candidate.source().source == separate.source);
        let mut foreign = r;
        foreign.source.registry += 1;
        let (status, candidate) = ledger.query(&n, &foreign);
        assert!(status == Status::InvalidScope && !candidate.is_some());
        let changed = ReadRequest {
            first_instance: 0,
            ..r
        };
        let (status, candidate) = ledger.query(&n, &changed);
        assert!(status == Status::RangeMismatch && !candidate.is_some());
        let changed = ReadRequest { instances: 2, ..r };
        assert_eq!(ledger.query(&n, &changed).0, Status::RangeMismatch);
    }
    #[test]
    fn overwrite_retirement_reuse_and_recreation() {
        {
            let mut ledger = Ledger::default();
            let n = native(2, 0, 1);
            let first = write(&mut ledger, &n, 0, 2, 1, 0x3A);
            let (status, old) = ledger.query(&n, &read(first, 0, 2, 0));
            assert_eq!(status, Status::Completed);
            assert_eq!(
                ledger.publish(
                    &n,
                    &Publication {
                        scope: scope(100),
                        head: 2
                    }
                ),
                Status::Published
            );
            let (status, pending) = ledger.begin(&n, &scope(2));
            assert_eq!(status, Status::Observed);
            let (status, current) = ledger.query(&n, &read(first, 0, 1, 0));
            assert!(status == Status::Pending && !current.is_some());
            assert_eq!(
                ledger.complete(&n, &completed(pending, 1, 1, 1), &[0u8; 48]),
                Status::Completed
            );
            let (status, current) = ledger.query(&n, &read(first, 0, 1, 0));
            assert!(status == Status::UnknownWriter && !current.is_some());
            assert!(
                old.bytes().len() == 96 && old.bytes()[0] == 0x3A && ledger.live_cpu_bytes() == 144
            );
            let (status, current) = ledger.query(&n, &read(pending, 1, 1, 1));
            assert!(status == Status::Completed && current.published().scope.stamp == 0);
            drop(old);
            assert_eq!(
                ledger.live_cpu_bytes(),
                48,
                "a released prior CPU payload returns its budget"
            );
        }
        {
            let mut ledger = Ledger::default();
            let mut n = native(2, 0, 1);
            let writer = write(&mut ledger, &n, 0, 2, 1, 0x3A);
            assert_eq!(
                ledger.query(&n, &read(writer, 0, 2, 0)).0,
                Status::Completed
            );
            n.tail = 2;
            assert_eq!(ledger.observe(&n), Status::Observed);
            let (status, candidate) = ledger.query(&n, &read(writer, 0, 1, 0));
            assert!(
                status == Status::UnknownWriter && !candidate.is_some(),
                "the retired tail kept the version"
            );
            n.head = 0x20002;
            n.tail = 0x20000;
            let next = write(&mut ledger, &n, 0x20000, 2, 2, 0x71);
            let (status, candidate) = ledger.query(&n, &read(next, 0x20000, 2, 0));
            assert!(status == Status::Completed && candidate.bytes()[0] == 0x71);
            let (status, candidate) = ledger.query(&n, &read(next, 0, 2, 0));
            assert!(status == Status::RangeMismatch && !candidate.is_some());
        }
        {
            let mut ledger = Ledger::default();
            let mut n = native(2, 0, 1);
            let writer = write(&mut ledger, &n, 0, 2, 1, 0x3A);
            assert_eq!(ledger.reuse(&n, 1, 1, None), Status::Overlap);
            assert_eq!(
                ledger.query(&n, &read(writer, 0, 1, 0)).0,
                Status::UnknownWriter
            );
            let next = write(&mut ledger, &n, 0, 1, 2, 0x3A);
            ledger.unknown(n.ring);
            assert_eq!(ledger.query(&n, &read(next, 0, 1, 0)).0, Status::Changed);
            n.generation += 1;
            assert_eq!(ledger.observe(&n), Status::Recreated);
            assert_eq!(
                ledger.query(&n, &read(next, 0, 1, 0)).0,
                Status::UnknownWriter
            );
            let fresh = write(&mut ledger, &n, 0, 1, 3, 0x3A);
            ledger.destroy(n.ring);
            assert_eq!(ledger.query(&n, &read(fresh, 0, 1, 0)).0, Status::Changed);
        }
        {
            let mut ledger = Ledger::default();
            let n = native(2, 0, 1);
            let (_, ticket) = ledger.begin(&n, &scope(1));
            assert_eq!(
                ledger.complete(&n, &completed(ticket, 0, 0, 1), &[0u8; 47]),
                Status::RangeMismatch
            );
            assert_eq!(
                ledger.observe(&n),
                Status::Changed,
                "a bad completion left the old provenance current"
            );
        }
        {
            let mut ledger = Ledger::default();
            let mut n = native(2, 0, 1);
            let (_, ticket) = ledger.begin(&n, &scope(1));
            assert_eq!(
                ledger.publish(
                    &n,
                    &Publication {
                        scope: scope(2),
                        head: 2
                    }
                ),
                Status::Pending
            );
            n.generation = 2;
            assert_eq!(ledger.observe(&n), Status::Recreated);
            assert_eq!(
                ledger.complete(&n, &completed(ticket, 0, 0, 1), &[0u8; 48]),
                Status::UnknownWriter
            );
        }
        {
            let mut ledger = Ledger::default();
            let mut n = native(2, 0, 1);
            let ticket = write(&mut ledger, &n, 0, 1, 1, 0x3A);
            let changed = RingObservation {
                resource: n.resource + 1,
                ..n
            };
            assert_eq!(ledger.observe(&changed), Status::Changed);
            assert_eq!(ledger.query(&n, &read(ticket, 0, 1, 0)).0, Status::Changed);
            n.generation += 1;
            assert_eq!(ledger.observe(&n), Status::Recreated);
            let fresh = write(&mut ledger, &n, 0, 1, 2, 0x3A);
            let unknown = WriterTicket {
                id: fresh.id + 1,
                ..fresh
            };
            assert_eq!(
                ledger.complete(&n, &completed(unknown, 1, 1, 1), &[0u8; 48]),
                Status::UnknownWriter
            );
            assert_eq!(ledger.query(&n, &read(fresh, 0, 1, 0)).0, Status::Changed);
        }
    }
    #[test]
    fn instance_ranges() {
        let mut n = native(2, 0, 1);
        assert!(instance_range(&n, u64::MAX, 0, 1).is_none());
        n.head = 0x20001;
        n.tail = 1;
        assert!(
            instance_range(&n, 0x1FFFF, 0x1FFFF, 2).is_none(),
            "one writer crossed the physical end"
        );
        let mut n = native(2, 0, 1);
        n.width -= 1;
        assert!(instance_range(&n, 0, 0, 1).is_none());
        let mut n = native(2, 0, 1);
        n.stride = 64;
        n.capacity = 0x8000;
        n.width = 0x200000;
        assert_eq!(
            instance_range(&n, 1, 1, 1).map(|r| (r.byte_offset, r.bytes)),
            Some((64, 64))
        );
    }
    #[test]
    fn bounded_retention() {
        {
            let mut ledger = Ledger::new(Limits {
                live_cpu_bytes: 96,
                ..Limits::default()
            });
            let n = native(2, 0, 1);
            let writer = write(&mut ledger, &n, 0, 2, 1, 0x3A);
            let (status, retained) = ledger.query(&n, &read(writer, 0, 2, 0));
            assert_eq!(status, Status::Completed);
            ledger.reset();
            assert_eq!(
                ledger.live_cpu_bytes(),
                96,
                "a reset hid externally retained payload bytes"
            );
            let (status, ticket) = ledger.begin(&n, &scope(2));
            assert_eq!(status, Status::Observed);
            assert_eq!(
                ledger.complete(&n, &completed(ticket, 0, 0, 1), &[0u8; 48]),
                Status::Capacity
            );
            drop(retained);
            assert_eq!(ledger.live_cpu_bytes(), 0);
        }
        {
            let mut ledger = Ledger::new(Limits {
                rings: 1,
                pending_writers: 1,
                ..Limits::default()
            });
            let mut n = native(2, 0, 1);
            assert_eq!(ledger.begin(&n, &scope(1)).0, Status::Observed);
            let (status, second) = ledger.begin(&n, &scope(2));
            assert!(status == Status::Capacity && second.id == 0);
            ledger.reset();
            assert_eq!(ledger.observe(&n), Status::Observed);
            n.ring += 0x100;
            assert_eq!(ledger.observe(&n), Status::Capacity);
        }
        {
            let mut ledger = Ledger::new(Limits {
                publications: 1,
                ..Limits::default()
            });
            let n = native(2, 0, 1);
            write(&mut ledger, &n, 0, 1, 1, 0x3A);
            assert_eq!(
                ledger.publish(
                    &n,
                    &Publication {
                        scope: scope(2),
                        head: 2
                    }
                ),
                Status::Published
            );
            let mut next = scope(3);
            next.stamp += 1;
            assert_eq!(
                ledger.publish(
                    &n,
                    &Publication {
                        scope: next,
                        head: 2
                    }
                ),
                Status::Capacity
            );
        }
        {
            let mut ledger = Ledger::new(Limits {
                ranges: 1,
                ..Limits::default()
            });
            let n = native(2, 0, 1);
            let old = write(&mut ledger, &n, 0, 1, 1, 0x3A);
            let presence = ledger.completed_presence(&old);
            let (status, retained) = ledger.query(&n, &read(old, 0, 1, 0));
            assert!(presence.upgrade().is_some() && status == Status::Completed);
            let (_, ticket) = ledger.begin(&n, &scope(2));
            assert_eq!(
                ledger.complete(&n, &completed(ticket, 1, 1, 1), &[0u8; 48]),
                Status::Completed
            );
            assert!(
                presence.upgrade().is_none()
                    && ledger.completed_presence(&ticket).upgrade().is_some()
            );
            assert_eq!(
                ledger.query(&n, &read(old, 0, 1, 0)).0,
                Status::UnknownWriter
            );
            assert!(retained.bytes()[0] == 0x3A && ledger.live_cpu_bytes() == 96);
            let (status, current) = ledger.query(&n, &read(ticket, 1, 1, 1));
            assert!(
                status == Status::Completed && current.bytes()[0] == 0,
                "cache saturation quarantined the ring"
            );
        }
        {
            let limits = Limits {
                ranges: 8,
                ..Limits::default()
            };
            let mut ledger = Ledger::new(limits);
            let mut inventory: Vec<Weak<WriterTicket>> = Vec::new();
            let mut oldest = WriterTicket::default();
            for index in 0..10000u32 {
                let n = native(index as u64 + 1, 0, 1);
                let last = write(
                    &mut ledger,
                    &n,
                    index as u64,
                    1,
                    index as u64 + 1,
                    index as u8,
                );
                if index == 0 {
                    oldest = last;
                }
                inventory.retain(|w| w.upgrade().is_some());
                inventory.push(ledger.completed_presence(&last));
                assert!(
                    inventory.len() <= limits.ranges
                        && ledger.live_cpu_bytes() <= limits.ranges * 48
                );
                let (status, candidate) = ledger.query(&n, &read(last, index as u64, 1, index));
                assert!(status == Status::Completed && candidate.bytes()[0] == index as u8);
            }
            assert_eq!(
                ledger.query(&native(10000, 0, 1), &read(oldest, 0, 1, 0)).0,
                Status::UnknownWriter
            );
            assert_eq!(ledger.observe(&native(10000, 10000, 1)), Status::Observed);
            assert!(
                inventory.iter().all(|w| w.upgrade().is_none()) && ledger.live_cpu_bytes() == 0
            );
        }
        {
            let mut ledger = Ledger::new(Limits {
                live_cpu_bytes: 96,
                ..Limits::default()
            });
            let n = native(4, 0, 1);
            let old = write(&mut ledger, &n, 0, 2, 1, 0x3A);
            let (status, held) = ledger.query(&n, &read(old, 0, 2, 0));
            assert_eq!(status, Status::Completed);
            let presence = ledger.completed_presence(&old);
            let (status, returned) = ledger.begin(&n, &scope(2));
            assert_eq!(status, Status::Observed);
            assert_eq!(
                ledger.complete(&n, &completed(returned, 2, 2, 1), &[0u8; 48]),
                Status::Capacity
            );
            assert!(presence.upgrade().is_none() && held.bytes()[0] == 0x3A);
            assert_eq!(
                ledger.query(&n, &read(returned, 2, 1, 2)).0,
                Status::UnknownWriter
            );
            drop(held);
            let fresh = write(&mut ledger, &n, 3, 1, 3, 0x3A);
            let (status, current) = ledger.query(&n, &read(fresh, 3, 1, 3));
            assert_eq!(
                status,
                Status::Completed,
                "an external release did not resume without reset"
            );
            let fresh_presence = ledger.completed_presence(&fresh);
            ledger.unknown(n.ring);
            assert!(fresh_presence.upgrade().is_none() && current.bytes().len() == 48);
        }
        {
            let mut ledger = Ledger::new(Limits {
                ranges: 1,
                ..Limits::default()
            });
            let n = native(4, 0, 1);
            write(&mut ledger, &n, 0, 1, 1, 0x3A);
            let (status, pending) = ledger.begin(&n, &scope(2));
            assert!(
                status == Status::Observed
                    && ledger.qualify_write(&n, &pending, 3, 3, 1) == Status::Observed
            );
            let (status, returned) = ledger.begin(&n, &scope(3));
            assert!(
                status == Status::Observed
                    && ledger.qualify_write(&n, &returned, 2, 2, 1) == Status::Observed
            );
            assert_eq!(
                ledger.complete(&n, &completed(returned, 2, 2, 1), &[0u8; 48]),
                Status::Completed
            );
            assert!(ledger.validate_write(&pending) == Status::Observed);
            assert!(ledger.completed_presence(&pending).upgrade().is_none());
            let (status, candidate) = ledger.query(&n, &read(pending, 0, 1, 0));
            assert!(status == Status::Pending && !candidate.is_some());
            assert_eq!(
                ledger.complete(&n, &completed(pending, 3, 3, 1), &[0u8; 48]),
                Status::Completed
            );
            let presence = ledger.completed_presence(&pending);
            ledger.reset();
            assert!(
                presence.upgrade().is_none(),
                "a reset kept the completed membership"
            );
        }
    }

    // The concurrent cases: real threads, a metadata mutex around the ledger, native phases outside it.
    struct Latch {
        count: Mutex<usize>,
        zero: Condvar,
    }
    impl Latch {
        fn new(count: usize) -> Self {
            Latch {
                count: Mutex::new(count),
                zero: Condvar::new(),
            }
        }
        fn count_down(&self) {
            let mut count = self.count.lock().expect("the latch");
            *count -= 1;
            if *count == 0 {
                self.zero.notify_all();
            }
        }
        fn wait(&self) {
            let mut count = self.count.lock().expect("the latch");
            while *count != 0 {
                count = self.zero.wait(count).expect("the latch");
            }
        }
    }
    struct Fixture {
        ledger: Mutex<Ledger>,
        head: AtomicU64,
        mapped: Mutex<Vec<u8>>,
    }
    impl Fixture {
        fn new(initial: u64, limits: Limits) -> Self {
            Fixture {
                ledger: Mutex::new(Ledger::new(limits)),
                head: AtomicU64::new(initial),
                mapped: Mutex::new(vec![0; 6 << 20]),
            }
        }
        fn native(&self) -> RingObservation {
            native(self.head.load(Ordering::Acquire), 0, 1)
        }
        fn meta<R>(&self, f: impl FnOnce(&mut Ledger, &RingObservation) -> R) -> R {
            let mut ledger = self.ledger.lock().expect("the metadata");
            let n = self.native();
            f(&mut ledger, &n)
        }
        fn native_copy(&self, first: u64, count: u32, value: u8) {
            let at = first as usize * 48;
            self.mapped.lock().expect("the mapping")[at..at + count as usize * 48].fill(value);
        }
        fn sample(&self, first: u64, bytes: usize) -> Vec<u8> {
            let at = first as usize * 48;
            self.mapped.lock().expect("the mapping")[at..at + bytes].to_vec()
        }
    }
    fn thread_scope(occurrence: u64) -> Scope {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::thread::current().id().hash(&mut hasher);
        Scope {
            thread: hasher.finish().max(1),
            ..scope(occurrence)
        }
    }
    fn thread_read(ticket: WriterTicket, absolute: u64, count: u32) -> ReadRequest {
        ReadRequest {
            writer: ticket,
            source: thread_scope(100 + ticket.id),
            absolute_begin: absolute,
            first_instance: (absolute % 0x20000) as u32,
            instances: count,
        }
    }
    fn filled(bytes: &[u8], value: u8) -> bool {
        !bytes.is_empty() && bytes.iter().all(|&b| b == value)
    }
    fn seed(f: &Fixture, absolute: u64, count: u32, occurrence: u64, value: u8) -> WriterTicket {
        let (status, ticket) = f.meta(|l, n| l.begin(n, &thread_scope(occurrence)));
        assert_eq!(status, Status::Observed);
        let first = (absolute % 0x20000) as u32;
        assert_eq!(
            f.meta(|l, n| l.qualify_write(n, &ticket, absolute, first, count)),
            Status::Observed
        );
        f.native_copy(first as u64, count, value);
        let bytes = f.sample(first as u64, count as usize * 48);
        assert_eq!(
            f.meta(|l, n| l.complete(n, &completed(ticket, absolute, first, count), &bytes)),
            Status::Completed
        );
        ticket
    }

    #[test]
    fn reversed_disjoint_callbacks() {
        let f = Fixture::new(0, Limits::default());
        let (entered, qualified, outside_native) =
            (Barrier::new(2), Barrier::new(2), Barrier::new(2));
        let (first_cas, second_event, second_complete) =
            (Latch::new(1), Latch::new(1), Latch::new(1));
        let (native_active, peak_native) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let worker = |index: u64| {
            let mut results = [Status::Exception; 6];
            let (status, ticket) = f.meta(|l, n| l.begin(n, &thread_scope(index + 1)));
            results[0] = status;
            entered.wait();
            if index == 1 {
                first_cas.wait();
            }
            let (old, next) = (index * 2, index * 2 + 2);
            let cas = f
                .head
                .compare_exchange(old, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
            if index == 0 {
                first_cas.count_down();
                second_event.wait();
            }
            results[1] = f.meta(|l, n| l.reuse(n, old, 2, Some(&ticket)));
            results[2] = f.meta(|l, n| l.qualify_write(n, &ticket, old, old as u32, 2));
            if index == 1 {
                second_event.count_down();
            }
            qualified.wait();
            let active = native_active.fetch_add(1, Ordering::AcqRel) + 1;
            peak_native.fetch_max(active, Ordering::AcqRel);
            // No metadata lock spans this simulated native copy/sampling phase.
            outside_native.wait();
            f.native_copy(old, 2, 0x31 + index as u8);
            let bytes = f.sample(old, 96);
            native_active.fetch_sub(1, Ordering::AcqRel);
            if index == 0 {
                second_complete.wait();
            }
            results[3] = f.meta(|l, _| l.validate_write(&ticket));
            results[4] =
                f.meta(|l, n| l.complete(n, &completed(ticket, old, old as u32, 2), &bytes));
            let (status, candidate) = f.meta(|l, n| l.query(n, &thread_read(ticket, old, 2)));
            results[5] = status;
            if index == 1 {
                second_complete.count_down();
            }
            (std::thread::current().id(), cas, ticket, results, candidate)
        };
        let (a, b) = std::thread::scope(|s| {
            let a = s.spawn(|| worker(0));
            let b = s.spawn(|| worker(1));
            (a.join().expect("worker 0"), b.join().expect("worker 1"))
        });
        assert!(
            a.0 != b.0 && a.1 && b.1,
            "the real worker/CAS reservation order differs"
        );
        assert_eq!(
            peak_native.load(Ordering::Acquire),
            2,
            "the native phases were not simultaneously active"
        );
        for (index, (_, _, ticket, results, candidate)) in [a, b].into_iter().enumerate() {
            assert!(
                results[..4].iter().all(|&s| s == Status::Observed),
                "worker {index}: {results:?}"
            );
            assert!(
                results[4] == Status::Completed && results[5] == Status::Completed,
                "worker {index}: {results:?}"
            );
            assert!(
                candidate.producer().writer == ticket
                    && filled(candidate.bytes(), 0x31 + index as u8)
            );
        }
        assert_eq!(f.ledger.lock().expect("the ledger").live_cpu_bytes(), 192);
    }
    #[test]
    fn cached_overwrite_and_unknown_pending() {
        let f = Fixture::new(128, Limits::default());
        let old_overlap = seed(&f, 0, 2, 1, 0x11);
        let old_disjoint = seed(&f, 4, 2, 2, 0x22);
        let (status, retained) = f.meta(|l, n| l.query(n, &thread_read(old_overlap, 0, 2)));
        assert_eq!(status, Status::Completed);
        let (unknown_started, unknown_tested, fresh_qualified) =
            (Latch::new(1), Latch::new(1), Latch::new(1));
        let copies = Barrier::new(2);
        let (cached, fresh) = std::thread::scope(|s| {
            let cached = s.spawn(|| {
                let mut results = [Status::Exception; 5];
                let (status, ticket) = f.meta(|l, n| l.begin(n, &thread_scope(3)));
                results[0] = status;
                unknown_started.wait();
                let blocked = f
                    .meta(|l, n| l.query(n, &thread_read(old_disjoint, 4, 2)))
                    .0;
                results[1] = f.meta(|l, n| l.qualify_write(n, &ticket, 0, 0, 2));
                let overwritten = f.meta(|l, n| l.query(n, &thread_read(old_overlap, 0, 2))).0;
                unknown_tested.count_down();
                fresh_qualified.wait();
                let survived = f
                    .meta(|l, n| l.query(n, &thread_read(old_disjoint, 4, 2)))
                    .0;
                copies.wait();
                f.native_copy(0, 2, 0x33);
                let bytes = f.sample(0, 96);
                results[2] = f.meta(|l, _| l.validate_write(&ticket));
                results[3] = f.meta(|l, n| l.complete(n, &completed(ticket, 0, 0, 2), &bytes));
                let (status, current) = f.meta(|l, n| l.query(n, &thread_read(ticket, 0, 2)));
                results[4] = status;
                (results, blocked, overwritten, survived, current)
            });
            let fresh = s.spawn(|| {
                let mut results = [Status::Exception; 6];
                let (status, ticket) = f.meta(|l, n| l.begin(n, &thread_scope(4)));
                results[0] = status;
                unknown_started.count_down();
                unknown_tested.wait();
                let cas = f
                    .head
                    .compare_exchange(128, 130, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok();
                results[1] = f.meta(|l, n| l.reuse(n, 128, 2, Some(&ticket)));
                results[2] = f.meta(|l, n| l.qualify_write(n, &ticket, 128, 128, 2));
                fresh_qualified.count_down();
                copies.wait();
                f.native_copy(128, 2, 0x44);
                let bytes = f.sample(128, 96);
                results[3] = f.meta(|l, _| l.validate_write(&ticket));
                results[4] = f.meta(|l, n| l.complete(n, &completed(ticket, 128, 128, 2), &bytes));
                let (status, current) = f.meta(|l, n| l.query(n, &thread_read(ticket, 128, 2)));
                results[5] = status;
                (results, cas, current)
            });
            (cached.join().expect("cached"), fresh.join().expect("fresh"))
        });
        let (results, blocked, overwritten, survived, current0) = cached;
        let (fresh_results, cas, current1) = fresh;
        assert!(cas, "the fresh disjoint native reservation failed");
        assert_eq!(
            blocked,
            Status::Pending,
            "an unknown pending range did not block a provisional query"
        );
        assert_eq!(
            overwritten,
            Status::UnknownWriter,
            "a cached qualification kept the old overlap"
        );
        assert_eq!(
            survived,
            Status::Completed,
            "a disjoint fresh reservation quarantined the prior provenance"
        );
        assert!(
            results[..3].iter().all(|&s| s == Status::Observed),
            "{results:?}"
        );
        assert!(
            results[3] == Status::Completed && results[4] == Status::Completed,
            "{results:?}"
        );
        assert!(
            fresh_results[..4].iter().all(|&s| s == Status::Observed),
            "{fresh_results:?}"
        );
        assert!(
            fresh_results[4] == Status::Completed && fresh_results[5] == Status::Completed,
            "{fresh_results:?}"
        );
        assert!(
            filled(retained.bytes(), 0x11)
                && filled(current0.bytes(), 0x33)
                && filled(current1.bytes(), 0x44)
        );
    }
    #[test]
    fn equal_samples_cannot_hide_overlapping_aba() {
        let f = Fixture::new(128, Limits::default());
        let (first_sampled, restored) = (Latch::new(1), Latch::new(1));
        let (first, second) = std::thread::scope(|s| {
            let first = s.spawn(|| {
                let mut results = [Status::Exception; 4];
                let (status, ticket) = f.meta(|l, n| l.begin(n, &thread_scope(1)));
                results[0] = status;
                results[1] = f.meta(|l, n| l.qualify_write(n, &ticket, 10, 10, 1));
                f.native_copy(10, 1, 0x55);
                let before = f.sample(10, 48);
                first_sampled.count_down();
                restored.wait();
                let after = f.sample(10, 48);
                results[2] = f.meta(|l, _| l.validate_write(&ticket));
                results[3] = f.meta(|l, n| l.complete(n, &completed(ticket, 10, 10, 1), &after));
                (results, before, after)
            });
            let second = s.spawn(|| {
                let mut results = [Status::Exception; 4];
                first_sampled.wait();
                let (status, ticket) = f.meta(|l, n| l.begin(n, &thread_scope(2)));
                results[0] = status;
                results[1] = f.meta(|l, n| l.qualify_write(n, &ticket, 10, 10, 1));
                f.native_copy(10, 1, 0x99);
                f.native_copy(10, 1, 0x55);
                let bytes = f.sample(10, 48);
                restored.count_down();
                results[2] = f.meta(|l, _| l.validate_write(&ticket));
                results[3] = f.meta(|l, n| l.complete(n, &completed(ticket, 10, 10, 1), &bytes));
                results
            });
            (first.join().expect("first"), second.join().expect("second"))
        });
        let (results, before, after) = first;
        assert!(
            before == after && filled(&before, 0x55),
            "the ABA timeline did not restore identical samples"
        );
        assert!(results[0] == Status::Observed && results[1] == Status::Observed);
        assert!(
            second[0] == Status::Observed && second[1] == Status::Overlap,
            "the overlapping range was not marked"
        );
        for r in [results, second] {
            assert!(
                r[2] == Status::Overlap && r[3] == Status::Pending,
                "equal samples legitimized a conflict: {r:?}"
            );
        }
        assert_eq!(f.ledger.lock().expect("the ledger").live_cpu_bytes(), 0);
    }
    #[test]
    fn same_generation_recovery_and_retained_budget() {
        let f = Fixture::new(
            128,
            Limits {
                live_cpu_bytes: 96,
                ..Limits::default()
            },
        );
        let ready = Latch::new(1);
        let retained: Mutex<Option<CpuCandidate>> = Mutex::new(None);
        let (producer, recovering) = std::thread::scope(|s| {
            let producer = s.spawn(|| {
                let mut results = [Status::Exception; 4];
                let (status, old) = f.meta(|l, n| l.begin(n, &thread_scope(1)));
                results[0] = status;
                results[1] = f.meta(|l, n| l.qualify_write(n, &old, 0, 0, 2));
                f.native_copy(0, 2, 0x71);
                let bytes = f.sample(0, 96);
                results[2] = f.meta(|l, n| l.complete(n, &completed(old, 0, 0, 2), &bytes));
                let (status, candidate) = f.meta(|l, n| l.query(n, &thread_read(old, 0, 2)));
                results[3] = status;
                *retained.lock().expect("the retained slot") = Some(candidate);
                ready.count_down();
                (results, old)
            });
            let recovering = s.spawn(|| {
                let mut results = [Status::Exception; 11];
                ready.wait();
                let old = retained
                    .lock()
                    .expect("the retained slot")
                    .as_ref()
                    .expect("retained")
                    .producer()
                    .writer;
                f.meta(|l, n| l.unknown(n.ring));
                results[4] = f.meta(|l, n| l.restart_observation(n));
                results[5] = f.meta(|l, n| l.query(n, &thread_read(old, 0, 2))).0;
                let late = f.sample(0, 96);
                results[9] = f.meta(|l, n| l.complete(n, &completed(old, 0, 0, 2), &late));
                results[10] = f.meta(|l, n| l.restart_observation(n));
                let held = f.meta(|l, _| l.live_cpu_bytes());
                let (begin, limited) = f.meta(|l, n| l.begin(n, &thread_scope(2)));
                let qualify = f.meta(|l, n| l.qualify_write(n, &limited, 4, 4, 1));
                f.native_copy(4, 1, 0x72);
                let bytes = f.sample(4, 48);
                results[6] = f.meta(|l, n| l.complete(n, &completed(limited, 4, 4, 1), &bytes));
                if begin != Status::Observed || qualify != Status::Observed {
                    results[6] = Status::Exception;
                }
                *retained.lock().expect("the retained slot") = None;
                let released = f.meta(|l, _| l.live_cpu_bytes());
                f.meta(|l, _| l.reset());
                let (status, fresh) = f.meta(|l, n| l.begin(n, &thread_scope(3)));
                results[7] = status;
                let qualified = f.meta(|l, n| l.qualify_write(n, &fresh, 4, 4, 1));
                results[8] = f.meta(|l, n| l.complete(n, &completed(fresh, 4, 4, 1), &bytes));
                if qualified != Status::Observed {
                    results[8] = Status::Exception;
                }
                (results, held, released, fresh)
            });
            (
                producer.join().expect("producer"),
                recovering.join().expect("recovering"),
            )
        });
        let (seeded, old) = producer;
        let (results, held, released, fresh) = recovering;
        assert!(seeded[0] == Status::Observed && seeded[1] == Status::Observed);
        assert!(seeded[2] == Status::Completed && seeded[3] == Status::Completed);
        assert_eq!(
            results[4],
            Status::Observed,
            "a fresh local observation could not recover the same generation"
        );
        assert_eq!(
            results[5],
            Status::UnknownWriter,
            "a same-generation local recovery revived an old ticket"
        );
        assert_eq!(
            results[9],
            Status::UnknownWriter,
            "a late old completion attached to the local recovery"
        );
        assert_eq!(results[10], Status::Observed);
        assert_eq!(
            results[6],
            Status::Capacity,
            "the local recovery hid externally retained CPU bytes"
        );
        assert!(results[7] == Status::Observed && results[8] == Status::Completed);
        assert!(held == 96 && released == 0 && fresh.ring_generation == old.ring_generation);
        assert!(fresh.observer_epoch != old.observer_epoch && fresh.id != old.id);
        assert_eq!(
            f.meta(|l, n| l.query(n, &thread_read(old, 0, 2))).0,
            Status::UnknownWriter
        );
        let (status, current) = f.meta(|l, n| l.query(n, &thread_read(fresh, 4, 1)));
        assert!(status == Status::Completed && filled(current.bytes(), 0x72));
    }
    #[test]
    fn returned_writer_before_peer_reservation() {
        let f = Fixture::new(128, Limits::default());
        let (status, first) = f.meta(|l, n| l.begin(n, &thread_scope(1)));
        assert_eq!(status, Status::Observed);
        assert_eq!(
            f.meta(|l, n| l.qualify_write(n, &first, 0, 0, 1)),
            Status::Observed
        );
        let (status, peer) = f.meta(|l, n| l.begin(n, &thread_scope(2)));
        assert_eq!(status, Status::Observed);
        assert_eq!(
            f.meta(|l, n| l.publish(
                n,
                &Publication {
                    scope: thread_scope(77),
                    head: n.head
                }
            )),
            Status::Pending
        );
        f.native_copy(0, 1, 0x31);
        assert_eq!(
            f.meta(|l, _| l.validate_write(&first)),
            Status::Pending,
            "an unknown peer must prevent sampling"
        );
        assert_eq!(f.meta(|l, _| l.discard_completed(&first)), Status::Observed);
        assert_eq!(
            f.meta(|l, n| l.reuse(n, 126, 2, Some(&peer))),
            Status::Observed
        );
        assert_eq!(
            f.meta(|l, n| l.qualify_write(n, &peer, 126, 126, 1)),
            Status::Observed
        );
        f.native_copy(126, 1, 0x72);
        let bytes = f.sample(126, 48);
        assert_eq!(
            f.meta(|l, n| l.complete(n, &completed(peer, 126, 126, 1), &bytes)),
            Status::Completed
        );
        let (status, result) = f.meta(|l, n| l.query(n, &thread_read(peer, 126, 1)));
        assert!(status == Status::Completed && filled(result.bytes(), 0x72));
        assert_eq!(f.meta(|l, _| l.discard_completed(&peer)), Status::Pending);
        assert_eq!(
            f.meta(|l, n| l.query(n, &thread_read(first, 0, 1))).0,
            Status::UnknownWriter
        );
    }
    #[test]
    fn unobserved_cached_writer_keeps_disjoint_versions() {
        let f = Fixture::new(128, Limits::default());
        let (_, saved) = f.meta(|l, n| l.begin(n, &thread_scope(1)));
        assert_eq!(
            f.meta(|l, n| l.qualify_write(n, &saved, 0, 0, 1)),
            Status::Observed
        );
        f.native_copy(0, 1, 0x23);
        let bytes = f.sample(0, 48);
        assert_eq!(
            f.meta(|l, n| l.complete(n, &completed(saved, 0, 0, 1), &bytes)),
            Status::Completed
        );
        let (status, unknown) = f.meta(|l, n| l.begin(n, &thread_scope(2)));
        assert_eq!(status, Status::Observed);
        assert_eq!(
            f.meta(|l, n| l.invalidate_physical(n, 4, 1)),
            Status::Observed
        );
        assert_eq!(
            f.meta(|l, n| l.query(n, &thread_read(saved, 0, 1))).0,
            Status::Pending
        );
        let (status, overlap) = f.meta(|l, n| l.begin(n, &thread_scope(3)));
        assert_eq!(status, Status::Observed);
        assert_eq!(
            f.meta(|l, n| l.qualify_write(n, &overlap, 4, 4, 1)),
            Status::Observed
        );
        f.native_copy(4, 1, 0x31);
        assert_eq!(
            f.meta(|l, n| l.finish_unobserved(n, &unknown, 4, 1)),
            Status::Observed
        );
        assert_eq!(f.meta(|l, _| l.validate_write(&overlap)), Status::Overlap);
        assert_eq!(
            f.meta(|l, _| l.discard_completed(&overlap)),
            Status::Observed
        );
        let (status, result) = f.meta(|l, n| l.query(n, &thread_read(saved, 0, 1)));
        assert!(status == Status::Completed && filled(result.bytes(), 0x23));
        assert_eq!(
            f.meta(|l, n| l.query(n, &thread_read(unknown, 4, 1))).0,
            Status::UnknownWriter
        );
    }
}
