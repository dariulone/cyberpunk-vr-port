//! THE NATIVE IA AND INDIRECT BINDINGS' RULES (the pure part of include/Render/NativeIaBinding.hpp): the exact native
//! IA call sites (index: 0x34291C, 0x3392C7; vertex: 0x339E6F, 0x342980, 0x339382) decoded into the registry's buffer
//! records of the bound handles; the native ExecuteIndirect call sites (0x342B11 / 0x413AE0 with RBX the context,
//! 0xD543BB with RDI) decoded -- twice, equal -- into the argument buffer's record and the command signature; the
//! source range inside a backing; and a buffer's native read state from the recording context's state map. The header
//! forwards to these in the Rust build; tools/single_pass_tests/indirect_binding_test, geometry_lease_test and
//! source_srv_binding_test run their cases against them (their Rust variants).
use core::ffi::c_void;

use crate::buffer_read::NativeBuffer;
use crate::source_cbv_details::ReadState;
use crate::texture_lease::UploadContext;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT};

/// geometry::IaFrame.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IaFrame {
    pub rva: u64,
    pub rsp: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub r12: u64,
    pub r14: u64,
    pub r15: u64,
}
/// geometry::IndirectFrame.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IndirectFrame {
    pub rva: u64,
    pub rbx: u64,
    pub rdi: u64,
}
/// geometry::IndirectCall.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct IndirectCall {
    pub list: u64,
    pub signature: u64,
    pub arguments: u64,
    pub argument_offset: u64,
    pub count_buffer: u64,
    pub count_offset: u64,
    pub max_count: u32,
}
/// geometry::IndirectNativeRecord.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IndirectNativeRecord {
    pub arguments: NativeBuffer,
    pub context: UploadContext,
    pub raw_record: [u8; 0xB0],
    pub record_address: u64,
    pub signature: u64,
    pub caller_rva: u32,
    pub signature_kind: u32,
    pub signature_stride: u32,
}
impl Default for IndirectNativeRecord {
    fn default() -> Self {
        IndirectNativeRecord {
            arguments: NativeBuffer::default(),
            context: UploadContext::default(),
            raw_record: [0; 0xB0],
            record_address: 0,
            signature: 0,
            caller_rva: 0,
            signature_kind: 0,
            signature_stride: 0,
        }
    }
}
const _: () = assert!(
    core::mem::size_of::<IaFrame>() == 72
        && core::mem::size_of::<IndirectFrame>() == 24
        && core::mem::size_of::<IndirectCall>() == 56
        && core::mem::size_of::<IndirectNativeRecord>() == 280
        && core::mem::offset_of!(IndirectNativeRecord, record_address) == 248
);

/// geometry::MemoryReader.
pub type Reader = Option<unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool>;
fn read_bytes(reader: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
    // SAFETY: the caller's reader writes at most `out.len()` bytes.
    reader.is_some_and(|r| {
        at != 0
            && at <= u64::MAX - out.len() as u64
            && unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) }
    })
}
fn read_into<T: Copy>(reader: Reader, user: *mut c_void, at: u64, value: &mut T) -> bool {
    // SAFETY: T is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut((value as *mut T).cast::<u8>(), core::mem::size_of::<T>())
    };
    read_bytes(reader, user, at, bytes)
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

/// DecodeIa: the bound buffers of an exact native IA call site (`index`: the index buffer).
pub fn decode_ia(
    f: &IaFrame,
    index: bool,
    start: u32,
    count: u32,
    reader: Reader,
    user: *mut c_void,
    result: &mut [NativeBuffer; 16],
) -> bool {
    *result = [NativeBuffer::default(); 16];
    if reader.is_none() || count == 0 || count > 16 || start > 16 - count {
        return false;
    }
    let record = |registry: u64, offset: u64, output: &mut NativeBuffer| -> bool {
        if registry == 0
            || offset % 0xB0 != 0
            || offset >= 0xB0 * 0x100000
            || registry > u64::MAX - 0x5C0B10 - offset
        {
            return false;
        }
        let mut bytes = [0u8; 24];
        if !read_bytes(reader, user, registry + offset + 0x5C0AF8, &mut bytes) {
            return false;
        }
        let size = u32_at(&bytes, 0);
        output.gpu = u64_at(&bytes, 8);
        output.resource = u64_at(&bytes, 16);
        output.bytes = size as u64;
        output.registry = registry;
        output.handle = (offset / 0xB0 + 1) as u32;
        output.kind = (bytes[6] >> 4) as u32;
        output.context = match f.rva {
            0x3392C7 | 0x339382 => f.r15,
            0x339E6F => f.rdi,
            _ => f.rsi,
        };
        output.resource != 0 && output.gpu != 0 && size != 0 && output.gpu <= u64::MAX - size as u64
    };
    let handle = |registry: u64, value: u32, output: &mut NativeBuffer| {
        value != 0 && value <= 0x100000 && record(registry, (value - 1) as u64 * 0xB0, output)
    };
    if index {
        if start != 0 || count != 1 {
            return false;
        }
        if f.rva == 0x34291C {
            return record(f.r12, f.rbx, &mut result[0]);
        }
        if f.rva == 0x3392C7 && f.rsp <= u64::MAX - 0x120 {
            let (mut pointer, mut id) = (0u64, 0u32);
            return read_into(reader, user, f.rsp + 0x118, &mut pointer)
                && read_into(reader, user, pointer, &mut id)
                && handle(f.rdi, id, &mut result[0]);
        }
    } else {
        if f.rva == 0x339E6F && count == 1 && start == f.rsi as u32 {
            return record(f.rbp, f.rbx, &mut result[0]);
        }
        if start != 0 {
            return false;
        }
        if f.rva == 0x342980 && count as u64 == f.r14 {
            if !record(f.r12, f.rbx, &mut result[0]) {
                return false;
            }
            for i in 1..count as usize {
                result[i] = result[0];
            }
            return true;
        }
        if f.rva == 0x339382 && f.rsp <= u64::MAX - 0x100 {
            let mut native_count = 0u32;
            if !read_into(reader, user, f.rsp + 0xF8, &mut native_count) || native_count != count {
                return false;
            }
            // R12 holds the signed difference between the handle and offset arrays; RSI has advanced to the end of
            // the offset array.
            let end = f.r12.wrapping_add(f.rsi);
            if end < count as u64 * 4 {
                return false;
            }
            for i in 0..count {
                let mut id = 0u32;
                if !read_into(reader, user, end - (count - i) as u64 * 4, &mut id)
                    || !handle(f.rdi, id, &mut result[i as usize])
                {
                    return false;
                }
            }
            return true;
        }
    }
    false
}

