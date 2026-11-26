//! THE ENGINE'S CULLING PER VIEW (the culling half of formerly src/Stereo/CullVisibility.cpp): the gather context's
//! detail threshold observed (and overridable per view, live), the software-occlusion gate forced on for every view
//! that tests occlusion, the frame-graph context's preparation -- the paired preparation of NativePreparation.cpp
//! around it, and the experimental cull reuse modes (6: MAIN consumes the second view's preserved cull output; 1/5,
//! 2: a view's cull skipped; 4: the second view's local occlusion context nulled) --, and DoCulling.
//!
//! And the PrepareRenderElements stage (the eager depth, NativePreparation's stage tickets, the stage profiler and
//! the descriptor cache audit), the cull callbacks' profile, the second view's visibility captured and replayed for
//! MAIN (modes 7-9), the fine materialize and the materialize worker.
//!
//! CullVisibility.cpp keeps the exports, the trampolines and the registry lines, and hands this file what it shares
//! with the rest of the stereo module.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use cvr_host::fast::FastMap;
use core::cell::Cell;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_crt::{format, Arg};
use cvr_engine::code::{read, write};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_vrcam_ctx_key: AtomicU64;
    static g_menuModeValue: i32;
    static CyberpunkVR_LodThreshOverrideEnable: AtomicU32;
    static CyberpunkVR_LodThreshValue: AtomicU32;
    static CyberpunkVR_LodThreshApplyMask: AtomicU32;
    static CyberpunkVR_DebugLodThreshHitsMain: AtomicU64;
    static CyberpunkVR_DebugLodThreshHitsVrcam: AtomicU64;
    static CyberpunkVR_DebugLodThreshHitsOther: AtomicU64;
    static CyberpunkVR_DebugLodThreshSeenMainBits: AtomicU32;
    static CyberpunkVR_DebugLodThreshSeenVrcamBits: AtomicU32;
    static CyberpunkVR_OcclusionGateForce: AtomicU32;
    static CyberpunkVR_DebugOcclGateForced: AtomicU64;
    static CyberpunkVR_DebugOcclGateAlreadyOn: AtomicU64;
    static CyberpunkVR_CullReuseMode: AtomicU32;
    static CyberpunkVR_DebugCullSkipHits: AtomicU64;
    static CyberpunkVR_DebugLocalCtxZeroHits: AtomicU64;
    static CyberpunkVR_DebugContainerRedirectHits: AtomicU64;
    static CyberpunkVR_DebugMainContainer: AtomicUsize;
    static CyberpunkVR_DebugVrcamContainer: AtomicUsize;
    static CyberpunkVR_DebugVisibilityResetSkipHits: AtomicU64;
    static CyberpunkVR_DebugEndRenderResetSkipHits: AtomicU64;
    static CyberpunkVR_DebugMainCullReuseHits: AtomicU64;
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
fn cull_reuse_mode() -> u32 {
    // SAFETY: an export.
    unsafe { CyberpunkVR_CullReuseMode.load(Relaxed) }
}
fn vrcam_key() -> u64 {
    // SAFETY: an export.
    unsafe { g_vrcam_ctx_key.load(SeqCst) }
}

/// The trampolines MinHook writes, registered from engine_detours.rs: the gather context's init, the visibility
/// query's prepare, the multifrustum worker, the graph context's reset, the preparation's stage, gather, filter,
/// finalize and four sorts, the main cull's prepare, the visibility collector, the materialize worker and the visible
/// append (the DoCulling, graph context prepare, main cull test and fine materialize originals stay the .cpp's: other
/// files call through them).
#[no_mangle]
pub static cvr_cull_originals: [core::sync::atomic::AtomicUsize; 16] = [const { core::sync::atomic::AtomicUsize::new(0) }; 16];
/// The trampolines of DoCulling, GraphContextPrepare, MainCullTest and FineMaterialize (registered from
/// engine_detours.rs; CullVisibility.cpp's PreparePair and the CPU collector probe call through them too).
#[no_mangle]
pub static cvr_cull_stage_originals: [core::sync::atomic::AtomicUsize; 4] = [const { core::sync::atomic::AtomicUsize::new(0) }; 4];

/// What CullVisibility.cpp shares with these detours.
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    main_view_ctx: *const AtomicUsize,
    vrcam_ctx_seen: *const AtomicUsize,
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    main_view_ctx: &raw const m::g_main_view_ctx,
    vrcam_ctx_seen: &raw const m::g_vrcam_ctx_seen,
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
/// is_main_view (SyncStereo.cpp's, over the main view's cached context).
fn is_main_view(view: usize) -> bool {
    // SAFETY: the C++ half's live atomic; the view is the engine's.
    unsafe { crate::main_view_check::cvr_ss_is_main_view(view as *const u8, (*rt().main_view_ctx).load(Acquire)) }
}

/// One line every 5 s: the threshold each view is handed (1.0 keeps everything; smaller drops detail earlier).
#[no_mangle]
pub extern "C" fn cvr_cull_lod_thresh_report() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // SAFETY: a plain system query.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let due = NEXT.load(Relaxed);
    if now < due
        || NEXT
            .compare_exchange(due, now + 5000, Relaxed, Relaxed)
            .is_err()
    {
        return;
    }
    // SAFETY: exports of the C++ half.
    let line = unsafe {
        let (bm, bv) = (
            CyberpunkVR_DebugLodThreshSeenMainBits.load(Relaxed),
            CyberpunkVR_DebugLodThreshSeenVrcamBits.load(Relaxed),
        );
        format(
            c"[lod] gather-ctx+0x28 detail threshold -- MAIN %.4f (0x%08X)  VRCAM %.4f (0x%08X)  applyMask=%u value=%.4f  hits M/V/other %llu/%llu/%llu",
            &[
                Arg::F(f32::from_bits(bm) as f64),
                Arg::U(bm as u64),
                Arg::F(f32::from_bits(bv) as f64),
                Arg::U(bv as u64),
                Arg::U(CyberpunkVR_LodThreshApplyMask.load(Relaxed) as u64),
                Arg::F(f32::from_bits(CyberpunkVR_LodThreshValue.load(Relaxed)) as f64),
                Arg::U(CyberpunkVR_DebugLodThreshHitsMain.load(Relaxed)),
                Arg::U(CyberpunkVR_DebugLodThreshHitsVrcam.load(Relaxed)),
                Arg::U(CyberpunkVR_DebugLodThreshHitsOther.load(Relaxed)),
            ],
        )
    };
    (rt().stereo_log)(line.as_ptr());
}
/// The shared gather context's build (sub_142286CC0(ctx, view, cull query, flag)): ctx + 0x28 is the detail threshold.
/// Observed per view, and overwritten for the views in CyberpunkVR_LodThreshApplyMask (b0 MAIN, b1 the second view,
/// b2 any other).
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_gather_ctx_init(
    ctx: *mut c_void,
    view: usize,
    cull_query: *mut c_void,
    flag: u8,
) -> *mut c_void {
    let original: unsafe extern "C" fn(*mut c_void, usize, *mut c_void, u8) -> *mut c_void =
        core::mem::transmute(cvr_cull_originals[0].load(Acquire));
    let result = original(ctx, view, cull_query, flag);
    if CyberpunkVR_LodThreshOverrideEnable.load(Relaxed) != 0 && !ctx.is_null() && view != 0 {
        let _ = (|| -> Option<()> {
            let key: u64 = read(view + 0x28)?;
            let _aspect: f32 = read(view + 0x98)?;
            let seen: u32 = read(ctx as usize + 0x28)?;
            let bit = if key == vrcam_key() {
                CyberpunkVR_DebugLodThreshSeenVrcamBits.store(seen, Relaxed);
                2
            } else if is_main_view(view) {
                CyberpunkVR_DebugLodThreshSeenMainBits.store(seen, Relaxed);
                1
            } else {
                4
            };
            if CyberpunkVR_LodThreshApplyMask.load(Relaxed) & bit != 0 {
                write(
                    ctx as usize + 0x28,
                    CyberpunkVR_LodThreshValue.load(Relaxed),
                )
                .then_some(())?;
                diag(match bit {
                    2 => &CyberpunkVR_DebugLodThreshHitsVrcam,
                    1 => &CyberpunkVR_DebugLodThreshHitsMain,
                    _ => &CyberpunkVR_DebugLodThreshHitsOther,
                });
            }
            cvr_cull_lod_thresh_report();
            Some(())
        })();
    }
    result
}
/// visQuerySingleFrustum's per-view prepare (sub_1405AC174): the whole CPU software-occlusion pipeline is gated on
/// tester + 0x19E (MAIN has it, the second view not); forced on, each view rasterises its own occluder depth from its
/// own frustum.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_visquery_prepare(tester: *mut c_void, job: *mut c_void) -> i64 {
    if CyberpunkVR_OcclusionGateForce.load(Relaxed) != 0 && !tester.is_null() {
        let gate = tester as usize + 0x19E;
        match read::<u8>(gate) {
            Some(0) => {
                if write(gate, 1u8) {
                    diag(&CyberpunkVR_DebugOcclGateForced);
                }
            }
            Some(_) => diag(&CyberpunkVR_DebugOcclGateAlreadyOn),
            None => {}
        }
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_cull_originals[1].load(Acquire));
    original(tester, job)
}

