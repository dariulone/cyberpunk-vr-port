//! THE NODE DISPATCHER (formerly src/Stereo/NodeDispatch.cpp's Detour_NodeDispatch): called once per frame-graph node
//! per view, and the hub every per-view decision hangs off.
//!
//! In order: who is dispatching (the work function, the view key at ctx+0x28 -- 0 for MAIN, g_vrcam_ctx_key for the
//! second eye, another camera's name hash otherwise -- the pass id, the capability and render-mask grants for the
//! second view, the sky and cloud probes, MAIN's view object bound from its Present and StartRender nodes); the
//! thread's node and view published for every hook that runs inside, saved and restored because SceneDrv re-enters
//! per pass; the second view's viewData holes filled; the optics view's mask share; the census; the node cut; the
//! mirror's CopyToTexture node armed; DrawHUD's census and MAIN's draw-block list lent to the second eye; the view
//! SIDE (0 MAIN by object identity, 1 the second eye, -1 anything else); the node's camera pose; the profiler at every
//! depth; the scoped composition lend; the original run through the preparation scope; and at the epilogue, the
//! second view's finished frame, the tonemap snapshot and the mirror output, each from its own gate.
//!
//! NodeDispatch.cpp keeps the hook's trampoline and forwards Detour_NodeDispatch here; the pose, fog-context,
//! preparation and optics-probe calls go through adapters.
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::capture_hooks::{tls_ptr, Tls};
use crate::capture_output::cvr_capture_publish_output;
use crate::capture_stable::{cvr_capture_final_frame_copy_now, cvr_capture_stable_inline_copy};
use crate::hud::{cvr_hud_dump_capability_mask, cvr_hud_grant_capability};
use crate::node_features::{cvr_nd_optics_hide_weapon_plane, cvr_nd_render_mask_grant};
use crate::node_probes::{
    cvr_nd_cloud_sel_note, cvr_nd_node_census_add, cvr_nd_node_census_dump,
    cvr_nd_optics_render_probe, cvr_nd_sky_probe_note,
};
use crate::stereo_profiler::{
    cvr_stereo_node_cut_match, cvr_stereo_prof_node_add, cvr_stereo_prof_now,
    cvr_stereo_prof_pair_add, cvr_stereo_prof_timeline_add, ProfPass,
};
use crate::view_reuse::{
    cvr_reuse_comp_lend_install, cvr_reuse_comp_lend_restore, cvr_reuse_complend_report,
    cvr_reuse_view_params_report, cvr_reuse_viewdata_fill_from_wc,
};
use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_DebugVrcamNodeHits: AtomicU64;
    static CyberpunkVR_DebugMainObjBinds: AtomicU64;
    static CyberpunkVR_DebugMirrorCopyNodeHits: AtomicU64;
    static CyberpunkVR_ViewDataFixMask: AtomicU32;
    static CyberpunkVR_FogMirrorMask: AtomicU32;
    static CyberpunkVR_EnvMirrorMask: AtomicU32;
    static CyberpunkVR_CompLendSet: AtomicU32;
    static CyberpunkVR_CompLendScoped: AtomicU32;
    static CyberpunkVR_MaskShareTiming: AtomicI32;
    static CyberpunkVR_OpticsPreworkTicks: AtomicU64;
    static CyberpunkVR_OpticsPreworkCalls: AtomicU64;
    static CyberpunkVR_NodeCensus: AtomicI32;
    static CyberpunkVR_NodeCutEnable: AtomicI32;
    static CyberpunkVR_NodeCutRetVal: AtomicI32;
    static CyberpunkVR_DebugNodeCutSkips: AtomicU64;
    static CyberpunkVR_DebugHudNodeVrcam: AtomicU64;
    static CyberpunkVR_DebugHudNodeMain: AtomicU64;
    static CyberpunkVR_DebugHudGateDenied: AtomicU64;
    static CyberpunkVR_DebugHudGateForced: AtomicU64;
    static CyberpunkVR_DebugHudBlockNull: AtomicU64;
    static CyberpunkVR_DebugHudBlockOk: AtomicU64;
    static CyberpunkVR_DebugHudBlockLent: AtomicU64;
    static CyberpunkVR_DebugHudCapGrants: AtomicU64;
    static CyberpunkVR_HudCapWord: AtomicU32;
    static CyberpunkVR_HudCapBits: AtomicU64;
    static CyberpunkVR_HudInVrcam: AtomicI32;
    static CyberpunkVR_HudBorrowBlocks: AtomicI32;
    static CyberpunkVR_HudGrantCap: AtomicI32;
    static CyberpunkVR_ProfEnable: AtomicI32;
    static CyberpunkVR_NodeTimeline: AtomicI32;
    static CyberpunkVR_OpticsFreezeCopy: AtomicI32;
    static CyberpunkVR_StableFromTonemap: AtomicI32;
    static CyberpunkVR_FinalGrab: AtomicI32;
    static CyberpunkVR_StableCopy: AtomicI32;
    static CyberpunkVR_NoStateLies: AtomicI32;
    static CyberpunkVR_DebugTonemapSnaps: AtomicU64;
    static CyberpunkVR_DebugForeignStateRefusals: AtomicU64;
    static CyberpunkVR_DebugMirrorCopyArms: AtomicU64;
    static g_vrcam_ctx_key: AtomicU64;
    static g_game_device: *mut c_void;
    static g_prof_disp_main_ticks: AtomicI64;
    static g_prof_disp_vrcam_ticks: AtomicI64;
    static g_prof_disp_main_nodes: AtomicU64;
    static g_prof_disp_vrcam_nodes: AtomicU64;
    static g_prof_top_main: AtomicU64;
    static g_prof_top_vrcam: AtomicU64;
    static g_prof_scenepass: [ProfPass; 256];
    fn cvr_optics_source_view_dispatched(side: i32, work_context: usize);
    fn cvr_optics_camera_copy_node_begin(work_context: usize);
    fn cvr_optics_camera_copy_node_end();
}

