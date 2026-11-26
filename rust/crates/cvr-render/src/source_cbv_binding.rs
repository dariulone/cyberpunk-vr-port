//! The source CBV binding's rules (`include/Render/NativeSourceCbvBinding.hpp`, `source_cbv::detail`): the tracked stage
//! masks; a stage's register slot read from the native pipeline state (layout +0x68, table +0x28, the stage/register
//! group byte at layout +0x453, the 16-byte range entry at layout +0x38, the CPU descriptor in the table); a CBV
//! descriptor's view inside its heap; the 2.5 creator frames (the standalone uploader 0x333C1A, the direct VS b5 creator
//! 0x3362A0 and its completed memcpy 0x33626F, the pooled writer 0x3346B1 with the registry's resource at +0x14F2240
//! and the record's key and handle); the slot selection, view and origin identities; the upload writer's exact native
//! return chain (0x3346B1 / 0x334469 / 0x33434B) and its per-thread ledger of nested writer scopes (8 deep, each
//! joined to its creation once and finished by its own stamp).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions and `UploadWriterLedger`'s methods forward to `cvr_scb_*`;
//! the ledger's members stay in C++, mirrored here. `tools/single_pass_tests/source_cbv_binding_test` runs its cases
//! against them in its Rust variant.

use core::ffi::c_void;

use crate::buffer_read::NativeBuffer;

pub const DIRECT_CREATOR_RVA: u32 = 0x3362A0;
pub const DIRECT_MEMCPY_RETURN_RVA: u32 = 0x33626F;
pub const STANDALONE_CREATOR_RVA: u32 = 0x333C1A;
pub const POOLED_CREATOR_RVA: u32 = 0x3346B1;
pub const UPLOAD_WRITER_RVA: u32 = 0x334328;
pub const UPLOAD_WRITER_MEMCPY_RETURN_RVA: u32 = 0x33435E;
pub const MAX_UPLOAD_WRITER_DEPTH: usize = 8;

/// source_cbv::Slot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub layout: u64,
    pub table: u64,
    pub address: u64,
    pub descriptor: u64,
    pub group: u32,
    pub parameter: u32,
    pub index: u32,
    pub offset: u32,
    pub first: u32,
    pub count: u32,
}
/// detail::CreationFrame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CreationFrame {
    pub rva: u64,
    pub rsp: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}
/// detail::Creation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Creation {
    pub resource: u64,
    pub offset: u64,
    pub cpu: u64,
    pub context: u64,
    pub caller: u32,
    pub handle: u32,
    pub logical_bytes: u32,
    pub reserved_bytes: u32,
    pub completed_producer_rva: u32,
}
/// descriptors::DescriptorStamp.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DescriptorStamp {
    pub gpu: u64,
    pub heap: u64,
    pub value: [u64; 20],
}
/// detail::UploadWriterContext.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadWriterContext {
    pub thread: u64,
    pub token: u64,
    pub list: u64,
    pub generation: u64,
    pub context: u64,
    pub registry: u64,
}
impl UploadWriterContext {
    pub fn valid(&self) -> bool {
        self.thread != 0 && self.token != 0 && self.list != 0 && self.generation != 0 && self.context != 0 && self.registry != 0
    }
}
/// detail::UploadWriterStamp.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadWriterStamp {
    pub nonce: u64,
    pub depth: u32,
}
/// detail::UploadWriterJoin.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadWriterJoin {
    pub origin: u64,
    pub destination: u64,
    pub resource: u64,
    pub offset: u64,
    pub handle: u32,
    pub logical_bytes: u32,
    pub reserved_bytes: u32,
}
/// detail::UploadWriterCompletion.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadWriterCompletion {
    pub nonce: u64,
    pub origin: u64,
    pub destination: u64,
    pub bytes: u32,
    pub producer_rva: u32,
}
/// UploadWriterLedger's entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WriterEntry {
    pub stamp: UploadWriterStamp,
    pub context: UploadWriterContext,
    pub joined: UploadWriterJoin,
    pub handle: u32,
    pub bytes: u32,
    pub valid: bool,
}
/// detail::UploadWriterLedger's members.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UploadWriterLedger {
    pub entries: [WriterEntry; MAX_UPLOAD_WRITER_DEPTH],
    pub depth: u32,
    pub nonce: u64,
}
const _: () = assert!(
    core::mem::size_of::<Slot>() == 56
        && core::mem::size_of::<CreationFrame>() == 80
        && core::mem::size_of::<Creation>() == 56
        && core::mem::size_of::<DescriptorStamp>() == 176
        && core::mem::size_of::<UploadWriterContext>() == 48
        && core::mem::size_of::<UploadWriterStamp>() == 16
        && core::mem::size_of::<UploadWriterJoin>() == 48
        && core::mem::size_of::<UploadWriterCompletion>() == 32
        && core::mem::size_of::<WriterEntry>() == 128
        && core::mem::size_of::<UploadWriterLedger>() == 1040
);

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;
fn bounded(read: &mut Option<Read<'_>>, at: u64, out: &mut [u8]) -> bool {
    match read {
        Some(read) => at != 0 && at <= u64::MAX - out.len() as u64 && read(at, out),
        None => false,
    }
}
fn word(read: &mut Option<Read<'_>>, at: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    bounded(read, at, &mut b).then(|| u64::from_le_bytes(b))
}
fn dword(read: &mut Option<Read<'_>>, at: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    bounded(read, at, &mut b).then(|| u32::from_le_bytes(b))
}

