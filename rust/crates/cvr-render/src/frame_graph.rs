//! WHAT THE ENGINE'S FRAME GRAPH DECIDES FOR EACH VIEW, AND THE RTT VIEW IT DECIDES IT FOR (formerly
//! src/Stereo/FrameGraph.cpp): the graph is rebuilt (fully or incrementally) and for each view computes a set of
//! feature bits and upscaler groups -- a bit set for MAIN and clear for the second view means a pass simply does not
//! run there.
//!
//! The build observers (fg_observe: every view's feature words logged once; the HUD identification re-armed when the
//! full builder runs under a new key; the second view's and the scope's flags forced to MAIN's; MAIN's upscaler
//! groups latched), FlagCompute (MAIN's context and environment source; for the second view and the scope their
//! render rect, MAIN's environment handles through the engine's own refcounted assign, the water block, the feature
//! policy -- cascade and GI reuse per CyberpunkVR_VrcamFlagMode, the extraction bit, the scope's linear scene and its
//! one-eye bits --, MAIN's DLSS mirrored automatically), the rect computer's commit of the pending viewport, and the
//! water-state assign handed MAIN's block for the second view's.
//!
//! And the RTT component's view-create: the component the second eye renders into bound once at the selected
//! resolution, the braindance camera's pose and dirty flag, the output texture's resize, and the scope flag the
//! environment overrides key on.
//!
//! FrameGraph.cpp keeps the exports, the trampolines, the registry lines and the render-mask report, and hands this
//! file what it shares with the rest of the stereo module.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::cell::Cell;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use cvr_engine::code::{guarded_call4, read, write};
use std::sync::{LazyLock, Mutex};

use crate::stereo_profiler::{
    cvr_stereo_prof_now as prof_now, cvr_stereo_prof_timeline_add as timeline_add,
};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_vrcam_ctx_key: AtomicU64;
    static g_vrcam_view_w: AtomicU32;
    static g_vrcam_view_h: AtomicU32;
    static g_prof_build_main_ticks: AtomicI64;
    static g_prof_build_vrcam_ticks: AtomicI64;
    static g_prof_build_main_calls: AtomicU64;
    static g_prof_build_vrcam_calls: AtomicU64;
    static CyberpunkVR_ProfEnable: AtomicI32;
    static CyberpunkVR_NodeTimeline: AtomicI32;
    static CyberpunkVR_DebugFullTotal: AtomicU64;
    static CyberpunkVR_DebugIncrTotal: AtomicU64;
    static CyberpunkVR_OpticsSceneLinear: AtomicI32;
    static CyberpunkVR_OpticsNoScreenSpace: AtomicI32;
    static CyberpunkVR_DebugFgMainF0: AtomicU64;
    static CyberpunkVR_DebugFgMainF1: AtomicU64;
    static CyberpunkVR_DebugFgRttF0: AtomicU64;
    static CyberpunkVR_DebugFgRttF1: AtomicU64;
    static CyberpunkVR_DebugMainUpscalerGroups: AtomicU64;
    static CyberpunkVR_DebugUpscalerForceHits: AtomicU64;
    static CyberpunkVR_VrcamDlss: AtomicI32;
    static CyberpunkVR_VrcamFlagMode: AtomicU32;
    static CyberpunkVR_VrcamExtractionFix: AtomicU32;
    static CyberpunkVR_DebugVrcamExtractionHits: AtomicU64;
    static CyberpunkVR_DebugRttFlagForceHits: AtomicU64;
    static CyberpunkVR_DebugRttEnvBindHits: AtomicU64;
    static CyberpunkVR_DistantReuseMode: AtomicU32;
    static CyberpunkVR_ForceVrcamRes: AtomicU32;
    static CyberpunkVR_DebugForceResHits: AtomicU64;
    static CyberpunkVR_DebugMainW: AtomicU32;
    static CyberpunkVR_DebugMainH: AtomicU32;
    static CyberpunkVR_EnvExtraMask: AtomicU32;
    static CyberpunkVR_DebugEnvExtraBinds: AtomicU64;
    static CyberpunkVR_WaterMirror: AtomicU32;
    static CyberpunkVR_DebugWaterMirrors: AtomicU64;
    static CyberpunkVR_DebugWaterPtrSkips: AtomicU64;
    static CyberpunkVR_OpticsViewSetupHits: AtomicU64;
    static CyberpunkVR_OpticsViewExtractHits: AtomicU64;
    static CyberpunkVR_DebugRttRectHits: AtomicU64;
    static CyberpunkVR_WaterAssign: AtomicU32;
    static CyberpunkVR_DebugWaterAssigns: AtomicU64;
    static g_vrcam_sel_w: AtomicU32;
    static g_vrcam_sel_h: AtomicU32;
    static g_bdActive: AtomicI32;
    static g_bdScenePoseValid: AtomicI32;
    static g_headQuatValid: u32;
    static g_headQuatComposed: [f32; 4];
    static g_bdScenePosFP: [AtomicI32; 3];
    static CyberpunkVR_DebugRttCompRejects: AtomicU64;
    static CyberpunkVR_DebugVrcamBaseFov: AtomicU32;
    static CyberpunkVR_DebugRttComp: AtomicU64;
    static CyberpunkVR_BdCamDirty: AtomicI32;
    static CyberpunkVR_IpdInWorldPos: AtomicI32;
    static CyberpunkVR_MainIsRightEye: AtomicI32;
    static CyberpunkVR_DebugBdCamPose: AtomicU64;
    static CyberpunkVR_DebugBdCamDirty: AtomicU64;
    static CyberpunkVR_DebugRttW: AtomicU32;
    static CyberpunkVR_DebugRttH: AtomicU32;
    static CyberpunkVR_DebugRttHits: AtomicU64;
    static CyberpunkVR_RttResizeMatchMain: AtomicU32;
    static CyberpunkVR_RttResizeW: AtomicU32;
    static CyberpunkVR_RttResizeH: AtomicU32;
    static CyberpunkVR_DebugRttDtexW: AtomicU32;
    static CyberpunkVR_DebugRttDtexH: AtomicU32;
    static CyberpunkVR_DebugRttResizeHits: AtomicU64;
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

