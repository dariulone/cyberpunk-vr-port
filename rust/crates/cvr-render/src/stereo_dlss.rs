//! STREAMLINE AND DLSS, MADE TO AGREE ABOUT TWO VIEWS (the detours of formerly src/Stereo/Dlss.cpp).
//!
//! Five detours: the Streamline constant upload (the camera writer), the DLSS constants, the evaluate call, the apply
//! node and the render-resolution query. Every one was written for a single view; the second view (VRCAM) is either
//! given its own answer or excluded from the first's.
//!
//!   * the camera writer pins MAIN's view context (the only place that sees the view object and the context of one
//!     view), mirrors MAIN's AA mode onto VRCAM (the Streamline history fix), takes MAIN's camera fov, zoom, near, far
//!     and projection (the ADS factor) and hands them to VRCAM's context and component, and marks the VRCAM call so
//!     the DLSS constants driver inside it switches to VRCAM's own Streamline viewport;
//!   * the constants and evaluate drivers flip the shared DLSS state to VRCAM's viewport (and VRCAM's own
//!     changed-detection cache) around VRCAM's calls;
//!   * the apply node mirrors MAIN's DLSS flag onto VRCAM; the render-resolution query makes VRCAM upscale like MAIN.
//!
//! Engine memory is read and written through the SEH adapter (cvr_engine::code). src/Stereo/Dlss.cpp keeps the
//! exports, the registry lines (CVR_DETOUR) with the trampolines it defines, the command-list hubs, and hands this
//! module the stereo state it shares with the C++ half (Runtime).
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use cvr_engine::code::{guarded_call4, read, read_bytes, write};
use std::sync::{LazyLock, Mutex};

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_vrcam_ctx_key: AtomicU64;
    static CyberpunkVR_DebugMainCtxBinds: AtomicU64;
    static CyberpunkVR_DebugMainCtx: AtomicU64;
    static CyberpunkVR_StreamlineHistoryFix: AtomicU32;
    static CyberpunkVR_DebugVrcamAaMode: AtomicU32;
    static CyberpunkVR_DebugVrcamBuildModeF90: AtomicU32;
    static CyberpunkVR_DebugMainAaMode: AtomicU32;
    static CyberpunkVR_DebugMainBuildModeF90: AtomicU32;
    static CyberpunkVR_DebugSlHistoryHits: AtomicU64;
    static CyberpunkVR_ForceVrcamCam: AtomicU32;
    static CyberpunkVR_MainAdsZoomFactor: AtomicU32;
    static CyberpunkVR_DebugMainCamFov: AtomicU32;
    static CyberpunkVR_DebugMainProjYY: AtomicU32;
    static CyberpunkVR_VrcamCamFields: AtomicU32;
    static CyberpunkVR_VrcamFovDeg: AtomicU32;
    static CyberpunkVR_DebugVrcamWantFov: AtomicU32;
    static CyberpunkVR_DebugForceCamHits: AtomicU64;
    static CyberpunkVR_VrcamDlss: AtomicI32;
    static CyberpunkVR_VrcamDlssViewport: AtomicI32;
    static CyberpunkVR_VrcamDlssZeroJitter: AtomicI32;
    static CyberpunkVR_DebugVrcamDlssConstHits: AtomicU64;
    static CyberpunkVR_DebugVrcamDlssEvalHits: AtomicU64;
    static CyberpunkVR_DebugVrcamApplyDlssHits: AtomicU64;
    static CyberpunkVR_VrcamDlssScale: AtomicU32;
    static CyberpunkVR_VrcamClearFlag64: AtomicU32;
    static CyberpunkVR_DebugClearFlag64Hits: AtomicU64;
    static CyberpunkVR_VrcamComputeResolve: AtomicU32;
    static CyberpunkVR_DebugP17D0Hits: AtomicU64;
    static CyberpunkVR_DebugP17D0Before: AtomicU64;
    static CyberpunkVR_DebugP17D0After: AtomicU64;
    static CyberpunkVR_DebugVrcamResScaleHits: AtomicU64;
    static CyberpunkVR_DebugVrcamRenderW: AtomicU32;
    static CyberpunkVR_DebugVrcamRenderH: AtomicU32;
    /// Hooks/DlssRes (rust/crates/cvr-rig/src/vrik/dlss_res.rs): the size log.
    fn OnDLSSResCallback(dlss: *mut c_void);
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn f32_of(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}
fn set_f32(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Relaxed);
}