fn context_valid(c: &UploadContext) -> bool {
    c.context != 0 && c.state != 0 && c.list != 0
}
pub fn same_record(a: &IndirectNativeRecord, b: &IndirectNativeRecord) -> bool {
    let (x, y) = (&a.arguments, &b.arguments);
    x.resource == y.resource
        && x.gpu == y.gpu
        && x.bytes == y.bytes
        && x.registry == y.registry
        && x.context == y.context
        && x.handle == y.handle
        && x.kind == y.kind
        && a.context == b.context
        && a.raw_record == b.raw_record
        && a.record_address == b.record_address
        && a.signature == b.signature
        && a.caller_rva == b.caller_rva
        && a.signature_kind == b.signature_kind
        && a.signature_stride == b.signature_stride
}
/// MatchesOrigin: the frame is the caller's site with its audited registers naming the current context (and registry).
pub fn matches_origin(
    frame: &IndirectFrame,
    caller: u32,
    current: &UploadContext,
    registry: u64,
) -> bool {
    if frame.rva != caller as u64 || !context_valid(current) || registry == 0 {
        return false;
    }
    match frame.rva {
        0x342B11 => frame.rbx == current.context && frame.rdi == registry,
        0xD543BB => frame.rdi == current.context,
        0x413AE0 => frame.rbx == current.context,
        _ => false,
    }
}
/// DecodeOnce: the argument buffer's registry record (class 0xB) and the command signature of the native call.
pub fn decode_once(
    game: u64,
    frame: &IndirectFrame,
    current: &UploadContext,
    call: &IndirectCall,
    reader: Reader,
    user: *mut c_void,
) -> Option<IndirectNativeRecord> {
    let (context, signature_offset, kind, stride) = match frame.rva {
        0x342B11 | 0x413AE0 => (frame.rbx, 0x1410678u64, 1u32, 20u32),
        0xD543BB => (frame.rdi, 0x1410670, 2, 16),
        _ => return None,
    };
    if game == 0
        || game > u64::MAX - 0x3BCDCB0
        || !context_valid(current)
        || current.context != context
        || current.list != call.list
        || call.signature == 0
        || call.arguments == 0
        || call.max_count != 1
        || call.count_buffer != 0
        || call.count_offset != 0
        || call.argument_offset % 4 != 0
        || context > u64::MAX - 0x640
    {
        return None;
    }
    let mut pending = IndirectNativeRecord {
        context: *current,
        caller_rva: frame.rva as u32,
        signature_kind: kind,
        signature_stride: stride,
        ..Default::default()
    };
    let native = &mut pending.arguments;
    let (mut actual_list, mut actual_state, mut submitted, mut signature) =
        (0u64, 0u64, 0u64, 0u64);
    let (mut context_kind, mut barriers, mut offset) = (0u32, 0u32, 0u32);
    if !read_into(reader, user, game + 0x3BCDCA8, &mut native.registry)
        || native.registry == 0
        || (frame.rva == 0x342B11 && frame.rdi != native.registry)
        || !read_into(reader, user, context + 0x30, &mut actual_list)
        || actual_list != call.list
        || !read_into(reader, user, context + 0x60, &mut actual_state)
        || actual_state != current.state
        || !read_into(reader, user, context + 0x68, &mut context_kind)
        || (context_kind != 1 && context_kind != 2)
        || !read_into(reader, user, context + 0x638, &mut submitted)
        || submitted != 0
        || !read_into(reader, user, context + 0x534, &mut barriers)
        || barriers != 0
        || !read_into(reader, user, context + 0x6C, &mut native.handle)
        || native.handle == 0
        || native.handle > 0x100000
        || !read_into(reader, user, context + 0x70, &mut offset)
        || offset as u64 != call.argument_offset
        || native.registry > u64::MAX - signature_offset - 8
        || !read_into(
            reader,
            user,
            native.registry + signature_offset,
            &mut signature,
        )
        || signature != call.signature
    {
        return None;
    }
    let relative = 0x5C0AF8 + (native.handle as u64 - 1) * 0xB0;
    if native.registry > u64::MAX - relative - 0xB0 {
        return None;
    }
    pending.record_address = pending.arguments.registry + relative;
    if !read_bytes(
        reader,
        user,
        pending.record_address,
        &mut pending.raw_record,
    ) || pending.raw_record[6] & 15 != 0xB
    {
        return None;
    }
    let native = &mut pending.arguments;
    native.bytes = u32_at(&pending.raw_record, 0) as u64;
    native.gpu = u64_at(&pending.raw_record, 8);
    native.resource = u64_at(&pending.raw_record, 16);
    native.kind = (pending.raw_record[6] >> 4) as u32;
    native.context = context;
    pending.signature = signature;
    let native = &pending.arguments;
    if native.resource != call.arguments
        || native.gpu == 0
        || native.bytes == 0
        || native.gpu > u64::MAX - native.bytes
    {
        return None;
    }
    Some(pending)
}
/// Decode: two equal decodings.
pub fn decode(
    game: u64,
    frame: &IndirectFrame,
    current: &UploadContext,
    call: &IndirectCall,
    reader: Reader,
    user: *mut c_void,
) -> Option<IndirectNativeRecord> {
    let first = decode_once(game, frame, current, call, reader, user)?;
    let second = decode_once(game, frame, current, call, reader, user)?;
    same_record(&first, &second).then_some(first)
}
/// SourceRange: the bytes at `offset` of the backing inside the native buffer (`fullBacking`: the buffer is the whole
/// backing).
pub fn source_range(
    native: &NativeBuffer,
    backing_gpu: u64,
    backing_bytes: u64,
    offset: u64,
    bytes: u64,
) -> Option<bool> {
    if backing_gpu == 0
        || backing_bytes == 0
        || backing_gpu > u64::MAX - backing_bytes
        || native.gpu == 0
        || native.bytes == 0
        || native.gpu > u64::MAX - native.bytes
        || offset > backing_bytes
        || bytes > backing_bytes - offset
        || native.gpu < backing_gpu
        || native.gpu - backing_gpu > backing_bytes
        || native.bytes > backing_bytes - (native.gpu - backing_gpu)
        || backing_gpu + offset < native.gpu
        || backing_gpu + offset - native.gpu > native.bytes
        || bytes > native.bytes - (backing_gpu + offset - native.gpu)
    {
        return None;
    }
    Some(native.gpu == backing_gpu && native.bytes == backing_bytes)
}
/// ReadNativeState: the buffer's state in the recording context's map (status 2), or why not (3 stale identity, 4
/// pending barriers, 5 unknown entry).
pub fn read_native_state(
    source: &NativeBuffer,
    list: u64,
    reader: Reader,
    user: *mut c_void,
) -> ReadState {
    let mut result = ReadState {
        value: 0,
        status: 3,
        kind: source.kind,
    };
    if source.registry == 0
        || source.context == 0
        || source.handle == 0
        || source.handle > 0x100000
        || source.context > u64::MAX - 0x590
    {
        return result;
    }
    let offset = 0x5C0AF8 + (source.handle as u64 - 1) * 0xB0;
    if source.registry > u64::MAX - offset - 24 {
        return result;
    }
    let mut record = [0u8; 24];
    let mut native_list = 0u64;
    if !read_bytes(reader, user, source.registry + offset, &mut record)
        || !read_into(reader, user, source.context + 0x30, &mut native_list)
        || native_list != list
    {
        return result;
    }
    if u64_at(&record, 16) != source.resource
        || u64_at(&record, 8) != source.gpu
        || u32_at(&record, 0) as u64 != source.bytes
        || (record[6] >> 4) as u32 != source.kind
    {
        return result;
    }
    let mut pending = 0u32;
    if !read_into(reader, user, source.context + 0x534, &mut pending) {
        return result;
    }
    if pending != 0 {
        result.status = 4;
        return result;
    }
    result.status = 5;
    let mut map = [0u8; 32];
    if !read_bytes(reader, user, source.context + 0x568, &mut map) {
        return result;
    }
    let (buckets, count, bucket_count, data, capacity, stride) = (
        u64_at(&map, 0),
        u32_at(&map, 8),
        u32_at(&map, 12),
        u64_at(&map, 16),
        u32_at(&map, 24),
        u32_at(&map, 28),
    );
    if count == 0
        || count > capacity
        || capacity > 1048576
        || bucket_count == 0
        || bucket_count > 1048576
        || stride != 16
        || buckets == 0
        || data == 0
    {
        return result;
    }
    let bucket_offset = (source.handle % bucket_count) as u64 * 4;
    if buckets > u64::MAX - bucket_offset - 4 {
        return result;
    }
    let mut at = 0u32;
    if !read_into(reader, user, buckets + bucket_offset, &mut at) {
        return result;
    }
    let mut n = 0u32;
    while n < count && n < 64 && at != u32::MAX {
        if at >= capacity || data > u64::MAX - at as u64 * 16 - 16 {
            return result;
        }
        let mut entry = [0u8; 16];
        if !read_bytes(reader, user, data + at as u64 * 16, &mut entry) {
            return result;
        }
        if u32_at(&entry, 4) == source.handle && u32_at(&entry, 8) == source.handle {
            let value = u32_at(&entry, 12);
            if value == 0x80000000 {
                return result;
            }
            result.value = value;
            result.status = 2;
            return result;
        }
        at = u32_at(&entry, 0);
        n += 1;
    }
    result
}

