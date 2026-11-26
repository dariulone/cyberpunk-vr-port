//! THE TWO VIEWS RENDERED UNDER THE SAME SETTINGS (formerly src/Stereo/RenderParity.cpp): MAIN's AA mode and sky
//! radiance observed at its FlagCompute and given to the second view's (and the scope's) for its own call, the second
//! view's frame-graph cache key split from MAIN's, and MAIN's volumetric-fog history reprojected into the second
//! view's scattering pass when both froxel grids span one frustum.
//!
//! The data model and the arithmetic are fog_history.rs. RenderParity.cpp keeps the exports, the graph-cache thunk's
//! registry line and RenderGraphCacheObservationInstalled, and forwards its C++ names here; the fog-history bind's
//! caller address comes from its one-line C++ forwarder.
#![allow(unsafe_op_in_unsafe_fn)]
extern "C" {
    /// roomscale::PlayerIdentity's Rust half (cvr-rig's roomscale_move.rs).
    fn cvr_roomscale_player_identity() -> usize;
    /// definition_tracking::BeforeCacheLookup's Rust half.
    fn cvr_ndt_before_cache_lookup(cache: u64, use_: u32, key: u64, views: u32, missed: u64, caller: u64);
}
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_engine::code::{read, read_bytes, write};
use std::sync::{LazyLock, Mutex, MutexGuard};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

use crate::fog_history::*;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_vrcam_ctx_key: AtomicU64;
    static g_vrcam_view_w: AtomicU32;
    static g_vrcam_view_h: AtomicU32;
    static g_camObjMain: AtomicUsize;
    static CyberpunkVR_StreamlineHistoryFix: AtomicU32;
    static CyberpunkVR_RenderCachePerEye: AtomicI32;
    static CyberpunkVR_EarlyAaMode: AtomicI32;
    static CyberpunkVR_FogHistorySync: AtomicI32;
    static CyberpunkVR_SkyRadianceSync: AtomicI32;
    static CyberpunkVR_OpticsSkyRadianceSync: AtomicI32;
    static mut CyberpunkVR_OpticsSkyRadianceDebug: [f32; 8];
    static CyberpunkVR_OpticsSkyRadianceSeq: AtomicU32;
    static CyberpunkVR_OpticsSkyRadianceWrites: AtomicU64;
    static CyberpunkVR_RenderParityDebugCapture: AtomicI32;
    static CyberpunkVR_RenderParityCounters: [AtomicU64; 10];
    static CyberpunkVR_RenderParityDebugSeq: [AtomicU32; 3];
    static mut CyberpunkVR_RenderParityDebug: [[u64; 10]; 3];
    static CyberpunkVR_SkyRadianceCounters: [AtomicU64; 3];
    static CyberpunkVR_SkyRadianceDebugSeq: AtomicU32;
    static mut CyberpunkVR_SkyRadianceDebug: [f32; 8];
    static CyberpunkVR_FogReprojectionCounters: [AtomicU64; 8];
    static mut CyberpunkVR_FogCameraDebug: [[f32; 4]; 2];
    static CyberpunkVR_FogFrustumMismatch: AtomicU64;
    static mut CyberpunkVR_FogCameraDump: [[f32; 212]; 2];
    static mut CyberpunkVR_FogConstantsDump: [[f32; 396]; 2];
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
/// CVR_PARITY_DIAGNOSTIC: the parity capture switch or the runtime diagnostics.
fn parity_diag() -> bool {
    // SAFETY: an export of the C++ half.
    let capture = unsafe { CyberpunkVR_RenderParityDebugCapture.load(Relaxed) != 0 };
    capture || diagnostics()
}
fn count(counter: &AtomicU64) {
    if parity_diag() {
        counter.fetch_add(1, Relaxed);
    }
}

