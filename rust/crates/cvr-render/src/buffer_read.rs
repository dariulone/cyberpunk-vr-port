//! THE NATIVE CONSTANT BUFFERS' READ PROOFS (formerly src/Render/NativeBufferRead.cpp): which native class-1 constant
//! pool buffers the engine's uploader last wrote, on which copy-queue recording and whether that recording reached
//! the GPU, so a later draw can prove the 292-byte camera block (or another range) it reads is the uploaded version.
//!
//! A buffer created in COMMON / GENERIC_READ on a default heap gets a receipt (a COM object held as the resource's
//! private data); every command list a recording (the same, on the list). The uploader's writes to a receipt's buffer
//! are attributed to the recording that made them; the recording's submission on the registry's own upload queue
//! moves the receipt's history along; anything else touching the buffer -- a transition, an alias, a write through
//! another path, an address-range write -- invalidates it with its first cause kept for the diagnostics. A proof is
//! taken at the draw (Preflight / CaptureRange: the native pool slot read twice, the engine's current upload context
//! on this list, the buffer's residency) and checked again before the consumer's submission (Validate*).
//!
//! The observers stay opt-in (observe_bindings). The copy windows' observers (src/Render/NativeCopyWindows.cpp) and the
//! engine's current upload context (camera_upload::CurrentContext) stay C++: the face calls the former and hands the
//! latter over at its static initialisation.
use core::cell::Cell;
use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::sync::{Mutex, MutexGuard};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface as _, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

// ---- the shapes the C++ half shares (include/Render/NativeBufferRead.hpp, NativeIaBinding.hpp)

/// geometry::NativeBuffer.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeBuffer {
    pub resource: u64,
    pub gpu: u64,
    pub bytes: u64,
    pub registry: u64,
    pub context: u64,
    pub handle: u32,
    pub kind: u32,
}
/// geometry::BufferLease as the face passes it: the resource borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Lease {
    pub resource: *mut c_void,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
    pub native: NativeBuffer,
}
/// buffer_read::Pool.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Pool {
    pub entry: u64,
    pub owner: u64,
    pub allocation: u32,
}
/// buffer_read::Proof.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Proof {
    pub receipt: u64,
    pub epoch: u64,
    pub recording: u64,
    pub generation: u64,
    pub registry: u64,
    pub direct_queue: u64,
    pub pool: Pool,
    pub com_offset: u64,
    pub copy_bytes: u64,
    pub read_registered: bool,
}
/// camera_upload::Context.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct UploadContext {
    pub context: u64,
    pub state: u64,
    pub list: u64,
}
impl UploadContext {
    fn valid(&self) -> bool {
        self.context != 0 && self.state != 0 && self.list != 0
    }
}
const _: () = assert!(
    core::mem::size_of::<NativeBuffer>() == 48
        && core::mem::size_of::<Lease>() == 80
        && core::mem::size_of::<Pool>() == 24
        && core::mem::size_of::<Proof>() == 96
        && core::mem::size_of::<UploadContext>() == 24
);

// ---- the header's pure parts (detail::)

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Recorded,
    Submitting,
    Submitted,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Producer {
    generation: u64,
    stage: Stage,
}
const HISTORY: usize = 128;
/// detail::History: the writes of one buffer since its last upload, by recording.
#[repr(C)]
#[derive(Clone)]
pub struct History {
    pub(crate) epoch: u64,
    pub(crate) producer: u64,
    pub(crate) valid: bool,
    pub(crate) uploaded: bool,
    pub(crate) pending: bool,
    pub(crate) producers: [Producer; HISTORY],
    pub(crate) count: u32,
}
impl History {
    pub(crate) fn new() -> Self {
        History {
            epoch: 1,
            producer: 0,
            valid: true,
            uploaded: false,
            pending: false,
            producers: [Producer {
                generation: 0,
                stage: Stage::Recorded,
            }; HISTORY],
            count: 0,
        }
    }
    pub(crate) fn find(&mut self, recording: u64) -> Option<&mut Producer> {
        self.producers[..self.count as usize]
            .iter_mut()
            .find(|p| p.generation == recording)
    }
    pub(crate) fn refresh(&mut self) {
        self.pending = false;
        self.uploaded = self.count != 0;
        for p in &self.producers[..self.count as usize] {
            self.pending |= p.stage == Stage::Submitting;
            self.uploaded &= p.stage == Stage::Submitted;
        }
    }
    pub(crate) fn invalidate(&mut self) {
        self.valid = false;
    }
    pub(crate) fn written(&mut self, recording: u64) {
        if !self.valid || recording == 0 || self.epoch == u64::MAX {
            self.valid = false;
            return;
        }
        let found = self.find(recording).map(|w| w.stage);
        if found.is_some_and(|s| s != Stage::Recorded) {
            self.valid = false;
            return;
        }
        let mut writer = found.is_some();
        if self.uploaded {
            self.count = 0;
            writer = false;
        }
        if !writer {
            if self.count as usize == HISTORY {
                self.valid = false;
                return;
            }
            self.producers[self.count as usize] = Producer {
                generation: recording,
                stage: Stage::Recorded,
            };
            self.count += 1;
        }
        self.epoch += 1;
        self.producer = recording;
        self.refresh();
    }
    pub(crate) fn before_submit(&mut self, recording: u64, queue_matches: bool) {
        let valid = self.valid;
        match self.find(recording) {
            Some(w) if valid && recording != 0 && queue_matches && w.stage == Stage::Recorded => {
                w.stage = Stage::Submitting
            }
            _ => {
                self.valid = false;
                return;
            }
        }
        self.refresh();
    }
    pub(crate) fn submitted(&mut self, recording: u64, queue_matches: bool) {
        let valid = self.valid;
        match self.find(recording) {
            Some(w) if valid && recording != 0 && queue_matches && w.stage == Stage::Submitting => {
                w.stage = Stage::Submitted
            }
            _ => {
                self.valid = false;
                return;
            }
        }
        self.refresh();
    }
    pub(crate) fn abandon(&mut self, recording: u64) {
        if self
            .find(recording)
            .is_some_and(|w| w.stage != Stage::Submitted)
        {
            self.valid = false;
        }
    }
    pub(crate) fn matches(&self, saved: u64) -> bool {
        self.valid && self.uploaded && !self.pending && saved != 0 && self.epoch == saved
    }
    pub(crate) fn bits(&self) -> u64 {
        self.valid as u64 | (self.uploaded as u64) << 1 | (self.pending as u64) << 2
    }
}
/// detail::RecordingState.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RecordingState {
    pub(crate) valid: bool,
    pub(crate) pending: bool,
    pub(crate) submitted: bool,
}
impl Default for RecordingState {
    fn default() -> Self {
        RecordingState {
            valid: true,
            pending: false,
            submitted: false,
        }
    }
}
impl RecordingState {
    pub(crate) fn before_submit(&mut self) -> bool {
        if !self.valid || self.pending || self.submitted {
            self.valid = false;
            return false;
        }
        self.pending = true;
        true
    }
    pub(crate) fn submitted(&mut self) -> bool {
        if !self.valid || !self.pending || self.submitted {
            self.valid = false;
            return false;
        }
        self.pending = false;
        self.submitted = true;
        true
    }
    pub(crate) fn bits(&self) -> u64 {
        self.valid as u64 | (self.pending as u64) << 1 | (self.submitted as u64) << 2
    }
}
fn initial_state(state: u32) -> bool {
    state == D3D12_RESOURCE_STATE_COMMON.0 as u32
        || state == D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32
}
/// detail::ValidRange: the range inside the native buffer's 512-byte slot of a class-1 pool buffer.
pub fn valid_range(
    source: &NativeBuffer,
    resource_gpu: u64,
    resource_bytes: u64,
    com_offset: u64,
    copy_bytes: u64,
) -> bool {
    if source.kind != 1
        || source.resource == 0
        || source.gpu == 0
        || source.bytes == 0
        || source.bytes > 512
        || resource_gpu == 0
        || resource_bytes < 512
        || resource_bytes > 512 * 1024
        || resource_bytes % 512 != 0
        || resource_gpu > u64::MAX - resource_bytes
        || source.gpu < resource_gpu
        || copy_bytes == 0
        || com_offset > resource_bytes
        || copy_bytes > resource_bytes - com_offset
    {
        return false;
    }
    let native_offset = source.gpu - resource_gpu;
    if native_offset % 512 != 0
        || native_offset > resource_bytes
        || source.bytes > resource_bytes - native_offset
        || com_offset < native_offset
        || com_offset - native_offset > source.bytes
        || copy_bytes > source.bytes - (com_offset - native_offset)
    {
        return false;
    }
    com_offset - native_offset < 512 && copy_bytes <= 512 - (com_offset - native_offset)
}
fn read(address: u64, out: &mut [u8]) -> bool {
    address != 0 && address <= u64::MAX - out.len() as u64 && cvr_engine::stack::read(address, out)
}
fn read_value<T: Copy + Default>(address: u64) -> Option<T> {
    if address == 0 || address > u64::MAX - core::mem::size_of::<T>() as u64 {
        return None;
    }
    cvr_engine::stack::read_value(address)
}
/// detail::ReadPoolRange: the native pool entry owning the buffer's slot (the reason of a refusal in `reason`), read
/// from the engine's memory.
fn read_pool_range(
    game: u64,
    source: &NativeBuffer,
    resource_gpu: u64,
    resource_bytes: u64,
    com_offset: u64,
    copy_bytes: u64,
    reason: &mut u32,
) -> Option<Pool> {
    read_pool_range_with(
        &mut read,
        game,
        source,
        resource_gpu,
        resource_bytes,
        com_offset,
        copy_bytes,
        reason,
    )
}
/// detail::ReadPoolRange through `read` (address, bytes) -> readable, in the C++ order of reads: a refused field ends
/// the reading there.
#[allow(clippy::too_many_arguments)]
pub fn read_pool_range_with(
    read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    game: u64,
    source: &NativeBuffer,
    resource_gpu: u64,
    resource_bytes: u64,
    com_offset: u64,
    copy_bytes: u64,
    reason: &mut u32,
) -> Option<Pool> {
    *reason = 1;
    if game == 0
        || game > u64::MAX - 0x3BCDCB0
        || source.registry == 0
        || source.resource == 0
        || source.kind != 1
        || source.handle == 0
        || source.handle > 0x100000
        || !valid_range(source, resource_gpu, resource_bytes, com_offset, copy_bytes)
    {
        return None;
    }
    *reason = 2;
    let registry = word(read, game + 0x3BCDCA8)?;
    if registry != source.registry {
        return None;
    }
    let mut unit = [0u8; 4];
    if !bounded(read, game + 0x36F44C8, &mut unit)
        || u32::from_le_bytes(unit) != 512
        || registry > u64::MAX - 0x14105C1
    {
        return None;
    }
    *reason = 3;
    let mut enabled = [0u8; 1];
    if !bounded(read, registry + 0x14105C0, &mut enabled) || enabled[0] == 0 {
        return None;
    }
    let offset = 0x5C0AF8 + (source.handle as u64 - 1) * 0xB0;
    if registry > u64::MAX - offset - 0x50 {
        return None;
    }
    let mut record = [0u8; 0x50];
    *reason = 4;
    if !bounded(read, registry + offset, &mut record) || record[6] != 0x16 {
        return None;
    }
    let bytes = u32::from_le_bytes(record[0..4].try_into().unwrap());
    let gpu = u64::from_le_bytes(record[8..16].try_into().unwrap());
    let resource = u64::from_le_bytes(record[16..24].try_into().unwrap());
    let allocation = u32::from_le_bytes(record[0x40..0x44].try_into().unwrap());
    let index = (allocation & 65535) >> 3;
    let slot = allocation >> 16;
    *reason = 5;
    if bytes as u64 != source.bytes
        || gpu != source.gpu
        || resource != source.resource
        || allocation & 7 != 0
        || slot as u64 >= resource_bytes / 512
        || gpu != resource_gpu.wrapping_add(slot as u64 * 512)
    {
        return None;
    }
    *reason = 6;
    let mut group = [0u8; 16];
    if !bounded(read, registry + 0x1410548, &mut group) {
        return None;
    }
    let data = u64::from_le_bytes(group[0..8].try_into().unwrap());
    let capacity = u32::from_le_bytes(group[8..12].try_into().unwrap());
    let count = u32::from_le_bytes(group[12..16].try_into().unwrap());
    if data == 0
        || count == 0
        || count > capacity
        || capacity > 8192
        || index >= count
        || data > u64::MAX - index as u64 * 144 - 144
    {
        return None;
    }
    let entry = data + index as u64 * 144;
    *reason = 7;
    let live = word(read, entry)?;
    if live != source.resource {
        return None;
    }
    let owner = word(read, entry + 8)?;
    if owner == 0 {
        return None;
    }
    let occupied = word(read, entry + 16 + (slot as u64 / 64) * 8)?;
    if occupied & (1u64 << (slot % 64)) == 0 {
        return None;
    }
    *reason = 0;
    Some(Pool {
        entry,
        owner,
        allocation,
    })
}

