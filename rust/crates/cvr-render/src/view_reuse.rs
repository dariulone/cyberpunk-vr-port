//! THE SECOND VIEW REUSES INSTEAD OF RECOMPUTING (the producer-skip detours of formerly src/Stereo/ViewReuse.cpp):
//! the sky build, the distant shadows (render and prepare), the reflection-probe atlas, the local shadow maps,
//! global illumination and the foliage wind (the impulse volume's claim, the SpeedTree wind latch); and the
//! environment overrides (the scanner's tint) run for our own view's source without erasing MAIN's entries.
//! Each is, to a very good approximation, the same for two eyes 65 mm apart, so for the shared
//! world views -- the port's VRCAM and the optics feed -- the producer is skipped (its feature bit stays set:
//! downstream consumers still run and read what MAIN produced), or for GI only the apply runs.
//!
//! Each switch is a live export (CyberpunkVR_*ReuseMode); the measured history of every one is in ViewReuse.cpp at
//! its export. Engine memory is read, and the GI functions called, through the SEH adapter. ViewReuse.cpp (and
//! SyncStereo.cpp for the sky) keeps the exports, the trampolines and the registry lines.
//!
//! And the shadow-cascade nodes: the cascade index published for the census and the cascade camera lend, the
//! cascade placement lent from the view that runs first to the one that runs second, their probes, and the mono
//! cascades' node skip and scope. And the volumetric clouds' constants (the noise-layer offsets the engine only
//! advances for MAIN) with the view-data mirror: MAIN's view block captured at the cloud fill, its holes, fog block
//! and atmosphere copied into the second view's and the scope's at node dispatch, and the diffs that named them.
//! And the sky-scattering node's probes ([viewrect] its three gates per view, [envptr] the resource slots).
//! And the composition lend for the second view's HUD layers (the set learned at the assign, MAIN's object held
//! with a count, the gate written per node or scoped to one call, the element guards) with the light-volume
//! pass's block-list lend -- installed only when a second view is wanted.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::stereo_shared as s;
use core::ffi::c_void;
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use cvr_engine::code::{guarded_call3, guarded_call4, read, read_bytes, write, write_bytes};
use std::sync::LazyLock;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_vrcam_ctx_key: AtomicU64;
    static CyberpunkVR_SkyReuseMode: AtomicU32;
    static CyberpunkVR_DebugSkySkipHits: AtomicU64;
    static CyberpunkVR_DebugSkyMainHits: AtomicU64;
    static CyberpunkVR_DistantReuseMode: AtomicU32;
    static CyberpunkVR_DebugDistantSkipHits: AtomicU64;
    static CyberpunkVR_ProbeReuseMode: AtomicU32;
    static CyberpunkVR_DebugProbeSkipHits: AtomicU64;
    static CyberpunkVR_DebugProbeNodeEntries: AtomicU64;
    static CyberpunkVR_LocalShadowReuseMode: AtomicU32;
    static CyberpunkVR_DebugLocalShadowSkipHits: AtomicU64;
    static CyberpunkVR_GiReuseMode: AtomicU32;
    static CyberpunkVR_DebugGiSkipHits: AtomicU64;
    static CyberpunkVR_FoliageWindMode: AtomicI32;
    static CyberpunkVR_DebugWindClaimMain: AtomicU64;
    static CyberpunkVR_DebugWindDenyVrcam: AtomicU64;
    static CyberpunkVR_SpeedTreeWindMode: AtomicI32;
    static CyberpunkVR_DebugStWindFirst: AtomicU64;
    static CyberpunkVR_DebugStWindExtraMain: AtomicU64;
    static CyberpunkVR_DebugStWindExtraVrcam: AtomicU64;
    static CyberpunkVR_RunViewParams: AtomicU32;
    static CyberpunkVR_DebugViewParamsRuns: AtomicU64;
    static CyberpunkVR_DebugViewParamsSeen: AtomicU64;
    static CyberpunkVR_DebugViewParamsEntries: AtomicU64;
    static CyberpunkVR_DebugViewParamsBusy: AtomicU64;
    static CyberpunkVR_DebugViewParamsWalked: AtomicU64;
    static CyberpunkVR_DebugViewParamsSkipNoObj: AtomicU64;
    static CyberpunkVR_DebugViewParamsSkipErase: AtomicU64;
    static CyberpunkVR_CascCountProbe: AtomicI32;
    static CyberpunkVR_CascadeBiasLend: AtomicI32;
    static CyberpunkVR_DebugCascBiasLends: AtomicU64;
    static CyberpunkVR_CascRecLend: AtomicI32;
    static CyberpunkVR_DebugCascRecLends: AtomicU64;
    static CyberpunkVR_CascFitProbe: AtomicI32;
    static CyberpunkVR_CascadeSkipMain: AtomicI32;
    static CyberpunkVR_DebugCascadeMainSkips: AtomicU64;
    static CyberpunkVR_CmdListHunt: AtomicI32;
    static CyberpunkVR_DebugCmdListOffset: AtomicU32;
    static CyberpunkVR_OpticsCascadeDraws: AtomicU64;
    static CyberpunkVR_OpticsCascadeClears: AtomicU64;
    static CyberpunkVR_CloudCbMode: AtomicU32;
    static CyberpunkVR_DebugCloudCbMain: AtomicU64;
    static CyberpunkVR_DebugCloudCbVrcam: AtomicU64;
    static CyberpunkVR_CloudCbExtra: AtomicU32;
    static CyberpunkVR_DebugCloudCbExtra: AtomicU64;
    static CyberpunkVR_ViewDataDiff: AtomicI32;
    static CyberpunkVR_ViewDataFixMask: AtomicU32;
    static CyberpunkVR_DebugViewDataFixes: AtomicU64;
    static CyberpunkVR_FogMirrorMask: AtomicU32;
    static CyberpunkVR_DebugFogMirrors: AtomicU64;
    static CyberpunkVR_EnvMirrorMask: AtomicU32;
    static CyberpunkVR_DebugEnvMirrors: AtomicU64;
    static CyberpunkVR_DebugEnvMirrorPtrSkips: AtomicU64;
    static CyberpunkVR_ViewDataBatchSnapshot: AtomicU32;
    static CyberpunkVR_OpticsVdFillGate: AtomicI32;
    static CyberpunkVR_OpticsVdFills: AtomicU64;
    static CyberpunkVR_OpticsVdSkips: AtomicU64;
    static CyberpunkVR_OpticsVdVerifies: AtomicU64;
    static CyberpunkVR_OpticsVdStale: AtomicU64;
    static CyberpunkVR_ViewRectProbe: AtomicI32;
    static CyberpunkVR_DebugViewRectReads: AtomicU64;
    static CyberpunkVR_EnvPtrProbe: AtomicI32;
    static CyberpunkVR_LightBlockLend: AtomicU32;
    static CyberpunkVR_DebugLightBlockLends: AtomicU64;
    static CyberpunkVR_CompLendScoped: AtomicU32;
    static CyberpunkVR_DebugCompScopedSet: AtomicU64;
    static CyberpunkVR_DebugCompScopedLends: AtomicU64;
    static CyberpunkVR_DebugCompScopedSkips: AtomicU64;
    static CyberpunkVR_CompLendFrameHold: AtomicU32;
    static CyberpunkVR_CompLendFrameStats: [AtomicU64; 9];
    static CyberpunkVR_CompLendFrameLatch: AtomicU32;
    static CyberpunkVR_CompLendSet: AtomicU32;
    static CyberpunkVR_DebugCompLendWrites: AtomicU64;
    static CyberpunkVR_DebugCompLendSkips: AtomicU64;
    static CyberpunkVR_DebugCompElemSkips: AtomicU64;
    static CyberpunkVR_DebugCompElemNulls: AtomicU64;
    static g_vrcam_sel_w: AtomicU32;
    static g_vrcam_sel_h: AtomicU32;
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}

const RENDERER_GLOBAL_RVA: usize = 0x3BBCAE8;
/// sub_14033AA6C(ctx + 0x14): the GI node's early-out.
const GI_EARLYCHK_RVA: usize = 0x33AA6C;
/// sub_1407F4230(applyMgr, a2): the GI apply.
const GI_APPLY_RVA: usize = 0x7F4230;

/// The trampolines MinHook writes, registered from engine_detours.rs (in this order): the composition's state assign,
/// element draw keys and element rect, RenderLightBuffers, the cloud CB, sky scattering, the distant prepare and
/// render, the local shadows, the reflection probes, the SpeedTree and impulse winds, the per-view parameters, the
/// viewData copy, the cascade clear and node (the sky work's and the GI node's stay the .cpp's: SyncStereo.cpp shares
/// them).
#[no_mangle]
pub static cvr_reuse_originals: [core::sync::atomic::AtomicUsize; 16] = [const { core::sync::atomic::AtomicUsize::new(0) }; 16];

/// The sky work's trampoline (written by engine_hooks.rs's install).
#[no_mangle]
pub static cvr_sky_work_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The GI node's trampoline (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_gi_node_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// What ViewReuse.cpp and SyncStereo.cpp share with these detours.
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    /// The view-data write epoch the hole filler keys on.
    vd_write_epoch: *const AtomicU64,
    /// The stereo log (Utils/StereoLog.hpp log: the switch and the rate limit, "[stereo] " in front).
    stereo_log: extern "C" fn(*const core::ffi::c_char),
    /// g_main_block_v5: MAIN's draw-block list for the light-volume lend (nothing writes it any more).
    main_block_v5: *const *mut c_void,
}
/// The view-data mirror's write epoch (formerly ViewReuse.cpp's file static g_vd_write_epoch).
static VD_WRITE_EPOCH: AtomicU64 = AtomicU64::new(0);
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    vd_write_epoch: &raw const VD_WRITE_EPOCH,
    stereo_log: s::stereo_log,
    main_block_v5: &raw const s::g_main_block_v5,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}

static OPTICS_FEED: LazyLock<u64> =
    LazyLock::new(|| crate::vrcam_config::cname_hash(b"vr_optics_feed"));
/// The shared world views: the port's VRCAM, and the optics feed.
fn shared_world_view(key: u64) -> bool {
    // SAFETY: an export of the C++ half.
    let vrcam = unsafe { g_vrcam_ctx_key.load(SeqCst) };
    (key != 0 && key == vrcam) || key == *OPTICS_FEED
}
/// The node's view: ctx = *(a2 + 0x18), its key at ctx + 0x28; false on a fault.
fn node_view_shared(a2: *mut c_void) -> bool {
    if a2.is_null() {
        return false;
    }
    (|| -> Option<bool> {
        let ctx: usize = read(a2 as usize + 0x18)?;
        Some(ctx != 0 && shared_world_view(read::<u64>(ctx + 0x28)?))
    })()
    .unwrap_or(false)
}