// ---- the C ABI (the header's inline functions in the Rust build)

/// # Safety
/// `frame` and `result` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_iab_decode_ia(
    frame: *const IaFrame,
    index: bool,
    start: u32,
    count: u32,
    reader: Reader,
    user: *mut c_void,
    result: *mut [NativeBuffer; 16],
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { decode_ia(&*frame, index, start, count, reader, user, &mut *result) }
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_iab_same_record(
    a: *const IndirectNativeRecord,
    b: *const IndirectNativeRecord,
) -> bool {
    // SAFETY: as the caller promises.
    same_record(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_iab_matches_origin(
    frame: *const IndirectFrame,
    caller: u32,
    current: *const UploadContext,
    registry: u64,
) -> bool {
    // SAFETY: as the caller promises.
    matches_origin(unsafe { &*frame }, caller, unsafe { &*current }, registry)
}
/// Decode (`once`: a single decoding); `out` written only on success.
///
/// # Safety
/// The records valid; `reader` reads for `user`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_iab_decode(
    game: u64,
    frame: *const IndirectFrame,
    current: *const UploadContext,
    call: *const IndirectCall,
    reader: Reader,
    user: *mut c_void,
    out: *mut IndirectNativeRecord,
    once: bool,
) -> bool {
    // SAFETY: as the caller promises.
    let (frame, current, call) = unsafe { (&*frame, &*current, &*call) };
    let value = if once {
        decode_once(game, frame, current, call, reader, user)
    } else {
        decode(game, frame, current, call, reader, user)
    };
    match value {
        Some(v) => {
            unsafe { *out = v };
            true
        }
        None => false,
    }
}
/// # Safety
/// `native` and `full_backing` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_iab_source_range(
    native: *const NativeBuffer,
    backing_gpu: u64,
    backing_bytes: u64,
    offset: u64,
    bytes: u64,
    full_backing: *mut bool,
) -> bool {
    // SAFETY: as the caller promises.
    let value = source_range(
        unsafe { &*native },
        backing_gpu,
        backing_bytes,
        offset,
        bytes,
    );
    unsafe { *full_backing = value.unwrap_or(false) };
    value.is_some()
}
/// # Safety
/// `source` and `out` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_iab_read_native_state(
    source: *const NativeBuffer,
    list: u64,
    reader: Reader,
    user: *mut c_void,
    out: *mut ReadState,
) {
    // SAFETY: as the caller promises.
    unsafe { *out = read_native_state(&*source, list, reader, user) };
}

// ---- the .cpp's frames, reads and leases (Native, CurrentIndirectContext, IndirectOrigin, AcquireBuffer, ReadStateAtDraw)
//
// The engine's memory is read with ReadProcessMemory (an unreadable address reads as false), the stack walked with
// cvr-engine's unwinder (its SEH step), the thread's TLS array taken from GS.