const RENDER_FINAL2D_WORK_RVA: usize = 0x41_72C8;
const MAIN_PRESENT_WORK_RVA: u32 = 0xBF_60F0;
const MAIN_STARTRENDER_WORK_RVA: u32 = 0x5D_798C;
const DRAWHUD_WORK_RVA: u32 = 0x33_E388;
const SCENE_DRIVER_WORK_RVA: u32 = 0x33_2D5C;
const VOLUMETRIC_FOG_NODE_RVA: u32 = 0xDD_B6B8;
const COMPOSITION_WORK_RVA: u32 = 0x3E_1B20;
const RT_DECLARE_WORK_RVA: u32 = 0xCE_A010;
const GATE_READER_770268_RVA: u32 = 0x77_3580;
/// The HUD's viewData accessor (sub_14033C248) and the draw-block list's slot in what it returns.
const HUD_VIEWDATA_GET_RVA: usize = 0x33_C248;
const HUD_BLOCK_SLOT: usize = 0x168;
/// The optics camera's copy node and the optics probe's stage node.
const OPTICS_COPY_NODE_RVA: u32 = 0x3F_E078;
const OPTICS_STAGE_NODE_RVA: u32 = 0x41_6600;

/// The node's camera pose and the one it displaced (formerly the .cpp's NdPoseScope).
struct PoseScope {
    node: PoseIdentity,
    previous: PoseIdentity,
}
use crate::xr_capture::PoseIdentity;
extern "C" {
    /// rust/crates/cvr-rig/src/vrik/pose_identity.rs: a camera's pose identity (view 1 MAIN, 2 the second eye), and
    /// the node's current one set (the displaced one returned).
    fn cvr_pose_read_camera(camera: usize, view: u32, out: *mut PoseIdentity) -> bool;
    fn cvr_pose_node_set(pose: *const PoseIdentity, previous: *mut PoseIdentity);
}
/// include/Render/ViewCameraLayout.hpp kCamera: the view's camera block.
const VIEW_CAMERA: usize = 0xE0;
/// The node's pose: read for MAIN and the second eye, the render frame stamped when frame generation captures inputs,
/// made current (the displaced one kept), the native view observed (formerly the .cpp's NdPoseEnter).
unsafe fn pose_enter(known: bool, key: u64, ctx: usize, work_context: *mut u8) -> PoseScope {
    // SAFETY: plain data, zeroed as the .cpp's `NdPoseScope{}`.
    let mut scope: PoseScope = unsafe { core::mem::zeroed() };
    // SAFETY: the dispatcher's view context and the published second-eye key.
    unsafe {
        if known && (key == 0 || key == g_vrcam_ctx_key.load(Relaxed)) {
            cvr_pose_read_camera(ctx + VIEW_CAMERA, if key == 0 { 1 } else { 2 }, &mut scope.node);
        }
        let capture = cvr_gpu::framegen_core::cvr_framegen_capture_inputs_enabled();
        if capture && scope.node.present() {
            scope.node.has_render_frame_id = crate::framegen_inputs::cvr_framegen_read_render_frame_index(&mut scope.node.render_frame_id);
        }
        cvr_pose_node_set(&scope.node, &mut scope.previous);
        if capture && scope.node.present() {
            crate::framegen_inputs::cvr_framegen_observe_native_view(work_context.cast());
        }
    }
    scope
}
/// The displaced pose made current again (formerly the .cpp's NdPoseExit).
fn pose_exit(scope: &PoseScope) {
    let mut unused: PoseIdentity = scope.previous;
    // SAFETY: the scope's own poses.
    unsafe { cvr_pose_node_set(&scope.previous, &mut unused) };
}
/// cvr::stereo::preparation::CopySource.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CopySource {
    view: u64,
    node: u32,
    side: i32,
}
/// The preparation's copy source set (the displaced one returned; formerly the .cpp's NdSetCopySource).
fn set_copy_source(source: &CopySource, previous: &mut CopySource) {
    use crate::preparation_trace::CopySource as Prep;
    let value = Prep { view: source.view, node: source.node, side: source.side };
    let mut old = Prep { view: 0, node: 0, side: -1 };
    // SAFETY: two local sources.
    unsafe { crate::preparation_trace::cvr_prep_copy_source(&value, &mut old) };
    *previous = CopySource { view: old.view, node: old.node, side: old.side };
}