/// The trampolines MinHook writes : the full build, the incremental build, the rect compute, the water
/// state assign and the flag compute (render_parity runs it with MAIN's AA) (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_fg_originals: [core::sync::atomic::AtomicUsize; 5] = [const { core::sync::atomic::AtomicUsize::new(0) }; 5];
/// The RTT view create's trampoline (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_fg_rtt_view_create_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// What FrameGraph.cpp shares with these detours.
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    /// cvr::sharpen::gameUpscaler.SelectDlss.
    select_dlss: extern "C" fn(bool),
    /// g_main_view_ctx.
    main_view_ctx: *const AtomicUsize,
    /// &g_handle_assign: sub_1407CDAE4(dst, src), the engine's refcounted handle assign (set at install).
    handle_assign: *const usize,
    /// The stereo log (Utils/StereoLog.hpp log).
    stereo_log: extern "C" fn(*const c_char),
    /// g_vrcam_comp, g_vrcam_base_fov: the bound component and its authored fov (the camera writers read them).
    vrcam_comp: *const AtomicUsize,
    vrcam_base_fov: *mut f32,
    /// g_mirror_vrcam_serial: a fresh second-view frame for the mirror.
    mirror_vrcam_serial: *const AtomicU64,
    /// g_rtt_res_override, g_rtt_w, g_rtt_h: the view size override (off).
    rtt_res_override: *const bool,
    rtt_w: *const u32,
    rtt_h: *const u32,
}
extern "C" {
    /// GetDesiredHalfIpd's Rust half (cvr-rig's camera_math.rs).
    fn cvr_camera_desired_half_ipd() -> f32;
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    select_dlss: s::cvr_cpp_select_dlss,
    main_view_ctx: &raw const m::g_main_view_ctx,
    handle_assign: &raw const s::g_handle_assign,
    stereo_log: s::stereo_log,
    vrcam_comp: &raw const m::g_vrcam_comp,
    vrcam_base_fov: (&raw const m::g_vrcam_base_fov).cast_mut().cast(),
    mirror_vrcam_serial: &raw const s::g_mirror_vrcam_serial,
    rtt_res_override: (&raw const m::g_rtt_res_override).cast(),
    rtt_w: (&raw const m::g_rtt_w).cast(),
    rtt_h: (&raw const m::g_rtt_h).cast(),
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn main_view_ctx() -> usize {
    // SAFETY: a C++ global, attached.
    unsafe { (*rt().main_view_ctx).load(Acquire) }
}
fn vrcam_key() -> u64 {
    // SAFETY: an export of the C++ half.
    unsafe { g_vrcam_ctx_key.load(SeqCst) }
}
static OPTICS_FEED: LazyLock<u64> =
    LazyLock::new(|| crate::vrcam_config::cname_hash(b"vr_optics_feed"));

/// f0 bit 51 SET: the builder references the existing cascade atlas instead of rendering it (2.31: bit 50).
const SHADOW_CASCADE_REUSE_BIT: u64 = 1 << 51;
/// f0 bit 11 CLEAR: the distant-shadow node no-ops.
const DISTANT_SHADOW_BIT: u64 = 1 << 11;
/// f0 bit 31: Global Illumination's update (CRenderNode_GlobalIllumination tests it before sub_14077F758).
const GI_FEATURE_BIT: u64 = 1 << 31;
/// The scope view's screen-space passes: AO (15), SSR (21) and planar SSR (22).
const SCOPE_SCREEN_SPACE_BITS: u64 = (1 << 15) | (1 << 21) | (1 << 22);
/// Render features 98 (both eyes in one frame) and 99, f1 bits 34/35: the scope view is one eye.
const SCOPE_STEREO_BITS_F1: u64 = (1 << (98 - 64)) | (1 << (99 - 64));
/// Graph feature 84, the shading-rate image.
const SHADING_RATE_IMAGE_FLAG_F1: u64 = 1 << 20;
/// 2.5's MAIN flags until the real MAIN view is known.
const BOOTSTRAP_MAIN_FLAGS: [u64; 2] = [0x7800_017F_AD75_FF51, 0x0000_0000_A0BF_8010];
/// Group 70 (DLSS); the upscaler groups 70/72/73/74/76/78 (2.31: 69/71/72/73).
const FG_DLSS_BIT_F1: u64 = 1 << (70 - 64);
const FG_UPSCALER_MASK_F1: u64 = (1 << (70 - 64))
    | (1 << (72 - 64))
    | (1 << (73 - 64))
    | (1 << (74 - 64))
    | (1 << (76 - 64))
    | (1 << (78 - 64));
/// The three environment handle slots MAIN fills and the second view leaves zero.
const ENV_HANDLE_OFFS: [usize; 3] = [0x2490, 0x2B18, 0x2FD8];
/// The scanner grade's candidate slots, one bit each in CyberpunkVR_EnvExtraMask (default 0).
const ENV_EXTRA_OFFS: [usize; 9] = [
    0x1F0, 0x220, 0x380, 0x590, 0x5C0, 0x720, 0x930, 0x960, 0xAC0,
];
/// The water description, by group (CyberpunkVR_WaterMirror, measured negative, default 0): depth and level, the
/// volume's constants, the plane's rotation, the environment's sample point.
const WATER_MIRROR: [(usize, usize); 4] = [
    (0x25E0, 0x08),
    (0x25E8, 0x18),
    (0x2600, 0x30),
    (0x2630, 0x10),
];
const WATER_OFF: usize = 0x25E0;

/// Force the second view's and the scope's flags (always on: the second view's full build is one-shot and early).
static RTT_FORCE_FLAGS: AtomicBool = AtomicBool::new(true);
static FORCE_VIEW_FLAGS: AtomicBool = AtomicBool::new(true);
/// MAIN's context (key 0), the live environment source.
static MAIN_CTX: AtomicUsize = AtomicUsize::new(0);
/// The second view's and the scope's view objects, captured off the camera-name key inside FlagCompute.
static VRCAM_VIEW: AtomicUsize = AtomicUsize::new(0);
static OPTICS_VIEW: AtomicUsize = AtomicUsize::new(0);
/// MAIN's chosen upscaler groups (f1 & mask).
static MAIN_UPSCALER_GROUPS: AtomicU64 = AtomicU64::new(0);
/// MAIN's computed features, captured at the producer (MainViewFeatureCache): its context and the two words.
static MAIN_FEATURES: Mutex<(usize, [u64; 2])> = Mutex::new((0, [0; 2]));

fn main_features_for_vrcam() -> [u64; 2] {
    let known = main_view_ctx();
    let cached = *MAIN_FEATURES.lock().unwrap_or_else(|e| e.into_inner());
    if known != 0 && cached.0 == known {
        cached.1
    } else {
        BOOTSTRAP_MAIN_FLAGS
    }
}
/// MAIN's result is transient: captured where it is produced.
unsafe fn observe_main_features(context: usize, computed: usize) {
    let _ = (|| -> Option<()> {
        if context == 0 || computed == 0 {
            return None;
        }
        let name: u64 = read(context + 0x28)?;
        let flags: [u64; 2] = read(computed)?;
        let known = main_view_ctx();
        if context != known || name != 0 || flags[0] | flags[1] == 0 {
            return None;
        }
        *MAIN_FEATURES.lock().unwrap_or_else(|e| e.into_inner()) = (context, flags);
        let main = main_features_for_vrcam();
        CyberpunkVR_DebugFgMainF0.store(main[0], Relaxed);
        CyberpunkVR_DebugFgMainF1.store(main[1], Relaxed);
        Some(())
    })();
}
/// The shading-rate image off on a device whose native tile size is zero (issue #93: PrepareShadingRateImage divides
/// by it).
fn supported_hardware_flags(flags: u64) -> u64 {
    let base = unsafe { *rt().exe_base };
    let Some(pool) = read::<usize>(base + 0x3BC_DCA8) else {
        return flags;
    };
    if pool == 0 {
        return flags;
    }
    let Some(tile) = read::<u8>(pool + 0xB) else {
        return flags;
    };
    let filtered = if tile != 0 {
        flags
    } else {
        flags & !SHADING_RATE_IMAGE_FLAG_F1
    };
    if filtered != flags {
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if !LOGGED.swap(true, Relaxed) {
            stereo_log("[fgflags] VRCAM shading-rate image disabled: native tile size is zero");
        }
    }
    filtered
}
fn qword_is_pointer(q: u64) -> bool {
    (0x100_0000_0000..0x7FFF_FFFF_FFFF).contains(&q)
}

/// fg_observe's own state: the keys logged, the last full-build key and its re-arm debounce.
struct Observe {
    logged: Vec<u64>,
    last_full_key: Option<u64>,
    last_rearm_ms: u64,
}
static OBSERVE: Mutex<Observe> = Mutex::new(Observe {
    logged: Vec::new(),
    last_full_key: None,
    last_rearm_ms: 0,
});
fn observe_state() -> std::sync::MutexGuard<'static, Observe> {
    OBSERVE.lock().unwrap_or_else(|e| e.into_inner())
}
static VRCAM_FULL_LOGGED: AtomicBool = AtomicBool::new(false);
static VRCAM_INCR_LOGGED: AtomicBool = AtomicBool::new(false);