/// The sky work (sub_1407F8714): MAIN owns the shared sky build; the second view's duplicate producer is skipped
/// (CyberpunkVR_SkyReuseMode 1).
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_sky_work(a1: *mut c_void, a2: *mut c_void) {
    if node_view_shared(a2) {
        if CyberpunkVR_SkyReuseMode.load(Relaxed) == 1 {
            if diagnostics() {
                CyberpunkVR_DebugSkySkipHits.fetch_add(1, SeqCst);
            }
            return;
        }
    } else if diagnostics() {
        CyberpunkVR_DebugSkyMainHits.fetch_add(1, SeqCst);
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        core::mem::transmute(cvr_sky_work_original.load(core::sync::atomic::Ordering::Acquire));
    original(a1, a2);
}

/// Distant shadows: skipped for the shared views only while distant reuse is on (mode 1); mode 0 leaves them to the
/// engine's own no-op.
fn distant_is_shared(a2: *mut c_void) -> bool {
    // SAFETY: an export of the C++ half.
    let on = unsafe { CyberpunkVR_DistantReuseMode.load(Relaxed) == 1 };
    on && node_view_shared(a2)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_distant_render(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) {
    if distant_is_shared(a2) {
        if diagnostics() {
            CyberpunkVR_DebugDistantSkipHits.fetch_add(1, Relaxed);
        }
        return;
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        core::mem::transmute(cvr_reuse_originals[7].load(Acquire));
    original(a1, a2, a3);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_distant_prepare(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) {
    if distant_is_shared(a2) {
        if diagnostics() {
            CyberpunkVR_DebugDistantSkipHits.fetch_add(1, Relaxed);
        }
        return;
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        core::mem::transmute(cvr_reuse_originals[6].load(Acquire));
    original(a1, a2, a3);
}

/// The reflection-probe atlas (sub_140DAC960). MEASURED HARMFUL AT 1 and kept at 0: the second view is the one that
/// fills the shared atlas. One line the first time through (entries at zero: the detour is not on the path; entries
/// with match=0: the view context is not at *(a2+0x18)), and every 5 s while the switch is armed.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_reflection_probes(a1: *mut c_void, a2: *mut c_void) -> i64 {
    if diagnostics() {
        CyberpunkVR_DebugProbeNodeEntries.fetch_add(1, SeqCst);
    }
    static SAID: AtomicI32 = AtomicI32::new(0);
    if SAID.swap(1, SeqCst) == 0 {
        let (mut ctx, mut key) = (0usize, 0u64);
        if !a2.is_null() {
            if let Some(c) = read::<usize>(a2 as usize + 0x18) {
                ctx = c;
                if ctx != 0 {
                    key = read::<u64>(ctx + 0x28).unwrap_or(0);
                }
            }
        }
        let vrcam = g_vrcam_ctx_key.load(SeqCst);
        stereo_log(&format!(
            "[probes] first entry: a2={:016X} ctx=*(a2+0x18)={ctx:016X} key={key:X} vrcamKey={vrcam:X} match={} mode={}",
            a2 as usize,
            (ctx != 0 && key == vrcam) as i32,
            CyberpunkVR_ProbeReuseMode.load(Relaxed)
        ));
    }
    // The repeating half: only while the switch is armed.
    let mode = CyberpunkVR_ProbeReuseMode.load(Relaxed);
    if mode != 0 {
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
        let last = LAST.load(Relaxed);
        if last == 0 || now - last >= 5000 {
            LAST.store(now, Relaxed);
            stereo_log(&format!(
                "[probes] node entries={} skips={} mode={mode}",
                CyberpunkVR_DebugProbeNodeEntries.load(Relaxed),
                CyberpunkVR_DebugProbeSkipHits.load(Relaxed)
            ));
        }
    }
    if mode == 1 && node_view_shared(a2) {
        if diagnostics() {
            CyberpunkVR_DebugProbeSkipHits.fetch_add(1, Relaxed);
        }
        return 0;
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[9].load(Acquire));
    original(a1, a2)
}

/// The local shadow maps (sub_140B7CF7C): an experimental skip for the shared views (mode 1; 0 native).
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_local_shadow(a1: *mut c_void, a2: *mut c_void) -> i64 {
    if CyberpunkVR_LocalShadowReuseMode.load(Relaxed) == 1 && node_view_shared(a2) {
        if diagnostics() {
            CyberpunkVR_DebugLocalShadowSkipHits.fetch_add(1, Relaxed);
        }
        return 0;
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[8].load(Acquire));
    original(a1, a2)
}

/// The GI node (sub_1407F4130): for the shared views (mode 1) the engine's early-out, then the apply alone on the
/// renderer's apply manager -- the update is MAIN's. A fault skips GI for that view this frame.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_gi_node(a1: *mut c_void, a2: *mut c_void) -> u8 {
    if !(CyberpunkVR_GiReuseMode.load(Relaxed) == 1 && node_view_shared(a2)) {
        let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u8 =
            core::mem::transmute(cvr_gi_node_original.load(Relaxed));
        return original(a1, a2);
    }
    let base = *rt().exe_base;
    (|| -> Option<u8> {
        let ctx: usize = read(a2 as usize + 0x18)?;
        let null = core::ptr::null_mut();
        let early = guarded_call4(
            base + GI_EARLYCHK_RVA,
            (ctx + 0x14) as *mut c_void,
            null,
            null,
            null,
        )? as usize as u8;
        if early != 0 {
            return Some(early);
        }
        let renderer: usize = read(base + RENDERER_GLOBAL_RVA)?;
        if renderer != 0 {
            let apply_mgr: usize = read(renderer + 0xC0)?;
            if apply_mgr != 0 {
                // The update skipped (MAIN's); the apply only.
                if diagnostics() {
                    CyberpunkVR_DebugGiSkipHits.fetch_add(1, Relaxed);
                }
                return Some(guarded_call4(
                    base + GI_APPLY_RVA,
                    apply_mgr as *mut c_void,
                    a2,
                    null,
                    null,
                )? as usize as u8);
            }
        }
        Some(0)
    })()
    .unwrap_or(0)
}

// ---- the foliage wind

/// The node's owner bit, its wind holder and the renderer's frame id: (wc, holder field) -> (object, frame).
unsafe fn wind_object(a2: usize, field: usize) -> Option<(usize, u32)> {
    let holder: usize = read(a2 + 0x20)?;
    let object: usize = if holder != 0 {
        read(holder + field)?
    } else {
        0
    };
    let renderer: usize = read(*rt().exe_base + RENDERER_GLOBAL_RVA)?;
    if object == 0 || renderer == 0 {
        return Some((0, 0));
    }
    Some((object, read(renderer + 0x4CC8)?))
}
/// THE WIND IMPULSE VOLUME (sub_1407F2BF0): one shared volume, claimed once per frame by whichever view's node runs
/// first on the CPU -- but MAIN's draws reach the GPU before the second view's dispatch, so MAIN read it a frame old.
/// The claim goes to MAIN: the second view finds the frame claimed, MAIN finds it rewound by one
/// (CyberpunkVR_FoliageWindMode 1; 0 the engine's own behaviour).
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_wind_impulse(a1: *mut c_void, a2: *mut c_void) -> u8 {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u8 =
        core::mem::transmute(cvr_reuse_originals[11].load(Acquire));
    if CyberpunkVR_FoliageWindMode.load(Relaxed) != 1 || a2.is_null() || *rt().exe_base == 0 {
        return original(a1, a2);
    }
    let wc = a2 as usize;
    let _ = (|| -> Option<()> {
        // Only the owner dispatches reach the gates at all.
        if read::<u8>(wc + 0x30)? & 2 == 0 {
            return Some(());
        }
        let (wind, frame) = wind_object(wc, 0xB0)?;
        if wind == 0 {
            return Some(());
        }
        let ctx: usize = read(wc + 0x18)?;
        let shared = ctx != 0 && shared_world_view(read::<u64>(ctx + 0x28)?);
        // The pass's own InterlockedExchange leaves the frame id behind either way.
        write(
            wind + 0x1C,
            if shared { frame } else { frame.wrapping_sub(1) },
        )
        .then_some(())?;
        if diagnostics() {
            if shared {
                CyberpunkVR_DebugWindDenyVrcam.fetch_add(1, SeqCst);
            } else {
                CyberpunkVR_DebugWindClaimMain.fetch_add(1, SeqCst);
            }
        }
        Some(())
    })();
    original(a1, a2)
}
/// THE SPEEDTREE WIND (sub_140D60084): 416 bytes of wind state uploaded once per frame by one view.
/// CyberpunkVR_SpeedTreeWindMode: 0 the engine's own behaviour, 1 reopen the latch for the view that finds it
/// claimed, 2 deny the second view the claim so one wind state serves the pair.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_speedtree_wind(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[10].load(Acquire));
    let mode = CyberpunkVR_SpeedTreeWindMode.load(Relaxed);
    if mode == 0 || a2.is_null() || *rt().exe_base == 0 {
        return original(a1, a2);
    }
    let wc = a2 as usize;
    let _ = (|| -> Option<()> {
        // Not an owner call: untouched.
        if read::<u8>(wc + 0x30)? & 2 == 0 {
            return Some(());
        }
        let (mgr, frame) = wind_object(wc, 0x80)?;
        if mgr == 0 {
            return Some(());
        }
        let vrcam_view = || -> Option<bool> {
            let ctx: usize = read(wc + 0x18)?;
            Some(ctx != 0 && read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst))
        };
        if mode == 2 {
            // One wind state per eye-pair: the view running second (MAIN) is handed the latch already claimed.
            if !vrcam_view()? {
                write(mgr + 0x48, frame).then_some(())?;
                if diagnostics() {
                    CyberpunkVR_DebugStWindExtraMain.fetch_add(1, SeqCst);
                }
            } else if diagnostics() {
                CyberpunkVR_DebugStWindFirst.fetch_add(1, SeqCst);
            }
            return Some(());
        }
        if read::<u32>(mgr + 0x48)? != frame {
            if diagnostics() {
                CyberpunkVR_DebugStWindFirst.fetch_add(1, SeqCst);
            }
        } else {
            // Reopen for this call only; the callee's own InterlockedExchange writes the frame id straight back.
            write(mgr + 0x48, frame.wrapping_sub(1)).then_some(())?;
            let vrcam = vrcam_view()?;
            if diagnostics() {
                if vrcam {
                    CyberpunkVR_DebugStWindExtraVrcam.fetch_add(1, SeqCst);
                } else {
                    CyberpunkVR_DebugStWindExtraMain.fetch_add(1, SeqCst);
                }
            }
        }
        Some(())
    })();
    original(a1, a2)
}

// ---- the view parameters: the active environment overrides, run for our own view's source

/// sub_1404928E4: applies ONE override entry.
const VIEW_PARAMS_ENTRY_RVA: usize = 0x4928E4;
/// The environment override stack MAIN's own apply was last called with (a persistent heap object).
static VIEW_PARAMS: AtomicUsize = AtomicUsize::new(0);
/// sub_140492838(stack, source): the engine's apply of the active environment overrides onto one view. MAIN's call
/// is passed on; its stack pointer is kept.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_view_params_apply(a1: usize, a2: i64) {
    if a1 > 0x10000 {
        VIEW_PARAMS.store(a1, Release);
        if diagnostics() {
            CyberpunkVR_DebugViewParamsSeen.fetch_add(1, SeqCst);
        }
    }
    let original: unsafe extern "C" fn(usize, i64) = core::mem::transmute(cvr_reuse_originals[12].load(Acquire));
    original(a1, a2);
}
#[link(name = "kernel32")]
extern "system" {
    fn TryEnterCriticalSection(cs: *mut c_void) -> i32;
    fn LeaveCriticalSection(cs: *mut c_void);
}
/// MODE 2: THE SAME APPLY, WITHOUT THE ERASE. The stack's buckets (count at +128, 16 bytes each from the stack
/// itself: array at +0, entry count at +12; entries 64 bytes, the override object at +16, its record at +24) walked
/// under the stack's own lock, the per-entry apply called and nothing removed; an entry the engine would erase is
/// skipped. TryEnter, never Enter: this runs inside the RTT preparer, and blocking on a lock held up our own stack
/// would hang the game.
unsafe fn apply_params_no_erase(stack: usize, src: i64) {
    let cs = (stack + 136) as *mut c_void;
    if TryEnterCriticalSection(cs) == 0 {
        if diagnostics() {
            CyberpunkVR_DebugViewParamsBusy.fetch_add(1, SeqCst);
        }
        return;
    }
    let entry = *rt().exe_base + VIEW_PARAMS_ENTRY_RVA;
    let count = |c: &AtomicU64| {
        if diagnostics() {
            c.fetch_add(1, SeqCst);
        }
    };
    let _ = (|| -> Option<()> {
        let mut buckets: u32 = read(stack + 128)?;
        if buckets > 4096 {
            // A count this large is not a count.
            buckets = 0;
        }
        for b in 0..buckets as usize {
            let bucket = stack + 16 * b;
            let base: usize = read(bucket)?;
            let n: u32 = read(bucket + 12)?;
            if base == 0 || n > 4096 {
                continue;
            }
            for i in 0..n as usize {
                let slot = base + 64 * i;
                let obj: usize = read(slot + 16)?;
                count(&CyberpunkVR_DebugViewParamsWalked);
                if obj == 0 {
                    // The pass would erase this.
                    count(&CyberpunkVR_DebugViewParamsSkipNoObj);
                    continue;
                }
                let rec = slot + 24;
                let w0: f32 = read(rec)?;
                let w: f32 = read(rec + 4)?;
                let state: i32 = read(rec + 12)?;
                if (w <= 0.0 || w0 == 0.0) && state == 2 {
                    // It would erase this too.
                    count(&CyberpunkVR_DebugViewParamsSkipErase);
                    continue;
                }
                guarded_call4(
                    entry,
                    rec as *mut c_void,
                    src as *mut c_void,
                    obj as *mut c_void,
                    core::ptr::null_mut(),
                )?;
                count(&CyberpunkVR_DebugViewParamsEntries);
            }
        }
        Some(())
    })();
    LeaveCriticalSection(cs);
}
/// The overrides run for our own source: mode 2 the walk above, 1 the engine's whole pass (which also erases faded
/// entries from a list MAIN shares: the 0x88 driver fault came back 229 s into the one session it ran in).
unsafe fn run_params_for_source(src: i64) {
    let mode = CyberpunkVR_RunViewParams.load(Relaxed);
    if src == 0 || mode == 0 {
        return;
    }
    let params = VIEW_PARAMS.load(Acquire);
    if params == 0 {
        return;
    }
    if mode >= 2 {
        apply_params_no_erase(params, src);
    } else {
        let original = cvr_reuse_originals[12].load(Acquire);
        if original == 0 {
            return;
        }
        let null = core::ptr::null_mut();
        if guarded_call4(
            original,
            params as *mut c_void,
            src as *mut c_void,
            null,
            null,
        )
        .is_none()
        {
            return;
        }
    }
    if diagnostics() {
        CyberpunkVR_DebugViewParamsRuns.fetch_add(1, SeqCst);
    }
}
/// sub_140499DF0: the copy that carries a freshly produced parameter struct into a view's block. Our own preparer's
/// scope is the only thing that opens this (t_rtt_prepare_ours), so no other view is touched.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_view_data_copy(dst: i64, src: i64) -> i64 {
    if src != 0 && crate::frame_graph::cvr_fg_rtt_prepare_ours() {
        run_params_for_source(src);
    }
    let original: unsafe extern "C" fn(i64, i64) -> i64 = core::mem::transmute(cvr_reuse_originals[13].load(Acquire));
    let result = original(dst, src);
    (*rt().vd_write_epoch).fetch_add(1, Release);
    result
}
/// One line every 5 s while the mode is on (diagnostics), so "it did not help" and "it never ran" cannot be confused.
#[no_mangle]
pub extern "C" fn cvr_reuse_view_params_report() {
    if !diagnostics() {
        return;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    // SAFETY: a plain system query; then exports of the C++ half.
    unsafe {
        let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
        let mode = CyberpunkVR_RunViewParams.load(Relaxed);
        let last = LAST.load(Relaxed);
        if mode == 0 || (last != 0 && now - last < 5000) {
            return;
        }
        LAST.store(now, Relaxed);
        cvr_host::log::write(&format!(
            "[viewparams] mode={mode} runs={} walked={} applied={} skip(noobj={} erase={}) lock busy={} (MAIN's params seen {})\n",
            CyberpunkVR_DebugViewParamsRuns.load(Relaxed),
            CyberpunkVR_DebugViewParamsWalked.load(Relaxed),
            CyberpunkVR_DebugViewParamsEntries.load(Relaxed),
            CyberpunkVR_DebugViewParamsSkipNoObj.load(Relaxed),
            CyberpunkVR_DebugViewParamsSkipErase.load(Relaxed),
            CyberpunkVR_DebugViewParamsBusy.load(Relaxed),
            CyberpunkVR_DebugViewParamsSeen.load(Relaxed)
        ));
    }
}

// ---- the shadow cascades

/// The cascade record: scene manager + 801616 + 1104 * index (one shared object each view rewrites before its pass).
const CASC_REC_BASE: usize = 801616;
const CASC_REC_STRIDE: usize = 1104;
const CASC_REC_SCAN: usize = 1104;
const CASC_EXTENT_OFF: usize = 0x40;
const CASC_CENTRE_OFF: usize = 0x80;
/// viewData + 2664 + 16 * cascade index: the cascade bias.
const CASC_BIAS_OFFSET: usize = 2664;
/// sub_14033C248: a work context's view data.
const CASC_VIEWDATA_RVA: usize = 0x33C248;

#[derive(Clone, Copy, Default)]
struct CascFit {
    centre: [f32; 3],
    extent: [f32; 2],
    have: bool,
    max_centre_d: f32,
    max_extent_d: f32,
    /// As the engine fitted it, before the lend touched anything.
    max_pre_d: f32,
    pairs: u64,
}
struct Cascades {
    rec: [[u8; CASC_REC_SCAN]; 8],
    rec_have: [bool; 8],
    bias_main: [f32; 8],
    bias_have: [bool; 8],
    fit: [CascFit; 8],
    dispatch: [[u64; 8]; 2],
}
static CASCADES: std::sync::Mutex<Cascades> = std::sync::Mutex::new(Cascades {
    rec: [[0; CASC_REC_SCAN]; 8],
    rec_have: [false; 8],
    bias_main: [0.0; 8],
    bias_have: [false; 8],
    fit: [CascFit {
        centre: [0.0; 3],
        extent: [0.0; 2],
        have: false,
        max_centre_d: 0.0,
        max_extent_d: 0.0,
        max_pre_d: 0.0,
        pairs: 0,
    }; 8],
    dispatch: [[0; 8]; 2],
});
fn cascades() -> std::sync::MutexGuard<'static, Cascades> {
    CASCADES.lock().unwrap_or_else(|e| e.into_inner())
}
/// A report at most every `ms` per caller.
fn every(last: &AtomicU64, ms: u64) -> bool {
    // SAFETY: a plain system query.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let l = last.load(Relaxed);
    if l != 0 && now - l < ms {
        return false;
    }
    last.store(now, Relaxed);
    true
}
/// snprintf into a buffer of `capacity`: what fits of it.
fn bounded(line: &mut String, piece: &str, capacity: usize) {
    let room = capacity.saturating_sub(1).saturating_sub(line.len());
    line.push_str(&piece[..piece.len().min(room)]);
}