/// geometry::BufferLease with its resource owned (a reference the holder releases).
#[repr(C)]
pub struct OwnedLease {
    pub resource: *mut c_void,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
    pub native: NativeBuffer,
}
const _: () = assert!(core::mem::size_of::<OwnedLease>() == 80 && core::mem::offset_of!(OwnedLease, native) == 32);
/// Read: the engine's memory as a Reader (the .cpp's decoders read through it too).
///
/// # Safety
/// `out` holds `bytes`.
#[no_mangle]
pub unsafe extern "C" fn cvr_iab_read(_: *mut c_void, address: u64, out: *mut c_void, bytes: usize) -> bool {
    if bytes == 0 {
        return crate::geometry_packets::read(address, &mut []);
    }
    // SAFETY: as documented.
    crate::geometry_packets::read(address, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
fn game() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
fn code(rva: u64, bytes: u64, expected: u64) -> bool {
    crate::code_tables::cvr_code_hash(game(), rva, bytes, expected, 2048)
}
fn read_word<T: Copy + Default>(address: u64) -> Option<T> {
    let mut value = T::default();
    read_into(Some(cvr_iab_read), core::ptr::null_mut(), address, &mut value).then_some(value)
}
/// The walks start below the .cpp's (this module's frames and the C++ forwarder): the same frames above are searched.
const OWN_FRAMES: u32 = 4;
/// NativeIaCompatible: the three IA binding sites as this executable has them.
pub fn native_ia_compatible() -> bool {
    static RESULT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RESULT.get_or_init(|| code(0x339150, 0x24D, 0x16AF_288F_10E7_FCCF) && code(0x342554, 0x52D, 0x0E75_80FE_6E75_1E01) && code(0x339DB0, 0xDC, 0xB9D1_AE5D_AB6A_FEA5))
}
/// Native: the buffers the engine binds at its IA call up this stack -- the first frame in the image (its RVA within 32
/// bits) whose registers decode (DecodeIa).
pub fn native_buffers(index: bool, start: u32, count: u32, output: &mut [NativeBuffer; 16]) -> bool {
    if !native_ia_compatible() {
        return false;
    }
    let game = game();
    cvr_engine::stack::walk(32 + OWN_FRAMES, |c, _, _| {
        if c.Rip < game || c.Rip - game > u64::from(u32::MAX) {
            return None;
        }
        let frame = IaFrame { rva: c.Rip - game, rsp: c.Rsp, rbx: c.Rbx, rbp: c.Rbp, rdi: c.Rdi, rsi: c.Rsi, r12: c.R12, r14: c.R14, r15: c.R15 };
        decode_ia(&frame, index, start, count, Some(cvr_iab_read), core::ptr::null_mut(), output).then_some(())
    })
    .is_some()
}
/// CurrentIndirectContext: the thread's engine context ([[TLS] + 0x198], the thread initialized at +0x14), its state
/// (+0x60) and command list (+0x30); None when a read fails, else the context and whether all three are set.
pub fn current_indirect_context() -> Option<(UploadContext, bool)> {
    let tls: u64;
    // SAFETY: the TEB's ThreadLocalStoragePointer.
    unsafe { core::arch::asm!("mov {}, gs:[0x58]", out(reg) tls, options(nostack, readonly, preserves_flags)) };
    let thread: u64 = read_word(tls)?;
    if thread == 0 || thread > u64::MAX - 0x1A0 || read_word::<u8>(thread + 0x14)? == 0 {
        return None;
    }
    let context: u64 = read_word(thread + 0x198)?;
    if context == 0 || context > u64::MAX - 0x68 {
        return None;
    }
    let state: u64 = read_word(context + 0x60)?;
    let list: u64 = read_word(context + 0x30)?;
    Some((UploadContext { context, state, list }, context != 0 && state != 0 && list != 0))
}
fn native_image_bytes() -> u64 {
    static BYTES: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BYTES.get_or_init(|| crate::image_size::cvr_image_bytes_above(game(), 0x413AE0))
}
/// IndirectOrigin: the first frame up the stack inside the image -- one of the three indirect draw sites (its RBX and
/// RDI), else none.
pub fn indirect_origin(game: u64) -> Option<IndirectFrame> {
    let image = native_image_bytes();
    if game == 0 || image == 0 {
        return None;
    }
    cvr_engine::stack::walk(32 + OWN_FRAMES, |c, _, _| {
        (c.Rip >= game && c.Rip - game < image).then(|| {
            let rva = c.Rip - game;
            matches!(rva, 0x342B11 | 0xD543BB | 0x413AE0).then_some(IndirectFrame { rva, rbx: c.Rbx, rdi: c.Rdi })
        })
    })
    .flatten()
}
/// The initial state BufferCreated keeps on a buffer (its private data).
const INITIAL_STATE_KEY: windows::core::GUID = windows::core::GUID::from_values(0xb7b5_0d38, 0xf497, 0x4fc3, [0xa3, 0xc1, 0x62, 0x40, 0x21, 0x37, 0x63, 0x20]);
/// AcquireBuffer: the engine's buffer (its record's resource) owned when `bytes` at `address` lie inside both the record
/// and the resource, a buffer with its heap known.
pub fn acquire_buffer(native: &NativeBuffer, address: u64, bytes: u64) -> Option<OwnedLease> {
    if native.resource == 0 || native.gpu == 0 || bytes == 0 || address < native.gpu || address - native.gpu > native.bytes || bytes > native.bytes - (address - native.gpu) {
        return None;
    }
    let raw = native.resource as *mut c_void;
    // SAFETY: the engine's live resource, retained for the lease.
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }?.clone();
    // SAFETY: a live resource.
    let (d, gpu) = unsafe { (resource.GetDesc(), resource.GetGPUVirtualAddress()) };
    let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
    if d.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || gpu == 0 || address < gpu || address - gpu > d.Width || bytes > d.Width - (address - gpu) || unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err() {
        return None;
    }
    Some(OwnedLease { resource: resource.into_raw(), gpu, bytes: d.Width, heap_type: heap.Type.0 as u32, flags: d.Flags.0 as u32, native: *native })
}
/// ReadStateAtDraw: an upload heap's buffer is GENERIC_READ (1); a default heap's from the recording context's map
/// (ReadNativeState) once its four code paths match, or -- an unknown entry of a fixed-read buffer of class 1, which
/// bypasses both map updates -- GENERIC_READ from the state it was created in (8: COMMON or GENERIC_READ) or, for a pool
/// that predates the hooks, from the pools' constructors (9); 3 without a list or resource, 6 another heap, 7 other code.
pub fn read_state_at_draw(list: u64, source: &OwnedLease) -> ReadState {
    let kind = source.native.kind;
    let generic = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
    if list == 0 || source.resource.is_null() {
        return ReadState { value: 0, status: 3, kind };
    }
    if source.heap_type == D3D12_HEAP_TYPE_UPLOAD.0 as u32 {
        return ReadState { value: generic, status: 1, kind };
    }
    if source.heap_type != D3D12_HEAP_TYPE_DEFAULT.0 as u32 {
        return ReadState { value: 0, status: 6, kind };
    }
    static COMPATIBLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let compatible = *COMPATIBLE.get_or_init(|| {
        native_ia_compatible()
            && code(0x337DA0, 0x180, 0xB36B_2D86_8965_D718)
            && code(0x338390, 0xDF, 0x262E_AAB2_CEC6_F5E0)
            && code(0x1EAF30, 0x136, 0xE809_17A1_96B7_1137)
            && code(0x3445AC, 0x5A2, 0x612F_7F49_D216_A154)
    });
    if !compatible {
        return ReadState { value: 0, status: 7, kind };
    }
    let state = read_native_state(&source.native, list, Some(cvr_iab_read), core::ptr::null_mut());
    if state.status == 5 && kind == 1 && source.flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 as u32 == 0 {
        // SAFETY: the lease's live resource.
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&source.resource) };
        let (mut initial, mut bytes) = (u32::MAX, 4u32);
        // SAFETY: four bytes of private data at most.
        let receipt = resource.map(|r| unsafe { r.GetPrivateData(&INITIAL_STATE_KEY, &mut bytes, Some((&mut initial as *mut u32).cast())) });
        match receipt {
            Some(Ok(())) => {
                if bytes == 4 && (initial == D3D12_RESOURCE_STATE_COMMON.0 as u32 || initial == generic) {
                    return ReadState { value: generic, status: 8, kind: 1 };
                }
            }
            Some(Err(e)) if e.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND => {
                static POOLS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                if *POOLS.get_or_init(|| code(0x342F48, 0x483, 0x232E_9A0D_5DBE_1C20) && code(0x22F2D00, 0x384, 0x09BF_1A44_FA19_923F)) {
                    return ReadState { value: generic, status: 9, kind: 1 };
                }
            }
            _ => {}
        }
    }
    state
}

