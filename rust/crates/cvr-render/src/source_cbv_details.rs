//! THE SOURCE CBVS' NATIVE DETAILS (the pure part of include/Render/NativeSourceCbvDetails.hpp's detail namespace): the
//! engine's buffer record behind a CBV, the upload context it was bound on, the pooled allocation record and its
//! allocator, an alias payload and the constant pool's slot -- each read twice through the caller's reader and accepted
//! only when both reads agree (the reference-counted ones may differ only in their count) and the ABI is the audited
//! 2.5 one. The header forwards to these in the Rust build; tools/single_pass_tests/source_cbv_details_test runs its
//! cases against them (its Rust variant).
use core::ffi::c_void;

use crate::buffer_read::NativeBuffer;

/// source_cbv_details::Observation.
pub const UNKNOWN: u32 = 0;
pub const KNOWN: u32 = 1;
pub const ABSENT: u32 = 2;
pub const NOT_APPLICABLE: u32 = 3;
pub const UNREADABLE: u32 = 4;
pub const CHANGED: u32 = 5;
pub const MISMATCH: u32 = 6;
pub const UNKNOWN_ABI: u32 = 7;
pub const UNSUPPORTED: u32 = 8;
/// The 2.5 vtables of the pooled allocation record and its allocator (their first five slots are checked).
pub const ALLOCATION_VTABLE: u64 = 0x30466A8;
pub const ALLOCATOR_VTABLE: u64 = 0x306ED78;

