//! The native setter tracers (formerly src/Hooks/NativeSetterMetaWrite.cpp, NativeSetterMetaConsume.cpp and
//! NativeSetterClear.cpp): three code patches that count a native setter's meta write, its consumption and its clear
//! into the worker's SetterTraceData (worker.rs), each a trampoline -- the replaced original bytes before or after a
//! push rax / push r11 / mov r11,&counter / the trace's stores / pop r11 / pop rax, then a jump back -- entered by a
//! jump written over the site (NOPs after it). Kept switched off (ENABLE_NATIVE_SETTER_TRACERS, the guard the boot always had); their
//! registrations are hook_table.rs's.

use core::ffi::c_void;

/// kEnableNativeSetterTracers.
const ENABLE_NATIVE_SETTER_TRACERS: i32 = 0;

extern "C" {
    /// cvr-ffi's AllocateTrampoline: executable memory within a rel32 jump of the target.
    fn cvr_engine_allocate_trampoline(target: *mut c_void, size: usize) -> *mut c_void;
}

/// One tracer: the site's pattern, the jump's offset in it, the bytes it replaces, the original bytes re-run before
/// and after the trace, the trace's stores and its counter's offset in SetterTraceData.
struct Tracer {
    pattern: &'static [u8],
    hook_at: usize,
    replace: usize,
    before: &'static [u8],
    body: &'static [u8],
    after: &'static [u8],
    counter: usize,
}
const META_WRITE: Tracer = Tracer {
    pattern: &[0x48, 0x8B, 0x07, 0x48, 0x89, 0x4F, 0x08, 0x48, 0x8D, 0x4D, 0xE7, 0x48, 0x89, 0x45, 0xE7],
    hook_at: 0,
    // mov rax,[rdi] / mov [rdi+08],rcx / lea rcx,[rbp-19]
    replace: 11,
    before: &[],
    body: &[
        0x48, 0x8D, 0x44, 0x24, 0x10, // lea rax,[rsp+10h]
        0x41, 0xFF, 0x03, // inc dword ptr [r11]
        0x49, 0x89, 0x7B, 0x08, // mov [r11+08],rdi
        0x49, 0x89, 0x4B, 0x10, // mov [r11+10],rcx
        0x49, 0x89, 0x43, 0x18, // mov [r11+18],rax
    ],
    after: &[
        0x48, 0x8B, 0x07, // mov rax,[rdi]
        0x48, 0x89, 0x4F, 0x08, // mov [rdi+08],rcx
        0x48, 0x8D, 0x4D, 0xE7, // lea rcx,[rbp-19]
    ],
    counter: 0,
};
const META_CONSUME: Tracer = Tracer {
    pattern: &[
        0x48, 0x8B, 0x42, 0x08, 0x48, 0x8D, 0x4C, 0x24, 0x20, 0x48, 0x83, 0x62, 0x08, 0x00, 0x48, 0x89, 0x44, 0x24, 0x28, 0x48,
        0x8B, 0x02,
    ],
    hook_at: 0,
    // mov rax,[rdx+08] / lea rcx,[rsp+20]
    replace: 9,
    before: &[
        0x48, 0x8B, 0x42, 0x08, // mov rax,[rdx+08]
        0x48, 0x8D, 0x4C, 0x24, 0x20, // lea rcx,[rsp+20]
    ],
    body: &[
        0x48, 0x8D, 0x44, 0x24, 0x10, // lea rax,[rsp+10h]
        0x41, 0xFF, 0x03, // inc dword ptr [r11]
        0x49, 0x89, 0x53, 0x08, // mov [r11+08],rdx
        0x49, 0x89, 0x43, 0x18, // mov [r11+18],rax
        0x48, 0x8B, 0x44, 0x24, 0x08, // mov rax,[rsp+8]
        0x49, 0x89, 0x43, 0x10, // mov [r11+10],rax
    ],
    after: &[],
    counter: 32,
};
const CLEAR: Tracer = Tracer {
    pattern: &[0xCC, 0x48, 0x83, 0x22, 0x00, 0x48, 0x83, 0x62, 0x08, 0x00, 0xC3, 0xCC],
    hook_at: 1,
    // and qword ptr [rdx],00 / and qword ptr [rdx+08],00
    replace: 9,
    before: &[],
    body: &[
        0x41, 0xFF, 0x03, // inc dword ptr [r11]
        0x49, 0x89, 0x53, 0x08, // mov [r11+08],rdx
        0x48, 0x8B, 0x44, 0x24, 0x10, // mov rax,[rsp+10h]
        0x49, 0x89, 0x43, 0x10, // mov [r11+10],rax
    ],
    after: &[
        0x48, 0x83, 0x22, 0x00, // and qword ptr [rdx],00
        0x48, 0x83, 0x62, 0x08, 0x00, // and qword ptr [rdx+08],00
    ],
    counter: 64,
};

