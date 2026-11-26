//! THE NATIVE COMMAND RECORDING CONTRACT (formerly src/Render/NativeCommandRecording.cpp and the detail functions of
//! include/Render/NativeCommandRecording.hpp): an audited native ownership contract for the engine's direct command
//! lists, not GPU-safe support for replay through a foreign caller.
//!
//! Every native Reset of a direct list (from the engine's own reset site) gives the list a fresh token, held by the list
//! as private data; a probe captures the token while the list records in the engine's current upload context, and may
//! validate it at any later point of the recording and at the list's one submission -- from the engine's own
//! ExecuteCommandLists site (directly, or through a wrapper whose OS-unwound frames reach it), on the registry's direct
//! queue, the list present once. A second reset, a second submission or anything else rejects the token for good.
//!
//! Failures (LastFailure, per thread): 1 arguments, 2 fingerprints, 3 missing token, 4 reset origin, 5 generation, 6
//! native context, 7 registry, 8 submit origin/queue, 9 replay, 10 token storage, 11 allocation/exception, 12 not
//! pending. Details: Execute caller, expected caller, actual/expected queue, occurrences, pending | submitted << 32 |
//! rejected << 33, Reset caller, generation.
use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::*};
use std::sync::{Mutex, OnceLock};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandList, ID3D12CommandQueue, D3D12_COMMAND_LIST_TYPE_DIRECT,
};

// ---- the contract's pure parts (the header's detail namespace) ----------------------------------------------------