// Offsets (include/Stereo/StereoInternal.hpp, EngineRvas.hpp).
const RENDERER_GLOBAL_RVA: usize = 0x3BBCAE8;
const OFF_VIEWSTATE: usize = 0x4658;
const DLSS_FLAGSET_OFF: usize = 0x25B8;
const DLSS_EVAL_FLAG_BIT: u64 = 1 << 6;
const BUILD_FLAG65_BIT: u64 = 1 << 1;
const DLSS_CACHE_OFF: usize = 0x3C8;
const DLSS_CACHE_SZ: usize = 32;
const DLSS_VP_OFF: usize = 0x498;
const DLSS_JITTER_OFF: usize = 0x1E0;

/// The stereo state src/Stereo/Dlss.cpp shares (C++ linkage there).
/// The trampolines MinHook writes : SetStreamlineConstants, the DLSS constants driver, the tag/eval driver,
/// ApplyDLSS and the render-resolution scaler (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_stereo_dlss_originals: [core::sync::atomic::AtomicUsize; 5] = [const { core::sync::atomic::AtomicUsize::new(0) }; 5];

#[repr(C)]
pub struct Runtime {
    main_view_obj: *const AtomicUsize,
    main_view_ctx: *const AtomicUsize,
    main_cam_fov: *mut f32,
    main_cam_zoom: *mut f32,
    main_cam_near: *mut f32,
    main_cam_far: *mut f32,
    main_proj_yy: *mut f32,
    ads_factor: *mut f32,
    vrcam_comp: *const AtomicUsize,
    vrcam_base_fov: *const f32,
    exe_base: *const usize,
    vrcam_dlss_rw: *mut i32,
    vrcam_dlss_rh: *mut i32,
    vrcam_dlss_ow: *mut i32,
    vrcam_dlss_oh: *mut i32,
    /// t_vrcam_dlss_post (a thread-local of the command-list hubs).
    set_dlss_post: extern "C" fn(bool),
    /// CurrentNodePoseIdentity().view.
    node_view: extern "C" fn() -> u32,
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    main_view_obj: &raw const m::g_main_view_obj,
    main_view_ctx: &raw const m::g_main_view_ctx,
    main_cam_fov: (&raw const m::g_main_cam_fov).cast_mut().cast(),
    main_cam_zoom: (&raw const m::g_main_cam_zoom).cast_mut().cast(),
    main_cam_near: (&raw const m::g_main_cam_near).cast_mut().cast(),
    main_cam_far: (&raw const m::g_main_cam_far).cast_mut().cast(),
    main_proj_yy: (&raw const m::g_main_proj_yy).cast_mut().cast(),
    ads_factor: (&raw const m::g_ads_factor).cast_mut().cast(),
    vrcam_comp: &raw const m::g_vrcam_comp,
    vrcam_base_fov: (&raw const m::g_vrcam_base_fov).cast(),
    exe_base: (&raw const s::g_exe_base).cast(),
    vrcam_dlss_rw: (&raw const s::g_vrcam_dlss_rw).cast_mut().cast(),
    vrcam_dlss_rh: (&raw const s::g_vrcam_dlss_rh).cast_mut().cast(),
    vrcam_dlss_ow: (&raw const s::g_vrcam_dlss_ow).cast_mut().cast(),
    vrcam_dlss_oh: (&raw const s::g_vrcam_dlss_oh).cast_mut().cast(),
    set_dlss_post: s::cvr_cpp_set_dlss_post,
    node_view: s::cvr_cpp_node_view,
};
fn rt() -> &'static Runtime {
    &RT
}

thread_local! {
    /// Set by the camera writer while it runs for the VRCAM view, so the constants driver called inside it flips too.
    static VRCAM_SL_ACTIVE: Cell<bool> = const { Cell::new(false) };
}
static OPTICS_FEED: LazyLock<u64> =
    LazyLock::new(|| crate::vrcam_config::cname_hash(b"vr_optics_feed"));

