//! THE NATIVE BREADCRUMB WRITES' RULES (the pure part of include/Render/NativeBreadcrumbWrite.hpp's detail namespace):
//! the engine's GPU breadcrumb markers write one immediate value (marker-in 1, marker-out 2) through
//! WriteBufferImmediate from two audited return sites; the frame of that native call (its argument holder: RDI in
//! marker-in, RBX in marker-out; the stacked parameter and mode), the marker's factory entry of its kind (stride,
//! slots, bitmap words, heap, buffer, GPU address), the slot's occupancy under no lock and the destination inside the
//! slot; the buffer's committed shape and the observed creation matching it. The header forwards to these in the Rust
//! build; tools/single_pass_tests/breadcrumb_write_test runs its cases against them (its Rust variant).
use core::ffi::c_void;

use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;

use crate::texture_immutable::Creation;

pub const MARKER_IN_RETURN: u64 = 0x3E529E;
pub const MARKER_OUT_RETURN: u64 = 0x3E5A59;

/// detail::Observation.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Observation {
    pub base: u64,
    pub registry: u64,
    pub device: u64,
    pub holder: u64,
    pub list: u64,
    pub marker: u64,
    pub entry: u64,
    pub heap: u64,
    pub resource: u64,
    pub gpu: u64,
    pub slot_gpu: u64,
    pub bitmap: u64,
    pub kind: u32,
    pub slot: u32,
    pub capacity: u32,
    pub stride: u32,
    pub slots: u32,
    pub words: u32,
}
/// detail::Frame.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Frame {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub holder: u64,
    pub stack_low: u64,
    pub stack_high: u64,
}
/// detail::FrameLayout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FrameLayout {
    pub parameters: u64,
    pub modes: u64,
    pub mode: u32,
}
const _: () = assert!(
    core::mem::size_of::<Observation>() == 120
        && core::mem::size_of::<Frame>() == 48
        && core::mem::size_of::<FrameLayout>() == 24
);

