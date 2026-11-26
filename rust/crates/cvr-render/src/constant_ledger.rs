//! The native constant capture's ledger (src/Render/NativeConstantCapture.cpp): the frame's draws, their CBV bindings,
//! the read receipts, the interned payloads and their blob, the lists still pending submission, the read positions and
//! source lists, and the deferred bindings.
//!
//! - `Begin` clears it (the null payload first).
//! - `Snapshot` records a draw's CBVs: each the C++ half located in the family's paired plan and resolved at the draw
//!   (unknown: 2), the shader-declared bytes read twice through the reader with one receipt (small: 4, unreadable or
//!   unstable: 8), hashed (FNV-1a) and interned within 64 MiB and 65536 payloads; the draw owns all its CBVs (flag 1)
//!   only when every one copied; the draw pends on its list.
//! - `BeforeSubmit` -- each submitted list's draws: submitted (flag 2); each copied or deferrable binding's descriptor
//!   still the same (else 16); a binding with a read position and a receipt (or an unreadable one) deferred (128);
//!   the rest validated through its receipt or re-read (else 32); all validated: flag 4.
//! - `Reset` abandons a list's pending draws (64).
//! - `Poll` resolves the deferred bindings through their positions (ready: interned, reserved 7; settled otherwise by
//!   constant_capture_checks' rule) and completes the draws (flags 7).
//! - `CopyConstantPair`'s draws and payloads.
//!
//! In the Rust build (`CVR_RUST_VRIK`) constant_capture_state.rs owns the ledger (the .cpp keeps the families, the
//! descriptor lookups and the bank pass); the unit tests below cover each step, and
//! tools/single_pass_tests/constant_capture_test runs against it in its Rust variant (constant_capture_rust.cpp).

use cvr_host::fast::FastMap;
use core::ffi::c_void;

use crate::constant_capture_checks::{deferred, pair_admitted, status, Draw};

pub const BLOB_LIMIT: usize = 64 * 1024 * 1024;
pub const BINDING_LIMIT: usize = 65536;

/// NativeConstantCapture.hpp's Binding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Binding {
    pub descriptor: u64,
    pub heap: u64,
    pub address: u64,
    pub hash: u64,
    pub draw: u32,
    pub parameter: u32,
    pub offset: u32,
    pub visibility: u32,
    pub shader_register: u32,
    pub bytes: u32,
    pub expected_bytes: u32,
    pub payload: u32,
    pub status: u32,
    pub reserved: u32,
}
/// NativeConstantCapture.hpp's Payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    pub offset: u32,
    pub bytes: u32,
    pub hash: u64,
}
/// A snapshotted draw.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SnapshotDraw {
    pub family: u64,
    pub list: u64,
    /// The registered read position (0: none).
    pub position: u64,
    pub sequence: u32,
    pub side: u32,
    pub frame: u32,
    pub _pad: u32,
}
/// One CBV use of the family, as the C++ half found it: `state` 0 not in the paired plan's tables, 1 located but not
/// resolved at the draw, 2 resolved (descriptor, heap, address and bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CbvUse {
    pub descriptor: u64,
    pub heap: u64,
    pub address: u64,
    pub bytes: u32,
    pub parameter: u32,
    pub offset: u32,
    pub visibility: u32,
    pub shader_register: u32,
    pub expected: u32,
    pub state: u32,
    pub _pad: u32,
}
const _: () = assert!(
    core::mem::size_of::<Binding>() == 72
        && core::mem::size_of::<Payload>() == 16
        && core::mem::size_of::<SnapshotDraw>() == 40
        && core::mem::size_of::<CbvUse>() == 56
);
/// The reader (ReadBytes), the validator (ValidateBytes) and the descriptor's current CBV.
pub type Read = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize, *mut u64) -> bool;
pub type Validate = unsafe extern "C-unwind" fn(u64, *mut c_void, u64, *const c_void, usize) -> bool;
/// The descriptor's CBV now: (heap, address, bytes) into the outputs.
pub type Resolve = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut u64, *mut u64, *mut u32) -> bool;
/// A registered read of a binding at its position.
pub type Register = unsafe extern "C-unwind" fn(*mut c_void, u64, u32, u32, u64, u32);
/// A deferred binding's resolution at its position: the timeline's status, its detail into the output.
pub type ResolvePosition = unsafe extern "C-unwind" fn(*mut c_void, u64, u64, u64, *mut c_void, usize, *mut u32) -> u32;
/// The C++ half's callbacks and readers.
#[repr(C)]
pub struct Host {
    pub user: *mut c_void,
    pub read: Option<Read>,
    pub validate: Option<Validate>,
    pub resolve: Resolve,
    pub register: Register,
    pub resolve_position: Option<ResolvePosition>,
}
/// What a step asks of the C++ half: bit 0 the capture failed (failure 2, inactive), bit 1 an intern refused (failure 1).
pub const THREW: u32 = 1;
pub const INTERN_REFUSED: u32 = 2;