fn bounded(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64, out: &mut [u8]) -> bool {
    address != 0 && address <= u64::MAX - out.len() as u64 && read(address, out)
}
fn word(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    bounded(read, address, &mut b).then(|| u64::from_le_bytes(b))
}

// ---- the header's detail for include/Render/NativeBufferRead.hpp (CVR_RUST_VRIK: its inline rules forward here;
// tools/single_pass_tests/buffer_read_state_test runs its cases against them in its Rust variant)

/// geometry::MemoryReader: (user, address, out, bytes) -> readable.
pub type MemoryReader = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
const _: () = assert!(
    core::mem::size_of::<History>() == 2080
        && core::mem::size_of::<RecordingState>() == 3
        && core::mem::size_of::<Proof>() == 96
        && core::mem::size_of::<NativeBuffer>() == 48
);

/// detail::ValidRange.
///
/// # Safety
/// `source` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_brd_valid_range(
    source: *const NativeBuffer,
    resource_gpu: u64,
    resource_bytes: u64,
    com_offset: u64,
    copy_bytes: u64,
) -> bool {
    unsafe {
        valid_range(
            &*source,
            resource_gpu,
            resource_bytes,
            com_offset,
            copy_bytes,
        )
    }
}
/// detail::ReadPoolRange (`out` zeroed first and written on success; `reason` null or written at each step).
///
/// # Safety
/// `source` is valid; `out` is writable; `reason` is null or writable; `reader` follows geometry::MemoryReader.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_brd_read_pool_range(
    reader: Option<MemoryReader>,
    user: *mut c_void,
    game: u64,
    source: *const NativeBuffer,
    resource_gpu: u64,
    resource_bytes: u64,
    com_offset: u64,
    copy_bytes: u64,
    out: *mut Pool,
    reason: *mut u32,
) -> bool {
    unsafe {
        *out = Pool::default();
        let mut step = 1u32;
        let mut read = |address: u64, bytes: &mut [u8]| match reader {
            Some(reader) => reader(user, address, bytes.as_mut_ptr().cast(), bytes.len()),
            None => false,
        };
        let result = read_pool_range_with(
            &mut read,
            game,
            &*source,
            resource_gpu,
            resource_bytes,
            com_offset,
            copy_bytes,
            &mut step,
        );
        if !reason.is_null() {
            *reason = step;
        }
        match result {
            Some(pool) => {
                *out = pool;
                true
            }
            None => false,
        }
    }
}
/// detail::ReadPool: the original material path with its 292-byte admission minimum.
///
/// # Safety
/// As for `cvr_brd_read_pool_range`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_brd_read_pool(
    reader: Option<MemoryReader>,
    user: *mut c_void,
    game: u64,
    source: *const NativeBuffer,
    resource_gpu: u64,
    resource_bytes: u64,
    out: *mut Pool,
    reason: *mut u32,
) -> bool {
    unsafe {
        *out = Pool::default();
        if !reason.is_null() {
            *reason = 1;
        }
        let s = &*source;
        if s.bytes < 292 || s.gpu < resource_gpu {
            return false;
        }
        cvr_brd_read_pool_range(
            reader,
            user,
            game,
            source,
            resource_gpu,
            resource_bytes,
            s.gpu - resource_gpu,
            292,
            out,
            reason,
        )
    }
}
/// detail::SameRangeObservation (`which` 0) / RegisteredRange (1, `a` the proof).
///
/// # Safety
/// The proofs are valid (`b` where used).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_brd_proof(
    which: u32,
    a: *const Proof,
    b: *const Proof,
    com_offset: u64,
    copy_bytes: u64,
) -> bool {
    unsafe {
        if which == 0 {
            same_range_observation(&*a, &*b)
        } else {
            registered_range(&*a, com_offset, copy_bytes)
        }
    }
}
/// detail::InitialState.
#[no_mangle]
pub extern "C-unwind" fn cvr_brd_initial_state(state: u32) -> bool {
    initial_state(state)
}
/// History: Find (`which` 0: the producer's index + 1, or 0), Refresh (1), Invalidate (2), Written (3), BeforeSubmit
/// (4), Submitted (5), Abandon (6), Matches (7: `recording` the saved epoch; 1 or 0).
///
/// # Safety
/// `history` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_brd_history(
    history: *mut History,
    which: u32,
    recording: u64,
    queue_matches: bool,
) -> u64 {
    let history = unsafe { &mut *history };
    match which {
        0 => history.producers[..history.count as usize]
            .iter()
            .position(|p| p.generation == recording)
            .map_or(0, |i| i as u64 + 1),
        1 => {
            history.refresh();
            0
        }
        2 => {
            history.invalidate();
            0
        }
        3 => {
            history.written(recording);
            0
        }
        4 => {
            history.before_submit(recording, queue_matches);
            0
        }
        5 => {
            history.submitted(recording, queue_matches);
            0
        }
        6 => {
            history.abandon(recording);
            0
        }
        _ => history.matches(recording) as u64,
    }
}
/// RecordingState::BeforeSubmit (`which` 0) / Submitted (1).
///
/// # Safety
/// `state` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_brd_recording(state: *mut RecordingState, which: u32) -> bool {
    unsafe {
        if which == 0 {
            (*state).before_submit()
        } else {
            (*state).submitted()
        }
    }
}

