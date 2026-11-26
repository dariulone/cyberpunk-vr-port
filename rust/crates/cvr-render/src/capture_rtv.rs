//! WHICH RENDER TARGET A DESCRIPTOR NAMES (the RTV half of formerly src/Stereo/Capture.cpp): every RTV the engine
//! creates kept as metadata by its descriptor handle (descriptor_cache: resource and size, the swapchain pinned, the
//! capacity raised to the RTV heaps' slots), the second view's output candidates kept by handle (an RGBA8-family or
//! packed-HDR 2D target of exactly the selected VRCAM size), and the port's own resources excluded from that
//! detection.
//!
//! Capture.cpp keeps the exports and forwards its C++ names here.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use cvr_host::fast::FastMap;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_engine::code::read;
use std::sync::{LazyLock, Mutex};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;

use crate::descriptor_cache::{
    cvr_rtv_cache_limit, cvr_rtv_cache_pin, cvr_rtv_cache_read, cvr_rtv_cache_reserve,
    cvr_rtv_cache_size, cvr_rtv_cache_store, DescriptorTarget,
};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_DebugMirrorRtvRegs: AtomicU64;
    static CyberpunkVR_DebugRttComp: AtomicU64;
    static g_vrcam_sel_w: AtomicU32;
    static g_vrcam_sel_h: AtomicU32;
    static g_vrcam_component: [c_char; 96];
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
#[repr(C)]
pub struct Runtime {
    /// mirror_get_resource_desc (SEH-guarded GetDesc).
    get_desc: extern "C" fn(*mut c_void, *mut D3D12_RESOURCE_DESC) -> bool,
    /// g_rtv_dim_count, g_rtv_dim_wrapped_logged (the HUD's composite report reads them).
    rtv_dim_count: *const AtomicU32,
    rtv_dim_wrapped_logged: *const AtomicBool,
    /// g_mirror_rtv_candidate_count (the mirror's starvation line reads it).
    candidate_count: *const AtomicU32,
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    get_desc: s::get_desc,
    rtv_dim_count: &raw const m::g_rtv_dim_count,
    rtv_dim_wrapped_logged: &raw const m::g_rtv_dim_wrapped_logged,
    candidate_count: &raw const s::MIRROR_RTV_CANDIDATE_COUNT,
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn get_desc(res: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
    let mut d = D3D12_RESOURCE_DESC::default();
    (!res.is_null() && (rt().get_desc)(res, &mut d)).then_some(d)
}

// ---- resources owned by US, not by the engine

static FOREIGN: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
/// Anything the port creates of the VRCAM shape (the OpenXR submit's capture textures) registers here and is skipped
/// by the detection, or the mirror samples our half-written capture.
#[no_mangle]
pub extern "C" fn cvr_capture_register_foreign_resource(r: *mut c_void) {
    if r.is_null() {
        return;
    }
    let r = r as usize;
    for slot in &FOREIGN {
        let current = slot.load(Acquire);
        if current == r {
            return;
        }
        if current != 0 {
            continue;
        }
        if slot.compare_exchange(0, r, AcqRel, Acquire).is_ok() {
            stereo_log(&format!(
                "[mirror] foreign resource registered {r:016X} (excluded from VRCAM detection)"
            ));
            return;
        }
    }
    stereo_log(&format!(
        "[mirror] WARNING foreign-resource table full, {r:016X} NOT excluded"
    ));
}
#[no_mangle]
pub extern "C" fn cvr_capture_is_foreign(r: *mut c_void) -> bool {
    if r.is_null() {
        return false;
    }
    for slot in &FOREIGN {
        let current = slot.load(Acquire);
        if current == 0 {
            // Filled in order: the first empty slot ends it.
            break;
        }
        if current == r as usize {
            return true;
        }
    }
    false
}
#[no_mangle]
pub extern "C" fn cvr_capture_rgba8_fmt(f: DXGI_FORMAT) -> bool {
    [
        DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_UNORM,
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        DXGI_FORMAT_R8G8B8A8_TYPELESS,
        DXGI_FORMAT_B8G8R8A8_TYPELESS,
        // The second view's RenderFinal2D output is a packed HDR target.
        DXGI_FORMAT_R11G11B10_FLOAT,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R10G10B10A2_UNORM,
    ]
    .contains(&f)
}
/// A live heap pointer: canonical, above the first 64 KB, 8-aligned (a freed component's slot holds debris).
fn plausible(p: usize) -> bool {
    (0x10000..0x7FFF_FFFF_FFFF).contains(&p) && p & 7 == 0
}
static TARGET: Mutex<(u32, u32)> = Mutex::new((0, 0));
/// ONE ACCEPTED SIZE, THE ONE FROM THE SELECTED COMPONENT'S NAME; the live RTT component's output texture only while
/// no name was parsed yet.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_target_dimensions(d: &D3D12_RESOURCE_DESC) -> bool {
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D {
        return false;
    }
    let (mut tw, mut th) = (g_vrcam_sel_w.load(Relaxed), g_vrcam_sel_h.load(Relaxed));
    if tw == 0 || th == 0 {
        let mut target = TARGET.lock().unwrap_or_else(|e| e.into_inner());
        if target.0 == 0 {
            let comp = CyberpunkVR_DebugRttComp.load(Relaxed) as usize;
            if plausible(comp) {
                if let Some(dtex) = read::<usize>(comp + 0x1E8) {
                    if dtex != 0 && plausible(dtex) {
                        if let (Some(w), Some(h)) =
                            (read::<u32>(dtex + 0x40), read::<u32>(dtex + 0x44))
                        {
                            if w != 0 && h != 0 {
                                *target = (w, h);
                            }
                        }
                    }
                }
            }
        }
        (tw, th) = *target;
    }
    if tw == 0 || th == 0 {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Relaxed) {
            let name =
                std::ffi::CStr::from_ptr((&raw const g_vrcam_component).cast()).to_string_lossy();
            stereo_log(&format!(
                "[mirror] no VRCAM resolution known (component={name}) -> RTV capture disabled"
            ));
        }
        return false;
    }
    d.Width as u32 == tw && d.Height == th
}