/// FNV-1a 64.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(1099511628211))
}
/// The ledger.
#[derive(Default)]
pub struct Ledger {
    pub draws: Vec<Draw>,
    pub bindings: Vec<Binding>,
    pub receipts: Vec<u64>,
    pub payloads: Vec<Payload>,
    pub blob: Vec<u8>,
    interned: FastMap<Vec<u8>, u32>,
    pending: FastMap<u64, Vec<u32>>,
    positions: Vec<u64>,
    lists: Vec<u64>,
    pub deferred: u32,
}
impl Ledger {
    pub fn new() -> Ledger {
        let mut ledger = Ledger::default();
        ledger.begin();
        ledger
    }
    /// Begin.
    pub fn begin(&mut self) {
        self.draws.clear();
        self.bindings.clear();
        self.receipts.clear();
        self.payloads.clear();
        self.payloads.push(Payload::default());
        self.blob.clear();
        self.interned.clear();
        self.pending.clear();
        self.positions.clear();
        self.lists.clear();
        self.deferred = 0;
    }
    pub fn pending_lists(&self) -> usize {
        self.pending.len()
    }
    pub fn has_pending(&self, list: u64) -> bool {
        self.pending.contains_key(&list)
    }
    /// A failed submission's catch: no list pends any more.
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }
    /// Intern (0 and the refusal when the blob or the payloads are full).
    fn intern(&mut self, bytes: Vec<u8>, hash: u64, asks: &mut u32) -> u32 {
        if let Some(&id) = self.interned.get(&bytes) {
            return id;
        }
        if bytes.len() > BLOB_LIMIT - self.blob.len() || self.payloads.len() >= BINDING_LIMIT {
            *asks |= INTERN_REFUSED;
            return 0;
        }
        let id = self.payloads.len() as u32;
        self.payloads.push(Payload { offset: self.blob.len() as u32, bytes: bytes.len() as u32, hash });
        self.blob.extend_from_slice(&bytes);
        self.interned.insert(bytes, id);
        id
    }
    /// Snapshot (the draw pends on its list unless the capture failed).
    ///
    /// # Safety
    /// The host's callbacks are the C++ half's; the list is the draw's.
    pub unsafe fn snapshot(&mut self, d: &SnapshotDraw, uses: &[CbvUse], host: &Host, capacity: usize) -> u32 {
        let mut asks = 0;
        if self.draws.len() >= capacity {
            return THREW;
        }
        let mut draw = Draw { family: d.family, sequence: d.sequence, first: self.bindings.len() as u32, count: 0, flags: 1, side: d.side, frame: d.frame };
        for u in uses {
            if self.bindings.len() >= BINDING_LIMIT {
                return asks | THREW;
            }
            let mut b = Binding { draw: d.sequence, visibility: u.visibility, shader_register: u.shader_register, expected_bytes: u.expected, reserved: 1, ..Binding::default() };
            let mut receipt = 0u64;
            if u.state == 0 {
                b.status = 2;
            } else {
                b.parameter = u.parameter;
                b.offset = u.offset;
                if u.state != 2 {
                    b.status = 2;
                } else {
                    b.descriptor = u.descriptor;
                    b.heap = u.heap;
                    b.address = u.address;
                    b.bytes = u.bytes;
                    if d.position != 0 {
                        // SAFETY: the C++ half's callback.
                        unsafe { (host.register)(host.user, d.position, u.visibility, u.shader_register, u.address, u.expected) };
                    }
                    if u.expected == 0 || u.bytes < u.expected {
                        b.status = 4;
                    } else {
                        // CBV views round up to 256 bytes; the native allocation need only supply the shader-declared
                        // logical payload.
                        let (mut bytes, mut check, mut repeated) = (vec![0u8; u.expected as usize], vec![0u8; u.expected as usize], 0u64);
                        let list = d.list as *mut c_void;
                        // SAFETY: the reader writes the buffers' bytes.
                        let read = host.read.is_some_and(|read| unsafe {
                            read(list, u.address, bytes.as_mut_ptr().cast(), bytes.len(), &mut receipt)
                                && read(list, u.address, check.as_mut_ptr().cast(), check.len(), &mut repeated)
                        });
                        if !read || receipt != repeated || bytes != check {
                            b.status = 8;
                        } else {
                            b.hash = fnv(&bytes);
                            b.payload = self.intern(bytes, b.hash, &mut asks);
                            b.status = if b.payload != 0 { 1 } else { 8 };
                            if receipt != 0 {
                                b.reserved |= 2;
                            }
                        }
                    }
                }
            }
            if b.status != 1 {
                draw.flags = 0;
            }
            self.bindings.push(b);
            self.receipts.push(receipt);
            draw.count += 1;
        }
        self.pending.entry(d.list).or_default().push(self.draws.len() as u32);
        self.draws.push(draw);
        self.positions.push(d.position);
        self.lists.push(d.list);
        asks
    }
    /// BeforeSubmit.
    ///
    /// # Safety
    /// As snapshot; `lists` the submitted lists.
    pub unsafe fn before_submit(&mut self, lists: &[*mut c_void], host: &Host) {
        for &list in lists {
            let Some(indices) = self.pending.remove(&(list as u64)) else { continue };
            for index in indices {
                let index = index as usize;
                let draw = &mut self.draws[index];
                draw.flags |= 2;
                let mut stable = draw.flags & 1 != 0;
                let (first, count) = (draw.first as usize, draw.count as usize);
                for j in first..first + count {
                    let receipt = self.receipts[j];
                    let b = &mut self.bindings[j];
                    let deferred = host.resolve_position.is_some() && self.positions[index] != 0 && (b.status == 8 || (b.status == 1 && receipt != 0));
                    if b.status != 1 && !deferred {
                        stable = false;
                        continue;
                    }
                    let (mut heap, mut address, mut bytes) = (0u64, 0u64, 0u32);
                    // SAFETY: the C++ half's callback.
                    if !unsafe { (host.resolve)(host.user, b.descriptor, &mut heap, &mut address, &mut bytes) } || heap != b.heap || address != b.address || bytes != b.bytes {
                        b.status = 16;
                        stable = false;
                        continue;
                    }
                    if deferred {
                        b.status = 128;
                        self.deferred += 1;
                        stable = false;
                        continue;
                    }
                    let p = self.payloads[b.payload as usize];
                    let expected = &self.blob[p.offset as usize..(p.offset + p.bytes) as usize];
                    let valid = if receipt != 0 {
                        // SAFETY: the validator reads the expected bytes.
                        host.validate.is_some_and(|validate| unsafe { validate(receipt, list, b.address, expected.as_ptr().cast(), expected.len()) })
                    } else {
                        let (mut current, mut again) = (vec![0u8; p.bytes as usize], 0u64);
                        // SAFETY: the reader writes the buffer's bytes.
                        host.read.is_some_and(|read| unsafe { read(list, b.address, current.as_mut_ptr().cast(), current.len(), &mut again) })
                            && again == 0
                            && current == expected
                    };
                    if !valid {
                        b.status = 32;
                        stable = false;
                    }
                }
                if stable {
                    self.draws[index].flags |= 4;
                }
            }
        }
    }
    /// Reset (true when the list had pending draws).
    pub fn reset(&mut self, list: u64) -> bool {
        let Some(indices) = self.pending.remove(&list) else { return false };
        for index in indices {
            let draw = &mut self.draws[index as usize];
            draw.flags = 0;
            for j in draw.first as usize..(draw.first + draw.count) as usize {
                let b = &mut self.bindings[j];
                if b.status == 128 {
                    self.deferred -= 1;
                }
                b.status = 64;
            }
        }
        true
    }
    /// Poll's deferred resolution.
    ///
    /// # Safety
    /// As snapshot; the host has its position resolution.
    pub unsafe fn resolve_deferred(&mut self, host: &Host) -> u32 {
        let mut asks = 0;
        let Some(resolve) = host.resolve_position else { return asks };
        for i in 0..self.draws.len() {
            let (first, count) = (self.draws[i].first as usize, self.draws[i].count as usize);
            let mut complete = count != 0;
            for j in first..first + count {
                if self.bindings[j].status == 128 {
                    let b = self.bindings[j];
                    let mut bytes = vec![0u8; b.expected_bytes as usize];
                    let mut detail = 0u32;
                    // SAFETY: the C++ half's callback writes the buffer's bytes.
                    let result = unsafe { resolve(host.user, self.positions[i], self.lists[i], b.address, bytes.as_mut_ptr().cast(), bytes.len(), &mut detail) };
                    if result == status::READY {
                        let hash = fnv(&bytes);
                        let payload = self.intern(bytes, hash, &mut asks);
                        let b = &mut self.bindings[j];
                        b.hash = hash;
                        b.payload = payload;
                        b.status = if payload != 0 { 1 } else { 8 };
                        b.reserved = 7;
                        self.deferred -= 1;
                    } else if let Some((settled, reserved)) = deferred(result, detail) {
                        let b = &mut self.bindings[j];
                        b.status = settled;
                        b.reserved = reserved;
                        self.deferred -= 1;
                    }
                }
                complete &= self.bindings[j].status == 1;
            }
            if complete && self.draws[i].flags & 2 != 0 {
                self.draws[i].flags = 7;
            }
        }
        asks
    }
    /// CopyConstantPair's draws: the complete (flags 7) draws of the two sequences, a pair.
    pub fn pair(&self, left: u32, right: u32) -> Option<[usize; 2]> {
        let find = |sequence: u32| self.draws.iter().position(|d| d.sequence == sequence && d.flags == 7);
        let (a, b) = (find(left)?, find(right)?);
        pair_admitted(&self.draws[a], &self.draws[b]).then_some([a, b])
    }
    /// A draw's copied payload for a paired binding.
    pub fn payload(&self, draw: usize, parameter: u32, offset: u32, visibility: u32, shader_register: u32) -> Option<&[u8]> {
        let d = self.draws.get(draw)?;
        let found = self.bindings[d.first as usize..(d.first + d.count) as usize]
            .iter()
            .find(|v| v.status == 1 && v.parameter == parameter && v.offset == offset && v.visibility == visibility && v.shader_register == shader_register)?;
        if found.payload == 0 {
            return None;
        }
        let p = self.payloads[found.payload as usize];
        Some(&self.blob[p.offset as usize..(p.offset + p.bytes) as usize])
    }
}