// ---- the receipts and recordings: COM objects held as their D3D12 objects' private data

const RECEIPT_KEY: GUID = GUID::from_u128(0xcf2d37a1_14b7_42d0_951a_41b3d86e7c10);
const RECEIPT_ID_KEY: GUID = GUID::from_u128(0xcf2d37a1_14b7_42d0_951a_41b3d86e7c11);
const RECORDING_KEY: GUID = GUID::from_u128(0xcf2d37a1_14b7_42d0_951a_41b3d86e7c12);
const RECORDING_ID_KEY: GUID = GUID::from_u128(0xcf2d37a1_14b7_42d0_951a_41b3d86e7c13);
const RECEIPT_LIMIT: usize = 16384;
const RECORDING_LIMIT: usize = 4096;
const WRITE_LIMIT: usize = 4096;

struct ReceiptData {
    id: u64,
    registry: u64,
    upload_queue: u64,
    gpu: u64,
    bytes: u64,
    resource: usize,
    history: History,
    first_reason: u32,
    first_details: [u64; 4],
}
struct RecordingData {
    id: u64,
    generation: u64,
    list: usize,
    state: RecordingState,
    writes: Vec<u64>,
}
/// A COM object whose last release only retires it: the registry deletes it under its lock (DrainRetired).
#[repr(C)]
struct Held<T> {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    next: *mut Held<T>,
    data: T,
}
static RETIRED_RECEIPTS: AtomicPtr<Held<ReceiptData>> = AtomicPtr::new(core::ptr::null_mut());
static RETIRED_RECORDINGS: AtomicPtr<Held<RecordingData>> = AtomicPtr::new(core::ptr::null_mut());
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn held_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Held.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *const AtomicU32).byte_add(8)).fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn held_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Held (refs at +8).
    unsafe { (*(this as *const AtomicU32).byte_add(8)).fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn receipt_release(this: *mut c_void) -> u32 {
    let held = this as *mut Held<ReceiptData>;
    // SAFETY: a live receipt; at zero it is pushed on the retired stack, deleted later under the registry's lock.
    unsafe {
        let n = (*held).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            let mut next = RETIRED_RECEIPTS.load(Relaxed);
            loop {
                (*held).next = next;
                match RETIRED_RECEIPTS.compare_exchange_weak(next, held, Release, Relaxed) {
                    Ok(_) => break,
                    Err(actual) => next = actual,
                }
            }
        }
        n
    }
}
unsafe extern "system" fn recording_release(this: *mut c_void) -> u32 {
    let held = this as *mut Held<RecordingData>;
    // SAFETY: as receipt_release.
    unsafe {
        let n = (*held).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            let mut next = RETIRED_RECORDINGS.load(Relaxed);
            loop {
                (*held).next = next;
                match RETIRED_RECORDINGS.compare_exchange_weak(next, held, Release, Relaxed) {
                    Ok(_) => break,
                    Err(actual) => next = actual,
                }
            }
        }
        n
    }
}
static RECEIPT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: held_query,
    AddRef: held_add_ref,
    Release: receipt_release,
};
static RECORDING_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: held_query,
    AddRef: held_add_ref,
    Release: recording_release,
};

struct Registry {
    receipts: BTreeMap<u64, *mut Held<ReceiptData>>,
    recordings: BTreeMap<u64, *mut Held<RecordingData>>,
    next_receipt: u64,
    next_recording: u64,
    next_generation: u64,
    unavailable: bool,
}
// SAFETY: the raw objects are reached only under the registry's lock.
unsafe impl Send for Registry {}
static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    receipts: BTreeMap::new(),
    recordings: BTreeMap::new(),
    next_receipt: 0,
    next_recording: 0,
    next_generation: 0,
    unavailable: false,
});
fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|p| p.into_inner())
}
fn receipt<'a>(p: *mut Held<ReceiptData>) -> &'a mut ReceiptData {
    // SAFETY: a registered receipt, reached under the registry's lock.
    unsafe { &mut (*p).data }
}
fn recording<'a>(p: *mut Held<RecordingData>) -> &'a mut RecordingData {
    // SAFETY: a registered recording, reached under the registry's lock.
    unsafe { &mut (*p).data }
}

thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
    static POOL_FAILURE: Cell<u32> = const { Cell::new(0) };
    static FAILURE_DETAILS: Cell<[u64; 4]> = const { Cell::new([0; 4]) };
}