/// a4 = ctx + 6096, the two feature words the builder reads. Every view's flags logged once; the HUD re-armed on
/// every change of the full builder's key (debounced 250 ms); the second view's and the scope's flags forced to
/// MAIN's right before the builder reads them, with the upscaler selector's guarantee; MAIN's DLSS latched.
unsafe fn fg_observe(a4: usize, full: bool) {
    if a4 == 0 {
        return;
    }
    let r = rt();
    let which = if full { "full" } else { "incr" };
    let _ = (|| -> Option<()> {
        let ctx = a4 - 6096;
        let key: u64 = read(ctx + 0x28)?;
        let w: u32 = read(ctx + 0x44)?;
        let h: u32 = read(ctx + 0x48)?;
        let f0: u64 = read(a4)?;
        let f1: u64 = read(a4 + 8)?;
        let vrcam = vrcam_key();
        {
            let mut s = observe_state();
            if !s.logged.contains(&key) && s.logged.len() < 32 {
                s.logged.push(key);
                stereo_log(&format!(
                    "[fgflags] {which} key={key:016X} {w}x{h} f0={f0:016X} f1={f1:016X}{}",
                    if key == vrcam { " <-VRCAM" } else { "" }
                ));
            }
        }
        // RE-ARM ON EVERY TRANSITION, NOT ON FIRST SIGHTING (the map and the inventory rebuild the graph under a new
        // key, and the HUD identification latched onto a node of the old one).
        if full && key != *OPTICS_FEED {
            let mut s = observe_state();
            match s.last_full_key {
                None => s.last_full_key = Some(key),
                Some(last) if last != key => {
                    s.last_full_key = Some(key);
                    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
                    if now - s.last_rearm_ms >= 250 {
                        s.last_rearm_ms = now;
                        drop(s);
                        // SAFETY: the HUD's own re-arm (hud.rs).
                        unsafe { crate::hud::cvr_hud_rearm_for_new_graph(key) };
                    }
                }
                _ => {}
            }
        }
        if key == vrcam {
            if full {
                if !VRCAM_FULL_LOGGED.swap(true, Relaxed) {
                    stereo_log(&format!(
                        "[fgflags] VRCAM built via FULL (sub_141D43040) f0={f0:016X} f1={f1:016X}"
                    ));
                }
            } else if !VRCAM_INCR_LOGGED.swap(true, Relaxed) {
                stereo_log(&format!(
                    "[fgflags] VRCAM built via INCR (sub_14227BD10) f0={f0:016X} f1={f1:016X}"
                ));
            }
            // The earliest point the second view's ctx is in hand: its HUD capability, before anything reads it.
            // SAFETY: the HUD's own grant (hud.rs).
            unsafe { crate::hud::cvr_hud_grant_capability(ctx) };
        }
        let optics = key == *OPTICS_FEED;
        if key == vrcam || optics {
            let main = main_features_for_vrcam();
            if RTT_FORCE_FLAGS.load(Relaxed) && main[0] | main[1] != 0 {
                let mut n0 = f0 | main[0];
                let mut n1 = supported_hardware_flags(f1 | main[1]);
                if optics {
                    n0 = crate::fog_history::merge_view_flags(
                        n0 | SHADOW_CASCADE_REUSE_BIT,
                        f0,
                        true,
                    );
                    n1 = (n1 & !FG_UPSCALER_MASK_F1) | (f1 & FG_UPSCALER_MASK_F1);
                    n1 &= !SCOPE_STEREO_BITS_F1;
                }
                write(a4, n0).then_some(())?;
                write(a4 + 8, n1).then_some(())?;
                if !optics {
                    CyberpunkVR_DebugFgRttF0.store(n0, Relaxed);
                    CyberpunkVR_DebugFgRttF1.store(n1, Relaxed);
                    static FORCED0: AtomicU64 = AtomicU64::new(0);
                    if FORCED0.swap(n0, Relaxed) != n0 {
                        stereo_log(&format!(
                            "[fgflags] FORCED vrcam -> f0={n0:016X} f1={n1:016X} (was {f0:016X}/{f1:016X})"
                        ));
                    }
                }
            } else if !optics {
                CyberpunkVR_DebugFgRttF0.store(f0, Relaxed);
                CyberpunkVR_DebugFgRttF1.store(f1, Relaxed);
            }
            // The upscaler selector's guarantee: group 70 (DLSS) set and the others cleared, only when MAIN chose DLSS.
            if !optics
                && CyberpunkVR_VrcamDlss.load(Relaxed) != 0
                && MAIN_UPSCALER_GROUPS.load(Acquire) & FG_DLSS_BIT_F1 != 0
            {
                let current: u64 = read(a4 + 8)?;
                let nf1 = (current & !FG_UPSCALER_MASK_F1) | FG_DLSS_BIT_F1;
                if nf1 != current {
                    write(a4 + 8, nf1).then_some(())?;
                    CyberpunkVR_DebugFgRttF1.store(nf1, Relaxed);
                    diag(&CyberpunkVR_DebugUpscalerForceHits);
                }
            }
        } else if key == 0 && ctx == main_view_ctx() && f1 & FG_DLSS_BIT_F1 != 0 {
            // MAIN's chosen upscaler (the primary capture is FlagCompute's); only latched while DLSS is present.
            MAIN_UPSCALER_GROUPS.store(f1 & FG_UPSCALER_MASK_F1, Release);
            (r.select_dlss)(true);
            CyberpunkVR_DebugMainUpscalerGroups.store(f1 & FG_UPSCALER_MASK_F1, Relaxed);
        }
        if optics {
            let mut n0: u64 = read(a4)?;
            if CyberpunkVR_OpticsSceneLinear.load(Relaxed) != 0 {
                n0 &= !3;
                write(a4, n0).then_some(())?;
            }
            if CyberpunkVR_OpticsNoScreenSpace.load(Relaxed) != 0 {
                n0 &= !SCOPE_SCREEN_SPACE_BITS;
                write(a4, n0).then_some(())?;
            }
            let n1: u64 = read(a4 + 8)?;
            write(a4 + 8, n1 & !SCOPE_STEREO_BITS_F1).then_some(())?;
        }
        Some(())
    })();
}
/// A graph build's duration to MAIN or the second view (by the view key), for the profiler and the node timeline.
unsafe fn prof_add_build(a4: usize, t0: i64, t1: i64) {
    let vrcam = a4 != 0 && read::<u64>(a4 - 6096 + 0x28).is_some_and(|key| key == vrcam_key());
    if CyberpunkVR_NodeTimeline.load(Relaxed) != 0 {
        timeline_add(t0, t1, 0, if vrcam { 5 } else { 4 }, 0);
    }
    if CyberpunkVR_ProfEnable.load(Relaxed) == 0 {
        return;
    }
    if vrcam {
        g_prof_build_vrcam_ticks.fetch_add(t1 - t0, Relaxed);
        g_prof_build_vrcam_calls.fetch_add(1, Relaxed);
    } else {
        g_prof_build_main_ticks.fetch_add(t1 - t0, Relaxed);
        g_prof_build_main_calls.fetch_add(1, Relaxed);
    }
}
type BuildFn = unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64;
unsafe fn build(original: usize, a: [i64; 5], full: bool) -> i64 {
    let original: BuildFn = core::mem::transmute(original);
    fg_observe(a[3] as usize, full);
    if CyberpunkVR_ProfEnable.load(Relaxed) != 0 || CyberpunkVR_NodeTimeline.load(Relaxed) != 0 {
        let t0 = prof_now();
        let result = original(a[0], a[1], a[2], a[3], a[4]);
        prof_add_build(a[3] as usize, t0, prof_now());
        return result;
    }
    original(a[0], a[1], a[2], a[3], a[4])
}
/// The full builder (sub_141D43040(v3, v50, v50 + 112, ctx + 6096, v5)).
#[no_mangle]
pub unsafe extern "C" fn cvr_fg_full_build(a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) -> i64 {
    diag(&CyberpunkVR_DebugFullTotal);
    build(cvr_fg_originals[0].load(Acquire), [a1, a2, a3, a4, a5], true)
}
/// The incremental builder (sub_14227BD10, same arguments).
#[no_mangle]
pub unsafe extern "C" fn cvr_fg_incr_build(a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) -> i64 {
    diag(&CyberpunkVR_DebugIncrTotal);
    build(cvr_fg_originals[1].load(Acquire), [a1, a2, a3, a4, a5], false)
}

