//! CAPTURE'S SHARE OF THE COMMAND-LIST HOOKS (part of formerly src/Stereo/Capture.cpp).
//!
//! OMSetRenderTargets: the cascade pass's depth target per view (the answered probe); which frame-graph node binds
//! what (the HUD surface is identified here -- a feature, not diagnostics); the bound target's size for this
//! recording thread (the crop-pass detection in the viewport hooks); the second view's output redirected to our own
//! committed target; after the bind, the 2-MRT tonemap pass and its RT0 armed for the epilogue snapshot, and the
//! second view's output latched for the mirror with the [rtvpick] account of what each bind resolved to.
//!
//! The hooks themselves stay in Capture.cpp for now: they also call into the C++ subsystems that observe the same
//! binds (target handoff and access, retained geometry, VRS, optics, the menu surface, cascades, the native probe,
//! image poses), in their order, around these.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*,
};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::capture_output::{
    cvr_capture_cascrt_note, cvr_capture_cascrt_report, cvr_capture_ensure_own_target_rtv,
};
use crate::capture_rtv::{
    cvr_capture_mirror_find_bound_rtv, cvr_capture_rtv_dim_lookup, cvr_capture_rtv_resource_lookup,
};
use crate::census_probes::{
    cvr_census_cull_count_note, cvr_census_cull_count_report, cvr_census_expo_mirror,
    cvr_census_expo_probe_copy, cvr_census_expo_probe_report, cvr_census_tile_is_grid,
    cvr_census_tile_probe_copy, cvr_census_tile_probe_report,
};
use crate::hud::{
    cvr_hud_adopt_by_node, cvr_hud_blur_signature, cvr_hud_node_note, cvr_hud_snapshot_copy,
};
use crate::vision_layer::{
    cvr_rtmap_note, cvr_vision_is_vrcam_full_size, cvr_vision_layer_signature,
    cvr_vision_matches_dispatch, cvr_vision_note_surface,
};
use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_CascRtProbe: AtomicI32;
    static CyberpunkVR_HudByNode: AtomicI32;
    static CyberpunkVR_HudNodeRva: AtomicU32;
    static CyberpunkVR_HudNodeRva2: AtomicU32;
    static CyberpunkVR_RtMapProbe: AtomicI32;
    static CyberpunkVR_HudNodeProbe: AtomicI32;
    static CyberpunkVR_VrcamOwnTarget: AtomicI32;
    static CyberpunkVR_DebugOwnTargetSubs: AtomicU64;
    static CyberpunkVR_Debug2RtBinds: AtomicU64;
    static CyberpunkVR_FinalGrab: AtomicI32;
    static CyberpunkVR_DebugRtvPickLog: AtomicI32;
    static CyberpunkVR_DebugMirrorRtvHits: AtomicU64;
    static CyberpunkVR_BarrierCounts: [AtomicU64; 8];
    static CyberpunkVR_DebugFinalArms: AtomicU64;
    static CyberpunkVR_DebugFinalGrabs: AtomicU64;
    static CyberpunkVR_HudToSecondEye: AtomicI32;
    static CyberpunkVR_VisionSnap: AtomicI32;
    static CyberpunkVR_VisionMap: AtomicI32;
    static CyberpunkVR_VisionNode: AtomicU32;
    static CyberpunkVR_VisionPick: AtomicI32;
    static CyberpunkVR_DebugVisionSnaps: AtomicU64;
    static CyberpunkVR_OpticsExpoHits: AtomicU64;
    static CyberpunkVR_OpticsExpoMirror: AtomicI32;
    static CyberpunkVR_DebugExpoVrcam: AtomicU64;
    static CyberpunkVR_DebugExpoMain: AtomicU64;
    static CyberpunkVR_DebugMirrorSrcState: AtomicU32;
    static CyberpunkVR_DebugMirrorBarrierHits: AtomicU64;
    static g_vrcam_sel_w: AtomicU32;
    static g_vrcam_sel_h: AtomicU32;
    static g_vrcam_ctx_key: AtomicU64;
}

/// The cascade pass's work function (sub_1404EDFE0, RenderShadowCascade) and the tonemap node's.
const CASCADE_NODE_RVA: u32 = 0x4E_DFE0;
const TONEMAP_WORK_RVA: usize = 0x55_7204;

/// The recording thread's render state the hooks read and write (SyncStereo.cpp's thread_locals), by index into the
/// C++ adapter that hands back this thread's variable.
#[repr(u32)]
#[derive(Clone, Copy)]
pub(crate) enum Tls {
    CurrentNodeWork = 0,
    CascadeIdx = 1,
    VrcamNodeActive = 2,
    CurRtW = 3,
    CurRtH = 4,
    MirrorCopyNodeActive = 5,
    InVrcam2Rt = 6,
    CbArmed2Rt = 7,
    TmRt0 = 8,
    TmFromFinal = 9,
    TmRt0List = 10,
    TmRt0State = 11,
    TmRt0StateSeen = 12,
    TmConsumed = 13,
    MirrorCopyRtv = 14,
    MirrorCopyRtvFormat = 15,
    MirrorCopyList = 16,
    MirrorSrcState = 17,
    MirrorSrcStateSeen = 18,
    FinalRes = 19,
    FinalState = 20,
    FinalList = 21,
    ActiveViewKnown = 22,
    ActiveViewKey = 23,
    VisionNode = 24,
    VisionOrd = 25,
    LastDisp = 26,
    CurrentNodeView = 27,
    ViewSide = 28,
    ProfDispDepth = 29,
    ProfChildTicks = 30,
}