/// cvr::camera::CurrentNodePoseIdentity's fields this file reads.
#[repr(C)]
#[derive(Default)]
pub struct NodePose {
    valid: bool,
    view: u32,
    pose_id: u64,
    origin_serial: u64,
}
/// What RenderParity.cpp shares with this file.
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    main_view_ctx: *const AtomicUsize,
    /// g_vrcam_comp: the bound RTT component (the second view's camera).
    vrcam_comp: *const AtomicUsize,
    /// g_game_device.
    game_device: *const *mut c_void,
    /// OpenXRManager and the menu: false in a menu, without a running session or with an external pose reset pending;
    /// else the tracking origin's serial.
    xr_owner: extern "C" fn(*mut u64) -> bool,
    node_pose: extern "C" fn(*mut NodePose),
}
unsafe extern "C" {
    /// volatile int g_menuModeValue (src/Core/VrCore.cpp, C linkage).
    static g_menuModeValue: i32;
    /// cvr-rig's pose_identity.rs: the current node's pose identity.
    fn cvr_pose_node_current(out: *mut c_void);
}
/// The XR session owns the pose -- not in a menu, the session running, no external pose reset pending -- with the
/// tracking origin's serial out (formerly RenderParity.cpp's, over OpenXRManager's getters).
extern "C" fn rt_xr_owner(origin: *mut u64) -> bool {
    // SAFETY: a C global read whole; the caller's output.
    unsafe {
        if core::ptr::read_volatile(&raw const g_menuModeValue) != 0
            || !cvr_xr::events::session_running()
            || crate::xr_manager_services::cvr_cpp_xr_external_pose_reset_pending()
        {
            return false;
        }
        *origin = crate::cpp_services::cvr_cpp_xr_tracking_origin_serial();
    }
    true
}
/// cvr::camera::CurrentNodePoseIdentity's fields this file reads (formerly RenderParity.cpp's).
extern "C" fn rt_node_pose(out: *mut NodePose) {
    let mut identity = core::mem::MaybeUninit::<crate::xr_capture::PoseIdentity>::zeroed();
    // SAFETY: an identity to fill (plain data, all zero the empty one); the caller's output.
    unsafe {
        cvr_pose_node_current(identity.as_mut_ptr().cast());
        let p = identity.assume_init();
        *out = NodePose { valid: p.present(), view: p.view, pose_id: p.pose_id, origin_serial: p.head.origin_serial };
    }
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    main_view_ctx: &raw const m::g_main_view_ctx,
    vrcam_comp: &raw const m::g_vrcam_comp,
    game_device: &raw const s::g_game_device,
    xr_owner: rt_xr_owner,
    node_pose: rt_node_pose,
};
fn rt() -> &'static Runtime {
    &RT
}
fn base() -> usize {
    // SAFETY: attached.
    unsafe { *rt().exe_base }
}
fn node_pose() -> NodePose {
    let mut pose = NodePose::default();
    (rt().node_pose)(&mut pose);
    pose
}
/// QPC microseconds (XrDiagNowUs).
fn now_us() -> u64 {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQUENCY: LazyLock<i64> = LazyLock::new(|| {
        let mut f = 0i64;
        // SAFETY: an out parameter.
        let _ = unsafe { QueryPerformanceFrequency(&mut f) };
        f
    });
    if *FREQUENCY == 0 {
        return 0;
    }
    let mut t = 0i64;
    // SAFETY: an out parameter.
    let _ = unsafe { QueryPerformanceCounter(&mut t) };
    (t * 1_000_000 / *FREQUENCY) as u64
}
static OPTICS_FEED: LazyLock<u64> =
    LazyLock::new(|| crate::vrcam_config::cname_hash(b"vr_optics_feed"));
const RENDERER_GLOBAL_RVA: usize = 0x3BBCAE8;
const VOLUMETRIC_FOG_NODE_RVA: usize = 0xDDB6B8;
const PREPARE_SCENE_NODE_WORK_RVA: usize = 0x5A764C;
/// The verified native call of the graph-cache lookup.
const GRAPH_CACHE_CALLER_RVA: usize = 0x40A1D1;

#[derive(Clone, Copy, Default)]
struct Prepared {
    name: u64,
    frame: u32,
    valid: bool,
}
#[derive(Clone, Copy, Default)]
struct FogGridSample {
    owner: RenderOwner,
    frame: u32,
    grid: FogGrid,
    valid: bool,
}
#[derive(Clone, Copy, Default)]
struct FogSource {
    sample: FogSample,
    projection: FogProjection,
    grid: FogGrid,
    valid: bool,
    frustum: FogProjection,
}
#[derive(Clone, Copy)]
struct FogWork {
    camera: FogCamera,
    owner: RenderOwner,
    frame: u32,
    scattering_output: u32,
    pose: u64,
    camera_valid: bool,
}
const NO_FOG_WORK: FogWork = FogWork {
    camera: [0.0; 212],
    owner: RenderOwner::ZERO,
    frame: 0,
    scattering_output: 0,
    pose: 0,
    camera_valid: false,
};
impl RenderOwner {
    const ZERO: RenderOwner = RenderOwner {
        renderer: 0,
        player: 0,
        main_camera: 0,
        eye_camera: 0,
        origin: 0,
        eye_name: 0,
        width: 0,
        height: 0,
    };
}
thread_local! {
    static PREPARED: Cell<Prepared> = const { Cell::new(Prepared { name: 0, frame: 0, valid: false }) };
    static FOG_WORK_CONTEXT: Cell<usize> = const { Cell::new(0) };
    static FOG_WORK: RefCell<FogWork> = const { RefCell::new(NO_FOG_WORK) };
}
#[derive(Default)]
struct Aa {
    main_aa: AaSample,
    main_sky: SkyRadianceSample,
}
static AA: LazyLock<Mutex<Aa>> = LazyLock::new(|| Mutex::new(Aa::default()));
#[derive(Default)]
struct Fog {
    main: FogSource,
    grids: [FogGridSample; 2],
}
static FOG: LazyLock<Mutex<Fog>> = LazyLock::new(|| Mutex::new(Fog::default()));
fn aa() -> MutexGuard<'static, Aa> {
    AA.lock().unwrap_or_else(|e| e.into_inner())
}
fn fog() -> MutexGuard<'static, Fog> {
    FOG.lock().unwrap_or_else(|e| e.into_inner())
}