pub const RESET_RETURN: u32 = 0x3E626A;
pub const EXECUTE_RETURN: u32 = 0x3E124F;
/// One audited function: its RVA, length and FNV-1a of its bytes.
#[derive(Clone, Copy)]
pub struct Code {
    pub rva: u32,
    pub bytes: u32,
    pub hash: u64,
}
/// Acquire/reset, handle ownership transfer, bulk/single submit, split fallback, context return and queue selection
/// (2.5, audited 2026-10-23: the context layout unchanged, the Reset and ExecuteCommandLists return sites, the queue at
/// registry + 0x14104D0).
pub const CONTRACT: [Code; 11] = [
    Code {
        rva: 0x3E5D54,
        bytes: 1695,
        hash: 0x0FB56D2DE5F3E34A,
    },
    Code {
        rva: 0x3E79C8,
        bytes: 239,
        hash: 0x6B24682AECE56AD1,
    },
    Code {
        rva: 0x22EF830,
        bytes: 1493,
        hash: 0x9117D2376320C0B7,
    },
    Code {
        rva: 0x22EF630,
        bytes: 0x1E6,
        hash: 0x8693CD8B6182484A,
    },
    Code {
        rva: 0x3E0B28,
        bytes: 1269,
        hash: 0xC8443F2F3CE39882,
    },
    Code {
        rva: 0x5B3DA8,
        bytes: 640,
        hash: 0xE030F844D980B8EE,
    },
    Code {
        rva: 0x3E1020,
        bytes: 682,
        hash: 0x898F20179715110F,
    },
    Code {
        rva: 0x2469DCE,
        bytes: 206,
        hash: 0x2C1F0022E230A82C,
    },
    Code {
        rva: 0x3E1360,
        bytes: 403,
        hash: 0x9C8B7DC7F6242F45,
    },
    Code {
        rva: 0x3E14F4,
        bytes: 167,
        hash: 0x13230B2BADF1035B,
    },
    Code {
        rva: 0x5B4110,
        bytes: 77,
        hash: 0xFA1D4E49FE87F2EE,
    },
];
/// FNV-1a 64 of `bytes`.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |hash, &b| {
        (hash ^ b as u64).wrapping_mul(1099511628211)
    })
}
/// `address` is the game image's `rva`.
pub fn caller(base: u64, address: u64, rva: u32) -> bool {
    base != 0 && base <= u64::MAX - rva as u64 && address == base + rva as u64
}
pub const MAX_UNWOUND_FRAMES: usize = 64;
/// The submission comes from the engine's ExecuteCommandLists site: the caller itself when it is in the game image,
/// else the first of the OS-unwound frames (nearest to farthest) that is.
pub fn execute_origin(base: u64, image_bytes: u64, caller_address: u64, frames: &[u64]) -> bool {
    if base == 0
        || image_bytes <= EXECUTE_RETURN as u64
        || image_bytes > u64::MAX - base
        || frames.len() > MAX_UNWOUND_FRAMES
    {
        return false;
    }
    let in_game = |address: u64| address >= base && address - base < image_bytes;
    if in_game(caller_address) {
        return caller(base, caller_address, EXECUTE_RETURN);
    }
    frames
        .iter()
        .find(|&&frame| in_game(frame))
        .is_some_and(|&frame| caller(base, frame, EXECUTE_RETURN))
}
/// camera_upload::Context (include/Render/NativeCameraBindingState.hpp).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    pub context: u64,
    pub state: u64,
    pub list: u64,
}
impl Context {
    pub fn valid(&self) -> bool {
        self.context != 0 && self.state != 0 && self.list != 0
    }
}
/// The list records in the expected native context: its list and state, a direct or copy kind, not yet submitted.
pub fn context_matches(
    expected: Context,
    list: u64,
    actual_list: u64,
    state: u64,
    kind: u32,
    submitted: u64,
) -> bool {
    expected.valid()
        && list != 0
        && expected.list == list
        && actual_list == list
        && expected.state == state
        && (kind == 1 || kind == 2)
        && submitted == 0
}
/// A token's submission: pending ExecuteCommandLists, submitted, rejected for good.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SubmissionState {
    pub pending: u32,
    pub submitted: bool,
    pub rejected: bool,
}
const _: () = assert!(core::mem::size_of::<SubmissionState>() == 8);
impl SubmissionState {
    pub fn fresh(&self) -> bool {
        self.pending == 0 && !self.submitted && !self.rejected
    }
    pub fn before_submit(&mut self, occurrences: u32, native: bool) -> bool {
        if occurrences == 0 {
            return false;
        }
        let first = self.fresh();
        if self.pending != u32::MAX {
            self.pending += 1;
        } else {
            self.rejected = true;
        }
        if !first || occurrences != 1 || !native {
            self.rejected = true;
        }
        self.validating()
    }
    pub fn validating(&self) -> bool {
        self.pending == 1 && !self.submitted && !self.rejected
    }
    pub fn submitted(&mut self) {
        if self.pending != 0 {
            self.pending -= 1;
            self.submitted = true;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_fingerprint(bytes: *const u8, len: usize) -> u64 {
    // SAFETY: the caller's bytes.
    fingerprint(if bytes.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(bytes, len) }
    })
}
#[no_mangle]
pub extern "C" fn cvr_native_recording_caller(base: u64, address: u64, rva: u32) -> bool {
    caller(base, address, rva)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_execute_origin(
    base: u64,
    image_bytes: u64,
    caller_address: u64,
    frames: *const u64,
    count: usize,
) -> bool {
    // SAFETY: the caller's frames.
    let frames = if frames.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(frames, count) }
    };
    execute_origin(base, image_bytes, caller_address, frames)
}
#[no_mangle]
pub extern "C" fn cvr_native_recording_context_matches(
    expected: Context,
    list: u64,
    actual_list: u64,
    state: u64,
    kind: u32,
    submitted: u64,
) -> bool {
    context_matches(expected, list, actual_list, state, kind, submitted)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_before_submit_state(
    state: *mut SubmissionState,
    occurrences: u32,
    native: bool,
) -> bool {
    // SAFETY: the caller's state.
    unsafe { state.as_mut() }.is_some_and(|s| s.before_submit(occurrences, native))
}
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_submitted_state(state: *mut SubmissionState) {
    // SAFETY: the caller's state.
    if let Some(s) = unsafe { state.as_mut() } {
        s.submitted();
    }
}

// ---- the tokens ----------------------------------------------------------------------------------------------------

const OWNER_KEY: GUID = GUID::from_values(
    0x39721d23,
    0x9ccf,
    0x4793,
    [0x88, 0x43, 0x18, 0x66, 0x7b, 0x0a, 0x6e, 0x10],
);
const TOKEN_TYPE: GUID = GUID::from_values(
    0x39721d23,
    0x9ccf,
    0x4793,
    [0x88, 0x43, 0x18, 0x66, 0x7b, 0x0a, 0x6e, 0x11],
);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

/// A recording's token (its fields are touched only under STATE's lock).
#[derive(Default)]
struct TokenState {
    generation: u64,
    context: u64,
    list: u64,
    registry: u64,
    native_reset: bool,
    submission: SubmissionState,
    queue: usize,
    observed: [u64; 8],
    rejection: u32,
}
#[repr(C)]
struct Token {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    active: AtomicBool,
    state: UnsafeCell<TokenState>,
}
// SAFETY: `state` is touched only under the module's lock; the rest is atomic.
unsafe impl Sync for Token {}
/// The list's private data: deactivates its token when the list drops it (a reset or the list's end).
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    token: *mut Token,
}