/// Eight bytes at a time from MAIN's water block, refusing any pair that reads as an address on either side.
unsafe fn water_mirror_apply(view: usize) {
    let mask = CyberpunkVR_WaterMirror.load(Relaxed);
    let main = MAIN_CTX.load(Acquire);
    if mask == 0 || view == 0 || main == 0 || view == main {
        return;
    }
    let _ = (|| -> Option<()> {
        for (k, &(off, len)) in WATER_MIRROR.iter().enumerate() {
            if mask & (1 << k) == 0 {
                continue;
            }
            for i in (0..len).step_by(8) {
                let sv: u64 = read(main + off + i)?;
                let dv: u64 = read(view + off + i)?;
                if qword_is_pointer(sv) || qword_is_pointer(dv) {
                    diag(&CyberpunkVR_DebugWaterPtrSkips);
                    continue;
                }
                if sv != dv {
                    write(view + off + i, sv).then_some(())?;
                    diag(&CyberpunkVR_DebugWaterMirrors);
                }
            }
        }
        Some(())
    })();
}

/// The rect FlagCompute leaves for the rect computer's commit (PendingVrcamViewport): the view's input at
/// view + 0x3030 and its size.
#[derive(Clone, Copy)]
struct Pending {
    input: usize,
    width: u32,
    height: u32,
}
const NO_PENDING: Pending = Pending {
    input: 0,
    width: 0,
    height: 0,
};
/// 2.5: +4247AC copies the rect computer's output into view + 0x14 (2.31: +4E4FCC); the earlier +4244E2 call only
/// computes dimensions and must not consume the override.
const COMMIT_CALLER_RVA: usize = 0x4247AC;
const INPUT_OFFSET: usize = 0x3030;
thread_local! {
    static VIEWPORT: Cell<Pending> = const { Cell::new(NO_PENDING) };
}
impl Pending {
    fn matches(&self, source: usize, caller: usize) -> bool {
        self.input != 0 && source == self.input && caller == COMMIT_CALLER_RVA
    }
}