/// Who owns the render now: the renderer, the player, MAIN's and the second view's cameras, the tracking origin, the
/// second view's name and size -- a sample is only good for the same owner. `frame` takes the renderer's frame clock.
fn owner(frame: &mut u32) -> Option<RenderOwner> {
    let r = rt();
    let mut origin = 0u64;
    if !(r.xr_owner)(&mut origin) {
        return None;
    }
    let renderer: usize = read(base() + RENDERER_GLOBAL_RVA)?;
    if renderer == 0 {
        return None;
    }
    *frame = read(renderer + 0x4CC8)?;
    // SAFETY: C++ globals and exports, attached.
    let next = unsafe {
        RenderOwner {
            renderer,
            player: cvr_roomscale_player_identity(),
            main_camera: g_camObjMain.load(Acquire),
            eye_camera: (*r.vrcam_comp).load(Acquire),
            origin,
            eye_name: g_vrcam_ctx_key.load(Acquire),
            width: g_vrcam_view_w.load(Acquire),
            height: g_vrcam_view_h.load(Acquire),
        }
    };
    next.valid().then_some(next)
}
/// The exact resource table the native SRV binder uses: pool + 0x2F200 + (handle - 1) * 0xB0, the wrapper at +0x18,
/// the SRV at +8, the D3D resource at -0x28. Identities only; a remembered game pointer is never dereferenced.
fn allocation(handle: u32) -> Option<FogAllocation> {
    if handle == 0 || handle > 0x7fff_ffff {
        return None;
    }
    let pool: usize = read(base() + 0x3BC_DCA8)?;
    if pool == 0 {
        return None;
    }
    let record = pool + 0x2F200 + (handle as usize - 1) * 0xB0;
    let value = FogAllocation {
        pool,
        resource: read(record - 0x28)?,
        object: read(record + 0x18)?,
        srv: read(record + 8)?,
    };
    value.valid().then_some(value)
}
/// The fog node's integrated output handle.
fn fog_output() -> u32 {
    let wc = FOG_WORK_CONTEXT.get();
    if wc == 0 {
        return 0;
    }
    (|| -> Option<u32> {
        let context: usize = read(wc + 0x18)?;
        let state: usize = if context != 0 {
            read(context + 0x2B78)?
        } else {
            0
        };
        if state == 0 {
            return None;
        }
        read(state + 0x18)
    })()
    .unwrap_or(0)
}

