//! THE NATIVE TEXTURES' FIXED PINS (formerly the fixed pins of src/Render/NativeTextureRead.cpp and the bootstrap of
//! include/Render/NativeTextureRead.hpp): a fixed asset named only by a numeric candidate (a resource address seen in a
//! descriptor) is pinned without calling COM on the weak resource -- under the registry's native byte lock its handle's
//! positive reference count is raised by CAS, the allocation record and its allocator are retained through their own
//! AddRef, and everything is read again and compared before the lock is released (the handle reference is released
//! before the unlock; the owners stay in the caller's packet). The pinned owner (texture_immutable.rs) then grants
//! immutable leases on later lists. The bootstrap is shared with the race fixtures of
//! tools/single_pass_tests/fixed_asset_pins_test (its Rust variant), whose reader may throw a C++ exception: the
//! callbacks and the bootstrap are C-unwind and the guard releases and unlocks during that unwinding too.
use core::cell::Cell;
use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI32, AtomicI8, AtomicPtr, Ordering::*};
use std::sync::{Mutex, OnceLock};

use windows::core::{IUnknown, Interface as _};
use windows::Win32::Graphics::Direct3D12::*;

use crate::native_recording::{
    cvr_native_recording_capture, cvr_native_recording_validate_recording, fingerprint,
    Context as RecordingContext, Lease as Recording,
};
use crate::source_cbv_details::{
    read_allocation, read_allocator, reader, Allocation, Allocator, Mem, KNOWN,
};
use crate::texture_immutable::{
    empty_owner, lease_out, observe, owner_ref, physical, query_creation, receipt_for,
    required_bytes, same_allocation, same_device, unprotected, Held, ImmutableLease,
};
use crate::texture_lease::{
    compatible, current_context, default_heap, fail, fixed_context, fixed_owner, game, identity,
    list_of, read, resource, validate, Lease, UploadContext, READ,
};
use crate::texture_read::{
    fixed_owner as record_owner, immutable_shape, read_state, NativeTexture,
};
use crate::texture_receipts::fixed_compatible;

// ---- the bootstrap (the header's detail part)

/// texture_read::detail::FixedHandle.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedHandle {
    pub native: NativeTexture,
    pub record: u64,
    pub lock: u64,
    pub owner: u64,
    pub references: i32,
}
/// texture_read::detail::FixedPinSnapshot.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FixedPinSnapshot {
    pub handle: FixedHandle,
    pub allocation: Allocation,
    pub allocator: Allocator,
}
/// texture_read::detail::FixedRead (may unwind: the fixtures' reader throws).
pub type FixedRead = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
/// texture_read::detail::FixedPinOperations.
#[repr(C)]
pub struct FixedPinOperations {
    pub read: Option<FixedRead>,
    pub try_lock: Option<unsafe extern "C-unwind" fn(*mut c_void, u64) -> bool>,
    pub retain_handle: Option<unsafe extern "C-unwind" fn(*mut c_void, u64, i32) -> bool>,
    /// false: the allocator, true: the allocation; retains only, never cleans up.
    pub retain_owner: Option<unsafe extern "C-unwind" fn(*mut c_void, u64, bool) -> bool>,
    pub release_handle: Option<unsafe extern "C-unwind" fn(*mut c_void, u32)>,
    pub unlock: Option<unsafe extern "C-unwind" fn(*mut c_void, u64)>,
}
const _: () = assert!(
    core::mem::size_of::<FixedHandle>() == 56
        && core::mem::size_of::<FixedPinSnapshot>() == 312
        && core::mem::offset_of!(FixedPinSnapshot, allocation) == 56
        && core::mem::offset_of!(FixedPinSnapshot, allocator) == 248
        && core::mem::size_of::<FixedPinOperations>() == 48
);