/// detail::TrackStageMask: (accepted, tracked).
pub fn track_stage_mask(stages: u32) -> (bool, u32) {
    if stages != 1 && stages != 2 && stages != 3 && stages != 7 { (false, 0) } else { (true, stages & 3) }
}
/// detail::ReadSlot.
pub fn read_slot(mut read: Option<Read<'_>>, state: u64, stage: u32, reg: u32) -> Option<Slot> {
    if state == 0 || state > u64::MAX - 0x70 || stage > 1 || reg > 13 {
        return None;
    }
    let mut next = Slot::default();
    next.layout = word(&mut read, state + 0x68)?;
    next.table = word(&mut read, state + 0x28)?;
    if next.layout == 0 || next.table == 0 || next.layout > u64::MAX - 0x48A {
        return None;
    }
    let mut group = [0u8; 1];
    if !bounded(&mut read, next.layout + 0x453 + stage as u64 * 0x1C + reg as u64 * 2, &mut group) || group[0] >= 64 {
        return None;
    }
    next.group = group[0] as u32;
    let entry = next.layout + 0x38 + group[0] as u64 * 16;
    let mut bytes = [0u8; 16];
    if !bounded(&mut read, entry, &mut bytes)
        || u32::from_le_bytes(bytes[0..4].try_into().unwrap()) != 2
        || bytes[13] == 2
        || bytes[14] >= 64
    {
        return None;
    }
    next.first = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as u32;
    next.count = u16::from_le_bytes(bytes[10..12].try_into().unwrap()) as u32;
    next.parameter = bytes[14] as u32;
    next.offset = reg.wrapping_sub(next.first);
    let start = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if reg < next.first || next.count == 0 || next.offset >= next.count || start >= 8192 || next.count > 8192 - start {
        return None;
    }
    next.index = start + next.offset;
    if next.table > u64::MAX - next.index as u64 * 8 - 8 {
        return None;
    }
    next.address = next.table + next.index as u64 * 8;
    next.descriptor = word(&mut read, next.address)?;
    if next.descriptor == 0 {
        return None;
    }
    Some(next)
}
/// detail::Descriptor.
pub fn descriptor(gpu: u64, width: u64, address: u64, bytes: u32) -> bool {
    gpu != 0
        && width != 0
        && gpu <= u64::MAX - width
        && address != 0
        && address & 255 == 0
        && bytes != 0
        && bytes <= 65536
        && bytes & 255 == 0
        && address >= gpu
        && address - gpu <= width
        && bytes as u64 <= width - (address - gpu)
}
/// detail::DirectProducer.
pub fn direct_producer(creation: &Creation, view_bytes: u32) -> bool {
    creation.caller == DIRECT_CREATOR_RVA
        && creation.completed_producer_rva == DIRECT_MEMCPY_RETURN_RVA
        && creation.logical_bytes != 0
        && creation.logical_bytes <= 65536
        && creation.reserved_bytes != 0
        && creation.reserved_bytes <= 65536
        && creation.reserved_bytes & 255 == 0
        && creation.reserved_bytes == ((creation.logical_bytes + 255) & !255)
        && view_bytes == creation.reserved_bytes
}
/// detail::DecodeCreation.
pub fn decode_creation(frame: &CreationFrame, mut read: Option<Read<'_>>) -> Option<Creation> {
    let mut next = Creation { caller: frame.rva as u32, ..Default::default() };
    if frame.rva == STANDALONE_CREATOR_RVA as u64 {
        if frame.rsp > u64::MAX - 0x40 {
            return None;
        }
        next.resource = word(&mut read, frame.rsp + 0x30)?;
        next.offset = word(&mut read, frame.rsp + 0x38)?;
        next.cpu = frame.rbx;
    } else if frame.rva == DIRECT_CREATOR_RVA as u64 {
        if frame.r13 == 0 || frame.r13 > 65536 || frame.r12 == 0 || frame.r12 > 65536 || frame.r12 & 255 != 0 || frame.r12 != ((frame.r13 + 255) & !255) {
            return None;
        }
        next.resource = frame.rbx;
        next.offset = frame.r15;
        next.context = frame.r14;
        next.logical_bytes = frame.r13 as u32;
        next.reserved_bytes = frame.r12 as u32;
        next.completed_producer_rva = DIRECT_MEMCPY_RETURN_RVA;
        if next.context > u64::MAX - 0x80 {
            return None;
        }
        next.cpu = word(&mut read, next.context + 0x78)?;
    } else if frame.rva == POOLED_CREATOR_RVA as u64 {
        next.offset = frame.r12;
        next.context = frame.r13;
        if frame.r14 == 0 || frame.r14 > u64::MAX - 0x14F2248 || frame.rdi < 0xC || frame.rdi > u64::MAX - 0x30 {
            return None;
        }
        next.resource = word(&mut read, frame.r14 + 0x14F2240)?;
        next.cpu = word(&mut read, frame.rdi + 0x28)?;
        let key = dword(&mut read, frame.rdi - 0xC)?;
        next.handle = dword(&mut read, frame.rdi - 8)?;
        if key != next.handle || key == 0 || key > 0x100000 {
            return None;
        }
    } else {
        return None;
    }
    if next.resource == 0 || next.cpu == 0 {
        return None;
    }
    Some(next)
}
/// detail::SameView.
pub fn same_view(stamp: &DescriptorStamp, address: u64, bytes: u32) -> bool {
    stamp.gpu != 0 && stamp.heap != 0 && stamp.value[0] == 1 && stamp.value[4] == address && stamp.value[5] == bytes as u64
}
/// detail::SameOrigin.
pub fn same_origin(stamp: &DescriptorStamp, cookie: u64, id: u64) -> bool {
    cookie != 0 && id != 0 && stamp.value[2] == cookie && stamp.value[3] == id
}
/// detail::DirectCreation.
pub fn direct_creation(stage: u32, reg: u32, caller: u32) -> bool {
    stage == 0 && reg == 5 && caller == DIRECT_CREATOR_RVA
}
/// detail::SelectSlot.
pub fn select_slot(read: Option<Read<'_>>, state: u64, stage: u32, reg: u32, descriptor: u64, require_current: bool) -> Option<Slot> {
    if stage > 1 || reg > 13 || descriptor == 0 {
        return None;
    }
    if let Some(current) = read_slot(read, state, stage, reg).filter(|current| current.descriptor == descriptor) {
        return Some(current);
    }
    if require_current {
        return None;
    }
    Some(Slot { descriptor, ..Default::default() })
}
/// detail::SameSourceSlot.
pub fn same_source_slot(a: &Slot, b: &Slot) -> bool {
    a.descriptor != 0 && a.descriptor == b.descriptor
}
/// detail::SameBuffer.
pub fn same_buffer(x: &NativeBuffer, y: &NativeBuffer) -> bool {
    x.resource == y.resource
        && x.gpu == y.gpu
        && x.bytes == y.bytes
        && x.registry == y.registry
        && x.context == y.context
        && x.handle == y.handle
        && x.kind == y.kind
}
/// detail::UploadWriterPath: the exact nearest native return chain from the OS unwinder.
pub fn upload_writer_path(returns: &[u64]) -> bool {
    returns.len() == 3 && returns[0] == 0x3346B1 && returns[1] == 0x334469 && returns[2] == 0x33434B
}

