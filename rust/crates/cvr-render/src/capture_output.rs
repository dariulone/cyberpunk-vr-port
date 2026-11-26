//! THE SECOND VIEW'S OUTPUT, AS CAPTURED (part of formerly src/Stereo/Capture.cpp): the target the second view's
//! RenderFinal2D wrote, published every frame (the engine rotates a ring of persistent targets, each AddRef'd once and
//! kept), with the watchdog that names which test refused it when the eye goes stale; the committed target of ours the
//! engine can be redirected to render into (CyberpunkVR_VrcamOwnTarget, off); and the cascade pass's depth target per
//! view and index (the answered probe).
//!
//! Capture.cpp keeps the exports and forwards its C++ names here.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::capture_rtv::{cvr_capture_is_foreign, cvr_capture_target_dimensions};

extern "C" {
    static CyberpunkVR_DebugPubOk: AtomicU64;
    static CyberpunkVR_DebugMirrorFmt: AtomicU32;
    static CyberpunkVR_DebugMirrorRes: AtomicU64;
}
#[repr(C)]
pub struct Runtime {
    get_desc: extern "C" fn(*mut c_void, *mut D3D12_RESOURCE_DESC) -> bool,
    /// g_captured_vrcam_res: the second view's current output (the mirror and the eye copy read it).
    captured: *const AtomicPtr<c_void>,
    /// g_own_target, g_own_rtv_heap, g_own_rtv (Mirror.cpp's).
    own_target: *mut *mut c_void,
    own_rtv_heap: *mut *mut c_void,
    own_rtv: *mut usize,
    /// Takes (true) or releases (false) Mirror.cpp's g_own_target_mtx, which CyberpunkVR_GetVrcamEyeTexture reads under.
    own_target_lock: extern "C" fn(bool),
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    get_desc: s::get_desc,
    captured: &raw const s::g_captured_vrcam_res,
    own_target: &raw mut s::g_own_target,
    own_rtv_heap: &raw mut s::g_own_rtv_heap,
    own_rtv: &raw mut s::g_own_rtv,
    own_target_lock: s::own_target_lock,
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// Why the output was or was not accepted, tallied for the watchdog.
struct Publish {
    rej_foreign: u64,
    rej_desc: u64,
    rej_dims: u64,
    same: u64,
    ok: u64,
    last_ok_ms: u64,
    last_warn_ms: u64,
}
static PUBLISH: Mutex<Publish> = Mutex::new(Publish {
    rej_foreign: 0,
    rej_desc: 0,
    rej_dims: 0,
    same: 0,
    ok: 0,
    last_ok_ms: 0,
    last_warn_ms: 0,
});
/// The second view's output target this frame: accepted when it is not ours and is of the VRCAM size, and published as
/// the current output (stereo_shared.rs MIRROR_OUTPUT: referenced while it is the output or leased by a copy in flight).
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_publish_output(
    resource: *mut c_void,
    view_format: DXGI_FORMAT,
) {
    let r = rt();
    let mut p = PUBLISH.lock().unwrap_or_else(|e| e.into_inner());
    // Watchdog: the output not refreshed for a second while binds keep arriving is the state the headset shows as
    // mono; said once every 5 s with the tally that explains it.
    let now = tick();
    if p.last_ok_ms == 0 {
        p.last_ok_ms = now;
    }
    if p.ok != CyberpunkVR_DebugPubOk.load(Relaxed) {
        CyberpunkVR_DebugPubOk.store(p.ok, Relaxed);
        p.last_ok_ms = now;
    }
    if now - p.last_ok_ms > 1000 && now - p.last_warn_ms > 5000 {
        p.last_warn_ms = now;
        stereo_log(&format!(
            "[mirror] vrcam output has not been accepted for {} ms -- the second eye is going stale. rejected: foreign={} desc={} dims={}, unchanged={}, accepted={}",
            now - p.last_ok_ms,
            p.rej_foreign,
            p.rej_desc,
            p.rej_dims,
            p.same,
            p.ok
        ));
    }
    if resource.is_null() || cvr_capture_is_foreign(resource) {
        p.rej_foreign += 1;
        return;
    }
    let mut desc = D3D12_RESOURCE_DESC::default();
    if !(r.get_desc)(resource, &mut desc) {
        p.rej_desc += 1;
        return;
    }
    if !cvr_capture_target_dimensions(&desc) {
        p.rej_dims += 1;
        return;
    }
    p.ok += 1;
    // The same resource as last time is the normal case on a still frame, counted apart from the failures.
    if !s::MIRROR_OUTPUT.publish(resource) {
        p.same += 1;
        return;
    }
    let _ = view_format;
    drop(p);
    CyberpunkVR_DebugMirrorFmt.store(desc.Format.0 as u32, Relaxed);
    CyberpunkVR_DebugMirrorRes.store(resource as u64, Relaxed);
}

/// g_own_target_mtx held for a scope.
struct OwnTargetGuard;
impl OwnTargetGuard {
    fn take() -> Self {
        (rt().own_target_lock)(true);
        OwnTargetGuard
    }
}
impl Drop for OwnTargetGuard {
    fn drop(&mut self) {
        (rt().own_target_lock)(false);
    }
}
/// A committed texture matching `src` (resting in RENDER_TARGET) and an RTV in a heap of ours, made once, so the engine
/// can render the second view's final straight into it. The RTV handle, or 0.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_ensure_own_target_rtv(
    src: *mut c_void,
    view_format: DXGI_FORMAT,
) -> usize {
    let r = rt();
    let device = crate::device_creation::game_objects::game_device();
    if device.is_null() || src.is_null() {
        return 0;
    }
    let _lock = OwnTargetGuard::take();
    if !(*r.own_target).is_null() {
        return *r.own_rtv;
    }
    let mut d = D3D12_RESOURCE_DESC::default();
    if !(r.get_desc)(src, &mut d) {
        return 0;
    }
    let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
        return 0;
    };
    let heap_props = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
    d.Flags |= D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    let mut tex: Option<ID3D12Resource> = None;
    if dev
        .CreateCommittedResource(
            &heap_props,
            D3D12_HEAP_FLAG_NONE,
            &d,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            None,
            &mut tex,
        )
        .is_err()
    {
        return 0;
    }
    let Some(tex) = tex else { return 0 };
    let heap_desc = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
        NumDescriptors: 1,
        ..Default::default()
    };
    let Ok(heap) = dev.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&heap_desc) else {
        return 0;
    };
    let format = if view_format != DXGI_FORMAT_UNKNOWN {
        view_format
    } else {
        d.Format
    };
    let rtv_desc = D3D12_RENDER_TARGET_VIEW_DESC {
        Format: format,
        ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let handle = heap.GetCPUDescriptorHandleForHeapStart();
    dev.CreateRenderTargetView(&tex, Some(&rtv_desc), handle);
    let name: Vec<u16> = "CyberpunkVR_VrcamOwnTarget"
        .encode_utf16()
        .chain([0])
        .collect();
    let _ = tex.SetName(windows::core::PCWSTR(name.as_ptr()));
    let raw = tex.into_raw();
    *r.own_target = raw;
    *r.own_rtv_heap = heap.into_raw();
    *r.own_rtv = handle.ptr;
    stereo_log(&format!(
        "[owntgt] committed vrcam target={:016X} {}x{} fmt={}",
        raw as usize, d.Width, d.Height, format.0
    ));
    handle.ptr
}