/// The view-output object from the node argument via the engine's own vcall: obj = ([[a2]] vtable[+0x20])(X).
unsafe fn sl_view_obj(a2: usize) -> Option<usize> {
    let x: usize = read(a2)?;
    if x == 0 {
        return Some(0);
    }
    let vt: usize = read(x)?;
    let getv: usize = read(vt + 0x20)?;
    guarded_call4(
        getv,
        x as *mut c_void,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    )
    .map(|p| p as usize)
}
/// is_main_view (src/Stereo/SyncStereo.cpp): true for MAIN's view ctx.
unsafe fn is_main_view(view: usize) -> Option<bool> {
    if view == 0 {
        return Some(false);
    }
    if read::<u64>(view + 0x28)? != 0 {
        return Some(false);
    }
    let cached = (*rt().main_view_ctx).load(Acquire);
    if cached != 0 {
        return Some(view == cached);
    }
    Some(read::<f32>(view + 0x98)? > 1.3)
}

// ---- the camera writer (sub_1407EE0BC, SetStreamlineConstants)

/// MAIN identity, step 2: this writer is the only place that sees the view OBJECT and the view CTX for the same view,
/// so it is where the object recorded from MAIN-only nodes becomes a ctx every other hook compares against. Re-pinned
/// every frame. key == 0 is required: without it VRCAM's ctx got pinned as MAIN.
unsafe fn pin_main(a2: usize) -> Option<()> {
    let want = (*rt().main_view_obj).load(Acquire);
    if want == 0 {
        return Some(());
    }
    let ctx: usize = read(a2 + 0x18)?;
    if ctx != 0
        && read::<u64>(ctx + 0x28)? == 0
        && sl_view_obj(a2)? == want
        && (*rt().main_view_ctx).swap(ctx, Release) != ctx
    {
        if diagnostics() {
            CyberpunkVR_DebugMainCtxBinds.fetch_add(1, Relaxed);
        }
        CyberpunkVR_DebugMainCtx.store(ctx as u64, Relaxed);
    }
    Some(())
}
/// The Streamline history fix: VRCAM (and the optics feed) take MAIN's AA mode.
unsafe fn history_fix(a2: usize) -> Option<()> {
    let ctx: usize = read(a2 + 0x18)?;
    if ctx == 0 {
        return Some(());
    }
    let key: u64 = read(ctx + 0x28)?;
    let obj = sl_view_obj(a2)?;
    if obj == 0 {
        return Some(());
    }
    let mode_at = obj + 0xFA4;
    let build_mode: u32 = read(obj + 0xFA0)?;
    let vrcam_key = g_vrcam_ctx_key.load(SeqCst);
    if key == vrcam_key || key == *OPTICS_FEED {
        let mode: u32 = read(mode_at)?;
        if key == vrcam_key {
            CyberpunkVR_DebugVrcamAaMode.store(mode, Relaxed);
            CyberpunkVR_DebugVrcamBuildModeF90.store(build_mode, Relaxed);
        }
        // Mirror MAIN.
        let want = CyberpunkVR_DebugMainAaMode.load(Relaxed);
        if mode != want {
            if !write(mode_at, want) {
                return None;
            }
            if diagnostics() {
                CyberpunkVR_DebugSlHistoryHits.fetch_add(1, Relaxed);
            }
        }
    } else if key == 0 {
        // Observe MAIN.
        CyberpunkVR_DebugMainAaMode.store(read(mode_at)?, Relaxed);
        CyberpunkVR_DebugMainBuildModeF90.store(build_mode, Relaxed);
    }
    Some(())
}
/// VRCAM's camera follows MAIN through the camera ctx (this writer runs many times a frame: smooth). MAIN's fov,
/// zoom, near, far and vertical projection are taken; VRCAM's ctx gets the fields CyberpunkVR_VrcamCamFields names,
/// and its component the vertical fov (the only input the RTT projection has), ADS applied.
unsafe fn force_vrcam_cam(a2: usize) -> Option<()> {
    let r = rt();
    let ctx: usize = read(a2 + 0x18)?;
    if ctx == 0 {
        return Some(());
    }
    let key: u64 = read(ctx + 0x28)?;
    if is_main_view(ctx)? {
        *r.main_cam_fov = read(ctx + 0x90)?;
        *r.main_cam_zoom = read(ctx + 0x9C)?;
        *r.main_cam_near = read(ctx + 0xB0)?;
        *r.main_cam_far = read(ctx + 0xB4)?;
        *r.main_proj_yy = read(ctx + 0x214)?;
        // Weapon ADS as a plain scale factor: MAIN's fov scalar is not touched by ADS, so cot(fovV/2) is the baseline
        // the live vertical scale divides by.
        if *r.main_cam_fov > 0.0 {
            let t = (*r.main_cam_fov * 0.5f32 * 0.01745329252f32).tan();
            if t > 0.0 {
                *r.ads_factor = *r.main_proj_yy * t;
                set_f32(&CyberpunkVR_MainAdsZoomFactor, *r.ads_factor);
            }
        }
        set_f32(&CyberpunkVR_DebugMainCamFov, *r.main_cam_fov);
        set_f32(&CyberpunkVR_DebugMainProjYY, *r.main_proj_yy);
    } else if key == g_vrcam_ctx_key.load(SeqCst) && *r.main_cam_fov > 0.0 {
        // Ctx scalars, one field at a time: the context steers culling, the component's fov what is rendered.
        let camf = CyberpunkVR_VrcamCamFields.load(Relaxed);
        if camf & 1 != 0 && !write(ctx + 0x90, *r.main_cam_fov) {
            return None;
        }
        if camf & 2 != 0 && !write(ctx + 0x9C, *r.main_cam_zoom) {
            return None;
        }
        if camf & 4 != 0 && !write(ctx + 0xB0, *r.main_cam_near) {
            return None;
        }
        if camf & 8 != 0 && !write(ctx + 0xB4, *r.main_cam_far) {
            return None;
        }
        // VRCAM's vertical fov (degrees, pre-ADS), computed in double: it round-trips fov -> cot -> projection.
        let mut src_fov = 0.0f64;
        let mut ads = *r.ads_factor as f64;
        let fov_deg = f32_of(&CyberpunkVR_VrcamFovDeg);
        if fov_deg > 1.0 {
            // An explicit override (the headset's own fov); ADS applies on top.
            src_fov = fov_deg as f64;
        } else if *r.main_proj_yy > 0.0 {
            // Follow MAIN: its projection already carries the ADS zoom.
            src_fov = 2.0 * (1.0 / *r.main_proj_yy as f64).atan() * 57.29577951308232;
            ads = 1.0;
        } else if *r.vrcam_base_fov > 0.0 {
            src_fov = *r.vrcam_base_fov as f64;
        }
        let comp = (*r.vrcam_comp).load(Acquire);
        if comp != 0 && src_fov > 1.0 && ads > 0.0 {
            let want = if ads == 1.0 {
                src_fov
            } else {
                2.0 * ((src_fov * 0.5 * 0.017453292519943295).tan() / ads).atan()
                    * 57.29577951308232
            };
            if want > 1.0 && want < 175.0 {
                if !write(comp + 0x128, want as f32) {
                    return None;
                }
                set_f32(&CyberpunkVR_DebugVrcamWantFov, want as f32);
            }
        }
        if diagnostics() {
            CyberpunkVR_DebugForceCamHits.fetch_add(1, Relaxed);
        }
    }
    Some(())
}
/// SetStreamlineConstants (Detour_SlConstants, main's 0.2.4): under the engine's DLSS lock, the camera scope's own
/// state swapped in around the camera writer (scope_dlss.rs); the writer's own work otherwise.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_sl_constants(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) -> i64 {
    let spec = a2 as usize;
    let obj = crate::scope_dlss::scope_object(spec, *OPTICS_FEED, |s| sl_view_obj(s));
    crate::scope_dlss::sl_constants(spec, obj, || sl_constants_inner(a1, a2, a3))
}
unsafe fn sl_constants_inner(a1: *mut c_void, a2: *mut c_void, a3: *mut c_void) -> i64 {
    let a = a2 as usize;
    if a != 0 {
        let _ = pin_main(a);
    }
    if CyberpunkVR_StreamlineHistoryFix.load(Relaxed) != 0 && a != 0 {
        let _ = history_fix(a);
    }
    if CyberpunkVR_ForceVrcamCam.load(Relaxed) != 0 && a != 0 {
        let _ = force_vrcam_cam(a);
    }
    // DLSS for VRCAM: mark this camera-writer call as VRCAM's so the constants driver (called inside the original)
    // flips to VRCAM's own Streamline viewport.
    let previous = VRCAM_SL_ACTIVE.with(|c| c.get());
    if CyberpunkVR_VrcamDlss.load(Relaxed) != 0 && a != 0 {
        let vrcam = (|| -> Option<bool> {
            let ctx: usize = read(a + 0x18)?;
            Some(ctx != 0 && read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst))
        })()
        .unwrap_or(false);
        VRCAM_SL_ACTIVE.with(|c| c.set(vrcam));
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_stereo_dlss_originals[0].load(Acquire));
    let result = original(a1, a2, a3);
    VRCAM_SL_ACTIVE.with(|c| c.set(previous));
    result
}

