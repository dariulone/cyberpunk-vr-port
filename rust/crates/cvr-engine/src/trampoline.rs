//! Code patches by hand (formerly src/Hooks/Trampoline.cpp and the patch tails of the hook files): the trampoline
//! arena, a byte emitter for the stubs written into it, and the jump that sends a game site there.
//!
//! ONE ARENA FOR THE WHOLE PLUGIN. An x86-64 jmp rel32 reaches +/-2 GB, so stub memory has to lie inside that window
//! of the site it serves: the arena is VirtualAlloc'd by walking outward from the FIRST address anybody asks about,
//! and every later stub is carved from the same 64 KB. A second arena fragments the window -- later installers find
//! no reservable page and their hooks are silently absent -- so the C++ half's AllocateTrampoline forwards here.
use core::ffi::c_void;
use std::sync::Mutex;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE};
use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const ARENA_SIZE: usize = 65536;

struct Arena {
    base: usize,
    offset: usize,
}
static ARENA: Mutex<Arena> = Mutex::new(Arena { base: 0, offset: 0 });

fn reserve(address: usize) -> usize {
    // SAFETY: a reservation request at a hint; failure is a null return.
    unsafe { VirtualAlloc(address as *const c_void, ARENA_SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as usize }
}

/// `size` bytes of executable scratch memory within jmp rel32 range of `target`; None when the arena is exhausted
/// or could not be placed -- the caller must then not patch.
pub fn allocate(target: usize, size: usize) -> Option<usize> {
    let mut a = ARENA.lock().unwrap_or_else(|p| p.into_inner());
    if a.base == 0 {
        let mut info: SYSTEM_INFO = unsafe { core::mem::zeroed() };
        // SAFETY: fills the struct.
        unsafe { GetSystemInfo(&mut info) };
        let granularity = info.dwAllocationGranularity as usize;
        let mut min_addr = if target > 0x7FFF_FFFF { target - 0x7FFF_FFFF } else { 0 };
        let mut max_addr = target.wrapping_add(0x7FFF_FFFF);
        if max_addr < target {
            max_addr = usize::MAX;
        }
        min_addr -= min_addr % granularity;
        let mut address = target.wrapping_sub(granularity);
        while address > min_addr {
            a.base = reserve(address);
            if a.base != 0 {
                break;
            }
            address = address.wrapping_sub(granularity);
        }
        if a.base == 0 {
            let mut address = target.wrapping_add(granularity);
            while address < max_addr {
                a.base = reserve(address);
                if a.base != 0 {
                    break;
                }
                address = address.wrapping_add(granularity);
            }
        }
    }
    if a.base != 0 && a.offset + size <= ARENA_SIZE {
        let at = a.base + a.offset;
        a.offset += size;
        return Some(at);
    }
    None
}

/// A stub assembled byte by byte at a known address (rel32 operands are computed from it), then copied in.
pub struct Code {
    at: usize,
    bytes: Vec<u8>,
}

impl Code {
    pub fn new(at: usize) -> Code {
        Code { at, bytes: Vec::with_capacity(256) }
    }
    /// Where the next byte goes.
    pub fn here(&self) -> usize {
        self.at + self.bytes.len()
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Code {
        self.bytes.extend_from_slice(b);
        self
    }
    /// mov rax, imm64. The value is usually one of our own functions or globals, BAKED IN AT INSTALL TIME: such a
    /// global must never become a per-file copy or a thread_local in a refactor.
    pub fn mov_rax(&mut self, value: u64) -> &mut Code {
        self.bytes(&[0x48, 0xB8]).bytes(&value.to_le_bytes())
    }
    /// mov r11, imm64.
    pub fn mov_r11(&mut self, value: u64) -> &mut Code {
        self.bytes(&[0x49, 0xBB]).bytes(&value.to_le_bytes())
    }
    /// jmp rel32 to `target`.
    pub fn jmp(&mut self, target: usize) -> &mut Code {
        let from = self.here() + 5;
        let rel = (target as i64 - from as i64) as i32;
        self.bytes(&[0xE9]).bytes(&rel.to_le_bytes())
    }
    /// The C++ stubs' call frame around a callback: pushfq and the volatile GPRs (rax rcx rdx r8-r11) and rbp, the
    /// first `xmm` xmm registers saved below them, rbp = rsp, rsp aligned to 16 with the shadow space, `arg` (the
    /// instruction that loads rcx), mov rax / call rax, then everything restored in reverse.
    pub fn volatile_call(&mut self, xmm: usize, arg: &[u8], callback: u64) -> &mut Code {
        let frame = (xmm * 16) as u8;
        self.bytes(&[0x9C, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x55]);
        self.bytes(&[0x48, 0x83, 0xEC, frame]);
        for k in 0..xmm {
            self.xmm_slot(0x11, k);
        }
        self.bytes(&[0x48, 0x89, 0xE5, 0x48, 0x83, 0xE4, 0xF0, 0x48, 0x83, 0xEC, 0x20]);
        self.bytes(arg).mov_rax(callback).bytes(&[0xFF, 0xD0]);
        self.bytes(&[0x48, 0x89, 0xEC]);
        for k in 0..xmm {
            self.xmm_slot(0x10, k);
        }
        self.bytes(&[0x48, 0x83, 0xC4, frame]);
        self.bytes(&[0x5D, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x58, 0x9D])
    }
    /// movups [rsp+16k],xmmk (0x11) or movups xmmk,[rsp+16k] (0x10).
    fn xmm_slot(&mut self, op: u8, k: usize) {
        if k == 0 {
            self.bytes(&[0x0F, op, 0x04, 0x24]);
        } else {
            self.bytes(&[0x0F, op, 0x44 | ((k as u8) << 3), 0x24, (k * 16) as u8]);
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
    /// Copies the stub to its address (arena memory: already executable and writable).
    ///
    /// # Safety
    /// `at` must be arena memory of at least `len()` bytes that no thread executes yet.
    pub unsafe fn commit(&self) {
        // SAFETY: as the caller vouches.
        unsafe { core::ptr::copy_nonoverlapping(self.bytes.as_ptr(), self.at as *mut u8, self.bytes.len()) };
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Sends `site` to `to`: E9 rel32, then NOPs over the rest of the `replace_len` bytes the stub re-executes.
///
/// # Safety
/// `site` must be game code whose first `replace_len` (>= 5) bytes the stub at `to` reproduces, and no thread may be
/// executing inside them.
pub unsafe fn patch_jump(site: usize, replace_len: usize, to: usize) {
    let mut old = 0u32;
    // SAFETY: as the caller vouches; the protection is restored below.
    unsafe {
        VirtualProtect(site as *const c_void, replace_len, PAGE_EXECUTE_READWRITE, &mut old);
        let p = site as *mut u8;
        *p = 0xE9;
        let rel = (to as i64 - (site as i64 + 5)) as i32;
        core::ptr::copy_nonoverlapping(rel.to_le_bytes().as_ptr(), p.add(1), 4);
        for i in 5..replace_len {
            *p.add(i) = 0x90;
        }
        VirtualProtect(site as *const c_void, replace_len, old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), site as *const c_void, replace_len);
    }
}

/// Copies `bytes` to `at` and flushes the instruction cache there.
///
/// # Safety
/// `at` must be arena memory of at least `bytes.len()` bytes that no thread executes yet.
pub unsafe fn write_code(at: usize, bytes: &[u8]) {
    // SAFETY: as the caller vouches.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, bytes.len());
        FlushInstructionCache(GetCurrentProcess(), at as *const c_void, bytes.len());
    }
}

/// Writes `bytes` over the code at `site` and puts its protection back; false, and the code as it was, when the page
/// cannot be made writable or its protection cannot be restored (src/Hooks/StoryAttention.cpp's order).
///
/// # Safety
/// `site` must be game code no thread is executing inside, and `bytes` a valid replacement for it.
pub unsafe fn patch_bytes(site: usize, bytes: &[u8]) -> bool {
    let mut old = 0u32;
    // SAFETY: as the caller vouches; every path restores the protection.
    unsafe {
        if VirtualProtect(site as *const c_void, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        let mut original = vec![0u8; bytes.len()];
        core::ptr::copy_nonoverlapping(site as *const u8, original.as_mut_ptr(), bytes.len());
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), site as *mut u8, bytes.len());
        let mut ignored = 0u32;
        let restored = VirtualProtect(site as *const c_void, bytes.len(), old, &mut ignored) != 0;
        if !restored {
            core::ptr::copy_nonoverlapping(original.as_ptr(), site as *mut u8, bytes.len());
            VirtualProtect(site as *const c_void, bytes.len(), old, &mut ignored);
        }
        FlushInstructionCache(GetCurrentProcess(), site as *const c_void, bytes.len());
        restored
    }
}

/// Several sites inside one span of code (`start`, `len` bytes) patched under one change of protection, so sites
/// sharing a page cannot see each other's temporary protection; all of them or none (src/Hooks/SurveillanceBody.cpp).
///
/// # Safety
/// As `patch_bytes`, for every site; each site lies inside the span.
pub unsafe fn patch_span(start: usize, len: usize, patches: &[(usize, &[u8])]) -> bool {
    let mut old = 0u32;
    // SAFETY: as the caller vouches; every path restores the protection.
    unsafe {
        if VirtualProtect(start as *const c_void, len, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        let originals: Vec<Vec<u8>> = patches
            .iter()
            .map(|&(at, bytes)| {
                let mut o = vec![0u8; bytes.len()];
                core::ptr::copy_nonoverlapping(at as *const u8, o.as_mut_ptr(), bytes.len());
                o
            })
            .collect();
        for &(at, bytes) in patches {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, bytes.len());
        }
        let mut ignored = 0u32;
        let restored = VirtualProtect(start as *const c_void, len, old, &mut ignored) != 0;
        if !restored {
            for (&(at, _), o) in patches.iter().zip(&originals) {
                core::ptr::copy_nonoverlapping(o.as_ptr(), at as *mut u8, o.len());
            }
            VirtualProtect(start as *const c_void, len, old, &mut ignored);
        }
        for &(at, bytes) in patches {
            FlushInstructionCache(GetCurrentProcess(), at as *const c_void, bytes.len());
        }
        restored
    }
}

/// Re-targets the call at `site` (E8 rel32) to `to`; false when `to` is out of rel32 reach or the page cannot be
/// made writable, and then nothing is written.
///
/// # Safety
/// `site` must be a call instruction whose callee `to` stands in for with the same signature.
pub unsafe fn patch_call(site: usize, to: usize) -> bool {
    let delta = to as i64 - (site as i64 + 5);
    if delta < i32::MIN as i64 || delta > i32::MAX as i64 {
        return false;
    }
    let mut patch = [0xE8u8, 0, 0, 0, 0];
    patch[1..].copy_from_slice(&(delta as i32).to_le_bytes());
    let mut old = 0u32;
    // SAFETY: as the caller vouches; the protection is restored below.
    unsafe {
        if VirtualProtect(site as *const c_void, patch.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        core::ptr::copy_nonoverlapping(patch.as_ptr(), site as *mut u8, patch.len());
        FlushInstructionCache(GetCurrentProcess(), site as *const c_void, patch.len());
        let mut ignored = 0u32;
        VirtualProtect(site as *const c_void, patch.len(), old, &mut ignored);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arena_is_one_block_near_its_first_target() {
        let target = allocate as *const () as usize;
        let a = allocate(target, 256).expect("arena");
        let b = allocate(target, 256).expect("second stub");
        assert_eq!(b, a + 256, "stubs are carved from one block");
        assert!((a as i64 - target as i64).abs() < 0x7FFF_FFFF, "within rel32 of the first target");
        assert!(allocate(target, ARENA_SIZE).is_none(), "exhaustion is a refusal, not a second arena");
    }
    #[test]
    fn volatile_call_is_the_cpp_frame() {
        // OnFootMoveXY's stub frame (src/Hooks/OnFootMoveXY.cpp): six xmm registers, rcx = rsi.
        let mut c = Code::new(0);
        c.volatile_call(6, &[0x48, 0x89, 0xF1], 0x1122_3344_5566_7788);
        let mut want = vec![0x9C, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x55, 0x48, 0x83, 0xEC, 0x60];
        want.extend_from_slice(&[0x0F, 0x11, 0x04, 0x24, 0x0F, 0x11, 0x4C, 0x24, 0x10, 0x0F, 0x11, 0x54, 0x24, 0x20]);
        want.extend_from_slice(&[0x0F, 0x11, 0x5C, 0x24, 0x30, 0x0F, 0x11, 0x64, 0x24, 0x40, 0x0F, 0x11, 0x6C, 0x24, 0x50]);
        want.extend_from_slice(&[0x48, 0x89, 0xE5, 0x48, 0x83, 0xE4, 0xF0, 0x48, 0x83, 0xEC, 0x20, 0x48, 0x89, 0xF1]);
        want.extend_from_slice(&[0x48, 0xB8, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0xFF, 0xD0, 0x48, 0x89, 0xEC]);
        want.extend_from_slice(&[0x0F, 0x10, 0x04, 0x24, 0x0F, 0x10, 0x4C, 0x24, 0x10, 0x0F, 0x10, 0x54, 0x24, 0x20]);
        want.extend_from_slice(&[0x0F, 0x10, 0x5C, 0x24, 0x30, 0x0F, 0x10, 0x64, 0x24, 0x40, 0x0F, 0x10, 0x6C, 0x24, 0x50]);
        want.extend_from_slice(&[0x48, 0x83, 0xC4, 0x60, 0x5D, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x58, 0x9D]);
        assert_eq!(c.as_slice(), &want[..]);
    }
    #[test]
    fn emitter_forms() {
        let mut c = Code::new(0x1000);
        c.mov_rax(0x1122_3344_5566_7788).bytes(&[0xFF, 0xD0]).jmp(0x1000);
        assert_eq!(&c.as_slice()[..10], &[0x48, 0xB8, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
        assert_eq!(&c.as_slice()[12..], &[0xE9, 0xEF, 0xFF, 0xFF, 0xFF], "jmp back to the start: -(12+5)");
        let mut r = Code::new(0);
        r.mov_r11(1);
        assert_eq!(r.as_slice(), &[0x49, 0xBB, 1, 0, 0, 0, 0, 0, 0, 0]);
    }
}