fn read_f32s<const N: usize>(at: usize) -> Option<[f32; N]> {
    read::<[f32; N]>(at)
}
fn cascfit_centre_delta(rec: usize, b: &[f32; 3]) -> f32 {
    let Some(c) = read_f32s::<3>(rec + CASC_CENTRE_OFF) else {
        return 0.0;
    };
    let (dx, dy, dz) = (c[0] - b[0], c[1] - b[1], c[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}
fn cascfit_note_pre(s: &mut Cascades, rec_main: usize, idx: usize) {
    let f = &mut s.fit[idx];
    if !f.have {
        return;
    }
    let d = cascfit_centre_delta(rec_main, &f.centre);
    if d > f.max_pre_d {
        f.max_pre_d = d;
    }
}
/// The second view runs first, so it records and MAIN compares: in-frame by construction.
fn cascfit_note(s: &mut Cascades, rec: usize, idx: usize, vrcam: bool) {
    let (Some(centre), Some(extent)) = (
        read_f32s::<3>(rec + CASC_CENTRE_OFF),
        read_f32s::<2>(rec + CASC_EXTENT_OFF),
    ) else {
        return;
    };
    let f = &mut s.fit[idx];
    if vrcam {
        f.centre = centre;
        f.extent = extent;
        f.have = true;
        return;
    }
    if !f.have {
        return;
    }
    let (dx, dy, dz) = (
        centre[0] - f.centre[0],
        centre[1] - f.centre[1],
        centre[2] - f.centre[2],
    );
    let dc = (dx * dx + dy * dy + dz * dz).sqrt();
    let de = (extent[0] - f.extent[0]).abs() + (extent[1] - f.extent[1]).abs();
    if dc > f.max_centre_d {
        f.max_centre_d = dc;
    }
    if de > f.max_extent_d {
        f.max_extent_d = de;
    }
    f.pairs += 1;
}
fn cascfit_report(s: &mut Cascades) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 3000) {
        return;
    }
    let mut line = String::new();
    for i in 0..8 {
        let f = &mut s.fit[i];
        if f.pairs == 0 {
            continue;
        }
        if line.len() < 420 - 70 {
            let piece = format!(
                "casc{i} engine={:.4} m after={:.4} m maxExtent={:.4} pairs={}  ",
                f.max_pre_d, f.max_centre_d, f.max_extent_d, f.pairs
            );
            bounded(&mut line, &piece, 420);
        }
        // Per interval, so the number describes the seconds being looked at.
        f.max_centre_d = 0.0;
        f.max_extent_d = 0.0;
        f.max_pre_d = 0.0;
        f.pairs = 0;
    }
    stereo_log(&format!(
        "[cascfit] centre separation, worst in this interval -- engine = as fitted, after = after the lend: {}",
        if line.is_empty() { "(no pair seen)" } else { line.as_str() }
    ));
}
fn casc_rec_report(idx: usize, mine: usize, theirs: &[u8; CASC_REC_SCAN]) {
    if !diagnostics() {
        return;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 5000) {
        return;
    }
    let mut bytes = [0u8; CASC_REC_SCAN];
    if !read_bytes(mine, &mut bytes) {
        return;
    }
    let mut line = String::new();
    let mut n = 0;
    let mut o = 16;
    while o + 4 <= CASC_REC_SCAN {
        if bytes[o..o + 4] != theirs[o..o + 4] {
            n += 1;
            let a = f32::from_ne_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
            let b = f32::from_ne_bytes([theirs[o], theirs[o + 1], theirs[o + 2], theirs[o + 3]]);
            if line.len() < 900 - 60 {
                bounded(&mut line, &format!("+{o:X} {a:.4}/{b:.4}  "), 900);
            }
        }
        o += 4;
    }
    // SAFETY: an export of the C++ half.
    let lends = unsafe { CyberpunkVR_DebugCascRecLends.load(Relaxed) };
    stereo_log(&format!(
        "[cascrec] cascade {idx} record, dwords differing between the views ({n}): {} || lends={lends}",
        if n != 0 { line.as_str() } else { "(none)" }
    ));
}
fn casc_count_report(s: &Cascades) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 5000) {
        return;
    }
    let mut line = String::new();
    for i in 0..8 {
        if s.dispatch[0][i] == 0 && s.dispatch[1][i] == 0 {
            continue;
        }
        if line.len() < 400 - 40 {
            bounded(
                &mut line,
                &format!("casc{i} M={} V={}  ", s.dispatch[0][i], s.dispatch[1][i]),
                400,
            );
        }
    }
    stereo_log(&format!(
        "[casccount] cascade-node dispatches per index and view: {}",
        if line.is_empty() {
            "(none)"
        } else {
            line.as_str()
        }
    ));
}
fn casc_bias_report(idx: usize, m: f32, v: f32) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 5000) {
        return;
    }
    // SAFETY: an export of the C++ half.
    let lends = unsafe { CyberpunkVR_DebugCascBiasLends.load(Relaxed) };
    stereo_log(&format!(
        "[cascbias] viewData+{CASC_BIAS_OFFSET}+16*{idx} : MAIN {m:.6} | VRCAM {v:.6} | delta {:.6} | lends={lends}",
        v - m
    ));
}