/// MAIN's environment handles assigned onto a view (the three slots, then the opted-in extras).
unsafe fn assign_environment(
    view: usize,
    main: usize,
    extras: u32,
    count_bind: bool,
) -> Option<()> {
    let assign = *rt().handle_assign;
    if main == 0 || assign == 0 {
        return Some(());
    }
    let null = core::ptr::null_mut();
    let mut bound = false;
    for off in ENV_HANDLE_OFFS {
        if read::<usize>(main + off)? != 0 {
            guarded_call4(
                assign,
                (view + off) as *mut c_void,
                (main + off) as *mut c_void,
                null,
                null,
            )?;
            bound = true;
        }
    }
    if bound && count_bind {
        diag(&CyberpunkVR_DebugRttEnvBindHits);
    }
    for (k, off) in ENV_EXTRA_OFFS.iter().enumerate() {
        if extras & (1 << k) == 0 || read::<usize>(main + off)? == 0 {
            continue;
        }
        guarded_call4(
            assign,
            (view + off) as *mut c_void,
            (main + off) as *mut c_void,
            null,
            null,
        )?;
        if count_bind {
            diag(&CyberpunkVR_DebugEnvExtraBinds);
        }
    }
    Some(())
}

/// THE PER-FRAME FEATURE-FLAG WRITER (sub_14227E5D0: a1 renderer, a2 the output flags, a3 the view context, a4 the
/// view state; returns the output). The second view's and the scope's rect, environment and water before it; MAIN's
/// AA around it (RenderParity); after it, their flags given MAIN's with the reuse policy, the extraction bits and
/// MAIN's upscaler.
#[no_mangle]
pub unsafe extern "C" fn cvr_fg_flag_compute(a1: *mut c_void, a2: i64, a3: i64, a4: i64) -> i64 {
    let r = rt();
    VIEWPORT.set(NO_PENDING);
    let ctx = a3 as usize;
    let mut vrcam = false;
    if ctx != 0 && RTT_FORCE_FLAGS.load(Relaxed) {
        vrcam = (|| -> Option<bool> {
            let key: u64 = read(ctx + 0x28)?;
            if key == 0 {
                MAIN_CTX.store(ctx, Release);
                return Some(false);
            }
            if key != vrcam_key() {
                return Some(false);
            }
            let mut w: u32 = read(ctx + 0x44)?;
            // H at +0x4C ([W, W, H, H]); +0x48 is a W duplicate.
            let mut h: u32 = read(ctx + 0x4C)?;
            let main = MAIN_CTX.load(Acquire);
            if main != 0 && CyberpunkVR_ForceVrcamRes.load(Relaxed) != 0 {
                let mw: u32 = read(main + 0x44)?;
                let mh: u32 = read(main + 0x48)?;
                CyberpunkVR_DebugMainW.store(mw, Relaxed);
                CyberpunkVR_DebugMainH.store(mh, Relaxed);
                if mw != 0 && mh != 0 {
                    (w, h) = (mw, mh);
                    write(ctx + 0x44, mw).then_some(())?;
                    write(ctx + 0x48, mh).then_some(())?;
                    diag(&CyberpunkVR_DebugForceResHits);
                }
            }
            // Armed even on the first setup, before the native code has written render dimensions.
            VIEWPORT.set(Pending {
                input: ctx + INPUT_OFFSET,
                width: w,
                height: h,
            });
            if w != 0 && h != 0 {
                // A valid render rect before the original reads ctx + 0x14 for the lighting feature set.
                write(ctx + 0x14, [0u32, 0, w, h]).then_some(())?;
                g_vrcam_view_w.store(w, Release);
                g_vrcam_view_h.store(h, Release);
            }
            assign_environment(ctx, main, CyberpunkVR_EnvExtraMask.load(Relaxed), true)?;
            VRCAM_VIEW.store(ctx, Release);
            water_mirror_apply(ctx);
            Some(true)
        })()
        .unwrap_or(false);
    }
    let mut optics = false;
    if ctx != 0 {
        optics = (|| -> Option<bool> {
            if read::<u64>(ctx + 0x28)? != *OPTICS_FEED {
                return Some(false);
            }
            let w: u32 = read(ctx + 0x44)?;
            let h: u32 = read(ctx + 0x4C)?;
            // The same verified commit as the native RTT eye: an empty rect selects the reduced graph.
            VIEWPORT.set(Pending {
                input: ctx + INPUT_OFFSET,
                width: w,
                height: h,
            });
            if w != 0 && h != 0 && w <= 8192 && h <= 8192 {
                write(ctx + 0x14, [0i32, 0, w as i32, h as i32]).then_some(())?;
            }
            assign_environment(
                ctx,
                MAIN_CTX.load(Acquire),
                CyberpunkVR_EnvExtraMask.load(Relaxed),
                false,
            )?;
            OPTICS_VIEW.store(ctx, Release);
            water_mirror_apply(ctx);
            diag(&CyberpunkVR_OpticsViewSetupHits);
            Some(true)
        })()
        .unwrap_or(false);
    }
    let mut parity_ready = false;
    let res = crate::render_parity::compute_flags_with_main_aa(
        cvr_fg_originals[4].load(Acquire),
        a1,
        a2,
        a3,
        a4,
        &mut parity_ready,
    );
    if res != 0 {
        observe_main_features(ctx, res as usize);
    }
    // And again once the original has run: whether the block is filled before FlagCompute or inside it is not
    // measured, and writing it twice covers both orders.
    if vrcam || optics {
        water_mirror_apply(ctx);
    }
    let out = res as usize;
    if res != 0 && (vrcam || optics) && FORCE_VIEW_FLAGS.load(Relaxed) {
        let _ = (|| -> Option<()> {
            let [m0, m1] = main_features_for_vrcam();
            if m0 | m1 == 0 {
                return Some(());
            }
            let mut vf0 = match CyberpunkVR_VrcamFlagMode.load(Relaxed) {
                1 => m0,
                2 => m0 | SHADOW_CASCADE_REUSE_BIT,
                3 => m0 & !GI_FEATURE_BIT,
                _ => (m0 | SHADOW_CASCADE_REUSE_BIT) & !GI_FEATURE_BIT,
            };
            if CyberpunkVR_DistantReuseMode.load(Relaxed) == 0 {
                vf0 &= !DISTANT_SHADOW_BIT;
            }
            // The scope takes the second eye's scene policy, but its own AA and resolution-dependent resolve.
            let f0 = crate::fog_history::merge_view_flags(vf0, read(out)?, optics || parity_ready);
            write(out, f0).then_some(())?;
            let own_resolve = read::<u64>(out + 8)? & FG_UPSCALER_MASK_F1;
            let mut f1 = supported_hardware_flags(m1);
            if optics {
                f1 = (f1 & !FG_UPSCALER_MASK_F1) | own_resolve;
            }
            write(out + 8, f1).then_some(())?;
            if !optics {
                CyberpunkVR_DebugFgRttF0.store(f0, Relaxed);
                CyberpunkVR_DebugFgRttF1.store(f1, Relaxed);
            }
            diag(&CyberpunkVR_DebugRttFlagForceHits);
            Some(())
        })();
    }
    // The second view's final-colour extraction (build bit 64, f1 bit 0), applied last so it survives the above.
    if res != 0 && vrcam && CyberpunkVR_VrcamExtractionFix.load(Relaxed) != 0 {
        if let Some(f1) = read::<u64>(out + 8) {
            if write(out + 8, f1 | 1) {
                CyberpunkVR_DebugFgRttF1.store(f1 | 1, Relaxed);
                diag(&CyberpunkVR_DebugVrcamExtractionHits);
            }
        }
    }
    if res != 0 && optics {
        let _ = (|| -> Option<()> {
            // CopyToTexture needs ExtractionFinalColor; groups 79/80 (the post-processing the native RTT omits) are
            // MAIN's; the scene stays linear for the eye's tonemap; one eye.
            let mut f1: u64 = read::<u64>(out + 8)? | 1;
            write(out + 8, f1).then_some(())?;
            const POST_GROUPS: u64 = (1 << 15) | (1 << 16);
            f1 = (f1 & !POST_GROUPS) | (main_features_for_vrcam()[1] & POST_GROUPS);
            let mut f0: u64 = read(out)?;
            if CyberpunkVR_OpticsSceneLinear.load(Relaxed) != 0 {
                f0 &= !3;
            }
            if CyberpunkVR_OpticsNoScreenSpace.load(Relaxed) != 0 {
                f0 &= !SCOPE_SCREEN_SPACE_BITS;
            }
            f1 &= !SCOPE_STEREO_BITS_F1;
            write(out + 8, f1).then_some(())?;
            write(out, f0).then_some(())?;
            diag(&CyberpunkVR_OpticsViewExtractHits);
            Some(())
        })();
    }
    // THE UPSCALER SELECTOR: MAIN's groups captured (and the second view's DLSS decided from them, automatically);
    // the second view given DLSS so the full builder emits no native TAAU resolve.
    if res != 0 {
        let _ = (|| -> Option<()> {
            let key: u64 = if ctx != 0 { read(ctx + 0x28)? } else { !0 };
            let f1: u64 = read(out + 8)?;
            if key == 0 {
                MAIN_UPSCALER_GROUPS.store(f1 & FG_UPSCALER_MASK_F1, Release);
                (r.select_dlss)(f1 & FG_DLSS_BIT_F1 != 0);
                CyberpunkVR_DebugMainUpscalerGroups.store(f1 & FG_UPSCALER_MASK_F1, Relaxed);
                let want = (f1 & FG_DLSS_BIT_F1 != 0) as i32;
                if want != CyberpunkVR_VrcamDlss.load(Relaxed) {
                    CyberpunkVR_VrcamDlss.store(want, Relaxed);
                    stereo_log(&format!(
                        "[dlss] MAIN upscaler groups={:X} -> VRCAM DLSS {} (automatic)",
                        f1 & FG_UPSCALER_MASK_F1,
                        if want != 0 { "ON" } else { "off" }
                    ));
                }
            } else if key == vrcam_key()
                && CyberpunkVR_VrcamDlss.load(Relaxed) != 0
                && MAIN_UPSCALER_GROUPS.load(Acquire) & FG_DLSS_BIT_F1 != 0
            {
                let nf1 = (f1 & !FG_UPSCALER_MASK_F1) | FG_DLSS_BIT_F1;
                if nf1 != f1 {
                    write(out + 8, nf1).then_some(())?;
                    CyberpunkVR_DebugFgRttF1.store(nf1, Relaxed);
                    diag(&CyberpunkVR_DebugUpscalerForceHits);
                }
            }
            Some(())
        })();
    }
    res
}