// ---- the DLSS constants driver (sub_1407EEA54 -> slSetConstants)

/// The constants driver (Detour_DlssConst): under the engine's DLSS lock; whether it uploaded the scope's constants
/// for this frame noted after it.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_dlss_const(a1: *mut c_void, a2: u32) -> i64 {
    let _guard = crate::scope_dlss::Guard::new();
    let result = dlss_const_inner(a1, a2);
    crate::scope_dlss::after_constants(a1 as usize, a2);
    result
}
unsafe fn dlss_const_inner(a1: *mut c_void, a2: u32) -> i64 {
    let a = a1 as usize;
    let mut flipped = false;
    let mut saved = 0i32;
    if CyberpunkVR_VrcamDlss.load(Relaxed) != 0 && VRCAM_SL_ACTIVE.with(|c| c.get()) && a != 0 {
        let ok = (|| -> Option<()> {
            saved = read(a + DLSS_VP_OFF)?;
            if !write(a + DLSS_VP_OFF, CyberpunkVR_VrcamDlssViewport.load(Relaxed)) {
                return None;
            }
            flipped = true;
            // A/B: zero the jitter the const-setter is about to copy into sl::Constants.
            if CyberpunkVR_VrcamDlssZeroJitter.load(Relaxed) != 0
                && (!write(a + DLSS_JITTER_OFF, 0.0f32) || !write(a + DLSS_JITTER_OFF + 4, 0.0f32))
            {
                return None;
            }
            if diagnostics() {
                CyberpunkVR_DebugVrcamDlssConstHits.fetch_add(1, Relaxed);
            }
            Some(())
        })();
        if ok.is_none() {
            flipped = false;
        }
    }
    // Framegen also consumes native TAA: its eye identity must not depend on whether the Streamline viewport override
    // is enabled for DLSS.
    // The scope's constants are not a headset eye's camera.
    if !crate::scope_dlss::constants_active() {
        crate::framegen_inputs::cvr_framegen_record_native_camera(a1, (rt().node_view)());
    }
    crate::scope_dlss::before_constants(a);
    let original: unsafe extern "C" fn(*mut c_void, u32) -> i64 =
        core::mem::transmute(cvr_stereo_dlss_originals[1].load(Acquire));
    let result = original(a1, a2);
    if flipped {
        let _ = write(a + DLSS_VP_OFF, saved);
    }
    result
}

