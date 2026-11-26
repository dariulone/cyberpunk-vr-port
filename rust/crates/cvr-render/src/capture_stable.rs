//! THE SECOND EYE'S STABLE SNAPSHOT (part of formerly src/Stereo/Capture.cpp): the copy, recorded INTO THE
//! ENGINE'S OWN command list at the second view's node epilogue, of its freshly written output into our committed
//! g_stable_tex -- in queue order after the last write and before any aliasing barrier re-purposes the transient's
//! heap (the proven bright/dark root); typed, because the finished frame is R8G8B8A8_TYPELESS and an SRV over a
//! typeless snapshot is the INVALID_CALL device removal. The eye texture the OpenXR submit reads (existence and
//! liveness, with the [stereo-eye] tally that separates the four ways the snapshot can stop); the finished frame
//! armed by the barrier hook and copied here; and the desktop mirror's texture and the copy into it.
//!
//! The objects are stereo_shared.rs's and moved_globals.rs's (formerly SyncStereo.cpp's and Mirror.cpp's).
#![allow(unsafe_op_in_unsafe_fn)]
extern "C" {
    /// LatestSubmittedVrcamImage's Rust half (cvr-rig's image_pose.rs).
    fn cvr_image_pose_latest_stable() -> *mut c_void;
}
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::capture_rtv::cvr_capture_rtv_descriptor_limit;
use crate::hud::Barrier;
use crate::single_pass::Hook;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_StableCopy: AtomicI32;
    static CyberpunkVR_StableSrgbView: AtomicI32;
    static CyberpunkVR_DebugStableCopies: AtomicU64;
    static CyberpunkVR_DebugStableSkips: AtomicU64;
    static CyberpunkVR_DebugStableSrcState: AtomicU32;
    static CyberpunkVR_DebugMirrorStage: AtomicU64;
    static CyberpunkVR_VrcamOwnTarget: AtomicI32;
    static CyberpunkVR_StereoEyeMaxAgeMs: AtomicU32;
    static CyberpunkVR_DebugVrcamEyeAgeMs: AtomicU32;
    static CyberpunkVR_DebugVrcamEyeAgeUs: AtomicU32;
    static CyberpunkVR_DebugRtvPickLog: AtomicI32;
    static CyberpunkVR_DebugMirrorRtvRegs: AtomicU64;
    static CyberpunkVR_DebugMirrorRtvEvicts: AtomicU64;
    static CyberpunkVR_DebugFinalGrabs: AtomicU64;
    fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const Hook;
}