impl UploadWriterLedger {
    pub fn current(&self) -> UploadWriterStamp {
        if self.depth != 0 { self.entries[self.depth as usize - 1].stamp } else { UploadWriterStamp::default() }
    }
    pub fn enter(&mut self, context: &UploadWriterContext, handle: u32, bytes: u32) -> UploadWriterStamp {
        for entry in &mut self.entries[..self.depth as usize] {
            entry.valid = false;
        }
        if !context.valid() || handle == 0 || handle > 0x100000 || bytes == 0 || bytes > 65536 {
            return UploadWriterStamp::default();
        }
        if self.depth as usize >= MAX_UPLOAD_WRITER_DEPTH || self.nonce == u64::MAX {
            return UploadWriterStamp::default();
        }
        let outermost = self.depth == 0;
        self.nonce += 1;
        let stamp = UploadWriterStamp { nonce: self.nonce, depth: self.depth + 1 };
        self.entries[self.depth as usize] = WriterEntry { stamp, context: *context, joined: UploadWriterJoin::default(), handle, bytes, valid: outermost };
        self.depth += 1;
        stamp
    }
    pub fn join(&mut self, stamp: &UploadWriterStamp, context: &UploadWriterContext, joined: &UploadWriterJoin) -> bool {
        if self.depth == 0 || stamp.nonce == 0 || self.current() != *stamp {
            return false;
        }
        let entry = &mut self.entries[self.depth as usize - 1];
        if !entry.valid
            || entry.joined.origin != 0
            || entry.context != *context
            || entry.handle != joined.handle
            || joined.origin == 0
            || joined.destination == 0
            || joined.resource == 0
            || joined.logical_bytes == 0
            || joined.logical_bytes > 65536
            || joined.reserved_bytes == 0
            || joined.reserved_bytes > 65536
            || joined.reserved_bytes != ((joined.logical_bytes + 255) & !255)
            || entry.bytes > joined.logical_bytes
            || joined.destination > u64::MAX - entry.bytes as u64
        {
            entry.valid = false;
            return false;
        }
        entry.joined = *joined;
        true
    }
    pub fn finish(&mut self, stamp: &UploadWriterStamp, context: &UploadWriterContext) -> Option<UploadWriterCompletion> {
        if self.depth == 0 || stamp.nonce == 0 || self.current() != *stamp {
            self.invalidate();
            return None;
        }
        self.depth -= 1;
        let entry = self.entries[self.depth as usize];
        self.entries[self.depth as usize] = WriterEntry::default();
        if !entry.valid || entry.joined.origin == 0 || entry.context != *context {
            return None;
        }
        Some(UploadWriterCompletion {
            nonce: stamp.nonce,
            origin: entry.joined.origin,
            destination: entry.joined.destination,
            bytes: entry.bytes,
            producer_rva: UPLOAD_WRITER_MEMCPY_RETURN_RVA,
        })
    }
    pub fn cancel(&mut self, stamp: &UploadWriterStamp) -> bool {
        if self.depth == 0 || stamp.nonce == 0 || self.current() != *stamp {
            return false;
        }
        self.depth -= 1;
        self.entries[self.depth as usize] = WriterEntry::default();
        true
    }
    pub fn invalidate(&mut self) {
        for entry in &mut self.entries[..self.depth as usize] {
            entry.valid = false;
        }
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// geometry::MemoryReader.
pub type MemoryReader = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
fn reader(read: Option<MemoryReader>, user: *mut c_void) -> Option<impl FnMut(u64, &mut [u8]) -> bool> {
    read.map(move |read| move |at: u64, out: &mut [u8]| unsafe { read(user, at, out.as_mut_ptr().cast(), out.len()) })
}

/// TrackStageMask (`tracked` always written).
///
/// # Safety
/// `tracked` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_track_stage_mask(stages: u32, tracked: *mut u32) -> bool {
    let (ok, value) = track_stage_mask(stages);
    unsafe { *tracked = value };
    ok
}
/// ReadSlot (`which` 0) / SelectSlot (1, with `descriptor` and `require_current`); `out` zeroed, written on success.
///
/// # Safety
/// `out` is writable; `read` follows geometry::MemoryReader.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_scb_slot(
    which: u32,
    read: Option<MemoryReader>,
    user: *mut c_void,
    state: u64,
    stage: u32,
    reg: u32,
    descriptor: u64,
    require_current: bool,
    out: *mut Slot,
) -> bool {
    let mut adapter = reader(read, user);
    let read = adapter.as_mut().map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool);
    let result = if which == 0 { read_slot(read, state, stage, reg) } else { select_slot(read, state, stage, reg, descriptor, require_current) };
    unsafe { *out = result.unwrap_or_default() };
    result.is_some()
}
/// Descriptor.
#[no_mangle]
pub extern "C-unwind" fn cvr_scb_descriptor(gpu: u64, width: u64, address: u64, bytes: u32) -> bool {
    descriptor(gpu, width, address, bytes)
}
/// DecodeCreation (`output` zeroed, written on success).
///
/// # Safety
/// `frame` is valid; `output` is writable; `read` follows geometry::MemoryReader.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_decode_creation(frame: *const CreationFrame, read: Option<MemoryReader>, user: *mut c_void, output: *mut Creation) -> bool {
    let mut adapter = reader(read, user);
    let result = unsafe { decode_creation(&*frame, adapter.as_mut().map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool)) };
    unsafe { *output = result.unwrap_or_default() };
    result.is_some()
}
/// DirectProducer (`which` 0: `value` the creation, `a` the view bytes), SameView (1: `value` the stamp, `b` the address,
/// `a` the bytes), SameOrigin (2: `value` the stamp, `b` the cookie, `c` the id), DirectCreation (3: `a` stage, `b`
/// register, `c` caller), SameSourceSlot (4: `value`, `other` slots), SameBuffer (5: `value`, `other` buffers),
/// UploadWriterPath (6: `value` the returns, `b` their count), UploadWriterContext::Valid (7: `value`).
///
/// # Safety
/// The records are valid where used.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_check(which: u32, value: *const c_void, other: *const c_void, a: u32, b: u64, c: u64) -> bool {
    unsafe {
        match which {
            0 => direct_producer(&*value.cast::<Creation>(), a),
            1 => same_view(&*value.cast::<DescriptorStamp>(), b, a),
            2 => same_origin(&*value.cast::<DescriptorStamp>(), b, c),
            3 => direct_creation(a, b as u32, c as u32),
            4 => same_source_slot(&*value.cast::<Slot>(), &*other.cast::<Slot>()),
            5 => same_buffer(&*value.cast::<NativeBuffer>(), &*other.cast::<NativeBuffer>()),
            6 => {
                let returns = if value.is_null() || b == 0 { &[][..] } else { core::slice::from_raw_parts(value.cast::<u64>(), b as usize) };
                upload_writer_path(returns)
            }
            _ => (*value.cast::<UploadWriterContext>()).valid(),
        }
    }
}
/// UploadWriterLedger: Current (`which` 0: `stamp_out`), Enter (1: `context`, `a` handle, `b` bytes; `stamp_out`), Join
/// (2: `stamp`, `context`, `joined`), Finish (3: `stamp`, `context`; `completion`), Cancel (4: `stamp`), Invalidate (5).
///
/// # Safety
/// `ledger` is valid; the other pointers are valid where used.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_scb_ledger(
    ledger: *mut UploadWriterLedger,
    which: u32,
    stamp: *const UploadWriterStamp,
    context: *const UploadWriterContext,
    joined: *const UploadWriterJoin,
    a: u32,
    b: u32,
    stamp_out: *mut UploadWriterStamp,
    completion: *mut UploadWriterCompletion,
) -> bool {
    unsafe {
        if which == 0 {
            *stamp_out = (*ledger).current();
            return true;
        }
        let ledger = &mut *ledger;
        match which {
            1 => {
                *stamp_out = ledger.enter(&*context, a, b);
                true
            }
            2 => ledger.join(&*stamp, &*context, &*joined),
            3 => {
                let result = ledger.finish(&*stamp, &*context);
                *completion = result.unwrap_or_default();
                result.is_some()
            }
            4 => ledger.cancel(&*stamp),
            _ => {
                ledger.invalidate();
                true
            }
        }
    }
}