#[repr(C)]
pub struct Runtime {
    get_desc: extern "C" fn(*mut c_void, *mut D3D12_RESOURCE_DESC) -> bool,
    exe_base: *const usize,
    hud_node_binds: *const AtomicU64,
    hud_node_unresolved: *const AtomicU64,
    own_target: *const *mut c_void,
    captured: *const AtomicPtr<c_void>,
    /// The 2-MRT pass's first four RT0 handles and their RT1 partners, and the tonemap pass's RT0 (SyncStereo.cpp's).
    seen_h0: *const AtomicUsize,
    seen_h1: *const AtomicUsize,
    tonemap_h0: *const AtomicUsize,
    have_tonemap_source: *const AtomicBool,
    mirror_pending_list: *const AtomicPtr<c_void>,
    stereo_log: extern "C" fn(*const c_char),
    // ---- the barrier hook's share
    capture_wanted: extern "C" fn() -> bool,
    /// The plugin's main log (`Log("%s", text)`), where [eyefin] has always gone.
    plain_log: extern "C" fn(*const c_char),
    /// The HUD surface, its last mip, the snapshot's freshness, demand and description (Hud.cpp's; the description
    /// under g_hud_snap_mtx), the batch-listed latch, the vision layer's stamp and the eye snapshot's (SyncStereo.cpp).
    hud_res: *const *mut c_void,
    stable_tick: *const AtomicU64,
    hud_consumed_tick: *const AtomicU64,
    hud_snap_fresh: *const AtomicBool,
    hud_snap_desc: *const D3D12_RESOURCE_DESC,
    hud_snap_lock: extern "C" fn(bool),
    hud_last_mip: *const AtomicU32,
    hud_batch_listed: *mut bool,
    vision_tick: *const AtomicU64,
    expo_main: *const AtomicPtr<c_void>,
    expo_vrcam: *const AtomicPtr<c_void>,
    /// cname_hash("vr_optics_feed"): the optics auxiliary view, which may never become a source.
    optics_feed_hash: u64,
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    get_desc: s::get_desc,
    exe_base: (&raw const s::g_exe_base).cast(),
    hud_node_binds: &raw const m::g_hud_node_binds,
    hud_node_unresolved: &raw const m::g_hud_node_unresolved,
    own_target: &raw const s::g_own_target,
    captured: &raw const s::g_captured_vrcam_res,
    seen_h0: (&raw const s::g_2rt_seen_h0).cast(),
    seen_h1: (&raw const s::g_2rt_seen_h1).cast(),
    tonemap_h0: &raw const s::g_tonemap_h0,
    have_tonemap_source: &raw const m::g_have_tonemap_source,
    mirror_pending_list: &raw const s::g_mirror_pending_list,
    stereo_log: s::stereo_log,
    capture_wanted: s::capture_wanted,
    plain_log: s::plain_log,
    hud_res: &raw const s::g_hud_res,
    stable_tick: &raw const m::g_stable_tick,
    hud_consumed_tick: &raw const m::g_hud_consumed_tick,
    hud_snap_fresh: &raw const m::g_hud_snap_fresh,
    hud_snap_desc: &raw const s::g_hud_snap_desc,
    hud_snap_lock: s::hud_snap_lock,
    hud_last_mip: &raw const m::g_hud_last_mip,
    hud_batch_listed: (&raw const m::g_hud_batch_listed).cast_mut().cast(),
    vision_tick: &raw const m::g_vision_tick,
    expo_main: &raw const s::g_expo_main,
    expo_vrcam: &raw const s::g_expo_vrcam,
    optics_feed_hash: s::OPTICS_FEED_HASH,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn diag(counter: &AtomicU64) {
    // SAFETY: an export of the C++ half.
    if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0 {
        counter.fetch_add(1, SeqCst);
    }
}
/// This thread's `which`, as a `T`.
pub(crate) unsafe fn tls<'a, T>(which: Tls) -> &'a mut T {
    &mut *crate::render_tls::field(which as u32).cast::<T>()
}
/// This thread's `which` as a raw pointer, for state held across calls that may write it themselves (the
/// dispatcher re-enters).
pub(crate) unsafe fn tls_ptr<T>(which: Tls) -> *mut T {
    crate::render_tls::field(which as u32).cast::<T>()
}
/// g_exe_base: the game's image as the C++ half recorded it (0 before that).
pub(crate) fn exe_base() -> usize {
    // SAFETY: the C++ half's global, attached at its static initialisation.
    unsafe { *rt().exe_base }
}
/// The tonemap node's work function (sub_140557204).
pub(crate) const TONEMAP_WORK: usize = TONEMAP_WORK_RVA;
fn rtv_increment() -> usize {
    // SAFETY: the game's device, or null.
    unsafe {
        let device = crate::device_creation::game_objects::game_device();
        ID3D12Device::from_raw_borrowed(&device)
            .map(|d| d.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize)
            .unwrap_or(0)
    }
}
/// The `i`th handle of a bind: the range's base plus `i` increments, or the `i`th of the array; read guarded, the
/// pointer being the engine's.
unsafe fn handle_at(
    handles: *const usize,
    i: u32,
    contiguous: bool,
    increment: usize,
) -> Option<usize> {
    if contiguous {
        code::read::<usize>(handles as usize).map(|h| h + increment * i as usize)
    } else {
        code::read::<usize>(handles.add(i as usize) as usize)
    }
}

/// Before the bind, part 1: the cascade pass's depth target per view and index (ahead of the colour gate: that pass
/// binds a depth target and usually no colour one), and which frame-graph node binds what -- the [rtmap] probe, the HUD
/// node's binds and the surface adopted by its node, the HUD node probe.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_om_observe(
    count: u32,
    handles: *const usize,
    depth: *const usize,
) {
    let r = rt();
    let base = *r.exe_base;
    let work = *tls::<usize>(Tls::CurrentNodeWork);
    let vrcam = *tls::<bool>(Tls::VrcamNodeActive);
    if CyberpunkVR_CascRtProbe.load(Relaxed) != 0
        && base != 0
        && !depth.is_null()
        && work > base
        && (work - base) as u32 == CASCADE_NODE_RVA
    {
        let ptr = code::read::<u64>(depth as usize).unwrap_or(0);
        if ptr != 0 {
            cvr_capture_cascrt_note(ptr, *tls::<i32>(Tls::CascadeIdx), vrcam);
            cvr_capture_cascrt_report();
        }
    }
    if base == 0 || handles.is_null() || count < 1 {
        return;
    }
    let rva = if work > base { (work - base) as u32 } else { 0 };
    let hud_node = CyberpunkVR_HudByNode.load(Relaxed) != 0
        && (rva == CyberpunkVR_HudNodeRva.load(Relaxed)
            || rva == CyberpunkVR_HudNodeRva2.load(Relaxed));
    let rtmap = CyberpunkVR_RtMapProbe.load(Relaxed) != 0;
    let node_probe = CyberpunkVR_HudNodeProbe.load(Relaxed) != 0;
    let mut bound = null_mut();
    if rtmap || node_probe || hud_node {
        bound = code::read::<usize>(handles as usize)
            .map_or(null_mut(), |h| cvr_capture_rtv_resource_lookup(h));
    }
    if rtmap && !bound.is_null() {
        cvr_rtmap_note(bound, vrcam, work, base);
    }
    if hud_node {
        (*r.hud_node_binds).fetch_add(1, Relaxed);
        if bound.is_null() {
            (*r.hud_node_unresolved).fetch_add(1, Relaxed);
        }
    }
    if hud_node && !bound.is_null() {
        cvr_hud_adopt_by_node(bound);
    }
    if node_probe && !bound.is_null() {
        cvr_hud_node_note(bound, vrcam);
    }
}