#[repr(C)]
pub struct Runtime {
    get_desc: extern "C" fn(*mut c_void, *mut D3D12_RESOURCE_DESC) -> bool,
    /// g_stable_tex / g_stable_desc, under g_stable_mtx (SyncStereo.cpp's; Mirror.cpp reads them).
    stable_tex: *mut *mut c_void,
    stable_desc: *mut D3D12_RESOURCE_DESC,
    stable_lock: extern "C" fn(bool),
    stable_fresh: *const AtomicBool,
    /// GetTickCount64 milliseconds (three readers compare it that way) and its QPC microseconds twin.
    stable_tick: *const AtomicU64,
    stable_tick_us: *const AtomicU64,
    have_tonemap_source: *const AtomicBool,
    /// The four preconditions of the second eye, counted where they fail (SyncStereo.cpp's).
    eye_node_hits: *const AtomicU64,
    eye_no_rtv: *const AtomicU64,
    eye_no_list: *const AtomicU64,
    eye_copy_calls: *const AtomicU64,
    candidate_count: *const AtomicU32,
    /// g_own_target under g_own_target_mtx (Mirror.cpp's).
    own_target: *const *mut c_void,
    own_target_lock: extern "C" fn(bool),
    /// The desktop mirror's texture and fence, under g_d12_mtx (Mirror.cpp's).
    d12_mtex: *mut *mut c_void,
    d12_mtex_is_rt: *mut bool,
    d12_fence: *mut *mut c_void,
    d12_lock: extern "C" fn(bool),
    d12_w: *mut u32,
    d12_h: *mut u32,
    d12_fmt: *mut DXGI_FORMAT,
    mirror_pending_list: *const AtomicPtr<c_void>,
    capture_wanted: extern "C" fn() -> bool,
    /// cvr::camera::RecordImagePose(list, stable, pose ? *pose : CurrentNodePoseIdentity(), true).
    record_stable_pose: extern "C" fn(*mut c_void, *mut c_void, *const c_void),
    /// The finished-frame slot (thread-local t_final_res / t_final_state / t_final_list): read, or cleared.
    final_slot: extern "C" fn(bool, *mut *mut c_void, *mut u32, *mut *mut c_void),
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    get_desc: s::get_desc,
    stable_tex: &raw mut s::g_stable_tex,
    stable_desc: &raw mut s::g_stable_desc,
    stable_lock: s::stable_lock,
    stable_fresh: &raw const m::g_stable_fresh,
    stable_tick: &raw const m::g_stable_tick,
    stable_tick_us: &raw const m::g_stable_tick_us,
    have_tonemap_source: &raw const m::g_have_tonemap_source,
    eye_node_hits: &raw const m::g_eye_node_hits,
    eye_no_rtv: &raw const m::g_eye_no_rtv,
    eye_no_list: &raw const m::g_eye_no_list,
    eye_copy_calls: &raw const m::g_eye_copy_calls,
    candidate_count: &raw const s::MIRROR_RTV_CANDIDATE_COUNT,
    own_target: &raw const s::g_own_target,
    own_target_lock: s::own_target_lock,
    d12_mtex: &raw mut s::g_d12_mtex,
    d12_mtex_is_rt: &raw mut s::g_d12_mtex_is_rt,
    d12_fence: &raw mut s::g_d12_fence,
    d12_lock: s::d12_lock,
    d12_w: &raw mut s::g_d12_w,
    d12_h: &raw mut s::g_d12_h,
    d12_fmt: &raw mut s::g_d12_fmt,
    mirror_pending_list: &raw const s::g_mirror_pending_list,
    capture_wanted: s::capture_wanted,
    record_stable_pose: s::record_stable_pose,
    final_slot: s::final_slot,
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn diag(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
/// A C++ std::mutex held for a scope, through its adapter.
struct Held(extern "C" fn(bool));
impl Held {
    fn take(lock: extern "C" fn(bool)) -> Self {
        lock(true);
        Held(lock)
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        (self.0)(false);
    }
}
fn set_name(res: &ID3D12Resource, name: &str) {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated wide string that outlives the call.
    let _ = unsafe { res.SetName(windows::core::PCWSTR(wide.as_ptr())) };
}
const fn state(s: D3D12_RESOURCE_STATES) -> u32 {
    s.0 as u32
}

/// A MILLISECOND CLOCK THAT CAN SEE A FRAME: GetTickCount64's ~15.6 ms granularity reported the eye's age as exactly
/// 0, 16 or 31 ms. QPC, still in milliseconds so CyberpunkVR_StereoEyeMaxAgeMs keeps its units.
fn stable_now_ms() -> f64 {
    static FREQ: AtomicU64 = AtomicU64::new(0);
    let mut freq = FREQ.load(Relaxed) as i64;
    if freq == 0 {
        // SAFETY: a plain system query.
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut freq);
        }
        FREQ.store(freq as u64, Relaxed);
    }
    if freq == 0 {
        return 0.0;
    }
    let mut t = 0i64;
    // SAFETY: a plain system query.
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut t);
    }
    t as f64 * 1000.0 / freq as f64
}

fn stage_desc_matches(a: &D3D12_RESOURCE_DESC, b: &D3D12_RESOURCE_DESC) -> bool {
    a.Dimension == b.Dimension
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == b.DepthOrArraySize
        && a.MipLevels == b.MipLevels
        && a.Format == b.Format
        && a.SampleDesc.Count == b.SampleDesc.Count
        && a.SampleDesc.Quality == b.SampleDesc.Quality
}

/// The snapshot's typed format: the 8-bit TYPELESS families as _UNORM_SRGB (ships; the finished frame carries
/// sRGB-encoded bytes, so sample-decode and ColorBlit's own sRGB target round-trip) or _UNORM
/// (CyberpunkVR_StableSrgbView=0); the wider TYPELESS families as their one usual member; a typed source untouched.
fn stable_typed_format(f: DXGI_FORMAT) -> DXGI_FORMAT {
    // SAFETY: an export of the C++ half.
    let srgb = unsafe { CyberpunkVR_StableSrgbView.load(Relaxed) } != 0;
    if f == DXGI_FORMAT_R8G8B8A8_TYPELESS {
        if srgb {
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        } else {
            DXGI_FORMAT_R8G8B8A8_UNORM
        }
    } else if f == DXGI_FORMAT_B8G8R8A8_TYPELESS {
        if srgb {
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        } else {
            DXGI_FORMAT_B8G8R8A8_UNORM
        }
    } else if f == DXGI_FORMAT_R10G10B10A2_TYPELESS {
        DXGI_FORMAT_R10G10B10A2_UNORM
    } else if f == DXGI_FORMAT_R16G16B16A16_TYPELESS {
        DXGI_FORMAT_R16G16B16A16_FLOAT
    } else if f == DXGI_FORMAT_R32G32B32A32_TYPELESS {
        DXGI_FORMAT_R32G32B32A32_FLOAT
    } else {
        f
    }
}