/// CvrBufferReadAttach: the C++ half's engine upload context (camera_upload::CurrentContext) and the copy windows'
/// "observer unavailable" (copy_windows::Unavailable, which every internal unavailability also reports).
#[repr(C)]
pub struct Attach {
    current_context: unsafe extern "C" fn(u64, *mut UploadContext),
    copy_unavailable: unsafe extern "C" fn(),
}
static ATTACH: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own table (formerly lent by src/Render/NativeBufferRead.cpp): camera_upload.rs' current context and
/// copy_windows.rs' unavailability. The Rust test variants attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::{Attach, UploadContext};
    unsafe extern "C" fn current_context(game: u64, out: *mut UploadContext) {
        let c = crate::camera_upload::current(game);
        // SAFETY: the caller's output.
        unsafe { *out = UploadContext { context: c.context, state: c.state, list: c.list } };
    }
    unsafe extern "C" fn copy_unavailable() {
        crate::copy_windows::cvr_cw_global(4, 0, 0);
    }
    pub static TABLE: Attach = Attach { current_context, copy_unavailable };
}
fn attach() -> Option<&'static Attach> {
    // SAFETY: the C++ half's table, alive for the process.
    let attached = unsafe { ATTACH.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::TABLE));
    attached
}
/// # Safety
/// `table` lives for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_attach(table: *const Attach) {
    ATTACH.store(table as *mut Attach, Release);
}
fn current_context(game: u64) -> UploadContext {
    let mut out = UploadContext::default();
    if let Some(t) = attach() {
        // SAFETY: the attached function.
        unsafe { (t.current_context)(game, &mut out) };
    }
    out
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Invalidation {
    Transition = 1,
    Alias,
    MissingRecording,
    Recording,
    Uploader,
    Origin,
    WriteHistory,
    BeforeRecording,
    BeforeQueue,
    BeforeHistory,
    AfterRecording,
    AfterQueue,
    AfterHistory,
    Abandon,
    AddressWrite,
}
fn enabled() -> bool {
    crate::stereo_probe::observe_bindings()
}
fn game() -> u64 {
    cvr_engine::code::image_base() as u64
}
fn first_cause(r: &mut ReceiptData, reason: Invalidation, details: [u64; 4]) {
    if r.first_reason != 0 {
        return;
    }
    r.first_reason = reason as u32;
    r.first_details = details;
}
fn invalidate(r: &mut ReceiptData, reason: Invalidation, details: [u64; 4]) {
    if r.history.valid {
        first_cause(r, reason, details);
    }
    r.history.invalidate();
}
fn abandon(reg: &Registry, rec: &RecordingData) {
    for id in &rec.writes {
        if let Some(&p) = reg.receipts.get(id) {
            let r = receipt(p);
            let (was_valid, was_producer, was_bits) =
                (r.history.valid, r.history.producer, r.history.bits());
            r.history.abandon(rec.generation);
            if was_valid && !r.history.valid {
                first_cause(
                    r,
                    Invalidation::Abandon,
                    [rec.list as u64, rec.generation, was_producer, was_bits],
                );
            }
        }
    }
}
fn drain_retired(reg: &mut Registry) {
    let mut p = RETIRED_RECEIPTS.swap(core::ptr::null_mut(), Acquire);
    while !p.is_null() {
        // SAFETY: a retired receipt (refcount 0), deleted once here.
        let next = unsafe { (*p).next };
        let id = receipt(p).id;
        if reg.receipts.get(&id) == Some(&p) {
            reg.receipts.remove(&id);
        }
        drop(unsafe { Box::from_raw(p) });
        p = next;
    }
    let mut p = RETIRED_RECORDINGS.swap(core::ptr::null_mut(), Acquire);
    while !p.is_null() {
        // SAFETY: a retired recording (refcount 0), deleted once here.
        let next = unsafe { (*p).next };
        let id = recording(p).id;
        if reg.recordings.get(&id) == Some(&p) {
            abandon(reg, recording(p));
            reg.recordings.remove(&id);
        }
        drop(unsafe { Box::from_raw(p) });
        p = next;
    }
}
fn private_id(object: &ID3D12Object, key: &GUID) -> Option<u64> {
    let mut id = 0u64;
    let mut bytes = 8u32;
    // SAFETY: at most 8 bytes written into `id`.
    unsafe { object.GetPrivateData(key, &mut bytes, Some(&mut id as *mut u64 as *mut c_void)) }
        .ok()?;
    (bytes == 8).then_some(id)
}
fn receipt_of(reg: &Registry, resource: *mut c_void) -> Option<*mut Held<ReceiptData>> {
    // SAFETY: a live resource of the caller's, or null.
    let r = unsafe { ID3D12Object::from_raw_borrowed(&resource) }?;
    let id = private_id(r, &RECEIPT_ID_KEY)?;
    let &p = reg.receipts.get(&id)?;
    (receipt(p).resource == resource as usize).then_some(p)
}
fn recording_of(reg: &Registry, list: *mut c_void) -> Option<*mut Held<RecordingData>> {
    // SAFETY: a live command list of the caller's, or null.
    let l = unsafe { ID3D12Object::from_raw_borrowed(&list) }?;
    let id = private_id(l, &RECORDING_ID_KEY)?;
    let &p = reg.recordings.get(&id)?;
    (recording(p).list == list as usize).then_some(p)
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}
fn compatible() -> bool {
    static COMPATIBLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *COMPATIBLE.get_or_init(|| {
        let game = game();
        const EXPECTED: [(u64, usize, u64); 20] = [
            (0x688F14, 0x2F1, 0xD50BD7002FEF17E1),
            (0x3DF71C, 0x6B, 0xF224A3F5039E06C6),
            (0x3445AC, 0x5A2, 0x612F7F49D216A154),
            (0x344B8C, 0xC4, 0xE3617AB0F86D80D7),
            (0x3490F4, 0x62, 0xAF5DD974E8F5ED23),
            (0x349254, 0x282, 0x172367073F0F6C1C),
            (0x3495D8, 0x226, 0xCA9BB65B64144A83),
            (0x34A688, 0x3EB, 0xF2AA1E0EF7125B55),
            // 2.5 rewrite of 2.31 sub_14021E238, not re-audited.
            (0x22F2D00, 0x384, 0x09BF1A44FA19923F),
            (0x3E5D54, 1695, 0x0FB56D2DE5F3E34A),
            (0x3E8A9C, 48, 0x356A8175F9F055FA),
            (0x3E7B90, 183, 0x64042E928C7E9B3C),
            (0x5B3DA8, 640, 0xE030F844D980B8EE),
            (0x5B4110, 77, 0xFA1D4E49FE87F2EE),
            (0x22EF830, 1493, 0x9117D2376320C0B7),
            (0x3E1020, 682, 0x898F20179715110F),
            (0x3E47E8, 382, 0xD8695C867CC19844),
            (0x3E12CC, 148, 0xA99EAC2C327A1A01),
            (0x337DA0, 0x180, 0xB36B2D868965D718),
            (0x3E3BB4, 0x34, 0x1A0F0B6F177A422C),
        ];
        let mut bytes = [0u8; 2048];
        EXPECTED.iter().all(|&(rva, len, hash)| {
            game != 0
                && game <= u64::MAX - rva
                && read(game + rva, &mut bytes[..len])
                && fnv(&bytes[..len]) == hash
        })
    })
}
fn queue_address(registry: u64, upload: bool) -> Option<u64> {
    let offset = if upload { 0x1410630 } else { 0x14104D0 };
    if registry == 0
        || registry > u64::MAX - offset - 8
        || read_value::<u64>(game() + 0x3BCDCA8)? != registry
    {
        return None;
    }
    read_value::<u64>(registry + offset).filter(|&q| q != 0)
}
fn native_queue(registry: u64, expected: u64, queue: *mut c_void, upload: bool) -> bool {
    // SAFETY: a live queue of the caller's, or null.
    let Some(q) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }) else {
        return false;
    };
    expected == queue as u64
        && queue_address(registry, upload) == Some(expected)
        && unsafe { q.GetDesc() }.Type
            == if upload {
                D3D12_COMMAND_LIST_TYPE_COPY
            } else {
                D3D12_COMMAND_LIST_TYPE_DIRECT
            }
}
fn list_type(list: *mut c_void) -> Option<D3D12_COMMAND_LIST_TYPE> {
    // SAFETY: a live command list of the caller's, or null.
    unsafe { ID3D12CommandList::from_raw_borrowed(&list) }.map(|l| unsafe { l.GetType() })
}
/// Uploader: the list is the registry's own upload context's (a copy list of kind 3) on this registry.
fn uploader(list: *mut c_void) -> (bool, u64, u64) {
    let mut registry = 0;
    let mut queue = 0;
    if list_type(list) != Some(D3D12_COMMAND_LIST_TYPE_COPY) || !compatible() {
        return (false, registry, queue);
    }
    let ok = (|| {
        registry = read_value::<u64>(game() + 0x3BCDCA8)?;
        if registry == 0 || registry > u64::MAX - 0x1410630 {
            return None;
        }
        let context = read_value::<u64>(registry + 0x1410628)?;
        if context == 0 || context > u64::MAX - 0x6C {
            return None;
        }
        if read_value::<u64>(context + 0x30)? != list as u64
            || read_value::<u32>(context + 0x68)? != 3
        {
            return None;
        }
        queue = queue_address(registry, true)?;
        Some(())
    })()
    .is_some();
    (ok, registry, queue)
}
fn context_ok(context: &UploadContext, source: &NativeBuffer) -> bool {
    context.valid()
        && context.context == source.context
        && context.context <= u64::MAX - 0x638
        && read_value::<u64>(context.context + 0x30) == Some(context.list)
        && read_value::<u32>(context.context + 0x68).is_some_and(|k| k == 1 || k == 2)
        && read_value::<u32>(context.context + 0x534) == Some(0)
}
fn pool_of_range(source: &Lease, com_offset: u64, copy_bytes: u64) -> Option<Pool> {
    let game = game();
    let mut reason = 0;
    let first = read_pool_range(
        game,
        &source.native,
        source.gpu,
        source.bytes,
        com_offset,
        copy_bytes,
        &mut reason,
    );
    POOL_FAILURE.set(reason);
    let first = first?;
    let again = read_pool_range(
        game,
        &source.native,
        source.gpu,
        source.bytes,
        com_offset,
        copy_bytes,
        &mut reason,
    );
    POOL_FAILURE.set(reason);
    (again? == first).then_some(first)
}
fn lease_resource<'a>(source: &'a Lease) -> Option<&'a ID3D12Resource> {
    // SAFETY: the lease's live resource, or null.
    unsafe { ID3D12Resource::from_raw_borrowed(&source.resource) }
}
fn buffer_shape(source: &Lease) -> bool {
    let Some(r) = lease_resource(source) else {
        return false;
    };
    // SAFETY (whole function): a live resource.
    let desc = unsafe { r.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Width == source.bytes
        && desc.Flags == D3D12_RESOURCE_FLAG_NONE
        && unsafe { r.GetGPUVirtualAddress() } == source.gpu
        && unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()
        && heap.Type == D3D12_HEAP_TYPE_DEFAULT
}
fn residency(source: &NativeBuffer) -> bool {
    let offset = 0x5C0AF8 + source.handle.wrapping_sub(1) as u64 * 0xB0;
    if source.registry > u64::MAX.wrapping_sub(offset).wrapping_sub(0x70) {
        return false;
    }
    let Some(owner) = read_value::<u64>(source.registry.wrapping_add(offset).wrapping_add(0x68))
    else {
        return false;
    };
    if owner == 0 {
        return true;
    }
    let Some(tracker) = read_value::<u64>(source.context.wrapping_add(0x630)) else {
        return false;
    };
    tracker != 0 && tracker <= u64::MAX - 38 && read_value::<u8>(tracker + 37) == Some(0)
}

// ---- the observers (the face calls the copy windows' first)

/// ObserverUnavailable: the copy windows told too, then this registry refuses everything.
fn observer_unavailable() {
    if let Some(t) = attach() {
        // SAFETY: the attached function.
        unsafe { (t.copy_unavailable)() };
    }
    registry().unavailable = true;
}
fn resource_created(resource: *mut c_void, initial: u32, placed: bool) {
    if !enabled() || cvr_host::probe::internal() || placed || !initial_state(initial) {
        return;
    }
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return;
    };
    // SAFETY (whole function): a live resource.
    let desc = unsafe { r.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || desc.Flags != D3D12_RESOURCE_FLAG_NONE
        || unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err()
        || heap.Type != D3D12_HEAP_TYPE_DEFAULT
    {
        return;
    }
    let mut reg = registry();
    drain_retired(&mut reg);
    if reg.unavailable || receipt_of(&reg, resource).is_some() {
        return;
    }
    if reg.receipts.len() >= RECEIPT_LIMIT || reg.next_receipt == u64::MAX {
        reg.unavailable = true;
        return;
    }
    reg.next_receipt += 1;
    let id = reg.next_receipt;
    let gpu = unsafe { r.GetGPUVirtualAddress() };
    let held = Box::into_raw(Box::new(Held {
        vtbl: &RECEIPT_VTBL,
        refs: AtomicU32::new(1),
        next: core::ptr::null_mut(),
        data: ReceiptData {
            id,
            registry: 0,
            upload_queue: 0,
            gpu,
            bytes: desc.Width,
            resource: resource as usize,
            history: History::new(),
            first_reason: 0,
            first_details: [0; 4],
        },
    }));
    // The creator's reference: released when this function returns (the resource's private data keeps its own).
    // SAFETY: the new object, an IUnknown.
    let owned = unsafe { IUnknown::from_raw(held.cast()) };
    if gpu == 0 || desc.Width == 0 || gpu > u64::MAX - desc.Width {
        reg.unavailable = true;
        return;
    }
    let set = unsafe {
        r.SetPrivateDataInterface(&RECEIPT_KEY, &owned).is_ok()
            && r.SetPrivateData(&RECEIPT_ID_KEY, 8, Some(&id as *const u64 as *const c_void))
                .is_ok()
    };
    if !set {
        reg.unavailable = true;
        return;
    }
    reg.receipts.insert(id, held);
    drop(reg);
    drop(owned);
}
fn barriers(list: *mut c_void, count: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
    if !enabled() || count == 0 {
        return;
    }
    if barriers.is_null() {
        observer_unavailable();
        return;
    }
    // SAFETY: the caller's `count` barriers.
    let barriers = unsafe { core::slice::from_raw_parts(barriers, count as usize) };
    let mut reg = registry();
    drain_retired(&mut reg);
    for b in barriers {
        // SAFETY: the union member the barrier's type names.
        unsafe {
            if b.Type == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION {
                let t = &b.Anonymous.Transition;
                let resource = t
                    .pResource
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw());
                if resource.is_null() {
                    reg.unavailable = true;
                    return;
                }
                if let Some(p) = receipt_of(&reg, resource) {
                    invalidate(
                        receipt(p),
                        Invalidation::Transition,
                        [
                            list as u64,
                            t.StateBefore.0 as u32 as u64,
                            t.StateAfter.0 as u32 as u64,
                            (b.Flags.0 as u64) << 32 | t.Subresource as u64,
                        ],
                    );
                }
            } else if b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING {
                // Every eligible receipt is committed, so distinct allocations cannot physically overlap. Explicit
                // references still reject.
                let a = &b.Anonymous.Aliasing;
                let before = a
                    .pResourceBefore
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw());
                let after = a
                    .pResourceAfter
                    .as_ref()
                    .map_or(core::ptr::null_mut(), |r| r.as_raw());
                let details = [list as u64, before as u64, after as u64, 0];
                if let Some(p) = receipt_of(&reg, before) {
                    invalidate(receipt(p), Invalidation::Alias, details);
                }
                if let Some(p) = receipt_of(&reg, after) {
                    invalidate(receipt(p), Invalidation::Alias, details);
                }
            }
        }
    }
}
fn written(list: *mut c_void, destination: *mut c_void) {
    if !enabled() {
        return;
    }
    let mut reg = registry();
    drain_retired(&mut reg);
    if destination.is_null() {
        reg.unavailable = true;
        return;
    }
    let Some(rp) = receipt_of(&reg, destination) else {
        return;
    };
    let r = receipt(rp);
    if !r.history.valid {
        return;
    }
    let address = list as u64;
    let Some(cp) = recording_of(&reg, list) else {
        invalidate(r, Invalidation::MissingRecording, [address, 0, 0, 0]);
        return;
    };
    let c = recording(cp);
    if !c.state.valid || c.state.pending || c.state.submitted {
        invalidate(
            r,
            Invalidation::Recording,
            [address, c.generation, c.state.bits(), 0],
        );
        return;
    }
    let (ok, registry_address, queue) = uploader(list);
    if !ok {
        let kind = list_type(list).map_or(0, |t| t.0 as u32 as u64);
        invalidate(
            r,
            Invalidation::Uploader,
            [address, kind, registry_address, queue],
        );
        return;
    }
    if r.registry != 0 && (r.registry != registry_address || r.upload_queue != queue) {
        invalidate(
            r,
            Invalidation::Origin,
            [address, c.generation, r.registry, r.upload_queue],
        );
        return;
    }
    if !c.writes.contains(&r.id) {
        if c.writes.len() >= WRITE_LIMIT {
            reg.unavailable = true;
            return;
        }
        c.writes.push(r.id);
    }
    r.registry = registry_address;
    r.upload_queue = queue;
    let (was_valid, was_producer, was_bits) =
        (r.history.valid, r.history.producer, r.history.bits());
    r.history.written(c.generation);
    if was_valid && !r.history.valid {
        first_cause(
            r,
            Invalidation::WriteHistory,
            [address, c.generation, was_producer, was_bits],
        );
    }
}
fn written_address(address: u64, bytes: u64) {
    if !enabled() || bytes == 0 {
        return;
    }
    let mut reg = registry();
    drain_retired(&mut reg);
    if address == 0 || address > u64::MAX - bytes {
        reg.unavailable = true;
        return;
    }
    for &p in reg.receipts.values() {
        let r = receipt(p);
        if address < r.gpu + r.bytes && r.gpu < address + bytes {
            let details = [address, bytes, r.gpu, r.bytes];
            invalidate(r, Invalidation::AddressWrite, details);
        }
    }
}
fn reset(list: *mut c_void) {
    if !enabled() || list.is_null() {
        return;
    }
    let ty = list_type(list);
    if ty != Some(D3D12_COMMAND_LIST_TYPE_DIRECT) && ty != Some(D3D12_COMMAND_LIST_TYPE_COPY) {
        return;
    }
    let mut reg = registry();
    drain_retired(&mut reg);
    if reg.unavailable {
        return;
    }
    if reg.next_generation == u64::MAX {
        reg.unavailable = true;
        return;
    }
    if let Some(cp) = recording_of(&reg, list) {
        abandon(&reg, recording(cp));
        reg.next_generation += 1;
        let c = recording(cp);
        c.generation = reg.next_generation;
        c.state = RecordingState::default();
        c.writes.clear();
        return;
    }
    if reg.recordings.len() >= RECORDING_LIMIT || reg.next_recording == u64::MAX {
        reg.unavailable = true;
        return;
    }
    reg.next_recording += 1;
    reg.next_generation += 1;
    let id = reg.next_recording;
    let held = Box::into_raw(Box::new(Held {
        vtbl: &RECORDING_VTBL,
        refs: AtomicU32::new(1),
        next: core::ptr::null_mut(),
        data: RecordingData {
            id,
            generation: reg.next_generation,
            list: list as usize,
            state: RecordingState::default(),
            writes: Vec::new(),
        },
    }));
    // SAFETY: the new object, an IUnknown; the list (a live ID3D12Object) keeps its own reference.
    let owned = unsafe { IUnknown::from_raw(held.cast()) };
    let l = unsafe { ID3D12Object::from_raw_borrowed(&list) }.unwrap();
    let set = unsafe {
        l.SetPrivateDataInterface(&RECORDING_KEY, &owned).is_ok()
            && l.SetPrivateData(
                &RECORDING_ID_KEY,
                8,
                Some(&id as *const u64 as *const c_void),
            )
            .is_ok()
    };
    if !set {
        reg.unavailable = true;
        return;
    }
    reg.recordings.insert(id, held);
    drop(reg);
    drop(owned);
}
fn submit_step(queue: *mut c_void, count: u32, lists: *const *mut c_void, after: bool) {
    if !enabled() || count == 0 {
        return;
    }
    if queue.is_null() || lists.is_null() {
        observer_unavailable();
        return;
    }
    // SAFETY: the caller's `count` lists.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let mut reg = registry();
    drain_retired(&mut reg);
    for &list in lists {
        let Some(cp) = recording_of(&reg, list) else {
            continue;
        };
        let c = recording(cp);
        let fresh = if after {
            c.state.submitted()
        } else {
            c.state.before_submit()
        };
        for id in c.writes.clone() {
            let Some(&p) = reg.receipts.get(&id) else {
                continue;
            };
            let r = receipt(p);
            let (was_valid, was_producer, was_bits) =
                (r.history.valid, r.history.producer, r.history.bits());
            let matches = fresh && native_queue(r.registry, r.upload_queue, queue, true);
            if after {
                r.history.submitted(c.generation, matches);
            } else {
                r.history.before_submit(c.generation, matches);
            }
            if was_valid && !r.history.valid {
                let (rec_reason, queue_reason, history_reason) = if after {
                    (
                        Invalidation::AfterRecording,
                        Invalidation::AfterQueue,
                        Invalidation::AfterHistory,
                    )
                } else {
                    (
                        Invalidation::BeforeRecording,
                        Invalidation::BeforeQueue,
                        Invalidation::BeforeHistory,
                    )
                };
                let reason = if !fresh {
                    rec_reason
                } else if !matches {
                    queue_reason
                } else {
                    history_reason
                };
                let history = matches!(
                    reason,
                    Invalidation::AfterHistory | Invalidation::BeforeHistory
                );
                first_cause(
                    r,
                    reason,
                    [
                        c.list as u64,
                        c.generation,
                        if history { was_producer } else { queue as u64 },
                        if history { was_bits } else { r.upload_queue },
                    ],
                );
            }
        }
    }
}