/// The node dispatcher's trampoline (written by engine_hooks.rs's install).
#[no_mangle]
pub static cvr_node_dispatch_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    main_view_obj: *const AtomicUsize,
    render_mask_report: extern "C" fn(),
    eye_node_hits: *const AtomicU64,
    eye_no_rtv: *const AtomicU64,
    eye_no_list: *const AtomicU64,
    eye_copy_calls: *const AtomicU64,
    /// g_hud_viewdata_get (resolved here on first use) and g_hud_block_main.
    hud_viewdata_get: *mut usize,
    hud_block_main: *mut *mut c_void,
    capture_wanted: extern "C" fn() -> bool,
    have_tonemap_source: *const AtomicBool,
    mirror_vrcam_serial: *const AtomicU64,
    mirror_armed_serial: *const AtomicU64,
    mirror_copy_armed: *const AtomicBool,
    /// cname_hash("vr_optics_feed").
    optics_feed_hash: u64,
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    main_view_obj: &raw const m::g_main_view_obj,
    render_mask_report: s::cvr_cpp_render_mask_report,
    eye_node_hits: &raw const m::g_eye_node_hits,
    eye_no_rtv: &raw const m::g_eye_no_rtv,
    eye_no_list: &raw const m::g_eye_no_list,
    eye_copy_calls: &raw const m::g_eye_copy_calls,
    hud_viewdata_get: &raw mut s::g_hud_viewdata_get,
    hud_block_main: &raw mut s::g_hud_block_main,
    capture_wanted: s::capture_wanted,
    have_tonemap_source: &raw const m::g_have_tonemap_source,
    mirror_vrcam_serial: &raw const s::g_mirror_vrcam_serial,
    mirror_armed_serial: &raw const s::g_mirror_armed_serial,
    mirror_copy_armed: &raw const s::g_mirror_copy_armed,
    optics_feed_hash: s::OPTICS_FEED_HASH,
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
fn qpc() -> i64 {
    let mut t = 0i64;
    // SAFETY: a plain system query.
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut t);
    }
    t
}

/// The view-state object a work context's view wrapper hands back (its vtable slot 4), guarded.
unsafe fn sl_view_obj(work_context: *mut u8) -> Option<usize> {
    let x = code::read::<usize>(work_context as usize)?;
    if x == 0 {
        return Some(0);
    }
    let vtable = code::read::<usize>(x)?;
    let get_view = code::read::<usize>(vtable + 0x20)?;
    code::guarded_call4(
        get_view,
        x as *mut c_void,
        null_mut(),
        null_mut(),
        null_mut(),
    )
    .map(|v| v as usize)
}

/// The second view's RenderFinal2D node (the CopyToTexture the mirror copies from).
unsafe fn is_vrcam_copy_to_texture(node: *mut usize, work_context: *mut u8, base: usize) -> bool {
    if node.is_null() || work_context.is_null() {
        return false;
    }
    let hit = (|| -> Option<bool> {
        let ctx = code::read::<usize>(work_context as usize + 0x18)?;
        if ctx == 0 || code::read::<u64>(ctx + 0x28)? != g_vrcam_ctx_key.load(SeqCst) {
            return Some(false);
        }
        let vtable = code::read::<usize>(node as usize)?;
        let work = code::read::<usize>(vtable + 8)?;
        Some(work == base.wrapping_add(RENDER_FINAL2D_WORK_RVA))
    })()
    .unwrap_or(false);
    if hit {
        diag(&CyberpunkVR_DebugMirrorCopyNodeHits);
    }
    hit
}

/// preparation::InvokeGraphContext: the original work under the node's graph context (preparation_trace.rs) and its
/// scene-child scope (definition tracking), both left in reverse order when the work returns.
fn invoke_graph_context(original: usize, node: *mut usize, wc: *mut u8, args: *mut c_void) -> u8 {
    struct Context(u64);
    impl Drop for Context {
        fn drop(&mut self) {
            crate::preparation_trace::cvr_prep_graph_context_swap(self.0);
        }
    }
    struct Child;
    impl Drop for Child {
        fn drop(&mut self) {
            crate::definition_tracking::scene_child::cvr_ndt_child_leave();
        }
    }
    let _context = Context(crate::preparation_trace::cvr_prep_graph_context_swap(wc as u64));
    crate::definition_tracking::scene_child::cvr_ndt_child_enter(node as u64, wc as u64, 0);
    let _child = Child;
    // SAFETY: the dispatcher's original work function.
    let work: extern "C" fn(*mut usize, *mut u8, *mut c_void) -> u8 = unsafe { core::mem::transmute(original) };
    work(node, wc, args)
}