fn default_heap() -> D3D12_HEAP_PROPERTIES {
    D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    }
}

/// The desktop mirror's texture (ALLOW_RENDER_TARGET so the HUD debug overlay can be drawn onto it, falling back to
/// a plain target) and its fence, made once from the source's desc.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_d12_mirror_ensure(src: *const D3D12_RESOURCE_DESC) -> bool {
    let r = rt();
    if !(*r.d12_mtex).is_null() {
        return true;
    }
    let _lock = Held::take(r.d12_lock);
    if !(*r.d12_mtex).is_null() {
        return true;
    }
    let device = crate::device_creation::game_objects::game_device();
    let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
        return false;
    };
    let hp = default_heap();
    let mut d = *src;
    d.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    let mut tex: Option<ID3D12Resource> = None;
    let made = dev
        .CreateCommittedResource(
            &hp,
            D3D12_HEAP_FLAG_NONE,
            &d,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut tex,
        )
        .is_ok()
        && tex.is_some();
    if !made {
        // The historical plain target: the mirror is worth more than the overlay.
        d.Flags = D3D12_RESOURCE_FLAG_NONE;
        tex = None;
        let made = dev
            .CreateCommittedResource(
                &hp,
                D3D12_HEAP_FLAG_NONE,
                &d,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut tex,
            )
            .is_ok()
            && tex.is_some();
        if !made {
            return false;
        }
        stereo_log("[mirror] mirror-tex has no RENDER_TARGET flag -- HUD debug overlay disabled");
    } else {
        *r.d12_mtex_is_rt = true;
    }
    let Some(tex) = tex else { return false };
    if (*r.d12_fence).is_null() {
        match dev.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
            Ok(fence) => *r.d12_fence = fence.into_raw(),
            Err(_) => return false,
        }
    }
    set_name(&tex, "CyberpunkVR_MirrorTex");
    *r.d12_w = d.Width as u32;
    *r.d12_h = d.Height;
    *r.d12_fmt = d.Format;
    let raw = tex.into_raw();
    *r.d12_mtex = raw;
    CyberpunkVR_DebugMirrorStage.store(raw as u64, Relaxed);
    stereo_log(&format!(
        "[mirror] d12 mirror-tex={:016X} {}x{} fmt={}",
        raw as usize, d.Width as u32, d.Height, d.Format.0 as u32
    ));
    true
}