/// The node's view is the port's VRCAM (`known`: a context was read at all).
fn casc_ctx_is_vrcam(a2: *mut c_void) -> (bool, bool) {
    if a2.is_null() {
        return (false, false);
    }
    (|| -> Option<(bool, bool)> {
        let ctx: usize = read(a2 as usize + 0x18)?;
        if ctx == 0 {
            return Some((false, false));
        }
        // SAFETY: an export of the C++ half.
        Some((
            read::<u64>(ctx + 0x28)? == unsafe { g_vrcam_ctx_key.load(SeqCst) },
            true,
        ))
    })()
    .unwrap_or((false, false))
}
fn casc_skip_for_main(a2: *mut c_void) -> bool {
    // SAFETY: exports of the C++ half.
    unsafe {
        if CyberpunkVR_CascadeSkipMain.load(Relaxed) == 0 {
            return false;
        }
        let (vrcam, known) = casc_ctx_is_vrcam(a2);
        if !known || vrcam {
            return false;
        }
        if diagnostics() {
            CyberpunkVR_DebugCascadeMainSkips.fetch_add(1, SeqCst);
        }
    }
    true
}
/// The optics feed's work (its own frustum and shadow transforms).
fn optics_work(a2: *mut c_void) -> bool {
    if a2.is_null() {
        return false;
    }
    (|| -> Option<bool> {
        let ctx: usize = read(a2 as usize + 0x18)?;
        Some(ctx != 0 && read::<u64>(ctx + 0x28)? == *OPTICS_FEED)
    })()
    .unwrap_or(false)
}

/// Readable as the OS says (committed, neither no-access nor guard).
fn os_readable(p: usize, n: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
    };
    if p == 0 {
        return false;
    }
    // SAFETY: VirtualQuery only describes memory.
    unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = core::mem::zeroed();
        if VirtualQuery(
            p as *const c_void,
            &mut mbi,
            core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        ) == 0
            || mbi.State != MEM_COMMIT
            || mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0
        {
            return false;
        }
        p + n <= mbi.BaseAddress as usize + mbi.RegionSize
    }
}
/// The file name of the module holding `addr`.
fn module_of(addr: usize) -> Option<String> {
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameA, GetModuleHandleExA, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    // SAFETY: the module lookup by address; the name into a local buffer.
    unsafe {
        let mut module = core::ptr::null_mut();
        if GetModuleHandleExA(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            addr as *const u8,
            &mut module,
        ) == 0
            || module.is_null()
        {
            return None;
        }
        let mut path = [0u8; 260];
        let n = GetModuleFileNameA(module, path.as_mut_ptr(), 260) as usize;
        if n == 0 {
            return None;
        }
        let path = &path[..n];
        let name = path
            .iter()
            .rposition(|&c| c == b'\\')
            .map_or(path, |i| &path[i + 1..]);
        Some(String::from_utf8_lossy(name).into_owned())
    }
}
/// Which COM pointers in an object carry a graphics or capture vtable (one line every 10 s): the command list the
/// engine holds, found rather than assumed.
fn cmdlist_hunt(obj: usize, what: &str, bytes: usize) {
    // SAFETY: an export of the C++ half.
    if unsafe { CyberpunkVR_CmdListHunt.load(Relaxed) } == 0 || obj == 0 {
        return;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 10000) {
        return;
    }
    let mut line = String::new();
    let mut hits = 0;
    let mut off = 0usize;
    while off + 8 <= bytes {
        let slot = obj + off;
        off += 8;
        if !os_readable(slot, 8) {
            continue;
        }
        let Some(cand) = read::<usize>(slot) else {
            continue;
        };
        if !os_readable(cand, 8) {
            continue;
        }
        let Some(vt) = read::<usize>(cand) else {
            continue;
        };
        if !os_readable(vt, 8) {
            continue;
        }
        let Some(first) = read::<usize>(vt) else {
            continue;
        };
        if !os_readable(first, 8) {
            continue;
        }
        let Some(module) = module_of(first) else {
            continue;
        };
        let lower = module.to_ascii_lowercase();
        if !(lower == "renderdoc.dll" || lower.starts_with("d3d12") || lower == "nvwgf2umx.dll") {
            continue;
        }
        hits += 1;
        // SAFETY: an export of the C++ half.
        unsafe {
            if lower == "renderdoc.dll"
                && CyberpunkVR_DebugCmdListOffset.load(Relaxed) == 0xFFFF_FFFF
            {
                CyberpunkVR_DebugCmdListOffset.store((off - 8) as u32, Relaxed);
            }
        }
        if line.len() < 780 {
            bounded(&mut line, &format!("+0x{:X}->{module}  ", off - 8), 900);
        }
    }
    stereo_log(&format!(
        "[cmdhunt] {what}: {hits} candidate COM pointers with a graphics/capture vtable | {}",
        if hits != 0 { line.as_str() } else { "(none)" }
    ));
}

/// The cascade clear node (sub_1422A3F80): the native clear and its transitions kept; the optics feed's counted.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_cascade_clear(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[14].load(Acquire));
    if optics_work(a2) {
        CyberpunkVR_OpticsCascadeClears.fetch_add(1, Relaxed);
        return original(a1, a2);
    }
    if casc_skip_for_main(a2) {
        // The port's standing "node handled" return.
        return 1;
    }
    original(a1, a2)
}
/// THE SHADOW-CASCADE NODE (sub_1404EDFE0). The cascade index (a1 + 24) published in t_cascade_idx for the census
/// and the cascade camera lend; the placement lent from the view that runs first to the one that runs second (both
/// eyes rasterise on one texel grid); the probes (dispatch count, fit separation, bias lend); the mono cascades' node
/// skip and scope. The optics feed keeps its own frustum and is only counted.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_cascade_node(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let r = rt();
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[15].load(Acquire));
    if optics_work(a2) {
        CyberpunkVR_OpticsCascadeDraws.fetch_add(1, Relaxed);
        return original(a1, a2);
    }
    if casc_skip_for_main(a2) {
        return 1;
    }
    if a1.is_null() {
        return original(a1, a2);
    }
    let b = a2 as usize;
    if CyberpunkVR_CmdListHunt.load(Relaxed) != 0 && b != 0 {
        cmdlist_hunt(b, "node work object", 0x400);
        let ctx = if os_readable(b + 0x18, 8) {
            read::<usize>(b + 0x18).unwrap_or(0)
        } else {
            0
        };
        if ctx != 0 {
            cmdlist_hunt(ctx, "render context", 0x3080);
        }
    }
    let previous = crate::render_tls::cascade_idx();
    let idx = read::<i32>(a1 as usize + 24).unwrap_or(-1);
    crate::render_tls::set_cascade_idx(idx);
    let valid = (0..8).contains(&idx) && b != 0;
    let i = idx as usize;
    let view = |b: usize| -> Option<(usize, bool)> {
        let ctx: usize = read(b + 0x18)?;
        let vrcam = ctx != 0 && read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst);
        Some((ctx, vrcam))
    };
    // Dispatches per view and cascade (answered: both eyes render every cascade there is; default 0).
    if CyberpunkVR_CascCountProbe.load(Relaxed) != 0 && valid {
        let mut s = cascades();
        if let Some((_, vrcam)) = view(b) {
            s.dispatch[vrcam as usize][i] += 1;
        }
        casc_count_report(&s);
    }
    // The cascade placement: snapshot from the view that runs FIRST, given whole (from +0x10) to the one that runs
    // second.
    if CyberpunkVR_CascRecLend.load(Relaxed) != 0 && valid {
        let _ = (|| -> Option<()> {
            let (ctx, vrcam) = view(b)?;
            let mgr: usize = if ctx != 0 { read(ctx + 0x2C40)? } else { 0 };
            if mgr == 0 {
                return Some(());
            }
            let rec = mgr + CASC_REC_BASE + CASC_REC_STRIDE * i;
            let mut s = cascades();
            if vrcam {
                let mut bytes = [0u8; CASC_REC_SCAN];
                read_bytes(rec, &mut bytes).then_some(())?;
                s.rec[i] = bytes;
                s.rec_have[i] = true;
            } else if s.rec_have[i] {
                casc_rec_report(i, rec, &s.rec[i]);
                if CyberpunkVR_CascFitProbe.load(Relaxed) != 0 {
                    cascfit_note_pre(&mut s, rec, i);
                }
                let lent: [u8; CASC_REC_SCAN - 16] = s.rec[i][16..].try_into().ok()?;
                write(rec + 16, lent).then_some(())?;
                if diagnostics() {
                    CyberpunkVR_DebugCascRecLends.fetch_add(1, SeqCst);
                }
            }
            Some(())
        })();
    }
    // How far apart the two views fit this cascade, read AFTER the lend (zero with the lend on).
    if CyberpunkVR_CascFitProbe.load(Relaxed) != 0 && valid {
        let mut s = cascades();
        let _ = (|| -> Option<()> {
            let (ctx, vrcam) = view(b)?;
            let mgr: usize = if ctx != 0 { read(ctx + 0x2C40)? } else { 0 };
            if mgr != 0 {
                cascfit_note(&mut s, mgr + CASC_REC_BASE + CASC_REC_STRIDE * i, i, vrcam);
            }
            Some(())
        })();
        cascfit_report(&mut s);
    }
    if CyberpunkVR_CascadeBiasLend.load(Relaxed) != 0 && valid && *r.exe_base != 0 {
        let _ = (|| -> Option<()> {
            let null = core::ptr::null_mut();
            let vd = guarded_call4(*r.exe_base + CASC_VIEWDATA_RVA, a2, null, null, null)? as usize;
            let (_, vrcam) = view(b)?;
            if vd == 0 {
                return Some(());
            }
            let slot = vd + CASC_BIAS_OFFSET + 16 * i;
            let value: f32 = read(slot)?;
            let mut s = cascades();
            if !vrcam {
                s.bias_main[i] = value;
                s.bias_have[i] = true;
            } else if s.bias_have[i] {
                casc_bias_report(i, s.bias_main[i], value);
                if value != s.bias_main[i] {
                    write(slot, s.bias_main[i]).then_some(())?;
                    if diagnostics() {
                        CyberpunkVR_DebugCascBiasLends.fetch_add(1, SeqCst);
                    }
                }
            }
            Some(())
        })();
    }
    let side = crate::render_tls::view_side();
    if cvr_gpu::mono_cascades::cvr_mono_cascades_skip_node(side, idx) {
        crate::render_tls::set_cascade_idx(previous);
        return 1;
    }
    cvr_gpu::mono_cascades::cvr_mono_cascades_begin_node(side, idx);
    let result = original(a1, a2);
    cvr_gpu::mono_cascades::cvr_mono_cascades_end_node();
    // Restored, or a nested node would leave the wrong index behind.
    crate::render_tls::set_cascade_idx(previous);
    result
}