// ---- the proofs

fn validate_observation(
    source: &Lease,
    proof: &Proof,
    com_offset: u64,
    copy_bytes: u64,
    queue: *mut c_void,
) -> bool {
    if !enabled()
        || !compatible()
        || source.resource.is_null()
        || source.heap_type != D3D12_HEAP_TYPE_DEFAULT.0 as u32
        || source.flags != 0
        || source.native.resource != source.resource as u64
        || proof.registry != source.native.registry
        || proof.receipt == 0
        || proof.recording == 0
        || proof.generation == 0
        || proof.direct_queue == 0
        || proof.com_offset != com_offset
        || proof.copy_bytes != copy_bytes
        || !valid_range(
            &source.native,
            source.gpu,
            source.bytes,
            com_offset,
            copy_bytes,
        )
        || !buffer_shape(source)
    {
        return false;
    }
    if queue_address(proof.registry, false) != Some(proof.direct_queue)
        || pool_of_range(source, com_offset, copy_bytes) != Some(proof.pool)
        || (!queue.is_null() && !native_queue(proof.registry, proof.direct_queue, queue, false))
    {
        return false;
    }
    let mut reg = registry();
    drain_retired(&mut reg);
    let Some(rp) = receipt_of(&reg, source.resource) else {
        return false;
    };
    let r = receipt(rp);
    let Some(&cp) = reg.recordings.get(&proof.recording) else {
        return false;
    };
    let c = recording(cp);
    !reg.unavailable
        && r.id == proof.receipt
        && r.registry == proof.registry
        && r.history.matches(proof.epoch)
        && c.generation == proof.generation
        && c.state.valid
        && !c.state.submitted
        && (queue.is_null() || c.state.pending)
}
fn registered_range(proof: &Proof, com_offset: u64, copy_bytes: u64) -> bool {
    proof.read_registered
        && copy_bytes != 0
        && proof.com_offset == com_offset
        && proof.copy_bytes == copy_bytes
}
fn same_range_observation(a: &Proof, b: &Proof) -> bool {
    a.receipt != 0
        && a.epoch != 0
        && a.recording != 0
        && a.generation != 0
        && a.registry != 0
        && a.direct_queue != 0
        && a.copy_bytes != 0
        && a.receipt == b.receipt
        && a.epoch == b.epoch
        && a.recording == b.recording
        && a.generation == b.generation
        && a.registry == b.registry
        && a.direct_queue == b.direct_queue
        && a.pool == b.pool
        && a.com_offset == b.com_offset
        && a.copy_bytes == b.copy_bytes
}
fn validate_range(
    source: &Lease,
    proof: &Proof,
    com_offset: u64,
    copy_bytes: u64,
    queue: *mut c_void,
) -> bool {
    registered_range(proof, com_offset, copy_bytes)
        && validate_observation(source, proof, com_offset, copy_bytes, queue)
}
fn preflight_range(
    list: *mut c_void,
    source: &Lease,
    com_offset: u64,
    copy_bytes: u64,
) -> Option<Proof> {
    FAILURE_DETAILS.set([0; 4]);
    FAILURE.set(1);
    if !enabled()
        || list_type(list) != Some(D3D12_COMMAND_LIST_TYPE_DIRECT)
        || source.resource.is_null()
        || source.heap_type != D3D12_HEAP_TYPE_DEFAULT.0 as u32
        || source.flags != 0
        || source.native.kind != 1
        || source.native.resource != source.resource as u64
        || !valid_range(
            &source.native,
            source.gpu,
            source.bytes,
            com_offset,
            copy_bytes,
        )
        || !buffer_shape(source)
    {
        return None;
    }
    FAILURE.set(2);
    if !compatible() {
        return None;
    }
    let game = game();
    let context = current_context(game);
    FAILURE.set(3);
    if context.list != list as u64 || !context_ok(&context, &source.native) {
        return None;
    }
    let mut next = Proof {
        registry: source.native.registry,
        com_offset,
        copy_bytes,
        ..Default::default()
    };
    match pool_of_range(source, com_offset, copy_bytes) {
        Some(pool) => next.pool = pool,
        None => {
            FAILURE.set(20 + POOL_FAILURE.get());
            return None;
        }
    }
    FAILURE.set(4);
    next.direct_queue = queue_address(next.registry, false)?;
    {
        let mut reg = registry();
        drain_retired(&mut reg);
        let r = receipt_of(&reg, source.resource);
        let c = recording_of(&reg, list);
        FAILURE.set(5);
        if reg.unavailable {
            return None;
        }
        FAILURE.set(6);
        let r = receipt(r?);
        FAILURE_DETAILS.set(r.first_details);
        if !r.history.valid && r.first_reason != 0 {
            FAILURE.set(200 + r.first_reason);
            return None;
        }
        FAILURE.set(7);
        if r.registry != next.registry {
            return None;
        }
        if !r.history.matches(r.history.epoch) {
            FAILURE.set(100 + r.history.bits() as u32);
            return None;
        }
        FAILURE.set(8);
        let c = recording(c?);
        FAILURE.set(9);
        if !c.state.valid || c.state.pending || c.state.submitted {
            return None;
        }
        next.receipt = r.id;
        next.epoch = r.history.epoch;
        next.recording = c.id;
        next.generation = c.generation;
    }
    FAILURE.set(10);
    if !validate_observation(source, &next, com_offset, copy_bytes, core::ptr::null_mut())
        || current_context(game) != context
        || !context_ok(&context, &source.native)
        || !residency(&source.native)
    {
        return None;
    }
    FAILURE.set(0);
    Some(next)
}
fn capture_range(
    list: *mut c_void,
    source: &Lease,
    com_offset: u64,
    copy_bytes: u64,
    expected: Proof,
) -> Option<Proof> {
    let mut next = preflight_range(list, source, com_offset, copy_bytes)?;
    if expected != Proof::default() && !same_range_observation(&expected, &next) {
        FAILURE.set(13);
        return None;
    }
    let game = game();
    let context = current_context(game);
    FAILURE.set(10);
    if context.list != list as u64
        || !context_ok(&context, &source.native)
        || !validate_observation(source, &next, com_offset, copy_bytes, core::ptr::null_mut())
    {
        return None;
    }
    // Class 1 registers allocation use but skips the native transition maps. Both the source copy and the following
    // CBV draw use cumulative read promotion.
    {
        let _internal = cvr_host::probe::Internal::new();
        type Register = unsafe extern "C" fn(*mut c_void, u32, u32);
        // SAFETY: the engine's read registration (fingerprinted by compatible()), on its current upload context.
        unsafe {
            let register: Register = core::mem::transmute(game as usize + 0x337DA0);
            register(
                context.context as *mut c_void,
                source.native.handle,
                (D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER | D3D12_RESOURCE_STATE_COPY_SOURCE)
                    .0 as u32,
            );
        }
    }
    next.read_registered = true;
    FAILURE.set(11);
    if current_context(game) != context
        || !context_ok(&context, &source.native)
        || !residency(&source.native)
        || !validate_range(source, &next, com_offset, copy_bytes, core::ptr::null_mut())
    {
        return None;
    }
    FAILURE.set(0);
    Some(next)
}