fn fixed_mem(read: Option<FixedRead>, user: *mut c_void) -> impl Fn(u64, &mut [u8]) -> bool {
    move |at, out| {
        // SAFETY: the caller's reader writes at most `out.len()` bytes.
        read.is_some_and(|r| {
            at != 0
                && at <= u64::MAX - out.len() as u64
                && unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) }
        })
    }
}
/// FixedField: a value at address+offset.
fn field<T: Copy + Default>(mem: Mem, address: u64, offset: u64) -> Option<T> {
    if address == 0 || offset > u64::MAX - address {
        return None;
    }
    let mut value = T::default();
    // SAFETY: T is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut value as *mut T).cast::<u8>(),
            core::mem::size_of::<T>(),
        )
    };
    mem(address + offset, bytes).then_some(value)
}
/// ReadFixedHandle: under the caller's successful exclusive try-lock, the wanted record's positive reference count, its
/// fixed owner, and the swapchain table's 32 slots (none of them the wanted handle, the free count consistent).
pub fn read_fixed_handle(mem: Mem, base: u64, wanted: &NativeTexture) -> Option<FixedHandle> {
    if base == 0
        || wanted.registry == 0
        || wanted.resource == 0
        || wanted.handle == 0
        || wanted.handle > 32768
    {
        return None;
    }
    let registry = field::<u64>(mem, base, 0x3BCDCA8)?;
    if registry != wanted.registry || registry > u64::MAX - 0xCEBF30 - 0x1650 {
        return None;
    }
    if field::<u8>(mem, registry, 1)? != 0 {
        return None;
    }
    let lock = field::<u64>(mem, registry, 0x2F1C0)?;
    if lock != registry + 0x2F180 || field::<u8>(mem, lock, 0)? != 0xFF {
        return None;
    }
    let record = registry + 0x2F1D8 + (wanted.handle as u64 - 1) * 0xB0;
    let references = field::<i32>(mem, record - 8, 0)?;
    if references <= 0 || references >= i32::MAX {
        return None;
    }
    let mut bytes = [0u8; 0x58];
    if !mem(record, &mut bytes) {
        return None;
    }
    let resource = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
    let owner = record_owner(&bytes);
    if resource != wanted.resource || owner == 0 {
        return None;
    }
    let table = registry + 0xCEBF30;
    if field::<u64>(mem, table, 0)? != lock {
        return None;
    }
    let free_count = field::<u32>(mem, table, 8)?;
    if free_count > 32 {
        return None;
    }
    let mut free = 0u32;
    for i in 0..32u64 {
        let refs = field::<i32>(mem, table, 0x10 + i * 0xB0)?;
        if refs < -1 || refs == i32::MAX {
            return None;
        }
        let handle = field::<u32>(mem, table, 0x40 + i * 0xB0)?;
        if handle > 32768 || handle == wanted.handle {
            return None;
        }
        free += (refs == -1) as u32;
    }
    (free == free_count).then_some(FixedHandle {
        native: *wanted,
        record,
        lock,
        owner,
        references,
    })
}
/// ReadFixedPin: the handle with its allocation record and allocator, both known, the allocation the resource's own.
pub fn read_fixed_pin(mem: Mem, base: u64, wanted: &NativeTexture) -> Option<FixedPinSnapshot> {
    let handle = read_fixed_handle(mem, base, wanted)?;
    let allocation = read_allocation(mem, base, handle.owner);
    let allocator = read_allocator(mem, base, wanted.registry, &allocation);
    if allocation.observation != KNOWN
        || allocation.resource != wanted.resource
        || allocation.kind > 1
        || allocation.references >= i32::MAX as u32 - 1
        || allocator.observation != KNOWN
        || allocator.references >= i32::MAX as u32 - 1
        || !allocator.pimpl_matches
    {
        return None;
    }
    Some(FixedPinSnapshot {
        handle,
        allocation,
        allocator,
    })
}
/// SameFixedPins: the same handle, allocation and allocator (reference counts aside).
pub fn same_fixed_pins(a: &FixedPinSnapshot, b: &FixedPinSnapshot) -> bool {
    let (x, y, u, v) = (&a.allocation, &b.allocation, &a.allocator, &b.allocator);
    a.handle.native.registry == b.handle.native.registry
        && a.handle.native.resource == b.handle.native.resource
        && a.handle.native.handle == b.handle.native.handle
        && a.handle.record == b.handle.record
        && a.handle.lock == b.handle.lock
        && a.handle.owner == b.handle.owner
        && x.address == y.address
        && x.vtable == y.vtable
        && x.pimpl == y.pimpl
        && x.resource == y.resource
        && x.bytes == y.bytes
        && x.heap == y.heap
        && x.block == y.block
        && x.handle == y.handle
        && x.kind == y.kind
        && u.address == v.address
        && u.vtable == v.vtable
        && u.pimpl == v.pimpl
}
/// BootstrapFixedPins. Rejections: 1 arguments, 2 lock, 3 handle, 4 handle CAS, 5 pin, 6 allocator, 7 allocation, 8 changed.
/// The native owners stay in the caller's packet even on failure, so their COM releases happen after this guard unlocks.
pub fn bootstrap_fixed_pins(
    ops: &FixedPinOperations,
    user: *mut c_void,
    base: u64,
    wanted: &NativeTexture,
    out: &mut FixedPinSnapshot,
    rejected: &mut u32,
) -> bool {
    *out = FixedPinSnapshot::default();
    *rejected = 1;
    let (
        Some(_),
        Some(try_lock),
        Some(retain_handle),
        Some(retain_owner),
        Some(release_handle),
        Some(unlock),
    ) = (
        ops.read,
        ops.try_lock,
        ops.retain_handle,
        ops.retain_owner,
        ops.release_handle,
        ops.unlock,
    )
    else {
        return false;
    };
    if wanted.registry == 0 || wanted.registry > u64::MAX - 0x2F180 {
        return false;
    }
    let lock = wanted.registry + 0x2F180;
    *rejected = 2;
    // SAFETY (whole function): the caller's operations.
    if !unsafe { try_lock(user, lock) } {
        return false;
    }
    struct Guard {
        release: unsafe extern "C-unwind" fn(*mut c_void, u32),
        unlock: unsafe extern "C-unwind" fn(*mut c_void, u64),
        user: *mut c_void,
        lock: u64,
        handle: u32,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: the caller's operations; the handle reference first, then the lock.
            unsafe {
                if self.handle != 0 {
                    (self.release)(self.user, self.handle);
                }
                (self.unlock)(self.user, self.lock);
            }
        }
    }
    let mut guard = Guard {
        release: release_handle,
        unlock,
        user,
        lock,
        handle: 0,
    };
    let mem = fixed_mem(ops.read, user);
    *rejected = 3;
    let Some(first) = read_fixed_handle(&mem, base, wanted) else {
        return false;
    };
    if first.references >= i32::MAX - 1 {
        return false;
    }
    *rejected = 4;
    if !unsafe { retain_handle(user, first.record - 8, first.references) } {
        return false;
    }
    guard.handle = wanted.handle;
    *rejected = 5;
    let Some(snapshot) = read_fixed_pin(&mem, base, wanted) else {
        return false;
    };
    *rejected = 6;
    if !unsafe { retain_owner(user, snapshot.allocator.address, false) } {
        return false;
    }
    *rejected = 7;
    if !unsafe { retain_owner(user, snapshot.allocation.address, true) } {
        return false;
    }
    *rejected = 8;
    match read_fixed_pin(&mem, base, wanted) {
        Some(after) if same_fixed_pins(&snapshot, &after) => {}
        _ => return false,
    }
    *out = snapshot;
    *rejected = 0;
    true
}

