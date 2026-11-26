//! STREAMLINE'S TAGS (formerly src/Hooks/Ngx.cpp; that file keeps the export, the C++ face and the seams to the C++
//! frame generation and lens mask).
//!
//! Cyberpunk calls DLSS through Streamline (sl.interposer.dll), never NGX directly: slSetTag is where the game labels
//! each resource by purpose (motion vectors, depth, colour, ...). Its hook feeds the frame generation's per-eye inputs
//! and the engine stereo's per-eye DLSS (rust/crates/cvr-render/src/engine_stereo_dlss.rs), keeps the legacy single-view
//! motion/depth snapshot while diagnostics (or CyberpunkVR_NgxLegacyCapture) ask for it, and ends in the lens mask's
//! SetTag, which calls Streamline's own. MinHook relocates the export's prologue (a rel32 jump to this DLL can be out
//! of range under ASLR).
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use cvr_engine::code;
use cvr_host::log;
use windows::core::{Interface, PCSTR, PCWSTR};
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE,
};

use crate::engine_stereo_dlss::SetTagFn;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
}
#[allow(non_snake_case)]
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

/// The C++ half's state and functions the hook calls (C++ linkage there).
#[repr(C)]
pub struct Runtime {
    pub verbose_log: *const i32,
    pub legacy_capture: *const u32,
    /// cvr::framegen::RecordTags: the real per-eye frame generation inputs (never gated by diagnostics).
    pub framegen_record_tags: unsafe extern "C" fn(
        viewport: *const c_void,
        tags: *const c_void,
        count: u32,
        list: *mut c_void,
    ),
    /// cvr::framegen::InstallInputHooks.
    pub framegen_install_input_hooks: extern "C" fn() -> bool,
    /// cvr::optics::mask::SetTag with the recording thread's view side: the scope image's history mask rides with
    /// the velocity tag, then Streamline's own slSetTag.
    pub mask_set_tag: unsafe extern "C" fn(
        viewport: *const c_void,
        tags: *const c_void,
        count: u32,
        list: *mut c_void,
        original: SetTagFn,
    ) -> u32,
}
/// The plugin's table is built in Rust (stereo_tables.rs).
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const crate::stereo_tables::NGX).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
#[no_mangle]
pub extern "C" fn cvr_ngx_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}
fn verbose() -> bool {
    // SAFETY: the C++ half's global.
    runtime().is_some_and(|rt| unsafe { core::ptr::read_volatile(rt.verbose_log) } != 0)
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}

static ORIGINAL_SET_TAG: AtomicUsize = AtomicUsize::new(0);
static SET_TAG_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static SET_TAG_CALLS: AtomicU64 = AtomicU64::new(0);
static SET_TAG_INVALID: AtomicU64 = AtomicU64::new(0);

fn original_set_tag() -> Option<SetTagFn> {
    let p = ORIGINAL_SET_TAG.load(Ordering::Acquire);
    // SAFETY: MinHook's trampoline of slSetTag.
    (p != 0).then(|| unsafe { core::mem::transmute::<usize, SetTagFn>(p) })
}

// ---- the legacy diagnostic snapshot

struct Capture {
    mv: Option<ID3D12Resource>,
    depth: Option<ID3D12Resource>,
}
// SAFETY: COM objects with their own references, only touched under the mutex.
unsafe impl Send for Capture {}
static CAPTURE: Mutex<Capture> = Mutex::new(Capture {
    mv: None,
    depth: None,
});
static HAVE_CAPTURED: AtomicBool = AtomicBool::new(false);
static MV_WIDTH: AtomicU32 = AtomicU32::new(0);
static MV_HEIGHT: AtomicU32 = AtomicU32::new(0);
static MV_FORMAT: AtomicU32 = AtomicU32::new(0);
const BUFFER_DEPTH: u32 = 0;
const BUFFER_MOTION_VECTORS: u32 = 1;