unsafe fn com_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
    own: &GUID,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live object of ours.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != *own) {
            return E_NOINTERFACE;
        }
        *out = this;
        ((*(*(this as *const *const IUnknown_Vtbl))).AddRef)(this);
    }
    HRESULT(0)
}
unsafe extern "system" fn token_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: as com_query.
    unsafe { com_query(this, iid, out, &TOKEN_TYPE) }
}
unsafe extern "system" fn token_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Token.
    unsafe { (*(this as *const Token)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn token_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Token; the last release frees it (it came from Box::into_raw).
    unsafe {
        let n = (*(this as *const Token)).refs.fetch_sub(1, AcqRel) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Token));
        }
        n
    }
}
static TOKEN_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: token_query,
    AddRef: token_add_ref,
    Release: token_release,
};
unsafe extern "system" fn owner_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: as com_query.
    unsafe { com_query(this, iid, out, &OWNER_KEY) }
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Owner.
    unsafe { (*(this as *const Owner)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Owner; the last release deactivates its token and frees it.
    unsafe {
        let owner = this as *mut Owner;
        let n = (*owner).refs.fetch_sub(1, AcqRel) - 1;
        if n == 0 {
            let token = (*owner).token;
            (*token).active.store(false, Release);
            token_release(token.cast());
            drop(Box::from_raw(owner));
        }
        n
    }
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: owner_query,
    AddRef: owner_add_ref,
    Release: owner_release,
};

/// A held reference to one of our objects, released when dropped.
struct Held<T>(*mut T);
impl<T> Held<T> {
    fn get(&self) -> &T {
        // SAFETY: a held reference.
        unsafe { &*self.0 }
    }
}
impl<T> Drop for Held<T> {
    fn drop(&mut self) {
        // SAFETY: a held reference of a COM object of ours.
        unsafe { ((*(*(self.0 as *const *const IUnknown_Vtbl))).Release)(self.0.cast()) };
    }
}
impl Token {
    /// The token's state; only under the module's lock.
    #[allow(clippy::mut_from_ref)]
    fn s(&self, _lock: &Locked) -> &mut TokenState {
        // SAFETY: the lock serialises every access.
        unsafe { &mut *self.state.get() }
    }
}
/// `unknown` queried for one of our types.
fn query<T>(unknown: *mut c_void, iid: &GUID) -> Option<Held<T>> {
    if unknown.is_null() {
        return None;
    }
    let mut out = core::ptr::null_mut();
    // SAFETY: a live COM object.
    let hr = unsafe {
        ((*(*(unknown as *const *const IUnknown_Vtbl))).QueryInterface)(unknown, iid, &mut out)
    };
    (hr.is_ok() && !out.is_null()).then(|| Held(out as *mut T))
}
/// The list's recording owner (its private data).
fn recording(list: *mut c_void) -> Option<Held<Owner>> {
    // SAFETY: a live command list.
    let list = unsafe { ID3D12CommandList::from_raw_borrowed(&list) }?;
    let mut bytes = core::mem::size_of::<*mut c_void>() as u32;
    let mut unknown: *mut c_void = core::ptr::null_mut();
    // SAFETY: the list's private data: an interface pointer, referenced for us.
    let ok =
        unsafe { list.GetPrivateData(&OWNER_KEY, &mut bytes, Some((&raw mut unknown).cast())) }
            .is_ok();
    if !ok || bytes as usize != core::mem::size_of::<*mut c_void>() || unknown.is_null() {
        return None;
    }
    // SAFETY: the reference GetPrivateData added, released once queried.
    let unknown = unsafe { IUnknown::from_raw(unknown) };
    query::<Owner>(unknown.as_raw(), &OWNER_KEY)
}

// ---- the module's state --------------------------------------------------------------------------------------------

struct Locked {
    next_generation: u64,
}
static STATE: Mutex<Locked> = Mutex::new(Locked { next_generation: 0 });
thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
    static DETAILS: Cell<[u64; 8]> = const { Cell::new([0; 8]) };
}
fn set_failure(value: u32) {
    FAILURE.with(|f| f.set(value));
}
fn set_details(value: [u64; 8]) {
    DETAILS.with(|d| d.set(value));
}

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| crate::stereo_probe::cvr_observe_bindings())
}
fn base() -> u64 {
    // SAFETY: the process's own module.
    unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
        .map_or(0, |h| h.0 as u64)
}
fn read(address: u64, out: &mut [u8]) -> bool {
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut copied = 0usize;
    address != 0
        && address <= u64::MAX - out.len() as u64
        // SAFETY: a guarded read of this process's memory.
        && unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as *const c_void,
                out.as_mut_ptr().cast(),
                out.len(),
                Some(&mut copied),
            )
        }
        .is_ok()
        && copied == out.len()
}
fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn read_u32(address: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read(address, &mut b).then(|| u32::from_le_bytes(b))
}
fn image_bytes() -> u64 {
    static BYTES: OnceLock<u64> = OnceLock::new();
    *BYTES.get_or_init(|| {
        let base = base();
        let mut dos = [0u8; 64];
        if !read(base, &mut dos) || u16::from_le_bytes([dos[0], dos[1]]) != 0x5A4D {
            return 0;
        }
        let lfanew = i32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]);
        if lfanew <= 0 || lfanew > 0x100000 || base > u64::MAX - lfanew as u64 {
            return 0;
        }
        // IMAGE_NT_HEADERS64: Signature, FileHeader (Machine at +4), OptionalHeader at +24 (Magic, SizeOfImage at +56).
        let mut nt = [0u8; 264];
        if !read(base + lfanew as u64, &mut nt) {
            return 0;
        }
        let signature = u32::from_le_bytes([nt[0], nt[1], nt[2], nt[3]]);
        let machine = u16::from_le_bytes([nt[4], nt[5]]);
        let magic = u16::from_le_bytes([nt[24], nt[25]]);
        let size = u32::from_le_bytes([nt[80], nt[81], nt[82], nt[83]]) as u64;
        if signature != 0x4550
            || machine != 0x8664
            || magic != 0x20B
            || size <= EXECUTE_RETURN as u64
            || size > u64::MAX - base
        {
            return 0;
        }
        size
    })
}
fn unwound_execute_origin(base: u64, caller_address: usize) -> bool {
    let bytes = image_bytes();
    if bytes == 0 || (caller_address as u64 >= base && caller_address as u64 - base < bytes) {
        return false;
    }
    let mut addresses = [core::ptr::null_mut::<c_void>(); MAX_UNWOUND_FRAMES];
    // SAFETY: a plain stack walk into the local array.
    let count = unsafe {
        windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(
            0,
            &mut addresses,
            None,
        )
    } as usize;
    let frames: Vec<u64> = addresses[..count].iter().map(|&a| a as u64).collect();
    execute_origin(base, bytes, caller_address as u64, &frames)
}
fn registry() -> u64 {
    let base = base();
    if base == 0 || base > u64::MAX - 0x3BCDCB0 {
        return 0;
    }
    read_u64(base + 0x3BCDCA8).unwrap_or(0)
}
fn same_lease(token: &Token, s: &TokenState, lease: &Lease) -> bool {
    token.active.load(Acquire)
        && s.native_reset
        && s.generation == lease.generation
        && s.context == lease.context
        && s.list == lease.list
        && s.registry == lease.registry
}
fn native_queue(registry_address: u64, queue: *mut c_void, output: Option<&mut u64>) -> bool {
    let expected = if registry_address != 0 && registry_address <= u64::MAX - 0x14104D8 {
        read_u64(registry_address + 0x14104D0)
    } else {
        None
    };
    if let Some(o) = output {
        *o = expected.unwrap_or(0);
    }
    let Some(expected) = expected else {
        return false;
    };
    // SAFETY: a live queue.
    let Some(q) = (unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }) else {
        return false;
    };
    registry_address == registry()
        && expected == queue as u64
        // SAFETY: a live queue.
        && unsafe { q.GetDesc() }.Type == D3D12_COMMAND_LIST_TYPE_DIRECT
        && registry_address == registry()
}
fn submission_bits(s: &SubmissionState) -> u64 {
    s.pending as u64 | (s.submitted as u64) << 32 | (s.rejected as u64) << 33
}
fn current(context: Context, list: u64) -> bool {
    context.context != 0
        && context.context <= u64::MAX - 0x640
        && (|| {
            let actual = read_u64(context.context + 0x30)?;
            let state = read_u64(context.context + 0x60)?;
            let kind = read_u32(context.context + 0x68)?;
            let submitted = read_u64(context.context + 0x638)?;
            Some(context_matches(
                context, list, actual, state, kind, submitted,
            ))
        })()
        .unwrap_or(false)
}
fn direct(list: *mut c_void) -> bool {
    // SAFETY: a live list.
    unsafe { ID3D12CommandList::from_raw_borrowed(&list) }
        .is_some_and(|l| unsafe { l.GetType() } == D3D12_COMMAND_LIST_TYPE_DIRECT)
}