/// Before the bind, part 2: the primary target's size for THIS recording thread (the viewport and scissor hooks run
/// on the same thread right after and detect the crop pass by it), then the second view's output redirected to our
/// own committed target -- only inside the ctx-keyed RenderFinal2D node and only for a VRCAM-size candidate. The
/// substituted handles land in `sub` (8); true when any was substituted.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_om_substitute(
    count: u32,
    handles: *const usize,
    contiguous: i32,
    sub: *mut usize,
) -> bool {
    if !handles.is_null() && count >= 1 {
        let (mut w, mut h) = (0u32, 0u32);
        let known = code::read::<usize>(handles as usize)
            .is_some_and(|hd| cvr_capture_rtv_dim_lookup(hd, &mut w, &mut h));
        *tls::<u32>(Tls::CurRtW) = if known { w } else { 0 };
        *tls::<u32>(Tls::CurRtH) = if known { h } else { 0 };
    }
    if CyberpunkVR_VrcamOwnTarget.load(Relaxed) == 0
        || !*tls::<bool>(Tls::MirrorCopyNodeActive)
        || handles.is_null()
        || count == 0
        || count > 8
        || crate::device_creation::game_objects::game_device().is_null()
    {
        return false;
    }
    let increment = rtv_increment();
    let mut subbed = false;
    for i in 0..count {
        let Some(handle) = handle_at(handles, i, contiguous != 0, increment) else {
            return false;
        };
        *sub.add(i as usize) = handle;
        let mut vf = DXGI_FORMAT_UNKNOWN;
        let res = cvr_capture_mirror_find_bound_rtv(handle, &mut vf);
        if !res.is_null() {
            let own = cvr_capture_ensure_own_target_rtv(res, vf);
            if own != 0 {
                *sub.add(i as usize) = own;
                subbed = true;
            }
        }
    }
    subbed
}