// ---- their C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_iab_native_compatible() -> bool {
    native_ia_compatible()
}
/// CurrentIndirectContext into `out` (written once every read succeeded).
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_current_context(out: *mut UploadContext) -> bool {
    match current_indirect_context() {
        Some((context, set)) => {
            unsafe { *out = context };
            set
        }
        None => false,
    }
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_indirect_origin(game: u64, out: *mut IndirectFrame) -> bool {
    match indirect_origin(game) {
        Some(frame) => {
            unsafe { *out = frame };
            true
        }
        None => false,
    }
}
/// BufferCreated: a buffer without unordered access keeps the state it was created in.
///
/// # Safety
/// `resource` is null or a live resource.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_buffer_created(resource: *mut c_void, state: u32) {
    // SAFETY: as documented.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else { return };
    let d = unsafe { r.GetDesc() };
    if d.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 != 0 {
        return;
    }
    // SAFETY: four bytes of private data.
    let _ = unsafe { r.SetPrivateData(&INITIAL_STATE_KEY, 4, Some((&state as *const u32).cast())) };
}
/// AcquireBuffer into the empty lease `out` (left empty when refused).
///
/// # Safety
/// `native` is valid; `out` is an empty BufferLease.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_acquire(native: *const NativeBuffer, address: u64, bytes: u64, out: *mut OwnedLease) {
    if let Some(lease) = acquire_buffer(unsafe { &*native }, address, bytes) {
        unsafe { out.write(lease) };
    }
}
/// # Safety
/// `list` is null or a live list; `source` a valid lease; `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_read_state(list: *mut c_void, source: *const OwnedLease, out: *mut ReadState) {
    unsafe { *out = read_state_at_draw(list as u64, &*source) };
}
/// BoundIndex: the engine's index buffer at the IA call (a 16- or 32-bit view) leased, into the empty `out`.
///
/// # Safety
/// `view` is valid; `out` an empty BufferLease.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_bound_index(view: *const D3D12_INDEX_BUFFER_VIEW, out: *mut OwnedLease) {
    let view = unsafe { &*view };
    let mut sources = [NativeBuffer::default(); 16];
    if view.BufferLocation == 0 || (view.Format != DXGI_FORMAT_R16_UINT && view.Format != DXGI_FORMAT_R32_UINT) || !native_buffers(true, 0, 1, &mut sources) {
        return;
    }
    unsafe { cvr_iab_acquire(&sources[0], view.BufferLocation, u64::from(view.SizeInBytes), out) };
}
/// BoundVertices: the engine's vertex buffers `start`.. at the IA call leased, into the 16 empty leases `out`.
///
/// # Safety
/// `views` holds `count` views or is null; `out` holds 16 empty BufferLeases.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_bound_vertices(start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW, out: *mut OwnedLease) {
    let mut sources = [NativeBuffer::default(); 16];
    if views.is_null() || count == 0 || start > 16 || count > 16 - start || !native_buffers(false, start, count, &mut sources) {
        return;
    }
    let views = unsafe { core::slice::from_raw_parts(views, count as usize) };
    for (i, view) in views.iter().enumerate() {
        unsafe { cvr_iab_acquire(&sources[i], view.BufferLocation, u64::from(view.SizeInBytes), out.add(i)) };
    }
}

// ---- the indirect binding (BoundIndirect, RevalidateIndirect)