// ---- the runtime pins

thread_local! {
    /// FixedPinLastDetails: the last attempt's evidence (registry, handle, resource, allocation/record words, rejection,
    /// owner, counts) -- POD only, never an ownership grant.
    static DETAILS: Cell<[u64; 8]> = const { Cell::new([0; 8]) };
}
fn set_details(update: impl FnOnce(&mut [u64; 8])) {
    let mut d = DETAILS.get();
    update(&mut d);
    DETAILS.set(d);
}

type Chain = unsafe extern "C" fn(u64, u64, bool, *mut u32) -> bool;
static CHAIN: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `chain` is target_lifetime::detail::ReadResizeChain (false) / ReadShutdownChain (true) over this process's memory.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_pins_attach(chain: Chain) {
    CHAIN.store(chain as *mut c_void, Release);
}
fn chain(base: u64, cet: u64, shutdown: bool, rejected: &mut u32) -> bool {
    let f = CHAIN.load(Acquire);
    // The plugin's own (formerly lent by src/Render/NativeTextureRead.cpp): target_lifetime's resize / shutdown chain over
    // this process's memory.
    #[cfg(all(feature = "rust-build", not(test)))]
    if f.is_null() {
        let mut read = crate::target_lifetime::ResizeChain::default();
        // SAFETY: this process's reader; a writable chain and failure.
        return unsafe { crate::target_lifetime::cvr_tl_read_hook_chain(crate::texture_lease::READ, core::ptr::null_mut(), base, cet, shutdown, &mut read, rejected) };
    }
    // SAFETY: the attached function.
    !f.is_null()
        && unsafe { core::mem::transmute::<*mut c_void, Chain>(f)(base, cet, shutdown, rejected) }
}
/// FixedPinCompatible: the engine functions the pins rely on are the audited ones, and the resize and shutdown entries
/// are the original ones or the audited CET chain.
fn fixed_pin_compatible() -> bool {
    static RESULT: OnceLock<(bool, u64, u64, u64)> = OnceLock::new();
    let &(accepted, rva, expected, actual) = RESULT.get_or_init(|| {
        const EXPECTED: [(u64, usize, u64); 30] = [
            (0x5D99F4, 100, 0x416099FA1DEC4511),
            (0x347EEC, 133, 0xDFEDCE894319584E),
            (0x2FA5EE0, 135, 0x38FCAFE70967B57A),
            (0x34C3C0, 806, 0x9667FCCC34D1C033),
            (0x34C39C, 36, 0x41112018CADE440D),
            (0x34C33C, 94, 0x56F46708A50F91CD),
            (0x22EE570, 0x72, 0x997908A535F51556),
            (0x34C320, 27, 0x23EB44C7C2F87CDE),
            (0x8EC794, 410, 0x17CC8F1F467EDA14),
            (0x5A914C, 182, 0xA08B256E1A287BAC),
            (0x5D9A58, 73, 0x6A803BDA1A688053),
            (0x34C7A0, 97, 0xB4D21C2B77A7C288),
            (0x688788, 1302, 0x55625DB1357B5061),
            (0x22CA300, 0x117, 0xF6E2B8EDCE8E3545),
            (0x3E3248, 145, 0x61B5A10A99C316E9),
            (0x22CA420, 0x118, 0xDCAECFC3F6C5E70D),
            (0xBF4624, 175, 0xFAA8CC8CC5084187),
            (0x5B1938, 195, 0x773420FC3024FE47),
            (0x2FAAF80, 13, 0x4B71F66010D54C4D),
            (0x348220, 33, 0xEFE63006A4D426CB),
            (0x348244, 195, 0x8DFB4CD8294CBA43),
            (0x3483A4, 33, 0x16D17AF39F50AAA4),
            (0x18F3FB4, 105, 0x8550A7D22883C599),
            (0x18F3F48, 52, 0x95650F6E1318A268),
            (0x2FAC534, 57, 0x2BA6EF10E4DE61C9),
            (0x349CFC, 224, 0x44B98676710DAFF2),
            (0x34A688, 1018, 0x48C1DFD837BDA037),
            (0x348548, 532, 0x8DA5EE9F82249354),
            (0x2458C6A, 96, 0xAD6AE86DE1275D37),
            (0x68AF98, 278, 0xE36CFE953900198E),
        ];
        let base = game();
        let mut bytes = [0u8; 1536];
        for (rva, len, hash) in EXPECTED {
            if len > bytes.len() || !read(base + rva, &mut bytes[..len]) {
                return (false, rva, hash, 0);
            }
            let actual = fingerprint(&bytes[..len]);
            if actual != hash {
                return (false, rva, hash, actual);
            }
        }
        (true, 0, 0, 0)
    });
    if !accepted {
        set_details(|d| {
            d[0] = rva;
            d[6] = expected;
            d[7] = actual;
        });
        return false;
    }
    let base = game();
    // SAFETY: a module lookup by name.
    let cet = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(windows::core::w!(
            "cyber_engine_tweaks.asi"
        ))
    }
    .map_or(0, |m| m.0 as u64);
    let mut rejected = 0u32;
    if !chain(base, cet, false, &mut rejected) {
        set_details(|d| {
            d[0] = 0x68813C; // target_lifetime::detail::ResizeRva
            d[7] = rejected as u64;
        });
        return false;
    }
    if !chain(base, cet, true, &mut rejected) {
        set_details(|d| {
            d[0] = 0x115CE4C; // target_lifetime::detail::ShutdownRva
            d[7] = rejected as u64;
        });
        return false;
    }
    true
}
/// Numeric hints only: every cached handle is checked anew under the native lock; neither an old registry snapshot nor
/// a recycled resource address grants a pin.
static CANDIDATES: Mutex<(u64, BTreeMap<u64, u32>)> = Mutex::new((0, BTreeMap::new()));
/// FindFixedCandidate: the one texture record naming the candidate resource (0 in the map: named twice).
fn find_fixed_candidate(registry: u64, candidate: u64) -> Option<NativeTexture> {
    const COUNT: usize = 32768;
    const STRIDE: u64 = 0xB0;
    if registry == 0 || candidate == 0 || registry > u64::MAX - 0x2F1D8 - COUNT as u64 * STRIDE {
        return None;
    }
    let mut cache = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
    let native = |handle| NativeTexture {
        registry,
        resource: candidate,
        handle,
        reserved: 0,
    };
    if cache.0 == registry {
        if let Some(&handle) = cache.1.get(&candidate) {
            let mut current = [0u8; 8];
            if handle != 0
                && read(
                    registry + 0x2F1D8 + (handle as u64 - 1) * STRIDE,
                    &mut current,
                )
                && u64::from_le_bytes(current) == candidate
            {
                return Some(native(handle));
            }
        }
    }
    let mut records = vec![0u8; COUNT * STRIDE as usize];
    if !read(registry + 0x2F1D8, &mut records) {
        return None;
    }
    let mut next = BTreeMap::new();
    for i in 0..COUNT {
        let at = i * STRIDE as usize;
        let value = u64::from_le_bytes(records[at..at + 8].try_into().unwrap());
        if value != 0 {
            next.entry(value)
                .and_modify(|h| *h = 0)
                .or_insert(i as u32 + 1);
        }
    }
    *cache = (registry, next);
    match cache.1.get(&candidate) {
        Some(&handle) if handle != 0 => Some(native(handle)),
        _ => None,
    }
}