// ---- the second view's output candidates, by descriptor handle

#[derive(Clone, Copy)]
struct Candidate {
    resource: usize,
    view_format: DXGI_FORMAT,
}
static CANDIDATES: LazyLock<Mutex<FastMap<usize, Candidate>>> =
    LazyLock::new(|| Mutex::new(cvr_host::fast::fast_map()));
/// An RTV created at `handle`: a candidate when the view and the resource are of the output's family and size; a
/// reused descriptor drops its old entry even when the replacement is not a candidate.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_mirror_register_rtv(
    resource: *mut c_void,
    view_format: DXGI_FORMAT,
    handle: usize,
) {
    if handle == 0 {
        return;
    }
    let desc = if resource.is_null()
        || cvr_capture_is_foreign(resource)
        || !cvr_capture_rgba8_fmt(view_format)
    {
        None
    } else {
        get_desc(resource)
            .filter(|d| cvr_capture_rgba8_fmt(d.Format) && cvr_capture_target_dimensions(d))
    };
    let mut candidates = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
    match desc {
        Some(_) => {
            candidates.insert(
                handle,
                Candidate {
                    resource: resource as usize,
                    view_format,
                },
            );
        }
        None => {
            if candidates.remove(&handle).is_none() {
                return;
            }
        }
    }
    (*rt().candidate_count).store(candidates.len() as u32, Release);
    if diagnostics() {
        CyberpunkVR_DebugMirrorRtvRegs.fetch_add(1, SeqCst);
    }
}
/// The candidate bound at `handle` and its view format.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_mirror_find_bound_rtv(
    handle: usize,
    view_format: *mut DXGI_FORMAT,
) -> *mut c_void {
    let candidates = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
    let Some(c) = candidates.get(&handle) else {
        return core::ptr::null_mut();
    };
    if let Some(out) = view_format.as_mut() {
        *out = c.view_format;
    }
    c.resource as *mut c_void
}

// ---- every RTV's metadata (the crop pass's sizing, the image pose, the HUD and the optics read it)

/// The swapchain's buffers pinned before the engine creates their descriptors (startup churn included).
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_register_rtv_swapchain(swap_chain: *mut c_void) {
    let Some(sc) = IDXGISwapChain::from_raw_borrowed(&swap_chain) else {
        return;
    };
    let Ok(desc) = sc.GetDesc() else { return };
    if desc.BufferCount == 0 || desc.BufferCount > 16 {
        return;
    }
    let mut resources = [0usize; 16];
    for i in 0..desc.BufferCount {
        let Ok(buffer) = sc.GetBuffer::<ID3D12Resource>(i) else {
            return;
        };
        resources[i as usize] = buffer.as_raw() as usize;
    }
    cvr_rtv_cache_pin(resources.as_ptr(), desc.BufferCount as usize);
}
static HEAP_SLOTS: AtomicU64 = AtomicU64::new(0);
/// Every RTV slot the engine creates: the cache can hold all of them (released heaps stay counted).
#[no_mangle]
pub extern "C" fn cvr_capture_rtv_heap_created(slots: u32) {
    if slots != 0 {
        cvr_rtv_cache_reserve(
            (HEAP_SLOTS.fetch_add(slots as u64, Relaxed) + slots as u64) as usize,
        );
    }
}
#[no_mangle]
pub extern "C" fn cvr_capture_rtv_descriptor_limit() -> u32 {
    cvr_rtv_cache_limit() as u32
}
/// The resource and size an RTV at `handle` names (a non-texture or null target erases it).
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_rtv_dim_register(handle: usize, res: *mut c_void) {
    if handle == 0 {
        return;
    }
    let target = match get_desc(res) {
        Some(d) if d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D => DescriptorTarget {
            resource: res as usize,
            width: d.Width as u32,
            height: d.Height,
        },
        _ => DescriptorTarget::default(),
    };
    let mut evicted = false;
    cvr_rtv_cache_store(handle, target, &mut evicted);
    let r = rt();
    (*r.rtv_dim_count).store(cvr_rtv_cache_size() as u32, Relaxed);
    if evicted && !(*r.rtv_dim_wrapped_logged).swap(true, SeqCst) {
        stereo_log(&format!(
            "[mirror] RTV metadata cache full ({} of {} heap slots); retiring unused descriptors, retaining swapchain targets",
            cvr_capture_rtv_descriptor_limit(),
            HEAP_SLOTS.load(Relaxed)
        ));
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_rtv_resource_lookup(handle: usize) -> *mut c_void {
    let mut target = DescriptorTarget::default();
    if cvr_rtv_cache_read(handle, &mut target) {
        target.resource as *mut c_void
    } else {
        core::ptr::null_mut()
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_rtv_dim_lookup(
    handle: usize,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    let mut target = DescriptorTarget::default();
    if !cvr_rtv_cache_read(handle, &mut target) {
        return false;
    }
    *width = target.width;
    *height = target.height;
    true
}