// ---- the DLSS tag and evaluate driver (sub_14228F210)

/// VRCAM's own copy of the driver's changed-detection cache (one shared DLSS state otherwise recreates the feature
/// every frame: white flicker).
static VRCAM_CACHE: Mutex<([u8; DLSS_CACHE_SZ], bool)> = Mutex::new(([0; DLSS_CACHE_SZ], false));
type EvalFn = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
);
/// The tag/eval driver (Detour_DlssEval): under the engine's DLSS lock, the camera scope evaluated with its own state
/// (scope_dlss.rs); every other view as before.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_stereo_dlss_eval(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
    a10: i32,
    a11: i32,
    a12: i32,
    a13: i32,
    a14: i32,
) {
    let guard = crate::scope_dlss::Guard::new();
    let obj = crate::scope_dlss::scope_object(a2 as usize, *OPTICS_FEED, |s| sl_view_obj(s));
    let scope = crate::scope_dlss::evaluate(&guard, a1 as usize, obj, || {
        let original: EvalFn = core::mem::transmute(cvr_stereo_dlss_originals[2].load(Acquire));
        original(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    });
    if !scope {
        dlss_eval_inner(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    }
}
#[allow(clippy::too_many_arguments)]
unsafe fn dlss_eval_inner(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: i32,
    a4: i32,
    a5: i32,
    a6: i32,
    a7: i32,
    a8: i32,
    a9: i32,
    a10: i32,
    a11: i32,
    a12: i32,
    a13: i32,
    a14: i32,
) {
    let (a, b) = (a1 as usize, a2 as usize);
    let mut flipped = false;
    let mut saved = 0i32;
    let mut main_cache = [0u8; DLSS_CACHE_SZ];
    let cache_addr = a + DLSS_CACHE_OFF;
    if CyberpunkVR_VrcamDlss.load(Relaxed) != 0 && a != 0 && b != 0 {
        let ok = (|| -> Option<()> {
            let ctx: usize = read(b + 0x18)?;
            if ctx != 0 && read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst) {
                saved = read(a + DLSS_VP_OFF)?;
                if !write(a + DLSS_VP_OFF, CyberpunkVR_VrcamDlssViewport.load(Relaxed)) {
                    return None;
                }
                // Swap VRCAM's own changed-detection cache in (stops the per-frame feature recreate).
                if !read_bytes(cache_addr, &mut main_cache) {
                    return None;
                }
                let cache = VRCAM_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                if cache.1 && !write(cache_addr, cache.0) {
                    return None;
                }
                flipped = true;
                if diagnostics() {
                    CyberpunkVR_DebugVrcamDlssEvalHits.fetch_add(1, Relaxed);
                }
            }
            Some(())
        })();
        if ok.is_none() {
            flipped = false;
        }
    }
    let original: EvalFn = core::mem::transmute(cvr_stereo_dlss_originals[2].load(Acquire));
    original(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    if flipped {
        // VRCAM's updated cache kept for the next frame, MAIN's restored.
        let mut cache = VRCAM_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if read_bytes(cache_addr, &mut cache.0) {
            cache.1 = true;
            if write(cache_addr, main_cache) {
                let _ = write(a + DLSS_VP_OFF, saved);
            }
        }
    }
    // POST-DLSS CROP FIX: VRCAM's evaluate just recorded on this command-list thread; the viewport and scissor hubs
    // see the thread as post-DLSS until the list's next reset.
    if flipped && CyberpunkVR_VrcamDlssScale.load(Relaxed) != 0 {
        (rt().set_dlss_post)(true);
    }
}

// ---- the ApplyDLSS node (sub_14077E4C8)

/// Whether MAIN's ApplyDLSS had the DLSS evaluate flag (the game actually upscaling): VRCAM only mirrors it then.
static MAIN_DLSS_FLAG: AtomicBool = AtomicBool::new(false);
/// MAIN is observed; VRCAM gets the flag set (and kept: its tonemap reads it later in the frame) only while MAIN has it
/// and VRCAM DLSS is on, and cleared otherwise. Never forces DLSS on.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_apply_dlss(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let b = a2 as usize;
    if b != 0 {
        let _ = (|| -> Option<()> {
            let ctx: usize = read(b + 0x18)?;
            if ctx == 0 {
                return Some(());
            }
            let key: u64 = read(ctx + 0x28)?;
            let q = ctx + DLSS_FLAGSET_OFF;
            if key == 0 {
                MAIN_DLSS_FLAG.store(read::<u64>(q)? & DLSS_EVAL_FLAG_BIT != 0, Relaxed);
            } else if key == g_vrcam_ctx_key.load(SeqCst) {
                let want = CyberpunkVR_VrcamDlss.load(Relaxed) != 0 && MAIN_DLSS_FLAG.load(Relaxed);
                let cur: u64 = read(q)?;
                if want && cur & DLSS_EVAL_FLAG_BIT == 0 {
                    write(q, cur | DLSS_EVAL_FLAG_BIT).then_some(())?;
                } else if !want && cur & DLSS_EVAL_FLAG_BIT != 0 {
                    write(q, cur & !DLSS_EVAL_FLAG_BIT).then_some(())?;
                }
                if diagnostics() {
                    CyberpunkVR_DebugVrcamApplyDlssHits.fetch_add(1, Relaxed);
                }
            }
            Some(())
        })();
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_stereo_dlss_originals[3].load(Acquire));
    original(a1, a2)
}

// ---- the render-resolution scaler (sub_140425054): a1 == &view[0x34]

/// VRCAM UPSCALES instead of running DLAA: the master DLSS flag set early (so the original scales and the pass
/// viewports take the dynamic-res path), build flag 65 cleared (the graph built like MAIN's), MAIN's group flags
/// matched (the compute resolve), and after the original VRCAM's render size is its target times MAIN's DLSS scale.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_render_res(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) -> i64 {
    // the camera scope's RR view sized natively (main's 0.2.4)
    if crate::scope_dlss::full_resolution_ready(a1 as usize, a2 as usize, a3 as usize, *OPTICS_FEED) {
        return crate::scope_dlss::render_full_resolution(a1 as usize, a3 as usize, || {
            let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64 =
                core::mem::transmute(cvr_stereo_dlss_originals[4].load(Acquire));
            original(a1, a2, a3)
        });
    }
    let a = a1 as usize;
    let mut vrcam = false;
    if CyberpunkVR_VrcamDlss.load(Relaxed) != 0 && a != 0 {
        let ok = (|| -> Option<()> {
            let view = a - 0x34;
            if read::<u64>(view + 0x28)? != g_vrcam_ctx_key.load(SeqCst) {
                return Some(());
            }
            vrcam = true;
            let flags: u64 = read(view + DLSS_FLAGSET_OFF)?;
            write(view + DLSS_FLAGSET_OFF, flags | DLSS_EVAL_FLAG_BIT).then_some(())?;
            if CyberpunkVR_VrcamClearFlag64.load(Relaxed) != 0 {
                let flags: u64 = read(view + DLSS_FLAGSET_OFF)?;
                write(view + DLSS_FLAGSET_OFF, flags & !BUILD_FLAG65_BIT).then_some(())?;
                if diagnostics() {
                    CyberpunkVR_DebugClearFlag64Hits.fetch_add(1, SeqCst);
                }
            }
            let mode = CyberpunkVR_VrcamComputeResolve.load(Relaxed);
            if mode != 0 {
                let p = view + 0x25B0;
                let before: u64 = read(p)?;
                let nv = match mode {
                    1 => before & !(1u64 << 20),
                    2 => (before & !(1u64 << 20)) | (1u64 << 25),
                    3 => before | (1u64 << 25),
                    _ => before,
                };
                write(p, nv).then_some(())?;
                CyberpunkVR_DebugP17D0Before.store(before, Relaxed);
                CyberpunkVR_DebugP17D0After.store(nv, Relaxed);
                if diagnostics() {
                    CyberpunkVR_DebugP17D0Hits.fetch_add(1, SeqCst);
                }
            }
            Some(())
        })();
        if ok.is_none() {
            vrcam = false;
        }
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_stereo_dlss_originals[4].load(Acquire));
    let result = original(a1, a2, a3);
    // What the engine chose, before the VRCAM rescale below.
    OnDLSSResCallback(a1);
    if vrcam && a != 0 {
        let r = rt();
        let _ = (|| -> Option<()> {
            let mut scale = 0.0f32;
            let renderer: usize = read(*r.exe_base + RENDERER_GLOBAL_RVA)?;
            if renderer != 0 {
                let dlss: usize = read(renderer + OFF_VIEWSTATE)?;
                if dlss != 0 {
                    scale = read(dlss + 0x400)?;
                }
            }
            let tw: i32 = read(a + 32)?;
            let th: i32 = read(a + 36)?;
            // Only while MAIN is actually upscaling (no DLAA, no insane values).
            if scale > 0.30 && scale < 0.999 && tw > 0 && th > 0 {
                // 2.5's writer clamps every dimension to at least 16.
                let rw = ((tw as f32 * scale + 0.5) as i32).max(16);
                let rh = ((th as f32 * scale + 0.5) as i32).max(16);
                for at in [0usize, 16, 20] {
                    write(a + at, rw).then_some(())?;
                }
                for at in [4usize, 24, 28] {
                    write(a + at, rh).then_some(())?;
                }
                // Reset the accumulator, as the engine's scaled branch does.
                write(a + 48, 0u64).then_some(())?;
                CyberpunkVR_DebugVrcamRenderW.store(rw as u32, Relaxed);
                CyberpunkVR_DebugVrcamRenderH.store(rh as u32, Relaxed);
                // VRCAM's render and output size for the DLSS constants' subrect and motion fix.
                core::ptr::write_volatile(r.vrcam_dlss_rw, rw);
                core::ptr::write_volatile(r.vrcam_dlss_rh, rh);
                core::ptr::write_volatile(r.vrcam_dlss_ow, tw);
                core::ptr::write_volatile(r.vrcam_dlss_oh, th);
                if diagnostics() {
                    CyberpunkVR_DebugVrcamResScaleHits.fetch_add(1, SeqCst);
                }
            }
            Some(())
        })();
    }
    result
}