/// geometry::IndirectBindingFailure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const FINGERPRINTS: u32 = 2;
    pub const DEVICE: u32 = 3;
    pub const ORIGIN: u32 = 4;
    pub const CURRENT_CONTEXT: u32 = 5;
    pub const RECORDING: u32 = 6;
    pub const DECODE: u32 = 7;
    pub const SIGNATURE: u32 = 8;
    pub const RESOURCE_RANGE: u32 = 9;
    pub const STATE: u32 = 10;
    pub const REVALIDATION: u32 = 11;
}
thread_local! {
    static FAILURE: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
fn fail(value: u32) {
    FAILURE.with(|f| f.set(value));
}
/// geometry::IndirectBinding: its lease, signature and recording token owned (references the holder releases).
#[repr(C)]
pub struct Binding {
    pub arguments: OwnedLease,
    pub signature: *mut c_void,
    pub context: UploadContext,
    pub recording: crate::native_recording::Lease,
    pub state: ReadState,
    pub raw_record: [u8; 0xB0],
    pub record_address: u64,
    pub argument_offset: u64,
    pub argument_bytes: u64,
    pub caller_rva: u32,
    pub max_count: u32,
    pub signature_kind: u32,
    pub signature_stride: u32,
    pub full_backing: bool,
}
const _: () = assert!(
    core::mem::size_of::<Binding>() == 392
        && core::mem::offset_of!(Binding, context) == 88
        && core::mem::offset_of!(Binding, recording) == 112
        && core::mem::offset_of!(Binding, raw_record) == 164
        && core::mem::offset_of!(Binding, record_address) == 344
        && core::mem::offset_of!(Binding, caller_rva) == 368
        && core::mem::offset_of!(Binding, full_backing) == 384
);
/// The indirect draw's code contract (indirect_binding_detail::Contract): the native sites the decoding reads.
const CONTRACT: [(u64, u64, u64); 13] = [
    (0x341DE8, 111, 0x14D3_1448_25DB_188C),
    (0x342AA8, 116, 0x2E6C_6DB7_32EB_83BA),
    (0xD5434C, 122, 0x5CD6_6A35_C9D8_CF94),
    (0x413A74, 114, 0x6FAE_48C9_5AD7_9B1E),
    (0x3424D4, 125, 0xB3E4_B97D_B74F_9914),
    (0x342B1C, 41, 0xB4B1_D081_7B58_E382),
    (0x337230, 51, 0x0396_F3E2_9AEB_7926),
    (0x3E3A0C, 423, 0x0EC5_161B_BFFB_F98F),
    (0x3E3BB4, 52, 0x1A0F_0B6F_177A_422C),
    (0x337DA0, 384, 0xB36B_2D86_8965_D718),
    (0x338390, 223, 0x262E_AAB2_CEC6_F5E0),
    (0x15A3530, 341, 0xFB53_7E7D_4DF0_82B8),
    (0x341E58, 1657, 0x2916_4AE9_FAF2_EB68),
];
/// NativeIndirectCompatible.
pub fn native_indirect_compatible() -> bool {
    static RESULT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RESULT.get_or_init(|| CONTRACT.iter().all(|&(rva, bytes, hash)| code(rva, bytes, hash)))
}
/// IndirectState: the arguments' state at the draw holds INDIRECT_ARGUMENT and nothing past GENERIC_READ (the map,
/// upload or class-1 fallbacks), or an unknown entry (5, no value).
pub fn indirect_state(state: &ReadState) -> bool {
    let generic = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
    (state.status == 5 && state.value == 0)
        || (matches!(state.status, 1 | 2 | 8 | 9) && state.value & D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT.0 as u32 != 0 && state.value & !generic == 0)
}
fn same_recording(a: &crate::native_recording::Lease, b: &crate::native_recording::Lease) -> bool {
    !a.token.is_null() && a.token == b.token && a.generation != 0 && a.generation == b.generation && a.context == b.context && a.list == b.list && a.registry == b.registry
}
/// SourceDevice: `object` belongs to the list's device.
fn source_device(list: &ID3D12GraphicsCommandList, object: *mut c_void) -> bool {
    // SAFETY: a live device child or null.
    let Some(object) = (unsafe { ID3D12DeviceChild::from_raw_borrowed(&object) }) else { return false };
    // SAFETY: live objects.
    let (mut expected, mut actual): (Option<ID3D12Device>, Option<ID3D12Device>) = (None, None);
    // SAFETY: live objects.
    let fetched = unsafe { list.GetDevice(&mut expected).is_ok() && object.GetDevice(&mut actual).is_ok() };
    fetched && expected.is_some() && expected == actual
}
fn validate_recording(recording: &crate::native_recording::Lease, list: &ID3D12GraphicsCommandList, context: &UploadContext) -> bool {
    // SAFETY: the caller's lease, a live list and a context of the same layout.
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording, list.as_raw(), (context as *const UploadContext).cast()) }
}
fn current_context() -> Option<UploadContext> {
    current_indirect_context().and_then(|(context, set)| set.then_some(context))
}
/// DescribeIndirectResource: the call's argument buffer -- the decoded record's own resource, of the list's device -- as a
/// lease of its whole backing, and whether the record is that whole backing (SourceRange).
fn describe_indirect_resource(list: &ID3D12GraphicsCommandList, native: &IndirectNativeRecord, actual: *mut c_void, offset: u64, bytes: u64) -> Option<(ID3D12Resource, OwnedLease, bool)> {
    if actual.is_null() || native.arguments.resource != actual as u64 || !source_device(list, actual) {
        return None;
    }
    // SAFETY: the call's live argument buffer, retained for the lease.
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&actual) }?.clone();
    let desc = unsafe { resource.GetDesc() };
    let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err() {
        return None;
    }
    let gpu = unsafe { resource.GetGPUVirtualAddress() };
    let lease = OwnedLease { resource: resource.as_raw(), gpu, bytes: desc.Width, heap_type: heap.Type.0 as u32, flags: desc.Flags.0 as u32, native: native.arguments };
    let full = source_range(&native.arguments, gpu, desc.Width, offset, bytes)?;
    Some((resource, lease, full))
}
fn call_of(list: &ID3D12GraphicsCommandList, signature: *mut c_void, arguments: u64, offset: u64, count_buffer: u64, count_offset: u64, max_count: u32) -> IndirectCall {
    IndirectCall { list: list.as_raw() as u64, signature: signature as u64, arguments, argument_offset: offset, count_buffer, count_offset, max_count }
}
fn signature_matches(signature: *mut c_void, kind: u32, stride: u32) -> bool {
    // SAFETY: a live command signature.
    let receipt = unsafe { ID3D12CommandSignature::from_raw_borrowed(&signature) }.map(crate::indirect_policy::describe).unwrap_or_default();
    crate::indirect_policy::draw_kind(&receipt) == kind && crate::indirect_policy::pure_draw_stride(&receipt) == stride
}
fn direct(list: &ID3D12GraphicsCommandList) -> bool {
    // SAFETY: a live list.
    let kind = unsafe { list.GetType() };
    kind == D3D12_COMMAND_LIST_TYPE_DIRECT
}
/// RevalidateIndirect: `saved` still the native call it was bound from -- the same recording, the origin and context,
/// two equal decodings of the same record, the same signature, backing and state -- and once more after all that.
pub fn revalidate_indirect(list: Option<&ID3D12GraphicsCommandList>, recording: &crate::native_recording::Lease, saved: &Binding, internal: bool) -> bool {
    fail(failure::ARGUMENTS);
    let Some(list) = list else { return false };
    if internal || !direct(list) || saved.arguments.resource.is_null() || saved.signature.is_null() {
        return false;
    }
    fail(failure::FINGERPRINTS);
    if !native_indirect_compatible() {
        return false;
    }
    fail(failure::RECORDING);
    if !same_recording(recording, &saved.recording) {
        return false;
    }
    fail(failure::DEVICE);
    if !source_device(list, saved.signature) {
        return false;
    }
    let game = game();
    fail(failure::ORIGIN);
    let Some(frame) = indirect_origin(game) else { return false };
    fail(failure::CURRENT_CONTEXT);
    let Some(current) = current_context().filter(|c| *c == saved.context) else { return false };
    fail(failure::ORIGIN);
    if !matches_origin(&frame, saved.caller_rva, &current, recording.registry) {
        return false;
    }
    fail(failure::RECORDING);
    if !validate_recording(recording, list, &current) {
        return false;
    }
    let call = call_of(list, saved.signature, saved.arguments.resource as u64, saved.argument_offset, 0, 0, saved.max_count);
    let expected = IndirectNativeRecord {
        arguments: saved.arguments.native,
        context: saved.context,
        raw_record: saved.raw_record,
        record_address: saved.record_address,
        signature: call.signature,
        caller_rva: saved.caller_rva,
        signature_kind: saved.signature_kind,
        signature_stride: saved.signature_stride,
    };
    fail(failure::DECODE);
    let Some(fresh) = decode(game, &frame, &current, &call, Some(cvr_iab_read), core::ptr::null_mut()) else { return false };
    if !same_record(&fresh, &expected) || fresh.arguments.registry != recording.registry || saved.argument_bytes != u64::from(saved.max_count) * u64::from(saved.signature_stride) {
        return false;
    }
    fail(failure::SIGNATURE);
    if !signature_matches(saved.signature, saved.signature_kind, saved.signature_stride) {
        return false;
    }
    fail(failure::RESOURCE_RANGE);
    let Some((_held, resource, full)) = describe_indirect_resource(list, &fresh, saved.arguments.resource, saved.argument_offset, saved.argument_bytes) else { return false };
    if resource.gpu != saved.arguments.gpu || resource.bytes != saved.arguments.bytes || resource.heap_type != saved.arguments.heap_type || resource.flags != saved.arguments.flags || full != saved.full_backing {
        return false;
    }
    let state = read_state_at_draw(list.as_raw() as u64, &resource);
    fail(failure::STATE);
    if !indirect_state(&state) || state.value != saved.state.value || state.status != saved.state.status || state.kind != saved.state.kind {
        return false;
    }
    fail(failure::REVALIDATION);
    let valid = indirect_origin(game).is_some_and(|last| {
        current_context().is_some_and(|context| {
            context == current
                && matches_origin(&last, saved.caller_rva, &context, recording.registry)
                && validate_recording(recording, list, &context)
                && decode(game, &last, &context, &call, Some(cvr_iab_read), core::ptr::null_mut()).is_some_and(|record| same_record(&record, &fresh))
        })
    });
    if valid {
        fail(failure::NONE);
    }
    valid
}
/// BoundIndirect: the actual native ExecuteIndirect callback only (one draw, no count buffer, a pure DRAW or
/// DRAW_INDEXED signature, the API's offset) bound from its decoded native record, revalidated, into the empty `out`.
///
/// # Safety
/// The pointers are the call's live objects or null; `recording` is valid; `out` an empty IndirectBinding.
#[allow(clippy::too_many_arguments)]
pub unsafe fn bound_indirect(list: Option<&ID3D12GraphicsCommandList>, signature: *mut c_void, maximum: u32, arguments: *mut c_void, offset: u64, count_buffer: *mut c_void, count_offset: u64, recording: &crate::native_recording::Lease, internal: bool, out: *mut Binding) -> bool {
    fail(failure::ARGUMENTS);
    let Some(list) = list else { return false };
    if internal || !direct(list) || signature.is_null() || arguments.is_null() || maximum != 1 || !count_buffer.is_null() || count_offset != 0 || offset % 4 != 0 {
        return false;
    }
    fail(failure::FINGERPRINTS);
    if !native_indirect_compatible() {
        return false;
    }
    fail(failure::DEVICE);
    if !source_device(list, signature) {
        return false;
    }
    let game = game();
    fail(failure::ORIGIN);
    let Some(frame) = indirect_origin(game) else { return false };
    fail(failure::CURRENT_CONTEXT);
    let Some(current) = current_context() else { return false };
    fail(failure::RECORDING);
    if !validate_recording(recording, list, &current) {
        return false;
    }
    let call = call_of(list, signature, arguments as u64, offset, count_buffer as u64, count_offset, maximum);
    fail(failure::DECODE);
    let Some(native) = decode(game, &frame, &current, &call, Some(cvr_iab_read), core::ptr::null_mut()) else { return false };
    if native.arguments.registry != recording.registry {
        return false;
    }
    fail(failure::SIGNATURE);
    if !signature_matches(signature, native.signature_kind, native.signature_stride) {
        return false;
    }
    let argument_bytes = u64::from(maximum) * u64::from(native.signature_stride);
    fail(failure::RESOURCE_RANGE);
    let Some((held, lease, full_backing)) = describe_indirect_resource(list, &native, arguments, offset, argument_bytes) else { return false };
    fail(failure::STATE);
    let state = read_state_at_draw(list.as_raw() as u64, &lease);
    if !indirect_state(&state) {
        return false;
    }
    // Borrowed while revalidated; the references are taken once it holds.
    let pending = Binding {
        arguments: lease,
        signature,
        context: current,
        recording: crate::native_recording::Lease { token: recording.token, generation: recording.generation, context: recording.context, list: recording.list, registry: recording.registry },
        state,
        raw_record: native.raw_record,
        record_address: native.record_address,
        argument_offset: offset,
        argument_bytes,
        caller_rva: native.caller_rva,
        max_count: maximum,
        signature_kind: native.signature_kind,
        signature_stride: native.signature_stride,
        full_backing,
    };
    if !revalidate_indirect(Some(list), recording, &pending, internal) {
        return false;
    }
    let mut owned = pending;
    owned.arguments.resource = held.into_raw();
    // SAFETY: the live signature and the recording's live token, retained for the binding.
    owned.signature = unsafe { ID3D12CommandSignature::from_raw_borrowed(&signature) }.map_or(core::ptr::null_mut(), |s| s.clone().into_raw());
    owned.recording.token = unsafe { windows::core::IUnknown::from_raw_borrowed(&recording.token) }.map_or(core::ptr::null_mut(), |t| t.clone().into_raw());
    unsafe { out.write(owned) };
    fail(failure::NONE);
    true
}