/// After the bind: our own target published as the output when it was substituted; the 2-MRT pass (RT0 resolved
/// through the VRCAM-size candidates, RT1 ping-ponging across frames) identifying the tonemap, its RT0 armed for the
/// epilogue snapshot while the finished-frame source has not yet delivered; and inside the mirror's node, every
/// bound VRCAM candidate latched as the output with its state seeded, the [rtvpick] lines saying what each bind
/// resolved to and why a miss missed.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_om_after(
    list: *mut c_void,
    count: u32,
    handles: *const usize,
    contiguous: i32,
    subbed: bool,
) {
    let r = rt();
    if subbed {
        diag(&CyberpunkVR_DebugOwnTargetSubs);
        let own = *r.own_target;
        if !own.is_null() {
            s::MIRROR_OUTPUT.publish(own);
        }
    }
    let render_target = D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32;
    let mut is_2rt_bind = false;
    if count == 2 && !handles.is_null() && !crate::device_creation::game_objects::game_device().is_null() {
        let inc2 = rtv_increment();
        let h0 = *handles;
        let h1 = if contiguous != 0 {
            h0 + inc2
        } else {
            *handles.add(1)
        };
        let mut vf0 = DXGI_FORMAT_UNKNOWN;
        let rt0 = cvr_capture_mirror_find_bound_rtv(h0, &mut vf0);
        if !rt0.is_null() {
            is_2rt_bind = true;
            diag(&CyberpunkVR_Debug2RtBinds);
            // Tonemap identification: RT1 changed for a known RT0 => the ping-pong pass.
            let mut slot: i32 = -1;
            for i in 0..4 {
                let cell = &*r.seen_h0.add(i);
                let seen = cell.load(Acquire);
                if seen == h0 {
                    slot = i as i32;
                    break;
                }
                if seen == 0 {
                    if cell.compare_exchange(0, h0, SeqCst, SeqCst).is_ok() {
                        (*r.seen_h1.add(i)).store(h1, Release);
                        slot = -2; // just inserted; no history yet
                    } else if cell.load(Acquire) == h0 {
                        slot = i as i32;
                    }
                    if slot != -1 {
                        break;
                    }
                }
            }
            if slot >= 0 && (*r.seen_h1.add(slot as usize)).swap(h1, AcqRel) != h1 {
                let _ = (*r.tonemap_h0).compare_exchange(0, h0, SeqCst, SeqCst);
            }
            if (*r.tonemap_h0).load(Relaxed) == h0 {
                *tls::<bool>(Tls::InVrcam2Rt) = true;
                *tls::<bool>(Tls::CbArmed2Rt) = true;
            }
            // The tonemap RT0 capture, keyed on the node's WORK RVA (stable across frames, unlike the handle). ONE
            // WRITER PER SLOT: it stands down once the finished-frame source has delivered, or the snapshot's
            // source desc alternates and abandons a full-size texture per alternation.
            let final_grab = CyberpunkVR_FinalGrab.load(Relaxed) != 0;
            if (!final_grab || !(*r.have_tonemap_source).load(Acquire))
                && *tls::<usize>(Tls::CurrentNodeWork) == *r.exe_base + TONEMAP_WORK_RVA
            {
                *tls::<*mut c_void>(Tls::TmRt0) = rt0;
                *tls::<bool>(Tls::TmFromFinal) = false;
                *tls::<*mut c_void>(Tls::TmRt0List) = list;
                *tls::<u32>(Tls::TmRt0State) = render_target;
                *tls::<bool>(Tls::TmRt0StateSeen) = false; // a seed, not an observation
                *tls::<bool>(Tls::TmConsumed) = false; // arm the epilogue snapshot
            }
        }
    }
    if !is_2rt_bind {
        *tls::<bool>(Tls::InVrcam2Rt) = false; // any other bind closes the window
    }
    if !*tls::<bool>(Tls::MirrorCopyNodeActive) || handles.is_null() || count == 0 {
        return;
    }
    let increment = if crate::device_creation::game_objects::game_device().is_null() {
        0
    } else {
        rtv_increment()
    };
    for i in 0..count {
        let handle = if contiguous != 0 {
            *handles + increment * i as usize
        } else {
            *handles.add(i as usize)
        };
        let mut view_format = DXGI_FORMAT_UNKNOWN;
        let resource = cvr_capture_mirror_find_bound_rtv(handle, &mut view_format);
        // The pick is size-filtered and keeps the LAST match of a multi-RT bind; with MAIN forced to the VRCAM
        // size that is no longer safe, so the first binds say what was picked -- and for a miss, whether the
        // candidate filter refused it (known to the broad map) or the descriptor was never seen created. Reported
        // through a separate local: the diagnostic never changes the pick.
        if CyberpunkVR_DebugRtvPickLog.load(Relaxed) > 0 {
            CyberpunkVR_DebugRtvPickLog.fetch_sub(1, Relaxed);
            let (shown, via) = if resource.is_null() {
                let s = cvr_capture_rtv_resource_lookup(handle);
                (
                    s,
                    if s.is_null() {
                        " [descriptor never seen created]"
                    } else {
                        " [refused by the candidate filter]"
                    },
                )
            } else {
                (resource, "")
            };
            let mut rd = D3D12_RESOURCE_DESC::default();
            let got = !shown.is_null() && (r.get_desc)(shown, &mut rd);
            stereo_log(&format!(
                "[rtvpick] bind i={i}/{count} handle={:016X} -> res={:016X} {}{}x{} fmt={} view={} {}{via}",
                handle,
                shown as usize,
                if got { "" } else { "(no desc) " },
                if got { rd.Width } else { 0 },
                if got { rd.Height } else { 0 },
                if got { rd.Format.0 as u32 } else { 0 },
                view_format.0 as u32,
                if resource.is_null() { "not-a-candidate" } else { "MATCH-latched" },
            ));
        }
        if !resource.is_null() {
            diag(&CyberpunkVR_DebugMirrorRtvHits);
            *tls::<*mut c_void>(Tls::MirrorCopyRtv) = resource;
            *tls::<DXGI_FORMAT>(Tls::MirrorCopyRtvFormat) = view_format;
            // The engine activated this target (aliasing, -> RENDER_TARGET, Discard) right before binding it; the
            // barrier hook tracks its further transitions so the inline copy names the exact current state.
            *tls::<*mut c_void>(Tls::MirrorCopyList) = list;
            *tls::<u32>(Tls::MirrorSrcState) = render_target;
            *tls::<bool>(Tls::MirrorSrcStateSeen) = false; // a seed, not an observation
                                                           // This list runs the second view's CopyToTexture blit: the copy is triggered when the game SUBMITS it.
            (*r.mirror_pending_list).store(list, Release);
        }
    }
}