/// `src` (resting in `src_state`) copied into the committed, typed snapshot on the engine's own `list`, both
/// restored afterwards so the engine's own state tracking stays intact; the snapshot's image pose recorded beside it.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_stable_inline_copy(
    list: *mut c_void,
    src: *mut c_void,
    src_state: u32,
    pose: *const c_void,
) {
    let r = rt();
    let device = crate::device_creation::game_objects::game_device();
    if device.is_null() || list.is_null() || src.is_null() {
        return;
    }
    let mut d = D3D12_RESOURCE_DESC::default();
    let hook = cvr_cpp_command_list_hook_entry(list).as_ref();
    let Some(hook) =
        hook.filter(|h| h.barrier_call != 0 && h.copyres != 0 && (r.get_desc)(src, &mut d))
    else {
        diag(&CyberpunkVR_DebugStableSkips);
        return;
    };
    let stable = {
        let _lock = Held::take(r.stable_lock);
        if !(*r.stable_tex).is_null() && !stage_desc_matches(&*r.stable_desc, &d) {
            // Resolution changed: the old snapshot may still be referenced by an in-flight deferred copy, so it is
            // intentionally leaked (rare, dev-time only) and recreated.
            *r.stable_tex = null_mut();
        }
        if (*r.stable_tex).is_null() {
            let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
                return;
            };
            let mut nd = d;
            nd.Format = stable_typed_format(d.Format);
            // A plain copy target: ALLOW_SIMULTANEOUS_ACCESS here was inherited by the mirror's 11on12 surface and
            // failed it with DXGI_ERROR_ACCESS_DENIED.
            nd.Flags = D3D12_RESOURCE_FLAG_NONE;
            let mut tex: Option<ID3D12Resource> = None;
            let made = dev
                .CreateCommittedResource(
                    &default_heap(),
                    D3D12_HEAP_FLAG_NONE,
                    &nd,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut tex,
                )
                .is_ok();
            let Some(tex) = tex.filter(|_| made) else {
                diag(&CyberpunkVR_DebugStableSkips);
                return;
            };
            set_name(&tex, "CyberpunkVR_VrcamStable");
            let raw = tex.into_raw();
            *r.stable_tex = raw;
            *r.stable_desc = d;
            stereo_log(&format!(
                "[stable] committed vrcam snapshot={:016X} {}x{} fmt={} (source fmt={})",
                raw as usize, d.Width, d.Height, nd.Format.0 as u32, d.Format.0 as u32
            ));
        }
        *r.stable_tex
    };
    let barrier_call: unsafe extern "system" fn(*mut c_void, u32, *const Barrier) =
        core::mem::transmute(hook.barrier_call);
    let copy: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) =
        core::mem::transmute(hook.copyres);
    let copy_source = state(D3D12_RESOURCE_STATE_COPY_SOURCE);
    let copy_dest = state(D3D12_RESOURCE_STATE_COPY_DEST);
    let common = state(D3D12_RESOURCE_STATE_COMMON);
    // src: current state -> COPY_SOURCE (skipped if already there); stable: COMMON -> COPY_DEST; copy; restore both.
    let mut b = Vec::with_capacity(2);
    if src_state != copy_source {
        b.push(Barrier::transition(src, src_state, copy_source));
    }
    b.push(Barrier::transition(stable, common, copy_dest));
    barrier_call(list, b.len() as u32, b.as_ptr());
    copy(list, stable, src);
    // preparation::EyeCopyRecorded (preparation_trace.rs).
    crate::preparation_trace::cvr_prep_eye_copy(list as u64, src as u64, stable as u64);
    (r.record_stable_pose)(list, stable, pose);
    b.clear();
    if src_state != copy_source {
        b.push(Barrier::transition(src, copy_source, src_state));
    }
    b.push(Barrier::transition(stable, copy_dest, common));
    barrier_call(list, b.len() as u32, b.as_ptr());
    (*r.stable_fresh).store(true, Release);
    // TWO STAMPS: the coarse one keeps its unit and epoch for the gates that compare it with GetTickCount64 (moving
    // it to QPC once made the HUD's liveness test false and the HUD vanished from the second eye); the fine one
    // exists beside it for the reported age.
    (*r.stable_tick).store(tick(), Release);
    if diagnostics() {
        (*r.stable_tick_us).store((stable_now_ms() * 1000.0) as u64, Release);
    }
    CyberpunkVR_DebugStableSrcState.store(src_state, Relaxed);
    diag(&CyberpunkVR_DebugStableCopies);
}

/// The second view's colour for eye 1: our own committed target when the engine renders into it, else the latest
/// submitted snapshot once there has been one.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_get_vrcam_eye_texture() -> *mut c_void {
    let r = rt();
    if CyberpunkVR_VrcamOwnTarget.load(Relaxed) != 0 {
        let _lock = Held::take(r.own_target_lock);
        let own = *r.own_target;
        if !own.is_null() {
            return own;
        }
    }
    if !(*r.stable_fresh).load(Acquire) {
        return null_mut();
    }
    let _lock = Held::take(r.stable_lock);
    let _ = r;
    cvr_image_pose_latest_stable()
}

