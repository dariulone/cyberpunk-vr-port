//! A GEOMETRY DRAW'S NATIVE ORIGIN (include/Render/NativeDrawOrigin.hpp): the origin of an ordinary mesh draw taken
//! from the saved registers of the guarded draw entry (its caller the ordinary flush return 0x335496; renderer in RCX,
//! draw in RDX = R14, batch in R8 = RBX, flush RDI, packet R15, instance count R12), the per-thread stack of those
//! origins across nested draws, and its resolution against a renderer and instance batch -- the flush record still
//! naming the batch, renderer and packet, the batch's draw pointer at +0x48, the packet's instanced flag and its
//! skin bit matching the stride (a skinned batch's live count at +0x1C until return 0x335512 clears it), and the 64
//! draw bytes, kind 0. The header forwards to these in the Rust build; tools/single_pass_tests/draw_origin_test runs
//! its cases against them.
use core::ffi::c_void;

pub const ORDINARY_FLUSH_RETURN: u64 = 0x335496;

/// DrawEntryFrame.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawEntryFrame {
    pub home: [u64; 6],
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub flags: u64,
    pub vectors: [u8; 96],
    pub rdi: u64,
    pub r15: u64,
    pub r12: u64,
    pub rbx: u64,
    pub r14: u64,
    pub caller: u64,
}
/// DrawOrigin.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct DrawOrigin {
    pub renderer: u64,
    pub draw: u64,
    pub batch: u64,
    pub flush: u64,
    pub packet: u64,
    pub instances: u64,
    pub valid: bool,
}
/// DrawOriginStack's members.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawOriginStack {
    pub entries: [DrawOrigin; 32],
    pub depth: u32,
}
const _: () = assert!(
    core::mem::size_of::<DrawEntryFrame>() == 0x100
        && core::mem::offset_of!(DrawEntryFrame, rdi) == 0xD0
        && core::mem::offset_of!(DrawEntryFrame, caller) == 0xF8
        && core::mem::size_of::<DrawOrigin>() == 56
        && core::mem::size_of::<DrawOriginStack>() == 56 * 32 + 8
);
/// OriginRead: (context, address, out, bytes) -> readable.
pub type OriginRead = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;

/// OriginFromEntry.
pub fn origin_from_entry(f: &DrawEntryFrame, base: u64) -> DrawOrigin {
    if base == 0
        || base > u64::MAX - ORDINARY_FLUSH_RETURN
        || f.caller != base + ORDINARY_FLUSH_RETURN
        || f.rcx == 0
        || f.rdx == 0
        || f.r8 == 0
        || f.rdx != f.r14
        || f.r8 != f.rbx
        || f.rdi == 0
        || f.r15 == 0
        || f.r12 == 0
        || f.r12 > u32::MAX as u64
    {
        return DrawOrigin::default();
    }
    DrawOrigin {
        renderer: f.rcx,
        draw: f.rdx,
        batch: f.r8,
        flush: f.rdi,
        packet: f.r15,
        instances: f.r12,
        valid: true,
    }
}

impl DrawOriginStack {
    pub fn enter(&mut self, value: DrawOrigin) {
        if (self.depth as usize) < self.entries.len() {
            self.entries[self.depth as usize] = value;
        }
        self.depth += 1;
    }
    pub fn leave(&mut self) {
        if self.depth != 0 {
            self.depth -= 1;
        }
    }
    pub fn current(&self) -> Option<&DrawOrigin> {
        (self.depth != 0 && self.depth as usize <= self.entries.len())
            .then(|| &self.entries[self.depth as usize - 1])
    }
}

fn read_into<T: Copy>(reader: OriginRead, context: *mut c_void, at: u64, out: &mut T) -> bool {
    let bytes = core::mem::size_of::<T>();
    at != 0
        && at <= u64::MAX - bytes as u64
        && unsafe { reader(context, at, out as *mut T as *mut c_void, bytes) }
}

/// ResolveOrigin into `output` (the draw's 64 bytes).
pub fn resolve_origin(
    origin: &DrawOrigin,
    renderer: u64,
    instances: u32,
    stride: u32,
    reader: Option<OriginRead>,
    context: *mut c_void,
    output: &mut [u8; 64],
) -> bool {
    if !origin.valid
        || renderer == 0
        || instances == 0
        || (stride != 48 && stride != 64)
        || origin.renderer != renderer
        || origin.instances != instances as u64
    {
        return false;
    }
    let Some(reader) = reader else { return false };
    let mut flush = [0u64; 6];
    let mut draw = 0u64;
    let mut packet = [0u64; 2];
    if !read_into(reader, context, origin.flush, &mut flush)
        || flush[0] != origin.batch
        || flush[3] != renderer
        || flush[4] != origin.packet
        || origin.batch > u64::MAX - 0x50
        || !read_into(reader, context, origin.batch + 0x48, &mut draw)
        || draw != origin.draw
        || !read_into(reader, context, origin.packet, &mut packet)
        || packet[1] & (1u64 << 51) == 0
        || (packet[1] & (1u64 << 50) != 0) != (stride == 64)
    {
        return false;
    }
    // The skin flush precedes the ordinary R12 count load; its live batch count at +0x1C stays authoritative until
    // return 0x335512 clears it.
    if stride == 64 {
        let mut count = 0u32;
        if !read_into(reader, context, origin.batch + 0x1C, &mut count) || count != instances {
            return false;
        }
    }
    read_into(reader, context, draw, output) && output[0] == 0
}

/// OriginFromEntry.
///
/// # Safety
/// `frame` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dor_origin(
    frame: *const DrawEntryFrame,
    base: u64,
    out: *mut DrawOrigin,
) {
    unsafe { *out = origin_from_entry(&*frame, base) };
}

/// DrawOriginStack: Enter (`step` 0, `value`), Leave (1) or Current (2, the entry's address or null).
///
/// # Safety
/// `stack` points at a DrawOriginStack's members; `value` is valid for Enter.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dor_stack(
    stack: *mut DrawOriginStack,
    step: u32,
    value: *const DrawOrigin,
) -> *const DrawOrigin {
    unsafe {
        let s = &mut *stack;
        match step {
            0 => {
                s.enter(*value);
                core::ptr::null()
            }
            1 => {
                s.leave();
                core::ptr::null()
            }
            _ => s
                .current()
                .map_or(core::ptr::null(), |entry| entry as *const DrawOrigin),
        }
    }
}

/// ResolveOrigin.
///
/// # Safety
/// `origin` is valid; `output` holds 64 bytes; `reader` follows OriginRead with `context`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dor_resolve(
    origin: *const DrawOrigin,
    renderer: u64,
    instances: u32,
    stride: u32,
    reader: Option<OriginRead>,
    context: *mut c_void,
    output: *mut [u8; 64],
) -> bool {
    unsafe {
        resolve_origin(
            &*origin,
            renderer,
            instances,
            stride,
            reader,
            context,
            &mut *output,
        )
    }
}