/// detail::ReadMemory.
pub type Reader = Option<unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool>;
/// detail::Read: a non-empty read at a non-null address.
fn read(reader: Reader, user: *mut c_void, at: u64, out: &mut [u8]) -> bool {
    // SAFETY: the caller's reader writes at most `out.len()` bytes.
    reader.is_some_and(|r| {
        at != 0
            && !out.is_empty()
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
/// detail::Field.
fn field<T: Copy>(
    reader: Reader,
    user: *mut c_void,
    address: u64,
    offset: u64,
    value: &mut T,
) -> bool {
    address != 0 && address <= u64::MAX - offset && read_into(reader, user, address + offset, value)
}

/// CallerMode: 1 marker-in, 2 marker-out, 0 another caller.
pub fn caller_mode(base: u64, caller: u64) -> u32 {
    if base == 0 || caller < base {
        return 0;
    }
    match caller - base {
        MARKER_IN_RETURN => 1,
        MARKER_OUT_RETURN => 2,
        _ => 0,
    }
}
/// NearestCaller: the first OS-unwound frame inside the image decides.
pub fn nearest_caller(base: u64, image_bytes: u64, frames: &[u64]) -> u32 {
    if base == 0 || image_bytes == 0 || base > u64::MAX - image_bytes || frames.len() > 64 {
        return 0;
    }
    frames
        .iter()
        .find(|&&rip| rip >= base && rip - base < image_bytes)
        .map_or(0, |&rip| caller_mode(base, rip))
}
/// Parameters: one write of the caller's own mode and value to a 4-aligned destination.
pub fn parameters(caller_mode: u32, count: u32, dest: u64, value: u32, mode: u32) -> bool {
    count == 1
        && (caller_mode == 1 || caller_mode == 2)
        && mode == caller_mode
        && value == caller_mode
        && dest != 0
        && dest & 3 == 0
        && dest <= u64::MAX - 4
}
pub fn stack_span(frame: &Frame, address: u64, bytes: u64) -> bool {
    frame.stack_low != 0
        && frame.stack_low < frame.stack_high
        && frame.stack_high >= bytes
        && address >= frame.stack_low
        && address <= frame.stack_high - bytes
}
/// Layout: the stacked parameter (rsp+0x20) and mode (rsp+0x50 in marker-in, rsp+0x40 in marker-out) of the frame.
pub fn layout(base: u64, frame: &Frame) -> Option<FrameLayout> {
    let mode = caller_mode(base, frame.rip);
    if mode == 0
        || frame.holder == 0
        || frame.holder > u64::MAX - 8
        || frame.rsp % 16 != 0
        || frame.rsp > u64::MAX - 0x54
        || !stack_span(frame, frame.rsp, 8)
    {
        return None;
    }
    if mode == 1 && frame.rbp != frame.rsp + 0x30 {
        return None;
    }
    let value = FrameLayout {
        parameters: frame.rsp + 0x20,
        modes: frame.rsp + if mode == 1 { 0x50 } else { 0x40 },
        mode,
    };
    (stack_span(frame, value.parameters, 16) && stack_span(frame, value.modes, 4)).then_some(value)
}
/// FrameFromRegisters: only the audited nonvolatile argument holder (RDI in marker-in, RBX in marker-out).
#[allow(clippy::too_many_arguments)]
pub fn frame_from_registers(
    base: u64,
    rip: u64,
    rsp: u64,
    rbp: u64,
    rdi: u64,
    rbx: u64,
    low: u64,
    high: u64,
) -> Option<Frame> {
    let mode = caller_mode(base, rip);
    if mode == 0 {
        return None;
    }
    let value = Frame {
        rip,
        rsp,
        rbp: if mode == 1 { rbp } else { 0 },
        holder: if mode == 1 { rdi } else { rbx },
        stack_low: low,
        stack_high: high,
    };
    layout(base, &value).map(|_| value)
}
/// Observe. Failures: 1 arguments/frame, 2 globals/holder, 3 marker association, 4 factory shape, 5 heap/buffer, 6
/// bitmap/lock, 7 slot range, 8 destination, 9 native stack arguments differ. The observation read so far is returned
/// with a failure.
#[allow(clippy::too_many_arguments)]
pub fn observe(
    reader: Reader,
    user: *mut c_void,
    base: u64,
    frame: &Frame,
    actual_list: u64,
    dest: u64,
    parameter_value: u32,
    mode: u32,
) -> (Observation, u32) {
    let mut value = Observation {
        base,
        holder: frame.holder,
        list: actual_list,
        ..Default::default()
    };
    let Some(layout) = (base != 0 && base <= u64::MAX - 0x3C29078 && actual_list != 0)
        .then(|| layout(base, frame))
        .flatten()
        .filter(|l| parameters(l.mode, 1, dest, parameter_value, mode))
    else {
        return (value, 1);
    };
    let mut native = [0u8; 16];
    let mut native_mode = 0u32;
    if !read(reader, user, layout.parameters, &mut native)
        || !read_into(reader, user, layout.modes, &mut native_mode)
        || u64::from_le_bytes(native[0..8].try_into().unwrap()) != dest
        || u32::from_le_bytes(native[8..12].try_into().unwrap()) != parameter_value
        || native_mode != mode
    {
        return (value, 9);
    }
    let mut stopped = 0u8;
    if !field(reader, user, base, 0x3BCDCA8, &mut value.registry)
        || value.registry == 0
        || !field(reader, user, value.registry, 1, &mut stopped)
        || stopped != 0
        || !field(reader, user, base, 0x3C28E30, &mut value.device)
        || value.device == 0
        || !field(reader, user, frame.holder, 0, &mut value.marker)
        || value.marker == 0
    {
        return (value, 2);
    }
    let mut marker_list = 0u64;
    if !field(reader, user, value.marker, 0, &mut marker_list)
        || marker_list != actual_list
        || !field(reader, user, value.marker, 8, &mut value.kind)
        || value.kind == 0
        || value.kind > 4
        || !field(reader, user, value.marker, 0x10, &mut value.capacity)
        || !field(reader, user, value.marker, 0x60, &mut value.slot_gpu)
        || value.slot_gpu == 0
        || !field(reader, user, value.marker, 0x68, &mut value.slot)
        || value.slot == 0
    {
        return (value, 3);
    }
    value.entry = base + 0x3C28E38 + value.kind as u64 * 0x70;
    let expected_slots = match value.kind {
        1 => 1024,
        4 => 64,
        _ => 8,
    };
    let expected_stride = match value.kind {
        1 => 4096,
        4 => 2048,
        _ => 512,
    };
    if !field(reader, user, value.entry, 0x20, &mut value.stride)
        || value.stride != expected_stride
        || value.capacity != expected_stride / 4
        || !field(reader, user, value.entry, 0x60, &mut value.slots)
        || value.slots != expected_slots
        || value.slot > value.slots
        || !field(reader, user, value.entry, 0x5C, &mut value.words)
        || value.words != (value.slots + 63) / 64
    {
        return (value, 4);
    }
    if !field(reader, user, value.entry, 0x30, &mut value.heap)
        || value.heap == 0
        || !field(reader, user, value.entry, 0x38, &mut value.resource)
        || value.resource == 0
        || !field(reader, user, value.entry, 0x40, &mut value.gpu)
        || value.gpu == 0
    {
        return (value, 5);
    }
    let (mut lock, mut word) = (0u8, 0u64);
    if !field(reader, user, value.entry, 0x48, &mut lock)
        || lock != 0
        || !field(reader, user, value.entry, 0x50, &mut value.bitmap)
        || value.bitmap == 0
        || !field(
            reader,
            user,
            value.bitmap,
            ((value.slot - 1) / 64) as u64 * 8,
            &mut word,
        )
        || word & (1u64 << ((value.slot - 1) % 64)) == 0
        || !field(reader, user, value.entry, 0x48, &mut lock)
        || lock != 0
    {
        return (value, 6);
    }
    let total = value.stride as u64 * value.slots as u64;
    let offset = (value.slot - 1) as u64 * value.stride as u64;
    if value.gpu > u64::MAX - total
        || value.slot_gpu != value.gpu + offset
        || value.slot_gpu > u64::MAX - value.stride as u64
    {
        return (value, 7);
    }
    if dest < value.slot_gpu || dest - value.slot_gpu > value.stride as u64 - 4 {
        return (value, 8);
    }
    (value, 0)
}
/// Shape: the factory's committed buffer -- the slots' bytes, a row-major buffer denying shader resources (and allowing
/// cross-adapter use on a cross-adapter heap), an allocation inside its heap.
pub fn shape(
    observation: &Observation,
    desc: &D3D12_RESOURCE_DESC,
    heap: &D3D12_HEAP_DESC,
    allocation: &D3D12_RESOURCE_ALLOCATION_INFO,
    gpu: u64,
) -> bool {
    let cross = heap.Flags.0 & D3D12_HEAP_FLAG_SHARED_CROSS_ADAPTER.0 != 0;
    let flags = D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE.0
        | if cross {
            D3D12_RESOURCE_FLAG_ALLOW_CROSS_ADAPTER.0
        } else {
            0
        };
    gpu == observation.gpu
        && desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Width == observation.stride as u64 * observation.slots as u64
        && desc.Height == 1
        && desc.DepthOrArraySize == 1
        && desc.MipLevels == 1
        && desc.Format == DXGI_FORMAT_UNKNOWN
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && desc.Layout == D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        && desc.Flags.0 == flags
        && allocation.SizeInBytes != 0
        && allocation.SizeInBytes != u64::MAX
        && allocation.SizeInBytes >= desc.Width
        && allocation.Alignment != 0
        && allocation.SizeInBytes <= heap.SizeInBytes
        && allocation.SizeInBytes % allocation.Alignment == 0
        && (heap.Alignment == 0 || heap.Alignment == allocation.Alignment)
}
/// ObservedCreationMatches: the API creation receipt is the factory's placed buffer at offset 0 of its heap.
pub fn observed_creation_matches(
    value: &Creation,
    native: &Observation,
    allocation_bytes: u64,
) -> bool {
    let raw = |p: Option<*mut c_void>| p.map_or(0, |v| v as u64);
    value.kind == 2
        && raw(value.resource.as_ref().map(|r| r.as_raw())) == native.resource
        && raw(value.heap.as_ref().map(|h| h.as_raw())) == native.heap
        && value.offset == 0
        && value.bytes == allocation_bytes
        && (!value.known || value.state == D3D12_RESOURCE_STATE_COPY_DEST.0)
}

// ---- the C ABI (the header's inline functions in the Rust build)

#[no_mangle]
pub extern "C" fn cvr_bc_caller_mode(base: u64, caller: u64) -> u32 {
    caller_mode(base, caller)
}
/// # Safety
/// `frames` valid for `count`.
#[no_mangle]
pub unsafe extern "C" fn cvr_bc_nearest_caller(
    base: u64,
    image_bytes: u64,
    frames: *const u64,
    count: usize,
) -> u32 {
    // SAFETY: as the caller promises.
    let frames = if count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(frames, count) }
    };
    nearest_caller(base, image_bytes, frames)
}
#[no_mangle]
pub extern "C" fn cvr_bc_parameters(
    caller_mode: u32,
    count: u32,
    dest: u64,
    value: u32,
    mode: u32,
) -> bool {
    parameters(caller_mode, count, dest, value, mode)
}
/// # Safety
/// `frame` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_bc_stack_span(frame: *const Frame, address: u64, bytes: u64) -> bool {
    // SAFETY: as the caller promises.
    stack_span(unsafe { &*frame }, address, bytes)
}
/// # Safety
/// `frame` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_bc_layout(
    base: u64,
    frame: *const Frame,
    out: *mut FrameLayout,
) -> bool {
    // SAFETY: as the caller promises.
    let value = layout(base, unsafe { &*frame });
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// `out` valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_bc_frame_from_registers(
    base: u64,
    rip: u64,
    rsp: u64,
    rbp: u64,
    rdi: u64,
    rbx: u64,
    low: u64,
    high: u64,
    out: *mut Frame,
) -> bool {
    let value = frame_from_registers(base, rip, rsp, rbp, rdi, rbx, low, high);
    // SAFETY: as the caller promises.
    unsafe { *out = value.unwrap_or_default() };
    value.is_some()
}
/// # Safety
/// `frame` and `out` valid; `failure` valid or null; `reader` reads for `user`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_bc_observe(
    reader: Reader,
    user: *mut c_void,
    base: u64,
    frame: *const Frame,
    actual_list: u64,
    dest: u64,
    value: u32,
    mode: u32,
    out: *mut Observation,
    failure: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let (observation, code) = observe(
        reader,
        user,
        base,
        unsafe { &*frame },
        actual_list,
        dest,
        value,
        mode,
    );
    unsafe { *out = observation };
    if !failure.is_null() {
        unsafe { *failure = code };
    }
    code == 0
}
/// # Safety
/// The records valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_bc_shape(
    observation: *const Observation,
    desc: *const D3D12_RESOURCE_DESC,
    heap: *const D3D12_HEAP_DESC,
    allocation: *const D3D12_RESOURCE_ALLOCATION_INFO,
    gpu: u64,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { shape(&*observation, &*desc, &*heap, &*allocation, gpu) }
}
/// # Safety
/// The records valid (the creation borrowed: nothing is released).
#[no_mangle]
pub unsafe extern "C" fn cvr_bc_observed_creation_matches(
    value: *const Creation,
    native: *const Observation,
    allocation_bytes: u64,
) -> bool {
    // SAFETY: as the caller promises.
    observed_creation_matches(unsafe { &*value }, unsafe { &*native }, allocation_bytes)
}