// ---- ResourceBarrier ---------------------------------------------------------------------------------------------

/// D3D12_RESOURCE_BARRIER read as plain words: the transition's resource, subresource and two states, the aliasing
/// pair, or the UAV's resource.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawBarrier {
    kind: u32,
    flags: u32,
    res: *mut c_void,
    word1: u64,
    word2: u64,
}
const _: () =
    assert!(core::mem::size_of::<RawBarrier>() == core::mem::size_of::<D3D12_RESOURCE_BARRIER>());
const TRANSITION: u32 = 0;
const ALIASING: u32 = 1;
const UAV: u32 = 2;
impl RawBarrier {
    fn subresource(&self) -> u32 {
        self.word1 as u32
    }
    fn before(&self) -> u32 {
        (self.word1 >> 32) as u32
    }
    fn after(&self) -> u32 {
        self.word2 as u32
    }
    fn transition_of(&self, res: *mut c_void) -> bool {
        self.kind == TRANSITION && self.res == res
    }
}
const fn st(s: D3D12_RESOURCE_STATES) -> u32 {
    s.0 as u32
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
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

/// The engine's barriers by kind (vr_bench ab --counters): [0] calls, [1] transitions, [2] aliasing, [3] UAV, [4]
/// split begins, [5] split ends, [6] all-subresource transitions. The port's own barriers are not counted (the caller
/// skips them).
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_count_barriers(count: u32, barriers: *const RawBarrier) {
    if barriers.is_null() {
        return;
    }
    let mut n = [1u64, 0, 0, 0, 0, 0, 0];
    for b in core::slice::from_raw_parts(barriers, count as usize) {
        match b.kind {
            TRANSITION => {
                n[1] += 1;
                if b.subresource() == D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES {
                    n[6] += 1;
                }
            }
            ALIASING => n[2] += 1,
            UAV => n[3] += 1,
            _ => {}
        }
        if b.flags & D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY.0 as u32 != 0 {
            n[4] += 1;
        }
        if b.flags & D3D12_RESOURCE_BARRIER_FLAG_END_ONLY.0 as u32 != 0 {
            n[5] += 1;
        }
    }
    for (counter, v) in CyberpunkVR_BarrierCounts.iter().zip(n) {
        if v != 0 {
            counter.fetch_add(v, Relaxed);
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_capture_barrier_clock() -> u64 {
    let mut t = 0i64;
    // SAFETY: a plain system query.
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut t);
    }
    t as u64
}
/// [7]: time inside the runtime's and driver's ResourceBarrier, in 100 ns units.
#[no_mangle]
pub extern "C" fn cvr_capture_count_barrier_time(start: u64) {
    static FREQUENCY: std::sync::LazyLock<u64> = std::sync::LazyLock::new(|| {
        let mut f = 0i64;
        // SAFETY: a plain system query.
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f);
        }
        f as u64
    });
    let elapsed = cvr_capture_barrier_clock()
        .wrapping_sub(start)
        .wrapping_mul(10_000_000)
        / *FREQUENCY;
    // SAFETY: an export of the C++ half.
    unsafe { CyberpunkVR_BarrierCounts[7].fetch_add(elapsed, Relaxed) };
}