/// The cache key: the prepared second view's (or scope's) graph salted by its name -- consumed only by its verified
/// native call in the same render frame, and not before MAIN's AA is known -- and every graph built under a
/// graph-builder patch salted apart from the native ones (StageReplay).
fn graph_key(frame: u32, mut hash: u64, caller: usize) -> u64 {
    let site = base() + GRAPH_CACHE_CALLER_RVA;
    let prepared = PREPARED.get();
    // SAFETY: exports of the C++ half.
    let (per_eye, vrcam) = unsafe {
        (
            CyberpunkVR_RenderCachePerEye.load(Relaxed) != 0,
            g_vrcam_ctx_key.load(Acquire),
        )
    };
    if per_eye
        && prepared.valid
        && frame == prepared.frame
        && caller == site
        && prepared.name != 0
        && (prepared.name == vrcam || prepared.name == *OPTICS_FEED)
    {
        let mut current = 0;
        let ready = owner(&mut current)
            .is_some_and(|o| current == frame && aa().main_aa.read(&o, frame).is_some());
        if ready {
            hash = vrcam_graph_hash(hash, prepared.name);
            // SAFETY: an export.
            count(unsafe { &CyberpunkVR_RenderParityCounters[0] });
        }
    }
    if caller == site {
        hash ^= crate::stage_replay_runtime::cvr_srt_graph_salt();
    }
    hash
}
/// Called by the graph-cache thunk (src/Hooks/RenderGraphCacheThunk.asm) with the lookup's key.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn CvrRenderGraphKey(
    frame: u32,
    hash: u64,
    caller: usize,
    cache: usize,
    views: u32,
    missed: usize,
) -> u64 {
    let key = graph_key(frame, hash, caller);
    // SAFETY: definition_tracking::BeforeCacheLookup (the definition tracking's own export).
    unsafe { cvr_ndt_before_cache_lookup(
        cache as u64,
        frame,
        key,
        views,
        missed as u64,
        caller as u64,
    ) };
    key
}
#[no_mangle]
pub extern "C" fn cvr_rp_clear_prepared_graph() {
    PREPARED.set(Prepared::default());
}
/// The view a one-view preparation is for, with the render frame.
#[no_mangle]
pub extern "C" fn cvr_rp_observe_prepared_graph(manager: usize) {
    PREPARED.set(Prepared::default());
    let prepared = (|| -> Option<Prepared> {
        if manager == 0 || read::<u32>(manager + 0x54)? != 1 {
            return None;
        }
        let views: usize = read(manager + 0x48)?;
        let view: usize = if views != 0 { read(views)? } else { 0 };
        if view == 0 {
            return None;
        }
        let name: u64 = read(view + 0x28)?;
        let mut frame = 0u32;
        // SAFETY: an out parameter.
        unsafe { crate::framegen_inputs::cvr_framegen_read_render_frame_index(&mut frame) }
            .then_some(Prepared {
                name,
                frame,
                valid: true,
            })
    })();
    if let Some(p) = prepared {
        PREPARED.set(p);
    }
}
unsafe fn store_debug<T: Copy, const N: usize>(slot: *mut [T; N], values: &[T]) {
    let n = values.len().min(N);
    core::ptr::copy_nonoverlapping(values.as_ptr(), slot.cast::<T>(), n);
}