// ---- the creation frames (src/Render/NativeSourceCbvBinding.cpp, Frame and OriginalUploadWriterPath)

/// The image's span the creation sites lie in.
const CREATION_IMAGE: u64 = 0x2FF3000;
/// Frame: the first frame up the stack inside the image's span -- one of the three creation sites (the direct caller
/// itself when the call came from the image) -- and its registers.
pub fn creation_frame(game: u64, caller: u64) -> Option<CreationFrame> {
    // From four frames lower than the .cpp's 32.
    cvr_engine::stack::walk(32 + 4, |c, _, _| {
        if c.Rip < game || c.Rip - game >= CREATION_IMAGE {
            return None;
        }
        let rva = c.Rip - game;
        if !matches!(rva, 0x333C1A | 0x3346B1 | 0x3362A0) || (caller >= game && caller - game < CREATION_IMAGE && caller != c.Rip) {
            return Some(None);
        }
        Some(Some(CreationFrame { rva, rsp: c.Rsp, rbx: c.Rbx, rbp: c.Rbp, rdi: c.Rdi, rsi: c.Rsi, r12: c.R12, r13: c.R13, r14: c.R14, r15: c.R15 }))
    })
    .flatten()
}
/// OriginalUploadWriterPath: the first three frames up the stack inside the image's span are the upload writer's path.
pub fn original_upload_writer_path(game: u64) -> bool {
    let (mut native, mut count) = ([0u64; 3], 0usize);
    cvr_engine::stack::walk(32 + 4, |c, _, _| {
        if c.Rip < game || c.Rip - game >= CREATION_IMAGE {
            return None;
        }
        native[count] = c.Rip - game;
        count += 1;
        (count == native.len()).then(|| upload_writer_path(&native))
    })
    .unwrap_or(false)
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_creation_frame(game: u64, caller: u64, out: *mut CreationFrame) -> bool {
    match creation_frame(game, caller) {
        Some(frame) => {
            unsafe { *out = frame };
            true
        }
        None => false,
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_scb_upload_writer_path(game: u64) -> bool {
    original_upload_writer_path(game)
}

#[cfg(test)]
mod creation_frame_tests {
    use super::*;

    #[test]
    fn creation_frames_away_from_the_game() {
        // SAFETY: the test executable's own module.
        let exe = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 };
        assert!(creation_frame(exe, 0).is_none(), "the first frame in the image is no creation site");
        assert!(creation_frame(0x10000, 0).is_none() && !original_upload_writer_path(0x10000), "no frame in a span of nothing");
        assert!(!original_upload_writer_path(exe), "the test's frames are no upload writer's");
    }
}
