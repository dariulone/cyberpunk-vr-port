//! THE SOURCE SRV BINDINGS' RULES (the pure part of include/Render/NativeSourceSrvBinding.hpp's detail namespace): the
//! engine's pixel-stage SRV setters (individual: a handle pointer in RCX, register in RDX, stage in R8; cached: a bundle
//! in RCX, stage in RDX) nominate a buffer for t4 / t5 / t6; the candidate's handle (read twice), the bound state's
//! descriptor-table slot of that register (layout group, table entry, cached index or CPU descriptor), the mode join
//! between them, the registry's buffer record, and the view's logical range inside the native buffer (raw t4,
//! structured t5 / t6). The header forwards to these in the Rust build; tools/single_pass_tests/source_srv_binding_test
//! runs its cases against them (its Rust variant).
use core::ffi::c_void;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_UNKNOWN};

use crate::buffer_read::NativeBuffer;

pub const SHADER_REGISTER: u32 = 5;
pub const STAGE_INDEX: u32 = 1;
pub const ROOT_PARAMETER: u32 = 23;
pub const MAX_HANDLES: u64 = 0x100000;
pub const MAX_XSTATE_BYTES: u64 = 16384;
pub const INDIVIDUAL: u32 = 0;
pub const CACHED: u32 = 1;

pub fn tracked_register(value: u32) -> bool {
    (4..=6).contains(&value)
}
/// The registers of source_srv::ThunkFrame this reads (its prefix).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FrameRegisters {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
}
/// source_srv::Slot.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Slot {
    pub layout: u64,
    pub table: u64,
    pub entry: u64,
    pub cpu: u64,
    pub group: u32,
    pub first: u32,
    pub count: u32,
    pub start: u32,
    pub index: u32,
    pub offset: u32,
    pub parameter: u32,
    pub cached_index: u32,
    pub raw: [u8; 16],
}
/// source_srv::CandidateInput.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CandidateInput {
    pub route: u32,
    pub stage: u32,
    pub shader_register: u32,
    pub handle: u32,
    pub bundle: u32,
    pub bundle_index: u32,
    pub handle_pointer: u64,
    pub bundle_entry: u64,
    pub bundle_raw: [u8; 8],
}
const _: () =
    assert!(core::mem::size_of::<Slot>() == 80 && core::mem::size_of::<CandidateInput>() == 48);
/// geometry::MemoryReader.
pub type Reader = Option<unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool>;

/// detail::Read.
fn read(reader: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
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
    read(reader, user, at, bytes)
}
/// detail::Stable: two equal reads.
fn stable<const N: usize>(reader: Reader, user: *mut c_void, at: u64, bytes: &mut [u8; N]) -> bool {
    let mut again = [0u8; N];
    read(reader, user, at, bytes) && read(reader, user, at, &mut again) && *bytes == again
}
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