fn set_captured(c: &mut Capture, motion: bool, resource: Option<ID3D12Resource>) {
    let slot = if motion { &mut c.mv } else { &mut c.depth };
    if slot.as_ref().map(Interface::as_raw) == resource.as_ref().map(Interface::as_raw) {
        return;
    }
    let old = core::mem::replace(slot, resource);
    HAVE_CAPTURED.store(c.mv.is_some() || c.depth.is_some(), Ordering::Release);
    drop(old);
}
fn release_diagnostic_capture() {
    if !HAVE_CAPTURED.load(Ordering::Acquire) {
        return;
    }
    let mut c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    set_captured(&mut c, true, None);
    set_captured(&mut c, false, None);
    MV_WIDTH.store(0, Ordering::Relaxed);
    MV_HEIGHT.store(0, Ordering::Relaxed);
    MV_FORMAT.store(0, Ordering::Relaxed);
}
/// A committed, readable object whose first word points into a loaded image: a COM object's vtable.
fn is_likely_com_object(p: usize) -> bool {
    if p == 0 || p & 7 != 0 {
        return false;
    }
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    // SAFETY: a plain query of the address space.
    if unsafe {
        VirtualQuery(
            p as *const c_void,
            &mut mbi,
            core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } == 0
    {
        return false;
    }
    const READABLE: u32 = 0x02 | 0x04 | 0x08 | 0x20 | 0x40; // READONLY, READWRITE, WRITECOPY, EXECUTE_READ, EXECUTE_READWRITE
    if mbi.State != MEM_COMMIT || mbi.Protect & READABLE == 0 {
        return false;
    }
    // SAFETY: committed and readable, checked above.
    let vtable = unsafe { *(p as *const usize) };
    if vtable == 0 {
        return false;
    }
    let mut vmbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    // SAFETY: as above.
    if unsafe {
        VirtualQuery(
            vtable as *const c_void,
            &mut vmbi,
            core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } == 0
    {
        return false;
    }
    vmbi.Type == MEM_IMAGE
}
/// A tag as the live layout has it: the sl::Resource at +0x20 (its native resource at +0x28), the buffer type at
/// +0x28.
fn process_tag(tag: usize) {
    let invalid = || {
        if diagnostics() {
            SET_TAG_INVALID.fetch_add(1, Ordering::Relaxed);
        }
    };
    let (Some(resource), Some(ty)) = (
        code::read::<usize>(tag + 0x20),
        code::read::<u32>(tag + 0x28),
    ) else {
        invalid();
        return;
    };
    if resource == 0 {
        return;
    }
    let Some(native) = code::read::<usize>(resource + 0x28) else {
        invalid();
        return;
    };
    if !is_likely_com_object(native) {
        let n = SET_TAG_CALLS.load(Ordering::Relaxed);
        if verbose() && n < 8 {
            log!(
                "[NGX] setTag #{} type={} resourceStruct={:p} native={:p} NOT_COM\n",
                n,
                ty,
                resource as *const c_void,
                native as *const c_void
            );
        }
        invalid();
        return;
    }
    let raw = native as *mut c_void;
    // SAFETY: a COM object by the checks above; QueryInterface confirms it.
    let Some(d3d) = (unsafe { windows::core::IUnknown::from_raw_borrowed(&raw) })
        .and_then(|u| u.cast::<ID3D12Resource>().ok())
    else {
        invalid();
        return;
    };
    // SAFETY: a live resource.
    let desc = unsafe { d3d.GetDesc() };
    if ty == BUFFER_MOTION_VECTORS {
        let mut c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
        set_captured(&mut c, true, Some(d3d.clone()));
        MV_WIDTH.store(desc.Width as u32, Ordering::Relaxed);
        MV_HEIGHT.store(desc.Height, Ordering::Relaxed);
        MV_FORMAT.store(desc.Format.0 as u32, Ordering::Relaxed);
    } else if ty == BUFFER_DEPTH {
        let mut c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
        set_captured(&mut c, false, Some(d3d.clone()));
    }
    let n = SET_TAG_CALLS.load(Ordering::Relaxed);
    if verbose() && (n < 12 || n % 1800 == 0) {
        log!(
            "[NGX] setTag #{} bufType={} native={:p} (D3D12 {}x{} fmt={} dim={})\n",
            n,
            ty,
            raw,
            desc.Width,
            desc.Height,
            desc.Format.0 as u32,
            desc.Dimension.0 as u32
        );
    }
}

/// slSetTag's hook: the frame generation's tags, the engine stereo's per-eye DLSS, the legacy snapshot, then the lens
/// mask and Streamline's own.
unsafe extern "C" fn hooked_sl_set_tag(
    viewport: *const c_void,
    tags: *const c_void,
    count: u32,
    list: *mut c_void,
) -> u32 {
    let Some(rt) = runtime() else { return 1 };
    // SAFETY: the C++ half's frame generation, with Streamline's own arguments.
    unsafe { (rt.framegen_record_tags)(viewport, tags, count, list) };
    // SAFETY: as above.
    unsafe {
        crate::engine_stereo_dlss::cvr_engine_stereo_dlss_record_tags(
            viewport,
            tags,
            count,
            original_set_tag(),
        )
    };
    let diag = diagnostics();
    let n = if diag {
        SET_TAG_CALLS.fetch_add(1, Ordering::Relaxed)
    } else {
        0
    };
    if diag && verbose() && (n < 3 || n % 1800 == 0) {
        log!(
            "[NGX] slSetTag invoked #{} vp={:p} tags={:p} numTags={} cmdBuf={:p}\n",
            n,
            viewport,
            tags,
            count,
            list
        );
    }
    // SAFETY: the C++ half's export.
    if diag || unsafe { core::ptr::read_volatile(rt.legacy_capture) } != 0 {
        if !tags.is_null() && count > 0 && count < 64 {
            for i in 0..count as usize {
                process_tag(tags as usize + i * 64);
            }
        }
    } else {
        // Disabling diagnostics also releases the last retained textures.
        release_diagnostic_capture();
    }
    match original_set_tag() {
        // SAFETY: the lens mask's SetTag, which calls Streamline's own.
        Some(original) => unsafe { (rt.mask_set_tag)(viewport, tags, count, list, original) },
        None => 1, // SL_RESULT_FAIL
    }
}

/// NgxInstallEvaluateFeatureHook: the frame generation's input hooks, then slSetTag's (once sl.interposer.dll is in).
#[no_mangle]
pub extern "C" fn cvr_ngx_install() -> bool {
    let Some(rt) = runtime() else { return false };
    (rt.framegen_install_input_hooks)();
    if SET_TAG_HOOK_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    // SAFETY: plain Win32 calls with NUL-terminated names.
    let Ok(module) =
        (unsafe { GetModuleHandleW(PCWSTR(windows::core::w!("sl.interposer.dll").as_ptr())) })
    else {
        return false;
    };
    let target =
        unsafe { GetProcAddress(module, PCSTR(b"slSetTag\0".as_ptr())) }.map_or(0, |f| f as usize);
    if target != 0 {
        let mut original: *mut c_void = core::ptr::null_mut();
        // SAFETY: MinHook on Streamline's export.
        let created = unsafe {
            MH_CreateHook(
                target as *mut c_void,
                hooked_sl_set_tag as *const () as *mut c_void,
                &mut original,
            )
        };
        if created == 0 {
            ORIGINAL_SET_TAG.store(original as usize, Ordering::Release);
        }
        // SAFETY: as above.
        if created == 0 && unsafe { MH_EnableHook(target as *mut c_void) } == 0 {
            SET_TAG_HOOK_INSTALLED.store(true, Ordering::Release);
            log!("[framegen] Streamline tag hook installed with relocated instructions\n");
        } else if created == 0 {
            // SAFETY: as above.
            unsafe { MH_RemoveHook(target as *mut c_void) };
            ORIGINAL_SET_TAG.store(0, Ordering::Release);
        }
    }
    SET_TAG_HOOK_INSTALLED.load(Ordering::Acquire)
}

/// NgxAcquireMotionVectors / NgxAcquireDepth: the snapshot with a reference of the caller's, or null.
#[no_mangle]
pub extern "C" fn cvr_ngx_acquire(depth: bool) -> *mut c_void {
    let c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    let slot = if depth { &c.depth } else { &c.mv };
    slot.clone().map_or(core::ptr::null_mut(), |r| r.into_raw())
}
/// NgxGetMvWidth / Height / Format.
#[no_mangle]
pub extern "C" fn cvr_ngx_mv(which: u32) -> u32 {
    match which {
        0 => MV_WIDTH.load(Ordering::Relaxed),
        1 => MV_HEIGHT.load(Ordering::Relaxed),
        _ => MV_FORMAT.load(Ordering::Relaxed),
    }
}