/// The audited functions are this build's (checked once).
#[no_mangle]
pub extern "C" fn cvr_native_recording_compatible() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        let base = base();
        let mut bytes = vec![0u8; 2048];
        CONTRACT.iter().all(|code| {
            base != 0
                && base <= u64::MAX - code.rva as u64
                && code.bytes as usize <= bytes.len()
                && read(base + code.rva as u64, &mut bytes[..code.bytes as usize])
                && fingerprint(&bytes[..code.bytes as usize]) == code.hash
        })
    })
}
#[no_mangle]
pub extern "C" fn cvr_native_recording_last_failure() -> u32 {
    FAILURE.with(Cell::get)
}
/// # Safety
/// `out` holds eight values.
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_last_details(out: *mut [u64; 8]) {
    // SAFETY: the caller's array.
    if let Some(o) = unsafe { out.as_mut() } {
        *o = DETAILS.with(Cell::get);
    }
}

/// A native Reset of a direct list: its previous token deactivated, a fresh one -- native when the reset came from the
/// engine's own site -- held by the list.
#[no_mangle]
pub extern "C" fn cvr_native_recording_reset(list: *mut c_void, caller_address: usize) {
    if !enabled() || list.is_null() || !direct(list) {
        return;
    }
    let mut lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    set_failure(10);
    set_details([0; 8]);
    if let Some(previous) = recording(list) {
        // SAFETY: the owner's token, alive while the owner is.
        unsafe { (*previous.get().token).active.store(false, Release) };
    }
    if lock.next_generation == u64::MAX {
        return;
    }
    lock.next_generation += 1;
    let generation = lock.next_generation;
    let token = Box::into_raw(Box::new(Token {
        vtbl: &TOKEN_VTBL,
        refs: AtomicU32::new(1),
        active: AtomicBool::new(true),
        state: UnsafeCell::new(TokenState {
            generation,
            list: list as u64,
            registry: registry(),
            native_reset: cvr_native_recording_compatible()
                && caller(base(), caller_address as u64, RESET_RETURN),
            observed: [0, 0, 0, 0, 0, 0, caller_address as u64, generation],
            ..Default::default()
        }),
    }));
    let owner = Box::into_raw(Box::new(Owner {
        vtbl: &OWNER_VTBL,
        refs: AtomicU32::new(1),
        token,
    }));
    // SAFETY: the owner's one reference, handed to the IUnknown that releases it.
    let owner = unsafe { IUnknown::from_raw(owner.cast()) };
    // SAFETY: a live command list (checked above).
    let list = unsafe { ID3D12CommandList::from_raw_borrowed(&list) }.unwrap();
    // SAFETY: the list keeps its own reference to the owner.
    if unsafe { list.SetPrivateDataInterface(&OWNER_KEY, Some(&owner)) }.is_err() {
        return;
    }
    set_failure(0);
}