/// The bootstrap's packet: the native owners it retained, released when it drops (after the unlock).
struct Packet {
    base: u64,
    allocator: Option<IUnknown>,
    allocation: Option<IUnknown>,
}
unsafe extern "C-unwind" fn runtime_read(
    _: *mut c_void,
    address: u64,
    out: *mut c_void,
    bytes: usize,
) -> bool {
    // SAFETY: the caller's buffer of `bytes` bytes.
    read(address, unsafe {
        core::slice::from_raw_parts_mut(out as *mut u8, bytes)
    })
}
unsafe extern "C-unwind" fn try_fixed_lock(_: *mut c_void, address: u64) -> bool {
    // SAFETY: the registry's native lock byte.
    unsafe { AtomicI8::from_ptr(address as *mut i8) }
        .compare_exchange(0, -1, SeqCst, SeqCst)
        .is_ok()
}
unsafe extern "C-unwind" fn retain_fixed_handle(
    _: *mut c_void,
    address: u64,
    expected: i32,
) -> bool {
    // SAFETY: the record's native reference count, under the native lock.
    expected > 0
        && expected < i32::MAX - 1
        && unsafe { AtomicI32::from_ptr(address as *mut i32) }
            .compare_exchange(expected, expected + 1, SeqCst, SeqCst)
            .is_ok()
}
type NativeAddRef = unsafe extern "C" fn(*mut c_void) -> u32;
type NativeReleaseHandle = unsafe extern "C" fn(*mut u32) -> u32;
unsafe extern "C-unwind" fn retain_fixed_owner(
    opaque: *mut c_void,
    address: u64,
    allocation: bool,
) -> bool {
    // SAFETY: the bootstrap's packet; the engine's AddRef (fingerprinted by fixed_pin_compatible).
    unsafe {
        let packet = &mut *(opaque as *mut Packet);
        let held = if allocation {
            &mut packet.allocation
        } else {
            &mut packet.allocator
        };
        if held.is_some() || address == 0 {
            return false;
        }
        let add_ref: NativeAddRef = core::mem::transmute(packet.base as usize + 0x2FAAF80);
        let count = add_ref(address as *mut c_void);
        // Even a refused post-increment count remains owned until AFTER unlock.
        *held = Some(IUnknown::from_raw(address as *mut c_void));
        count > 1 && count < i32::MAX as u32
    }
}
unsafe extern "C-unwind" fn release_fixed_handle(opaque: *mut c_void, handle: u32) {
    // SAFETY: the bootstrap's packet; the engine's handle release (fingerprinted).
    unsafe {
        let packet = &*(opaque as *const Packet);
        let release: NativeReleaseHandle = core::mem::transmute(packet.base as usize + 0x3E3248);
        let mut handle = handle;
        release(&mut handle);
    }
}
unsafe extern "C-unwind" fn unlock_fixed(_: *mut c_void, address: u64) {
    // SAFETY: the registry's native lock byte, held by this bootstrap.
    unsafe { AtomicI8::from_ptr(address as *mut i8) }.swap(0, SeqCst);
}