#[cfg(test)]
mod tests {
    //! The ledger's steps: a snapshot's statuses, interning, the submission's validation and deferral, a reset, the
    //! deferred resolution and the pair.
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// The memory the reader sees by address, the registered reads, the descriptors' heap/address/bytes now.
        static MEMORY: RefCell<FastMap<u64, Vec<u8>>> = RefCell::new(cvr_host::fast::fast_map());
        static RECEIPTS: RefCell<FastMap<u64, u64>> = RefCell::new(cvr_host::fast::fast_map());
        static REGISTERED: RefCell<Vec<(u64, u32, u32, u64, u32)>> = const { RefCell::new(Vec::new()) };
        static DESCRIPTORS: RefCell<FastMap<u64, (u64, u64, u32)>> = RefCell::new(cvr_host::fast::fast_map());
        static TIMELINE: RefCell<FastMap<u64, (u32, u32, Vec<u8>)>> = RefCell::new(cvr_host::fast::fast_map());
        static VALIDATED: RefCell<bool> = const { RefCell::new(true) };
    }
    unsafe extern "C-unwind" fn read(_: *mut c_void, address: u64, out: *mut c_void, bytes: usize, receipt: *mut u64) -> bool {
        let Some(data) = MEMORY.with(|m| m.borrow().get(&address).cloned()) else { return false };
        if data.len() < bytes {
            return false;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), out.cast::<u8>(), bytes);
            *receipt = RECEIPTS.with(|r| r.borrow().get(&address).copied().unwrap_or(0));
        }
        true
    }
    unsafe extern "C-unwind" fn validate(_: u64, _: *mut c_void, _: u64, _: *const c_void, _: usize) -> bool {
        VALIDATED.with(|v| *v.borrow())
    }
    unsafe extern "C-unwind" fn resolve(_: *mut c_void, descriptor: u64, heap: *mut u64, address: *mut u64, bytes: *mut u32) -> bool {
        let Some((h, a, b)) = DESCRIPTORS.with(|d| d.borrow().get(&descriptor).copied()) else { return false };
        unsafe {
            *heap = h;
            *address = a;
            *bytes = b;
        }
        true
    }
    unsafe extern "C-unwind" fn register(_: *mut c_void, position: u64, visibility: u32, shader_register: u32, address: u64, bytes: u32) {
        REGISTERED.with(|r| r.borrow_mut().push((position, visibility, shader_register, address, bytes)));
    }
    unsafe extern "C-unwind" fn resolve_position(_: *mut c_void, _: u64, _: u64, address: u64, out: *mut c_void, bytes: usize, detail: *mut u32) -> u32 {
        let Some((result, d, data)) = TIMELINE.with(|t| t.borrow().get(&address).cloned()) else { return status::PENDING };
        unsafe {
            *detail = d;
            core::ptr::copy_nonoverlapping(data.as_ptr(), out.cast::<u8>(), bytes.min(data.len()));
        }
        result
    }
    fn host(positions: bool) -> Host {
        Host { user: core::ptr::null_mut(), read: Some(read), validate: Some(validate), resolve, register, resolve_position: positions.then_some(resolve_position) }
    }
    fn cbv(descriptor: u64, address: u64, register: u32, expected: u32) -> CbvUse {
        DESCRIPTORS.with(|d| d.borrow_mut().insert(descriptor, (0x900, address, 256)));
        CbvUse { descriptor, heap: 0x900, address, bytes: 256, parameter: 3, offset: register, visibility: 1, shader_register: register, expected, state: 2, _pad: 0 }
    }
    fn reset_world() {
        MEMORY.with(|m| m.borrow_mut().clear());
        RECEIPTS.with(|r| r.borrow_mut().clear());
        REGISTERED.with(|r| r.borrow_mut().clear());
        DESCRIPTORS.with(|d| d.borrow_mut().clear());
        TIMELINE.with(|t| t.borrow_mut().clear());
        VALIDATED.with(|v| *v.borrow_mut() = true);
    }
    fn draw(sequence: u32, side: u32, list: u64, position: u64) -> SnapshotDraw {
        SnapshotDraw { family: 77, list, position, sequence, side, frame: 9, _pad: 0 }
    }

    #[test]
    fn snapshot_statuses_and_interning() {
        reset_world();
        MEMORY.with(|m| {
            let mut m = m.borrow_mut();
            m.insert(0x1000, vec![7u8; 64]);
            m.insert(0x2000, vec![7u8; 64]);
        });
        let mut l = Ledger::new();
        let host = host(false);
        let mut unlocated = cbv(1, 0x1000, 0, 64);
        unlocated.state = 0;
        let mut unresolved = cbv(2, 0x1000, 1, 64);
        unresolved.state = 1;
        let small = CbvUse { bytes: 32, ..cbv(3, 0x1000, 2, 64) };
        let uses = [cbv(4, 0x1000, 3, 64), cbv(5, 0x2000, 4, 64), unlocated, unresolved, small, cbv(6, 0x3000, 5, 64)];
        assert_eq!(unsafe { l.snapshot(&draw(10, 1, 0xa, 0), &uses, &host, 4096) }, 0);
        let statuses: Vec<u32> = l.bindings.iter().map(|b| b.status).collect();
        assert_eq!(statuses, [1, 1, 2, 2, 4, 8], "copied, copied, not located, not resolved, small, unreadable");
        assert_eq!((l.bindings[2].parameter, l.bindings[3].parameter, l.bindings[3].offset), (0, 3, 1), "a located binding keeps its place");
        assert_eq!(l.bindings[0].payload, l.bindings[1].payload, "the same bytes interned once");
        assert_eq!((l.payloads.len(), l.blob.len(), l.bindings[0].hash), (2, 64, fnv(&[7u8; 64])));
        assert_eq!((l.draws[0].flags, l.draws[0].count, l.draws[0].first), (0, 6, 0), "not every CBV owned");
        assert_eq!(l.pending_lists(), 1);
        let mut full = Ledger::new();
        assert_eq!(unsafe { full.snapshot(&draw(11, 1, 0xa, 0), &[], &host, 0) }, THREW, "the draw capacity");
        full.blob.resize(BLOB_LIMIT - 10, 0);
        assert_eq!(unsafe { full.snapshot(&draw(11, 1, 0xa, 0), &[cbv(4, 0x1000, 3, 64)], &host, 4096) }, INTERN_REFUSED, "the blob full");
        assert_eq!(full.bindings[0].status, 8);
    }

    #[test]
    fn submission_reset_and_deferral() {
        reset_world();
        MEMORY.with(|m| {
            let mut m = m.borrow_mut();
            m.insert(0x1000, vec![1u8; 16]);
            m.insert(0x2000, vec![2u8; 16]);
        });
        RECEIPTS.with(|r| r.borrow_mut().insert(0x2000, 55));
        let mut l = Ledger::new();
        let with_positions = host(true);
        // Draw 0 on list a: one plain copy and one with a receipt, at position 5 (registered reads).
        assert_eq!(unsafe { l.snapshot(&draw(20, 1, 0xa, 5), &[cbv(1, 0x1000, 0, 16), cbv(2, 0x2000, 1, 16)], &with_positions, 4096) }, 0);
        assert_eq!(REGISTERED.with(|r| r.borrow().clone()), [(5, 1, 0, 0x1000, 16), (5, 1, 1, 0x2000, 16)]);
        assert_eq!(l.bindings[1].reserved, 3, "the receipt marks the reserved word");
        // Draw 1 on list b without a position.
        assert_eq!(unsafe { l.snapshot(&draw(21, 0, 0xb, 0), &[cbv(3, 0x1000, 0, 16), cbv(5, 0x1000, 1, 16)], &with_positions, 4096) }, 0);
        // Draw 2 on list c: abandoned by its reset.
        assert_eq!(unsafe { l.snapshot(&draw(22, 0, 0xc, 0), &[cbv(4, 0x1000, 0, 16)], &with_positions, 4096) }, 0);
        assert!(l.reset(0xc) && !l.reset(0xc));
        assert_eq!((l.draws[2].flags, l.bindings[4].status), (0, 64));
        // List a: the receipt binding deferred, the plain one re-read; list b validated whole.
        unsafe { l.before_submit(&[0xa as *mut c_void, 0xb as *mut c_void, 0xd as *mut c_void], &with_positions) };
        assert_eq!((l.bindings[0].status, l.bindings[1].status, l.deferred), (1, 128, 1));
        assert_eq!((l.draws[0].flags, l.draws[1].flags), (3, 7), "draw 0 deferred, draw 1 validated at submit");
        assert_eq!(l.pending_lists(), 0);
        // The deferred binding resolves ready at its position: interned, reserved 7, the draw complete.
        TIMELINE.with(|t| t.borrow_mut().insert(0x2000, (status::READY, 0, vec![2u8; 16])));
        assert_eq!(unsafe { l.resolve_deferred(&with_positions) }, 0);
        assert_eq!((l.bindings[1].status, l.bindings[1].reserved, l.deferred, l.draws[0].flags), (1, 7, 0, 7));
        assert_eq!(l.pair(20, 21), Some([0, 1]), "the second eye's draw and the first's");
        assert_eq!(l.payload(0, 3, 1, 1, 1), Some(&[2u8; 16][..]));
        assert_eq!(l.payload(0, 3, 9, 1, 1), None);
        assert_eq!(l.pair(21, 20), None, "the eyes reversed");
    }

    #[test]
    fn submission_refusals() {
        reset_world();
        MEMORY.with(|m| m.borrow_mut().insert(0x1000, vec![1u8; 16]));
        RECEIPTS.with(|r| r.borrow_mut().insert(0x1000, 9));
        let mut l = Ledger::new();
        let plain = host(false);
        assert_eq!(unsafe { l.snapshot(&draw(30, 1, 0xa, 0), &[cbv(1, 0x1000, 0, 16), cbv(2, 0x1000, 1, 16)], &plain, 4096) }, 0);
        // The second descriptor now points elsewhere; the first validates through its receipt but the validator refuses.
        DESCRIPTORS.with(|d| d.borrow_mut().insert(2, (0x900, 0x4000, 256)));
        VALIDATED.with(|v| *v.borrow_mut() = false);
        unsafe { l.before_submit(&[0xa as *mut c_void], &plain) };
        assert_eq!((l.bindings[0].status, l.bindings[1].status, l.draws[0].flags), (32, 16, 3));
        // A deferred binding that settles missing.
        reset_world();
        let mut l = Ledger::new();
        let positions = host(true);
        assert_eq!(unsafe { l.snapshot(&draw(31, 1, 0xa, 4), &[cbv(1, 0x5000, 0, 16)], &positions, 4096) }, 0);
        assert_eq!(l.bindings[0].status, 8, "unreadable at the draw");
        unsafe { l.before_submit(&[0xa as *mut c_void], &positions) };
        assert_eq!((l.bindings[0].status, l.deferred), (128, 1), "an unreadable binding with a position defers");
        assert_eq!(unsafe { l.resolve_deferred(&positions) }, 0);
        assert_eq!(l.bindings[0].status, 128, "still pending");
        TIMELINE.with(|t| t.borrow_mut().insert(0x5000, (status::MISSING, 3, Vec::new())));
        unsafe { l.resolve_deferred(&positions) };
        assert_eq!((l.bindings[0].status, l.bindings[0].reserved, l.deferred, l.draws[0].flags), (8, 0x8001_0003, 0, 2));
    }
}