/// FlagCompute with MAIN's AA: MAIN's mode and sky radiance (viewData + 0xFA4, + 0x440) observed at MAIN's call; at
/// the second view's and the scope's, MAIN's (at most one frame old, same owner) written before the original and the
/// AA mode put back after it. `ready`: the AA was agreed, so the native bit 33 is meaningful.
pub unsafe fn compute_flags_with_main_aa(
    original: usize,
    a1: *mut c_void,
    a2: i64,
    context: i64,
    view: i64,
    ready: &mut bool,
) -> i64 {
    *ready = false;
    let mut restore: Option<(usize, u32)> = None;
    if context != 0 && view != 0 {
        let (ctx, v) = (context as usize, view as usize);
        let mut frame = 0u32;
        let found = owner(&mut frame);
        let o = found.unwrap_or_default();
        let (mut name, mut mode) = (0u64, 0u32);
        let valid = found.is_some()
            && read::<u64>(ctx + 0x28).map(|n| name = n).is_some()
            && read::<u32>(v + 0xFA4).map(|m| mode = m).is_some()
            && mode <= 8;
        let mut native_sky: SkyRadiance = [0.0; 4];
        let sky_valid = valid
            && read::<SkyRadiance>(v + 0x440)
                .map(|s| native_sky = s)
                .is_some()
            && valid_sky_radiance(&native_sky);
        let (mut desired, mut change) = (0u32, false);
        let (mut desired_sky, mut change_sky): (SkyRadiance, bool) = ([0.0; 4], false);
        let main_ctx = (*rt().main_view_ctx).load(Acquire);
        {
            let mut s = aa();
            if !valid {
                *s = Aa::default();
            } else if is_main_aa_context(ctx, name, main_ctx) {
                s.main_aa = AaSample {
                    owner: o,
                    frame,
                    mode,
                    valid: true,
                };
                count(&CyberpunkVR_RenderParityCounters[1]);
                s.main_sky = SkyRadianceSample {
                    owner: o,
                    color: native_sky,
                    frame,
                    valid: sky_valid,
                };
                if sky_valid {
                    count(&CyberpunkVR_SkyRadianceCounters[0]);
                }
            } else if name == o.eye_name || name == *OPTICS_FEED {
                let optics = name == *OPTICS_FEED;
                let available = s.main_aa.read(&o, frame);
                if let Some(d) = available {
                    desired = d;
                }
                let apply = available.is_some()
                    && CyberpunkVR_EarlyAaMode.load(Relaxed) != 0
                    && CyberpunkVR_StreamlineHistoryFix.load(Relaxed) != 0;
                *ready = apply;
                change = apply && mode != desired;
                if available.is_none() {
                    count(&CyberpunkVR_RenderParityCounters[3]);
                }
                let sky_available = sky_valid
                    && s.main_sky
                        .read(&o, frame)
                        .map(|c| desired_sky = c)
                        .is_some();
                change_sky = CyberpunkVR_SkyRadianceSync.load(Relaxed) != 0
                    && (!optics || CyberpunkVR_OpticsSkyRadianceSync.load(Relaxed) != 0)
                    && sky_available
                    && native_sky != desired_sky;
                if optics {
                    CyberpunkVR_OpticsSkyRadianceSeq.fetch_add(1, Relaxed);
                    let both: [f32; 8] = core::array::from_fn(|i| {
                        if i < 4 {
                            native_sky[i]
                        } else {
                            desired_sky[i - 4]
                        }
                    });
                    store_debug(&raw mut CyberpunkVR_OpticsSkyRadianceDebug, &both);
                    CyberpunkVR_OpticsSkyRadianceSeq.fetch_add(1, Relaxed);
                    if change_sky {
                        CyberpunkVR_OpticsSkyRadianceWrites.fetch_add(1, Relaxed);
                    }
                }
                if parity_diag() {
                    if !sky_available {
                        CyberpunkVR_SkyRadianceCounters[2].fetch_add(1, Relaxed);
                    }
                    CyberpunkVR_SkyRadianceDebugSeq.fetch_add(1, Relaxed);
                    let both: [f32; 8] = core::array::from_fn(|i| {
                        if i < 4 {
                            native_sky[i]
                        } else {
                            desired_sky[i - 4]
                        }
                    });
                    store_debug(&raw mut CyberpunkVR_SkyRadianceDebug, &both);
                    CyberpunkVR_SkyRadianceDebugSeq.fetch_add(1, Relaxed);
                }
            }
            if parity_diag() {
                CyberpunkVR_RenderParityDebugSeq[0].fetch_add(1, Relaxed);
                let debug = [
                    ctx as u64,
                    v as u64,
                    main_ctx as u64,
                    frame as u64,
                    mode as u64,
                    desired as u64,
                    name,
                    valid as u64,
                    o.main_camera as u64,
                    o.eye_camera as u64,
                ];
                store_debug(
                    (&raw mut CyberpunkVR_RenderParityDebug).cast::<[u64; 10]>(),
                    &debug,
                );
                CyberpunkVR_RenderParityDebugSeq[0].fetch_add(1, Relaxed);
            }
        }
        // Before native preparation: +219730 updates the CPU sky storage the sky-modulated local lights then
        // sample; only this radiance float4 is copied.
        if change_sky && write(v + 0x440, desired_sky) {
            count(&CyberpunkVR_SkyRadianceCounters[1]);
        }
        if change {
            if write(v + 0xFA4, desired) {
                restore = Some((v + 0xFA4, mode));
                count(&CyberpunkVR_RenderParityCounters[2]);
            } else {
                *ready = false;
            }
        }
    } else {
        *aa() = Aa::default();
    }
    let original: unsafe extern "C" fn(*mut c_void, i64, i64, i64) -> i64 =
        core::mem::transmute(original);
    let result = original(a1, a2, context, view);
    if let Some((address, saved)) = restore {
        write(address, saved);
    }
    result
}
/// ComputeFlagsWithMainAa for the C++ callers.
#[no_mangle]
pub unsafe extern "C" fn cvr_rp_compute_flags(
    original: usize,
    a1: *mut c_void,
    a2: i64,
    context: i64,
    view: i64,
    ready: *mut bool,
) -> i64 {
    let mut agreed = false;
    let result = compute_flags_with_main_aa(original, a1, a2, context, view, &mut agreed);
    if !ready.is_null() {
        *ready = agreed;
    }
    result
}
#[no_mangle]
pub extern "C" fn cvr_rp_merge_view_flags(desired: u64, native: u64, ready: bool) -> u64 {
    merge_view_flags(desired, native, ready)
}
/// The fog node's work context for this thread; the previous one is returned for the caller to put back.
#[no_mangle]
pub extern "C" fn cvr_rp_set_fog_work_context(context: usize) -> usize {
    let previous = FOG_WORK_CONTEXT.replace(context);
    FOG_WORK.with(|w| *w.borrow_mut() = NO_FOG_WORK);
    previous
}
fn fog_active() -> bool {
    // SAFETY: exports.
    let on = unsafe {
        CyberpunkVR_FogHistorySync.load(Relaxed) != 0
            || CyberpunkVR_RenderParityDebugCapture.load(Relaxed) != 0
    };
    on || diagnostics()
}
/// The constant uploads the fog needs: the prepare-scene fog constants (1584 bytes: the froxel grid per view) and the
/// volumetric-fog node's camera (848 bytes: its projection, for this thread's fog work).
#[no_mangle]
pub unsafe extern "C" fn cvr_rp_observe_fog_constants(size: u32, source: *const c_void) {
    let side = crate::render_tls::view_side();
    if !fog_active() || source.is_null() || !(0..=1).contains(&side) {
        return;
    }
    let side = side as usize;
    let work = crate::render_tls::current_node_work();
    let base = base();
    if size != 1584
        && !(size == 848 && FOG_WORK_CONTEXT.get() != 0 && work == base + VOLUMETRIC_FOG_NODE_RVA)
    {
        return;
    }
    let mut frame = 0u32;
    let Some(o) = owner(&mut frame) else {
        return;
    };
    if size == 1584 && work == base + PREPARE_SCENE_NODE_WORK_RVA {
        let mut constants = [0u8; 1584];
        let grid = if read_bytes(source as usize, &mut constants) {
            read_fog_grid(&constants)
        } else {
            None
        };
        if parity_diag() {
            core::ptr::copy_nonoverlapping(
                constants.as_ptr(),
                (&raw mut CyberpunkVR_FogConstantsDump)
                    .cast::<[f32; 396]>()
                    .add(side)
                    .cast::<u8>(),
                1584,
            );
        }
        fog().grids[side] = FogGridSample {
            owner: o,
            frame,
            grid: grid.unwrap_or_default(),
            valid: grid.is_some(),
        };
        count(&CyberpunkVR_FogReprojectionCounters[if grid.is_some() { 6 } else { 7 }]);
    } else if size == 848 && FOG_WORK_CONTEXT.get() != 0 {
        let pose = node_pose();
        FOG_WORK.with(|w| {
            let mut w = w.borrow_mut();
            let mut valid = pose.valid && pose.view == side as u32 + 1;
            if valid {
                let mut bytes = [0u8; 848];
                valid = read_bytes(source as usize, &mut bytes);
                if valid {
                    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
                        w.camera[i] = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    }
                    valid = fog_current_projection(&w.camera).is_some();
                }
            }
            w.camera_valid = valid;
            w.owner = o;
            w.frame = frame;
            w.pose = pose.pose_id;
            if valid && parity_diag() {
                CyberpunkVR_FogReprojectionCounters[5].fetch_add(1, Relaxed);
                let c = &w.camera;
                let debug = [c[34], c[38], c[FOG_CAMERA_JITTER], c[FOG_CAMERA_JITTER + 1]];
                store_debug(
                    (&raw mut CyberpunkVR_FogCameraDebug)
                        .cast::<[f32; 4]>()
                        .add(side),
                    &debug,
                );
                store_debug(
                    (&raw mut CyberpunkVR_FogCameraDump)
                        .cast::<[f32; 212]>()
                        .add(side),
                    c,
                );
            }
        });
    }
}
/// The fog node's history handles: the scattering output noted, and at the integration's history resolve MAIN's
/// history published (with its projection, grid and frustum) for the second view to borrow.
#[no_mangle]
pub unsafe extern "C" fn cvr_rp_synchronize_fog_history(
    registry: usize,
    handle: *mut u32,
    logical_id: u32,
    caller: usize,
) {
    let side = crate::render_tls::view_side();
    let base = base();
    if !fog_active()
        || handle.is_null()
        || crate::render_tls::current_node_work() != base + VOLUMETRIC_FOG_NODE_RVA
        || !(0..=1).contains(&side)
    {
        return;
    }
    if caller == base + 0x77C609 {
        FOG_WORK.with(|w| w.borrow_mut().scattering_output = *handle);
        return;
    }
    if caller != base + 0x77D01F {
        return;
    }
    // Identified by the exact node AND the native return address: the resolved logical key is not the
    // declaration's 0x49471DDA.
    let mut frame = 0u32;
    let found = owner(&mut frame);
    let o = found.unwrap_or_default();
    let pose = node_pose();
    let now = now_us();
    let work = FOG_WORK.with(|w| *w.borrow());
    let valid = found.is_some()
        && pose.valid
        && pose.view == side as u32 + 1
        && pose.origin_serial == o.origin
        && work.camera_valid
        && work.owner == o
        && work.frame == frame
        && work.pose == pose.pose_id;
    let mut f = fog();
    if parity_diag() {
        let index = 1 + side as usize;
        CyberpunkVR_RenderParityDebugSeq[index].fetch_add(1, Relaxed);
        let m = &f.main.sample;
        let debug = [
            frame as u64,
            pose.pose_id,
            registry as u64,
            *handle as u64,
            m.frame as u64,
            m.pose,
            m.registry as u64,
            m.handle as u64,
            logical_id as u64,
            ((valid as u64) << 32) | fog_output() as u64,
        ];
        store_debug(
            (&raw mut CyberpunkVR_RenderParityDebug)
                .cast::<[u64; 10]>()
                .add(index),
            &debug,
        );
        CyberpunkVR_RenderParityDebugSeq[index].fetch_add(1, Relaxed);
    }
    if !valid {
        f.main = FogSource::default();
        count(&CyberpunkVR_RenderParityCounters[7]);
        return;
    }
    if side == 0 {
        let grid = f.grids[0];
        let source = (|| -> Option<FogSource> {
            if !(grid.valid && grid.owner == o && grid.frame == frame) {
                return None;
            }
            let allocation = allocation(*handle)?;
            let projection = fog_current_projection(&work.camera)?;
            let frustum = fog_frustum(&work.camera)?;
            Some(FogSource {
                sample: FogSample {
                    owner: o,
                    allocation,
                    registry,
                    pose: pose.pose_id,
                    stamp_us: now,
                    frame,
                    handle: *handle,
                },
                projection,
                grid: grid.grid,
                valid: true,
                frustum,
            })
        })();
        match source {
            Some(s) => {
                f.main = s;
                count(&CyberpunkVR_RenderParityCounters[4]);
                count(&CyberpunkVR_FogReprojectionCounters[0]);
            }
            None => {
                f.main = FogSource::default();
                count(&CyberpunkVR_RenderParityCounters[8]);
            }
        }
    }
    // The integration input is never replaced: it belongs to this view's current grid.
}