/// texture_read::FixedAssetPins::Data as the C++ lays it out.
#[repr(C)]
pub struct PinsData {
    pub held: *mut c_void,
    pub context: UploadContext,
    pub recording: Recording,
}
const _: () = assert!(core::mem::size_of::<PinsData>() == 72);
fn recording_context(c: &UploadContext) -> RecordingContext {
    RecordingContext {
        context: c.context,
        state: c.state,
        list: c.list,
    }
}
/// PinFixedAsset: a numeric candidate pinned on this direct list (see the module comment).
fn pin_fixed_asset(list: *mut c_void, candidate: u64, out: &mut PinsData) -> bool {
    DETAILS.set([0; 8]);
    fail(120);
    let Some(l) = list_of(&list) else {
        return false;
    };
    // SAFETY (whole function): live objects; the engine functions are fingerprinted.
    if candidate == 0
        || !observe()
        || cvr_host::probe::internal()
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
    {
        return false;
    }
    fail(121);
    if !compatible() || !fixed_compatible() || !fixed_pin_compatible() {
        return false;
    }
    let base = game();
    let context = current_context(base);
    let native_context = recording_context(&context);
    let mut recording = Recording {
        token: core::ptr::null_mut(),
        generation: 0,
        context: 0,
        list: 0,
        registry: 0,
    };
    fail(122);
    let captured = context.list == list as u64
        && fixed_context(&context)
        && unsafe { cvr_native_recording_capture(list, &native_context, &mut recording) };
    let token =
        (!recording.token.is_null()).then(|| unsafe { IUnknown::from_raw(recording.token) });
    if !captured {
        return false;
    }
    fail(123);
    let Some(native) = find_fixed_candidate(recording.registry, candidate) else {
        return false;
    };
    DETAILS.set([
        native.registry,
        native.handle as u64,
        native.resource,
        0,
        0,
        0,
        0,
        0,
    ]);
    let mut packet = Packet {
        base,
        allocator: None,
        allocation: None,
    };
    let ops = FixedPinOperations {
        read: Some(runtime_read),
        try_lock: Some(try_fixed_lock),
        retain_handle: Some(retain_fixed_handle),
        retain_owner: Some(retain_fixed_owner),
        release_handle: Some(release_fixed_handle),
        unlock: Some(unlock_fixed),
    };
    let mut snapshot = FixedPinSnapshot::default();
    let mut rejected = 0u32;
    fail(124);
    let packet_ptr = &mut packet as *mut Packet as *mut c_void;
    if !bootstrap_fixed_pins(
        &ops,
        packet_ptr,
        base,
        &native,
        &mut snapshot,
        &mut rejected,
    ) {
        set_details(|d| d[5] = rejected as u64);
        if rejected == 3 {
            // POD-only failure evidence after unlock, never an ownership grant.
            let record = native.registry + 0x2F1D8 + (native.handle as u64 - 1) * 0xB0;
            let (mut first, mut second) = ([0u8; 0x60], [0u8; 0x60]);
            if read(record - 8, &mut first) && read(record - 8, &mut second) && first == second {
                let u32_at =
                    |at: usize| u32::from_le_bytes(first[at..at + 4].try_into().unwrap()) as u64;
                let u64_at = |at: usize| u64::from_le_bytes(first[at..at + 8].try_into().unwrap());
                let flags = u16::from_le_bytes(first[0x5E..0x60].try_into().unwrap()) as u64;
                set_details(|d| {
                    d[3] = u64_at(8);
                    d[4] = first[0x10] as u64 | flags << 32;
                    d[6] = u64_at(0x18);
                    d[7] = u32_at(0) | u32_at(0x50) << 32;
                });
            }
        }
        return false;
    }
    set_details(|d| {
        d[3] = snapshot.allocation.address;
        d[4] = snapshot.allocator.address;
    });
    fail(125);
    let pinned = packet
        .allocation
        .as_ref()
        .map_or(core::ptr::null_mut(), |p| p.as_raw());
    let current = read_allocation(&reader(READ, core::ptr::null_mut()), base, pinned as u64);
    if !same_allocation(&snapshot.allocation, &current)
        || packet.allocator.is_none()
        || current.resource != candidate
    {
        return false;
    }
    // The pinned native allocation owns this resource (it releases +0x28 only at allocation destruction). No
    // driver/interposer call ran under the lock.
    let resource_ptr = current.resource as *mut c_void;
    let mut o = empty_owner();
    o.allocator_pin = packet.allocator.take();
    o.allocation_pin = packet.allocation.take();
    o.held = resource(&resource_ptr).cloned();
    o.source = Lease {
        resource: resource_ptr,
        native,
        receipt: 0,
        owner: snapshot.allocation.address,
    };
    o.allocation = snapshot.allocation;
    o.allocator = snapshot.allocator;
    let mut owner = Held::new(o);
    fail(126);
    let Some(creation) = query_creation(resource_ptr) else {
        return false;
    };
    owner.get_mut().creation = creation;
    if !physical(owner.get())
        || !identity(&native)
        || fixed_owner(&native) != snapshot.allocation.address
        || current_context(base) != context
        || !fixed_context(&context)
        || !unsafe { cvr_native_recording_validate_recording(&recording, list, &native_context) }
    {
        return false;
    }
    *out = PinsData {
        held: owner.into_raw(),
        context,
        recording: Recording {
            token: token.map_or(core::ptr::null_mut(), |t| t.into_raw()),
            ..recording
        },
    };
    fail(0);
    true
}
/// GetFixedPinnedResource: the pinned resource (referenced).
fn fixed_pinned_resource(pins: Option<&PinsData>, out: &mut *mut c_void) -> bool {
    fail(127);
    let Some(owner) = pins.and_then(|p| owner_ref(p.held)) else {
        return false;
    };
    let Some(r) = owner.held.as_ref() else {
        return false;
    };
    *out = r.clone().into_raw();
    fail(0);
    true
}
/// AcquirePinnedImmutable: an immutable lease of the pinned asset on a later direct list of the same native recording,
/// sharing the authenticated pins with a new creation receipt.
fn acquire_pinned_immutable(
    list: *mut c_void,
    pins: Option<&PinsData>,
    out: &mut ImmutableLease,
) -> bool {
    DETAILS.set([0; 8]);
    fail(128);
    let (Some(l), Some(pins)) = (list_of(&list), pins) else {
        return false;
    };
    let Some(held) = owner_ref(pins.held) else {
        return false;
    };
    // SAFETY (whole function): live objects.
    if !observe()
        || cvr_host::probe::internal()
        || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
        || !compatible()
        || !fixed_compatible()
    {
        return false;
    }
    let source = held.source;
    let base = game();
    let context = current_context(base);
    let native_context = recording_context(&context);
    fail(129);
    if context != pins.context
        || context.list != list as u64
        || !fixed_context(&context)
        || !unsafe {
            cvr_native_recording_validate_recording(&pins.recording, list, &native_context)
        }
        || !identity(&source.native)
        || fixed_owner(&source.native) != source.owner
        || !physical(held)
        || read_state(
            &source.native,
            context.context,
            context.list,
            READ,
            core::ptr::null_mut(),
        )
        .status
            != 8
    {
        return false;
    }
    fail(130);
    let Some(r) = resource(&source.resource) else {
        return false;
    };
    if !immutable_shape(&unsafe { r.GetDesc() }) || !default_heap(r) || !unprotected(r) {
        return false;
    }
    let Some(device) = same_device(r, l) else {
        return false;
    };
    let required = required_bytes(&device, r);
    if required == 0 || required == u64::MAX || held.allocation.bytes < required {
        return false;
    }
    let receipt = receipt_for(source.resource, source.native.registry);
    if receipt == 0 {
        return false;
    }
    // Copy only authenticated, already-owned pins. A changed registry owner is a rejection above, never a replacement
    // allocation to AddRef here.
    let mut o = empty_owner();
    o.allocator_pin = held.allocator_pin.clone();
    o.allocation_pin = held.allocation_pin.clone();
    o.held = held.held.clone();
    o.source = Lease { receipt, ..source };
    o.creation = held.creation.clone();
    o.allocation = held.allocation;
    o.allocator = held.allocator;
    let owner = Held::new(o);
    fail(131);
    if !physical(owner.get())
        || !validate(&owner.get().source, core::ptr::null_mut())
        || !fixed_context(&context)
        || current_context(base) != context
        || !unsafe {
            cvr_native_recording_validate_recording(&pins.recording, list, &native_context)
        }
    {
        return false;
    }
    *out = lease_out(&owner);
    fail(0);
    true
}