/// Mode 6's state: the second view's graph container preserved through MAIN's preparation, and MAIN armed to reuse
/// its cull output.
const VIS_REUSE_IDLE: u32 = 0;
const VIS_REUSE_VRCAM_ACTIVE: u32 = 1;
const VIS_REUSE_VRCAM_PRESERVED: u32 = 2;
const VIS_REUSE_MAIN: u32 = 3;
static MAIN_REUSE_ARMED: AtomicBool = AtomicBool::new(false);
static REUSE_PHASE: AtomicU32 = AtomicU32::new(VIS_REUSE_IDLE);
thread_local! {
    static PRESERVE_VRCAM_GRAPH: Cell<bool> = const { Cell::new(false) };
    static PRESERVE_CONTAINER: Cell<usize> = const { Cell::new(0) };
}

/// The frame-graph context's preparation (sub_140423180): NativePreparation's paired preparation around the original,
/// the prepared view noted for the graph-cache key; under mode 6 the second view's container is preserved when MAIN
/// rebuilds the manager after it.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_graph_context_prepare(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) -> *mut c_void {
    let r = rt();
    crate::preparation_trace::cvr_prep_graph(a1 as usize as u64, false);
    crate::render_parity::cvr_rp_clear_prepared_graph();
    let previous_preserve = PRESERVE_VRCAM_GRAPH.replace(false);
    let previous_container = PRESERVE_CONTAINER.replace(0);
    if cull_reuse_mode() == 6 && !a1.is_null() {
        let _ = (|| -> Option<()> {
            let count: u32 = read(a1 as usize + 0x54)?;
            let views: usize = read(a1 as usize + 0x48)?;
            let old_view: usize = if count == 1 && views != 0 {
                read(views)?
            } else {
                0
            };
            if old_view == 0 {
                return Some(());
            }
            let key: u64 = read(old_view + 0x28)?;
            let container: usize = read(old_view + 0x2C40)?;
            if key == vrcam_key() && container != 0 {
                // The prepare now rebuilding this manager is MAIN's: its reset must not destroy the second view's
                // cull and draw payload MAIN is to consume.
                PRESERVE_VRCAM_GRAPH.set(true);
                PRESERVE_CONTAINER.set(container);
                MAIN_REUSE_ARMED.store(false, Release);
                CyberpunkVR_DebugVrcamContainer.store(container, Relaxed);
            } else if is_main_view(old_view) {
                // MAIN -> second view: the normal reset before the second view writes a fresh frame.
                MAIN_REUSE_ARMED.store(false, Release);
                REUSE_PHASE.store(VIS_REUSE_IDLE, Release);
            }
            Some(())
        })();
    } else {
        MAIN_REUSE_ARMED.store(false, Release);
        REUSE_PHASE.store(VIS_REUSE_IDLE, Release);
    }
    let gameplay = core::ptr::read_volatile(&raw const g_menuModeValue) == 0
        && cull_reuse_mode() == 0
        && crate::preparation_trace::cvr_prep_pair_compatible();
    let result = crate::preparation_trace::cvr_prep_prepare_pair(
        core::mem::transmute::<usize, Option<crate::preparation_trace::PrepareGraph>>(cvr_cull_stage_originals[1].load(core::sync::atomic::Ordering::Relaxed)),
        a1,
        a2,
        a3,
        g_vrcam_ctx_key.load(Acquire),
        (*r.main_view_ctx).load(Acquire) as u64,
        gameplay,
        (*r.vrcam_ctx_seen).load(Acquire) as u64,
    );
    crate::preparation_trace::cvr_prep_graph(a1 as usize as u64, true);
    crate::render_parity::cvr_rp_observe_prepared_graph(a1 as usize);
    PRESERVE_VRCAM_GRAPH.set(previous_preserve);
    PRESERVE_CONTAINER.set(previous_container);
    result
}
/// The graph container's reset (sub_1404ECEA0), entered with its caller's return address in rdx.
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_graph_context_reset(container: *mut c_void) -> i64 {
    core::arch::naked_asm!("mov rdx, [rsp]", "jmp {body}", body = sym graph_context_reset);
}
/// Mode 6: the reset skipped at the second view's EndRender (sub_14079A804's call, return 0x8EB20A) and for the
/// container MAIN's preparation preserves; MAIN's own EndRender afterwards performs the normal cleanup.
unsafe extern "C" fn graph_context_reset(container: *mut c_void, return_address: usize) -> i64 {
    let base = *rt().exe_base;
    let return_rva = if base != 0 {
        return_address.wrapping_sub(base)
    } else {
        0
    };
    let phase = REUSE_PHASE.load(Acquire);
    let mode6 = cull_reuse_mode() == 6;
    let preserve_end_render = mode6 && return_rva == 0x8EB20A && phase == VIS_REUSE_VRCAM_ACTIVE;
    let preserve_main_prepare =
        mode6 && PRESERVE_VRCAM_GRAPH.get() && container as usize == PRESERVE_CONTAINER.get();
    if preserve_end_render || preserve_main_prepare {
        diag(&CyberpunkVR_DebugVisibilityResetSkipHits);
        if preserve_end_render {
            diag(&CyberpunkVR_DebugEndRenderResetSkipHits);
        }
        // The legacy counter, kept for live telemetry.
        diag(&CyberpunkVR_DebugContainerRedirectHits);
        if preserve_end_render {
            REUSE_PHASE.store(VIS_REUSE_VRCAM_PRESERVED, Release);
        } else {
            REUSE_PHASE.store(VIS_REUSE_MAIN, Release);
            MAIN_REUSE_ARMED.store(true, Release);
        }
        return 0;
    }
    if mode6 && return_rva == 0x8EB20A && phase == VIS_REUSE_MAIN {
        MAIN_REUSE_ARMED.store(false, Release);
        REUSE_PHASE.store(VIS_REUSE_IDLE, Release);
    }
    let original: unsafe extern "C" fn(*mut c_void) -> i64 =
        core::mem::transmute(cvr_cull_originals[3].load(Acquire));
    original(container)
}
/// Modes 1/5 skip the second view's cull, mode 2 MAIN's gameplay cull (diagnostic).
fn doculling_skips(a2: *mut c_void) -> bool {
    let mode = cull_reuse_mode();
    if mode == 0 || a2.is_null() {
        return false;
    }
    (|| -> Option<bool> {
        let ctx: usize = read(a2 as usize + 0x18)?;
        if ctx == 0 {
            return Some(false);
        }
        let key: u64 = read(ctx + 0x28)?;
        Some(match mode {
            1 | 5 => key == vrcam_key(),
            2 => is_main_view(ctx),
            _ => false,
        })
    })()
    .unwrap_or(false)
}
/// The view side of a node work object: 1 the second view, 0 MAIN, -1 another.
fn work_side(work: usize) -> i32 {
    (|| -> Option<i32> {
        let view: usize = read(work + 0x18)?;
        if view == 0 {
            return Some(-1);
        }
        Some(if read::<u64>(view + 0x28)? == vrcam_key() {
            1
        } else if is_main_view(view) {
            0
        } else {
            -1
        })
    })()
    .unwrap_or(-1)
}
/// CRenderNode_DoCulling (sub_140BF436C): NativePreparation's cull around the original while it is active; mode 6
/// lets MAIN consume the second view's preserved output; modes 1/5 and 2 skip a view's cull.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_doculling(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) -> u8 {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> u8 =
        core::mem::transmute(cvr_cull_stage_originals[0].load(core::sync::atomic::Ordering::Relaxed));
    if crate::preparation_trace::active() && cull_reuse_mode() == 0 && !a2.is_null() {
        let side = work_side(a2 as usize);
        crate::preparation_trace::cvr_prep_cull(a2 as usize as u64, a3 as usize as u64, side, false, a1 as usize as u64);
        let result = original(a1, a2, a3);
        crate::preparation_trace::cvr_prep_cull(a2 as usize as u64, a3 as usize as u64, side, true, a1 as usize as u64);
        return result;
    }
    if cull_reuse_mode() == 6 && !a2.is_null() {
        let reused = (|| -> Option<bool> {
            let view: usize = read(a2 as usize + 0x18)?;
            if view == 0 {
                return Some(false);
            }
            let key: u64 = read(view + 0x28)?;
            let _aspect: f32 = read(view + 0x98)?;
            if key == vrcam_key() {
                REUSE_PHASE.store(VIS_REUSE_VRCAM_ACTIVE, Release);
            }
            if is_main_view(view)
                && MAIN_REUSE_ARMED.load(Acquire)
                && REUSE_PHASE.load(Acquire) == VIS_REUSE_MAIN
            {
                CyberpunkVR_DebugMainContainer.store(read(view + 0x2C40)?, Relaxed);
                diag(&CyberpunkVR_DebugMainCullReuseHits);
                diag(&CyberpunkVR_DebugCullSkipHits);
                return Some(true);
            }
            Some(false)
        })()
        .unwrap_or(false);
        if reused {
            return 0;
        }
    }
    if doculling_skips(a2) {
        diag(&CyberpunkVR_DebugCullSkipHits);
        return 0;
    }
    original(a1, a2, a3)
}
/// The multifrustum worker (sub_14029C158): mode 4 runs it with the second view's local occlusion context
/// (query + 0x348, the one stable difference from MAIN) nulled, every invocation.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_querywork(query: *mut c_void, a2: *mut c_void) -> i64 {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_cull_originals[2].load(Acquire));
    if cull_reuse_mode() == 4 && !query.is_null() {
        let slot = query as usize + 0x348;
        let saved: usize = read(slot).unwrap_or(0);
        if saved != 0 && write(slot, 0usize) {
            diag(&CyberpunkVR_DebugLocalCtxZeroHits);
            let result = original(query, a2);
            write(slot, saved);
            return result;
        }
    }
    original(query, a2)
}