// ---- the cascade pass's depth target, per view and index (answered: one descriptor slot, two textures; default off)

struct CascRt {
    dsv: [u64; 2],
    hits: [u64; 2],
}
static CASC_RT: Mutex<[CascRt; 8]> = Mutex::new(
    [const {
        CascRt {
            dsv: [0; 2],
            hits: [0; 2],
        }
    }; 8],
);
#[no_mangle]
pub extern "C" fn cvr_capture_cascrt_note(dsv: u64, index: i32, vrcam: bool) {
    if !(0..8).contains(&index) {
        return;
    }
    let mut c = CASC_RT.lock().unwrap_or_else(|e| e.into_inner());
    let v = vrcam as usize;
    c[index as usize].dsv[v] = dsv;
    c[index as usize].hits[v] += 1;
}
#[no_mangle]
pub extern "C" fn cvr_capture_cascrt_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now - last < 5000 {
        return;
    }
    LAST.store(now, Relaxed);
    let c = CASC_RT.lock().unwrap_or_else(|e| e.into_inner());
    let mut line = String::new();
    for (i, x) in c.iter().enumerate() {
        if x.hits == [0, 0] || line.len() >= 600 - 90 {
            continue;
        }
        let both = x.hits[0] != 0 && x.hits[1] != 0;
        line.push_str(&format!(
            "casc{i} M={:X}({}) V={:X}({}) {}  ",
            x.dsv[0],
            x.hits[0],
            x.dsv[1],
            x.hits[1],
            if !both {
                "ONE-SIDED"
            } else if x.dsv[0] == x.dsv[1] {
                "SAME"
            } else {
                "differ"
            }
        ));
    }
    stereo_log(&format!(
        "[cascrt] depth target bound by the cascade pass, per view and index: {}",
        if line.is_empty() {
            "(none seen)"
        } else {
            line.as_str()
        }
    ));
}