/// detail::Relevant: a call of the route for the pixel stage (and, individually, a tracked register).
pub fn relevant(route: u32, frame: &FrameRegisters) -> bool {
    if route == INDIVIDUAL {
        tracked_register(frame.rdx as u32) && frame.r8 as u8 as u32 == STAGE_INDEX
    } else {
        route == CACHED && frame.rdx as u8 as u32 == STAGE_INDEX
    }
}
/// detail::DecodeCandidate: the handle the call binds (individually through its pointer, cached through the
/// registry's bundle entry of the register).
pub fn decode_candidate(
    route: u32,
    frame: &FrameRegisters,
    registry: u64,
    reader: Reader,
    user: *mut c_void,
    shader_register: u32,
) -> Option<CandidateInput> {
    if !relevant(route, frame)
        || !tracked_register(shader_register)
        || (route == INDIVIDUAL && frame.rdx as u32 != shader_register)
    {
        return None;
    }
    let mut next = CandidateInput {
        route,
        stage: STAGE_INDEX,
        shader_register,
        ..Default::default()
    };
    if route == INDIVIDUAL {
        let mut bytes = [0u8; 4];
        next.handle_pointer = frame.rcx;
        if !stable(reader, user, next.handle_pointer, &mut bytes) {
            return None;
        }
        next.handle = u32_at(&bytes, 0);
    } else {
        next.bundle = frame.rcx as u32;
        if registry == 0 || next.bundle == 0 || next.bundle as u64 > MAX_HANDLES {
            return None;
        }
        let offset = 0xB50B00 + (next.bundle as u64 - 1) * 0x44 + shader_register as u64 * 8;
        if registry > u64::MAX - offset - 8 {
            return None;
        }
        next.bundle_entry = registry + offset;
        next.bundle_index = (next.bundle - 1).wrapping_mul(8);
        if !stable(reader, user, next.bundle_entry, &mut next.bundle_raw) || next.bundle_raw[4] != 2
        {
            return None;
        }
        next.handle = u32_at(&next.bundle_raw, 0);
    }
    (next.handle != 0 && next.handle as u64 <= MAX_HANDLES).then_some(next)
}
/// detail::ReadSlot: the bound state's descriptor-table slot of the register (root parameter 23, 8 descriptors from
/// register 0), with its cached index or CPU descriptor.
pub fn read_slot(
    reader: Reader,
    user: *mut c_void,
    state: u64,
    shader_register: u32,
) -> Option<Slot> {
    if !tracked_register(shader_register) || state == 0 || state > u64::MAX - 0x180 {
        return None;
    }
    let mut next = Slot {
        cached_index: u32::MAX,
        ..Default::default()
    };
    if !read_into(reader, user, state + 0x68, &mut next.layout)
        || next.layout == 0
        || next.layout > u64::MAX - 0x5D0
        || !read_into(reader, user, state + 0x28, &mut next.table)
    {
        return None;
    }
    let mut group = 0u8;
    if !read_into(
        reader,
        user,
        next.layout + 0x4C3 + STAGE_INDEX as u64 * 0x100 + shader_register as u64 * 2,
        &mut group,
    ) || group >= 64
    {
        return None;
    }
    next.group = group as u32;
    next.entry = next.layout + 0x38 + group as u64 * 16;
    if !stable(reader, user, next.entry, &mut next.raw)
        || u32_at(&next.raw, 0) != 0
        || next.raw[13] == 2
    {
        return None;
    }
    next.first = u16_at(&next.raw, 8) as u32;
    next.count = u16_at(&next.raw, 10) as u32;
    next.start = u32_at(&next.raw, 4);
    next.parameter = next.raw[14] as u32;
    if next.first != 0
        || next.count != 8
        || next.parameter != ROOT_PARAMETER
        || next.start > 8192 - next.count
    {
        return None;
    }
    next.offset = shader_register - next.first;
    next.index = next.start + next.offset;
    if !read_into(
        reader,
        user,
        state + 0x80 + group as u64 * 4,
        &mut next.cached_index,
    ) {
        return None;
    }
    if next.cached_index == u32::MAX {
        if next.table == 0
            || next.table > u64::MAX - next.index as u64 * 8 - 8
            || !read_into(
                reader,
                user,
                next.table + next.index as u64 * 8,
                &mut next.cpu,
            )
            || next.cpu == 0
        {
            return None;
        }
    } else if next.cached_index > i32::MAX as u32 {
        return None;
    }
    Some(next)
}
/// detail::MatchesMode: an individual binding has no cached index; a cached one the bundle's.
pub fn matches_mode(candidate: &CandidateInput, slot: &Slot) -> bool {
    if candidate.route == INDIVIDUAL {
        slot.cached_index == u32::MAX
    } else {
        candidate.route == CACHED && slot.cached_index == candidate.bundle_index
    }
}
/// detail::CachedGroup: the slot's group is the stage's first group.
pub fn cached_group(reader: Reader, user: *mut c_void, slot: &Slot) -> bool {
    let mut first = 0u8;
    read_into(
        reader,
        user,
        slot.layout + 0x4C3 + STAGE_INDEX as u64 * 0x100,
        &mut first,
    ) && first as u32 == slot.group
}
/// detail::ReadRecord: the registry's buffer record of the handle (read twice), as a native buffer.
pub fn read_record(
    reader: Reader,
    user: *mut c_void,
    registry: u64,
    context: u64,
    handle: u32,
) -> (bool, [u8; 0xB0], u64, NativeBuffer) {
    let mut raw = [0u8; 0xB0];
    if registry == 0 || context == 0 || handle == 0 || handle as u64 > MAX_HANDLES {
        return (false, raw, 0, NativeBuffer::default());
    }
    let offset = 0x5C0AF8 + (handle as u64 - 1) * 0xB0;
    if registry > u64::MAX - offset - raw.len() as u64 {
        return (false, raw, 0, NativeBuffer::default());
    }
    let address = registry + offset;
    if !stable(reader, user, address, &mut raw) {
        return (false, raw, address, NativeBuffer::default());
    }
    let native = NativeBuffer {
        resource: u64_at(&raw, 0x10),
        gpu: u64_at(&raw, 8),
        bytes: u32_at(&raw, 0) as u64,
        registry,
        context,
        handle,
        kind: (raw[6] >> 4) as u32,
    };
    let ok = native.kind != 4
        && native.resource != 0
        && native.gpu != 0
        && native.bytes != 0
        && native.gpu <= u64::MAX - native.bytes;
    (ok, raw, address, native)
}
/// detail::ViewRange: the view's logical byte range (raw t4, structured t5 with 128-byte and t6 with 64-byte stride)
/// inside the resource and the native buffer.
pub fn view_range(
    native: &NativeBuffer,
    gpu: u64,
    backing_bytes: u64,
    view: &D3D12_SHADER_RESOURCE_VIEW_DESC,
    shader_register: u32,
) -> Option<(u64, u64)> {
    let stride: u64 = match shader_register {
        4 => 4,
        5 => 128,
        _ => 64,
    };
    // SAFETY: the Buffer arm of the union (the dimension is checked with it).
    let buffer = unsafe { view.Anonymous.Buffer };
    if !tracked_register(shader_register)
        || gpu == 0
        || backing_bytes == 0
        || gpu > u64::MAX - backing_bytes
        || view.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        || buffer.NumElements == 0
        || buffer.FirstElement > u64::MAX / stride
    {
        return None;
    }
    if shader_register == 4 {
        if view.Format != DXGI_FORMAT_R32_TYPELESS
            || buffer.Flags != D3D12_BUFFER_SRV_FLAG_RAW
            || buffer.StructureByteStride != 0
        {
            return None;
        }
    } else if view.Format != DXGI_FORMAT_UNKNOWN
        || buffer.Flags.0 != 0
        || buffer.StructureByteStride as u64 != stride
    {
        return None;
    }
    let begin = buffer.FirstElement * stride;
    let span = buffer.NumElements as u64 * stride;
    if begin > backing_bytes
        || span > backing_bytes - begin
        || native.gpu < gpu
        || native.gpu - gpu > backing_bytes
        || native.bytes > backing_bytes - (native.gpu - gpu)
        || gpu + begin < native.gpu
        || gpu + begin - native.gpu > native.bytes
        || span > native.bytes - (gpu + begin - native.gpu)
    {
        return None;
    }
    Some((begin, span))
}
pub fn valid_xstate(enabled: u64, supported: u64, bytes: u64) -> bool {
    enabled & 3 == 3 && enabled & !supported == 0 && bytes >= 576 && bytes <= MAX_XSTATE_BYTES
}