// ---- the volumetric clouds' constants, and the view-data mirror

/// The cloud constant buffer: 0xC0, per the upload right after its fill.
const CLOUD_CB_BYTES: usize = 192;
/// The view object's size, from the HUD reversing.
const VIEWDATA_BYTES: usize = 0x1000;
/// The graph context: covers everything ever seen used (0x1E10+).
const CTX_BYTES: usize = 0x3080;

/// MAIN's copies, under one lock (the C++ half's g_cloud_cb_mtx).
struct Mirror {
    cloud_cb: [u8; CLOUD_CB_BYTES],
    vd: [u8; VIEWDATA_BYTES],
    ctx: [u8; CTX_BYTES],
}
static MIRROR: std::sync::Mutex<Mirror> = std::sync::Mutex::new(Mirror {
    cloud_cb: [0; CLOUD_CB_BYTES],
    vd: [0; VIEWDATA_BYTES],
    ctx: [0; CTX_BYTES],
});
fn mirror() -> std::sync::MutexGuard<'static, Mirror> {
    MIRROR.lock().unwrap_or_else(|e| e.into_inner())
}
static CLOUD_CB_HAVE: AtomicBool = AtomicBool::new(false);
static VD_HAVE: AtomicBool = AtomicBool::new(false);
/// Bumped under the lock with every write of MAIN's view-data copy, so a reader holding the value it saw knows whether
/// its own copy is still MAIN's current one.
static VD_GEN: AtomicU64 = AtomicU64::new(0);
static VD_LEN: AtomicUsize = AtomicUsize::new(0);
static VD_MAIN_SOURCE: AtomicUsize = AtomicUsize::new(0);
static CTX_HAVE: AtomicBool = AtomicBool::new(false);
static SCOPE_FILL_KEY: AtomicU64 = AtomicU64::new(0);
thread_local! {
    /// One coherent copy of MAIN per thread, taken again whenever MAIN has been captured since.
    static SNAPSHOT: core::cell::RefCell<([u8; VIEWDATA_BYTES], u64)> = const { core::cell::RefCell::new(([0; VIEWDATA_BYTES], 0)) };
}

/// The MAIN-set / second-view-zero ranges the diff reported live (offset, length), one bit each in
/// CyberpunkVR_ViewDataFixMask; what reads each is in ViewReuse.cpp at the table's C++ twin:
/// rain block, composition out (a resource set: dangerous), composition out x3, rain block, cloud wind, rain wetness
/// (proven), composition/debug x3, the night pair, and 0x4B0 that nothing reads.
const HOLES: [(u16, u16); 13] = [
    (0x150, 4),
    (0x168, 16),
    (0x1A0, 4),
    (0x1DC, 8),
    (0x268, 4),
    (0x350, 12),
    (0x5FC, 24),
    (0xB08, 8),
    (0xF98, 8),
    (0xFDC, 4),
    (0xFF0, 12),
    (0x1CC, 12),
    (0x4B0, 4),
];
/// The distant-fog switch (bit 0), the scale pair with it (bit 1), the whole parameter array (bit 2).
const FOG_MIRROR: [(u16, u16); 3] = [(0x930, 4), (0x920, 0x14), (0x8D0, 0x90)];
/// The atmosphere block and the environment look, in bisectable pieces (CyberpunkVR_EnvMirrorMask bit per entry in
/// ENV_BIT); the measured history of every range is at the C++ twin of this table.
const ENV_MIRROR: [(u16, u16); 34] = [
    (0x370, 0x90),
    (0x400, 0x70),
    (0x470, 0xA0),
    (0x510, 0xE0),
    (0x620, 0x28),
    (0x70C, 0x10),
    (0x840, 0x88),
    (0x960, 0x18),
    (0x9A8, 0x08),
    (0xA24, 0x04),
    (0xAD4, 0x34),
    (0xB30, 0x18),
    (0xA28, 0x04),
    (0xBE0, 0x20),
    (0x660, 0x80),
    (0x798, 0x20),
    (0x7C8, 0x10),
    (0x7F0, 0x10),
    (0xA10, 0x10),
    (0x120, 0x20),
    (0x1A0, 0x48),
    (0x330, 0x08),
    (0x748, 0x08),
    (0x5F8, 0x20),
    (0xFC8, 0x18),
    (0xFF8, 0x08),
    (0x730, 0x08),
    (0x788, 0x08),
    (0x7F4, 0x04),
    (0xA20, 0x04),
    (0xA2C, 0x04),
    (0xA30, 0x08),
    (0xF30, 0x04),
    (0x820, 0x04),
];
const ENV_BIT: [u32; 34] = [
    0, 1, 2, 3, 4, 4, 5, 6, 6, 7, 7, 8, 9, 10, 12, 13, 14, 14, 15, 16, 17, 18, 18, 19, 20, 21, 11,
    11, 11, 11, 11, 11, 11, 22,
];

fn diag(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
/// printf's `%.4g`.
fn g4(v: f32) -> String {
    cvr_crt::format(c"%.4g", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
fn dword(b: &[u8], i: usize) -> u32 {
    u32::from_ne_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}
fn qword(b: &[u8], at: usize) -> u64 {
    let mut q = [0u8; 8];
    q.copy_from_slice(&b[at..at + 8]);
    u64::from_ne_bytes(q)
}
fn float(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32::from_ne_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]))
}
/// A user-space address: MAIN's pointer in the second view's slot is a dereference waiting to happen.
fn qword_is_pointer(q: u64) -> bool {
    (0x100_0000_0000..0x7FFF_FFFF_FFFF).contains(&q)
}
fn looks_like_pointer(p: &[u8]) -> bool {
    p.len() >= 8
        && (0..=p.len() - 8)
            .step_by(8)
            .any(|i| qword_is_pointer(qword(p, i)))
}
/// As much as is actually there, in 64-byte steps: the bytes read.
fn raw_copy_upto(dst: &mut [u8], src: usize) -> usize {
    let mut done = 0;
    while done < dst.len() {
        let step = (dst.len() - done).min(64);
        if !read_bytes(src + done, &mut dst[done..done + step]) {
            break;
        }
        done += step;
    }
    done
}

/// a3 is the node work context: view ctx = *(a3 + 0x18), key at ctx + 0x28. MAIN is 0, the second view 1, the optics
/// feed 2; every other view (shadow, reflection, ...) -1, left alone.
fn cloud_cb_view(a3: usize) -> i32 {
    if a3 == 0 {
        return -1;
    }
    (|| -> Option<i32> {
        let ctx: usize = read(a3 + 0x18)?;
        if ctx == 0 {
            return Some(-1);
        }
        let key: u64 = read(ctx + 0x28)?;
        // SAFETY: an export of the C++ half.
        let vrcam = unsafe { g_vrcam_ctx_key.load(SeqCst) };
        Some(if key == 0 {
            0
        } else if key == vrcam {
            1
        } else if key == *OPTICS_FEED {
            2
        } else {
            -1
        })
    })()
    .unwrap_or(-1)
}
unsafe fn cloud_cb_capture_main(cb: usize) {
    let mut tmp = [0u8; CLOUD_CB_BYTES];
    if cb == 0 || !read_bytes(cb, &mut tmp) {
        return;
    }
    mirror().cloud_cb = tmp;
    CLOUD_CB_HAVE.store(true, Release);
    diag(&CyberpunkVR_DebugCloudCbMain);
}
/// Report-only diff of the whole 192 bytes, taken BEFORE anything is written.
unsafe fn cloud_cb_diff_vrcam(cb: usize) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if cb == 0 || !CLOUD_CB_HAVE.load(Acquire) {
        return;
    }
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now - last < 15000 {
        return;
    }
    let mut cur = [0u8; CLOUD_CB_BYTES];
    if !read_bytes(cb, &mut cur) {
        return;
    }
    let reference = mirror().cloud_cb;
    LAST.store(now, Relaxed);
    block_diff_log("cloudCB", &reference, &cur);
}
/// Mode 3: the light-intensity float4 at 0x40 (CyberpunkVR_CloudCbExtra bit 0) and the three noise-layer offsets at
/// +80..+103; 1: MAIN's buffer with this view's size and jitter kept; 2: MAIN's verbatim. No MAIN snapshot yet: the
/// engine's own constants stay.
unsafe fn cloud_cb_apply_vrcam(cb: usize) {
    if cb == 0 || !CLOUD_CB_HAVE.load(Acquire) {
        return;
    }
    let mode = CyberpunkVR_CloudCbMode.load(Relaxed);
    let mut tmp = mirror().cloud_cb;
    if mode == 3 {
        let extra = CyberpunkVR_CloudCbExtra.load(Relaxed);
        if extra & 1 != 0 {
            write_bytes(cb + 0x40, &tmp[0x40..0x50]);
        }
        if extra != 0 {
            diag(&CyberpunkVR_DebugCloudCbExtra);
        }
        if write_bytes(cb + 80, &tmp[80..104]) {
            diag(&CyberpunkVR_DebugCloudCbVrcam);
        }
        return;
    }
    if mode == 1 {
        let mut keep = [0u8; 16];
        let (size, jitter) = keep.split_at_mut(8);
        if read_bytes(cb + 152, size) && read_bytes(cb + 168, jitter) {
            tmp[152..160].copy_from_slice(size);
            tmp[168..176].copy_from_slice(jitter);
        }
    }
    if write_bytes(cb, &tmp) {
        diag(&CyberpunkVR_DebugCloudCbVrcam);
    }
}

