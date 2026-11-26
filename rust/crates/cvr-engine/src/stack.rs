//! Walking up this thread's own stack, without suspending it: the registers of each caller in turn, as the OS unwinds
//! them from the functions' unwind data. The engine keeps what a detour needs (a context, a camera, a node) in a
//! caller's nonvolatile registers; these walks read them while the frames are live and keep no pointer for later.
//!
//! The unwinder is the OS's (RtlLookupFunctionEntry, RtlVirtualUnwind), each step run under SEH in the crate's C
//! accesses (seh.rs, formerly native/seh.c): a fault ends the walk instead of the process. A frame without unwind data is a leaf whose
//! return address is the word at RSP. Memory is read with ReadProcessMemory, which reports an unreadable address
//! rather than faulting.
use core::ffi::c_void;
use windows_sys::Win32::System::Diagnostics::Debug::{
    ReadProcessMemory, RtlCaptureContext, CONTEXT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThreadStackLimits};

pub use windows_sys::Win32::System::Diagnostics::Debug::CONTEXT as Context;

extern "C" {
    fn cvr_seh_unwind_step(context: *mut CONTEXT) -> i32;
    fn cvr_seh_function_entry(rip: u64, image: *mut u64, begin: *mut u32, end: *mut u32) -> i32;
}

/// `bytes` read from `address` in this process; false when the address is 0 or any byte is unreadable.
pub fn read(address: u64, bytes: &mut [u8]) -> bool {
    let mut copied = 0usize;
    address != 0
        // SAFETY: the OS copies at most `bytes.len()` bytes into the caller's buffer and reports what it could read.
        && unsafe {
            ReadProcessMemory(GetCurrentProcess(), address as *const c_void, bytes.as_mut_ptr().cast(), bytes.len(), &mut copied)
        } != 0
        && copied == bytes.len()
}
/// A plain value read with [`read`].
pub fn read_value<T: Copy + Default>(address: u64) -> Option<T> {
    let mut value = T::default();
    // SAFETY: T is plain data; its bytes are overwritten whole or the value is dropped.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            (&mut value as *mut T).cast::<u8>(),
            core::mem::size_of::<T>(),
        )
    };
    read(address, bytes).then_some(value)
}

/// The thread's stack limits (low, high).
pub fn limits() -> (u64, u64) {
    let (mut low, mut high) = (0usize, 0usize);
    // SAFETY: two output words.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    (low as u64, high as u64)
}

/// Walks at most `depth` frames up from the caller of `walk` while RSP stays inside the stack (`low <= RSP` and
/// `RSP + 8 <= high`): `visit` sees each frame's registers and the stack limits and ends the walk with `Some`. `None`
/// when no frame answered, the walk left the stack, a leaf's return address was unreadable or the unwinder faulted.
pub fn walk<T>(depth: u32, mut visit: impl FnMut(&Context, u64, u64) -> Option<T>) -> Option<T> {
    // The OS reads and writes CONTEXT with aligned SSE stores: it must be 16-byte aligned, which windows-sys's
    // declaration (align 8) does not promise.
    #[repr(C, align(16))]
    struct Aligned(Context);
    // SAFETY: CONTEXT is plain data; RtlCaptureContext fills it with this frame's registers.
    let mut aligned: Aligned = unsafe { core::mem::zeroed() };
    let context = &mut aligned.0;
    unsafe { RtlCaptureContext(context) };
    let (low, high) = limits();
    for _ in 0..depth {
        if context.Rsp < low || context.Rsp.wrapping_add(8) > high {
            return None;
        }
        if let Some(answer) = visit(context, low, high) {
            return Some(answer);
        }
        // SAFETY: the context of a frame of this thread, stepped by the OS unwinder under SEH.
        match unsafe { cvr_seh_unwind_step(context) } {
            1 => {}
            2 => {
                context.Rip = read_value::<u64>(context.Rsp)?;
                context.Rsp = context.Rsp.wrapping_add(8);
            }
            _ => return None,
        }
    }
    None
}