/// The viewport-rect computer (sub_14206EF20), entered with its caller's return address in r9.
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_fg_rect_compute(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) -> i64 {
    core::arch::naked_asm!("mov r9, [rsp]", "jmp {body}", body = sym rect_compute);
}
/// For the view FlagCompute armed, at the commit into view + 0x14 only: the full render rect (0, 0, W, H), or on the
/// first setup -- sizes still zero -- the native destination bounds; an empty rect makes the engine skip the whole
/// lighting composite.
unsafe extern "C" fn rect_compute(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    return_address: usize,
) -> i64 {
    let r = rt();
    let caller = return_address.wrapping_sub(*r.exe_base);
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_fg_originals[2].load(Acquire));
    let res = original(a1, a2, a3);
    let pending = VIEWPORT.get();
    if res != 0 && pending.matches(a1 as usize, caller) {
        let applied = (|| -> Option<()> {
            let (w, h) = (pending.width, pending.height);
            let bounds: [i32; 4] = if (w == 0 || h == 0) && !a3.is_null() {
                read(a3 as usize)?
            } else {
                [0; 4]
            };
            VIEWPORT.set(NO_PENDING);
            if w > i32::MAX as u32 || h > i32::MAX as u32 {
                return Some(());
            }
            let rect = if w != 0 && h != 0 {
                [0, 0, w as i32, h as i32]
            } else if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
                return Some(());
            } else {
                bounds
            };
            write(res as usize, rect).then_some(())?;
            diag(&CyberpunkVR_DebugRttRectHits);
            Some(())
        })();
        if applied.is_none() {
            VIEWPORT.set(NO_PENDING);
        }
    }
    res
}