// ---- the C ABI (include/Render/NativeBufferRead.hpp in the Rust build)

#[no_mangle]
pub extern "C" fn cvr_buffer_read_unavailable() {
    observer_unavailable();
}
/// # Safety
/// `resource` is a live resource or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_resource_created(
    resource: *mut c_void,
    initial: u32,
    placed: bool,
) {
    resource_created(resource, initial, placed);
}
/// # Safety
/// `items` holds `count` barriers (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_barriers(
    list: *mut c_void,
    count: u32,
    items: *const D3D12_RESOURCE_BARRIER,
) {
    barriers(list, count, items);
}
/// # Safety
/// `list` and `destination` are live objects or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_written(list: *mut c_void, destination: *mut c_void) {
    written(list, destination);
}
#[no_mangle]
pub extern "C" fn cvr_buffer_read_written_address(address: u64, bytes: u64) {
    written_address(address, bytes);
}
/// # Safety
/// `list` is a live command list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_reset(list: *mut c_void) {
    reset(list);
}
/// # Safety
/// `lists` holds `count` command lists (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_before_submit(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    submit_step(queue, count, lists, false);
}
/// # Safety
/// As cvr_buffer_read_before_submit.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_submitted(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    submit_step(queue, count, lists, true);
}
#[no_mangle]
pub extern "C" fn cvr_buffer_read_last_failure() -> u32 {
    FAILURE.get()
}
/// # Safety
/// `out` holds four values.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_last_failure_details(out: *mut [u64; 4]) {
    // SAFETY: the caller's output.
    unsafe { *out = FAILURE_DETAILS.get() };
}
/// # Safety
/// `source` and `proof` are valid; `queue` a live queue or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_validate_range(
    source: *const Lease,
    proof: *const Proof,
    com_offset: u64,
    copy_bytes: u64,
    queue: *mut c_void,
) -> bool {
    // SAFETY: as the caller promises.
    let (source, proof) = unsafe { (&*source, &*proof) };
    validate_range(source, proof, com_offset, copy_bytes, queue)
}
/// # Safety
/// `source` is valid; `out` receives the proof (cleared on refusal).
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_preflight_range(
    list: *mut c_void,
    source: *const Lease,
    com_offset: u64,
    copy_bytes: u64,
    out: *mut Proof,
) -> bool {
    // SAFETY: as the caller promises.
    let (source, out) = unsafe { (&*source, &mut *out) };
    *out = Proof::default();
    match preflight_range(list, source, com_offset, copy_bytes) {
        Some(proof) => {
            *out = proof;
            true
        }
        None => false,
    }
}
/// # Safety
/// `source` is valid; `out` holds the expected proof (or all zero) and receives the captured one.
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_capture_range(
    list: *mut c_void,
    source: *const Lease,
    com_offset: u64,
    copy_bytes: u64,
    out: *mut Proof,
) -> bool {
    // SAFETY: as the caller promises.
    let (source, out) = unsafe { (&*source, &mut *out) };
    let expected = *out;
    *out = Proof::default();
    match capture_range(list, source, com_offset, copy_bytes, expected) {
        Some(proof) => {
            *out = proof;
            true
        }
        None => false,
    }
}
/// Capture: the 292-byte camera block at the native buffer's own offset.
///
/// # Safety
/// `source` is valid; `out` receives the proof (cleared first).
#[no_mangle]
pub unsafe extern "C" fn cvr_buffer_read_capture(
    list: *mut c_void,
    source: *const Lease,
    out: *mut Proof,
) -> bool {
    // SAFETY: as the caller promises.
    let (source, out) = unsafe { (&*source, &mut *out) };
    *out = Proof::default();
    FAILURE_DETAILS.set([0; 4]);
    FAILURE.set(1);
    if source.native.bytes < 292 || source.native.gpu < source.gpu {
        return false;
    }
    match capture_range(
        list,
        source,
        source.native.gpu - source.gpu,
        292,
        Proof::default(),
    ) {
        Some(proof) => {
            *out = proof;
            true
        }
        None => false,
    }
}