/// Lease (include/Render/NativeCommandRecording.hpp): the token (an IUnknown reference) and the recording it labels.
#[repr(C)]
pub struct Lease {
    pub token: *mut c_void,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
const _: () = assert!(core::mem::size_of::<Lease>() == 40);

/// Capture: the list's native token in the engine's current upload context, labelled with it; `out` gets a reference
/// to the token.
///
/// # Safety
/// `context` and `out` are valid; `list` a live command list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_capture(
    list: *mut c_void,
    context: *const Context,
    out: *mut Lease,
) -> bool {
    // SAFETY: the caller's records.
    let (Some(&context), Some(out)) = (unsafe { context.as_ref() }, unsafe { out.as_mut() }) else {
        return false;
    };
    *out = Lease {
        token: core::ptr::null_mut(),
        generation: 0,
        context: 0,
        list: 0,
        registry: 0,
    };
    set_failure(1);
    set_details([0; 8]);
    if !enabled() || list.is_null() || !context.valid() || !direct(list) {
        return false;
    }
    set_failure(2);
    if !cvr_native_recording_compatible() {
        return false;
    }
    let lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    set_failure(3);
    let Some(owner) = recording(list) else {
        return false;
    };
    // SAFETY: the owner's token, alive while the owner is.
    let token = unsafe { &*owner.get().token };
    let s = token.s(&lock);
    let mut details = s.observed;
    details[5] = submission_bits(&s.submission);
    set_details(details);
    set_failure(4);
    if !s.native_reset {
        return false;
    }
    set_failure(5);
    if !token.active.load(Acquire) || !s.submission.fresh() {
        return false;
    }
    set_failure(6);
    if (s.context != 0 && s.context != context.context) || !current(context, s.list) {
        return false;
    }
    set_failure(7);
    if s.registry == 0 || s.registry != registry() {
        return false;
    }
    set_failure(6);
    if !current(context, s.list) {
        return false;
    }
    s.context = context.context;
    // SAFETY: one more reference to the token, handed to the lease.
    unsafe { token_add_ref(owner.get().token.cast()) };
    *out = Lease {
        token: owner.get().token.cast(),
        generation: s.generation,
        context: s.context,
        list: s.list,
        registry: s.registry,
    };
    set_failure(0);
    true
}