/// geometry::MemoryReader.
pub type Reader = Option<unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool>;
/// The readers below read through this: `mem(address, out)` fills `out` or fails.
pub type Mem<'a> = &'a dyn Fn(u64, &mut [u8]) -> bool;
/// A C reader and its user as a Mem.
pub fn reader(read: Reader, user: *mut c_void) -> impl Fn(u64, &mut [u8]) -> bool {
    move |at, out| copy(read, user, at, out)
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    pub observation: u32,
    pub address: u64,
    pub list: u64,
    pub state: u64,
    pub submitted: u64,
    pub kind: u32,
    pub pending: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Record {
    pub observation: u32,
    pub address: u64,
    pub bytes: [u8; 0xB0],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Alias {
    pub observation: u32,
    pub payload: u64,
    pub control: u64,
    pub allocation: u64,
    pub alignment: u64,
    pub bytes: u64,
    pub flags: u32,
    pub references: i32,
    pub raw: [u8; 0x38],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Pool {
    pub observation: u32,
    pub entry: u64,
    pub resource: u64,
    pub owner: u64,
    pub occupied_word: u64,
    pub unit: u32,
    pub packed: u32,
    pub group: u32,
    pub slot: u32,
    pub occupied: bool,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Allocation {
    pub observation: u32,
    pub address: u64,
    pub vtable: u64,
    pub pimpl: u64,
    pub bytes: u64,
    pub resource: u64,
    pub heap: u64,
    pub block: u64,
    pub handle: u64,
    pub references: u32,
    pub kind: u32,
    pub association_known: bool,
    pub resource_matches: bool,
    pub extent_covers: bool,
    pub alias_shape_known: bool,
    pub alias_shape_matches: bool,
    pub raw: [u8; 0x68],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Allocator {
    pub observation: u32,
    pub address: u64,
    pub vtable: u64,
    pub pimpl: u64,
    pub references: u32,
    pub association_known: bool,
    pub pimpl_matches: bool,
    pub raw: [u8; 0x18],
}
/// geometry::ReadState.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ReadState {
    pub value: u32,
    pub status: u32,
    pub kind: u32,
}
const _: () = assert!(
    core::mem::size_of::<Context>() == 48
        && core::mem::size_of::<Record>() == 192
        && core::mem::size_of::<Alias>() == 112
        && core::mem::size_of::<Pool>() == 64
        && core::mem::size_of::<Allocation>() == 192
        && core::mem::size_of::<Allocator>() == 64
        && core::mem::size_of::<ReadState>() == 12
);
impl Default for Record {
    fn default() -> Self {
        Record {
            observation: 0,
            address: 0,
            bytes: [0; 0xB0],
        }
    }
}
impl Default for Alias {
    fn default() -> Self {
        Alias {
            observation: 0,
            payload: 0,
            control: 0,
            allocation: 0,
            alignment: 0,
            bytes: 0,
            flags: 0,
            references: 0,
            raw: [0; 0x38],
        }
    }
}
impl Default for Allocation {
    fn default() -> Self {
        Allocation {
            observation: 0,
            address: 0,
            vtable: 0,
            pimpl: 0,
            bytes: 0,
            resource: 0,
            heap: 0,
            block: 0,
            handle: 0,
            references: 0,
            kind: 0,
            association_known: false,
            resource_matches: false,
            extent_covers: false,
            alias_shape_known: false,
            alias_shape_matches: false,
            raw: [0; 0x68],
        }
    }
}
impl Default for Allocator {
    fn default() -> Self {
        Allocator {
            observation: 0,
            address: 0,
            vtable: 0,
            pimpl: 0,
            references: 0,
            association_known: false,
            pimpl_matches: false,
            raw: [0; 0x18],
        }
    }
}

fn copy(read: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
    // SAFETY: the caller's reader writes at most `out.len()` bytes into `out`.
    read.is_some_and(|r| {
        at != 0
            && at <= u64::MAX - out.len() as u64
            && unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) }
    })
}
/// Copy into a plain-data field in place (as the C++ does: a failed read may leave what the reader wrote).
fn copy_into<T: Copy>(mem: Mem, at: u64, value: &mut T) -> bool {
    // SAFETY: T is plain data (integers here).
    let bytes = unsafe {
        core::slice::from_raw_parts_mut((value as *mut T).cast::<u8>(), core::mem::size_of::<T>())
    };
    mem(at, bytes)
}
fn field_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn field_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
/// Bytes: two reads that must agree.
fn bytes<const N: usize>(mem: Mem, at: u64, out: &mut [u8; N]) -> u32 {
    let mut again = [0u8; N];
    if !mem(at, out) || !mem(at, &mut again) {
        return UNREADABLE;
    }
    if *out == again {
        KNOWN
    } else {
        CHANGED
    }
}
/// RefCountedBytes: two reads that may differ only in the reference count at +8 (the second is kept): the guarded
/// native AddRef/Release mutate only that DWORD.
fn ref_counted_bytes<const N: usize>(mem: Mem, at: u64, out: &mut [u8; N]) -> u32 {
    let mut again = [0u8; N];
    if !mem(at, out) || !mem(at, &mut again) {
        return UNREADABLE;
    }
    let (before, after) = (field_u32(out, 8), field_u32(&again, 8));
    if before == 0 || before >= i32::MAX as u32 || after == 0 || after >= i32::MAX as u32 {
        return UNSUPPORTED;
    }
    if (0..N).any(|i| (i < 8 || i >= 12) && out[i] != again[i]) {
        return CHANGED;
    }
    *out = again;
    KNOWN
}
pub fn state_value_known(state: ReadState) -> bool {
    matches!(state.status, 1 | 2 | 8 | 9)
}
pub fn matches_record(b: &[u8; 0xB0], native: &NativeBuffer) -> bool {
    if b[6] & 15 != 6 || (b[6] >> 4) as u32 != native.kind || field_u32(b, 0) as u64 != native.bytes
    {
        return false;
    }
    native.kind == 4 || (field_u64(b, 8) == native.gpu && field_u64(b, 0x10) == native.resource)
}
pub fn read_record(mem: Mem, native: &NativeBuffer) -> Record {
    let mut out = Record::default();
    if native.handle == 0 {
        out.observation = NOT_APPLICABLE;
        return out;
    }
    let offset = 0x5C0AF8 + (native.handle as u64 - 1) * 0xB0;
    if native.handle > 0x100000
        || native.registry == 0
        || native.registry > u64::MAX - offset - 0xB0
    {
        out.observation = MISMATCH;
        return out;
    }
    out.address = native.registry + offset;
    out.observation = bytes(mem, out.address, &mut out.bytes);
    if out.observation == KNOWN && !matches_record(&out.bytes, native) {
        out.observation = MISMATCH;
    }
    out
}
/// One context sample, field by field (a failed read stops it).
fn context_once(mem: Mem, address: u64, value: &mut Context) -> bool {
    value.address = address;
    address != 0
        && address <= u64::MAX - 0x640
        && copy_into(mem, address + 0x30, &mut value.list)
        && copy_into(mem, address + 0x60, &mut value.state)
        && copy_into(mem, address + 0x68, &mut value.kind)
        && copy_into(mem, address + 0x638, &mut value.submitted)
        && copy_into(mem, address + 0x534, &mut value.pending)
}
pub fn read_context(mem: Mem, address: u64, list: u64) -> Context {
    let mut out = Context {
        address,
        ..Default::default()
    };
    let mut again = Context::default();
    if !context_once(mem, address, &mut out) || !context_once(mem, address, &mut again) {
        out.observation = UNREADABLE;
    } else if out != again {
        out.observation = CHANGED;
    } else {
        out.observation = if out.list == list
            && out.state != 0
            && (out.kind == 1 || out.kind == 2)
            && out.submitted == 0
        {
            KNOWN
        } else {
            MISMATCH
        };
    }
    out
}
fn methods(mem: Mem, base: u64, allocator: bool) -> bool {
    let offset = if allocator {
        ALLOCATOR_VTABLE
    } else {
        ALLOCATION_VTABLE
    };
    if base == 0 || base > u64::MAX - offset - 40 {
        return false;
    }
    let mut b = [0u8; 40];
    if bytes(mem, base + offset, &mut b) != KNOWN {
        return false;
    }
    let expected: [u64; 5] = if allocator {
        [0x2FAC534, 0x2FAAF80, 0x348220, 0x18F3FB4, 0x18F3F48]
    } else {
        [0x2FAC534, 0x2FAAF80, 0x348220, 0x3483A4, 0x348244]
    };
    expected
        .iter()
        .enumerate()
        .all(|(i, &e)| base <= u64::MAX - e && field_u64(&b, i * 8) == base + e)
}
pub fn read_allocation(mem: Mem, base: u64, address: u64) -> Allocation {
    let mut out = Allocation {
        address,
        ..Default::default()
    };
    if address == 0 {
        out.observation = ABSENT;
        return out;
    }
    out.observation = ref_counted_bytes(mem, address, &mut out.raw);
    if out.observation != KNOWN {
        return out;
    }
    out.vtable = field_u64(&out.raw, 0);
    if base == 0
        || base > u64::MAX - ALLOCATION_VTABLE
        || out.vtable != base + ALLOCATION_VTABLE
        || !methods(mem, base, false)
    {
        out.observation = UNKNOWN_ABI;
        return out;
    }
    out.references = field_u32(&out.raw, 8);
    out.pimpl = field_u64(&out.raw, 0x10);
    out.bytes = field_u64(&out.raw, 0x18);
    out.resource = field_u64(&out.raw, 0x28);
    out.kind = field_u32(&out.raw, 0x60) & 3;
    if out.references == 0
        || out.references >= i32::MAX as u32
        || out.pimpl == 0
        || out.bytes == 0
        || out.kind == 3
    {
        out.observation = UNSUPPORTED;
        return out;
    }
    if out.kind == 1 {
        out.handle = field_u64(&out.raw, 0x40);
        out.block = field_u64(&out.raw, 0x48);
        let mut again = 0u64;
        if out.block == 0
            || out.block > u64::MAX - 0x40
            || !copy_into(mem, out.block + 0x38, &mut out.heap)
            || !copy_into(mem, out.block + 0x38, &mut again)
            || out.heap == 0
        {
            out.observation = UNREADABLE;
        } else if out.heap != again {
            out.observation = CHANGED;
        }
    } else if out.kind == 2 {
        out.heap = field_u64(&out.raw, 0x58);
        if out.heap == 0 {
            out.observation = UNSUPPORTED;
        }
    }
    out
}
pub fn read_allocator(mem: Mem, base: u64, registry: u64, allocation: &Allocation) -> Allocator {
    let mut out = Allocator::default();
    let mut again = 0u64;
    if registry == 0
        || registry > u64::MAX - 0x1410548
        || !copy_into(mem, registry + 0x1410540, &mut out.address)
        || !copy_into(mem, registry + 0x1410540, &mut again)
    {
        out.observation = UNREADABLE;
        return out;
    }
    if out.address != again {
        out.observation = CHANGED;
        return out;
    }
    if out.address == 0 {
        out.observation = ABSENT;
        return out;
    }
    out.observation = ref_counted_bytes(mem, out.address, &mut out.raw);
    if out.observation != KNOWN {
        return out;
    }
    out.vtable = field_u64(&out.raw, 0);
    if base == 0
        || base > u64::MAX - ALLOCATOR_VTABLE
        || out.vtable != base + ALLOCATOR_VTABLE
        || !methods(mem, base, true)
    {
        out.observation = UNKNOWN_ABI;
        return out;
    }
    out.references = field_u32(&out.raw, 8);
    out.pimpl = field_u64(&out.raw, 0x10);
    if out.references == 0 || out.references >= i32::MAX as u32 || out.pimpl == 0 {
        out.observation = UNSUPPORTED;
        return out;
    }
    out.association_known = allocation.observation == KNOWN;
    out.pimpl_matches = out.association_known && out.pimpl == allocation.pimpl;
    out
}
pub fn read_alias(mem: Mem, payload: u64, control: u64) -> Alias {
    let mut out = Alias {
        payload,
        control,
        ..Default::default()
    };
    if payload == 0 && control == 0 {
        out.observation = ABSENT;
        return out;
    }
    if payload == 0 || control == 0 {
        out.observation = MISMATCH;
        return out;
    }
    out.observation = bytes(mem, payload, &mut out.raw);
    if out.observation != KNOWN {
        return out;
    }
    let mut again = 0i32;
    if !copy_into(mem, control, &mut out.references) || !copy_into(mem, control, &mut again) {
        out.observation = UNREADABLE;
        return out;
    }
    if out.references != again {
        out.observation = CHANGED;
        return out;
    }
    out.allocation = field_u64(&out.raw, 8);
    out.flags = field_u32(&out.raw, 0x10);
    out.alignment = field_u64(&out.raw, 0x18);
    out.bytes = field_u64(&out.raw, 0x20);
    if out.references <= 0
        || out.references == i32::MAX
        || out.allocation == 0
        || out.flags != 0xC0
        || out.alignment != 0x10000
        || out.bytes == 0
    {
        out.observation = UNSUPPORTED;
    }
    out
}
pub fn read_pool(
    mem: Mem,
    base: u64,
    native: &NativeBuffer,
    record: &Record,
    gpu: u64,
    width: u64,
) -> Pool {
    let mut out = Pool::default();
    if native.kind != 1 {
        out.observation = NOT_APPLICABLE;
        return out;
    }
    if record.observation != KNOWN
        || record.bytes[6] != 0x16
        || !matches_record(&record.bytes, native)
        || base == 0
        || base > u64::MAX - 0x36F44CC
        || native.bytes == 0
        || native.bytes > 512
        || gpu == 0
        || width < 512
        || width > 512 * 1024
        || width % 512 != 0
        || gpu > u64::MAX - width
        || native.registry > u64::MAX - 0x14105C1
    {
        out.observation = UNSUPPORTED;
        return out;
    }
    let mut enabled = 0u8;
    if !copy_into(mem, base + 0x36F44C8, &mut out.unit)
        || !copy_into(mem, native.registry + 0x14105C0, &mut enabled)
    {
        out.observation = UNREADABLE;
        return out;
    }
    if enabled == 0 {
        out.observation = ABSENT;
        return out;
    }
    out.packed = field_u32(&record.bytes, 0x40);
    out.group = (out.packed & 65535) >> 3;
    out.slot = out.packed >> 16;
    if out.unit != 512
        || out.packed & 7 != 0
        || out.slot as u64 >= width / 512
        || native.gpu != gpu + out.slot as u64 * 512
    {
        out.observation = MISMATCH;
        return out;
    }
    let mut group = [0u8; 16];
    out.observation = bytes(mem, native.registry + 0x1410548, &mut group);
    if out.observation != KNOWN {
        return out;
    }
    let data = field_u64(&group, 0);
    let (capacity, count) = (field_u32(&group, 8), field_u32(&group, 12));
    if data == 0
        || count == 0
        || count > capacity
        || capacity > 8192
        || out.group >= count
        || data > u64::MAX - out.group as u64 * 144 - 144
    {
        out.observation = MISMATCH;
        return out;
    }
    out.entry = data + out.group as u64 * 144;
    let mut entry = [0u8; 144];
    out.observation = bytes(mem, out.entry, &mut entry);
    if out.observation != KNOWN {
        return out;
    }
    out.resource = field_u64(&entry, 0);
    out.owner = field_u64(&entry, 8);
    out.occupied_word = field_u64(&entry, 16 + (out.slot as usize / 64) * 8);
    out.occupied = out.occupied_word & (1u64 << (out.slot % 64)) != 0;
    if out.resource != native.resource || out.owner == 0 || !out.occupied {
        out.observation = MISMATCH;
    }
    out
}

// ---- the C ABI (the header's inline functions in the Rust build)

/// # Safety
/// `state` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_state_value_known(state: *const ReadState) -> bool {
    // SAFETY: as the caller promises.
    state_value_known(unsafe { *state })
}
/// # Safety
/// The arguments are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_matches_record(
    record: *const [u8; 0xB0],
    native: *const NativeBuffer,
) -> bool {
    // SAFETY: as the caller promises.
    matches_record(unsafe { &*record }, unsafe { &*native })
}
/// # Safety
/// `native` and `out` are valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_read_record(
    read: Reader,
    user: *mut c_void,
    native: *const NativeBuffer,
    out: *mut Record,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_record(&reader(read, user), &*native) };
}
/// # Safety
/// `out` is valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_read_context(
    read: Reader,
    user: *mut c_void,
    address: u64,
    list: u64,
    out: *mut Context,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_context(&reader(read, user), address, list) };
}
/// # Safety
/// `out` is valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_read_allocation(
    read: Reader,
    user: *mut c_void,
    base: u64,
    address: u64,
    out: *mut Allocation,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_allocation(&reader(read, user), base, address) };
}
/// # Safety
/// `allocation` and `out` are valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_read_allocator(
    read: Reader,
    user: *mut c_void,
    base: u64,
    registry: u64,
    allocation: *const Allocation,
    out: *mut Allocator,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_allocator(&reader(read, user), base, registry, &*allocation) };
}
/// # Safety
/// `out` is valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_read_alias(
    read: Reader,
    user: *mut c_void,
    payload: u64,
    control: u64,
    out: *mut Alias,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_alias(&reader(read, user), payload, control) };
}
/// # Safety
/// The pointers are valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_scd_read_pool(
    read: Reader,
    user: *mut c_void,
    base: u64,
    native: *const NativeBuffer,
    record: *const Record,
    gpu: u64,
    width: u64,
    out: *mut Pool,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_pool(&reader(read, user), base, &*native, &*record, gpu, width) };
}