/// The water state's assign (sub_14067B380): for exactly the second view's or the scope's own water struct, MAIN's
/// is the source -- through the engine's own operator, so the two refcounted handles at +0x60/+0x68 travel correctly.
#[no_mangle]
pub unsafe extern "C" fn cvr_fg_water_assign(dst: *mut c_void, src: *mut c_void) -> *mut c_void {
    let mut src = src;
    if CyberpunkVR_WaterAssign.load(Relaxed) != 0 {
        let (vv, ov, mc) = (
            VRCAM_VIEW.load(Acquire),
            OPTICS_VIEW.load(Acquire),
            MAIN_CTX.load(Acquire),
        );
        let address = dst as usize;
        if mc != 0
            && ((vv != 0 && vv != mc && address == vv + WATER_OFF)
                || (ov != 0 && ov != mc && address == ov + WATER_OFF))
        {
            src = (mc + WATER_OFF) as *mut c_void;
            diag(&CyberpunkVR_DebugWaterAssigns);
        }
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        core::mem::transmute(cvr_fg_originals[3].load(Acquire));
    original(dst, src)
}
/// MAIN's context as FlagCompute last saw it (the RTT resize and the render-mask report read it).
#[no_mangle]
pub extern "C" fn cvr_fg_main_ctx() -> usize {
    MAIN_CTX.load(Acquire)
}
/// The second view's view object (the scope camera's readiness reads it).
#[no_mangle]
pub extern "C" fn cvr_fg_vrcam_view() -> usize {
    VRCAM_VIEW.load(Acquire)
}
/// PhysicalOpticsStereoReady (formerly Stereo/FrameGraph.cpp's, for the rig's provider natives): the auxiliary camera
/// may start once the right eye has produced a fresh frame -- MAIN's view and the second view's distinct, keyed as the
/// main camera and the VRCAM feed. Under engine stereo, once the engine renders both eyes (not a single picture).
#[no_mangle]
pub extern "C" fn cvr_cpp_physical_optics_stereo_ready() -> bool {
    if crate::engine_stereo::cvr_engine_stereo_enabled() {
        return crate::engine_stereo::cvr_engine_stereo_rendering() && !crate::eye_windows::one_picture();
    }
    let tick = m::g_stable_tick.load(Acquire);
    // SAFETY: a plain clock read.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    if tick == 0 || now < tick || now - tick > 500 {
        return false;
    }
    let main = m::g_main_view_ctx.load(Acquire);
    let right = cvr_fg_vrcam_view();
    let key = m::g_vrcam_ctx_key.load(Acquire);
    if main == 0 || right == 0 || main == right || key == 0 {
        return false;
    }
    read::<u64>(main + 0x28) == Some(0) && read::<u64>(right + 0x28) == Some(key)
}
/// SetPhysicalOpticsPath (formerly Stereo/DeviceHooks.cpp's): the scope's optics path for the PSO twins.
#[no_mangle]
pub extern "C" fn cvr_cpp_set_physical_optics_path(path: i32) {
    // SAFETY: the export's plain store, as the C++ wrote it.
    unsafe { crate::used_moved_exports::CyberpunkVR_OpticsPath = path };
}

// ---- the RTT component's view-create: the component the second eye renders into, bound and kept

/// entRenderToTextureCameraComponent's render-host vtable.
const RTT_HOST_VTABLE_RVA: usize = 0x36116E8;
/// sub_142E8C310, the texture manager's thread-marshaled ResizeDynamicTexture(texMgr, &dtex, w, h, flag).
const RESIZE_DYNTEX_RVA: usize = 0x2E8C310;
const RENDERER_GLOBAL_RVA: usize = 0x3BBCAE8;