#[no_mangle]
pub extern "C-unwind" fn cvr_iab_last_failure() -> u32 {
    FAILURE.with(|f| f.get())
}
#[no_mangle]
pub extern "C-unwind" fn cvr_iab_indirect_compatible() -> bool {
    native_indirect_compatible()
}
/// # Safety
/// As bound_indirect.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iab_bound_indirect(list: *mut c_void, signature: *mut c_void, maximum: u32, arguments: *mut c_void, offset: u64, count_buffer: *mut c_void, count_offset: u64, recording: *const crate::native_recording::Lease, internal: bool, out: *mut Binding) -> bool {
    // SAFETY: as documented.
    unsafe { bound_indirect(ID3D12GraphicsCommandList::from_raw_borrowed(&list), signature, maximum, arguments, offset, count_buffer, count_offset, &*recording, internal, out) }
}
/// # Safety
/// `list` is null or live; `recording` and `saved` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_revalidate_indirect(list: *mut c_void, recording: *const crate::native_recording::Lease, saved: *const Binding, internal: bool) -> bool {
    // SAFETY: as documented.
    unsafe { revalidate_indirect(ID3D12GraphicsCommandList::from_raw_borrowed(&list), &*recording, &*saved, internal) }
}

#[cfg(test)]
mod frame_tests {
    //! The leases and the read state on WARP buffers; the frames refused away from the game's code.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn buffer(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, state: D3D12_RESOURCE_STATES) -> ID3D12Resource {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 256,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
        let mut out: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.unwrap();
        out.unwrap()
    }

    #[test]
    fn leases_and_read_states() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let upload = buffer(&device, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        let gpu = unsafe { upload.GetGPUVirtualAddress() };
        let native = NativeBuffer { resource: upload.as_raw() as u64, gpu, bytes: 1024, registry: 0, context: 0, handle: 1, kind: 1 };
        let lease = acquire_buffer(&native, gpu + 16, 64).expect("inside the record and the resource");
        assert!(lease.gpu == gpu && lease.bytes == 256 && lease.heap_type == D3D12_HEAP_TYPE_UPLOAD.0 as u32 && (lease.native.resource, lease.native.bytes, lease.native.kind) == (native.resource, 1024, 1));
        let generic = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
        let s = read_state_at_draw(1, &lease);
        assert_eq!((s.value, s.status, s.kind), (generic, 1, 1), "an upload heap");
        assert_eq!(read_state_at_draw(0, &lease).status, 3, "no list");
        drop(unsafe { ID3D12Resource::from_raw(lease.resource) });
        assert!(acquire_buffer(&native, gpu + 16, 300).is_none(), "past the resource, inside the record");
        assert!(acquire_buffer(&NativeBuffer { bytes: 64, ..native }, gpu + 16, 64).is_none(), "past the record");
        assert!(acquire_buffer(&NativeBuffer { resource: 0, ..native }, gpu, 64).is_none());
        let default = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
        unsafe { cvr_iab_buffer_created(default.as_raw(), D3D12_RESOURCE_STATE_COPY_DEST.0 as u32) };
        let (mut kept, mut bytes) = (0u32, 4u32);
        unsafe { default.GetPrivateData(&INITIAL_STATE_KEY, &mut bytes, Some((&mut kept as *mut u32).cast())) }.unwrap();
        assert_eq!((bytes, kept), (4, D3D12_RESOURCE_STATE_COPY_DEST.0 as u32), "the created state kept");
        let gpu = unsafe { default.GetGPUVirtualAddress() };
        let lease = acquire_buffer(&NativeBuffer { resource: default.as_raw() as u64, gpu, bytes: 256, ..native }, gpu, 256).unwrap();
        assert_eq!(read_state_at_draw(1, &lease).status, 7, "a default heap away from the game's code");
        drop(unsafe { ID3D12Resource::from_raw(lease.resource) });
        let mut sources = [NativeBuffer::default(); 16];
        assert!(!native_buffers(false, 0, 1, &mut sources) && indirect_origin(0).is_none(), "no game code, no frames");
    }
}