/// Before an ExecuteCommandLists: each recorded list's token takes the submission -- accepted only for its one native
/// submission from the engine's site, on its registry's direct queue.
///
/// # Safety
/// `lists` holds `count` command lists.
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_before_submit(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
    caller_address: usize,
) {
    if !enabled() {
        return;
    }
    set_failure(1);
    set_details([0; 8]);
    if queue.is_null() || lists.is_null() || count == 0 {
        return;
    }
    // SAFETY: the caller's lists.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let base = base();
    let compatible = cvr_native_recording_compatible();
    let mut native = compatible && caller(base, caller_address as u64, EXECUTE_RETURN);
    let mut unwound = false;
    set_failure(0);
    for (i, &list) in lists.iter().enumerate() {
        if lists[..i].contains(&list) {
            continue;
        }
        let Some(owner) = recording(list) else {
            continue;
        };
        // SAFETY: the owner's token, alive while the owner is.
        let token = unsafe { &*owner.get().token };
        let s = token.s(&lock);
        let occurrences = lists.iter().filter(|&&l| l == list).count() as u32;
        let mut expected_queue = 0u64;
        let queue_valid = native_queue(s.registry, queue, Some(&mut expected_queue));
        let active = token.active.load(Acquire);
        if !native
            && !unwound
            && compatible
            && s.context != 0
            && s.native_reset
            && s.submission.fresh()
            && active
            && occurrences == 1
            && queue_valid
        {
            unwound = true;
            native = unwound_execute_origin(base, caller_address);
        }
        let allowed = native && s.native_reset && token.active.load(Acquire) && queue_valid;
        let first = s.submission.fresh();
        s.queue = queue as usize;
        let accepted = s.submission.before_submit(occurrences, allowed);
        s.observed = [
            caller_address as u64,
            if base != 0 && base <= u64::MAX - EXECUTE_RETURN as u64 {
                base + EXECUTE_RETURN as u64
            } else {
                0
            },
            queue as u64,
            expected_queue,
            occurrences as u64,
            submission_bits(&s.submission),
            s.observed[6],
            s.generation,
        ];
        if !accepted {
            s.rejection = if !first || occurrences != 1 { 9 } else { 8 };
            if s.context != 0 {
                set_failure(s.rejection);
                set_details(s.observed);
            }
        }
    }
}