// ---- the PrepareRenderElements stage profiler (diagnostic, default off) and the stage's eager depth

use crate::stereo_profiler::cvr_stereo_prof_now as prof_now;
use std::sync::{LazyLock, Mutex};

const PREPARE_STAGE_MAX: u32 = 64;
const PREPARE_MODE_MAX: u32 = 4;
const PREPARE_STAGE_MODE_COUNT: usize = (PREPARE_STAGE_MAX * PREPARE_MODE_MAX) as usize;
const PREPARE_HIST_BUCKETS: usize = 10;
const PREPARE_TOP_BUCKETS: usize = 24;
const CULL_CALLBACK_MAX: usize = 128;

extern "C" {
    static CyberpunkVR_PrepareProfileEnable: AtomicU32;
    static CyberpunkVR_PrepareCacheAuditEnable: AtomicU32;
    static CyberpunkVR_DebugPrepareStageTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareStageTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareGatherTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareGatherTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareFilterTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareFilterTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareFinalizeTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareFinalizeTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareSortATicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareSortATicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareSortBTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareSortBTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareSortCTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareSortCTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareSortFinalTicksMain: AtomicU64;
    static CyberpunkVR_DebugPrepareSortFinalTicksVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareCallsMain: AtomicU64;
    static CyberpunkVR_DebugPrepareCallsVrcam: AtomicU64;
    static CyberpunkVR_DebugPrepareTopBucketCount: AtomicU32;
    static CyberpunkVR_DebugPrepareTopBucketStage: [AtomicU32; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketMode: [AtomicU32; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketTicksMain: [AtomicU64; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketTicksVrcam: [AtomicU64; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketDescMain: [AtomicU64; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketDescVrcam: [AtomicU64; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketCallsMain: [AtomicU32; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareTopBucketCallsVrcam: [AtomicU32; PREPARE_TOP_BUCKETS];
    static CyberpunkVR_DebugPrepareHistCallsMain: [AtomicU64; PREPARE_HIST_BUCKETS];
    static CyberpunkVR_DebugPrepareHistCallsVrcam: [AtomicU64; PREPARE_HIST_BUCKETS];
    static CyberpunkVR_DebugPrepareHistDescMain: [AtomicU64; PREPARE_HIST_BUCKETS];
    static CyberpunkVR_DebugPrepareHistDescVrcam: [AtomicU64; PREPARE_HIST_BUCKETS];
    static CyberpunkVR_DebugPrepareHistTicksMain: [AtomicU64; PREPARE_HIST_BUCKETS];
    static CyberpunkVR_DebugPrepareHistTicksVrcam: [AtomicU64; PREPARE_HIST_BUCKETS];
    static CyberpunkVR_DebugPrepareBucketCallsMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketCallsVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketDescMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketDescVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketTicksMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketTicksVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortATicksMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortATicksVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortBTicksMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortBTicksVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortCTicksMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortCTicksVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortFinalTicksMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketSortFinalTicksVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketCountSqMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareBucketCountSqVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareFilterInMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareFilterInVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareFilterOutMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareFilterOutVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareFilterCallsMain: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareFilterCallsVrcam: [AtomicU64; PREPARE_STAGE_MODE_COUNT];
    static CyberpunkVR_DebugPrepareCacheHits: AtomicU64;
    static CyberpunkVR_DebugPrepareCacheMisses: AtomicU64;
    static CyberpunkVR_DebugPrepareCacheHitDescriptors: AtomicU64;
    static CyberpunkVR_DebugPrepareSetHits: AtomicU64;
    static CyberpunkVR_DebugPrepareSetMisses: AtomicU64;
    static CyberpunkVR_DebugPrepareSetHitDescriptors: AtomicU64;
    static CyberpunkVR_CullCallbackProfileEnable: AtomicU32;
    static CyberpunkVR_DebugCullCallbackMethodRva: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugCullCallbackCallsMain: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugCullCallbackCallsVrcam: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugCullCallbackDescMain: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugCullCallbackDescVrcam: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugCullCallbackTicksMain: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugCullCallbackTicksVrcam: [AtomicU64; CULL_CALLBACK_MAX];
    static CyberpunkVR_DebugMainCullPrepareSkips: AtomicU64;
    static CyberpunkVR_DebugVisibilityBatchCaptures: AtomicU64;
    static CyberpunkVR_DebugVisibilityBatchReplays: AtomicU64;
    static CyberpunkVR_DebugVisibilityCandidatesCaptured: AtomicU64;
    static CyberpunkVR_DebugVisibilityCandidatesReplayed: AtomicU64;
    static CyberpunkVR_DebugFineCandidateCaptures: AtomicU64;
    static CyberpunkVR_DebugFineCandidateReplays: AtomicU64;
    static CyberpunkVR_DebugFineCandidateFallbacks: AtomicU64;
    static CyberpunkVR_DebugFineDrawableIdsCaptured: AtomicU64;
    static CyberpunkVR_DebugFineDrawableIdsReplayed: AtomicU64;
    static CyberpunkVR_MaterializeProfileEnable: AtomicU32;
    static CyberpunkVR_DebugMaterializeWorkerTasksMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeWorkerTasksVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeTaggedMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeTaggedVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeFineCallsMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeFineCallsVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeFineRangesMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeFineRangesVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeDrawableAppendsMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeDrawableAppendsVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeUniqueMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeUniqueVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeDuplicateMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeDuplicateVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeProbeOverflowMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeProbeOverflowVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeCtxUniqueMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeCtxUniqueVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeCtxDuplicateMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeCtxDuplicateVrcam: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowMain: AtomicU64;
    static CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowVrcam: AtomicU64;
}
const KIND_GATHER: u32 = 2;
const KIND_FILTER: u32 = 3;
const KIND_FINALIZE: u32 = 4;
/// sub_142286CC0, the gather context's build.
const MAIN_CULL_CTX_INIT_RVA: usize = 0x2286CC0;

thread_local! {
    /// 1 the second view, 2 MAIN's gameplay view.
    static PREPARE_VIEW_KIND: Cell<u32> = const { Cell::new(0) };
    static PREPARE_BUCKET_KEY: Cell<usize> = const { Cell::new(0) };
    static PREPARE_STAGE_ID: Cell<u32> = const { Cell::new(0) };
    static PREPARE_MODE: Cell<u8> = const { Cell::new(0) };
}
fn prepare_enabled() -> (bool, bool) {
    // SAFETY: exports.
    unsafe {
        (
            CyberpunkVR_PrepareProfileEnable.load(Relaxed) != 0,
            CyberpunkVR_PrepareCacheAuditEnable.load(Relaxed) != 0,
        )
    }
}
fn prepare_hist_bucket(count: u32) -> usize {
    match count {
        0..64 => 0,
        64..128 => 1,
        128..256 => 2,
        256..512 => 3,
        512..1024 => 4,
        1024..2048 => 5,
        2048..4096 => 6,
        4096..8192 => 7,
        8192..16384 => 8,
        _ => 9,
    }
}
fn prepare_stage_mode_index(stage: u32, mode: u8) -> Option<usize> {
    (stage < PREPARE_STAGE_MAX && (mode as u32) < PREPARE_MODE_MAX)
        .then(|| (stage * PREPARE_MODE_MAX + mode as u32) as usize)
}
fn prepare_bucket_add(main: &[AtomicU64], vrcam: &[AtomicU64], index: Option<usize>, value: u64) {
    let Some(i) = index else { return };
    match PREPARE_VIEW_KIND.get() {
        1 => vrcam[i].fetch_add(value, SeqCst),
        2 => main[i].fetch_add(value, SeqCst),
        _ => 0,
    };
}
fn prepare_prof_add(main: &AtomicU64, vrcam: &AtomicU64, ticks: i64) {
    match PREPARE_VIEW_KIND.get() {
        1 => vrcam.fetch_add(ticks as u64, SeqCst),
        2 => main.fetch_add(ticks as u64, SeqCst),
        _ => 0,
    };
}
/// FNV-1a.
fn prepare_descriptor_hash(data: &[u8]) -> u64 {
    data.iter().fold(1_469_598_103_934_665_603u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1_099_511_628_211)
    })
}
/// splitmix64's finaliser.
fn prepare_mix64(mut v: u64) -> u64 {
    v ^= v >> 30;
    v = v.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    v ^= v >> 27;
    v = v.wrapping_mul(0x94D0_49BB_1331_11EB);
    v ^ (v >> 31)
}

struct FinalizeBucket {
    key: AtomicU64,
    ticks_main: AtomicU64,
    ticks_vrcam: AtomicU64,
    desc_main: AtomicU64,
    desc_vrcam: AtomicU64,
    calls_main: AtomicU32,
    calls_vrcam: AtomicU32,
}
static FINALIZE_BUCKETS: [FinalizeBucket; 128] = [const {
    FinalizeBucket {
        key: AtomicU64::new(0),
        ticks_main: AtomicU64::new(0),
        ticks_vrcam: AtomicU64::new(0),
        desc_main: AtomicU64::new(0),
        desc_vrcam: AtomicU64::new(0),
        calls_main: AtomicU32::new(0),
        calls_vrcam: AtomicU32::new(0),
    }
}; 128];
fn prepare_finalize_bucket(stage: u32, mode: u8) -> Option<&'static FinalizeBucket> {
    let want = ((mode as u64) << 32) | stage as u64 | 1;
    let base = (stage.wrapping_mul(131).wrapping_add((mode as u32) * 17) & 127) as usize;
    for i in 0..128 {
        let slot = &FINALIZE_BUCKETS[(base + i) & 127];
        let current = slot.key.load(Acquire);
        if current == want
            || (current == 0 && slot.key.compare_exchange(0, want, AcqRel, Acquire).is_ok())
        {
            return Some(slot);
        }
    }
    None
}
/// The finalize buckets ranked by total ticks into the top-24 exports, and drained.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_dump_prepare_finalize_buckets() {
    struct Row {
        stage: u32,
        mode: u8,
        tm: u64,
        tv: u64,
        dm: u64,
        dv: u64,
        cm: u32,
        cv: u32,
    }
    let mut rows: Vec<Row> = Vec::new();
    for slot in &FINALIZE_BUCKETS {
        let key = slot.key.load(Relaxed);
        if key == 0 {
            continue;
        }
        let row = Row {
            stage: ((key & 0xFFFF_FFFF) - 1) as u32,
            mode: (key >> 32) as u8,
            tm: slot.ticks_main.swap(0, Relaxed),
            tv: slot.ticks_vrcam.swap(0, Relaxed),
            dm: slot.desc_main.swap(0, Relaxed),
            dv: slot.desc_vrcam.swap(0, Relaxed),
            cm: slot.calls_main.swap(0, Relaxed),
            cv: slot.calls_vrcam.swap(0, Relaxed),
        };
        if row.tm | row.tv | row.dm | row.dv | row.cm as u64 | row.cv as u64 != 0 {
            rows.push(row);
        }
    }
    // Selection sort by total ticks, as the C++ half ranked them (stable for equal totals the same way).
    for i in 0..rows.len() {
        let mut best = i;
        for j in i + 1..rows.len() {
            if (rows[j].tm as i64).wrapping_add(rows[j].tv as i64)
                > (rows[best].tm as i64).wrapping_add(rows[best].tv as i64)
            {
                best = j;
            }
        }
        rows.swap(i, best);
    }
    for i in 0..PREPARE_TOP_BUCKETS {
        CyberpunkVR_DebugPrepareTopBucketStage[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketMode[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketTicksMain[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketTicksVrcam[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketDescMain[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketDescVrcam[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketCallsMain[i].store(0, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketCallsVrcam[i].store(0, Relaxed);
    }
    CyberpunkVR_DebugPrepareTopBucketCount.store(0, Relaxed);
    let limit = rows.len().min(PREPARE_TOP_BUCKETS);
    for (i, r) in rows.iter().take(limit).enumerate() {
        CyberpunkVR_DebugPrepareTopBucketStage[i].store(r.stage, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketMode[i].store(r.mode as u32, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketTicksMain[i].store(r.tm, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketTicksVrcam[i].store(r.tv, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketDescMain[i].store(r.dm, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketDescVrcam[i].store(r.dv, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketCallsMain[i].store(r.cm, Relaxed);
        CyberpunkVR_DebugPrepareTopBucketCallsVrcam[i].store(r.cv, Relaxed);
    }
    CyberpunkVR_DebugPrepareTopBucketCount.store(limit as u32, Relaxed);
}

#[derive(Default)]
struct CacheAuditEntry {
    hash: u64,
    set_sum: u64,
    set_xor: u64,
    set_count: u32,
    input: Vec<u8>,
}
static CACHE_AUDIT: LazyLock<Mutex<FastMap<usize, CacheAuditEntry>>> =
    LazyLock::new(|| Mutex::new(cvr_host::fast::fast_map()));

type PrepareStageFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32);
/// The PrepareRenderElements stage (sub_14229D3E0): the eager depth first (render.rs's NativeDepthPreparation), then
/// NativePreparation's stage ticket and the profiler's view kind, bucket key, stage and mode for the stage's calls.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_stage(
    stage_context: *mut c_void,
    buckets: *mut c_void,
    output: *mut c_void,
    flags0: u32,
    flags1: u32,
) {
    let original: PrepareStageFn = core::mem::transmute(cvr_cull_originals[4].load(Acquire));
    let (profile, audit) = prepare_enabled();
    if !profile
        && !audit
        && crate::eager_depth::cvr_eager_depth_intercept(
            core::mem::transmute(cvr_cull_originals[4].load(Acquire)),
            core::mem::transmute(cvr_cull_originals[5].load(Acquire)),
            core::mem::transmute(cvr_cull_originals[6].load(Acquire)),
            core::mem::transmute(cvr_cull_originals[7].load(Acquire)),
            stage_context,
            buckets,
            output,
            flags0,
            flags1,
        )
    {
        return;
    }
    if !profile && !audit && !crate::preparation_trace::active() {
        original(stage_context, buckets, output, flags0, flags1);
        return;
    }
    let previous = (
        PREPARE_VIEW_KIND.get(),
        PREPARE_BUCKET_KEY.get(),
        PREPARE_STAGE_ID.get(),
        PREPARE_MODE.get(),
    );
    PREPARE_VIEW_KIND.set(0);
    PREPARE_STAGE_ID.set(0);
    PREPARE_MODE.set(0);
    PREPARE_BUCKET_KEY.set(
        (prepare_mix64(buckets as u64) ^ prepare_mix64(flags0 as u64 | ((flags1 as u64) << 32)))
            as usize,
    );
    let _ = (|| -> Option<()> {
        let sc = stage_context as usize;
        if sc != 0 {
            PREPARE_STAGE_ID.set(read(sc + 0x14)?);
            PREPARE_MODE.set(read(sc + 0x18)?);
            let signature = PREPARE_STAGE_ID.get() as u64
                | ((PREPARE_MODE.get() as u64) << 32)
                | ((read::<u8>(sc + 0x19)? as u64) << 40);
            PREPARE_BUCKET_KEY.set(PREPARE_BUCKET_KEY.get() ^ prepare_mix64(signature) as usize);
        }
        let render_context: usize = read(sc)?;
        let view: usize = if render_context != 0 {
            read(render_context + 0x18)?
        } else {
            0
        };
        if view != 0 {
            if read::<u64>(view + 0x28)? == vrcam_key() {
                PREPARE_VIEW_KIND.set(1);
            } else if is_main_view(view) {
                PREPARE_VIEW_KIND.set(2);
            }
        }
        Some(())
    })();
    let side = match PREPARE_VIEW_KIND.get() {
        1 => 1,
        2 => 0,
        _ => -1,
    };
    let mut ticket = [0u64; 17];
    crate::preparation_trace::cvr_prep_begin_stage(
        stage_context as usize as u64,
        buckets as usize as u64,
        output as usize as u64,
        flags0,
        flags1,
        side,
        ticket.as_mut_ptr().cast(),
    );
    let t0 = if profile { prof_now() } else { 0 };
    original(stage_context, buckets, output, flags0, flags1);
    if profile {
        prepare_prof_add(
            &CyberpunkVR_DebugPrepareStageTicksMain,
            &CyberpunkVR_DebugPrepareStageTicksVrcam,
            prof_now() - t0,
        );
        match PREPARE_VIEW_KIND.get() {
            1 => diag(&CyberpunkVR_DebugPrepareCallsVrcam),
            2 => diag(&CyberpunkVR_DebugPrepareCallsMain),
            _ => {}
        }
    }
    crate::preparation_trace::cvr_prep_end_stage(ticket.as_ptr().cast());
    PREPARE_VIEW_KIND.set(previous.0);
    PREPARE_BUCKET_KEY.set(previous.1);
    PREPARE_STAGE_ID.set(previous.2);
    PREPARE_MODE.set(previous.3);
}
fn profiling() -> bool {
    prepare_enabled().0 && PREPARE_VIEW_KIND.get() != 0
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_gather(
    buckets: *mut c_void,
    output: *mut c_void,
) -> *mut c_void {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        core::mem::transmute(cvr_cull_originals[5].load(Acquire));
    let t0 = if profiling() { Some(prof_now()) } else { None };
    let result = original(buckets, output);
    if let Some(t0) = t0 {
        prepare_prof_add(
            &CyberpunkVR_DebugPrepareGatherTicksMain,
            &CyberpunkVR_DebugPrepareGatherTicksVrcam,
            prof_now() - t0,
        );
    }
    crate::preparation_trace::cvr_prep_output(KIND_GATHER, output as usize as u64);
    result
}
fn output_count(output: *mut c_void) -> u32 {
    if output.is_null() {
        0
    } else {
        read(output as usize + 0x0C).unwrap_or(0)
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_filter(
    output: *mut c_void,
    mode: i8,
    flags0: u32,
    flags1: u32,
) -> i64 {
    let original: unsafe extern "C" fn(*mut c_void, i8, u32, u32) -> i64 =
        core::mem::transmute(cvr_cull_originals[6].load(Acquire));
    if !profiling() {
        let result = original(output, mode, flags0, flags1);
        crate::preparation_trace::cvr_prep_output(KIND_FILTER, output as usize as u64);
        return result;
    }
    let bucket = prepare_stage_mode_index(PREPARE_STAGE_ID.get(), mode as u8);
    prepare_bucket_add(
        &CyberpunkVR_DebugPrepareFilterInMain,
        &CyberpunkVR_DebugPrepareFilterInVrcam,
        bucket,
        output_count(output) as u64,
    );
    prepare_bucket_add(
        &CyberpunkVR_DebugPrepareFilterCallsMain,
        &CyberpunkVR_DebugPrepareFilterCallsVrcam,
        bucket,
        1,
    );
    let t0 = prof_now();
    let result = original(output, mode, flags0, flags1);
    prepare_bucket_add(
        &CyberpunkVR_DebugPrepareFilterOutMain,
        &CyberpunkVR_DebugPrepareFilterOutVrcam,
        bucket,
        output_count(output) as u64,
    );
    prepare_prof_add(
        &CyberpunkVR_DebugPrepareFilterTicksMain,
        &CyberpunkVR_DebugPrepareFilterTicksVrcam,
        prof_now() - t0,
    );
    crate::preparation_trace::cvr_prep_output(KIND_FILTER, output as usize as u64);
    result
}
/// The finalize: the cache audit (the second view's mode-1 descriptor sets compared with the last of the same bucket,
/// byte-exact and as a set), the profiler's histograms and buckets, NativePreparation's output.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_finalize(
    output: *mut c_void,
    mode: i8,
    a3: i64,
    a4: i64,
) {
    let original: unsafe extern "C" fn(*mut c_void, i8, i64, i64) =
        core::mem::transmute(cvr_cull_originals[7].load(Acquire));
    let (profile, audit) = prepare_enabled();
    let kind = PREPARE_VIEW_KIND.get();
    if audit && kind == 1 && mode == 1 && PREPARE_BUCKET_KEY.get() != 0 && !output.is_null() {
        let count = output_count(output);
        let data: usize = read(output as usize).unwrap_or(0);
        if count != 0 && data != 0 {
            let mut bytes = vec![0u8; count as usize * 16];
            if cvr_engine::code::read_bytes(data, &mut bytes) {
                let hash = prepare_descriptor_hash(&bytes);
                let (mut set_sum, mut set_xor) = (0u64, 0u64);
                for item in bytes.chunks_exact(16) {
                    let a = u64::from_ne_bytes(item[..8].try_into().unwrap());
                    let b = u64::from_ne_bytes(item[8..].try_into().unwrap());
                    let item_hash =
                        prepare_mix64(a) ^ prepare_mix64(b.wrapping_add(0x9E37_79B9_7F4A_7C15));
                    set_sum = set_sum.wrapping_add(item_hash);
                    set_xor ^= prepare_mix64(item_hash.wrapping_add(0xD6E8_FEB8_6659_FD93));
                }
                let (hit, set_hit) = {
                    let mut audit = CACHE_AUDIT.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = audit.entry(PREPARE_BUCKET_KEY.get()).or_default();
                    let hit = entry.hash == hash && entry.input == bytes;
                    let set_hit = entry.set_count == count
                        && entry.set_sum == set_sum
                        && entry.set_xor == set_xor;
                    *entry = CacheAuditEntry {
                        hash,
                        set_sum,
                        set_xor,
                        set_count: count,
                        input: bytes,
                    };
                    (hit, set_hit)
                };
                if diagnostics() {
                    if hit {
                        CyberpunkVR_DebugPrepareCacheHits.fetch_add(1, SeqCst);
                        CyberpunkVR_DebugPrepareCacheHitDescriptors.fetch_add(count as u64, SeqCst);
                    } else {
                        CyberpunkVR_DebugPrepareCacheMisses.fetch_add(1, SeqCst);
                    }
                    if set_hit {
                        CyberpunkVR_DebugPrepareSetHits.fetch_add(1, SeqCst);
                        CyberpunkVR_DebugPrepareSetHitDescriptors.fetch_add(count as u64, SeqCst);
                    } else {
                        CyberpunkVR_DebugPrepareSetMisses.fetch_add(1, SeqCst);
                    }
                }
            }
        }
    }
    let stage = PREPARE_STAGE_ID.get();
    if profile && kind != 0 && !output.is_null() {
        let count = output_count(output);
        let bucket = prepare_stage_mode_index(stage, mode as u8);
        let hist = Some(prepare_hist_bucket(count));
        prepare_bucket_add(
            &CyberpunkVR_DebugPrepareHistCallsMain,
            &CyberpunkVR_DebugPrepareHistCallsVrcam,
            hist,
            1,
        );
        prepare_bucket_add(
            &CyberpunkVR_DebugPrepareHistDescMain,
            &CyberpunkVR_DebugPrepareHistDescVrcam,
            hist,
            count as u64,
        );
        prepare_bucket_add(
            &CyberpunkVR_DebugPrepareBucketCallsMain,
            &CyberpunkVR_DebugPrepareBucketCallsVrcam,
            bucket,
            1,
        );
        prepare_bucket_add(
            &CyberpunkVR_DebugPrepareBucketDescMain,
            &CyberpunkVR_DebugPrepareBucketDescVrcam,
            bucket,
            count as u64,
        );
        prepare_bucket_add(
            &CyberpunkVR_DebugPrepareBucketCountSqMain,
            &CyberpunkVR_DebugPrepareBucketCountSqVrcam,
            bucket,
            count as u64 * count as u64,
        );
        if let Some(slot) = prepare_finalize_bucket(stage, mode as u8) {
            if kind == 1 {
                slot.desc_vrcam.fetch_add(count as u64, SeqCst);
                slot.calls_vrcam.fetch_add(1, Relaxed);
            } else if kind == 2 {
                slot.desc_main.fetch_add(count as u64, SeqCst);
                slot.calls_main.fetch_add(1, Relaxed);
            }
        }
    }
    if !profile || kind == 0 {
        original(output, mode, a3, a4);
        crate::preparation_trace::cvr_prep_output(KIND_FINALIZE, output as usize as u64);
        return;
    }
    let t0 = prof_now();
    original(output, mode, a3, a4);
    let dt = prof_now() - t0;
    crate::preparation_trace::cvr_prep_output(KIND_FINALIZE, output as usize as u64);
    prepare_prof_add(
        &CyberpunkVR_DebugPrepareFinalizeTicksMain,
        &CyberpunkVR_DebugPrepareFinalizeTicksVrcam,
        dt,
    );
    let count = output_count(output);
    let hist = Some(prepare_hist_bucket(count));
    let bucket = prepare_stage_mode_index(stage, mode as u8);
    prepare_bucket_add(
        &CyberpunkVR_DebugPrepareHistTicksMain,
        &CyberpunkVR_DebugPrepareHistTicksVrcam,
        hist,
        dt as u64,
    );
    prepare_bucket_add(
        &CyberpunkVR_DebugPrepareBucketTicksMain,
        &CyberpunkVR_DebugPrepareBucketTicksVrcam,
        bucket,
        dt as u64,
    );
    if let Some(slot) = prepare_finalize_bucket(stage, mode as u8) {
        if kind == 1 {
            slot.ticks_vrcam.fetch_add(dt as u64, SeqCst);
        } else if kind == 2 {
            slot.ticks_main.fetch_add(dt as u64, SeqCst);
        }
    }
}
type SortFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void);
unsafe fn prepare_sort(
    original: usize,
    a: (*mut c_void, *mut c_void, u32, *mut c_void),
    ticks: (&AtomicU64, &AtomicU64),
    buckets: (&[AtomicU64], &[AtomicU64]),
) {
    let original: SortFn = core::mem::transmute(original);
    if !profiling() {
        original(a.0, a.1, a.2, a.3);
        return;
    }
    let t0 = prof_now();
    original(a.0, a.1, a.2, a.3);
    let dt = prof_now() - t0;
    prepare_prof_add(ticks.0, ticks.1, dt);
    prepare_bucket_add(
        buckets.0,
        buckets.1,
        prepare_stage_mode_index(PREPARE_STAGE_ID.get(), PREPARE_MODE.get()),
        dt as u64,
    );
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_sort_a(
    begin: *mut c_void,
    end: *mut c_void,
    count: u32,
    keys: *mut c_void,
) {
    prepare_sort(
        cvr_cull_originals[8].load(Acquire),
        (begin, end, count, keys),
        (
            &CyberpunkVR_DebugPrepareSortATicksMain,
            &CyberpunkVR_DebugPrepareSortATicksVrcam,
        ),
        (
            &CyberpunkVR_DebugPrepareBucketSortATicksMain,
            &CyberpunkVR_DebugPrepareBucketSortATicksVrcam,
        ),
    )
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_sort_b(
    begin: *mut c_void,
    end: *mut c_void,
    count: u32,
    keys: *mut c_void,
) {
    prepare_sort(
        cvr_cull_originals[9].load(Acquire),
        (begin, end, count, keys),
        (
            &CyberpunkVR_DebugPrepareSortBTicksMain,
            &CyberpunkVR_DebugPrepareSortBTicksVrcam,
        ),
        (
            &CyberpunkVR_DebugPrepareBucketSortBTicksMain,
            &CyberpunkVR_DebugPrepareBucketSortBTicksVrcam,
        ),
    )
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_sort_c(
    begin: *mut c_void,
    end: *mut c_void,
    count: u32,
    keys: *mut c_void,
) {
    prepare_sort(
        cvr_cull_originals[10].load(Acquire),
        (begin, end, count, keys),
        (
            &CyberpunkVR_DebugPrepareSortCTicksMain,
            &CyberpunkVR_DebugPrepareSortCTicksVrcam,
        ),
        (
            &CyberpunkVR_DebugPrepareBucketSortCTicksMain,
            &CyberpunkVR_DebugPrepareBucketSortCTicksVrcam,
        ),
    )
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_prepare_sort_final(
    begin: *mut c_void,
    end: *mut c_void,
    count: u32,
    keys: *mut c_void,
) {
    prepare_sort(
        cvr_cull_originals[11].load(Acquire),
        (begin, end, count, keys),
        (
            &CyberpunkVR_DebugPrepareSortFinalTicksMain,
            &CyberpunkVR_DebugPrepareSortFinalTicksVrcam,
        ),
        (
            &CyberpunkVR_DebugPrepareBucketSortFinalTicksMain,
            &CyberpunkVR_DebugPrepareBucketSortFinalTicksVrcam,
        ),
    )
}

// ---- the cull callbacks' profile, and the second view's visibility captured and replayed for MAIN (modes 7-9)

/// sub_14029CCEC's batch: up to 32 tagged candidate pointers (record | {1 intersect, 2 inside}), count at 0x100.
#[repr(C)]
#[derive(Clone, Copy)]
struct ReplayVisibilityBatch {
    tags: [usize; 32],
    count: u32,
    reserved: u32,
}
const _: () = assert!(core::mem::offset_of!(ReplayVisibilityBatch, count) == 0x100);
const FINE_REUSE_IDLE: u32 = 0;
const FINE_REUSE_CAPTURE: u32 = 1;
const FINE_REUSE_REPLAY: u32 = 2;
static FINE_REUSE_PHASE: AtomicU32 = AtomicU32::new(FINE_REUSE_IDLE);
static VISIBILITY_BATCHES: Mutex<Vec<ReplayVisibilityBatch>> = Mutex::new(Vec::new());
static VISIBILITY_GENERATION: AtomicU64 = AtomicU64::new(0);
static FINE_VISIBLE_IDS: LazyLock<Mutex<FastMap<usize, Vec<usize>>>> =
    LazyLock::new(|| Mutex::new(cvr_host::fast::fast_map()));
thread_local! {
    static CAPTURE_VRCAM_VISIBILITY: Cell<bool> = const { Cell::new(false) };
    static VRCAM_VISIBILITY_BATCHES: core::cell::RefCell<Vec<ReplayVisibilityBatch>> = const { core::cell::RefCell::new(Vec::new()) };
    static CAPTURE_FINE_IDS: Cell<bool> = const { Cell::new(false) };
    static FINE_IDS: core::cell::RefCell<Vec<usize>> = const { core::cell::RefCell::new(Vec::new()) };
    /// 1 the second view, 2 MAIN's gameplay view.
    static MATERIALIZE_VIEW_KIND: Cell<u32> = const { Cell::new(0) };
    static MATERIALIZE_OUTPUT_KEY: Cell<usize> = const { Cell::new(0) };
}
fn visibility_replay_enabled() -> bool {
    matches!(cull_reuse_mode(), 7..=9)
}
fn materialize_prof_add(main: &AtomicU64, vrcam: &AtomicU64, value: u64) {
    match MATERIALIZE_VIEW_KIND.get() {
        1 => vrcam.fetch_add(value, SeqCst),
        2 => main.fetch_add(value, SeqCst),
        _ => 0,
    };
}
/// The range hash tables (4M slots each, diagnostic only), made on first use.
const MATERIALIZE_RANGE_HASH_CAP: usize = 1 << 22;
fn hash_table() -> Vec<AtomicU64> {
    (0..MATERIALIZE_RANGE_HASH_CAP)
        .map(|_| AtomicU64::new(0))
        .collect()
}
static RANGE_HASH: LazyLock<[Vec<AtomicU64>; 2]> = LazyLock::new(|| [hash_table(), hash_table()]);
static RANGE_CTX_HASH: LazyLock<[Vec<AtomicU64>; 2]> =
    LazyLock::new(|| [hash_table(), hash_table()]);
unsafe fn materialize_observe(
    tables: &LazyLock<[Vec<AtomicU64>; 2]>,
    key: u64,
    counters: [[&AtomicU64; 3]; 2],
) {
    let kind = MATERIALIZE_VIEW_KIND.get();
    if kind == 0 || key == 0 {
        return;
    }
    let side = (kind == 1) as usize;
    let table = &tables[side];
    let [unique, duplicate, overflow] = counters[side];
    let start = key as u32 as usize & (MATERIALIZE_RANGE_HASH_CAP - 1);
    for probe in 0..16 {
        let slot = &table[(start + probe) & (MATERIALIZE_RANGE_HASH_CAP - 1)];
        let current = slot.load(Acquire);
        if current == key {
            duplicate.fetch_add(1, Relaxed);
            return;
        }
        if current == 0 && slot.compare_exchange(0, key, AcqRel, Acquire).is_ok() {
            unique.fetch_add(1, Relaxed);
            return;
        }
    }
    overflow.fetch_add(1, Relaxed);
}
/// The view kind of a cull job's view (job + 0x18): 1 the second view, 2 MAIN's gameplay view, 0 another.
fn job_view_kind(view: usize) -> u32 {
    if view == 0 {
        return 0;
    }
    match read::<u64>(view + 0x28) {
        Some(key) if key == vrcam_key() => 1,
        Some(_) if is_main_view(view) => 2,
        _ => 0,
    }
}
/// The main cull's prepare (sub_140C65218): with the callback profile on, the engine's own gather loop re-run here
/// per callback (method at vtable + 248) with its calls, descriptors and ticks per view; modes 8/9 skip MAIN's
/// duplicate candidate gather.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_main_cull_prepare(
    manager: *mut c_void,
    job: *mut c_void,
    output: *mut c_void,
) -> i64 {
    let base = *rt().exe_base;
    if CyberpunkVR_CullCallbackProfileEnable.load(Relaxed) != 0
        && !manager.is_null()
        && !job.is_null()
        && !output.is_null()
        && base != 0
    {
        let init: unsafe extern "C" fn(*mut u8, usize, *mut c_void, u8) -> *mut c_void =
            core::mem::transmute(base + MAIN_CULL_CTX_INIT_RVA);
        let view: usize = read(job as usize + 0x18).unwrap_or(0);
        let view_kind = match job_view_kind(view) {
            1 => 1,
            2 => 2,
            _ => 0,
        };
        #[repr(C, align(16))]
        struct GatherCtx([u8; 72]);
        let mut gather = GatherCtx([0; 72]);
        init(
            gather.0.as_mut_ptr(),
            read::<usize>(job as usize + 24).unwrap_or(0),
            output,
            0,
        );
        let callbacks: usize = read(manager as usize + 516_512).unwrap_or(0);
        let count: u32 = read(manager as usize + 516_524).unwrap_or(0);
        let mut result = 0i64;
        let mut i = 0usize;
        while callbacks != 0 && i < count as usize && i < CULL_CALLBACK_MAX {
            let object: usize = read(callbacks + 8 * i).unwrap_or(0);
            if object != 0 {
                let method: usize = read::<usize>(object)
                    .and_then(|vt| read(vt + 248))
                    .unwrap_or(0);
                CyberpunkVR_DebugCullCallbackMethodRva[i]
                    .store(method.wrapping_sub(base) as u64, Relaxed);
                let before = output_count(output);
                let t0 = prof_now();
                let call: unsafe extern "C" fn(usize, *mut u8) -> i64 =
                    core::mem::transmute(method);
                result = call(object, gather.0.as_mut_ptr());
                let dt = (prof_now() - t0) as u64;
                let after = output_count(output);
                let delta = after.saturating_sub(before) as u64;
                if diagnostics() {
                    if view_kind == 1 {
                        CyberpunkVR_DebugCullCallbackCallsVrcam[i].fetch_add(1, Relaxed);
                        CyberpunkVR_DebugCullCallbackDescVrcam[i].fetch_add(delta, Relaxed);
                        CyberpunkVR_DebugCullCallbackTicksVrcam[i].fetch_add(dt, Relaxed);
                    } else if view_kind == 2 {
                        CyberpunkVR_DebugCullCallbackCallsMain[i].fetch_add(1, Relaxed);
                        CyberpunkVR_DebugCullCallbackDescMain[i].fetch_add(delta, Relaxed);
                        CyberpunkVR_DebugCullCallbackTicksMain[i].fetch_add(dt, Relaxed);
                    }
                }
            }
            i += 1;
        }
        return result;
    }
    if matches!(cull_reuse_mode(), 8 | 9) && !job.is_null() {
        let view: usize = read(job as usize + 0x18).unwrap_or(0);
        if is_main_view(view) {
            diag(&CyberpunkVR_DebugMainCullPrepareSkips);
            return 0;
        }
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_cull_originals[12].load(Acquire));
    original(manager, job, output)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_visible_append(
    output: *mut c_void,
    drawable_id: *mut usize,
) -> u8 {
    if CyberpunkVR_MaterializeProfileEnable.load(Relaxed) != 0 {
        materialize_prof_add(
            &CyberpunkVR_DebugMaterializeDrawableAppendsMain,
            &CyberpunkVR_DebugMaterializeDrawableAppendsVrcam,
            1,
        );
    }
    if cull_reuse_mode() == 9 && CAPTURE_FINE_IDS.get() && !drawable_id.is_null() {
        FINE_IDS.with(|ids| ids.borrow_mut().push(*drawable_id));
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut usize) -> u8 =
        core::mem::transmute(cvr_cull_originals[15].load(Acquire));
    original(output, drawable_id)
}
/// The fine materialize (sub_14029D110): NativeCpuCollector's capture first; the materialize profile's range hashes;
/// mode 9 captures the second view's drawable IDs per candidate range and replays them for MAIN.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_fine_materialize(
    tester: *mut c_void,
    range: *mut i64,
    partial: i8,
    output: *mut c_void,
) -> u8 {
    let original: unsafe extern "C" fn(*mut c_void, *mut i64, i8, *mut c_void) -> u8 =
        core::mem::transmute(cvr_cull_stage_originals[3].load(core::sync::atomic::Ordering::Relaxed));
    let mut captured = 0i8;
    if crate::collector_probe::cvr_ccp_capture_fine(tester, range, partial, output, &mut captured) {
        return captured as u8;
    }
    let first = if range.is_null() { 0 } else { *range };
    if CyberpunkVR_MaterializeProfileEnable.load(Relaxed) != 0 && first != 0 {
        let second = *range.add(1);
        let items = ((second - first) / 40) as u64;
        let range_hash = prepare_mix64(first as u64)
            ^ prepare_mix64((second as u64).wrapping_add(0x9E37_79B9_7F4A_7C15));
        let range_ctx_hash = range_hash
            ^ prepare_mix64(
                (MATERIALIZE_OUTPUT_KEY.get() as u64).wrapping_add(0xD6E8_FEB8_6659_FD93),
            );
        materialize_prof_add(
            &CyberpunkVR_DebugMaterializeFineCallsMain,
            &CyberpunkVR_DebugMaterializeFineCallsVrcam,
            1,
        );
        materialize_prof_add(
            &CyberpunkVR_DebugMaterializeFineRangesMain,
            &CyberpunkVR_DebugMaterializeFineRangesVrcam,
            items,
        );
        materialize_observe(
            &RANGE_HASH,
            range_hash,
            [
                [
                    &CyberpunkVR_DebugMaterializeRangeUniqueMain,
                    &CyberpunkVR_DebugMaterializeRangeDuplicateMain,
                    &CyberpunkVR_DebugMaterializeRangeProbeOverflowMain,
                ],
                [
                    &CyberpunkVR_DebugMaterializeRangeUniqueVrcam,
                    &CyberpunkVR_DebugMaterializeRangeDuplicateVrcam,
                    &CyberpunkVR_DebugMaterializeRangeProbeOverflowVrcam,
                ],
            ],
        );
        materialize_observe(
            &RANGE_CTX_HASH,
            range_ctx_hash,
            [
                [
                    &CyberpunkVR_DebugMaterializeRangeCtxUniqueMain,
                    &CyberpunkVR_DebugMaterializeRangeCtxDuplicateMain,
                    &CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowMain,
                ],
                [
                    &CyberpunkVR_DebugMaterializeRangeCtxUniqueVrcam,
                    &CyberpunkVR_DebugMaterializeRangeCtxDuplicateVrcam,
                    &CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowVrcam,
                ],
            ],
        );
    }
    if cull_reuse_mode() != 9 || first == 0 {
        return original(tester, range, partial, output);
    }
    let candidate = first as usize;
    match FINE_REUSE_PHASE.load(Acquire) {
        FINE_REUSE_CAPTURE => {
            let previous = CAPTURE_FINE_IDS.replace(true);
            FINE_IDS.with(|ids| {
                let mut ids = ids.borrow_mut();
                ids.clear();
                ids.reserve(64);
            });
            let result = original(tester, range, partial, output);
            CAPTURE_FINE_IDS.set(previous);
            if !previous {
                let ids = FINE_IDS.with(|ids| ids.borrow().clone());
                let n = ids.len() as u64;
                FINE_VISIBLE_IDS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(candidate, ids);
                diag(&CyberpunkVR_DebugFineCandidateCaptures);
                if diagnostics() {
                    CyberpunkVR_DebugFineDrawableIdsCaptured.fetch_add(n, SeqCst);
                }
            }
            result
        }
        FINE_REUSE_REPLAY if cvr_cull_originals[15].load(Acquire) != 0 => {
            let ids = FINE_VISIBLE_IDS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&candidate)
                .cloned();
            if let Some(ids) = ids {
                let append: unsafe extern "C" fn(*mut c_void, *mut usize) -> u8 =
                    core::mem::transmute(cvr_cull_originals[15].load(Acquire));
                for mut id in ids.iter().copied() {
                    append(output, &mut id);
                }
                diag(&CyberpunkVR_DebugFineCandidateReplays);
                if diagnostics() {
                    CyberpunkVR_DebugFineDrawableIdsReplayed.fetch_add(ids.len() as u64, SeqCst);
                }
                return 0;
            }
            diag(&CyberpunkVR_DebugFineCandidateFallbacks);
            original(tester, range, partial, output)
        }
        _ => original(tester, range, partial, output),
    }
}
/// The visibility collector (sub_1404ED29C): the second view's batches captured while its cull test runs.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_visibility_collector(
    context: *mut c_void,
    batch: *mut c_void,
) -> i64 {
    if visibility_replay_enabled() && CAPTURE_VRCAM_VISIBILITY.get() && !batch.is_null() {
        if let Some(b) = read::<ReplayVisibilityBatch>(batch as usize) {
            let count = b.count.min(32);
            if count != 0 {
                let mut cached = ReplayVisibilityBatch {
                    tags: [0; 32],
                    count,
                    reserved: 0,
                };
                cached.tags[..count as usize].copy_from_slice(&b.tags[..count as usize]);
                VRCAM_VISIBILITY_BATCHES.with(|v| v.borrow_mut().push(cached));
                if diagnostics() {
                    CyberpunkVR_DebugVisibilityCandidatesCaptured.fetch_add(count as u64, SeqCst);
                }
            }
        }
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        core::mem::transmute(cvr_cull_originals[13].load(Acquire));
    original(context, batch)
}
/// The main cull test (sub_140C651C4): NativeCpuCollector's capture first; modes 7-9 capture the second view's
/// tagged visibility and replay it into MAIN's fresh output in place of MAIN's own tester loop.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_main_cull_test(
    manager: *mut c_void,
    job: *mut c_void,
    output: *mut c_void,
    tester: *mut c_void,
    query: *mut c_void,
) -> i64 {
    let original: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
    ) -> i64 = core::mem::transmute(cvr_cull_stage_originals[2].load(core::sync::atomic::Ordering::Relaxed));
    let mut captured = 0i64;
    if crate::collector_probe::cvr_ccp_capture_scene(manager, job, output, tester, query, &mut captured) {
        return captured;
    }
    if !visibility_replay_enabled() || job.is_null() {
        return original(manager, job, output, tester, query);
    }
    let view: usize = read(job as usize + 0x18).unwrap_or(0);
    if view == 0 {
        return original(manager, job, output, tester, query);
    }
    let key: u64 = read(view + 0x28).unwrap_or(0);
    let _aspect: f32 = read(view + 0x98).unwrap_or(0.0);
    if key == vrcam_key() {
        if cull_reuse_mode() == 9 {
            FINE_REUSE_PHASE.store(FINE_REUSE_CAPTURE, Release);
        }
        let previous = CAPTURE_VRCAM_VISIBILITY.replace(true);
        VRCAM_VISIBILITY_BATCHES.with(|v| {
            let mut v = v.borrow_mut();
            v.clear();
            v.reserve(128);
        });
        let result = original(manager, job, output, tester, query);
        CAPTURE_VRCAM_VISIBILITY.set(previous);
        if !previous {
            *VISIBILITY_BATCHES.lock().unwrap_or_else(|e| e.into_inner()) =
                VRCAM_VISIBILITY_BATCHES.with(|v| v.borrow().clone());
            VISIBILITY_GENERATION.fetch_add(1, Release);
            diag(&CyberpunkVR_DebugVisibilityBatchCaptures);
        }
        return result;
    }
    if is_main_view(view)
        && VISIBILITY_GENERATION.load(Acquire) != 0
        && cvr_cull_originals[13].load(Acquire) != 0
    {
        if cull_reuse_mode() == 9 {
            FINE_REUSE_PHASE.store(FINE_REUSE_REPLAY, Release);
        }
        let mut batches = VISIBILITY_BATCHES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if !batches.is_empty() {
            let mut context = [
                job as usize,
                output as usize,
                tester as usize,
                query as usize,
                read::<usize>(manager as usize + 7224).unwrap_or(0),
            ];
            let collector: unsafe extern "C" fn(*mut usize, *mut ReplayVisibilityBatch) -> i64 =
                core::mem::transmute(cvr_cull_originals[13].load(Acquire));
            let (mut result, mut replayed) = (0i64, 0u64);
            for batch in &mut batches {
                replayed += batch.count as u64;
                result = collector(context.as_mut_ptr(), batch);
            }
            diag(&CyberpunkVR_DebugVisibilityBatchReplays);
            if diagnostics() {
                CyberpunkVR_DebugVisibilityCandidatesReplayed.fetch_add(replayed, SeqCst);
            }
            return result;
        }
    }
    if cull_reuse_mode() != 9 {
        FINE_REUSE_PHASE.store(FINE_REUSE_IDLE, Release);
    }
    original(manager, job, output, tester, query)
}
/// The materialize worker (sub_1404ED62C): NativePreparation's materialize ticket while it is active; else the
/// materialize profile's view kind and output key for the worker's calls.
#[no_mangle]
pub unsafe extern "C" fn cvr_cull_materialize_worker(
    task: *mut c_void,
    queue_ctx: *mut c_void,
) -> *mut c_void {
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        core::mem::transmute(cvr_cull_originals[14].load(Acquire));
    if crate::preparation_trace::active() {
        let view: usize = if task.is_null() {
            0
        } else {
            read(task as usize + 24).unwrap_or(0)
        };
        let side = match job_view_kind(view) {
            1 => 1,
            2 => 0,
            _ => -1,
        };
        let mut ticket = [0u64; 15];
        crate::preparation_trace::cvr_prep_begin_task(0, task as usize as u64, 0, side, ticket.as_mut_ptr().cast());
        let result = original(task, queue_ctx);
        crate::preparation_trace::cvr_prep_end_task(0, ticket.as_ptr().cast());
        return result;
    }
    if CyberpunkVR_MaterializeProfileEnable.load(Relaxed) == 0 || task.is_null() {
        return original(task, queue_ctx);
    }
    let previous = (
        MATERIALIZE_VIEW_KIND.replace(0),
        MATERIALIZE_OUTPUT_KEY.replace(0),
    );
    let _ = (|| -> Option<()> {
        let view: usize = read(task as usize + 24)?;
        MATERIALIZE_VIEW_KIND.set(job_view_kind(view));
        MATERIALIZE_OUTPUT_KEY.set(read(task as usize + 80)?);
        let tagged: u32 = read(task as usize + 352)?;
        materialize_prof_add(
            &CyberpunkVR_DebugMaterializeWorkerTasksMain,
            &CyberpunkVR_DebugMaterializeWorkerTasksVrcam,
            1,
        );
        materialize_prof_add(
            &CyberpunkVR_DebugMaterializeTaggedMain,
            &CyberpunkVR_DebugMaterializeTaggedVrcam,
            tagged as u64,
        );
        Some(())
    })();
    let result = original(task, queue_ctx);
    MATERIALIZE_VIEW_KIND.set(previous.0);
    MATERIALIZE_OUTPUT_KEY.set(previous.1);
    result
}