/// The cases of tools/single_pass_tests/buffer_read_state_test.cpp over this History (the C++ header's detail::History).
#[cfg(test)]
mod tests {
    use super::History;

    fn submit(h: &mut History, recording: u64) {
        h.before_submit(recording, true);
        assert!(h.valid && h.pending && !h.matches(h.epoch));
        h.submitted(recording, true);
        assert!(h.matches(h.epoch));
    }
    fn uploaded() -> History {
        let mut h = History::new();
        h.written(101);
        submit(&mut h, 101);
        h
    }
    fn active() -> History {
        let mut h = History::new();
        h.written(101);
        h.written(202);
        h.written(303);
        h
    }

    #[test]
    fn upload_epochs() {
        let mut h = History::new();
        assert!(h.valid && h.epoch != 0 && !h.matches(h.epoch));
        let initial = h.epoch;
        h.written(101);
        assert!(h.valid && h.epoch == initial + 1 && !h.matches(h.epoch));
        h.written(101);
        assert!(h.valid && h.epoch == initial + 2);
        submit(&mut h, 101);
        let first = h.epoch;
        assert!(!h.matches(0) && !h.matches(first - 1) && !h.matches(first + 1));
        for recording in [202u64, 303, 404] {
            let previous = h.epoch;
            h.written(recording);
            assert!(h.valid && h.epoch == previous + 1 && !h.matches(previous));
            h.written(recording);
            assert!(h.valid && h.epoch == previous + 2);
            submit(&mut h, recording);
            assert!(!h.matches(previous) && !h.matches(first));
        }
        let saved = h.epoch;
        h.invalidate();
        h.written(505);
        h.before_submit(505, true);
        h.submitted(505, true);
        assert!(!h.valid && !h.matches(saved) && h.epoch == saved);
    }