/// Read-only validation at the current native recording position: the same live token, still fresh, in the same
/// context and registry.
///
/// # Safety
/// `lease` and `context` are valid; `list` a live command list or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_validate_recording(
    lease: *const Lease,
    list: *mut c_void,
    context: *const Context,
) -> bool {
    set_failure(1);
    set_details([0; 8]);
    // SAFETY: the caller's records.
    let (Some(lease), Some(&context)) = (unsafe { lease.as_ref() }, unsafe { context.as_ref() })
    else {
        return false;
    };
    if !enabled()
        || list.is_null()
        || !context.valid()
        || lease.token.is_null()
        || !direct(list)
        || list as u64 != lease.list
    {
        return false;
    }
    set_failure(2);
    if !cvr_native_recording_compatible() {
        return false;
    }
    let lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    set_failure(3);
    let Some(token) = query::<Token>(lease.token, &TOKEN_TYPE) else {
        return false;
    };
    let s = token.get().s(&lock);
    let mut details = s.observed;
    details[5] = submission_bits(&s.submission);
    set_details(details);
    set_failure(5);
    if !same_lease(token.get(), s, lease) || !s.submission.fresh() {
        return false;
    }
    set_failure(6);
    if context.context != lease.context || !current(context, lease.list) {
        return false;
    }
    set_failure(7);
    if lease.registry == 0 || lease.registry != registry() {
        return false;
    }
    set_failure(6);
    if !current(context, lease.list) {
        return false;
    }
    set_failure(0);
    true
}

/// The lease's token holds its one native submission, pending on `queue`.
///
/// # Safety
/// `lease` is valid; `queue` a live queue or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_validate_submission(
    lease: *const Lease,
    queue: *mut c_void,
) -> bool {
    set_failure(1);
    set_details([0; 8]);
    // SAFETY: the caller's lease.
    let Some(lease) = (unsafe { lease.as_ref() }) else {
        return false;
    };
    if !enabled() || lease.token.is_null() || queue.is_null() {
        return false;
    }
    let lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    set_failure(3);
    let Some(token) = query::<Token>(lease.token, &TOKEN_TYPE) else {
        return false;
    };
    let s = token.get().s(&lock);
    let mut details = s.observed;
    details[5] = submission_bits(&s.submission);
    set_details(details);
    set_failure(5);
    if !same_lease(token.get(), s, lease) {
        return false;
    }
    if s.submission.rejected {
        set_failure(if s.rejection != 0 { s.rejection } else { 9 });
        return false;
    }
    set_failure(8);
    if s.queue != 0 && s.queue != queue as usize {
        return false;
    }
    set_failure(12);
    if !s.submission.validating() {
        return false;
    }
    set_failure(8);
    if !native_queue(lease.registry, queue, None) {
        return false;
    }
    set_failure(0);
    true
}

/// After an ExecuteCommandLists: each list's token pending on `queue` is submitted.
///
/// # Safety
/// `lists` holds `count` command lists.
#[no_mangle]
pub unsafe extern "C" fn cvr_native_recording_submitted(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    if !enabled() || queue.is_null() || lists.is_null() || count == 0 {
        return;
    }
    // SAFETY: the caller's lists.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let lock = STATE.lock().unwrap_or_else(|e| e.into_inner());
    for (i, &list) in lists.iter().enumerate() {
        if lists[..i].contains(&list) {
            continue;
        }
        let Some(owner) = recording(list) else {
            continue;
        };
        // SAFETY: the owner's token, alive while the owner is.
        let s = unsafe { &*owner.get().token }.s(&lock);
        if s.queue == queue as usize && s.submission.pending != 0 {
            s.submission.submitted();
        }
    }
}