/// The thread's stack as its TIB records it: (StackLimit, StackBase), the committed low end and the base.
pub fn tib_limits() -> (u64, u64) {
    let (low, high): (u64, u64);
    // SAFETY: the TEB's NT_TIB (StackBase at +0x08, StackLimit at +0x10).
    unsafe {
        core::arch::asm!("mov {}, gs:[0x10]", out(reg) low, options(nostack, readonly, preserves_flags));
        core::arch::asm!("mov {}, gs:[0x08]", out(reg) high, options(nostack, readonly, preserves_flags));
    }
    (low, high)
}
/// A frame's unwind data: its image base and its function's RVA range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Function {
    pub image: u64,
    pub begin: u32,
    pub end: u32,
}
/// Why a strict walk ended without an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrictEnd {
    /// RIP 0, or RSP outside the TIB's stack.
    LeftStack,
    /// A frame `visit` passed has no unwind data.
    NoUnwind,
    /// An unwind that did not raise RSP.
    NoProgress,
    /// The lookup or the unwind faulted.
    Fault,
    /// `depth` frames walked.
    Exhausted,
}
/// A strict walk: at most `depth` frames up from the caller of `walk_strict` while RIP is set and RSP lies inside the
/// TIB's stack (`low <= RSP < high`). `visit` sees each frame's registers, the stack limits and the frame's unwind data
/// (None without), and ends the walk with `Some`. A frame `visit` passes must have unwind data, and unwinding it must
/// raise RSP. No frame is taken as a leaf.
pub fn walk_strict<T>(depth: usize, mut visit: impl FnMut(&Context, u64, u64, Option<Function>) -> Option<T>) -> Result<T, StrictEnd> {
    #[repr(C, align(16))]
    struct Aligned(Context);
    // SAFETY: CONTEXT is plain data; RtlCaptureContext fills it with this frame's registers.
    let mut aligned: Aligned = unsafe { core::mem::zeroed() };
    let context = &mut aligned.0;
    unsafe { RtlCaptureContext(context) };
    let (low, high) = tib_limits();
    for _ in 0..depth {
        if context.Rip == 0 || context.Rsp < low || context.Rsp >= high {
            return Err(StrictEnd::LeftStack);
        }
        let (mut image, mut begin, mut end) = (0u64, 0u32, 0u32);
        // SAFETY: the OS's lookup under SEH; three outputs.
        let function = match unsafe { cvr_seh_function_entry(context.Rip, &mut image, &mut begin, &mut end) } {
            1 => Some(Function { image, begin, end }),
            2 => None,
            _ => return Err(StrictEnd::Fault),
        };
        if let Some(answer) = visit(context, low, high, function) {
            return Ok(answer);
        }
        if function.is_none() {
            return Err(StrictEnd::NoUnwind);
        }
        let before = context.Rsp;
        // SAFETY: the context of a frame of this thread, stepped by the OS unwinder under SEH.
        match unsafe { cvr_seh_unwind_step(context) } {
            1 => {}
            2 => return Err(StrictEnd::NoUnwind),
            _ => return Err(StrictEnd::Fault),
        }
        if context.Rsp <= before {
            return Err(StrictEnd::NoProgress);
        }
    }
    Err(StrictEnd::Exhausted)
}
/// An unwinding walk that reports each step: at most `depth` steps up from the caller of `unwind_frames` while RIP is
/// set, RSP is 8-aligned and `low <= RSP <= high - 8` (GetCurrentThreadStackLimits). Each step unwinds one frame -- by
/// its unwind data, or as a leaf whose return address is the word at RSP -- and must raise RSP without passing `high`;
/// `visit` then sees the caller's registers and the start of the function just unwound (0 for a leaf) and ends the walk
/// with `true`. Err when the lookup or the unwind faulted (no answer can be trusted then).
pub fn unwind_frames(depth: u32, mut visit: impl FnMut(&Context, u64) -> bool) -> Result<(), ()> {
    #[repr(C, align(16))]
    struct Aligned(Context);
    // SAFETY: CONTEXT is plain data; RtlCaptureContext fills it with this frame's registers.
    let mut aligned: Aligned = unsafe { core::mem::zeroed() };
    let context = &mut aligned.0;
    unsafe { RtlCaptureContext(context) };
    let (low, high) = limits();
    for _ in 0..depth {
        if context.Rip == 0 || context.Rsp < low || context.Rsp > high.wrapping_sub(8) || context.Rsp & 7 != 0 {
            break;
        }
        let previous = context.Rsp;
        let (mut image, mut begin, mut end) = (0u64, 0u32, 0u32);
        // SAFETY: the OS's lookup under SEH; three outputs.
        let start = match unsafe { cvr_seh_function_entry(context.Rip, &mut image, &mut begin, &mut end) } {
            1 => image + u64::from(begin),
            2 => 0,
            _ => return Err(()),
        };
        if start != 0 {
            // SAFETY: the context of a frame of this thread, stepped by the OS unwinder under SEH.
            if unsafe { cvr_seh_unwind_step(context) } != 1 {
                return Err(());
            }
        } else {
            let Some(caller) = read_value::<u64>(context.Rsp) else { break };
            context.Rip = caller;
            context.Rsp = context.Rsp.wrapping_add(8);
        }
        if context.Rsp <= previous || context.Rsp > high {
            break;
        }
        if visit(context, start) {
            break;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[inline(never)]
    fn unwound_until(target: u64) -> Option<u64> {
        let mut found = None;
        unwind_frames(32, |c, start| {
            if c.Rip >= target && c.Rip < target + 0x400 {
                found = Some(start);
                return true;
            }
            false
        })
        .ok()?;
        found
    }
    #[inline(never)]
    fn unwinding_outer() -> Option<u64> {
        let found = unwound_until(unwinding_outer as fn() -> Option<u64> as usize as u64);
        core::hint::black_box(found)
    }

    #[test]
    fn unwind_frames_reaches_a_caller() {
        let start = unwinding_outer().expect("the caller's frame");
        assert_ne!(start, 0, "the frame below it has unwind data");
    }
    #[inline(never)]
    fn strict_innermost(target: u64) -> Result<(u32, Function, u64), StrictEnd> {
        let mut seen = 0u32;
        walk_strict(16, |c, low, high, function| {
            seen += 1;
            assert!(low <= c.Rsp && c.Rsp < high);
            (c.Rip >= target && c.Rip < target + 0x400).then(|| (seen, function.expect("a Rust frame has unwind data"), c.Rip))
        })
    }
    #[inline(never)]
    fn strict_outer() -> Result<(u32, Function, u64), StrictEnd> {
        let found = strict_innermost(strict_outer as fn() -> Result<(u32, Function, u64), StrictEnd> as usize as u64);
        core::hint::black_box(found)
    }

    #[test]
    fn strict_walk_finds_a_caller_by_its_unwind_data() {
        let (depth, function, rip) = strict_outer().expect("the caller's frame");
        assert!((2..=8).contains(&depth), "found at depth {depth}");
        assert!(function.image + u64::from(function.begin) <= rip && rip < function.image + u64::from(function.end), "the frame inside its unwind data's range");
        let (low, high) = tib_limits();
        assert!(low < high);
        assert!(walk_strict(3, |_, _, _, _| None::<()>).is_err(), "no answer");
    }
    #[inline(never)]
    fn innermost(target: u64) -> Option<u32> {
        // The frames above this one: the walk's own, this function's, then outer's -- found by its code range.
        let mut seen = 0u32;
        walk(16, |c, _, _| {
            seen += 1;
            (c.Rip >= target && c.Rip < target + 0x400).then_some(seen)
        })
    }
    #[inline(never)]
    fn outer() -> Option<u32> {
        let found = innermost(outer as fn() -> Option<u32> as usize as u64);
        core::hint::black_box(found)
    }

    #[test]
    fn walks_to_a_caller() {
        let depth = outer().expect("the caller's frame");
        assert!((2..=8).contains(&depth), "found at depth {depth}");
    }

    #[test]
    fn reads_report_unreadable_memory() {
        let value = 0x1122_3344_5566_7788u64;
        assert_eq!(read_value::<u64>(&value as *const u64 as u64), Some(value));
        assert_eq!(read_value::<u64>(0), None);
        assert_eq!(read_value::<u64>(8), None);
    }

    #[test]
    fn walk_ends_without_an_answer() {
        assert_eq!(walk(64, |_, _, _| None::<()>), None);
        let (low, high) = limits();
        assert!(low < high);
    }
}

/// The frames above a captured context (an exception's), unwound as [`unwind_frames`] does: `visit` sees each caller's
/// RIP and the start of the function it unwound (0 for a leaf). The context is changed in place.
pub fn unwind_context(context: &mut Context, depth: u32, mut visit: impl FnMut(u64, u64)) {
    let (low, high) = limits();
    for _ in 0..depth {
        if context.Rip == 0 || context.Rsp < low || context.Rsp > high.wrapping_sub(8) || context.Rsp & 7 != 0 {
            break;
        }
        let previous = context.Rsp;
        let (mut image, mut begin, mut end) = (0u64, 0u32, 0u32);
        // SAFETY: the OS's lookup under SEH; three outputs.
        let start = match unsafe { cvr_seh_function_entry(context.Rip, &mut image, &mut begin, &mut end) } {
            1 => image + u64::from(begin),
            2 => 0,
            _ => break,
        };
        if start != 0 {
            // SAFETY: a context of this thread's frames, stepped by the OS unwinder under SEH.
            if unsafe { cvr_seh_unwind_step(context) } != 1 {
                break;
            }
        } else {
            let Some(caller) = read_value::<u64>(context.Rsp) else { break };
            context.Rip = caller;
            context.Rsp = context.Rsp.wrapping_add(8);
        }
        if context.Rsp <= previous || context.Rsp > high {
            break;
        }
        visit(context.Rip, start);
    }
}