    #[test]
    fn invalid_upload_histories() {
        let mut h = History::new();
        h.written(0);
        h.written(101);
        assert!(!h.valid);
        let mut h = History::new();
        h.written(101);
        h.before_submit(101, true);
        h.written(101);
        assert!(!h.valid && !h.matches(h.epoch));
        let mut h = History::new();
        h.epoch = u64::MAX;
        h.written(101);
        assert!(!h.valid && h.epoch == u64::MAX);
        let mut h = History::new();
        h.before_submit(101, true);
        assert!(!h.valid);
        for recording in [0u64, 202] {
            let mut before = History::new();
            before.written(101);
            before.before_submit(recording, true);
            assert!(!before.valid);
            let mut after = History::new();
            after.written(101);
            after.before_submit(101, true);
            after.submitted(recording, true);
            assert!(!after.valid && !after.matches(after.epoch));
        }
        let mut h = History::new();
        h.written(101);
        h.before_submit(101, false);
        h.submitted(101, true);
        assert!(!h.valid);
        let mut h = History::new();
        h.written(101);
        h.before_submit(101, true);
        h.submitted(101, false);
        assert!(!h.valid);
        let mut h = History::new();
        h.written(101);
        h.submitted(101, true);
        assert!(!h.valid);
        let mut h = History::new();
        h.written(101);
        h.before_submit(101, true);
        h.before_submit(101, true);
        h.submitted(101, true);
        assert!(!h.valid);
        let mut h = uploaded();
        h.submitted(101, true);
        assert!(!h.valid);
        let mut h = uploaded();
        h.before_submit(101, true);
        assert!(!h.valid);
        let mut h = uploaded();
        h.written(101);
        assert!(!h.valid);
        for pending in [false, true] {
            let mut h = History::new();
            h.written(101);
            if pending {
                h.before_submit(101, true);
            }
            h.abandon(202);
            assert!(h.valid);
            h.abandon(101);
            h.submitted(101, true);
            assert!(!h.valid);
        }
        let mut h = uploaded();
        let saved = h.epoch;
        h.abandon(0);
        h.abandon(202);
        h.abandon(101);
        assert!(h.matches(saved));
    }

    #[test]
    fn producer_orders() {
        let orders = [
            [101u64, 202, 303],
            [101, 303, 202],
            [202, 101, 303],
            [202, 303, 101],
            [303, 101, 202],
            [303, 202, 101],
        ];
        for order in orders {
            for batch in [false, true] {
                let mut h = History::new();
                let initial = h.epoch;
                for recording in [101u64, 202, 303] {
                    h.written(recording);
                }
                assert!(h.valid && h.epoch == initial + 3 && !h.uploaded);
                let saved = h.epoch;
                assert!(!h.matches(saved));
                if batch {
                    for recording in [101u64, 202, 303] {
                        h.before_submit(recording, true);
                        assert!(h.valid && h.pending && !h.uploaded);
                    }
                }
                for (i, &recording) in order.iter().enumerate() {
                    if !batch {
                        h.before_submit(recording, true);
                    }
                    assert!(h.valid && h.pending && !h.matches(saved));
                    h.submitted(recording, true);
                    let complete = i + 1 == order.len();
                    assert!(h.valid && h.uploaded == complete && h.matches(saved) == complete);
                    assert!(h.pending == (batch && !complete));
                }
            }
        }
        let mut g = History::new();
        g.written(101);
        g.before_submit(101, true);
        let mut previous = g.epoch;
        g.written(202);
        assert!(g.valid && g.pending && g.epoch == previous + 1 && !g.matches(previous));
        previous = g.epoch;
        g.written(202);
        assert!(g.valid && g.pending && g.epoch == previous + 1);
        g.before_submit(202, true);
        previous = g.epoch;
        g.written(303);
        assert!(g.valid && g.pending && g.epoch == previous + 1);
        g.submitted(202, true);
        g.abandon(202);
        assert!(g.valid && g.pending && !g.uploaded);
        g.submitted(101, true);
        g.abandon(101);
        assert!(g.valid && !g.pending && !g.uploaded && !g.matches(g.epoch));
        previous = g.epoch;
        g.written(303);
        assert!(g.valid && g.epoch == previous + 1);
        submit(&mut g, 303);
    }

    #[test]
    fn invalid_producer_cohorts() {
        for recording in [0u64, 404] {
            let mut before = active();
            before.before_submit(recording, true);
            assert!(!before.valid);
            let mut after = active();
            after.before_submit(101, true);
            after.submitted(recording, true);
            assert!(!after.valid);
        }
        for recording in [101u64, 202, 303] {
            let mut before = active();
            before.before_submit(recording, false);
            assert!(!before.valid);
            let mut after = active();
            after.before_submit(recording, true);
            after.submitted(recording, false);
            assert!(!after.valid);
            let mut missing = active();
            missing.submitted(recording, true);
            assert!(!missing.valid);
            let mut duplicate = active();
            duplicate.before_submit(recording, true);
            duplicate.before_submit(recording, true);
            assert!(!duplicate.valid);
            let mut pending_write = active();
            pending_write.before_submit(recording, true);
            pending_write.written(recording);
            assert!(!pending_write.valid);
            let mut completed = active();
            completed.before_submit(recording, true);
            completed.submitted(recording, true);
            assert!(completed.valid && !completed.uploaded);
            let mut replay = completed.clone();
            replay.before_submit(recording, true);
            assert!(!replay.valid);
            let mut duplicate_after = completed.clone();
            duplicate_after.submitted(recording, true);
            assert!(!duplicate_after.valid);
            completed.written(recording);
            assert!(!completed.valid);
        }
        for pending in [false, true] {
            let mut h = active();
            h.before_submit(101, true);
            h.submitted(101, true);
            if pending {
                h.before_submit(202, true);
            }
            h.abandon(101);
            h.abandon(404);
            assert!(h.valid);
            h.abandon(202);
            assert!(!h.valid && !h.matches(h.epoch));
        }
    }

    #[test]
    fn producer_capacity_and_reuse() {
        let mut full = History::new();
        let initial = full.epoch;
        for recording in 1..=128u64 {
            full.written(recording);
        }
        assert!(full.valid && full.epoch == initial + 128 && !full.uploaded);
        full.written(64);
        assert!(full.valid && full.epoch == initial + 129);
        let mut overflow = full.clone();
        overflow.written(129);
        assert!(!overflow.valid && !overflow.matches(overflow.epoch));
        full.before_submit(1, true);
        full.submitted(1, true);
        assert!(full.valid && !full.uploaded);
        full.abandon(1);
        assert!(full.valid);
        let mut partial = full.clone();
        partial.written(129);
        assert!(!partial.valid);
        for recording in (2..=128u64).rev() {
            full.before_submit(recording, true);
            full.submitted(recording, true);
            assert!(full.valid && full.uploaded == (recording == 2));
        }
        let saved = full.epoch;
        assert!(full.matches(saved));
        let mut replay = full.clone();
        replay.written(128);
        assert!(!replay.valid);
    }
}