/// The dword runs that differ between the views, the ones where MAIN has a value and the second view nothing marked
/// H; mode 2 (CyberpunkVR_ViewDataDiff) prints every run with both sides and its length.
unsafe fn block_diff_log(tag: &str, refb: &[u8], curb: &[u8]) {
    let n = refb.len().min(curb.len()) / 4;
    let all = CyberpunkVR_ViewDataDiff.load(Relaxed) >= 2;
    let mut line = String::new();
    let (mut runs, mut holes) = (0, 0);
    let mut i = 0;
    while i < n {
        if dword(refb, i) == dword(curb, i) {
            i += 1;
            continue;
        }
        let s = i;
        while i < n && dword(refb, i) != dword(curb, i) {
            i += 1;
        }
        runs += 1;
        let vr_zero = (s..i).all(|k| dword(curb, k) == 0);
        let main_set = (s..i).any(|k| dword(refb, k) != 0);
        let hole = vr_zero && main_set;
        if hole {
            holes += 1;
        }
        if all || hole {
            if line.len() > 4000 - 48 {
                stereo_log(&format!("[vdiff] {tag} M|V cont: {line}"));
                line.clear();
            }
            let piece = format!(
                "{:X}*{}{{{:08X}|{:08X}}}{} ",
                s * 4,
                i - s,
                dword(refb, s),
                dword(curb, s),
                if hole { "H" } else { "" }
            );
            bounded(&mut line, &piece, 4000);
        }
    }
    if all {
        stereo_log(&format!(
            "[vdiff] {tag} M|V {runs} runs ({holes} holes, H marked) END: {}",
            if line.is_empty() {
                "(none)"
            } else {
                line.as_str()
            }
        ));
    } else {
        stereo_log(&format!(
            "[vdiff] {tag} MAIN vs VRCAM: {runs} differing runs, {holes} MAIN-set/VRCAM-zero: {}",
            if holes != 0 { line.as_str() } else { "(none)" }
        ));
    }
}
fn ctx_capture_main(ctx: usize) {
    let mut tmp = vec![0u8; CTX_BYTES];
    if ctx == 0 || !read_bytes(ctx, &mut tmp) {
        return;
    }
    mirror().ctx.copy_from_slice(&tmp);
    CTX_HAVE.store(true, Release);
}
/// The graph context's diff, and ALL the frame-graph feature words (*(ctx + 8 * (bit >> 6) + 6096)).
unsafe fn ctx_diff_vrcam(ctx: usize) {
    if ctx == 0 || !CTX_HAVE.load(Acquire) {
        return;
    }
    let mut cur = vec![0u8; CTX_BYTES];
    if !read_bytes(ctx, &mut cur) {
        return;
    }
    let reference = mirror().ctx.to_vec();
    block_diff_log("graphCtx", &reference, &cur);
    let mut fl = String::new();
    for w in 0..24 {
        if fl.len() >= 1200 - 64 {
            break;
        }
        let (m, v) = (
            qword(&reference, 0x25B0 + w * 8),
            qword(&cur, 0x25B0 + w * 8),
        );
        if m == v {
            continue;
        }
        bounded(
            &mut fl,
            &format!("f{w} m={m:016X} v={v:016X} miss={:016X} ", m & !v),
            1200,
        );
    }
    stereo_log(&format!(
        "[fgflags-all] words differing between the views (miss = MAIN has, VRCAM lacks): {}",
        if fl.is_empty() { "(none)" } else { fl.as_str() }
    ));
}
fn viewdata_capture_main(vd: usize) {
    if vd == 0 {
        return;
    }
    let mut tmp = [0u8; VIEWDATA_BYTES];
    let got = raw_copy_upto(&mut tmp, vd);
    if got < 256 {
        return;
    }
    {
        let mut m = mirror();
        m.vd[..got].copy_from_slice(&tmp[..got]);
        VD_GEN.fetch_add(1, Release);
    }
    VD_LEN.store(got, Release);
    VD_MAIN_SOURCE.store(vd, Release);
    VD_HAVE.store(true, Release);
}
/// On a timer, so the scene can be changed and a fresh answer read out: the view-data diff, the fog overlay's inputs
/// by name, and what MAIN holds in the holes (a float is data, an address a resource the second view does not own).
unsafe fn viewdata_diff_vrcam(vd: usize) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if vd == 0 || !VD_HAVE.load(Acquire) {
        return;
    }
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now - last < 15000 {
        return;
    }
    let mut cur = [0u8; VIEWDATA_BYTES];
    let n = raw_copy_upto(&mut cur, vd).min(VD_LEN.load(Acquire)) & !3;
    if n < 256 {
        stereo_log(&format!(
            "[viewData] unreadable: got {n} bytes of {VIEWDATA_BYTES}"
        ));
        LAST.store(now, Relaxed);
        return;
    }
    let mut reference = [0u8; VIEWDATA_BYTES];
    reference[..n].copy_from_slice(&mirror().vd[..n]);
    LAST.store(now, Relaxed);
    block_diff_log("viewData", &reference[..n], &cur[..n]);
    if n >= 0x938 {
        let mut fl = String::new();
        for off in [
            0x8D0usize, 0x8D4, 0x8D8, 0x8DC, 0x8E0, 0x8E4, 0x928, 0x92C, 0x930, 0x934,
        ] {
            let (fm, fv) = (float(&reference, off), float(&cur, off));
            if fl.len() < 700 - 64 {
                let piece = format!(
                    "{off:X}{{M {}|V {}}}{} ",
                    g4(fm),
                    g4(fv),
                    if fm == fv { "" } else { " <<" }
                );
                bounded(&mut fl, &piece, 700);
            }
        }
        stereo_log(&format!(
            "[fog] RenderFogOverlay inputs, MAIN|VRCAM (<< marks a difference): {fl}"
        ));
    }
    let mut vals = String::new();
    for (i, &(off, len)) in HOLES.iter().enumerate() {
        let off = off as usize;
        let f0 = float(&reference, off);
        let (f1, q) = if len >= 8 {
            (float(&reference, off + 4), qword(&reference, off))
        } else {
            (0.0, 0)
        };
        if vals.len() < 900 - 80 {
            let piece = format!(
                "b{i}@{off:X}{{{},{}{}}} ",
                g4(f0),
                g4(f1),
                if len >= 8 && qword_is_pointer(q) {
                    ",PTR"
                } else {
                    ""
                }
            );
            bounded(&mut vals, &piece, 900);
        }
    }
    stereo_log(&format!("[vdiff] MAIN's values in the holes: {vals}"));
}

/// MAIN's bytes into the selected ranges of a view block: the holes (CyberpunkVR_ViewDataFixMask), the fog block
/// (CyberpunkVR_FogMirrorMask) and the atmosphere (CyberpunkVR_EnvMirrorMask, a qword at a time and never across an
/// address on either side). `environment_only`: the scope's share -- the rain and wetness holes and the shipped
/// atmosphere/grade bits. `verify`: compare only, write nothing. The chunks written (or found differing).
unsafe fn viewdata_fill_holes(vd: usize, environment_only: bool, verify: bool) -> u32 {
    let mask = CyberpunkVR_ViewDataFixMask.load(Relaxed)
        & if environment_only {
            1 | 1 << 5 | 1 << 7
        } else {
            !0
        };
    let fogm = CyberpunkVR_FogMirrorMask.load(Relaxed);
    if vd == 0
        || (mask == 0 && fogm == 0 && CyberpunkVR_EnvMirrorMask.load(Relaxed) == 0)
        || !VD_HAVE.load(Acquire)
    {
        return 0;
    }
    let batch = CyberpunkVR_ViewDataBatchSnapshot.load(Relaxed) != 0;
    SNAPSHOT.with(|cell| {
        let mut snapshot = cell.borrow_mut();
        if batch && VD_GEN.load(Acquire) != snapshot.1 {
            let m = mirror();
            snapshot.0 = m.vd;
            snapshot.1 = VD_GEN.load(Relaxed);
        }
        let snapshot = &snapshot.0;
        let main = |off: usize, out: &mut [u8]| {
            if batch {
                out.copy_from_slice(&snapshot[off..off + out.len()]);
            } else {
                out.copy_from_slice(&mirror().vd[off..off + out.len()]);
            }
        };
        let mut chunks = 0u32;
        let mut vstage = [0u8; 0x100];
        for (i, &(off, len)) in HOLES.iter().enumerate() {
            let (off, len) = (off as usize, len as usize);
            if mask & (1 << i) == 0 || len > 64 {
                continue;
            }
            let mut staging = [0u8; 64];
            let staging = &mut staging[..len];
            main(off, staging);
            if looks_like_pointer(staging) {
                continue;
            }
            if verify {
                if read_bytes(vd + off, &mut vstage[..len]) && vstage[..len] != *staging {
                    chunks += 1;
                }
                continue;
            }
            if write_bytes(vd + off, staging) {
                chunks += 1;
                diag(&CyberpunkVR_DebugViewDataFixes);
            }
        }
        if fogm != 0 {
            for (k, &(off, len)) in FOG_MIRROR.iter().enumerate() {
                let (off, len) = (off as usize, len as usize);
                if fogm & (1 << k) == 0 || len > 0x90 {
                    continue;
                }
                let mut fstage = [0u8; 0x90];
                let fstage = &mut fstage[..len];
                main(off, fstage);
                if verify {
                    if read_bytes(vd + off, &mut vstage[..len]) && vstage[..len] != *fstage {
                        chunks += 1;
                    }
                    continue;
                }
                if write_bytes(vd + off, fstage) {
                    chunks += 1;
                    diag(&CyberpunkVR_DebugFogMirrors);
                }
            }
        }
        // The scope receives only the shipped atmosphere/grade ranges, never the experimental ones above bit 15.
        let envm =
            CyberpunkVR_EnvMirrorMask.load(Relaxed) & if environment_only { 0xFFFF } else { !0 };
        if envm != 0 {
            for (k, &(off, len)) in ENV_MIRROR.iter().enumerate() {
                let (off, len) = (off as usize, len as usize);
                if envm & (1 << ENV_BIT[k]) == 0 || len > 0x100 {
                    continue;
                }
                let mut estage = [0u8; 0x100];
                let estage = &mut estage[..len];
                main(off, estage);
                let dst = vd + off;
                // Not a whole number of qwords: the whole range at once, as before (skipping such ranges once switched
                // seven working entries off without a word).
                if len < 8 || len & 7 != 0 {
                    if looks_like_pointer(estage) {
                        continue;
                    }
                    if verify {
                        if read_bytes(dst, &mut vstage[..len]) && vstage[..len] != *estage {
                            chunks += 1;
                        }
                        continue;
                    }
                    if write_bytes(dst, estage) {
                        chunks += 1;
                        diag(&CyberpunkVR_DebugEnvMirrors);
                    }
                    continue;
                }
                // What the second view holds now, so a pointer is refused from either side.
                let mut dstage = [0u8; 0x100];
                let dstage = &mut dstage[..len];
                if !read_bytes(dst, dstage) {
                    continue;
                }
                let mut any = false;
                for q in (0..len).step_by(8) {
                    let (sq, dq) = (qword(estage, q), qword(dstage, q));
                    if qword_is_pointer(sq) || qword_is_pointer(dq) {
                        diag(&CyberpunkVR_DebugEnvMirrorPtrSkips);
                        continue;
                    }
                    if sq == dq {
                        continue;
                    }
                    if verify {
                        chunks += 1;
                        continue;
                    }
                    if write_bytes(dst + q, &estage[q..q + 8]) {
                        any = true;
                        chunks += 1;
                    }
                }
                if any {
                    diag(&CyberpunkVR_DebugEnvMirrors);
                }
            }
        }
        chunks
    })
}
/// The scope view's fill once per change instead of once per node: keyed on the block, MAIN's capture generation and
/// the engine's view-data write epoch; every 64th skipped call verifies, and a difference re-fills at once (counted
/// in CyberpunkVR_OpticsVdStale). True: this call needs no further fill.
unsafe fn scope_fill_gated(vd: usize) -> bool {
    let mut key = (vd as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ VD_GEN.load(Acquire).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (*rt().vd_write_epoch)
            .load(Acquire)
            .wrapping_mul(0x1656_67B1_9E37_79F9);
    if key == 0 {
        key = 1;
    }
    if key == SCOPE_FILL_KEY.load(Acquire) {
        let n = CyberpunkVR_OpticsVdSkips.fetch_add(1, SeqCst) + 1;
        if n & 63 != 0 {
            return true;
        }
        CyberpunkVR_OpticsVdVerifies.fetch_add(1, SeqCst);
        let stale = viewdata_fill_holes(vd, true, true);
        if stale == 0 {
            return true;
        }
        CyberpunkVR_OpticsVdStale.fetch_add(stale as u64, SeqCst);
    }
    viewdata_fill_holes(vd, true, false);
    SCOPE_FILL_KEY.store(key, Release);
    CyberpunkVR_OpticsVdFills.fetch_add(1, SeqCst);
    true
}
/// From the node dispatch (NodeDispatch.cpp's viewdata_fill_from_wc): the view block through the engine's own
/// accessor, (*wc)->vt[4](); the composition gate lent first (the second view), or for the scope's environment-only
/// fill, MAIN's own block refused and the fill gated.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_viewdata_fill_from_wc(wc: *mut c_void, environment_only: bool) {
    let Some(vd) = (|| -> Option<usize> {
        let obj: usize = if wc.is_null() { 0 } else { read(wc as usize)? };
        let vt: usize = if obj != 0 { read(obj)? } else { 0 };
        if vt == 0 {
            return Some(0);
        }
        let accessor: usize = read(vt + 32)?;
        if accessor == 0 {
            return Some(0);
        }
        let null = core::ptr::null_mut();
        Some(guarded_call4(accessor, obj as *mut c_void, null, null, null)? as usize)
    })() else {
        return;
    };
    if environment_only {
        if vd == 0 || vd == VD_MAIN_SOURCE.load(Acquire) {
            return;
        }
        if CyberpunkVR_OpticsVdFillGate.load(Relaxed) != 0 && scope_fill_gated(vd) {
            return;
        }
    } else {
        vd_lend_composition(vd);
    }
    if vd != 0 {
        viewdata_fill_holes(vd, environment_only, false);
    }
}
/// The cloud constant fill (sub_1407F825C): MAIN's buffer captured, the second view's and the optics feed's given the
/// noise-layer offsets (mode 3); and MAIN's view block and graph context captured here, where viewData is a4 - 0x440,
/// for the fill and for the diffs (CyberpunkVR_ViewDataDiff).
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_cloud_cb_fill(
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: *mut i32,
    a7: i32,
    a8: i32,
) -> i64 {
    let original: unsafe extern "C" fn(i64, i64, i64, i64, i64, *mut i32, i32, i32) -> i64 =
        core::mem::transmute(cvr_reuse_originals[4].load(Acquire));
    let r = original(a1, a2, a3, a4, a5, a6, a7, a8);
    let mode = CyberpunkVR_CloudCbMode.load(Relaxed);
    let diff = CyberpunkVR_ViewDataDiff.load(Relaxed);
    let v = if mode != 0 || diff != 0 {
        cloud_cb_view(a3 as usize)
    } else {
        -1
    };
    if mode != 0 {
        if v == 0 {
            cloud_cb_capture_main(a2 as usize);
        } else if v >= 1 {
            if v == 1 && diff >= 2 {
                cloud_cb_diff_vrcam(a2 as usize);
            }
            cloud_cb_apply_vrcam(a2 as usize);
        }
    }
    if a4 != 0 && (diff != 0 || CyberpunkVR_ViewDataFixMask.load(Relaxed) != 0) {
        let view_data = a4.wrapping_sub(0x440) as usize;
        let ctx = if diff != 0 {
            read::<usize>((a3 as usize).wrapping_add(0x18)).unwrap_or(0)
        } else {
            0
        };
        if v == 0 {
            viewdata_capture_main(view_data);
            if ctx != 0 {
                ctx_capture_main(ctx);
            }
        } else if v == 1 && diff != 0 {
            if ctx != 0 {
                ctx_diff_vrcam(ctx);
            }
            viewdata_diff_vrcam(view_data);
        }
    }
    r
}