/// The descriptor the patched fog camera is uploaded through: one CPU descriptor per recording thread out of a private
/// 128-entry heap (the engine copies it at dispatch, as it does its own scratch descriptor; b6's slot is untouched).
struct Descriptors {
    heap: usize,
    device: usize,
    stride: u32,
    next: u32,
}
static DESCRIPTORS: Mutex<Descriptors> = Mutex::new(Descriptors {
    heap: 0,
    device: 0,
    stride: 0,
    next: 0,
});
thread_local! {
    static DESCRIPTOR: Cell<usize> = const { Cell::new(0) };
}
unsafe fn bind_fog_camera(camera: &FogCamera) -> bool {
    let base = base();
    let device = *rt().game_device;
    if base == 0 || device.is_null() {
        return false;
    }
    let mut descriptor = DESCRIPTOR.get();
    {
        let mut d = DESCRIPTORS.lock().unwrap_or_else(|e| e.into_inner());
        if d.device != 0 && d.device != device as usize {
            return false;
        }
        if descriptor == 0 {
            if d.next >= 128 {
                return false;
            }
            if d.heap == 0 {
                let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
                    return false;
                };
                let desc = D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    NumDescriptors: 128,
                    ..Default::default()
                };
                let Ok(heap) = dev.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) else {
                    return false;
                };
                d.stride = dev.GetDescriptorHandleIncrementSize(desc.Type);
                // Kept for the process, as the C++ half's static ComPtr was.
                d.heap = heap.into_raw() as usize;
                d.device = device as usize;
            }
            let heap_raw = d.heap as *mut c_void;
            let Some(heap) = ID3D12DescriptorHeap::from_raw_borrowed(&heap_raw) else {
                return false;
            };
            descriptor =
                heap.GetCPUDescriptorHandleForHeapStart().ptr + (d.next * d.stride) as usize;
            d.next += 1;
            DESCRIPTOR.set(descriptor);
        }
    }
    let current: unsafe extern "C" fn() -> usize = core::mem::transmute(base + 0x337230);
    let current = current();
    let state: usize = if current != 0 {
        read(current + 0x60).unwrap_or(0)
    } else {
        0
    };
    if state == 0 {
        return false;
    }
    let upload: unsafe extern "C" fn(u32, *const c_void, usize) =
        core::mem::transmute(base + 0x333B88);
    upload(
        core::mem::size_of::<FogCamera>() as u32,
        camera.as_ptr().cast(),
        descriptor,
    );
    let bind: unsafe extern "C" fn(usize, u8, u32, u32, *const usize, bool) =
        core::mem::transmute(base + 0x336D3C);
    bind(state, 2, 2, 1, &descriptor, true);
    true
}
/// The fog history bind's trampoline (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_rp_fog_srv_original: AtomicUsize = AtomicUsize::new(0);
/// The address the hook returns to (`_ReturnAddress()`): frame 0 is the hook (this inlined), frame 1 its caller.
#[inline(always)]
fn return_address() -> usize {
    let mut frames = [core::ptr::null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None) };
    if n == 0 { 0 } else { frames[0] as usize }
}
/// The fog history detour (formerly RenderParity.cpp's BindFogHistory adapter): the bind with its caller.
///
/// # Safety
/// The engine's call with its arguments.
#[no_mangle]
pub unsafe extern "C" fn cvr_rp_fog_history_hook(slot: u32, handle: *mut u32, unused: usize, mip: u32, stage: u8) -> i64 {
    let caller = return_address();
    // SAFETY: as documented.
    unsafe { cvr_rp_bind_fog_history(slot, handle, unused, mip, stage, caller) }
}
/// The history SRV bind (sub_1403E05F8): at the scattering shader's t1 (+61CB63's call, return 0x77C6A6) for the
/// second view only, MAIN's history handle is bound instead, with the camera's previous-VP replaced by MAIN's
/// projection -- when the owner, frame, pose, allocation, grid and frustum all agree.
#[no_mangle]
pub unsafe extern "C" fn cvr_rp_bind_fog_history(
    slot: u32,
    handle: *mut u32,
    unused: usize,
    mip: u32,
    stage: u8,
    caller: usize,
) -> i64 {
    let base = base();
    let original: unsafe extern "C" fn(u32, *mut u32, usize, u32, u8) -> i64 =
        core::mem::transmute(cvr_rp_fog_srv_original.load(Relaxed));
    let fog_context = FOG_WORK_CONTEXT.get();
    let work = FOG_WORK.with(|w| *w.borrow());
    if CyberpunkVR_FogHistorySync.load(Relaxed) == 0
        || caller != base + 0x77C6A6
        || slot != 1
        || mip != 0
        || stage != 2
        || crate::render_tls::current_node_work() != base + VOLUMETRIC_FOG_NODE_RVA
        || crate::render_tls::view_side() != 1
        || handle.is_null()
        || *handle == 0
        || fog_context == 0
        || !work.camera_valid
    {
        return original(slot, handle, unused, mip, stage);
    }
    let mut frame = 0u32;
    let pose = node_pose();
    let checked = (|| -> Option<(RenderOwner, usize)> {
        let o = owner(&mut frame)?;
        if o != work.owner || frame != work.frame || !pose.valid || pose.pose_id != work.pose {
            return None;
        }
        let registry_pointer: usize = read(fog_context + 8)?;
        if registry_pointer == 0 {
            return None;
        }
        Some((o, read(registry_pointer)?))
    })();
    let Some((o, registry)) = checked else {
        return original(slot, handle, unused, mip, stage);
    };
    let (source, grid) = {
        let f = fog();
        (f.main, f.grids[1])
    };
    if !source.valid || !grid.valid || grid.owner != o || grid.frame != frame {
        count(&CyberpunkVR_FogReprojectionCounters[2]);
        return original(slot, handle, unused, mip, stage);
    }
    if !fog_frustum(&work.camera).is_some_and(|own| same_fog_frustum(&own, &source.frustum)) {
        CyberpunkVR_FogFrustumMismatch.fetch_add(1, Relaxed);
        return original(slot, handle, unused, mip, stage);
    }
    let patched = (|| -> Option<FogCamera> {
        let live = allocation(source.sample.handle)?;
        let scattering = allocation(work.scattering_output)?;
        let integrated = allocation(fog_output())?;
        can_borrow_fog(
            &source.sample,
            &o,
            registry,
            frame,
            pose.pose_id,
            now_us(),
            &live,
            &scattering,
            &integrated,
            &source.grid,
            &grid.grid,
        )
        .then_some(())?;
        apply_fog_history_projection(&work.camera, &source.projection)
    })();
    let Some(patched) = patched else {
        count(&CyberpunkVR_FogReprojectionCounters[3]);
        return original(slot, handle, unused, mip, stage);
    };
    if !bind_fog_camera(&patched) {
        count(&CyberpunkVR_FogReprojectionCounters[4]);
        return original(slot, handle, unused, mip, stage);
    }
    let mut borrowed = source.sample.handle;
    if parity_diag() {
        CyberpunkVR_FogReprojectionCounters[1].fetch_add(1, Relaxed);
        CyberpunkVR_RenderParityCounters[5].fetch_add(1, Relaxed);
    }
    original(slot, &mut borrowed, unused, mip, stage)
}
