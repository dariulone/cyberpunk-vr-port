//! The upload ring allocator's successful-CAS receipts (the logic of include/Render/NativeUploadReservation.hpp).
//!
//! 2.5: sub_140419DF4 inlines the full-ring wait 2.31 kept out of line (sub_141EF5D8A), so the guarded body is the
//! whole function (312 bytes). The successful-CAS JNE sits at +0x10C and retries backwards to 0x419EEB (2.31: +0x76,
//! forward to 0x7745F4); the five success-only bytes after it are unchanged. A CALL there (patched by an external
//! debugger from an immutable, checksummed plan and accepted from its receipt) reports each successful reservation;
//! the `Ledger` keeps those witnesses as absolute ranges bound to an explicitly observed arena generation. A row is
//! evidence of the successful native CAS only: it carries no writer, retirement, source-submission, no-overwrite,
//! read or copy permission. Physical offsets never establish a generation; overflow or doubt quarantines.
use core::mem::{offset_of, size_of};

pub const ALLOCATOR_RVA: u64 = 0x419DF4;
pub const ALLOCATOR_BYTES: usize = 312;
pub const WITNESS_RVA: u64 = 0x419F00;
pub const RETRY_RVA: u64 = 0x419EEB;
pub const ALLOCATOR_FNV: u64 = 0x6B65_A165_0C2F_23E4;
pub const RETRY_DISPLACEMENT: u8 = (RETRY_RVA as i32 - (WITNESS_RVA as i32 + 7)) as u8;
pub const MAX_ROWS: usize = 4096;
pub const MAX_RINGS: usize = 64;
pub const MAX_XSTATE_BYTES: usize = 16384;