#[cfg(test)]
mod indirect_tests {
    //! The indirect binding's refusals before the game's code (a WARP direct list) and its state rule.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn indirect_refusals_and_states() {
        let (ia, generic) = (D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT.0 as u32, D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32);
        let state = |value: u32, status: u32| ReadState { value, status, kind: 0 };
        assert!(indirect_state(&state(0, 5)) && !indirect_state(&state(ia, 5)), "an unknown entry only without a value");
        assert!(indirect_state(&state(ia, 2)) && indirect_state(&state(generic, 1)) && indirect_state(&state(ia, 9)));
        assert!(!indirect_state(&state(ia | 0x400, 2)) && !indirect_state(&state(0x400, 2)) && !indirect_state(&state(ia, 3)), "past GENERIC_READ, without it, another status");
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let recording = crate::native_recording::Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
        let mut out: Binding = unsafe { core::mem::zeroed() };
        let fake = 0x1000 as *mut c_void;
        let bind = |list: Option<&ID3D12GraphicsCommandList>, signature: *mut c_void, maximum: u32, offset: u64, out: &mut Binding| {
            let bound = unsafe { bound_indirect(list, signature, maximum, fake, offset, core::ptr::null_mut(), 0, &recording, false, out) };
            (bound, cvr_iab_last_failure())
        };
        assert_eq!(bind(None, fake, 1, 0, &mut out), (false, failure::ARGUMENTS), "no list");
        assert_eq!(bind(Some(&list), core::ptr::null_mut(), 1, 0, &mut out), (false, failure::ARGUMENTS), "no signature");
        assert_eq!(bind(Some(&list), fake, 2, 0, &mut out), (false, failure::ARGUMENTS), "more than one draw");
        assert_eq!(bind(Some(&list), fake, 1, 2, &mut out), (false, failure::ARGUMENTS), "an offset off four bytes");
        assert_eq!(bind(Some(&list), fake, 1, 4, &mut out), (false, failure::FINGERPRINTS), "away from the game's code");
        assert!(unsafe { bound_indirect(Some(&list), fake, 1, fake, 0, core::ptr::null_mut(), 0, &recording, true, &mut out) } == false && cvr_iab_last_failure() == failure::ARGUMENTS, "the port's own commands");
        assert!(out.signature.is_null() && out.arguments.resource.is_null(), "nothing bound");
        let saved: Binding = unsafe { core::mem::zeroed() };
        assert!(!revalidate_indirect(Some(&list), &recording, &saved, false) && cvr_iab_last_failure() == failure::ARGUMENTS, "nothing to revalidate");
    }
}