// ---- the C ABI (the header's inline functions in the Rust build)

/// # Safety
/// `frame` a ThunkFrame (its register prefix read).
#[no_mangle]
pub unsafe extern "C" fn cvr_srv_relevant(route: u32, frame: *const FrameRegisters) -> bool {
    // SAFETY: as the caller promises.
    relevant(route, unsafe { &*frame })
}
/// # Safety
/// `frame` and `out` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_srv_decode_candidate(
    route: u32,
    frame: *const FrameRegisters,
    registry: u64,
    reader: Reader,
    user: *mut c_void,
    out: *mut CandidateInput,
    shader_register: u32,
) -> bool {
    // SAFETY: as the caller promises.
    let value = decode_candidate(
        route,
        unsafe { &*frame },
        registry,
        reader,
        user,
        shader_register,
    );
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// `out` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_srv_read_slot(
    reader: Reader,
    user: *mut c_void,
    state: u64,
    out: *mut Slot,
    shader_register: u32,
) -> bool {
    let value = read_slot(reader, user, state, shader_register);
    // SAFETY: as the caller promises.
    unsafe {
        *out = value.unwrap_or(Slot {
            cached_index: u32::MAX,
            ..Default::default()
        })
    };
    value.is_some()
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_srv_matches_mode(
    candidate: *const CandidateInput,
    slot: *const Slot,
) -> bool {
    // SAFETY: as the caller promises.
    matches_mode(unsafe { &*candidate }, unsafe { &*slot })
}
/// # Safety
/// `slot` valid; `reader` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_srv_cached_group(
    reader: Reader,
    user: *mut c_void,
    slot: *const Slot,
) -> bool {
    // SAFETY: as the caller promises.
    cached_group(reader, user, unsafe { &*slot })
}
/// # Safety
/// The outputs valid; `reader` reads for `user`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_srv_read_record(
    reader: Reader,
    user: *mut c_void,
    registry: u64,
    context: u64,
    handle: u32,
    raw: *mut [u8; 0xB0],
    address: *mut u64,
    native: *mut NativeBuffer,
) -> bool {
    let (ok, bytes, at, value) = read_record(reader, user, registry, context, handle);
    // SAFETY: as the caller promises.
    unsafe {
        *raw = bytes;
        *address = at;
        *native = value;
    }
    ok
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_srv_view_range(
    native: *const NativeBuffer,
    gpu: u64,
    backing_bytes: u64,
    view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    offset: *mut u64,
    bytes: *mut u64,
    shader_register: u32,
) -> bool {
    // SAFETY: as the caller promises.
    let value = view_range(
        unsafe { &*native },
        gpu,
        backing_bytes,
        unsafe { &*view },
        shader_register,
    );
    let (o, b) = value.unwrap_or((0, 0));
    unsafe {
        *offset = o;
        *bytes = b;
    }
    value.is_some()
}
#[no_mangle]
pub extern "C" fn cvr_srv_valid_xstate(enabled: u64, supported: u64, bytes: u64) -> bool {
    valid_xstate(enabled, supported, bytes)
}