thread_local! {
    /// Inside our own RTT preparer's call (the second view's or the scope's), on this thread.
    static RTT_PREPARE_OURS: Cell<bool> = const { Cell::new(false) };
}
/// t_rtt_prepare_ours: the view-data copy runs the environment overrides only inside this scope.
#[no_mangle]
pub extern "C" fn cvr_fg_rtt_prepare_ours() -> bool {
    RTT_PREPARE_OURS.get()
}
/// The original, with the scope flag raised when this preparer call is for our view -- identified by the camera-name
/// hash at a1 + 0x260 and by nothing else. Every return path of the detour goes through here.
unsafe fn rtt_call_orig(a1: i64, a2: i64) -> i64 {
    let ours = a1 != 0
        && read::<u64>(a1 as usize + 0x260)
            .is_some_and(|key| key == g_vrcam_ctx_key.load(Relaxed) || key == *OPTICS_FEED);
    let previous = RTT_PREPARE_OURS.replace(ours);
    let original: unsafe extern "C" fn(i64, i64) -> u8 =
        core::mem::transmute(cvr_fg_rtt_view_create_original.load(Relaxed));
    let result = original(a1, a2);
    RTT_PREPARE_OURS.set(previous);
    result as i8 as i64
}
/// q's right vector (CameraMath's ComputeRightVectorFromQuaternion).
fn right_from_quaternion(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q;
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
    ]
}
fn plausible_unit_vector(v: [f32; 3]) -> bool {
    if !v.iter().all(|c| c.is_finite()) {
        return false;
    }
    let len_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    len_sq > 0.25 && len_sq < 4.0
}
/// The output DynamicTexture (*(comp + 0x1E8)) resized to the target -- CyberpunkVR_RttResizeW/H, else MAIN's render
/// size -- through the engine's marshaled resize, and its own CPU size and scaleToViewport aligned (default off).
unsafe fn maybe_resize_rtt(comp: usize) {
    if CyberpunkVR_RttResizeMatchMain.load(Relaxed) == 0 || comp == 0 {
        return;
    }
    let _ = (|| -> Option<()> {
        let dtex: usize = read(comp + 0x1E8)?;
        if dtex == 0 {
            return Some(());
        }
        let cw: u32 = read(dtex + 0x40)?;
        let ch: u32 = read(dtex + 0x44)?;
        CyberpunkVR_DebugRttDtexW.store(cw, Relaxed);
        CyberpunkVR_DebugRttDtexH.store(ch, Relaxed);
        let (mut tw, mut th) = (
            CyberpunkVR_RttResizeW.load(Relaxed),
            CyberpunkVR_RttResizeH.load(Relaxed),
        );
        if tw == 0 || th == 0 {
            let main = MAIN_CTX.load(Acquire);
            if main == 0 {
                return Some(());
            }
            tw = read(main + 0x44)?;
            th = read(main + 0x4C)?;
        }
        if tw == 0 || th == 0 || tw > 8192 || th > 8192 || (cw == tw && ch == th) {
            return Some(());
        }
        let base = *rt().exe_base;
        let renderer: usize = read(base + RENDERER_GLOBAL_RVA)?;
        let tex_mgr: usize = if renderer != 0 {
            read(renderer + 0x70)?
        } else {
            0
        };
        if tex_mgr == 0 {
            return Some(());
        }
        // The proven-safe pattern: a pointer to a local holder.
        let mut holder = dtex;
        let resize: unsafe extern "C" fn(usize, *mut usize, u32, u32, i32) -> u8 =
            core::mem::transmute(base + RESIZE_DYNTEX_RVA);
        resize(tex_mgr, &mut holder, tw, th, 1);
        write(dtex + 0x40, tw).then_some(())?;
        write(dtex + 0x44, th).then_some(())?;
        write(dtex + 0x48, 0u32).then_some(())?;
        diag(&CyberpunkVR_DebugRttResizeHits);
        Some(())
    })();
}
/// sub_14048FAF4(component, a2), the RTT view-create. For our render host's components: bind ONE component -- the
/// one at the selected resolution -- and keep it (re-bound when the same resolution comes back as a new object),
/// its authored fov captured before anything of ours writes it; in a braindance mark the camera dirty with this
/// frame's composed pose; the output texture resized to target (off by default); the size override (off).
#[no_mangle]
pub unsafe extern "C" fn cvr_fg_rtt_view_create(a1: i64, a2: i64) -> i64 {
    let r = rt();
    let comp = a1 as usize;
    if comp != 0 {
        let _ = (|| -> Option<()> {
            if read::<usize>(comp)? != *r.exe_base + RTT_HOST_VTABLE_RVA {
                return Some(());
            }
            let w: u32 = read(comp + 0x258)?;
            let h: u32 = read(comp + 0x25C)?;
            let cached = (*r.vrcam_comp).load(Relaxed);
            let (sel_w, sel_h) = (g_vrcam_sel_w.load(Relaxed), g_vrcam_sel_h.load(Relaxed));
            let dims_match = sel_w == 0 || sel_h == 0 || (w == sel_w && h == sel_h);
            if cached != 0 && comp != cached {
                if !dims_match {
                    diag(&CyberpunkVR_DebugRttCompRejects);
                    return Some(());
                }
                // The same selected resolution as a different object: destroyed and re-created.
                (*r.vrcam_comp).store(comp, Release);
                let fov: f32 = read(comp + 0x128)?;
                *r.vrcam_base_fov = fov;
                CyberpunkVR_DebugVrcamBaseFov.store(fov.to_bits(), Relaxed);
                stereo_log(&format!(
                    "[rtt] re-bound vrcam component {cached:016X} -> {comp:016X} ({w}x{h})"
                ));
            } else if cached == 0 {
                if !dims_match {
                    if diagnostics()
                        && CyberpunkVR_DebugRttCompRejects.fetch_add(1, Relaxed) % 600 == 0
                    {
                        stereo_log(&format!("[rtt] ignoring component {comp:016X} {w}x{h} (selected {sel_w}x{sel_h})"));
                    }
                    return Some(());
                }
                (*r.vrcam_comp).store(comp, Release);
                let fov: f32 = read(comp + 0x128)?;
                *r.vrcam_base_fov = fov;
                CyberpunkVR_DebugVrcamBaseFov.store(fov.to_bits(), Relaxed);
                stereo_log(&format!(
                    "[rtt] bound vrcam component {comp:016X} {w}x{h} fov={fov:.3}"
                ));
            }
            CyberpunkVR_DebugRttComp.store(comp as u64, Relaxed);
            // The driver tests the dirty flag at the top of its next tick, so a store made here is consumed on the
            // following frame; the pose is the one LocateCamera composed for this frame (VrCore.cpp's
            // CyberpunkVR_BdCamDirty note).
            let dirty = CyberpunkVR_BdCamDirty.load(Relaxed);
            if dirty != 0 && g_bdActive.load(Relaxed) != 0 && g_bdScenePoseValid.load(Acquire) != 0
            {
                let _ = (|| -> Option<()> {
                    if dirty & 2 != 0 && core::ptr::read_volatile(&raw const g_headQuatValid) != 0 {
                        let hq: [f32; 4] = core::ptr::read_volatile(&raw const g_headQuatComposed);
                        write(comp + 0xF0, hq).then_some(())?;
                        if CyberpunkVR_IpdInWorldPos.load(Relaxed) != 0 {
                            let right = right_from_quaternion(hq);
                            if plausible_unit_vector(right) {
                                // SAFETY: a plain getter.
                                let half = unsafe { cvr_camera_desired_half_ipd() };
                                let sign = if CyberpunkVR_MainIsRightEye.load(Relaxed) != 0 {
                                    -1.0f32
                                } else {
                                    1.0
                                };
                                let p: [i32; 3] = core::array::from_fn(|i| {
                                    g_bdScenePosFP[i]
                                        .load(Relaxed)
                                        .wrapping_add((right[i] * half * sign * 131072.0) as i32)
                                });
                                write(comp + 0xE0, p).then_some(())?;
                            }
                        }
                        diag(&CyberpunkVR_DebugBdCamPose);
                    }
                    if dirty & 1 != 0 {
                        write(comp + 0xA00, 1i32).then_some(())?;
                        diag(&CyberpunkVR_DebugBdCamDirty);
                    }
                    Some(())
                })();
            }
            CyberpunkVR_DebugRttW.store(w, Relaxed);
            CyberpunkVR_DebugRttH.store(h, Relaxed);
            (*r.mirror_vrcam_serial).fetch_add(1, Release);
            maybe_resize_rtt(comp);
            if diagnostics() {
                let hits = CyberpunkVR_DebugRttHits.fetch_add(1, Relaxed);
                if hits % 300 == 0 {
                    stereo_log(&format!(
                        "[rtt] view-create comp={comp:016X} dims={w}x{h} hits={}",
                        hits + 1
                    ));
                }
            }
            if *r.rtt_res_override {
                write(comp + 0x258, *r.rtt_w).then_some(())?;
                write(comp + 0x25C, *r.rtt_h).then_some(())?;
            }
            Some(())
        })();
    }
    rtt_call_orig(a1, a2)
}