/// All enabled XSAVE state fits the thunk frame's fixed area.
pub fn valid_xstate(enabled: u64, supported: u64, bytes: u64) -> bool {
    enabled & 3 == 3
        && enabled & !supported == 0
        && bytes >= 576
        && bytes <= MAX_XSTATE_BYTES as u64
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Ignored,
    Captured,
    UnknownGeneration,
    InvalidWitness,
    Capacity,
    Quarantined,
    UnknownReservation,
    GenerationMismatch,
    RangeMismatch,
    Disabled,
    GuardMismatch,
    InstallFailure,
    Contention,
}
impl Status {
    fn from_raw(raw: u64) -> Status {
        const ALL: [Status; 13] = [
            Status::Ignored,
            Status::Captured,
            Status::UnknownGeneration,
            Status::InvalidWitness,
            Status::Capacity,
            Status::Quarantined,
            Status::UnknownReservation,
            Status::GenerationMismatch,
            Status::RangeMismatch,
            Status::Disabled,
            Status::GuardMismatch,
            Status::InstallFailure,
            Status::Contention,
        ];
        ALL.get(raw as usize)
            .copied()
            .unwrap_or(Status::InvalidWitness)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Witness {
    pub ring: u64,
    pub old_head: u64,
    pub absolute_begin: u64,
    pub absolute_end: u64,
    pub bytes: u64,
    pub capacity: u64,
    pub native_stack: u64,
    pub native_caller: u64,
    pub flags: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub sequence: u64,
    pub observer_epoch: u64,
    pub arena_generation: u64,
    pub ring: u64,
    pub old_head: u64,
    pub absolute_begin: u64,
    pub absolute_end: u64,
    pub bytes: u64,
    pub capacity: u64,
    pub physical_offset: u64,
    pub thread: u64,
    pub native_caller: u64,
    pub native_stack: u64,
    pub flags: u64,
    pub status: u64,
    pub witness_rva: u64,
}
impl Default for Row {
    fn default() -> Self {
        Row {
            sequence: 0,
            observer_epoch: 0,
            arena_generation: 0,
            ring: 0,
            old_head: 0,
            absolute_begin: 0,
            absolute_end: 0,
            bytes: 0,
            capacity: 0,
            physical_offset: 0,
            thread: 0,
            native_caller: 0,
            native_stack: 0,
            flags: 0,
            status: 0,
            witness_rva: WITNESS_RVA,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reservation {
    pub sequence: u64,
    pub observer_epoch: u64,
    pub arena_generation: u64,
    pub ring: u64,
    pub absolute_begin: u64,
    pub absolute_end: u64,
    pub capacity: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub absolute_begin: u64,
    pub absolute_end: u64,
    pub physical_offset: u64,
    pub padded_bytes: u64,
    pub logical_bytes: u64,
}

/// FNV-1a 64.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}

pub const EXTERNAL_PLAN_VERSION: u64 = 0x5550_4C41_4E30_3031;
pub const EXTERNAL_RECEIPT_VERSION: u64 = 0x5550_5245_4330_3031;
pub const ORIGINAL_WITNESS_BYTES: [u8; 7] = [0x75, 0xE9, 0x33, 0xD2, 0x48, 0x8B, 0xC1];

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalInstallState {
    Idle,
    Prepared,
    Accepted,
    PrepareRejected,
    ReceiptRejected,
    UnsupportedRequest,
    LegacyOwned,
}
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalInstallOutcome {
    None,
    CommittedDetached,
    RolledBackDetached,
    FailedNoWriteDetached,
    CommittedHeldEvent,
}
/// Immutable once published: the external debugger commits only this prepared CALL/JNE patch while holding the real
/// attach breakpoint event.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalInstallPlan {
    pub version: u64,
    pub nonce: u64,
    pub process_id: u64,
    pub process_creation_time: u64,
    pub game_base: u64,
    pub allocator_address: u64,
    pub site_address: u64,
    pub relay_address: u64,
    pub thunk_address: u64,
    pub module_pin_address: u64,
    pub allocator_bytes: u64,
    pub site_bytes: u64,
    pub relay_bytes: u64,
    pub thunk_bytes: u64,
    pub original_protection: u64,
    pub guard_fnv: u64,
    pub checksum: u64,
    pub reserved: [u64; 3],
    pub guard: [u8; ALLOCATOR_BYTES],
    pub original: [u8; 7],
    pub patch: [u8; 7],
    pub relay_code: [u8; 14],
    pub reserved_bytes: [u8; 4],
}
impl Default for ExternalInstallPlan {
    fn default() -> Self {
        // SAFETY: plain integers and byte arrays.
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExternalInstallReceipt {
    pub version: u64,
    pub nonce: u64,
    pub plan_checksum: u64,
    pub process_id: u64,
    pub process_creation_time: u64,
    pub outcome: u64,
    pub debug_event_code: u64,
    pub debug_event_thread: u64,
    pub original_protection: u64,
    pub final_protection: u64,
    pub bytes_written: u64,
    pub patch_verified: u64,
    pub cache_flushed: u64,
    pub detach_succeeded: u64,
    pub win32_error: u64,
    pub exception_code: u64,
    pub first_chance: u64,
    pub checksum: u64,
}
const _: () = {
    assert!(size_of::<Row>() == 128 && size_of::<ExternalInstallPlan>() == 504);
    assert!(
        size_of::<Witness>() == 72 && size_of::<Reservation>() == 56 && size_of::<Range>() == 40
    );
    assert!(
        offset_of!(ExternalInstallPlan, checksum) == 128
            && offset_of!(ExternalInstallPlan, guard) == 160
    );
    assert!(
        offset_of!(ExternalInstallPlan, original) == 472
            && offset_of!(ExternalInstallPlan, patch) == 479
    );
    assert!(offset_of!(ExternalInstallPlan, relay_code) == 486);
    assert!(
        size_of::<ExternalInstallReceipt>() == 144
            && offset_of!(ExternalInstallReceipt, checksum) == 136
    );
};
fn bytes_of<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: the plan/receipt are repr(C) integers and byte arrays without padding.
    unsafe { core::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}
/// The plan's FNV-1a with its checksum field taken as zero.
pub fn external_plan_checksum(value: &ExternalInstallPlan) -> u64 {
    let at = offset_of!(ExternalInstallPlan, checksum);
    bytes_of(value)
        .iter()
        .enumerate()
        .fold(14695981039346656037u64, |h, (i, &b)| {
            let byte = if (at..at + 8).contains(&i) { 0 } else { b };
            (h ^ byte as u64).wrapping_mul(1099511628211)
        })
}
pub fn external_receipt_checksum(value: &ExternalInstallReceipt) -> u64 {
    fnv(&bytes_of(value)[..offset_of!(ExternalInstallReceipt, checksum)])
}
pub fn valid_external_plan(value: &ExternalInstallPlan) -> bool {
    if value.version != EXTERNAL_PLAN_VERSION
        || value.nonce == 0
        || value.process_id == 0
        || value.process_id > u32::MAX as u64
        || value.process_creation_time == 0
        || value.game_base == 0
        || value.game_base > u64::MAX - ALLOCATOR_RVA - ALLOCATOR_BYTES as u64
        || value.allocator_address != value.game_base + ALLOCATOR_RVA
        || value.site_address != value.game_base + WITNESS_RVA
        || value.relay_address == 0
        || value.thunk_address == 0
        || value.module_pin_address == 0
        || value.module_pin_address > value.thunk_address
        || value.thunk_bytes == 0
        || value.thunk_bytes > 4096
        || value.thunk_address > u64::MAX - value.thunk_bytes
        || value.allocator_bytes != ALLOCATOR_BYTES as u64
        || value.site_bytes != 7
        || value.relay_bytes != 14
        || value.relay_address > u64::MAX - value.relay_bytes
        || !matches!(value.original_protection, 0x20 | 0x40 | 0x80)
        || value.guard_fnv != ALLOCATOR_FNV
        || fnv(&value.guard) != ALLOCATOR_FNV
        || value.original != ORIGINAL_WITNESS_BYTES
        || value.checksum != external_plan_checksum(value)
    {
        return false;
    }
    if value.reserved.iter().any(|&r| r != 0) || value.reserved_bytes.iter().any(|&r| r != 0) {
        return false;
    }
    let witness = (WITNESS_RVA - ALLOCATOR_RVA) as usize;
    if value.guard[witness..witness + 7] != value.original {
        return false;
    }
    if value.patch[0] != 0xE8
        || value.patch[5] != 0x75
        || value.patch[6] != RETRY_DISPLACEMENT
        || value.relay_code[0] != 0xFF
        || value.relay_code[1] != 0x25
        || value.relay_code[2..6].iter().any(|&b| b != 0)
    {
        return false;
    }
    let target = u64::from_le_bytes(value.relay_code[6..14].try_into().expect("eight bytes"));
    if target != value.thunk_address {
        return false;
    }
    let displacement = i32::from_le_bytes(value.patch[1..5].try_into().expect("four bytes")) as i64;
    let return_address = value.site_address + 5;
    if displacement >= 0 {
        return_address <= u64::MAX - displacement as u64
            && return_address + displacement as u64 == value.relay_address
    } else {
        return_address >= displacement.unsigned_abs()
            && return_address - displacement.unsigned_abs() == value.relay_address
    }
}
/// The current allocator body is the guard with exactly the plan's patch at the witness.
pub fn matches_external_patched_body(
    value: &ExternalInstallPlan,
    current: &[u8; ALLOCATOR_BYTES],
) -> bool {
    let witness = (WITNESS_RVA - ALLOCATOR_RVA) as usize;
    current.iter().enumerate().all(|(i, &b)| {
        b == if (witness..witness + value.patch.len()).contains(&i) {
            value.patch[i - witness]
        } else {
            value.guard[i]
        }
    })
}
pub fn valid_external_receipt(
    plan: &ExternalInstallPlan,
    receipt: &ExternalInstallReceipt,
) -> bool {
    valid_external_plan(plan)
        && receipt.version == EXTERNAL_RECEIPT_VERSION
        && receipt.nonce == plan.nonce
        && receipt.plan_checksum == plan.checksum
        && receipt.process_id == plan.process_id
        && receipt.process_creation_time == plan.process_creation_time
        && receipt.outcome == ExternalInstallOutcome::CommittedDetached as u64
        && receipt.debug_event_code == 1
        && receipt.debug_event_thread != 0
        && receipt.debug_event_thread <= u32::MAX as u64
        && receipt.exception_code == 0x8000_0003
        && receipt.first_chance == 1
        && receipt.original_protection == plan.original_protection
        && receipt.final_protection == plan.original_protection
        && receipt.bytes_written == 7
        && receipt.patch_verified == 1
        && receipt.cache_flushed == 1
        && receipt.detach_succeeded == 1
        && receipt.win32_error == 0
        && receipt.checksum == external_receipt_checksum(receipt)
}
pub fn valid_witness(value: &Witness) -> bool {
    value.flags & 0x40 != 0
        && value.ring != 0
        && value.ring <= u64::MAX - 0x18
        && value.capacity != 0
        && value.bytes != 0
        && value.bytes <= value.capacity
        && value.old_head <= value.absolute_begin
        && value.absolute_begin <= u64::MAX - value.bytes
        && value.absolute_end == value.absolute_begin + value.bytes
        && value.absolute_end - value.old_head <= value.capacity
        && value.absolute_begin % value.capacity <= value.capacity - value.bytes
}

#[derive(Clone, Copy, Default)]
struct Ring {
    address: u64,
    generation: u64,
    capacity: u64,
    quarantined: bool,
}

/// The bounded receipts of one observer epoch (rows never evicted: overflow quarantines everything): at most `rows`
/// rows of at most `rings` rings (the C++ template's Rows / Rings).
pub struct Ledger {
    rows: Box<[Row]>,
    rings: Box<[Ring]>,
    epoch: u64,
    sequence: u64,
    count: usize,
    ring_count: usize,
    exhausted: bool,
}
impl Ledger {
    pub fn new(rows: usize, rings: usize, observer_epoch: u64) -> Self {
        Ledger {
            rows: vec![Row::default(); rows].into_boxed_slice(),
            rings: vec![Ring::default(); rings].into_boxed_slice(),
            epoch: observer_epoch.max(1),
            sequence: 0,
            count: 0,
            ring_count: 0,
            exhausted: false,
        }
    }
    pub fn reset(&mut self, observer_epoch: u64) {
        self.rows.fill(Row::default());
        self.rings.fill(Ring::default());
        self.epoch = observer_epoch.max(1);
        self.sequence = 0;
        self.count = 0;
        self.ring_count = 0;
        self.exhausted = false;
    }
    /// Only an exact arena lifecycle observation may supply this identity: earlier rows stay unknown.
    pub fn begin_generation(&mut self, ring: u64, generation: u64, capacity: u64) -> bool {
        if ring == 0 || ring > u64::MAX - 0x18 || generation == 0 || capacity == 0 || self.exhausted
        {
            return false;
        }
        let index = match self.find_ring(ring) {
            Some(i) if generation <= self.rings[i].generation => {
                self.rings[i].quarantined = true;
                return false;
            }
            Some(i) => i,
            None => match self.add_ring(ring) {
                Some(i) => i,
                None => return false,
            },
        };
        self.rings[index] = Ring {
            address: ring,
            generation,
            capacity,
            quarantined: false,
        };
        true
    }
    pub fn end_generation(&mut self, ring: u64) {
        if let Some(i) = self.find_ring(ring) {
            self.rings[i].quarantined = true;
        }
    }
    pub fn quarantine_all(&mut self) {
        for ring in &mut self.rings[..self.ring_count] {
            ring.quarantined = true;
        }
        self.exhausted = true;
    }
    /// One allocator witness recorded (`capacity_stable`: the ring capacity still equals the allocator's saved R8).
    pub fn observe(
        &mut self,
        value: &Witness,
        thread: u64,
        capacity_stable: bool,
    ) -> (Status, Row) {
        if value.flags & 0x40 == 0 {
            return (Status::Ignored, Row::default());
        }
        if self.count == self.rows.len() || self.sequence == u64::MAX {
            self.quarantine_all();
            return (Status::Capacity, Row::default());
        }
        let mut state = self.find_ring(value.ring);
        let status;
        if !capacity_stable || !valid_witness(value) {
            status = Status::InvalidWitness;
            if let Some(i) = state {
                self.rings[i].quarantined = true;
            }
        } else {
            if state.is_none() {
                state = self.add_ring(value.ring);
            }
            status = match state {
                None => Status::Capacity,
                Some(i) if self.exhausted || self.rings[i].quarantined => Status::Quarantined,
                Some(i)
                    if self.rings[i].capacity != 0 && self.rings[i].capacity != value.capacity =>
                {
                    self.rings[i].quarantined = true;
                    Status::Quarantined
                }
                Some(i) => {
                    self.rings[i].capacity = value.capacity;
                    if self.rings[i].generation != 0 {
                        Status::Captured
                    } else {
                        Status::UnknownGeneration
                    }
                }
            };
        }
        self.sequence += 1;
        let row = Row {
            sequence: self.sequence,
            observer_epoch: self.epoch,
            arena_generation: state.map_or(0, |i| self.rings[i].generation),
            ring: value.ring,
            old_head: value.old_head,
            absolute_begin: value.absolute_begin,
            absolute_end: value.absolute_end,
            bytes: value.bytes,
            capacity: value.capacity,
            physical_offset: if value.capacity != 0 {
                value.absolute_begin % value.capacity
            } else {
                0
            },
            thread,
            native_caller: value.native_caller,
            native_stack: value.native_stack,
            flags: value.flags,
            status: status as u64,
            witness_rva: WITNESS_RVA,
        };
        self.rows[self.count] = row;
        self.count += 1;
        (status, row)
    }
    pub fn find(&self, sequence: u64) -> (Status, Reservation) {
        let Some(row) = self.find_row(sequence) else {
            return (Status::UnknownReservation, Reservation::default());
        };
        if row.status != Status::Captured as u64 {
            return (Status::from_raw(row.status), Reservation::default());
        }
        let state = self.find_ring(row.ring).map(|i| self.rings[i]);
        let Some(state) = state.filter(|s| !self.exhausted && !s.quarantined) else {
            return (Status::Quarantined, Reservation::default());
        };
        if row.arena_generation == 0
            || row.arena_generation != state.generation
            || row.capacity != state.capacity
        {
            return (Status::GenerationMismatch, Reservation::default());
        }
        (
            Status::Captured,
            Reservation {
                sequence: row.sequence,
                observer_epoch: row.observer_epoch,
                arena_generation: row.arena_generation,
                ring: row.ring,
                absolute_begin: row.absolute_begin,
                absolute_end: row.absolute_end,
                capacity: row.capacity,
            },
        )
    }
    pub fn validate_range(
        &self,
        reservation: &Reservation,
        local_offset: u64,
        physical_offset: u64,
        padded_bytes: u64,
        logical_bytes: u64,
        backing_bytes: u64,
    ) -> (Status, Range) {
        let (status, actual) = self.find(reservation.sequence);
        if status != Status::Captured {
            return (status, Range::default());
        }
        if actual != *reservation {
            return (Status::GenerationMismatch, Range::default());
        }
        let span = actual.absolute_end - actual.absolute_begin;
        if padded_bytes == 0
            || logical_bytes == 0
            || logical_bytes > padded_bytes
            || local_offset > span
            || padded_bytes > span - local_offset
            || actual.absolute_begin > u64::MAX - local_offset
        {
            return (Status::RangeMismatch, Range::default());
        }
        let begin = actual.absolute_begin + local_offset;
        if begin > u64::MAX - padded_bytes
            || physical_offset != begin % actual.capacity
            || physical_offset > actual.capacity
            || padded_bytes > actual.capacity - physical_offset
            || physical_offset > backing_bytes
            || padded_bytes > backing_bytes - physical_offset
        {
            return (Status::RangeMismatch, Range::default());
        }
        (
            Status::Captured,
            Range {
                absolute_begin: begin,
                absolute_end: begin + padded_bytes,
                physical_offset,
                padded_bytes,
                logical_bytes,
            },
        )
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn at(&self, i: usize) -> &Row {
        &self.rows[i]
    }
    /// Every row slot, default beyond `count` (the C++ array's view).
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn exhausted(&self) -> bool {
        self.exhausted
    }

    fn find_ring(&self, ring: u64) -> Option<usize> {
        self.rings[..self.ring_count]
            .iter()
            .position(|r| r.address == ring)
    }
    fn add_ring(&mut self, ring: u64) -> Option<usize> {
        if self.ring_count == self.rings.len() {
            self.quarantine_all();
            return None;
        }
        let index = self.ring_count;
        self.ring_count += 1;
        self.rings[index] = Ring {
            address: ring,
            generation: 0,
            capacity: 0,
            quarantined: self.exhausted,
        };
        Some(index)
    }
    fn find_row(&self, sequence: u64) -> Option<&Row> {
        if sequence == 0 || sequence > self.count as u64 {
            return None;
        }
        let row = &self.rows[sequence as usize - 1];
        (row.sequence == sequence).then_some(row)
    }
}
impl Default for Ledger {
    fn default() -> Self {
        Ledger::new(MAX_ROWS, MAX_RINGS, 1)
    }
}

/// The thunk's saved state. The real CALL at the witness supplies a valid unwind return address; the installer
/// selects the full enabled XSAVE state, or legacy FXSAVE only when OSXSAVE is absent. The fixed area includes the
/// alignment slack and is never grown.
#[repr(C, align(16))]
pub struct ThunkFrame {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub flags: u64,
    pub native_stack: u64,
    pub native_caller: u64,
    pub xstate_address: u64,
    pub xstate_mask: u64,
    pub capacity: u64,
    pub xstate_mode: u64,
    pub xstate: [u8; MAX_XSTATE_BYTES + 63],
}
const _: () = {
    assert!(offset_of!(ThunkFrame, flags) == 0x78 && offset_of!(ThunkFrame, native_stack) == 0x80);
    assert!(
        offset_of!(ThunkFrame, xstate_address) == 0x90 && offset_of!(ThunkFrame, xstate) == 0xB0
    );
    assert!(size_of::<ThunkFrame>() == 0x40F0);
};

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/upload_reservation_test.cpp's cases, and the external plan's checks.
    use super::*;

    fn cas(old_head: u64, begin: u64, bytes: u64, capacity: u64) -> Witness {
        Witness {
            ring: 0x1000,
            old_head,
            absolute_begin: begin,
            absolute_end: begin.wrapping_add(bytes),
            bytes,
            capacity,
            native_stack: 0x2000,
            native_caller: 0x1F28C7,
            flags: 0x246,
        }
    }
    fn cas0(old_head: u64, begin: u64) -> Witness {
        cas(old_head, begin, 256, 4096)
    }
    const HEX: &str = concat!(
        "48895c241855565741544155415641574883ec40498bf94c8bea488bd94d85c9740441c6010048",
        "8b6908498d40ff48898424800000004c8be049f7d44c8b0b4a8d0c084923cc4c8b431033d2488b",
        "c149f7f04a8d042a493bc07609498bc0482bc24803c84a8d1429488bc2482bc5493bc00f868d00",
        "00004885ff0f84b20000004032f6488d5331488d8c2488000000e8d80ebe0190488b4308483be8",
        "753e837b2400743b4c8b73188b4328488d3440eb02f390498b54f608498b0cf6e81fcefcff84c0",
        "74ec498b44f61048874308488d542420488d4b18e804e0fcff40b60133c9488b84248800000086",
        "084084f67442488b6b08c60701488b842480000000e938ffffff498bc1f0480fb11375e933d248",
        "8bc148f77310488bc2488b9c24900000004883c440415f415e415d415c5f5e5dc3488b5310ebdf",
    );
    fn body() -> [u8; ALLOCATOR_BYTES] {
        assert_eq!(HEX.len(), ALLOCATOR_BYTES * 2);
        let mut body = [0u8; ALLOCATOR_BYTES];
        for (i, b) in body.iter_mut().enumerate() {
            *b = u8::from_str_radix(&HEX[i * 2..i * 2 + 2], 16).expect("hex");
        }
        body
    }

    #[test]
    fn body_guard_and_xstate() {
        assert!(valid_xstate(7, 0xE7, 832) && valid_xstate(0xE7, 0xE7, 2688));
        assert!(
            !valid_xstate(1, 7, 832) && !valid_xstate(0x207, 7, 2048) && !valid_xstate(7, 7, 575)
        );
        assert!(!valid_xstate(7, 7, MAX_XSTATE_BYTES as u64 + 1));
        let mut body = body();
        assert_eq!(
            fnv(&body),
            ALLOCATOR_FNV,
            "the exact whole-allocator guard (2.5 sub_140419DF4)"
        );
        let witness = (WITNESS_RVA - ALLOCATOR_RVA) as usize;
        assert_eq!(body[witness..witness + 7], ORIGINAL_WITNESS_BYTES);
        body[witness] ^= 1;
        assert_ne!(fnv(&body), ALLOCATOR_FNV);
        assert_eq!(RETRY_DISPLACEMENT, 0xE4);
    }
    #[test]
    fn receipts_generations_and_ranges() {
        let mut ledger = Ledger::new(8, 2, 7);
        let mut failed = cas0(0, 0);
        failed.flags &= !0x40;
        let (status, row) = ledger.observe(&failed, 9, true);
        assert!(status == Status::Ignored && ledger.count() == 0 && row.sequence == 0);
        let (status, row) = ledger.observe(&cas0(0, 0), 9, true);
        assert!(
            status == Status::UnknownGeneration
                && row.old_head == 0
                && row.absolute_end == 256
                && row.thread == 9
        );
        let (status, receipt) = ledger.find(row.sequence);
        assert!(status == Status::UnknownGeneration && receipt.sequence == 0);
        assert!(ledger.begin_generation(0x1000, 11, 4096));
        assert_eq!(
            ledger.find(1).0,
            Status::UnknownGeneration,
            "a later lifecycle promoted an earlier reservation"
        );
        let (status, row) = ledger.observe(&cas(4000, 4096, 512, 4096), 10, true);
        assert!(
            status == Status::Captured && row.physical_offset == 0 && row.arena_generation == 11
        );
        let (status, receipt) = ledger.find(row.sequence);
        assert_eq!(status, Status::Captured);
        let (status, range) = ledger.validate_range(&receipt, 256, 256, 256, 144, 4096);
        assert!(
            status == Status::Captured
                && range.absolute_begin == 4352
                && range.absolute_end == 4608
        );
        assert_eq!(range.logical_bytes, 144);
        assert_eq!(
            ledger.validate_range(&receipt, 256, 0, 256, 144, 4096).0,
            Status::RangeMismatch
        );
        assert_eq!(
            ledger.validate_range(&receipt, 257, 257, 256, 144, 4096).0,
            Status::RangeMismatch
        );
        assert_eq!(
            ledger.validate_range(&receipt, 256, 256, 256, 257, 4096).0,
            Status::RangeMismatch
        );
        assert_eq!(
            ledger.validate_range(&receipt, 256, 256, 256, 144, 511).0,
            Status::RangeMismatch
        );
        let forged = Reservation {
            arena_generation: receipt.arena_generation + 1,
            ..receipt
        };
        assert_eq!(
            ledger.validate_range(&forged, 0, 0, 256, 144, 4096).0,
            Status::GenerationMismatch
        );
        // Callbacks may arrive in the reverse order of concurrent successful CASes.
        assert_eq!(
            ledger.observe(&cas0(4864, 4864), 10, true).0,
            Status::Captured
        );
        assert_eq!(
            ledger.observe(&cas0(4608, 4608), 11, true).0,
            Status::Captured
        );
        assert!(ledger.begin_generation(0x1000, 12, 4096));
        assert_eq!(ledger.find(receipt.sequence).0, Status::GenerationMismatch);
        assert!(
            !ledger.begin_generation(0x1000, 12, 4096),
            "a duplicate generation quarantined"
        );
        assert_eq!(
            ledger.observe(&cas0(5120, 5120), 10, true).0,
            Status::Quarantined
        );
        ledger.reset(8);
        assert!(ledger.count() == 0 && !ledger.exhausted());
        assert!(ledger.begin_generation(0x1000, 1, 4096));
        let mut invalid = cas0(u64::MAX - 100, u64::MAX - 100);
        invalid.absolute_end = 155;
        assert_eq!(ledger.observe(&invalid, 10, true).0, Status::InvalidWitness);
        assert_eq!(ledger.observe(&cas0(0, 0), 10, true).0, Status::Quarantined);

        let mut bounded = Ledger::new(2, 1, 1);
        assert!(bounded.begin_generation(0x1000, 1, 4096));
        let (status, row) = bounded.observe(&cas0(0, 0), 1, true);
        assert_eq!(status, Status::Captured);
        let first = row.sequence;
        assert_eq!(
            bounded.observe(&cas0(256, 256), 1, true).0,
            Status::Captured
        );
        assert_eq!(
            bounded.observe(&cas0(512, 512), 1, true).0,
            Status::Capacity
        );
        assert!(
            bounded.exhausted() && bounded.count() == 2,
            "rows evicted on overflow"
        );
        assert_eq!(bounded.find(first).0, Status::Quarantined);
        let mut ring_limit = Ledger::new(4, 1, 1);
        assert!(ring_limit.begin_generation(0x1000, 1, 4096));
        let other = Witness {
            ring: 0x3000,
            ..cas0(0, 0)
        };
        assert!(
            ring_limit.observe(&other, 1, true).0 == Status::Capacity && ring_limit.exhausted()
        );
        let mut changed = Ledger::new(4, 1, 1);
        assert!(changed.begin_generation(0x1000, 1, 4096));
        assert_eq!(
            changed.observe(&cas(0, 0, 256, 8192), 1, true).0,
            Status::Quarantined
        );
        let mut raced = Ledger::new(4, 1, 1);
        assert!(raced.begin_generation(0x1000, 1, 4096));
        assert_eq!(
            raced.observe(&cas0(0, 0), 1, false).0,
            Status::InvalidWitness
        );
        let mut recycled = Ledger::new(4, 1, 1);
        assert!(recycled.begin_generation(0x1000, 1, 4096));
        let (status, row) = recycled.observe(&cas0(0, 0), 1, true);
        let (found, receipt) = recycled.find(row.sequence);
        assert!(status == Status::Captured && found == Status::Captured);
        recycled.reset(2);
        assert!(recycled.begin_generation(0x1000, 1, 4096));
        let (status, row) = recycled.observe(&cas0(0, 0), 1, true);
        assert_eq!(status, Status::Captured);
        assert_eq!(
            recycled.validate_range(&receipt, 0, 0, 256, 144, 4096).0,
            Status::GenerationMismatch
        );
        recycled.end_generation(0x1000);
        assert_eq!(recycled.find(row.sequence).0, Status::Quarantined);
        assert!(
            !valid_witness(&cas0(0, 4096))
                && !valid_witness(&cas(0, 3840, 512, 4096))
                && !valid_witness(&cas0(1, 0))
        );
    }
    fn plan() -> ExternalInstallPlan {
        let base = 0x1_4000_0000u64;
        let mut plan = ExternalInstallPlan {
            version: EXTERNAL_PLAN_VERSION,
            nonce: 5,
            process_id: 1234,
            process_creation_time: 99,
            game_base: base,
            allocator_address: base + ALLOCATOR_RVA,
            site_address: base + WITNESS_RVA,
            relay_address: base + WITNESS_RVA + 5 + 0x1000,
            thunk_address: 0x7FF0_0000_1000,
            module_pin_address: 0x7FF0_0000_0000,
            allocator_bytes: ALLOCATOR_BYTES as u64,
            site_bytes: 7,
            relay_bytes: 14,
            thunk_bytes: 256,
            original_protection: 0x20,
            guard_fnv: ALLOCATOR_FNV,
            guard: body(),
            original: ORIGINAL_WITNESS_BYTES,
            ..ExternalInstallPlan::default()
        };
        plan.patch = [0xE8, 0x00, 0x10, 0x00, 0x00, 0x75, RETRY_DISPLACEMENT];
        plan.relay_code[0] = 0xFF;
        plan.relay_code[1] = 0x25;
        plan.relay_code[6..14].copy_from_slice(&plan.thunk_address.to_le_bytes());
        plan.checksum = external_plan_checksum(&plan);
        plan
    }
    #[test]
    fn external_plan_and_receipt() {
        let p = plan();
        assert!(valid_external_plan(&p));
        let mut patched = p.guard;
        let witness = (WITNESS_RVA - ALLOCATOR_RVA) as usize;
        patched[witness..witness + 7].copy_from_slice(&p.patch);
        assert!(
            matches_external_patched_body(&p, &patched)
                && !matches_external_patched_body(&p, &p.guard)
        );
        let mut tampered = p;
        tampered.nonce += 1;
        assert!(
            !valid_external_plan(&tampered),
            "a changed plan kept its checksum"
        );
        let mut wrong_relay = p;
        wrong_relay.patch[1] = 0x01;
        wrong_relay.checksum = external_plan_checksum(&wrong_relay);
        assert!(!valid_external_plan(&wrong_relay));
        let mut receipt = ExternalInstallReceipt {
            version: EXTERNAL_RECEIPT_VERSION,
            nonce: p.nonce,
            plan_checksum: p.checksum,
            process_id: p.process_id,
            process_creation_time: p.process_creation_time,
            outcome: ExternalInstallOutcome::CommittedDetached as u64,
            debug_event_code: 1,
            debug_event_thread: 77,
            original_protection: 0x20,
            final_protection: 0x20,
            bytes_written: 7,
            patch_verified: 1,
            cache_flushed: 1,
            detach_succeeded: 1,
            exception_code: 0x8000_0003,
            first_chance: 1,
            ..ExternalInstallReceipt::default()
        };
        receipt.checksum = external_receipt_checksum(&receipt);
        assert!(valid_external_receipt(&p, &receipt));
        receipt.bytes_written = 6;
        assert!(!valid_external_receipt(&p, &receipt));
    }
}