/// After the engine's barriers, Capture's share (gated, like the snapshot, on eye-capture demand): the second view's
/// finished frame armed at its acquire, the HUD and its blur pyramid snapshotted once their last mip is shader-readable,
/// the whole-chain capture and its states, the outline layer, the cull counters, the light grid and the exposure
/// accumulators, and the states of the captured mirror and tonemap targets.
#[no_mangle]
pub unsafe extern "C" fn cvr_capture_barriers(
    list: *mut c_void,
    count: u32,
    barriers: *const RawBarrier,
    hooked: bool,
) {
    let r = rt();
    if !(r.capture_wanted)() || barriers.is_null() || !hooked {
        return;
    }
    let all = core::slice::from_raw_parts(barriers, count as usize);
    let base = *r.exe_base;
    let vrcam = *tls::<bool>(Tls::VrcamNodeActive);
    let render_target = st(D3D12_RESOURCE_STATE_RENDER_TARGET);
    let unordered = st(D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    let pixel_read = st(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    let non_pixel_read = st(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    let shader_read = pixel_read | non_pixel_read;
    let desc_of =
        |res: *mut c_void, d: &mut D3D12_RESOURCE_DESC| !res.is_null() && (r.get_desc)(res, d);

    // ---- the second view's finished frame, ARMED at the engine's own acquire (RENDER_TARGET -> UNORDERED_ACCESS)
    // inside the node that writes it, and copied at that node's epilogue. Identified by exhaustion: the only
    // full-size (relative to the selected VRCAM size) R8G8B8A8_TYPELESS target allowing both RT and UAV.
    if CyberpunkVR_FinalGrab.load(Relaxed) != 0 && vrcam {
        let vw = g_vrcam_sel_w.load(Relaxed);
        let vh = g_vrcam_sel_h.load(Relaxed);
        let (half_w, half_h) = ((vw + 1) / 2, (vh + 1) / 2);
        for b in all {
            if b.kind != TRANSITION
                || b.res.is_null()
                || b.before() != render_target
                || b.after() != unordered
            {
                continue;
            }
            let mut fd = D3D12_RESOURCE_DESC::default();
            if !desc_of(b.res, &mut fd)
                || fd.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || fd.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS
                || vw == 0
                || vh == 0
                || fd.Width <= half_w as u64
                || fd.Height <= half_h
            {
                continue;
            }
            if (fd.Flags & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET).0 == 0
                || (fd.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 == 0
            {
                continue;
            }
            *tls::<*mut c_void>(Tls::FinalRes) = b.res;
            *tls::<u32>(Tls::FinalState) = unordered;
            *tls::<*mut c_void>(Tls::FinalList) = list;
            diag(&CyberpunkVR_DebugFinalArms);
            // At most once per 5 s: armed says the target was found, copied that the epilogue delivered it.
            if diagnostics() {
                static LAST: AtomicU64 = AtomicU64::new(0);
                let now = tick();
                let last = LAST.load(Relaxed);
                if last == 0 || now.wrapping_sub(last) >= 5000 {
                    LAST.store(now, Relaxed);
                    let line = std::ffi::CString::new(format!(
                        "[eyefin] armed={} copied={}\n",
                        CyberpunkVR_DebugFinalArms.load(Relaxed),
                        CyberpunkVR_DebugFinalGrabs.load(Relaxed)
                    ))
                    .unwrap_or_default();
                    (r.plain_log)(line.as_ptr());
                }
            }
        }
    }
    // ---- the HUD snapshot, taken once the glow mips exist: the last mip becoming shader-readable is the one barrier
    // where all five subresources sit in the same state, the one point a single ALL_SUBRESOURCES transition is
    // honest. The blurred pyramid likewise on its mip 3 (the engine ping-pongs two; the later copy wins).
    let hud_res = *r.hud_res;
    if CyberpunkVR_HudToSecondEye.load(Relaxed) != 0 && !hud_res.is_null() {
        let mut hud_ready = null_mut();
        let mut blur_ready = null_mut();
        let stamp = (*r.stable_tick).load(Acquire);
        let now = tick();
        let used = (*r.hud_consumed_tick).load(Acquire);
        let live = stamp != 0
            && now.wrapping_sub(stamp) < 2000
            && (!(*r.hud_snap_fresh).load(Acquire) || (used != 0 && now.wrapping_sub(used) < 2000));
        if live {
            let hud_w = (*r.hud_snap_desc).Width;
            for b in all {
                if b.kind != TRANSITION || b.after() & pixel_read == 0 {
                    continue;
                }
                if b.res == hud_res {
                    if b.subresource() == (*r.hud_last_mip).load(Acquire) {
                        hud_ready = hud_res;
                    }
                    continue;
                }
                let mut bd = D3D12_RESOURCE_DESC::default();
                if b.subresource() == 3
                    && hud_w != 0
                    && desc_of(b.res, &mut bd)
                    && cvr_hud_blur_signature(&bd, hud_w)
                {
                    blur_ready = b.res;
                }
            }
        }
        // MAIN's finished frame and scene are released in the same call that retires the HUD; listed once.
        let hud_retired = all
            .iter()
            .any(|b| b.transition_of(hud_res) && b.after() == non_pixel_read);
        let (mut main_out, mut main_scene) = (null_mut::<c_void>(), null_mut::<c_void>());
        if hud_retired && live {
            let (w, h) = {
                let _lock = Held::take(r.hud_snap_lock);
                ((*r.hud_snap_desc).Width, (*r.hud_snap_desc).Height)
            };
            for b in all {
                if w == 0 {
                    break;
                }
                if b.kind != TRANSITION {
                    continue;
                }
                let mut rd = D3D12_RESOURCE_DESC::default();
                if !desc_of(b.res, &mut rd) {
                    continue;
                }
                if rd.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                    || rd.Width != w
                    || rd.Height != h
                    || rd.MipLevels != 1
                {
                    continue;
                }
                if !*r.hud_batch_listed {
                    stereo_log(&format!(
                        "[hud] retire batch: res={:016X} {}x{} fmt={} before={} after={}",
                        b.res as usize,
                        rd.Width,
                        rd.Height,
                        rd.Format.0 as u32,
                        b.before(),
                        b.after()
                    ));
                }
                let f = rd.Format;
                if f == DXGI_FORMAT_R8G8B8A8_UNORM
                    || f == DXGI_FORMAT_R8G8B8A8_TYPELESS
                    || f == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
                    || f == DXGI_FORMAT_R10G10B10A2_UNORM
                {
                    main_out = b.res;
                } else if f == DXGI_FORMAT_R11G11B10_FLOAT {
                    main_scene = b.res;
                }
            }
        }
        if !hud_ready.is_null() {
            cvr_hud_snapshot_copy(list, hud_ready, 0, shader_read);
        }
        if !blur_ready.is_null() {
            cvr_hud_snapshot_copy(list, blur_ready, 1, shader_read);
        }
        if hud_retired && !*r.hud_batch_listed {
            *r.hud_batch_listed = true;
            stereo_log(&format!(
                "[hud] retire batch resolved: out={:016X} scene={:016X}",
                main_out as usize, main_scene as usize
            ));
        }
    }
    // ---- whole-chain capture: during any second-view node, every large texture moved to a read state, tagged with
    // the node's work RVA -- only while the luma probe's diagnostic is on (c41b2e23d).
    let capture_tm = crate::mirror::tm_capture_enabled();
    if capture_tm && vrcam {
        for b in all {
            if b.kind != TRANSITION || b.res.is_null() || b.after() & shader_read == 0 {
                continue;
            }
            let mut rd = D3D12_RESOURCE_DESC::default();
            if !desc_of(b.res, &mut rd)
                || rd.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || rd.Width < 1000
            {
                continue;
            }
            let work = *tls::<usize>(Tls::CurrentNodeWork);
            let rva = if work != 0 {
                work.wrapping_sub(base) as u32
            } else {
                0
            };
            s::TM_INPUTS.observe(b.res, b.after(), rd.Format.0 as u32, rva);
        }
    }
    // The captured resources' LAST state kept fresh (any list, any thread).
    if capture_tm && !s::TM_INPUTS.is_empty() {
        for b in all {
            if b.kind != TRANSITION || b.res.is_null() {
                continue;
            }
            s::TM_INPUTS.update(b.res, b.after());
        }
    }
    for b in all {
        // The outline layer: identified by its signature and the dispatch that just covered it, numbered within its
        // node; the second view's own full-size one copied while the transient is still alive.
        if CyberpunkVR_VisionSnap.load(Relaxed) != 0 && b.kind == UAV && !b.res.is_null() {
            let mut vd = D3D12_RESOURCE_DESC::default();
            if desc_of(b.res, &mut vd)
                && cvr_vision_layer_signature(&vd)
                && cvr_vision_matches_dispatch(&vd, tls::<[u32; 3]>(Tls::LastDisp).as_ptr())
            {
                let work = *tls::<usize>(Tls::CurrentNodeWork);
                let rva = if base != 0 && work > base {
                    (work - base) as u32
                } else {
                    0
                };
                let node = tls::<usize>(Tls::VisionNode);
                let ord_cell = tls::<i32>(Tls::VisionOrd);
                if work != *node {
                    *node = work;
                    *ord_cell = 0;
                }
                let ord = *ord_cell;
                *ord_cell += 1;
                if CyberpunkVR_VisionMap.load(Relaxed) != 0 {
                    cvr_vision_note_surface(b.res, vrcam, rva, ord);
                }
                if vrcam
                    && rva == CyberpunkVR_VisionNode.load(Relaxed)
                    && ord == CyberpunkVR_VisionPick.load(Relaxed)
                    && cvr_vision_is_vrcam_full_size(&vd)
                {
                    cvr_hud_snapshot_copy(list, b.res, 4, unordered);
                    (*r.vision_tick).store(tick(), Release);
                    diag(&CyberpunkVR_DebugVisionSnaps);
                }
            }
        }
        if b.kind == UAV && !b.res.is_null() {
            let mut ud = D3D12_RESOURCE_DESC::default();
            if desc_of(b.res, &mut ud)
                && ud.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
                && (4..=32).contains(&ud.Width)
                && (ud.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0
            {
                cvr_census_cull_count_note(list, b.res, ud.Width as u32, vrcam);
                cvr_census_cull_count_report();
            }
            continue;
        }
        if b.kind != TRANSITION
            || b.before() != unordered
            || b.after() != shader_read
            || b.res.is_null()
        {
            continue;
        }
        let mut rd = D3D12_RESOURCE_DESC::default();
        if !(r.get_desc)(b.res, &mut rd) {
            continue;
        }
        // The per-tile light grid publishes on the same transition.
        if cvr_census_tile_is_grid(&rd) {
            cvr_census_tile_probe_copy(list, b.res, &rd, b.after(), vrcam);
            continue;
        }
        // The adapted-exposure accumulator (a 28-byte buffer leaving UAV), attributed by the virtual camera's name
        // hash: 0 is MAIN, g_vrcam_ctx_key the port's second view; every other view (a probe face, the desktop
        // window) is neither -- an unresolved view is never guessed.
        if rd.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || rd.Width != 28
            || !*tls::<bool>(Tls::ActiveViewKnown)
        {
            continue;
        }
        let vrcam_key = g_vrcam_ctx_key.load(Acquire);
        let key = *tls::<u64>(Tls::ActiveViewKey);
        let is_vrcam = vrcam_key != 0 && key == vrcam_key;
        let is_main = key == 0;
        if key == r.optics_feed_hash {
            // An auxiliary view may never become a source: a shared eye resource is refused first.
            if b.res != (*r.expo_main).load(Acquire) && b.res != (*r.expo_vrcam).load(Acquire) {
                CyberpunkVR_OpticsExpoHits.fetch_add(1, Relaxed);
                cvr_census_expo_probe_copy(list, b.res, 2);
                if CyberpunkVR_OpticsExpoMirror.load(Relaxed) != 0 {
                    cvr_census_expo_mirror(list, b.res, true);
                }
            }
            continue;
        }
        if !is_vrcam && !is_main {
            continue;
        }
        let slot = if is_vrcam {
            &*r.expo_vrcam
        } else {
            &*r.expo_main
        };
        let prev = slot.swap(b.res, AcqRel);
        if prev != b.res {
            if let Some(res) = ID3D12Resource::from_raw_borrowed(&b.res) {
                core::mem::forget(res.clone());
            }
            if !prev.is_null() {
                drop(ID3D12Resource::from_raw(prev));
            }
        }
        if is_vrcam {
            CyberpunkVR_DebugExpoVrcam.store(b.res as u64, Relaxed);
            cvr_census_expo_probe_copy(list, b.res, 1);
            cvr_census_expo_mirror(list, b.res, true);
        } else {
            CyberpunkVR_DebugExpoMain.store(b.res as u64, Relaxed);
            cvr_census_expo_mirror(list, b.res, false);
            cvr_census_expo_probe_copy(list, b.res, 0);
            cvr_census_expo_probe_report();
            cvr_census_tile_probe_report();
        }
    }
    // In-node, in-order state of the captured target and of the tonemap RT0 (same recording thread): the inline
    // snapshots name the EXACT state before, never a cross-frame guess (a hazard barrier stalls the engine's list).
    if *tls::<bool>(Tls::MirrorCopyNodeActive) {
        let rtv = *tls::<*mut c_void>(Tls::MirrorCopyRtv);
        if !rtv.is_null() {
            for b in all.iter().filter(|b| b.transition_of(rtv)) {
                *tls::<u32>(Tls::MirrorSrcState) = b.after();
                *tls::<bool>(Tls::MirrorSrcStateSeen) = true; // observed, for this resource
            }
        }
    }
    let tm = *tls::<*mut c_void>(Tls::TmRt0);
    if !tm.is_null() {
        for b in all.iter().filter(|b| b.transition_of(tm)) {
            *tls::<u32>(Tls::TmRt0State) = b.after();
            *tls::<bool>(Tls::TmRt0StateSeen) = true;
        }
    }
    let output = (*r.captured).load(Acquire);
    if output.is_null() {
        return;
    }
    for b in all.iter().filter(|b| b.transition_of(output)) {
        CyberpunkVR_DebugMirrorSrcState.store(b.after(), Relaxed);
        diag(&CyberpunkVR_DebugMirrorBarrierHits);
    }
}
