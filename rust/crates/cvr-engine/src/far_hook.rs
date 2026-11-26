//! A FAR INLINE HOOK (formerly src/Hooks/FarInlineHook.cpp, main's 0.2.4): for a function with no free memory within
//! reach of a 5-byte jump, which MinHook needs for its relay (MH_ERROR_MEMORY_ALLOC -- seen with nvngx_dlss.dll mapped
//! among the game's reservations, nothing free within 2 GB). The target's first instructions are replaced by a 14-byte
//! absolute jump (FF 25) and moved into a trampoline that can live anywhere. Only for a prologue whose first 14 bytes
//! hold no relative branch, no RIP-relative operand and no return or indirect jump (Unsupported otherwise). The other
//! threads are suspended while the jump is written; Busy when one of them is inside the bytes being replaced (try
//! again later). Never removed.
//!
//! The instruction lengths come from MinHook's own decoder (hde64, compiled into this crate by build.rs).
use core::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    FlushInstructionCache, GetThreadContext, CONTEXT, CONTEXT_CONTROL_AMD64,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, VirtualProtect, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
    THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
};

/// FF 25 00000000 <address64>.
const JUMP: usize = 14;
const MAX_THREADS: usize = 8192;

/// FarInlineHook's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarHookResult {
    Ok,
    Unsupported,
    Busy,
    Failed,
}
impl FarHookResult {
    /// ToString's words.
    pub fn text(self) -> &'static str {
        match self {
            FarHookResult::Ok => "ok",
            FarHookResult::Unsupported => "prologue not movable",
            FarHookResult::Busy => "a thread is inside the prologue",
            FarHookResult::Failed => "allocation or protection failed",
        }
    }
}

/// hde64s (externals/minhook/src/hde/hde64.h), packed.
#[repr(C, packed)]
#[derive(Default)]
struct Hde64 {
    len: u8,
    p_rep: u8,
    p_lock: u8,
    p_seg: u8,
    p_66: u8,
    p_67: u8,
    rex: u8,
    rex_w: u8,
    rex_r: u8,
    rex_x: u8,
    rex_b: u8,
    opcode: u8,
    opcode2: u8,
    modrm: u8,
    modrm_mod: u8,
    modrm_reg: u8,
    modrm_rm: u8,
    sib: u8,
    sib_scale: u8,
    sib_index: u8,
    sib_base: u8,
    imm: u64,
    disp: u32,
    flags: u32,
}
const _: () = assert!(core::mem::size_of::<Hde64>() == 37);
const F_MODRM: u32 = 0x0000_0001;
const F_RELATIVE: u32 = 0x0000_0200;
const F_ERROR: u32 = 0x0000_1000;

extern "C" {
    fn hde64_disasm(code: *const c_void, hs: *mut Hde64) -> u32;
}