/// Installs one tracer: false when its site or a trampoline is missing.
fn install(tracer: &Tracer) -> bool {
    let mask = vec![b'x'; tracer.pattern.len()];
    let Some(found) = cvr_engine::code::find_pattern("Cyberpunk2077.exe", tracer.pattern, &mask) else { return false };
    let hook_at = (found + tracer.hook_at) as *mut u8;
    // SAFETY: a site in the game image.
    let tramp = unsafe { cvr_engine_allocate_trampoline(hook_at.cast(), 256) }.cast::<u8>();
    if tramp.is_null() {
        return false;
    }
    let trace = crate::worker::setter_trace() + tracer.counter;
    let mut code: Vec<u8> = Vec::with_capacity(96);
    code.extend_from_slice(tracer.before);
    code.extend_from_slice(&[0x50, 0x41, 0x53]); // push rax / push r11
    code.extend_from_slice(&[0x49, 0xBB]); // mov r11,imm64
    code.extend_from_slice(&(trace as u64).to_le_bytes());
    code.extend_from_slice(tracer.body);
    code.extend_from_slice(&[0x41, 0x5B, 0x58]); // pop r11 / pop rax
    code.extend_from_slice(tracer.after);
    code.push(0xE9);
    // SAFETY: the trampoline holds 256 bytes; the jump back lands after the replaced bytes.
    unsafe {
        let back = hook_at.add(tracer.replace) as isize - (tramp as isize + code.len() as isize + 4);
        code.extend_from_slice(&(back as i32).to_le_bytes());
        core::ptr::copy_nonoverlapping(code.as_ptr(), tramp, code.len());
        let mut old = windows_sys::Win32::System::Memory::PAGE_PROTECTION_FLAGS::default();
        windows_sys::Win32::System::Memory::VirtualProtect(
            hook_at.cast(),
            tracer.replace,
            windows_sys::Win32::System::Memory::PAGE_EXECUTE_READWRITE,
            &mut old,
        );
        *hook_at = 0xE9;
        let rel = tramp as isize - (hook_at as isize + 5);
        core::ptr::write_unaligned(hook_at.add(1).cast::<i32>(), rel as i32);
        for i in 5..tracer.replace {
            *hook_at.add(i) = 0x90;
        }
        windows_sys::Win32::System::Memory::VirtualProtect(hook_at.cast(), tracer.replace, old, &mut old);
        windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            hook_at.cast(),
            tracer.replace,
        );
    }
    true
}
/// NativeSetterTracersWanted.
pub extern "C" fn tracers_wanted() -> bool {
    ENABLE_NATIVE_SETTER_TRACERS != 0
}
/// InstallNativeSetterMetaWriteHook.
pub extern "C" fn install_meta_write() -> bool {
    install(&META_WRITE)
}
/// InstallNativeSetterMetaConsumeHook.
pub extern "C" fn install_meta_consume() -> bool {
    install(&META_CONSUME)
}
/// InstallNativeSetterClearHook.
pub extern "C" fn install_clear() -> bool {
    install(&CLEAR)
}