/// The same, but null once the second view has gone quiet: one eye pinned to a still image reads far worse than
/// mono. The gate runs on the coarse clock; the reported age on the fine one.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_get_vrcam_eye_texture_fresh() -> *mut c_void {
    let r = rt();
    let last = (*r.stable_tick).load(Acquire);
    if last == 0 {
        CyberpunkVR_DebugVrcamEyeAgeMs.store(0xFFFF_FFFF, Relaxed);
        return null_mut();
    }
    let age = tick().wrapping_sub(last);
    if diagnostics() {
        let last_us = (*r.stable_tick_us).load(Acquire);
        let now_us = (stable_now_ms() * 1000.0) as u64;
        let age_us = if last_us != 0 && now_us > last_us {
            now_us - last_us
        } else {
            0
        };
        CyberpunkVR_DebugVrcamEyeAgeUs.store(age_us.min(0xFFFF_FFFE) as u32, Relaxed);
    }
    CyberpunkVR_DebugVrcamEyeAgeMs.store(age.min(0xFFFF_FFFE) as u32, Relaxed);
    let limit = CyberpunkVR_StereoEyeMaxAgeMs.load(Relaxed);
    if age > limit as u64 {
        // THIS RETURN IS THE MONO: said once every 5 s with the tally that separates the ways the snapshot stops
        // (nodeHits: the node runs; noRtv: no output recognised; noList: no hooked list; copies/skips: attempted
        // and refused) and the RTV table's candidates against its registrations and evictions.
        if diagnostics() {
            static LAST_SAID: AtomicU64 = AtomicU64::new(0);
            let now = tick();
            if now.wrapping_sub(LAST_SAID.load(Relaxed)) > 5000 {
                LAST_SAID.store(now, Relaxed);
                stereo_log(&format!(
                    "[stereo-eye] stale by {age} ms (limit {limit}) -> submitting MONO. nodeHits={} noRtv={} noList={} copies={} skips={} attempted={} | rtv table: candidates={}, metadata limit {}, regs={} evicts={}",
                    (*r.eye_node_hits).load(Relaxed),
                    (*r.eye_no_rtv).load(Relaxed),
                    (*r.eye_no_list).load(Relaxed),
                    CyberpunkVR_DebugStableCopies.load(Relaxed),
                    CyberpunkVR_DebugStableSkips.load(Relaxed),
                    (*r.eye_copy_calls).load(Relaxed),
                    (*r.candidate_count).load(Relaxed),
                    cvr_capture_rtv_descriptor_limit(),
                    CyberpunkVR_DebugMirrorRtvRegs.load(Relaxed),
                    CyberpunkVR_DebugMirrorRtvEvicts.load(Relaxed)
                ));
                // [rtvpick] burned its budget during startup; a short burst shows the handles bound NOW.
                if CyberpunkVR_DebugRtvPickLog.load(Relaxed) <= 0 {
                    CyberpunkVR_DebugRtvPickLog.store(6, Relaxed);
                }
            }
        }
        return null_mut();
    }
    cvr_capture_get_vrcam_eye_texture()
}

/// The copy dtex -> mirror texture appended to the game's own list at the dtex's RENDER_TARGET -> `after`
/// transition, bracketed with matching states; the list then marked for the mirror's submit.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_d12_append_mirror_copy(
    hook: *const c_void,
    list: *mut c_void,
    dtex: *mut c_void,
    after: u32,
) {
    let r = rt();
    let mtex = *r.d12_mtex;
    let Some(hook) = hook.cast::<Hook>().as_ref() else { return };
    if hook.copyres == 0 || hook.barrier_call == 0 || dtex.is_null() || mtex.is_null() {
        return;
    }
    let barrier_call: unsafe extern "system" fn(*mut c_void, u32, *const Barrier) =
        core::mem::transmute(hook.barrier_call);
    let copy: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) =
        core::mem::transmute(hook.copyres);
    let copy_source = state(D3D12_RESOURCE_STATE_COPY_SOURCE);
    let copy_dest = state(D3D12_RESOURCE_STATE_COPY_DEST);
    let common = state(D3D12_RESOURCE_STATE_COMMON);
    let before = [
        Barrier::transition(dtex, after, copy_source),
        Barrier::transition(mtex, common, copy_dest),
    ];
    barrier_call(list, 2, before.as_ptr());
    copy(list, mtex, dtex);
    let restore = [
        Barrier::transition(dtex, copy_source, after),
        Barrier::transition(mtex, copy_dest, common),
    ];
    barrier_call(list, 2, restore.as_ptr());
    (*r.mirror_pending_list).store(list, Release);
}

/// THE FINISHED FRAME AT THE NODE EPILOGUE: the barrier hook only arms the slot (copying from inside it landed one
/// frame in five); the second view's CompositionPostProcess epilogue copies it here, every write in the list, the
/// list still open, the resource not aliased yet. The first delivery retires the older sources for good.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_final_frame_copy_now() {
    let r = rt();
    let (mut res, mut st, mut list) = (null_mut(), 0u32, null_mut());
    (r.final_slot)(false, &mut res, &mut st, &mut list);
    if res.is_null() || st == 0 || list.is_null() {
        return;
    }
    if CyberpunkVR_StableCopy.load(Relaxed) != 0 && (r.capture_wanted)() {
        cvr_capture_stable_inline_copy(list, res, st, core::ptr::null());
        (*r.have_tonemap_source).store(true, Release);
        diag(&CyberpunkVR_DebugFinalGrabs);
    }
    (r.final_slot)(true, null_mut(), null_mut(), null_mut());
}