// ---- the C ABI (the header's bootstrap and src/Render/NativeTextureRead.cpp's pins in the Rust build)

/// # Safety
/// `wanted` and `out` are valid; `read` reads for `user` (and may unwind).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tr_read_fixed_handle(
    read: Option<FixedRead>,
    user: *mut c_void,
    base: u64,
    wanted: *const NativeTexture,
    out: *mut FixedHandle,
) -> bool {
    // SAFETY: as the caller promises.
    let value = read_fixed_handle(&fixed_mem(read, user), base, unsafe { &*wanted });
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// As cvr_tr_read_fixed_handle.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tr_read_fixed_pin(
    read: Option<FixedRead>,
    user: *mut c_void,
    base: u64,
    wanted: *const NativeTexture,
    out: *mut FixedPinSnapshot,
) -> bool {
    // SAFETY: as the caller promises.
    let value = read_fixed_pin(&fixed_mem(read, user), base, unsafe { &*wanted });
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// The snapshots are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_same_fixed_pins(
    a: *const FixedPinSnapshot,
    b: *const FixedPinSnapshot,
) -> bool {
    // SAFETY: as the caller promises.
    same_fixed_pins(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// The records are valid; the operations are the caller's (they may unwind; the guard still releases and unlocks).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tr_bootstrap_fixed_pins(
    ops: *const FixedPinOperations,
    user: *mut c_void,
    base: u64,
    wanted: *const NativeTexture,
    out: *mut FixedPinSnapshot,
    rejected: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { bootstrap_fixed_pins(&*ops, user, base, &*wanted, &mut *out, &mut *rejected) }
}
/// # Safety
/// `out` receives 8 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_fixed_pin_details(out: *mut [u64; 8]) {
    // SAFETY: as the caller promises.
    unsafe { *out = DETAILS.get() };
}
/// # Safety
/// `list` live or null; `out` a zeroed FixedAssetPins::Data.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_pin_fixed_asset(
    list: *mut c_void,
    candidate: u64,
    out: *mut PinsData,
) -> bool {
    // SAFETY: as the caller promises.
    pin_fixed_asset(list, candidate, unsafe { &mut *out })
}
/// # Safety
/// `pins` valid or null; `out` a null ComPtr.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_fixed_pinned_resource(
    pins: *const PinsData,
    out: *mut *mut c_void,
) -> bool {
    // SAFETY: as the caller promises.
    fixed_pinned_resource(unsafe { pins.as_ref() }, unsafe { &mut *out })
}
/// # Safety
/// `list` live or null; `pins` valid or null; `out` a zeroed ImmutableLease.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_acquire_pinned_immutable(
    list: *mut c_void,
    pins: *const PinsData,
    out: *mut ImmutableLease,
) -> bool {
    // SAFETY: as the caller promises.
    acquire_pinned_immutable(list, unsafe { pins.as_ref() }, unsafe { &mut *out })
}