/// Detour_NodeDispatch.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_dispatch(
    node: *mut usize,
    work_context: *mut u8,
    args: *mut c_void,
) -> u8 {
    let r = rt();
    let base = *r.exe_base;
    let wc = work_context;
    // ---- attribution FIRST (work function, view, pass id): the profiler, the node cut and the mirror path all
    // read it, and the bind/barrier hooks gate on the thread's node. A fault on the way leaves the work function 0.
    let mut vrcam_node = false;
    let mut owner_bit = false; // +0x30 bit 1 gates the actual DoCulling execution
    let mut prof_work = 0usize;
    let mut scene_rtid = 0xFFu8; // ctx+0x38: the pass/RT slot id
    let mut view_key = 0u64; // ctx+0x28: 0 = MAIN, g_vrcam_ctx_key = the second eye, else another camera
    let mut view_key_known = false; // false when the node carries no view ctx at all
    let mut pose_ctx = 0usize;
    let attributed = (|| -> Option<()> {
        let vtable = if node.is_null() {
            0
        } else {
            code::read::<usize>(node as usize)?
        };
        prof_work = if vtable != 0 {
            code::read::<usize>(vtable + 8)?
        } else {
            0
        };
        if wc.is_null() {
            return Some(());
        }
        let ctx = code::read::<usize>(wc as usize + 0x18)?;
        if ctx != 0 {
            view_key = code::read::<u64>(ctx + 0x28)?;
            pose_ctx = ctx;
            view_key_known = true;
        }
        if ctx != 0 && code::read::<u64>(ctx + 0x28)? == g_vrcam_ctx_key.load(SeqCst) {
            vrcam_node = true;
            // Re-asserted per node (fg_observe grants at build time, for the builder path it sees): no node of this
            // view runs before the capability and the named render-mask categories are present.
            cvr_hud_grant_capability(ctx);
            cvr_nd_render_mask_grant(ctx, true);
        }
        if ctx != 0 {
            let second = view_key == g_vrcam_ctx_key.load(SeqCst);
            cvr_nd_sky_probe_note(ctx, second);
            cvr_nd_cloud_sel_note(ctx, second);
        }
        if vrcam_node {
            diag(&CyberpunkVR_DebugVrcamNodeHits);
            (r.render_mask_report)();
        }
        // MAIN identity: these nodes run with no view ctx, so the view OBJECT they carry is what is recorded.
        if !vrcam_node && (!view_key_known || view_key == 0) && prof_work != 0 && base != 0 {
            let rva = prof_work.wrapping_sub(base) as u32;
            if rva == MAIN_PRESENT_WORK_RVA || rva == MAIN_STARTRENDER_WORK_RVA {
                let obj = sl_view_obj(wc)?;
                if obj != 0 && (*r.main_view_obj).swap(obj, Release) != obj {
                    diag(&CyberpunkVR_DebugMainObjBinds);
                }
            }
        }
        owner_bit = code::read::<u8>(wc as usize + 0x30)? & 2 != 0;
        // The engine's executor indexes its RT table with ctx+0x38 unconditionally; there is no valid flag.
        scene_rtid = code::read::<u8>(wc as usize + 0x38)?;
        Some(())
    })();
    if attributed.is_none() {
        prof_work = 0;
    }
    // Saved and restored: a node that dispatches children would otherwise attribute everything after them to the
    // last child.
    let p_work = tls_ptr::<usize>(Tls::CurrentNodeWork);
    let p_view = tls_ptr::<usize>(Tls::CurrentNodeView);
    let previous_node_work = *p_work;
    let previous_node_view = *p_view;
    *p_work = prof_work;
    *p_view = pose_ctx;
    let work_rva = if prof_work != 0 && base != 0 && prof_work > base {
        (prof_work - base) as u32
    } else {
        0
    };
    // The second view's viewData holes closed as early as the view is seen at all.
    if (CyberpunkVR_ViewDataFixMask.load(Relaxed) != 0
        || CyberpunkVR_FogMirrorMask.load(Relaxed) != 0
        || CyberpunkVR_EnvMirrorMask.load(Relaxed) != 0
        || CyberpunkVR_CompLendSet.load(Relaxed) != 0)
        && vrcam_node
        && !wc.is_null()
    {
        cvr_reuse_viewdata_fill_from_wc(wc.cast(), false);
    }
    if view_key_known && view_key == r.optics_feed_hash && !wc.is_null() {
        let p0 = if CyberpunkVR_MaskShareTiming.load(Relaxed) != 0 {
            qpc()
        } else {
            0
        };
        cvr_nd_render_mask_grant(pose_ctx, false);
        cvr_nd_optics_hide_weapon_plane(pose_ctx);
        cvr_reuse_viewdata_fill_from_wc(wc.cast(), true);
        if CyberpunkVR_MaskShareTiming.load(Relaxed) != 0 {
            CyberpunkVR_OpticsPreworkTicks.fetch_add(qpc().wrapping_sub(p0) as u64, SeqCst);
            CyberpunkVR_OpticsPreworkCalls.fetch_add(1, SeqCst);
        }
    }
    // The per-view node census: only views that can be named.
    if CyberpunkVR_NodeCensus.load(Relaxed) != 0 && work_rva != 0 && view_key_known {
        if view_key == 0 {
            cvr_nd_node_census_add(work_rva, false);
            cvr_nd_node_census_dump();
        } else if view_key == g_vrcam_ctx_key.load(SeqCst) {
            cvr_nd_node_census_add(work_rva, true);
        }
    }
    // The node cut: the whole node skipped when an armed rule matches.
    if CyberpunkVR_NodeCutEnable.load(Relaxed) != 0
        && work_rva != 0
        && cvr_stereo_node_cut_match(work_rva, scene_rtid, vrcam_node)
    {
        diag(&CyberpunkVR_DebugNodeCutSkips);
        *p_work = previous_node_work;
        *p_view = previous_node_view;
        return CyberpunkVR_NodeCutRetVal.load(Relaxed) as u8;
    }
    // The mirror: the second view's CopyToTexture node arms the per-node capture state.
    let mirror_copy_node = is_vrcam_copy_to_texture(node, wc, base);
    let p_mirror_active = tls_ptr::<bool>(Tls::MirrorCopyNodeActive);
    let p_mirror_rtv = tls_ptr::<*mut c_void>(Tls::MirrorCopyRtv);
    let p_mirror_format = tls_ptr::<DXGI_FORMAT>(Tls::MirrorCopyRtvFormat);
    let p_mirror_list = tls_ptr::<*mut c_void>(Tls::MirrorCopyList);
    let p_mirror_state = tls_ptr::<u32>(Tls::MirrorSrcState);
    let p_mirror_seen = tls_ptr::<bool>(Tls::MirrorSrcStateSeen);
    let previous_mirror_active = *p_mirror_active;
    let previous_mirror_rtv = *p_mirror_rtv;
    let previous_mirror_format = *p_mirror_format;
    let previous_mirror_list = *p_mirror_list;
    if mirror_copy_node {
        diag(&*r.eye_node_hits);
        *p_mirror_active = true;
        *p_mirror_rtv = null_mut();
        *p_mirror_format = DXGI_FORMAT_UNKNOWN;
        *p_mirror_list = null_mut();
        *p_mirror_state = D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32;
        *p_mirror_seen = false; // a seed, not an observation
    }
    // Does the HUD node reach the second view at all (an override is useless on a node never dispatched for it)?
    if work_rva == DRAWHUD_WORK_RVA {
        diag(if vrcam_node {
            &CyberpunkVR_DebugHudNodeVrcam
        } else {
            &CyberpunkVR_DebugHudNodeMain
        });
        cvr_hud_dump_capability_mask(wc as usize, vrcam_node);
        if diagnostics() && CyberpunkVR_DebugHudNodeMain.load(Relaxed) % 900 == 1 {
            stereo_log(&format!(
                "[hud] DrawHUD main={} vrcam={} | gate denied={} forced={} | blocks null={} ok={} lent={} | capGrants={} w{}={:016X} | HudInVrcam={} borrow={} grant={}",
                CyberpunkVR_DebugHudNodeMain.load(Relaxed),
                CyberpunkVR_DebugHudNodeVrcam.load(Relaxed),
                CyberpunkVR_DebugHudGateDenied.load(Relaxed),
                CyberpunkVR_DebugHudGateForced.load(Relaxed),
                CyberpunkVR_DebugHudBlockNull.load(Relaxed),
                CyberpunkVR_DebugHudBlockOk.load(Relaxed),
                CyberpunkVR_DebugHudBlockLent.load(Relaxed),
                CyberpunkVR_DebugHudCapGrants.load(Relaxed),
                CyberpunkVR_HudCapWord.load(Relaxed),
                CyberpunkVR_HudCapBits.load(Relaxed),
                CyberpunkVR_HudInVrcam.load(Relaxed),
                CyberpunkVR_HudBorrowBlocks.load(Relaxed),
                CyberpunkVR_HudGrantCap.load(Relaxed)
            ));
        }
    }
    // MAIN's draw-block list lent to the second eye for the HUD node, restored whatever the node does.
    let mut hud_block_slot = 0usize;
    let mut hud_block_saved = 0usize;
    if work_rva == DRAWHUD_WORK_RVA && !wc.is_null() {
        if *r.hud_viewdata_get == 0 && base != 0 {
            *r.hud_viewdata_get = base + HUD_VIEWDATA_GET_RVA;
        }
        let get = *r.hud_viewdata_get;
        if get != 0 {
            let lent = (|| -> Option<()> {
                let vd = code::guarded_call4(get, wc.cast(), null_mut(), null_mut(), null_mut())?
                    as usize;
                if vd == 0 {
                    return Some(());
                }
                let slot = vd + HUD_BLOCK_SLOT;
                let cur = code::read::<usize>(slot)?;
                if !vrcam_node {
                    // MAIN's own HUD node, same frame: the most honest source for the list the second eye misses.
                    if cur != 0 {
                        *r.hud_block_main = cur as *mut c_void;
                    }
                } else if cur == 0 {
                    diag(&CyberpunkVR_DebugHudBlockNull);
                    let main = *r.hud_block_main;
                    if CyberpunkVR_HudInVrcam.load(Relaxed) != 0
                        && CyberpunkVR_HudBorrowBlocks.load(Relaxed) != 0
                        && !main.is_null()
                    {
                        hud_block_slot = slot;
                        hud_block_saved = cur;
                        if !code::write::<usize>(slot, main as usize) {
                            return None;
                        }
                        diag(&CyberpunkVR_DebugHudBlockLent);
                    }
                } else {
                    // Not empty: this is not where the node stops, and the borrow would be the wrong fix.
                    diag(&CyberpunkVR_DebugHudBlockOk);
                }
                Some(())
            })();
            if lent.is_none() {
                hud_block_slot = 0;
            }
        }
    }
    let p_vrcam = tls_ptr::<bool>(Tls::VrcamNodeActive);
    let previous_vrcam_node = *p_vrcam;
    *p_vrcam = vrcam_node;
    // WHICH OF THE THREE: MAIN is the dispatch whose view OBJECT is the one bound above (the key alone reads 0 for
    // every engine view, reflection-probe faces included), the second eye its key, and a named auxiliary camera or
    // anything else neither -- never MAIN.
    let p_side = tls_ptr::<i32>(Tls::ViewSide);
    let previous_view_side = *p_side;
    *p_side = if view_key_known && view_key == g_vrcam_ctx_key.load(SeqCst) {
        1
    } else if view_key_known && view_key != 0 {
        -1
    } else {
        let main_obj = (*r.main_view_obj).load(Acquire);
        let obj = if wc.is_null() {
            0
        } else {
            sl_view_obj(wc).unwrap_or(0)
        };
        if main_obj != 0 && obj == main_obj {
            0
        } else {
            -1
        }
    };
    if *p_side >= 0 {
        cvr_optics_source_view_dispatched(*p_side, wc as usize);
    }
    // The exact view for the camera hooks inside this dispatch.
    let p_known = tls_ptr::<bool>(Tls::ActiveViewKnown);
    let p_key = tls_ptr::<u64>(Tls::ActiveViewKey);
    let previous_view_known = *p_known;
    let previous_view_key = *p_key;
    *p_known = view_key_known;
    *p_key = view_key;
    let pose = pose_enter(view_key_known, view_key, pose_ctx, wc);
    let fog_node = work_rva == VOLUMETRIC_FOG_NODE_RVA;
    let previous_fog_context = if fog_node {
        crate::render_parity::cvr_rp_set_fog_work_context(wc as usize)
    } else {
        0
    };

    // ---- the profiler at EVERY depth (SceneDrv drives ~37 scene passes back through this hook), inclusive and self
    // time through a thread-local accumulator each node reports upward through.
    let p_depth = tls_ptr::<i32>(Tls::ProfDispDepth);
    let p_child = tls_ptr::<i64>(Tls::ProfChildTicks);
    let prof_on = CyberpunkVR_ProfEnable.load(Relaxed) != 0;
    let prof_top = prof_on && *p_depth == 0;
    let timeline_depth = *p_depth as u32;
    let timeline_mode = CyberpunkVR_NodeTimeline.load(Relaxed);
    let timeline = timeline_mode != 0 && (timeline_depth == 0 || timeline_mode >= 2);
    let timeline_t0 = if timeline { cvr_stereo_prof_now() } else { 0 };
    *p_depth += 1;
    let (mut prof_t0, mut prof_saved_child) = (0i64, 0i64);
    if prof_on {
        prof_saved_child = *p_child;
        *p_child = 0;
        prof_t0 = cvr_stereo_prof_now();
    }
    // The composition state for the measured readers of viewData+0x168 (never DrawComposition, which walks a list
    // built for another view and crashes on it).
    let mut comp_saved = [0u64; 2];
    let mut comp_lent_vd = 0usize;
    let scoped = CyberpunkVR_CompLendScoped.load(Relaxed);
    if scoped != 0 && vrcam_node && !wc.is_null() && prof_work != 0 && base != 0 {
        let crva = if prof_work > base {
            (prof_work - base) as u32
        } else {
            0
        };
        let wanted = (scoped & 1 != 0 && crva == COMPOSITION_WORK_RVA)
            || (scoped & 2 != 0 && crva == RT_DECLARE_WORK_RVA)
            || (scoped & 4 != 0 && crva == DRAWHUD_WORK_RVA)
            || (scoped & 8 != 0 && crva == GATE_READER_770268_RVA);
        if wanted {
            comp_lent_vd = cvr_reuse_comp_lend_install(wc.cast(), comp_saved.as_mut_ptr());
        }
    }
    cvr_reuse_view_params_report();
    cvr_reuse_complend_report();
    if view_key_known {
        cvr_nd_optics_render_probe(pose_ctx, wc as usize, view_key, work_rva);
    }
    // Diagnostic: hold the external camera image while everything else renders normally.
    let optical_copy =
        view_key_known && view_key == r.optics_feed_hash && work_rva == OPTICS_COPY_NODE_RVA;
    let freeze_optical = CyberpunkVR_OpticsFreezeCopy.load(Relaxed) != 0 && optical_copy;
    if optical_copy && !freeze_optical {
        cvr_optics_camera_copy_node_begin(wc as usize);
    }
    let result = if freeze_optical {
        1
    } else {
        invoke_graph_context(cvr_node_dispatch_original.load(core::sync::atomic::Ordering::Acquire), node, wc, args)
    };
    if optical_copy && !freeze_optical {
        cvr_optics_camera_copy_node_end();
    }
    if view_key_known
        && cvr_gpu::optics_probe::cvr_optics_probe_stage_view(view_key, g_vrcam_ctx_key.load(SeqCst))
        && work_rva == OPTICS_STAGE_NODE_RVA
    {
        cvr_gpu::optics_probe::cvr_optics_probe_stage_end(g_game_device);
    }
    if comp_lent_vd != 0 {
        cvr_reuse_comp_lend_restore(comp_lent_vd, comp_saved.as_ptr());
    }
    // The draw-block slot given back before anything else can run on this view.
    if hud_block_slot != 0 {
        let _ = code::write::<usize>(hud_block_slot, hud_block_saved);
    }
    if prof_on {
        let dt = cvr_stereo_prof_now() - prof_t0;
        let self_time = (dt - *p_child).max(0); // clock jitter across cores
        *p_child = prof_saved_child + dt; // our inclusive time handed to the parent
        if prof_top {
            // Frame totals stay top-level only, else they double-count.
            if vrcam_node {
                g_prof_disp_vrcam_ticks.fetch_add(dt, Relaxed);
                g_prof_disp_vrcam_nodes.fetch_add(1, Relaxed);
                g_prof_top_vrcam.fetch_add(1, Relaxed);
            } else {
                g_prof_disp_main_ticks.fetch_add(dt, Relaxed);
                g_prof_disp_main_nodes.fetch_add(1, Relaxed);
                g_prof_top_main.fetch_add(1, Relaxed);
            }
        }
        cvr_stereo_prof_node_add(prof_work, base, dt, self_time, vrcam_node);
        cvr_stereo_prof_pair_add(
            scene_rtid, work_rva, self_time, vrcam_node, !prof_top, owner_bit,
        );
        if work_rva == SCENE_DRIVER_WORK_RVA {
            let pass = &g_prof_scenepass[scene_rtid as usize];
            pass.add(vrcam_node, dt);
        }
    }
    *p_depth -= 1;
    if timeline {
        // extra: bit 8 a scene pass, bits 0-7 its rtId; nested spans add the side (bits 12-13) and depth (16-19).
        let side = if vrcam_node {
            1u32
        } else if view_key_known && view_key != 0 {
            2
        } else {
            0
        };
        let pass = 0x100u32 | scene_rtid as u32;
        if timeline_depth == 0 {
            cvr_stereo_prof_timeline_add(timeline_t0, cvr_stereo_prof_now(), work_rva, side, pass);
        } else {
            cvr_stereo_prof_timeline_add(
                timeline_t0,
                cvr_stereo_prof_now(),
                work_rva,
                16,
                pass | side << 12 | timeline_depth.min(15) << 16,
            );
        }
    }

    let node_side = *p_side;
    *p_vrcam = previous_vrcam_node;
    *p_side = previous_view_side;
    *p_known = previous_view_known;
    *p_key = previous_view_key;
    pose_exit(&pose);
    if fog_node {
        crate::render_parity::cvr_rp_set_fog_work_context(previous_fog_context);
    }
    *p_work = previous_node_work;
    *p_view = previous_node_view;
    let trace_copy = crate::preparation_trace::pairing();
    let mut previous_copy = CopySource::default();
    if trace_copy {
        let source = CopySource {
            view: pose_ctx as u64,
            node: work_rva,
            side: node_side,
        };
        set_copy_source(&source, &mut previous_copy);
    }
    // THE FINISHED FRAME at the epilogue of the second-view node that produced it (gated on the view alone: the
    // barrier that arms it sits inside that node's own work function).
    if vrcam_node {
        cvr_capture_final_frame_copy_now();
    }
    // The tonemap output snapshot, each source on its own gate and its own resource (t_tm_from_final says who armed
    // the slot), the slot released after the copy: the armed resource is a per-frame transient.
    let from_final = *tls_ptr::<bool>(Tls::TmFromFinal);
    let p_consumed = tls_ptr::<bool>(Tls::TmConsumed);
    let p_tm_rt0 = tls_ptr::<*mut c_void>(Tls::TmRt0);
    let p_tm_list = tls_ptr::<*mut c_void>(Tls::TmRt0List);
    let p_tm_state = tls_ptr::<u32>(Tls::TmRt0State);
    let p_tm_seen = tls_ptr::<bool>(Tls::TmRt0StateSeen);
    let from_tonemap = CyberpunkVR_StableFromTonemap.load(Relaxed) != 0;
    let final_grab = CyberpunkVR_FinalGrab.load(Relaxed) != 0;
    let stable_copy = CyberpunkVR_StableCopy.load(Relaxed) != 0;
    let no_lies = CyberpunkVR_NoStateLies.load(Relaxed) != 0;
    if ((from_tonemap && !from_final) || (final_grab && from_final))
        && !*p_consumed
        && !(*p_tm_rt0).is_null()
        && !(*p_tm_list).is_null()
        && stable_copy
        && (r.capture_wanted)()
        && (*p_tm_seen || !no_lies)
    {
        cvr_capture_stable_inline_copy(*p_tm_list, *p_tm_rt0, *p_tm_state, (&pose.node as *const PoseIdentity).cast());
        *p_consumed = true;
        *p_tm_rt0 = null_mut();
        *p_tm_list = null_mut();
        *p_tm_seen = false;
        // Only a copy from the PREFERRED source may claim the role.
        if from_final || from_tonemap {
            (*r.have_tonemap_source).store(true, Release);
        }
        diag(&CyberpunkVR_DebugTonemapSnaps);
    }
    let mirror_output = if mirror_copy_node {
        *p_mirror_rtv
    } else {
        null_mut()
    };
    let mirror_output_format = if mirror_copy_node {
        *p_mirror_format
    } else {
        DXGI_FORMAT_UNKNOWN
    };
    let mirror_list = if mirror_copy_node {
        *p_mirror_list
    } else {
        null_mut()
    };
    let mirror_src_state = *p_mirror_state;
    if mirror_copy_node {
        *p_mirror_active = previous_mirror_active;
        *p_mirror_rtv = previous_mirror_rtv;
        *p_mirror_format = previous_mirror_format;
        *p_mirror_list = previous_mirror_list;
    }
    if !mirror_output.is_null() {
        // The valid-window snapshot of the mirror's own source, unless a preferred source has delivered (the latch,
        // never the key alone: a frame where it found nothing must still feed the eye).
        let tonemap_src = (from_tonemap || final_grab) && (*r.have_tonemap_source).load(Acquire);
        let seen = *p_mirror_seen;
        if stable_copy
            && (r.capture_wanted)()
            && !mirror_list.is_null()
            && !tonemap_src
            && (seen || !no_lies)
        {
            diag(&*r.eye_copy_calls);
            cvr_capture_stable_inline_copy(
                mirror_list,
                mirror_output,
                mirror_src_state,
                (&pose.node as *const PoseIdentity).cast(),
            );
        } else if no_lies
            && !seen
            && !mirror_list.is_null()
            && stable_copy
            && (r.capture_wanted)()
            && !tonemap_src
        {
            diag(&CyberpunkVR_DebugForeignStateRefusals); // refused rather than guessed
        } else if stable_copy && (r.capture_wanted)() && !tonemap_src {
            diag(&*r.eye_no_list); // the output found, no list to record on
        }
        cvr_capture_publish_output(mirror_output, mirror_output_format);
        let serial = (*r.mirror_vrcam_serial).load(Acquire);
        let armed = (*r.mirror_armed_serial).load(Relaxed);
        if serial != 0
            && armed != serial
            && (*r.mirror_armed_serial)
                .compare_exchange(armed, serial, AcqRel, Acquire)
                .is_ok()
        {
            (*r.mirror_copy_armed).store(true, Release);
            diag(&CyberpunkVR_DebugMirrorCopyArms);
        }
    } else if mirror_copy_node {
        diag(&*r.eye_no_rtv); // the node ran and bound nothing recognised as its output
    }
    if trace_copy {
        let mut unused = CopySource::default();
        set_copy_source(&previous_copy, &mut unused);
    }
    result
}
