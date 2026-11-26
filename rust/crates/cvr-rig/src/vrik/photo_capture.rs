//! PhotoModeCapture (formerly src/Hooks/PhotoModeCapture.cpp; the policy's C++ header stays for
//! tools/photo_capture_tests): a Photo Mode capture is taken at the VR window's size rather than the engine's, and its
//! readback is not clamped to 3840x2160 when it is exactly that size. The capture-size query (2.5 sub_1420B5AE0) is
//! detoured to src/Hooks/PhotoCaptureClampThunk.asm's register-neutral CvrPhotoSizeDetour (the native query first, then
//! `CvrPhotoSizePolicy`), and only the readback's call of the clamp (exe+0x20B5BDD) goes through CvrPhotoClampDetour
//! (`CvrPhotoClampSkip`, else the native clamp): the clamp's own entry stays untouched for the world widgets that call
//! it with live R8/R9. The exports and the registration (Boot, 86) stay in src/Hooks/PhotoModeCapture.cpp.
//!
//! Also PatchBuffer (formerly src/Hooks/PatchBuffer.cpp): an observer of the engine's float buffer copy, registered but
//! off (its tracer constant is 0).
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};
use cvr_engine::code::{find_pattern, image_base, read, read_bytes};
use cvr_engine::hooks::{install_chain, Detour};
use cvr_engine::trampoline::{allocate, patch_jump, Code};

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_PhotoCaptureSizeOverrides: AtomicU64;
    static CyberpunkVR_PhotoCaptureClampOverrides: AtomicU64;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    fn GetForcedWindowWidth() -> u32;
    fn GetForcedWindowHeight() -> u32;
    fn CvrPhotoClampDetour();
    fn CvrPhotoSizeDetour();
}
#[link(name = "kernel32")]
extern "system" {
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

/// The thunks' targets (their EXTERNs).
#[no_mangle]
pub static mut CvrPhotoClampOriginal: usize = 0;
#[no_mangle]
pub static mut CvrPhotoSizeOriginal: *mut c_void = core::ptr::null_mut();

/// Native request types written by RVA 1D87940: Photo Mode 1, the savegame screenshot 3 (the +1C0 active byte shared).
const PHOTO_MODE_REQUEST: u32 = 1;
/// The readback's call of the clamp returns here.
const READBACK_CLAMP_RETURN: usize = 0x20B5BE2;
const CLAMP_RVA: usize = 0x27FEB44;

fn diagnostics() -> bool {
    // SAFETY: a plain read of the C++ half's flag.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
/// The renderer's current capture's type (0 when there is none).
fn current_capture_type() -> u32 {
    let base = image_base();
    let renderer = read::<usize>(base + 0x3BBFB38).unwrap_or(0);
    if renderer == 0 {
        return 0;
    }
    let capture = read::<usize>(renderer + 0x320).unwrap_or(0);
    if capture == 0 {
        return 0;
    }
    read::<u32>(capture + 0x1C4).unwrap_or(0)
}

/// From CvrPhotoClampDetour with the clamp's width and height: true skips the clamp (a Photo Mode readback of exactly
/// the VR window's size).
#[no_mangle]
pub unsafe extern "C" fn CvrPhotoClampSkip(width: *mut u32, height: *mut u32) -> bool {
    let (vw, vh) = (GetForcedWindowWidth(), GetForcedWindowHeight());
    let (Some(w), Some(h)) = (width.as_ref(), height.as_ref()) else {
        return false;
    };
    let preserve = current_capture_type() == PHOTO_MODE_REQUEST && vw != 0 && vh != 0 && *w == vw && *h == vh;
    if preserve && diagnostics() {
        CyberpunkVR_PhotoCaptureClampOverrides.fetch_add(1, Ordering::Relaxed);
    }
    preserve
}
/// From CvrPhotoSizeDetour after the native query: an active Photo Mode request's size becomes the VR window's.
#[no_mangle]
pub unsafe extern "C" fn CvrPhotoSizePolicy(capture: usize, size: *mut [u32; 2], requested: bool) {
    if !requested || size.is_null() {
        return;
    }
    let (Some(active), Some(ty)) = (read::<u8>(capture + 0x1C0), read::<u32>(capture + 0x1C4)) else {
        return;
    };
    if active == 0 || ty != PHOTO_MODE_REQUEST {
        return;
    }
    let (w, h) = (GetForcedWindowWidth(), GetForcedWindowHeight());
    if w != 0 && h != 0 && ((*size)[0] != w || (*size)[1] != h) {
        *size = [w, h];
        if diagnostics() {
            CyberpunkVR_PhotoCaptureSizeOverrides.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The size query detoured, the readback's clamp call pointed at a relay to the clamp thunk.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_photo_mode_capture_install() -> bool {
    let pattern = b"\x48\x83\xEC\x28\x80\xB9\xC0\x01\x00\x00\x00\x4C\x8B\xCA\x74\x2B\x8B\x89\x10";
    let Some(target) = find_pattern("Cyberpunk2077.exe", pattern, &[b'x'; 19]) else {
        return false;
    };
    let base = image_base();
    let clamp = base + CLAMP_RVA;
    let call = base + READBACK_CLAMP_RETURN - 5;
    let mut clamp_bytes = [0u8; 16];
    let mut call_bytes = [0u8; 5];
    if !read_bytes(clamp, &mut clamp_bytes) || !read_bytes(call, &mut call_bytes) {
        return false;
    }
    let displacement = i32::from_le_bytes([call_bytes[1], call_bytes[2], call_bytes[3], call_bytes[4]]);
    if &clamp_bytes != b"\x81\x39\x00\x0F\x00\x00\x77\x08\x81\x3A\x70\x08\x00\x00\x76\x50"
        || call_bytes[0] != 0xE8
        || (base + READBACK_CLAMP_RETURN) as i64 + displacement as i64 != clamp as i64
    {
        return false;
    }
    let Some(relay) = allocate(call, 14) else { return false };
    let relative = relay as i64 - (call as i64 + 5);
    if relative < i32::MIN as i64 || relative > i32::MAX as i64 {
        return false;
    }
    let mut jump = [0u8; 14];
    jump[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    jump[6..].copy_from_slice(&(CvrPhotoClampDetour as *const () as u64).to_le_bytes());
    core::ptr::copy_nonoverlapping(jump.as_ptr(), relay as *mut u8, jump.len());
    FlushInstructionCache(GetCurrentProcess(), relay as *const c_void, 14);
    CvrPhotoClampOriginal = clamp;
    let detours = [Detour {
        name: "capture size query sub_1420B5AE0",
        target,
        absolute: true,
        detour: CvrPhotoSizeDetour as *const c_void,
        original: &raw mut CvrPhotoSizeOriginal,
        expect: None,
    }];
    if install_chain("photo-mode-capture", &detours).is_err() {
        return false;
    }
    let mut after = [0xE8u8, 0, 0, 0, 0];
    after[1..].copy_from_slice(&(relative as i32).to_le_bytes());
    let mut protection = 0u32;
    if VirtualProtect(call as *mut c_void, 5, 0x40, &mut protection) == 0 {
        cvr_engine::hooks::uninstall_chain("photo-mode-capture");
        return false;
    }
    core::ptr::copy_nonoverlapping(after.as_ptr(), call as *mut u8, 5);
    let mut ignored = 0u32;
    if VirtualProtect(call as *mut c_void, 5, protection, &mut ignored) == 0 {
        core::ptr::copy_nonoverlapping(call_bytes.as_ptr(), call as *mut u8, 5);
        VirtualProtect(call as *mut c_void, 5, protection, &mut ignored);
        FlushInstructionCache(GetCurrentProcess(), call as *const c_void, 5);
        cvr_engine::hooks::uninstall_chain("photo-mode-capture");
        return false;
    }
    FlushInstructionCache(GetCurrentProcess(), call as *const c_void, 5);
    super::env::log("[photo-capture] readback-only clamp call installed; native UI clamp untouched\n");
    true
}

// ---- PatchBuffer ----

static PATCH_BUFFER_HITS: AtomicU64 = AtomicU64::new(0);
/// rcx = dest, rdx = src, r8 = count.
#[no_mangle]
pub extern "C" fn OnPatchBufferCallback(_dest: *mut f32, _src: *mut f32, _count: usize) {
    PATCH_BUFFER_HITS.fetch_add(1, Ordering::Relaxed);
}
/// Off: the tracer constant is 0 (the registry reports it skipped).
#[no_mangle]
pub extern "C" fn cvr_hook_patch_buffer_wanted() -> bool {
    false
}
/// mov rax,rcx / lea r10,[rip+x] / cmp r8,0Fh: the callback, then the three with the LEA made absolute.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_patch_buffer_install() -> bool {
    let pattern = b"\x48\x8B\xC1\x4C\x8D\x15\x00\x00\x00\x00\x49\x83\xF8\x0F";
    let Some(found) = find_pattern("Cyberpunk2077.exe", pattern, b"xxxxxx????xxxx") else {
        return false;
    };
    const REPLACE: usize = 14;
    let Some(tramp) = allocate(found, 512) else { return false };
    let lea = read::<i32>(found + 6).unwrap_or(0);
    let target = (found as i64 + 10 + lea as i64) as u64;
    let mut c = Code::new(tramp);
    c.volatile_call(4, &[], OnPatchBufferCallback as *const () as u64)
        .bytes(&[0x48, 0x8B, 0xC1]) // mov rax, rcx
        .bytes(&[0x49, 0xBA]) // mov r10, imm64
        .bytes(&target.to_le_bytes())
        .bytes(&[0x49, 0x83, 0xF8, 0x0F]) // cmp r8, 0Fh
        .jmp(found + REPLACE);
    c.commit();
    patch_jump(found, REPLACE, tramp);
    true
}