// ---- the sky-scattering node's probes: its three gates per view, and the resource slots the atmosphere mirror skips

/// sub_14033E358(work, bit): the frame graph's feature-bit test.
const FEATURE_BIT_TEST_RVA: usize = 0x33E358;
/// The viewData resource slots (0x6E0, 0xF58, 0xF88, 0xFB8 as the diff named them, read sixteen bytes on).
const ENV_PTR_OFF: [usize; 4] = [0x6F0, 0xF68, 0xF98, 0xFC8];

#[derive(Clone, Copy)]
struct ViewRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    bit35: i32,
    glob: i32,
    have: bool,
}
const NO_RECT: ViewRect = ViewRect {
    x0: 0,
    y0: 0,
    x1: 0,
    y1: 0,
    bit35: 0,
    glob: 0,
    have: false,
};
struct SkyProbes {
    /// [0] MAIN, [1] the second view.
    rect: [ViewRect; 2],
    envptr: [[usize; 4]; 2],
    envptr_have: [bool; 2],
}
static SKY_PROBES: std::sync::Mutex<SkyProbes> = std::sync::Mutex::new(SkyProbes {
    rect: [NO_RECT; 2],
    envptr: [[0; 4]; 2],
    envptr_have: [false; 2],
});
fn sky_probes() -> std::sync::MutexGuard<'static, SkyProbes> {
    SKY_PROBES.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn viewrect_report(s: &SkyProbes) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 5000) {
        return;
    }
    let (m, v) = (&s.rect[0], &s.rect[1]);
    let empty = |r: &ViewRect| (r.have && (r.x0 >= r.x1 || r.y0 >= r.y1)) as i32;
    stereo_log(&format!(
        "[viewrect] sky node gates -- MAIN rect ({},{})..({},{}) empty={} bit35={} glob={} | VRCAM rect ({},{})..({},{}) \
         empty={} bit35={} glob={} | reads={}{}",
        m.x0,
        m.y0,
        m.x1,
        m.y1,
        empty(m),
        m.bit35,
        m.glob,
        v.x0,
        v.y0,
        v.x1,
        v.y1,
        empty(v),
        v.bit35,
        v.glob,
        CyberpunkVR_DebugViewRectReads.load(Relaxed),
        if m.have && v.have { "" } else { "  (one side not seen yet)" }
    ));
}
fn envptr_report(s: &SkyProbes) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !every(&LAST, 5000) {
        return;
    }
    let mut line = String::new();
    for (k, off) in ENV_PTR_OFF.iter().enumerate() {
        let (m, v) = (s.envptr[0][k], s.envptr[1][k]);
        let piece = format!(
            "+0x{off:X} M={m:016X} V={v:016X}{}  ",
            if m == v { " same" } else { " DIFFER" }
        );
        bounded(&mut line, &piece, 600);
    }
    stereo_log(&format!(
        "[envptr] viewData resource slots the atmosphere mirror skips: {line}{}",
        if s.envptr_have[0] && s.envptr_have[1] {
            ""
        } else {
            " (one side not seen yet)"
        }
    ));
}
/// RenderSkyScattering's node work (sub_1407F86CC), probed and passed on: the three gates exactly as the engine
/// evaluates them (feature bit 35 through the engine's own test, the empty-rect test on ctx + 0x14, the global at
/// *(qword_143BBCAE8 + 200)) per view, and the four viewData resource slots side by side. Read only.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_sky_scattering(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let r = rt();
    let base = *r.exe_base;
    if CyberpunkVR_ViewRectProbe.load(Relaxed) != 0 && !a2.is_null() {
        let mut s = sky_probes();
        let _ = (|| -> Option<()> {
            let ctx: usize = read(a2 as usize + 0x18)?;
            if ctx == 0 {
                return Some(());
            }
            let rect: [i32; 4] = read(ctx + 0x14)?;
            let vrcam = read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst);
            let slot = &mut s.rect[vrcam as usize];
            [slot.x0, slot.y0, slot.x1, slot.y1] = rect;
            // The feature bit, asked the way the node asks it; -1: the test could not be reached.
            slot.bit35 = -1;
            if base != 0 {
                let null = core::ptr::null_mut();
                slot.bit35 =
                    guarded_call3(base + FEATURE_BIT_TEST_RVA, a2, 35 as *mut c_void, null)? as i32;
            }
            // The global the node checks last, view-independent by nature.
            slot.glob = -1;
            if base != 0 {
                let renderer: usize = read(base + RENDERER_GLOBAL_RVA)?;
                slot.glob = (renderer != 0 && read::<usize>(renderer + 200)? != 0) as i32;
            }
            slot.have = true;
            diag(&CyberpunkVR_DebugViewRectReads);
            Some(())
        })();
        viewrect_report(&s);
    }
    if CyberpunkVR_EnvPtrProbe.load(Relaxed) != 0 && !a2.is_null() && base != 0 {
        let mut s = sky_probes();
        let _ = (|| -> Option<()> {
            let null = core::ptr::null_mut();
            let vd = guarded_call4(base + CASC_VIEWDATA_RVA, a2, null, null, null)? as usize;
            let ctx: usize = read(a2 as usize + 0x18)?;
            if vd != 0 && ctx != 0 {
                let side = (read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst)) as usize;
                for (k, off) in ENV_PTR_OFF.iter().enumerate() {
                    s.envptr[side][k] = read(vd + off)?;
                }
                s.envptr_have[side] = true;
            }
            Some(())
        })();
        envptr_report(&s);
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[5].load(Acquire));
    original(a1, a2)
}

// ---- the light-volume pass's block-list lend, and the composition lend

/// sub_140B7CB18: the engine's copy of a refcounted {object, count} handle.
const HANDLE_COPY_RVA: usize = 0xB7CB18;
/// The composition-state assign's return addresses: in the game frame sub_1405D48F0 after the frame submit (menus and
/// loading), and in the view fill sub_14029CDD8 (in the world).
const COMP_ASSIGN_FRAME_RET_RVA: usize = 0x5D317F;
const COMP_ASSIGN_VIEW_RET_RVA: usize = 0x4C3255;

/// RenderLightBuffers (sub_140411B94): MAIN's draw-block list lent at viewData+0x168 for one call, only where the
/// second view has none (CyberpunkVR_LightBlockLend, default 0: it crashed; and its source, g_main_block_v5, is no
/// longer written by anything).
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_light_buffers(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let r = rt();
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_reuse_originals[3].load(Acquire));
    let main_block = *r.main_block_v5;
    let base = *r.exe_base;
    if CyberpunkVR_LightBlockLend.load(Relaxed) == 0
        || a2.is_null()
        || !crate::render_tls::vrcam_node_active()
        || main_block.is_null()
        || base == 0
    {
        return original(a1, a2);
    }
    let slot = (|| -> Option<(usize, usize)> {
        let null = core::ptr::null_mut();
        let vd = guarded_call4(base + CASC_VIEWDATA_RVA, a2, null, null, null)? as usize;
        if vd == 0 {
            return None;
        }
        Some((vd + 0x168, read::<usize>(vd + 0x168)?))
    })();
    // Only lend where the view has nothing of its own; never displace a real list.
    let Some((slot, 0)) = slot else {
        return original(a1, a2);
    };
    write(slot, main_block as usize);
    diag(&CyberpunkVR_DebugLightBlockLends);
    let result = original(a1, a2);
    write(slot, 0usize);
    result
}