fn write_jump(at: &mut [u8], destination: usize) {
    at[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    at[6..14].copy_from_slice(&(destination as u64).to_le_bytes());
}

/// Whole instructions covering at least JUMP bytes that run the same from any address; their length.
///
/// # Safety
/// `code` is readable code of at least the instructions decoded.
unsafe fn movable(code: *const u8) -> Option<usize> {
    let mut length = 0usize;
    while length < JUMP {
        let mut hs = Hde64::default();
        // SAFETY: as the caller vouches.
        let size = unsafe { hde64_disasm(code.add(length).cast(), &mut hs) } as usize;
        let flags = hs.flags;
        if size == 0 || flags & F_ERROR != 0 || flags & F_RELATIVE != 0 {
            return None;
        }
        // [rip+disp32]
        if flags & F_MODRM != 0 && hs.modrm_mod == 0 && hs.modrm_rm == 5 {
            return None;
        }
        let op = hs.opcode;
        if matches!(op, 0xC3 | 0xC2 | 0xCB | 0xCA | 0xCC | 0xCD) {
            return None;
        }
        // call / jmp r/m
        if op == 0xFF && flags & F_MODRM != 0 && (2..=5).contains(&hs.modrm_reg) {
            return None;
        }
        length += size;
    }
    Some(length)
}

fn thaw(threads: &mut Vec<HANDLE>) {
    for &thread in threads.iter() {
        // SAFETY: our handles of suspended threads.
        unsafe {
            ResumeThread(thread);
            CloseHandle(thread);
        }
    }
    threads.clear();
}

/// Suspends every other thread of the process. No heap use while threads are held: the list's storage is reserved
/// before the first suspension. False when a thread sits in [from, to) or the list would outgrow its storage.
fn freeze(threads: &mut Vec<HANDLE>, from: usize, to: usize) -> bool {
    threads.reserve(MAX_THREADS);
    // SAFETY: plain Win32 calls on this process's threads; every handle opened is closed (thaw) or kept in the list.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let process = GetCurrentProcessId();
        let me = GetCurrentThreadId();
        let mut entry: THREADENTRY32 = core::mem::zeroed();
        entry.dwSize = core::mem::size_of::<THREADENTRY32>() as u32;
        let mut clear = true;
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more && clear {
            if entry.th32OwnerProcessID == process && entry.th32ThreadID != me {
                let thread = OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                    0,
                    entry.th32ThreadID,
                );
                if !thread.is_null() {
                    if threads.len() >= MAX_THREADS || SuspendThread(thread) == u32::MAX {
                        CloseHandle(thread);
                        clear = threads.len() < MAX_THREADS;
                    } else {
                        threads.push(thread);
                        let mut context: CONTEXT = core::mem::zeroed();
                        context.ContextFlags = CONTEXT_CONTROL_AMD64;
                        if GetThreadContext(thread, &mut context) != 0
                            && (context.Rip as usize) >= from
                            && (context.Rip as usize) < to
                        {
                            clear = false;
                        }
                    }
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        if !clear {
            thaw(threads);
        }
        clear
    }
}

/// FarInlineHook: `original` receives the trampoline that runs the moved instructions and jumps back.
///
/// # Safety
/// `target` is the entry of a function in loaded code; `detour` has its signature; `original` is writable.
pub unsafe fn far_inline_hook(target: *mut c_void, detour: *const c_void, original: *mut *mut c_void) -> FarHookResult {
    if target.is_null() || detour.is_null() || original.is_null() {
        return FarHookResult::Failed;
    }
    let code = target.cast::<u8>();
    // SAFETY (whole body): as the caller vouches; the trampoline and the patch are our own pages and bytes.
    unsafe {
        let Some(length) = movable(code) else {
            return FarHookResult::Unsupported;
        };
        // The moved instructions, then the way back to the rest of the function.
        let trampoline =
            VirtualAlloc(core::ptr::null(), length + JUMP, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE).cast::<u8>();
        if trampoline.is_null() {
            return FarHookResult::Failed;
        }
        core::ptr::copy_nonoverlapping(code, trampoline, length);
        write_jump(core::slice::from_raw_parts_mut(trampoline.add(length), JUMP), code as usize + length);
        let mut previous = 0u32;
        VirtualProtect(trampoline.cast(), length + JUMP, PAGE_EXECUTE_READ, &mut previous);
        FlushInstructionCache(GetCurrentProcess(), trampoline.cast(), length + JUMP);

        let mut protection = 0u32;
        if VirtualProtect(target, length, PAGE_EXECUTE_READWRITE, &mut protection) == 0 {
            VirtualFree(trampoline.cast(), 0, MEM_RELEASE);
            return FarHookResult::Failed;
        }
        let mut patch = [0xCCu8; 32];
        write_jump(&mut patch, detour as usize);
        let mut threads: Vec<HANDLE> = Vec::new();
        if !freeze(&mut threads, code as usize, code as usize + length) {
            let mut ignored = 0u32;
            VirtualProtect(target, length, protection, &mut ignored);
            VirtualFree(trampoline.cast(), 0, MEM_RELEASE);
            return FarHookResult::Busy;
        }
        *original = trampoline.cast();
        core::ptr::copy_nonoverlapping(patch.as_ptr(), code, length);
        FlushInstructionCache(GetCurrentProcess(), target, length);
        thaw(&mut threads);
        let mut ignored = 0u32;
        VirtualProtect(target, length, protection, &mut ignored);
        FarHookResult::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_prologue_is_moved_and_called_through() {
        // mov rax, 0x1111 ; mov rcx, rcx (x3 padding) ; ret -- 10 + 3*3 + 1 bytes, no relative operand
        let mut body = vec![0x48u8, 0xB8, 0x11, 0x11, 0, 0, 0, 0, 0, 0];
        for _ in 0..3 {
            body.extend_from_slice(&[0x48, 0x89, 0xC9]);
        }
        body.push(0xC3);
        // SAFETY: a page of our own made executable, a function written into it.
        unsafe {
            let page = VirtualAlloc(core::ptr::null(), 4096, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE).cast::<u8>();
            assert!(!page.is_null());
            core::ptr::copy_nonoverlapping(body.as_ptr(), page, body.len());
            extern "C" fn detour() -> u64 {
                0x2222
            }
            let mut original: *mut c_void = core::ptr::null_mut();
            let r = far_inline_hook(page.cast(), detour as *const c_void, &mut original);
            assert_eq!(r, FarHookResult::Ok);
            let hooked: extern "C" fn() -> u64 = core::mem::transmute(page);
            let through: extern "C" fn() -> u64 = core::mem::transmute(original);
            assert_eq!(hooked(), 0x2222);
            assert_eq!(through(), 0x1111);
        }
    }

    #[test]
    fn a_relative_prologue_is_refused() {
        // jmp rel32 at the entry
        let body = [0xE9u8, 0, 0, 0, 0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xC3];
        // SAFETY: a readable buffer of whole instructions.
        assert!(unsafe { movable(body.as_ptr()) }.is_none());
        // lea rax, [rip+0]
        let rip = [0x48u8, 0x8D, 0x05, 0, 0, 0, 0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xC3];
        // SAFETY: as above.
        assert!(unsafe { movable(rip.as_ptr()) }.is_none());
    }
}
