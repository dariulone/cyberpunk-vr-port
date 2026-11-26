//! LocateCamera (formerly src/Hooks/LocateCamera.cpp): where the engine locates the camera -- the serialised camera
//! setup buffer SerializeSetup fills from the component (2.5: movss [rbx+20h],xmm0 / lea rdx,[rsp+20h]). It composes
//! the heading and publishes it for PatchCamera, the head translation recipe and delta, the view frame for the solve,
//! the located frame, the braindance and takeover writes and pushes, the labels of the scene and takeover cameras, and
//! the ShotSnap packet. It reads its clean base from g_engineCamQuat -- the quaternion PatchCamera snapshotted BEFORE
//! its own write: reading the camera's own quaternion back is the documented way to spin the camera up without bound.
//!
//! The long notes on every decision are at the C++ original's lines; this port keeps the decisions and their order,
//! operation for operation. RefreshPlayerCameraState (the player's RTTI flags) stays C++ until the engine's scripting
//! is in Rust (src/Camera/PlayerCameraState.cpp).
use super::body_heading::{cvr_body_centre_yaw, cvr_body_read_tracking_yaw, cvr_body_tracking_yaw};
use super::camera_link::{
    cvr_camera_anchor_recipe_publish, cvr_camera_body_anchor_yaw, cvr_camera_cam_write_quat_publish, cvr_camera_cam_write_quat_read,
    cvr_camera_cam_write_record_push, cvr_camera_located_frame_publish, cvr_camera_main_eye_centre_read, cvr_camera_view_frame_publish, HeadPose,
    LocatedCameraFrame, Vec2, ViewFrame,
};
use super::camera_math::{
    cvr_camera_desired_half_ipd, cvr_camera_plausible_unit_quaternion, cvr_camera_plausible_unit_vector3, cvr_camera_right_vector_from_quaternion,
    cvr_camera_world_scale,
};
use super::pose_identity::{cvr_pose_invalidate, cvr_pose_publish_setup};
use super::pose_transport::cvr_pose_transport_serialized_component;
use super::roomscale_move::{cvr_roomscale_camera_consumed_at, cvr_roomscale_physical_body_heading_owned};
use super::xinput::InputControls;
use cvr_body::cmath::{atan2, cos, sin, sqrt};
use cvr_body::policy::is_passenger_window_combat;
use cvr_camera::anchor::{active_vehicle_camera_offset, av, compose_view_translation, make_anchor_recipe};
use cvr_camera::frames::{composition_base_yaw, use_native_main_heading};
use cvr_camera::scene_comfort::{angle_deg, SceneCameraComfort, SceneComfortSettings};
use cvr_engine::code::{image_base, read, write};
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Overlay/LiveControlsUi.hpp AdsCameraTelemetryUiState.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AdsCameraTelemetryUiState {
    pub available: i32,
    pub aiming: i32,
    pub baseline_valid: i32,
    pub samples: u32,
    pub residual: [f32; 3],
    pub delta: [f32; 3],
    pub peak: [f32; 3],
}

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut g_locateCameraHits: u64;
    static mut g_dbgEntryYaw: f32;
    static mut g_dbgEntryPosX: f32;
    static mut g_dbgEntryPosY: f32;
    static mut g_dbgEntryPosZ: f32;
    static g_engineCamQuat: [f32; 4];
    static g_engineCamQuatValid: u32;
    static mut g_headingSy: f32;
    static mut g_headingCy: f32;
    static mut g_headingPitchS: f32;
    static mut g_headingPitchC: f32;
    static mut g_headingValid: u32;
    static g_gamePitchRadians: f32;
    static g_bdActive: AtomicI32;
    static g_bdScenePoseValid: AtomicI32;
    static g_bdSceneQuat: [f32; 4];
    static g_bdScenePosFP: [AtomicI32; 3];
    static mut g_headQuatComposed: [f32; 4];
    static mut g_headQuatValid: u32;
    static mut g_bdPushQuat: [f32; 4];
    static g_lastLocateSeq: AtomicU32;
    static mut g_lastLocatePosFP: [i32; 3];
    static mut g_lastIpdShiftFP: [i32; 3];
    static mut g_lastLocateQuat: [f32; 4];
    static g_headDeltaFP: [AtomicI32; 3];
    static g_headDeltaValid: AtomicU32;
    static g_locateCenterFP: [AtomicI32; 3];
    static g_locatePosFP: [AtomicI32; 3];
    static g_locatePosValid: AtomicI32;
    static g_devCamPosValid: AtomicI32;
    static g_devCamPosFP: [AtomicI32; 3];
    static g_devCamAimValid: AtomicI32;
    static g_devCamAimYaw: f32;
    static g_camObjMain: AtomicUsize;
    static g_camObjVrcam: AtomicUsize;
    static g_lensComp: AtomicUsize;
    static g_viewYawUsedRad: f32;
    static g_viewYawUsedValid: i32;
    static g_isAiming: bool;
    static g_isInVehicle: bool;
    static g_sceneTier: AtomicI32;
    static g_VrikFrameEpoch: AtomicU32;
    static CyberpunkVR_AdsWeaponZoomSync: i32;
    static mut CyberpunkVR_DebugAdsZoomWrites: u64;
    static mut CyberpunkVR_DebugAdsZoomRejects: u64;
    static CyberpunkVR_MainAdsZoomFactor: f32;
    static CyberpunkVR_SightZoom: i32;
    static mut CyberpunkVR_DebugSceneComfortLagDeg: f32;
    static mut CyberpunkVR_DebugSceneComfortCuts: u32;
    static mut CyberpunkVR_DebugSceneLabelPublished: u64;
    static CyberpunkVR_CamWriteInPatch: i32;
    static CyberpunkVR_CamComposeAtWrite: i32;
    static CyberpunkVR_OneSamplePerFrame: i32;
    static CyberpunkVR_BdQuatFromBuffer: i32;
    static mut CyberpunkVR_DebugBdQuatFromBuffer: u64;
    static CyberpunkVR_BdBaseFromLocate: i32;
    static mut CyberpunkVR_DebugBdBaseFromLocate: u64;
    static mut CyberpunkVR_DebugBdBaseFromScript: u64;
    static CyberpunkVR_BdQuatFromWriteSite: i32;
    static mut CyberpunkVR_DebugBdQuatFromWriteSite: u64;
    static mut CyberpunkVR_DebugTidLocateCam: u32;
    static CyberpunkVR_ViewYawFromEngine: i32;
    static CyberpunkVR_EngineBodyYawValid: i32;
    static CyberpunkVR_EngineBodyYawZ: f32;
    static CyberpunkVR_EngineBodyYawW: f32;
    static CyberpunkVR_VehicleAnchorFromViewYaw: i32;
    static CyberpunkVR_HeadTranslationInPatch: i32;
    static CyberpunkVR_BdIpdInLocate: i32;
    static CyberpunkVR_IpdInWorldPos: i32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_BdMainPosFromScene: i32;
    static mut CyberpunkVR_DebugBdMainPos: u64;
    static CyberpunkVR_DevCamOffsetX: f32;
    static CyberpunkVR_DevCamOffsetY: f32;
    static CyberpunkVR_DevCamOffsetZ: f32;
    static mut CyberpunkVR_DebugBdIpdLocate: u64;
    static mut CyberpunkVR_DebugTakeoverPush: u64;
    static CyberpunkVR_BdPushTransform: i32;
    static CyberpunkVR_BdPushBase: i32;
    static mut CyberpunkVR_DebugBdPushTransform: u64;
    static mut CyberpunkVR_DebugBdPushMain: u64;
    fn cvr_cpp_telemetry_locate(hits: u32, rbx: u64, xmm0: f32);
    fn cvr_cpp_refresh_player_camera_state();
    fn cvr_cpp_shot_shared() -> *mut f32;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_bd_push_owns_composition() -> bool;
    fn cvr_locate_owns_takeover() -> bool;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_pose_vehicle_state() -> i32;
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_xr_render_eye_index() -> i32;
    fn cvr_xr_acquire_camera_pose_frame(head: *mut HeadPose, consumed: *mut Vec2, sequence: *mut u64) -> bool;
    fn cvr_xr_get_head_pose(out: *mut HeadPose) -> bool;
    fn cvr_cpp_xr_push_render_head_pose(pose: *const HeadPose);
    fn cvr_xr_store_render_eye_pose(eye: i32, pose: *const HeadPose, seq: u32);
    fn cvr_cpp_xr_camera_offset(out: *mut f32);
    fn cvr_xr_get_hand_pose(hand: i32, out: *mut HeadPose) -> bool;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