/// The composition set, learned from the assign gated on branch A's precondition.
static COMP_SET: AtomicUsize = AtomicUsize::new(0);
/// The handles MAIN received from the set, this assign's and the previous one's ({object, count}, a count held on
/// each through the engine's own handle copy): an object stays alive for two more assigns after the second view
/// last read it.
static COMP_HOLD: std::sync::RwLock<[[i64; 2]; 2]> = std::sync::RwLock::new([[0; 2]; 2]);
static COMP_FRAME: AtomicU64 = AtomicU64::new(0);

/// One of the two objects the port holds a count on, i.e. still alive.
fn comp_held(value: u64) -> bool {
    if value == 0 {
        return false;
    }
    let hold = COMP_HOLD.read().unwrap_or_else(|e| e.into_inner());
    hold[0][0] as u64 == value || hold[1][0] as u64 == value
}
unsafe fn comp_hold_frame(packet: usize) {
    let copy = *rt().exe_base + HANDLE_COPY_RVA;
    let Some(mut pair) = read::<[i64; 2]>(packet + 0x168) else {
        return;
    };
    let null = core::ptr::null_mut();
    {
        let mut hold = COMP_HOLD.write().unwrap_or_else(|e| e.into_inner());
        let [newer, older] = &mut *hold;
        guarded_call4(
            copy,
            older.as_mut_ptr().cast(),
            newer.as_mut_ptr().cast(),
            null,
            null,
        );
        guarded_call4(
            copy,
            newer.as_mut_ptr().cast(),
            pair.as_mut_ptr().cast(),
            null,
            null,
        );
        COMP_FRAME.store(hold[0][0] as u64, Release);
    }
    CyberpunkVR_CompLendFrameStats[0].fetch_add(1, SeqCst);
}
/// The composition-state assign (sub_140545714), entered with its caller's return address in r8.
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_comp_assign(set: i64, src: i64) -> i64 {
    core::arch::naked_asm!("mov r8, [rsp]", "jmp {body}", body = sym comp_assign);
}
/// Learns the set pointer, gated on branch A's precondition read AT ENTRY (before the original consumes the one-shot
/// flag); after the assign of MAIN's packet from either of its two places it takes the count (COMP_HOLD).
unsafe extern "C" fn comp_assign(set: i64, src: i64, return_address: usize) -> i64 {
    let r = rt();
    if set > 0x10000 {
        if let Some([up, handle]) = read::<[u32; 2]>(set as usize + 8) {
            if up != 0 && handle != 0 {
                COMP_SET.store(set as usize, Release);
                diag(&CyberpunkVR_DebugCompScopedSet);
            }
        }
    }
    let ret = return_address.wrapping_sub(*r.exe_base);
    let original: unsafe extern "C" fn(i64, i64) -> i64 = core::mem::transmute(cvr_reuse_originals[0].load(Acquire));
    let result = original(set, src);
    if CyberpunkVR_CompLendFrameHold.load(Relaxed) != 0
        && CyberpunkVR_CompLendSet.load(Relaxed) != 0
        && src != 0
        && (ret == COMP_ASSIGN_FRAME_RET_RVA || ret == COMP_ASSIGN_VIEW_RET_RVA)
    {
        if set as usize == COMP_SET.load(Acquire) {
            comp_hold_frame(src as usize);
            CyberpunkVR_CompLendFrameStats[if ret == COMP_ASSIGN_FRAME_RET_RVA {
                3
            } else {
                4
            }]
            .fetch_add(1, SeqCst);
        } else {
            CyberpunkVR_CompLendFrameStats[5].fetch_add(1, SeqCst);
        }
    }
    result
}
/// More than half the selected VRCAM resolution in both axes; no selection yet: nothing is full size.
fn comp_lend_full_size(wh: [u32; 2]) -> bool {
    // SAFETY: exports of the C++ half.
    let (w, h) = unsafe { (g_vrcam_sel_w.load(Relaxed), g_vrcam_sel_h.load(Relaxed)) };
    w != 0 && h != 0 && wh[0] > (w + 1) / 2 && wh[1] > (h + 1) / 2
}
/// The set's live object: holder at set+176 (branch A), {value, refcount} at holder+600.
fn comp_lend_set_object() -> u64 {
    let set = COMP_SET.load(Acquire);
    if set == 0 {
        return 0;
    }
    match read::<u64>(set + 176) {
        Some(holder) if holder != 0 => {
            read::<[u64; 2]>(holder as usize + 600).map_or(0, |pair| pair[0])
        }
        _ => 0,
    }
}
/// What the second view lends -- the object held from MAIN's frame packet, or the set's live one when nothing is
/// held -- and whether it was the held one; 0 when nothing full-size is there (the pass size at +0x50/+0x54).
unsafe fn comp_lend_object() -> (u64, bool) {
    let mut value = if CyberpunkVR_CompLendFrameHold.load(Relaxed) != 0 {
        COMP_FRAME.load(Acquire)
    } else {
        0
    };
    let held = value != 0;
    if value == 0 {
        value = comp_lend_set_object();
    }
    if value == 0 {
        return (0, held);
    }
    match read::<[u32; 2]>(value as usize + 0x50) {
        Some(wh) if comp_lend_full_size(wh) => (value, held),
        _ => (0, held),
    }
}
/// The composition gate written into the second view's block at every node it dispatches: {MAIN's object, NO
/// refcount} (the engine's release is a no-op on it), never over a pair the view owns, and with the frame latch the
/// block keeps its frame's object while that object is still held.
unsafe fn vd_lend_composition(vd: usize) {
    if vd == 0 || CyberpunkVR_CompLendSet.load(Relaxed) == 0 {
        return;
    }
    let (value, held) = comp_lend_object();
    if value == 0 {
        diag(&CyberpunkVR_DebugCompLendSkips);
        return;
    }
    let Some(current) = read::<[u64; 2]>(vd + 0x168) else {
        return;
    };
    // A paired MAIN root already owns its composition.
    if current[1] != 0 || current[0] == value {
        return;
    }
    let stats = &CyberpunkVR_CompLendFrameStats;
    if current[0] != 0 && held && CyberpunkVR_CompLendFrameLatch.load(Relaxed) != 0 {
        if comp_held(current[0]) {
            stats[7].fetch_add(1, SeqCst);
            return;
        }
        stats[8].fetch_add(1, SeqCst);
    }
    if current[0] == 0 {
        stats[6].fetch_add(1, SeqCst);
    }
    if !write(vd + 0x168, [value, 0u64]) {
        return;
    }
    diag(&CyberpunkVR_DebugCompLendWrites);
    if held {
        stats[1].fetch_add(1, SeqCst);
        if comp_lend_set_object() != value {
            stats[2].fetch_add(1, SeqCst);
        }
    }
}
/// The scoped lend (CyberpunkVR_CompLendScoped) for one node call: the viewData whose +0x168 was written, or 0;
/// `saved` takes what was there (by the guard, always a zeroed pair).
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_comp_lend_install(
    work_context: *mut c_void,
    saved: *mut u64,
) -> usize {
    let base = *rt().exe_base;
    if CyberpunkVR_CompLendScoped.load(Relaxed) == 0 || work_context.is_null() || base == 0 {
        return 0;
    }
    let null = core::ptr::null_mut();
    let Some(vd) = guarded_call4(base + CASC_VIEWDATA_RVA, work_context, null, null, null) else {
        return 0;
    };
    let vd = vd as usize;
    if vd == 0 {
        return 0;
    }
    let (value, _) = comp_lend_object();
    if value == 0 {
        diag(&CyberpunkVR_DebugCompScopedSkips);
        return 0;
    }
    let Some(found) = read::<[u64; 2]>(vd + 0x168) else {
        return 0;
    };
    *saved = found[0];
    *saved.add(1) = found[1];
    // It has one of its own: never displace that.
    if found != [0, 0] || !write(vd + 0x168, [value, 0u64]) {
        return 0;
    }
    diag(&CyberpunkVR_DebugCompScopedLends);
    vd
}
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_comp_lend_restore(vd: usize, saved: *const u64) {
    if vd != 0 {
        write(vd + 0x168, [*saved, *saved.add(1)]);
    }
}
/// sub_1402ABA18, the element's draw keys: an element with no owner in this view contributes none (its caller
/// discards the result), the filter at the subtree's entry under the composition lend.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_comp_elem_keys(
    a1: i64,
    a2: *mut c_void,
    a3: u32,
    a4: i64,
    a5: i64,
) -> i64 {
    if a4 == 0 && CyberpunkVR_CompLendSet.load(Relaxed) != 0 {
        diag(&CyberpunkVR_DebugCompElemSkips);
        return 0;
    }
    let original: unsafe extern "C" fn(i64, *mut c_void, u32, i64, i64) -> i64 =
        core::mem::transmute(cvr_reuse_originals[1].load(Acquire));
    original(a1, a2, a3, a4, a5)
}
/// sub_1402AC150, the element's scissor rect: a1 == 0 given the answer the function gives a null *(a1 + 0x70) -- the
/// twenty output bytes zeroed and a2 returned.
#[no_mangle]
pub unsafe extern "C" fn cvr_reuse_comp_elem_rect(a1: i64, a2: i64, a3: *mut u16) -> i64 {
    if a1 == 0 && CyberpunkVR_CompLendSet.load(Relaxed) != 0 {
        if a2 != 0 {
            write(a2 as usize, [0u8; 20]);
        }
        diag(&CyberpunkVR_DebugCompElemNulls);
        return a2;
    }
    let original: unsafe extern "C" fn(i64, i64, *mut u16) -> i64 =
        core::mem::transmute(cvr_reuse_originals[2].load(Acquire));
    original(a1, a2, a3)
}
/// One line every five seconds while the lend is armed (diagnostics): elements filtered at the subtree entry against
/// nulls the leaf guard caught.
#[no_mangle]
pub extern "C" fn cvr_reuse_complend_report() {
    if !diagnostics() {
        return;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    // SAFETY: exports of the C++ half.
    unsafe {
        let set = CyberpunkVR_CompLendSet.load(Relaxed);
        let last = LAST.load(Relaxed);
        if set == 0 || (last != 0 && now - last < 5000) {
            return;
        }
        LAST.store(now, Relaxed);
        cvr_host::log::write(&format!(
            "[complend] set={set} lends={} skips={} | elem: skipped={} leafNulls={}\n",
            CyberpunkVR_DebugCompLendWrites.load(Relaxed),
            CyberpunkVR_DebugCompLendSkips.load(Relaxed),
            CyberpunkVR_DebugCompElemSkips.load(Relaxed),
            CyberpunkVR_DebugCompElemNulls.load(Relaxed)
        ));
    }
}