// ---- NativeFrame (src/Render/NativeBreadcrumbWrite.cpp): the marker write's own frame, strictly unwound

/// NativeFrame: strictly up the stack (the TIB's limits, no leaf frames) to the first frame in the image -- a marker-in
/// or marker-out return (the direct caller itself when the call came from the image), inside its function's unwind data
/// (0x3E5174 marker-in, 0x3E59DC marker-out) -- and the frame its registers describe. `details[1]` the reason refused
/// (101 no image size, 102 a direct caller of no marker, 103 off the stack, 104 not a marker frame, 105 not its
/// function, 106 no frame from its registers, 107 no unwind data, 108 no progress, 109 a fault, 110 too deep; 0 found),
/// `details[2]` the caller or the frame's return and `details[3]` its argument holder.
pub fn native_frame(base: u64, caller: u64, details: &mut [u64; 12]) -> Option<Frame> {
    let size = crate::image_size::cvr_image_bytes_above(base, MARKER_OUT_RETURN);
    if size == 0 {
        details[1] = 101;
        return None;
    }
    let direct = caller >= base && caller - base < size;
    if direct && caller_mode(base, caller) == 0 {
        details[2] = caller;
        details[1] = 102;
        return None;
    }
    // MaxUnwoundFrames, from four frames lower than the .cpp's walk.
    let walked = cvr_engine::stack::walk_strict(64 + 4, |c, low, high, function| {
        if c.Rip < base || c.Rip - base >= size {
            return None;
        }
        details[2] = c.Rip;
        let mode = caller_mode(base, c.Rip);
        if mode == 0 || (direct && c.Rip != caller) {
            return Some(Err(104));
        }
        let begin = if mode == 1 { 0x3E5174 } else { 0x3E59DC };
        if !function.is_some_and(|f| f.image == base && f.begin == begin && u64::from(f.end) > c.Rip - base) {
            return Some(Err(105));
        }
        details[3] = if mode == 1 { c.Rdi } else { c.Rbx };
        Some(frame_from_registers(base, c.Rip, c.Rsp, c.Rbp, c.Rdi, c.Rbx, low, high).ok_or(106))
    });
    use cvr_engine::stack::StrictEnd;
    let (frame, code) = match walked {
        Ok(Ok(frame)) => (Some(frame), 0),
        Ok(Err(code)) => (None, code),
        Err(StrictEnd::LeftStack) => (None, 103),
        Err(StrictEnd::NoUnwind) => (None, 107),
        Err(StrictEnd::NoProgress) => (None, 108),
        Err(StrictEnd::Fault) => (None, 109),
        Err(StrictEnd::Exhausted) => (None, 110),
    };
    details[1] = code;
    frame
}
/// NativeFrame into `out` (zero when refused).
///
/// # Safety
/// `details` holds 12 words; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bc_native_frame(base: u64, caller: u64, details: *mut [u64; 12], out: *mut Frame) -> bool {
    // SAFETY: as documented.
    let frame = native_frame(base, caller, unsafe { &mut *details });
    unsafe { *out = frame.unwrap_or_default() };
    frame.is_some()
}

#[cfg(test)]
mod native_frame_tests {
    use super::*;

    #[test]
    fn native_frame_refusals() {
        let mut details = [0u64; 12];
        assert!(native_frame(0x10000, 0, &mut details).is_none() && details[1] == 101, "no image there");
        // SAFETY: the test executable's own module.
        let exe = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 };
        assert!(native_frame(exe, 0, &mut details).is_none());
        assert_eq!(details[1], 104, "the first frame in the image is no marker's");
        assert!(details[2] >= exe, "its return noted");
        assert!(native_frame(exe, exe + 0x10, &mut details).is_none() && details[1] == 102 && details[2] == exe + 0x10, "a direct caller of no marker");
    }
}