macro_rules! diag {
    ($($e:tt)*) => {
        if diagnostics() {
            // SAFETY: the plugin's exported counters, as the C++ CVR_DIAGNOSTIC writes them.
            #[allow(unused_unsafe)]
            unsafe { $($e)*; }
        }
    };
}
fn verbose() -> bool {
    // SAFETY: a C++ service.
    unsafe { cvr_cpp_verbose_log() != 0 }
}
fn plausible_q(q: &[f32; 4]) -> bool {
    // SAFETY: four floats.
    unsafe { cvr_camera_plausible_unit_quaternion(q.as_ptr()) }
}
fn mul_quat(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    // VrCore.cpp MulQuat
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn normalize_quat(q: &mut [f32; 4]) {
    // VrCore.cpp NormalizeQuat
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len_sq <= 0.000_001 {
        *q = [0.0, 0.0, 0.0, 1.0];
        return;
    }
    let inv = 1.0 / sqrt(len_sq);
    for v in q.iter_mut() {
        *v *= inv;
    }
}
fn tick_ms() -> u64 {
    // SAFETY: a function pointer the tests set, or GetTickCount64.
    unsafe { super::gestures::now_ms() }
}
unsafe fn consumed_at(origin: u64) -> Vec2 {
    // SAFETY: the room-scale state.
    let c = unsafe { cvr_roomscale_camera_consumed_at(origin) };
    Vec2 { x: c.x, y: c.y }
}

// ---- ADS: the weapon layer magnified like the world ----
struct AdsZoom {
    saved_cam: [usize; 2],
    saved_weight: [f32; 2],
    saved_value: [f32; 2],
    saved: [bool; 2],
    prev_aiming: bool,
    told_zoom_pair: bool,
}
static ADS_ZOOM: Mutex<AdsZoom> =
    Mutex::new(AdsZoom { saved_cam: [0; 2], saved_weight: [0.0; 2], saved_value: [1.0; 2], saved: [false; 2], prev_aiming: false, told_zoom_pair: false });

unsafe fn sync_ads_weapon_zoom_one(z: &mut AdsZoom, i: usize, cam: usize, aiming: bool) {
    if cam == 0 {
        return;
    }
    let (weight_addr, value_addr) = (cam + 0x280, cam + 0x284);
    // SAFETY (whole body): the engine's camera through SEH reads and writes, the plugin's exports.
    unsafe {
        if aiming {
            // Capture the engine's own pair once per aim-in, to put back exactly what was there on the way out.
            if !z.prev_aiming || !z.saved[i] || z.saved_cam[i] != cam {
                let w = read::<f32>(weight_addr);
                let v = if w.is_some() { read::<f32>(value_addr) } else { None };
                match (w, v) {
                    (Some(w), Some(v)) if w.is_finite() && v.is_finite() && w >= -0.01 && w <= 1.01 && v >= 0.0 && v < 64.0 => {
                        z.saved_cam[i] = cam;
                        z.saved_weight[i] = w;
                        z.saved_value[i] = v;
                        z.saved[i] = true;
                    }
                    _ => {
                        diag!(CyberpunkVR_DebugAdsZoomRejects += 1);
                        if !z.told_zoom_pair {
                            z.told_zoom_pair = true;
                            if verbose() {
                                super::env::log(&format!(
                                    "[ads] the weapon-zoom pair at camObj+0x280/+0x284 does not look like (weight, zoom) -- read {:.4} / {:.4}. Not writing; set CyberpunkVR_AdsWeaponZoomSync=0 to silence, or re-verify the offsets against this build.\n",
                                    w.unwrap_or(0.0),
                                    v.unwrap_or(0.0)
                                ));
                            }
                        }
                        return;
                    }
                }
            }
            let world_zoom = CyberpunkVR_MainAdsZoomFactor;
            if z.saved[i] && world_zoom.is_finite() && world_zoom > 0.5 && world_zoom < 12.0 && write(weight_addr, 1.0f32) && write(value_addr, world_zoom) {
                diag!(CyberpunkVR_DebugAdsZoomWrites += 1);
            }
        } else if z.prev_aiming && z.saved[i] && z.saved_cam[i] == cam {
            write(weight_addr, z.saved_weight[i]);
            write(value_addr, z.saved_value[i]);
            z.saved[i] = false;
        }
    }
}
unsafe fn sync_ads_weapon_zoom_to_world() {
    let mut z = ADS_ZOOM.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: the plugin's flags and cameras.
    unsafe {
        // The sight zoom magnifies the world the same way aiming does.
        let aiming = g_isAiming || CyberpunkVR_SightZoom != 0;
        if CyberpunkVR_AdsWeaponZoomSync == 0 {
            z.prev_aiming = aiming;
            return;
        }
        let cams = [g_camObjMain.load(Ordering::Acquire), g_camObjVrcam.load(Ordering::Acquire)];
        for (i, cam) in cams.into_iter().enumerate() {
            sync_ads_weapon_zoom_one(&mut z, i, cam, aiming);
        }
        z.prev_aiming = aiming;
    }
}

// ---- the braindance / takeover push: the second eye's transform through the engine's own notify ----
fn in_game_image(at: usize) -> bool {
    static IMAGE: Mutex<(usize, usize)> = Mutex::new((0, 0));
    let mut image = IMAGE.lock().unwrap_or_else(|p| p.into_inner());
    if image.1 == 0 {
        let base = image_base();
        let size = read::<u32>(base + 0x3C).and_then(|lfanew| read::<u32>(base + lfanew as usize + 0x50)).unwrap_or(0);
        *image = (base, base + size as usize);
    }
    at >= image.0 && at < image.1
}
/// The component `comp` gets the composed rotation and the scene's (or `base`'s) position with its half IPD, then the
/// engine's own "the transform changed" call. Nothing is written into an object whose vtable and notify slot are not
/// in the game image (a latch can outlive its component).
#[no_mangle]
pub unsafe extern "C" fn cvr_locate_bd_push_transform_once(
    comp: usize,
    qx: f32,
    qy: f32,
    qz: f32,
    qw: f32,
    second_eye: bool,
    base: *const i32,
    pose: *const HeadPose,
    pose_id: u64,
) {
    if comp <= 0x10000 {
        return;
    }
    // SAFETY (whole body): the engine's component through SEH reads and writes; the caller's base and pose.
    unsafe {
        let Some(vt) = read::<usize>(comp) else { return };
        if !in_game_image(vt) {
            return;
        }
        let Some(notify) = read::<usize>(vt + 0x240) else { return };
        if !in_game_image(notify) {
            return;
        }
        let hq = [qx, qy, qz, qw];
        if !plausible_q(&hq) {
            return;
        }
        let mut r = [0.0f32; 3];
        cvr_camera_right_vector_from_quaternion(hq.as_ptr(), r.as_mut_ptr());
        let have_right = cvr_camera_plausible_unit_vector3(r.as_ptr());
        let half = if CyberpunkVR_IpdInWorldPos != 0 { cvr_camera_desired_half_ipd() } else { 0.0 };
        let sgn0 = if second_eye { 1.0f32 } else { -1.0 };
        let sign = if CyberpunkVR_MainIsRightEye != 0 { -sgn0 } else { sgn0 };
        cvr_pose_invalidate(comp + 0xE0);
        for i in 0..3 {
            let mut v = if CyberpunkVR_BdPushBase != 0 && !base.is_null() { *base.add(i) } else { g_bdScenePosFP[i].load(Ordering::Relaxed) };
            if have_right && half != 0.0 {
                v = v.wrapping_add((r[i] * half * sign * 131072.0) as i32);
            }
            if !write(comp + 0xE0 + i * 4, v) {
                return;
            }
        }
        for (i, v) in hq.iter().enumerate() {
            if !write(comp + 0xF0 + i * 4, *v) {
                return;
            }
        }
        // Publish before notify: native RTT construction may read the component inside that call.
        if !pose.is_null() && pose_id != 0 {
            cvr_pose_publish_setup(comp + 0xE0, comp, if second_eye { 2 } else { 1 }, pose_id, pose, false);
        }
        core::mem::transmute::<usize, unsafe extern "C" fn(usize, usize)>(notify)(comp, comp + 0x100);
        if second_eye {
            diag!(CyberpunkVR_DebugBdPushTransform += 1);
        } else {
            diag!(CyberpunkVR_DebugBdPushMain += 1);
        }
    }
}

// ---- the scene camera comfort: one filter for the scene camera MAIN is composed onto ----
struct SceneComfort {
    comfort: SceneCameraComfort,
    qpc: i64,
    raw: [f32; 4],
    corr: [f32; 4],
    valid: bool,
}
static SCENE_COMFORT: Mutex<SceneComfort> =
    Mutex::new(SceneComfort { comfort: SceneCameraComfort::new(), qpc: 0, raw: [0.0, 0.0, 0.0, 1.0], corr: [0.0, 0.0, 0.0, 1.0], valid: false });
fn scene_comfort_dt(s: &mut SceneComfort) -> f32 {
    let (now, freq) = super::env::qpc();
    let mut dt = 0.0f32;
    if s.qpc != 0 && freq > 0 {
        dt = ((now - s.qpc) as f64 / freq as f64) as f32;
    }
    s.qpc = now;
    dt
}
fn scene_comfort_stop() {
    let mut s = SCENE_COMFORT.lock().unwrap_or_else(|p| p.into_inner());
    if !s.comfort.valid() && !s.valid {
        return;
    }
    s.comfort.reset();
    s.qpc = 0;
    s.valid = false;
    // SAFETY: an exported float.
    unsafe { CyberpunkVR_DebugSceneComfortLagDeg = 0.0 };
}
/// raw: the scene camera orientation chosen as MAIN's base this frame; position: that camera's, metres.
unsafe fn scene_comfort_filter(q: &mut [f32; 4], position: &[f32; 3]) {
    // SAFETY: C++ services and exports.
    unsafe {
        let dead_deg = super::live_reads::scene_cam_comfort_deg();
        // Never in a vehicle: a scene there turns with the car.
        if !(dead_deg > 0.0) || g_isInVehicle {
            scene_comfort_stop();
            return;
        }
        let settings = SceneComfortSettings { dead_deg, ..SceneComfortSettings::default() };
        let mut s = SCENE_COMFORT.lock().unwrap_or_else(|p| p.into_inner());
        let raw = *q;
        let dt = scene_comfort_dt(&mut s);
        let held = s.comfort.step(&raw, Some(position), dt, &settings);
        let mut r = raw;
        normalize_quat(&mut r);
        s.raw = r;
        let mut corr = mul_quat(held, [-r[0], -r[1], -r[2], r[3]]);
        normalize_quat(&mut corr);
        s.corr = corr;
        s.valid = true;
        *q = held;
        CyberpunkVR_DebugSceneComfortLagDeg = s.comfort.lag_deg();
        CyberpunkVR_DebugSceneComfortCuts = s.comfort.cuts();
    }
}
/// The correction LocateCamera applied, for the other sites composing from the same scene camera (PatchCamera).
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_comfort_apply(raw: *const f32, out: *mut f32) -> bool {
    if raw.is_null() || out.is_null() {
        return false;
    }
    // SAFETY: the caller's four floats each way.
    unsafe {
        let r = [*raw, *raw.add(1), *raw.add(2), *raw.add(3)];
        for (i, v) in r.iter().enumerate() {
            *out.add(i) = *v;
        }
        let s = SCENE_COMFORT.lock().unwrap_or_else(|p| p.into_inner());
        if !s.valid || !(super::live_reads::scene_cam_comfort_deg() > 0.0) {
            return false;
        }
        // A raw value a cut away from the one LocateCamera last filtered is the next shot: taken as it is.
        if angle_deg(&r, &s.raw) > SceneComfortSettings::default().cut_deg {
            return false;
        }
        let mut o = mul_quat(s.corr, r);
        normalize_quat(&mut o);
        for (i, v) in o.iter().enumerate() {
            *out.add(i) = *v;
        }
        true
    }
}

// ---- the ADS camera-pivot telemetry and the sprint jerk census (diagnostics) ----
struct AdsTelemetry {
    state: AdsCameraTelemetryUiState,
    last_tick: f32,
    baseline: [f32; 3],
    peak: [f32; 3],
    baseline_valid: bool,
    prev_aiming: bool,
    samples: u32,
}
static ADS_TELEMETRY: Mutex<AdsTelemetry> = Mutex::new(AdsTelemetry {
    state: AdsCameraTelemetryUiState { available: 0, aiming: 0, baseline_valid: 0, samples: 0, residual: [0.0; 3], delta: [0.0; 3], peak: [0.0; 3] },
    last_tick: -1.0,
    baseline: [0.0; 3],
    peak: [0.0; 3],
    baseline_valid: false,
    prev_aiming: false,
    samples: 0,
});
#[no_mangle]
pub unsafe extern "C" fn GetAdsCameraTelemetryUiState(out: *mut AdsCameraTelemetryUiState) {
    if !out.is_null() {
        // SAFETY: the caller's state.
        unsafe { *out = ADS_TELEMETRY.lock().unwrap_or_else(|p| p.into_inner()).state };
    }
}
struct Jerk {
    last_tick: f32,
    prev_dev: [f32; 3],
    prev_valid: bool,
    prev_sp2: f32,
    window: i32,
    last_arm_ms: u64,
}
static JERK: Mutex<Jerk> = Mutex::new(Jerk { last_tick: -1.0, prev_dev: [0.0; 3], prev_valid: false, prev_sp2: 0.0, window: 0, last_arm_ms: 0 });
static STACK_REJECTS: Mutex<(u32, u64)> = Mutex::new((0, 0));
static VIEW_SEQ: AtomicU32 = AtomicU32::new(0);

/// The current thread's stack, from its TEB (NT_TIB StackBase +0x08, StackLimit +0x10).
fn thread_stack() -> (usize, usize) {
    let (base, limit): (usize, usize);
    // SAFETY: plain reads of the current thread's TEB.
    unsafe {
        core::arch::asm!("mov {}, gs:[0x08]", out(reg) base, options(nostack, readonly, preserves_flags));
        core::arch::asm!("mov {}, gs:[0x10]", out(reg) limit, options(nostack, readonly, preserves_flags));
    }
    (limit, base)
}

/// rcx = rbx at the site (the located camera: int32 fixed-point position, then the quaternion), xmm1 = xmm0.
#[no_mangle]
pub unsafe extern "C" fn OnLocateCameraCallback(rbx: *mut f32, xmm0: f32) {
    // SAFETY: the engine's located camera, the plugin's globals and C++ services.
    unsafe { locate(rbx, xmm0) }
}

unsafe fn locate(rbx: *mut f32, xmm0: f32) {
    // SAFETY (whole body): as OnLocateCameraCallback.
    unsafe {
        g_locateCameraHits = g_locateCameraHits.wrapping_add(1);
        cvr_cpp_telemetry_locate(g_locateCameraHits as u32, rbx as u64, xmm0);
        let at = rbx as usize;
        if rbx.is_null() || at < 0x10000 {
            return;
        }
        let pos_fp = rbx as *mut i32;
        let quat = rbx.add(4); // +16 bytes
        if read::<f32>(quat as usize).is_none() {
            return;
        }
        // THE ENGINE'S OWN ROTATION FOR THIS CAMERA, taken before a single field is written.
        let bd_entry_quat = [*quat, *quat.add(1), *quat.add(2), *quat.add(3)];
        let entry_pos = [*pos_fp as f32 / 131072.0, *pos_fp.add(1) as f32 / 131072.0, *pos_fp.add(2) as f32 / 131072.0];
        g_dbgEntryYaw = atan2(
            2.0 * (bd_entry_quat[3] * bd_entry_quat[2] + bd_entry_quat[0] * bd_entry_quat[1]),
            1.0 - 2.0 * (bd_entry_quat[1] * bd_entry_quat[1] + bd_entry_quat[2] * bd_entry_quat[2]),
        );
        g_dbgEntryPosX = entry_pos[0];
        g_dbgEntryPosY = entry_pos[1];
        g_dbgEntryPosZ = entry_pos[2];
        // The real gameplay camera is heap-backed: transient transforms built on this thread's stack are rejected.
        let (lo, hi) = thread_stack();
        if at >= lo && at < hi {
            let mut rej = STACK_REJECTS.lock().unwrap_or_else(|p| p.into_inner());
            rej.0 += 1;
            let now = tick_ms();
            if rej.1 == 0 {
                rej.1 = now;
            }
            if now - rej.1 >= 1000 {
                if verbose() {
                    super::env::log(&format!("[STACKCAM] rejected {}/s stack-temp camera locates (the foreign second bake)\n", rej.0));
                }
                *rej = (0, now);
            }
            return;
        }
        sync_ads_weapon_zoom_to_world();
        cvr_cpp_refresh_player_camera_state();

        // SPRINT-START JERK DETECTOR (diagnostics): the located camera against the tick entity and the clean pair.
        let sh = cvr_cpp_shot_shared();
        if diagnostics() && !sh.is_null() && !g_isInVehicle && *sh.add(131) != 0.0 {
            let mut j = JERK.lock().unwrap_or_else(|p| p.into_inner());
            let tick_now = *sh.add(99);
            if tick_now != j.last_tick {
                j.last_tick = tick_now;
                let dev = [g_dbgEntryPosX - (*sh.add(96) + *sh.add(128)), g_dbgEntryPosY - (*sh.add(97) + *sh.add(129)), g_dbgEntryPosZ - (*sh.add(98) + *sh.add(130))];
                let sp2 = *sh.add(132) * *sh.add(132) + *sh.add(133) * *sh.add(133);
                let now_ms = tick_ms();
                let mut why: Option<&str> = None;
                if j.prev_sp2 < 16.0 && sp2 >= 16.0 {
                    why = Some("SPRINT-ENGAGE");
                } else if j.prev_sp2 > 12.25 && sp2 <= 12.25 {
                    why = Some("SPRINT-STOP");
                } else if j.prev_valid {
                    let dz = dev[2] - j.prev_dev[2];
                    if (dz > 0.008 || dz < -0.008) && now_ms - j.last_arm_ms > 1000 {
                        why = Some("Z-KICK");
                    }
                }
                j.prev_sp2 = sp2;
                if let (Some(w), 0) = (why, j.window) {
                    j.window = 45;
                    j.last_arm_ms = now_ms;
                    if verbose() {
                        super::env::log(&format!("[jerk] ARM({w}) speed={:.2} dev=({:.4},{:.4},{:.4})\n", sqrt(sp2), dev[0], dev[1], dev[2]));
                    }
                }
                j.prev_dev = dev;
                j.prev_valid = true;
                if j.window > 0 {
                    j.window -= 1;
                    if verbose() {
                        super::env::log(&format!(
                            "[jerk] ms={now_ms} tick={tick_now:.0} dev=({:.4},{:.4},{:.4}) v=({:.2},{:.2})\n",
                            dev[0],
                            dev[1],
                            dev[2],
                            *sh.add(132),
                            *sh.add(133)
                        ));
                    }
                }
            }
        }

        let mut camera_q = bd_entry_quat;
        let mut skip_hmd_orientation = false;
        // Menu stability: a full-screen menu is not driven by the head (the lobby's world is looked around in).
        let mut menu_open = cvr_cpp_menu_mode() != 0 && !cvr_cpp_lobby_world_shown();
        if !menu_open && !sh.is_null() && core::ptr::read_volatile((sh as *const u32).add(81)) != 0 {
            menu_open = true;
        }
        if menu_open {
            skip_hmd_orientation = true;
        }
        // The heading base must be a value WE NEVER WROTE.
        let base_q = if CyberpunkVR_CamWriteInPatch != 0 && core::ptr::read_volatile(&raw const g_engineCamQuatValid) != 0 {
            core::ptr::read_volatile(&raw const g_engineCamQuat)
        } else {
            camera_q
        };
        // X = right, Y = forward, Z = up: the quaternion's "up" column is the game's forward.
        let mut body_fwd_x = 2.0 * (base_q[0] * base_q[1] - base_q[2] * base_q[3]);
        let mut body_fwd_y = 1.0 - 2.0 * (base_q[0] * base_q[0] + base_q[2] * base_q[2]);
        let mut tracking_yaw = 0.0f32;
        if cvr_body_read_tracking_yaw(&mut tracking_yaw) {
            body_fwd_x = -sin(tracking_yaw);
            body_fwd_y = cos(tracking_yaw);
        }
        // THE FRAME'S ONE HEAD SAMPLE -- the same struct PatchCamera composes the orientation from.
        let mut xr_pose = HeadPose::default();
        let mut xr_consumed = Vec2::default();
        let render_eye = cvr_cpp_xr_render_eye_index();
        let mut camera_pose_sequence = 0u64;
        let has_xr = if CyberpunkVR_OneSamplePerFrame != 0 {
            cvr_xr_acquire_camera_pose_frame(&mut xr_pose, &mut xr_consumed, &mut camera_pose_sequence)
        } else {
            cvr_xr_get_head_pose(&mut xr_pose)
        };
        let compose_at_write = CyberpunkVR_CamWriteInPatch != 0 && CyberpunkVR_CamComposeAtWrite != 0;
        let mut takeover_pose = HeadPose::default();
        let mut takeover_pose_id = 0u64;
        let mut takeover_label_known = false;
        let mut scene_pose = HeadPose::default();
        let mut scene_pose_id = 0u64;
        let mut scene_label_known = false;
        if has_xr {
            // Hand the EXACT sample to the submit path, only when this site composes what gets written.
            if !compose_at_write || cvr_bd_push_owns_composition() {
                cvr_cpp_xr_push_render_head_pose(&xr_pose);
            }
            let current_seq = g_lastLocateSeq.load(Ordering::Relaxed).wrapping_add(1);
            if !sh.is_null() {
                *sh.add(94) = render_eye as f32;
                *sh.add(95) = cvr_camera_desired_half_ipd();
            }
            cvr_xr_store_render_eye_pose(0, &xr_pose, current_seq);
            cvr_xr_store_render_eye_pose(1, &xr_pose, current_seq);
            let game_yaw = atan2(-body_fwd_x, body_fwd_y);
            let mut controls = InputControls::default();
            super::live_reads::input_controls(&mut controls);
            let game_pitch = if controls.disable_mouse_y != 0 { 0.0 } else { core::ptr::read_volatile(&raw const g_gamePitchRadians) };
            let cy = cos(game_yaw * 0.5);
            let sy = sin(game_yaw * 0.5);
            let pc = cos(game_pitch * 0.5);
            let ps = sin(game_pitch * 0.5);
            // ---- ADS CAMERA-PIVOT TELEMETRY (dabinn, TofuExpress f8a827eb) ----
            if !sh.is_null() && *sh.add(131) != 0.0 {
                let mut t = ADS_TELEMETRY.lock().unwrap_or_else(|p| p.into_inner());
                let tick = *sh.add(99);
                if tick != t.last_tick {
                    t.last_tick = tick;
                    let dx = g_dbgEntryPosX - (*sh.add(96) + *sh.add(128));
                    let dy = g_dbgEntryPosY - (*sh.add(97) + *sh.add(129));
                    let dz = g_dbgEntryPosZ - (*sh.add(98) + *sh.add(130));
                    let (cyaw, syaw) = (cos(game_yaw), sin(game_yaw));
                    let local = [cyaw * dx + syaw * dy, -syaw * dx + cyaw * dy, dz];
                    if !g_isAiming {
                        t.baseline = local;
                        t.baseline_valid = true;
                        t.peak = [0.0; 3];
                        t.samples = 0;
                    } else {
                        if !t.prev_aiming {
                            t.peak = [0.0; 3];
                            t.samples = 0;
                        }
                        t.samples += 1;
                    }
                    let delta = if t.baseline_valid { [local[0] - t.baseline[0], local[1] - t.baseline[1], local[2] - t.baseline[2]] } else { [0.0; 3] };
                    if g_isAiming {
                        for i in 0..3 {
                            let a = delta[i].abs();
                            if a > t.peak[i] {
                                t.peak[i] = a;
                            }
                        }
                    }
                    let (bv, samples, peak) = (t.baseline_valid, t.samples, t.peak);
                    t.state = AdsCameraTelemetryUiState {
                        available: 1,
                        aiming: g_isAiming as i32,
                        baseline_valid: bv as i32,
                        samples,
                        residual: local,
                        delta,
                        peak,
                    };
                    t.prev_aiming = g_isAiming;
                }
            }
            core::ptr::write_volatile(&raw mut g_headingSy, sy);
            core::ptr::write_volatile(&raw mut g_headingCy, cy);
            core::ptr::write_volatile(&raw mut g_headingPitchS, ps);
            core::ptr::write_volatile(&raw mut g_headingPitchC, pc);
            core::ptr::write_volatile(&raw mut g_headingValid, if skip_hmd_orientation { 0 } else { 1 });
            CyberpunkVR_DebugTidLocateCam = GetCurrentThreadId();
            let xr_game = [xr_pose.ori[0], -xr_pose.ori[2], xr_pose.ori[1], xr_pose.ori[3]];
            // Camera = tracking reference * full HMD orientation; yaw * pitch * HMD.
            let mut head = mul_quat([0.0, 0.0, sy, cy], [ps, 0.0, 0.0, pc]);
            // IN A BRAINDANCE THE BASE IS THE SCENE'S OWN CAMERA, NOT THE HEADING.
            if g_bdActive.load(Ordering::Relaxed) != 0 && g_bdScenePoseValid.load(Ordering::Acquire) != 0 {
                let mut from_buffer = false;
                if CyberpunkVR_BdQuatFromBuffer != 0 {
                    let q = [*quat, *quat.add(1), *quat.add(2), *quat.add(3)];
                    let l2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
                    let mut ours = false;
                    if CyberpunkVR_BdQuatFromBuffer >= 2 && core::ptr::read_volatile(&raw const g_headQuatValid) != 0 {
                        let c = core::ptr::read_volatile(&raw const g_headQuatComposed);
                        let mut d = q[0] * c[0] + q[1] * c[1] + q[2] * c[2] + q[3] * c[3];
                        if d < 0.0 {
                            d = -d;
                        }
                        ours = d > 0.99999;
                    }
                    if l2 > 0.9 && l2 < 1.1 && !ours {
                        head = q;
                        from_buffer = true;
                        diag!(CyberpunkVR_DebugBdQuatFromBuffer += 1);
                    }
                }
                if !from_buffer {
                    let e = bd_entry_quat;
                    let l2 = e[0] * e[0] + e[1] * e[1] + e[2] * e[2] + e[3] * e[3];
                    if CyberpunkVR_BdBaseFromLocate != 0 && l2 > 0.9 && l2 < 1.1 {
                        head = e;
                        diag!(CyberpunkVR_DebugBdBaseFromLocate += 1);
                    } else {
                        head = g_bdSceneQuat;
                        diag!(CyberpunkVR_DebugBdBaseFromScript += 1);
                    }
                }
                // THE SCENE CAMERA'S OWN TURNING, HELD BACK (xr_scene_cam_comfort_deg).
                scene_comfort_filter(&mut head, &entry_pos);
            } else {
                scene_comfort_stop();
            }
            let mut tmp = mul_quat(head, xr_game);
            normalize_quat(&mut tmp);
            camera_q = tmp;
            let shared_scene = CyberpunkVR_CamWriteInPatch != 0
                && CyberpunkVR_CamComposeAtWrite != 0
                && !g_isInVehicle
                && use_native_main_heading(false, g_sceneTier.load(Ordering::Relaxed), cvr_cpp_device_cam_active(), g_bdActive.load(Ordering::Relaxed) != 0);
            let mut matched_scene_write = false;
            if shared_scene && camera_pose_sequence != 0 {
                let mut written = [0.0f32; 4];
                let mut written_head = HeadPose::default();
                let mut pose_id = 0u64;
                if cvr_camera_cam_write_quat_read(&mut written, &mut written_head, core::ptr::null_mut(), core::ptr::null_mut(), &mut pose_id)
                    && pose_id == camera_pose_sequence
                    && written_head.origin_serial == xr_pose.origin_serial
                {
                    if let Some(yaw) = composition_base_yaw(&written, &written_head.ori) {
                        camera_q = written;
                        body_fwd_x = -sin(yaw);
                        body_fwd_y = cos(yaw);
                        matched_scene_write = true;
                    }
                }
            }
            // Publish the composed orientation for the PatchCamera writer (the scene's writer owns it otherwise).
            if !shared_scene || matched_scene_write {
                g_headQuatComposed = camera_q;
                core::ptr::write_volatile(&raw mut g_headQuatValid, if skip_hmd_orientation { 0 } else { 1 });
            } else if skip_hmd_orientation {
                core::ptr::write_volatile(&raw mut g_headQuatValid, 0);
            }
            g_bdPushQuat = camera_q;
            // AND THE RING ENTRY GOES WITH IT, when this site composes.
            if cvr_bd_push_owns_composition() {
                cvr_camera_cam_write_quat_publish(camera_q[0], camera_q[1], camera_q[2], camera_q[3], &xr_pose, core::ptr::null(), 0);
                cvr_camera_cam_write_record_push(&camera_q, &xr_pose);
            }
            let bd_now = g_bdActive.load(Ordering::Relaxed) != 0 && g_bdScenePoseValid.load(Ordering::Acquire) != 0;
            let takeover_now = cvr_locate_owns_takeover();
            // AND WHAT GOES IN IS WHAT THE WRITE SITE COMPOSED: one sample, one label, both eyes.
            let mut bd_written = [0.0f32; 4];
            let bd_use_write_site = (if bd_now { CyberpunkVR_BdQuatFromWriteSite != 0 } else { takeover_now })
                && cvr_camera_cam_write_quat_read(&mut bd_written, &mut takeover_pose, core::ptr::null_mut(), core::ptr::null_mut(), &mut takeover_pose_id)
                && (bd_written[0] * bd_written[0] + bd_written[1] * bd_written[1] + bd_written[2] * bd_written[2] + bd_written[3] * bd_written[3]) > 0.9;
            if bd_use_write_site {
                camera_q = bd_written;
                g_headQuatComposed = camera_q;
                diag!(CyberpunkVR_DebugBdQuatFromWriteSite += 1);
            }
            if !skip_hmd_orientation && (CyberpunkVR_CamWriteInPatch == 0 || bd_now || takeover_now) {
                for (i, v) in camera_q.iter().enumerate() {
                    *quat.add(i) = *v;
                }
                takeover_label_known = takeover_now
                    && bd_use_write_site
                    && takeover_pose.valid
                    && takeover_pose_id != 0
                    && takeover_pose_id == camera_pose_sequence
                    && takeover_pose.origin_serial == xr_pose.origin_serial
                    && cvr_pose_transport_serialized_component() == g_lensComp.load(Ordering::Acquire);
                if bd_now && !takeover_now {
                    if bd_use_write_site {
                        scene_pose = takeover_pose;
                        scene_pose_id = takeover_pose_id;
                        scene_label_known = takeover_pose.valid
                            && takeover_pose_id != 0
                            && takeover_pose_id == camera_pose_sequence
                            && takeover_pose.origin_serial == xr_pose.origin_serial;
                    } else {
                        scene_pose = xr_pose;
                        scene_pose_id = camera_pose_sequence;
                        scene_label_known = CyberpunkVR_OneSamplePerFrame != 0 && camera_pose_sequence != 0 && xr_pose.valid;
                    }
                }
            }
        }

        // In a menu, also skip the HMD POSITION injection: the map/menu must be a flat, static 2D panel.
        if has_xr && !menu_open {
            let pos_scale = 1.0 * cvr_camera_world_scale();
            let mut cam_bake = [0.0f32; 3];
            if !g_isInVehicle {
                cvr_cpp_xr_camera_offset(cam_bake.as_mut_ptr());
            }
            let mut offsets = [0.0f32; 6]; // head x y z, vehicle head x y z
            super::live_reads::camera_offsets(offsets.as_mut_ptr());
            let mut veh_off = [0.0f32; 3];
            if g_isInVehicle {
                let active = active_vehicle_camera_offset(
                    av(offsets[3], offsets[4], offsets[5]),
                    is_passenger_window_combat(cvr_cpp_pose_vehicle_state()),
                    super::wheel_grab::cvr_anim_wheel_mounted_bike(),
                );
                veh_off = [active.x, active.y, active.z];
            }
            let mut eye_bake = [0.0f32; 3];
            if !sh.is_null() {
                if !g_isInVehicle && *sh.add(119) == 1.0 {
                    eye_bake = [*sh.add(116), *sh.add(117), *sh.add(118)];
                }
                for i in 0..3 {
                    *sh.add(120 + i) = offsets[i] + cam_bake[i] + eye_bake[i] + veh_off[i];
                }
                *sh.add(123) = 1.0;
            }
            let mut local_right = xr_pose.pos[0] * pos_scale + (offsets[0] + cam_bake[0] + eye_bake[0] + veh_off[0]);
            let mut local_forward = -xr_pose.pos[2] * pos_scale + (offsets[1] + cam_bake[1] + eye_bake[1] + veh_off[1]);
            let local_up = xr_pose.pos[1] * pos_scale + (offsets[2] + cam_bake[2] + eye_bake[2] + veh_off[2]);
            // THE HEAD IS ROTATED BY THE VIEW'S HEADING, NOT THE BODY'S.
            let mut flat_yaw = atan2(-body_fwd_x, body_fwd_y);
            if CyberpunkVR_ViewYawFromEngine != 0 && CyberpunkVR_EngineBodyYawValid != 0 && !g_isInVehicle {
                let (mut wz, mut ww) = (CyberpunkVR_EngineBodyYawZ, CyberpunkVR_EngineBodyYawW);
                if ww < 0.0 {
                    wz = -wz;
                    ww = -ww;
                }
                if wz != 0.0 || ww != 0.0 {
                    flat_yaw = cvr_body_tracking_yaw(2.0 * atan2(wz, ww));
                }
            }
            // MOUNTED, TAKE THE YAW THE VIEW WAS ACTUALLY COMPOSED WITH.
            if g_isInVehicle && CyberpunkVR_VehicleAnchorFromViewYaw != 0 && core::ptr::read_volatile(&raw const g_viewYawUsedValid) != 0 {
                flat_yaw = core::ptr::read_volatile(&raw const g_viewYawUsedRad);
            }
            let mut anchor = make_anchor_recipe(
                av(offsets[0], offsets[1], offsets[2]),
                av(veh_off[0], veh_off[1], veh_off[2]),
                av(cam_bake[0] + eye_bake[0], cam_bake[1] + eye_bake[1], cam_bake[2] + eye_bake[2]),
                g_isInVehicle,
                pos_scale,
                flat_yaw,
                cvr_camera_body_anchor_yaw(flat_yaw),
            );
            anchor.body_orbit = cvr_roomscale_physical_body_heading_owned();
            anchor.centre_yaw = cvr_body_centre_yaw(anchor.model_yaw);
            cvr_camera_anchor_recipe_publish(&anchor);
            let consumed = if CyberpunkVR_OneSamplePerFrame != 0 { xr_consumed } else { consumed_at(xr_pose.origin_serial) };
            local_right -= consumed.x * pos_scale;
            local_forward -= consumed.y * pos_scale;
            let p = xr_pose.pos;
            let delta = compose_view_translation(
                av((p[0] - consumed.x) * pos_scale, (-p[2] - consumed.y) * pos_scale, p[1] * pos_scale),
                &anchor,
                anchor.tracking_yaw,
                anchor.model_yaw,
            );
            let delta_fp = [(delta.x * 131072.0) as i32, (delta.y * 131072.0) as i32, (delta.z * 131072.0) as i32];
            // THE DELTA IS NO LONGER ADDED HERE (CyberpunkVR_HeadTranslationInPatch).
            if CyberpunkVR_HeadTranslationInPatch == 0 {
                for i in 0..3 {
                    *pos_fp.add(i) = (*pos_fp.add(i)).wrapping_add(delta_fp[i]);
                }
            }
            for i in 0..3 {
                g_headDeltaFP[i].store(delta_fp[i], Ordering::Relaxed);
            }
            g_headDeltaValid.store(1, Ordering::Release);
            // [104..111] RENDER-VIEW POSE v2 + [141..142] heading, seqlocked by [143].
            if !sh.is_null() {
                let seq = (sh as *mut u32).add(143);
                core::ptr::write_volatile(seq, VIEW_SEQ.fetch_add(1, Ordering::Relaxed) + 1); // odd: write in progress
                for i in 0..4 {
                    *sh.add(104 + i) = camera_q[i];
                }
                let in_camera = CyberpunkVR_HeadTranslationInPatch != 0;
                *sh.add(108) = if in_camera { 0.0 } else { delta.x };
                *sh.add(109) = if in_camera { 0.0 } else { delta.y };
                *sh.add(110) = if in_camera { 0.0 } else { delta.z };
                *sh.add(111) = 2.0;
                *sh.add(141) = atan2(-body_fwd_x, body_fwd_y);
                *sh.add(142) = 1.0;
                for i in 0..4 {
                    *sh.add(227 + i) = xr_pose.ori[i];
                }
                let (c, f) = super::env::qpc();
                let ms = if f > 0 { c as f64 * 1000.0 / f as f64 } else { 0.0 };
                *sh.add(68) = (ms % 100000.0) as f32;
                core::ptr::write_volatile(seq, VIEW_SEQ.fetch_add(1, Ordering::Relaxed) + 1); // even: packet complete
                // THE SAME FRAME, HANDED TO THE SOLVE WITHOUT LEAVING THE PROCESS.
                let vf = ViewFrame {
                    view_quat: camera_q,
                    world_delta: [*sh.add(108), *sh.add(109), *sh.add(110)],
                    delta_semantics: *sh.add(111),
                    heading_rad: *sh.add(141),
                    head_ori: xr_pose.ori,
                    stamp_ms: *sh.add(68),
                };
                cvr_camera_view_frame_publish(&vf);
            }
            if verbose() && g_locateCameraHits % 600 == 1 {
                super::env::log(&format!(
                    "LocateCamera translation: allow=1 posScale={pos_scale:.4} local=({local_right:.4}, {local_forward:.4}, {local_up:.4})\n"
                ));
            }
        }

        // The per-eye shift (the late IPD census) and MAIN's braindance half, computed here, applied below.
        let mut ipd_shift_fp = [0i32; 3];
        let mut bd_ipd_shift_fp = [0i32; 3];
        if has_xr {
            let render_eye2 = (g_locateCameraHits % 2) as i32;
            let mut right = [0.0f32; 3];
            cvr_camera_right_vector_from_quaternion(camera_q.as_ptr(), right.as_mut_ptr());
            if cvr_camera_plausible_unit_vector3(right.as_ptr()) {
                let half_ipd = cvr_camera_desired_half_ipd();
                let eye_sign = if render_eye2 == 0 { -1.0f32 } else { 1.0 };
                let ipd_shift = half_ipd * eye_sign;
                for i in 0..3 {
                    ipd_shift_fp[i] = (right[i] * ipd_shift * 131072.0) as i32;
                }
                if CyberpunkVR_BdIpdInLocate != 0
                    && CyberpunkVR_IpdInWorldPos != 0
                    && g_bdActive.load(Ordering::Relaxed) != 0
                    && g_bdScenePoseValid.load(Ordering::Acquire) != 0
                {
                    let sign = if CyberpunkVR_MainIsRightEye != 0 { 1.0f32 } else { -1.0 };
                    for i in 0..3 {
                        bd_ipd_shift_fp[i] = (right[i] * half_ipd * sign * 131072.0) as i32;
                    }
                }
                if verbose() && g_locateCameraHits % 600 == 1 {
                    super::env::log(&format!(
                        "LocateCamera IPD: eye={render_eye2} halfIpd={half_ipd:.4} right=({:.3}, {:.3}, {:.3}) shift={ipd_shift:.4}\n",
                        right[0], right[1], right[2]
                    ));
                }
            }
        }
        // MAIN'S VIEWPOINT, FROM THE SAME PER-FRAME SCENE POSE THE SECOND EYE IS PLACED WITH.
        if CyberpunkVR_BdMainPosFromScene != 0 && g_bdActive.load(Ordering::Relaxed) != 0 && g_bdScenePoseValid.load(Ordering::Acquire) != 0 {
            for i in 0..3 {
                *pos_fp.add(i) = g_bdScenePosFP[i].load(Ordering::Relaxed);
            }
            diag!(CyberpunkVR_DebugBdMainPos += 1);
        }
        // THE VIEWPOINT SLIDER FOR A TAKEN-OVER CAMERA, applied to the buffer itself (yaw only, about the lens).
        if cvr_locate_owns_takeover() {
            let (ox, oy, oz) = (CyberpunkVR_DevCamOffsetX, CyberpunkVR_DevCamOffsetY, CyberpunkVR_DevCamOffsetZ);
            if ox != 0.0 || oy != 0.0 || oz != 0.0 {
                let yaw = if g_devCamAimValid.load(Ordering::Acquire) != 0 { g_devCamAimYaw } else { 0.0 };
                let (sy, cy) = (sin(yaw), cos(yaw));
                *pos_fp = (*pos_fp).wrapping_add(((ox * cy - oy * sy) * 131072.0) as i32);
                *pos_fp.add(1) = (*pos_fp.add(1)).wrapping_add(((ox * sy + oy * cy) * 131072.0) as i32);
                *pos_fp.add(2) = (*pos_fp.add(2)).wrapping_add((oz * 131072.0) as i32);
            }
        }
        let rendered_pos_fp = [*pos_fp, *pos_fp.add(1), *pos_fp.add(2)];
        // NOW the braindance half goes in -- after the head centre has been taken.
        if bd_ipd_shift_fp != [0; 3] {
            for i in 0..3 {
                *pos_fp.add(i) = (*pos_fp.add(i)).wrapping_add(bd_ipd_shift_fp[i]);
            }
            diag!(CyberpunkVR_DebugBdIpdLocate += 1);
        }
        // ...AND ONLY FOR THE CAMERA THE TAKEOVER RENDERS THROUGH: the lens position identifies it, within a metre.
        let mut located_is_lens = false;
        if g_devCamPosValid.load(Ordering::Acquire) != 0 {
            let k = 1.0f32 / 131072.0;
            let mut d2 = 0.0f32;
            for i in 0..3 {
                let d = (rendered_pos_fp[i].wrapping_sub(g_devCamPosFP[i].load(Ordering::Relaxed))) as f32 * k;
                d2 += d * d;
            }
            located_is_lens = d2 <= 1.0;
        }
        if cvr_locate_owns_takeover() && core::ptr::read_volatile(&raw const g_headQuatValid) != 0 && located_is_lens {
            let mut tq = core::ptr::read_volatile(&raw const g_headQuatComposed);
            if takeover_label_known {
                tq = camera_q;
                cvr_pose_publish_setup(at, cvr_pose_transport_serialized_component(), 1, takeover_pose_id, &takeover_pose, false);
            }
            if plausible_q(&tq) {
                cvr_locate_bd_push_transform_once(
                    g_camObjVrcam.load(Ordering::Relaxed),
                    tq[0],
                    tq[1],
                    tq[2],
                    tq[3],
                    true,
                    rendered_pos_fp.as_ptr(),
                    if takeover_label_known { &takeover_pose } else { core::ptr::null() },
                    takeover_pose_id,
                );
                diag!(CyberpunkVR_DebugTakeoverPush += 1);
            }
        }
        // A SCENE CAMERA IS LABELLED LIKE A TAKEOVER, and frame generation depends on it.
        if scene_label_known && scene_pose_id != 0 {
            cvr_pose_publish_setup(at, cvr_pose_transport_serialized_component(), 1, scene_pose_id, &scene_pose, false);
            diag!(CyberpunkVR_DebugSceneLabelPublished += 1);
        }
        if CyberpunkVR_BdPushTransform != 0
            && core::ptr::read_volatile(&raw const g_headQuatValid) != 0
            && g_bdActive.load(Ordering::Relaxed) != 0
            && g_bdScenePoseValid.load(Ordering::Acquire) != 0
        {
            let q = g_bdPushQuat;
            if CyberpunkVR_BdPushTransform & 1 != 0 {
                cvr_locate_bd_push_transform_once(g_camObjVrcam.load(Ordering::Relaxed), q[0], q[1], q[2], q[3], true, rendered_pos_fp.as_ptr(), core::ptr::null(), 0);
            }
            if CyberpunkVR_BdPushTransform & 2 != 0 {
                cvr_locate_bd_push_transform_once(g_camObjMain.load(Ordering::Relaxed), q[0], q[1], q[2], q[3], false, rendered_pos_fp.as_ptr(), core::ptr::null(), 0);
            }
        }
        // The head CENTRE, before the separation: what the second view sits on in mode 3.
        for i in 0..3 {
            g_locateCenterFP[i].store(rendered_pos_fp[i], Ordering::Relaxed);
        }
        for i in 0..3 {
            g_locatePosFP[i].store(*pos_fp.add(i), Ordering::Relaxed);
        }
        g_locatePosValid.store(1, Ordering::Release);
        if !sh.is_null() {
            for i in 0..3 {
                *sh.add(204 + i) = rendered_pos_fp[i] as f32 / 131072.0;
            }
            *sh.add(207) = 1.0;
        }
        g_lastLocatePosFP = rendered_pos_fp;
        g_lastIpdShiftFP = ipd_shift_fp;
        g_lastLocateQuat = [*quat, *quat.add(1), *quat.add(2), *quat.add(3)];
        let seq = g_lastLocateSeq.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        {
            let mut body_centre_fp = rendered_pos_fp;
            let mut body_base_fp = rendered_pos_fp;
            let mut body_centre_known = true;
            // SerializeSetup copies MAIN's EYE when the IPD is written in PatchCamera: only the body reference is centred.
            if (CyberpunkVR_IpdInWorldPos != 0 || CyberpunkVR_HeadTranslationInPatch != 0) && !cvr_cpp_device_cam_active() && g_bdActive.load(Ordering::Relaxed) == 0 {
                body_centre_known = cvr_camera_main_eye_centre_read(&rendered_pos_fp, &mut body_centre_fp, &mut body_base_fp);
            }
            let frame = LocatedCameraFrame {
                world_pos: [body_centre_fp[0] as f32 / 131072.0, body_centre_fp[1] as f32 / 131072.0, body_centre_fp[2] as f32 / 131072.0],
                world_quat: camera_q,
                sequence: seq,
                frame_epoch: g_VrikFrameEpoch.load(Ordering::Relaxed),
                body_centre_known: body_centre_known as u32,
                body_base_world: [body_base_fp[0] as f32 / 131072.0, body_base_fp[1] as f32 / 131072.0, body_base_fp[2] as f32 / 131072.0],
            };
            cvr_camera_located_frame_publish(&frame);
        }
        // The located camera and a controller-aim quaternion for the plugin's ShotSnap.
        if has_xr && !sh.is_null() {
            let mut hand = HeadPose::default();
            if cvr_xr_get_hand_pose(1, &mut hand) && hand.valid {
                let game_yaw2 = atan2(-body_fwd_x, body_fwd_y);
                let (cy2, sy2) = (cos(game_yaw2 * 0.5), sin(game_yaw2 * 0.5));
                let cg = [hand.ori[0], -hand.ori[2], hand.ori[1], hand.ori[3]];
                let mut a = mul_quat([0.0, 0.0, sy2, cy2], cg);
                normalize_quat(&mut a);
                let cam = at as u64;
                *(sh as *mut u32).add(51) = cam as u32;
                *(sh as *mut u32).add(52) = (cam >> 32) as u32;
                for i in 0..4 {
                    *sh.add(53 + i) = a[i];
                }
                *sh.add(60) = 2.0 * (a[0] * a[1] - a[2] * a[3]);
                *sh.add(61) = 1.0 - 2.0 * (a[0] * a[0] + a[2] * a[2]);
                *sh.add(62) = 2.0 * (a[1] * a[2] + a[0] * a[3]);
                // DELTA quat = inv(hmd) * controller in raw XR space, conjugated into game space by P = rotX(+90).
                let dxr = mul_quat([-xr_pose.ori[0], -xr_pose.ori[1], -xr_pose.ori[2], xr_pose.ori[3]], hand.ori);
                let (px, pw) = (0.707_106_78f32, 0.707_106_78f32);
                let t1 = mul_quat([px, 0.0, 0.0, pw], dxr);
                let mut d = mul_quat(t1, [-px, 0.0, 0.0, pw]);
                normalize_quat(&mut d);
                for i in 0..4 {
                    *sh.add(63 + i) = d[i];
                }
                *sh.add(50) = (seq & 0xFF_FFFF) as f32; // valid/heartbeat
            }
        }
    }
}

/// movss [rbx+20h],xmm0 / lea rdx,[rsp+20h] / mov rax,[rsi]
const PATTERN: &[u8] = b"\xF3\x0F\x11\x43\x20\x48\x8D\x54\x24\x20\x48\x8B\x06";
const MASK: &[u8] = b"xxxxxxxxxxxxx";
const REPLACE: usize = 10;

/// Installed from src/Hooks/LocateCamera.cpp's registration (Boot, 10).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_locate_camera_install() -> bool {
    use cvr_engine::code::find_pattern;
    use cvr_engine::trampoline::{allocate, patch_jump, Code};
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 512) else { return false };
    let mut c = Code::new(tramp);
    c.volatile_call(4, &[0x48, 0x89, 0xD9, 0x0F, 0x28, 0xC8], OnLocateCameraCallback as *const () as u64) // mov rcx,rbx / movaps xmm1,xmm0
        .bytes(&PATTERN[..REPLACE]) // movss [rbx+20h],xmm0 / lea rdx,[rsp+20h]
        .jmp(found + REPLACE);
    // SAFETY: fresh arena memory, then a site whose 10 bytes the stub reproduces.
    unsafe {
        c.commit();
        patch_jump(found, REPLACE, tramp);
    }
    true
}
