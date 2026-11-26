//! PatchCamera (formerly src/Hooks/PatchCamera.cpp): where the cameras are written -- the engine's world-transform
//! write of every placed component (2.5: two sites, sub_140329890+97 and sub_14093A638+91), of which MAIN, VRCAM and
//! the device camera are ours. It consumes the heading, writes the camera's orientation and position (the head's
//! composition, the head translation, the eye separation), and publishes back the engine's pre-write quaternion
//! (LocateCamera's base next frame -- the back edge of the only real cycle) and the write ring entry FinalCamera
//! identifies the frame by.
//!
//! The compare-exchange on the aim epoch is not decoration: the site is reached from several engine job threads, so
//! "compose once per interval" needs it to mean once. The long notes on every decision below are at the C++
//! original's lines; this port keeps the decisions and their order, operation for operation.
use super::body_heading::{cvr_body_heading_offset, cvr_body_read_tracking_yaw};
use super::camera_link::{
    cvr_camera_anchor_recipe_read, cvr_camera_body_anchor_yaw, cvr_camera_cam_write_quat_publish, cvr_camera_cam_write_quat_read,
    cvr_camera_cam_write_record_push, cvr_camera_main_eye_centre_publish, cvr_camera_placed_pose_enabled, cvr_camera_placed_pose_push, HeadPose, Vec2,
};
use super::camera_math::{
    cvr_camera_desired_half_ipd, cvr_camera_plausible_position_vec4, cvr_camera_plausible_unit_quaternion, cvr_camera_plausible_unit_vector3, cvr_camera_right_vector_from_quaternion,
};
use super::pose_identity::{cvr_pose_invalidate, cvr_pose_publish_setup};
use super::roomscale_move::{cvr_roomscale_camera_consumed_at, cvr_roomscale_player_identity};
use core::ffi::c_void;
use cvr_body::cmath::{atan2, cos, sin, sqrt};
use cvr_camera::anchor::{av, compose_view_translation, AnchorRecipe};
use cvr_camera::frames::use_native_main_heading;
use cvr_camera::scene_comfort::{SceneCameraComfort, SceneComfortSettings};
use cvr_camera::vehicle::{Selection, VehicleStereoHeading};
use cvr_engine::code::{find_pattern, read, read_bytes, write};
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    // the camera state (include/Camera/CameraState.hpp, C linkage)
    static mut g_patchCameraHits: u64;
    static g_lensComp: AtomicUsize;
    static mut g_devCamLastWritten: [f32; 4];
    static mut g_devCamBase: [f32; 4];
    static mut g_devCamViewQuat: [f32; 4];
    static mut g_devCamAimYaw: f32;
    static mut g_devCamAimPitch: f32;
    static mut g_devCamFovOrig: f32;
    static g_devCamAimValid: AtomicI32;
    static g_devCamBaseValid: AtomicI32;
    static g_devCamFovSaved: AtomicI32;
    static g_devCamViewValid: AtomicI32;
    static g_devCamPosValid: AtomicI32;
    static g_devCamPosFP: [AtomicI32; 3];
    static g_camObjMain: AtomicUsize;
    static mut g_engineCamQuat: [f32; 4];
    static mut g_engineCamQuatValid: u32;
    static g_takeoverEntity: AtomicUsize;
    static g_sceneTier: AtomicI32;
    static g_bdActive: AtomicI32;
    static g_camComposedForPresent: AtomicU64;
    static mut g_headingSy: f32;
    static mut g_headingCy: f32;
    static mut g_viewYawUsedRad: f32;
    static mut g_viewYawUsedValid: i32;
    static g_bdSceneQuat: [f32; 4];
    static g_bdScenePoseValid: AtomicI32;
    static g_bdScenePosFP: [AtomicI32; 3];
    static mut g_headQuatComposed: [f32; 4];
    static mut g_headQuatValid: u32;
    static g_bdPushQuat: [f32; 4];
    static g_locatePosValid: AtomicI32;
    static g_locatePosFP: [AtomicI32; 3];
    static g_locateCenterFP: [AtomicI32; 3];
    static g_headDeltaValid: AtomicU32;
    static g_headDeltaFP: [AtomicI32; 3];
    static g_lookStickX: AtomicU32;
    static g_headingValid: u32;
    static g_headingPitchS: f32;
    static g_headingPitchC: f32;
    static g_normalFovOverrideValue: f32;
    static g_isInVehicle: bool;
    // the switches and the census
    static CyberpunkVR_SceneStickYawDps: f32;
    static mut CyberpunkVR_DebugSceneStickYawDeg: f32;
    static mut CyberpunkVR_DebugFppSceneComfortLagDeg: f32;
    static mut CyberpunkVR_DebugVehicleWorldRecompose: u64;
    static mut CyberpunkVR_DebugSceneWorldRecompose: u64;
    static mut CyberpunkVR_DebugTakeoverWorldRecompose: u64;
    static CyberpunkVR_SightApproach: [f32; 3];
    static mut CyberpunkVR_DebugTidPatchCam: u32;
    static mut CyberpunkVR_DebugCamThreadSwitches: u64;
    static CyberpunkVR_DebugPatchCamMain: u64;
    static CyberpunkVR_DeviceCamOrient: i32;
    static CyberpunkVR_CamWriteInPatch: i32;
    static CyberpunkVR_CamComposeAtWrite: i32;
    static CyberpunkVR_OneSamplePerFrame: i32;
    static CyberpunkVR_PoseLocateAtWrite: i32;
    static CyberpunkVR_HeadingFromPreWrite: i32;
    static CyberpunkVR_ViewYawFromEngine: i32;
    static CyberpunkVR_EngineBodyYawValid: i32;
    static CyberpunkVR_EngineBodyYawZ: f32;
    static CyberpunkVR_EngineBodyYawW: f32;
    static mut CyberpunkVR_DebugHeadingStepDeg: f32;
    static CyberpunkVR_BdSceneBaseInPatch: i32;
    static mut CyberpunkVR_DebugBdSceneBaseInPatch: u64;
    static mut CyberpunkVR_DebugPoseLocatedAtWrite: u64;
    static mut CyberpunkVR_DebugPoseFromCache: u64;
    static mut CyberpunkVR_DebugCamComposed: u64;
    static mut CyberpunkVR_DebugCamVrcamFirst: u64;
    static mut CyberpunkVR_DebugCamNoHmd: u64;
    static CyberpunkVR_CamWriteOrientInVehicle: i32;
    static CyberpunkVR_BdEditorAlign: i32;
    static mut CyberpunkVR_DebugBdEditorAlign: u64;
    static CyberpunkVR_BdPushTransform: i32;
    static CyberpunkVR_VrcamPosFromMain: i32;
    static mut CyberpunkVR_DebugVrcamPosFromMain: u64;
    static mut CyberpunkVR_EngineCamPosFP: [i32; 3];
    static mut CyberpunkVR_EngineCamPosValid: i32;
    static CyberpunkVR_DevCamOffsetX: f32;
    static CyberpunkVR_DevCamOffsetY: f32;
    static CyberpunkVR_DevCamOffsetZ: f32;
    static CyberpunkVR_PlayerEntityValid: i32;
    static CyberpunkVR_PlayerEntityPos: [f32; 3];
    static CyberpunkVR_BodyYawFinalValid: i32;
    static CyberpunkVR_BodyYawFinalRad: f32;
    static CyberpunkVR_CamMountCompensate: i32;
    static mut CyberpunkVR_DebugCamMountM: f32;
    static CyberpunkVR_HeadTranslationInPatch: i32;
    static CyberpunkVR_VrcamHeadTranslation: i32;
    static CyberpunkVR_DeltaFromFreshSample: i32;
    static mut CyberpunkVR_DebugDeltaRebuilt: u64;
    static mut CyberpunkVR_DebugVrcamPosWrites: u64;
    static mut CyberpunkVR_DebugMainPosWrites: u64;
    static CyberpunkVR_IpdInWorldPos: i32;
    static CyberpunkVR_BdIpdInLocate: i32;
    static mut CyberpunkVR_MainIsRightEye: i32;
    static mut CyberpunkVR_DebugIpdWorldWrites: u64;
    static CyberpunkVR_IpdInPosAB: i32;
    // the rest of the plugin, in C form (src/Hooks/HookServices.cpp)
    fn cvr_classify_patch_camera_owner(owner: usize) -> i32;
    fn cvr_refresh_lens_from_component();
    fn cvr_braindance_camera_match(object: usize, quat: *const f32) -> bool;
    fn cvr_locate_owns_takeover() -> bool;
    fn cvr_bd_push_owns_composition() -> bool;
    fn cvr_cpp_scene_stick_yaw_armed() -> bool;
    fn cvr_cpp_scene_comfort_apply(raw: *const f32, out: *mut f32) -> bool;
    fn cvr_push_lens_head_transform(quat: *const f32);
    fn cvr_cpp_surveillance_view_yaw(lens: usize, owner: usize, origin: u64, owner_yaw: f32, yaw: *mut f32) -> bool;
    fn cvr_cpp_fbt_camera_eye_height(z: *mut f32) -> bool;
    fn cvr_afw_active() -> bool;
    fn cvr_afw_frame_eye() -> i32;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_xr_tracking_origin_serial() -> u64;
    fn cvr_xr_acquire_camera_pose_frame(head: *mut HeadPose, consumed: *mut Vec2, sequence: *mut u64) -> bool;
    fn cvr_xr_locate_head_pose_at(time: i64, out: *mut HeadPose) -> bool;
    fn cvr_xr_get_head_pose(out: *mut HeadPose) -> bool;
    fn cvr_cpp_xr_push_render_head_pose(pose: *const HeadPose);
    fn cvr_cpp_patch_fast_path() -> usize;
    fn cvr_cpp_telemetry_patch_hits() -> usize;
    fn cvr_body_native_pair_publish(
        centre: *const i32,
        body_base: *const i32,
        entity: *const i32,
        camera_quat: *const f32,
        entity_quat: *const f32,
        owner: usize,
        origin: u64,
        owner_stable: bool,
    );
    fn GetMenuRectMode() -> i32;
    fn GetMenuMode() -> i32;
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
fn plausible_q(q: &[f32; 4]) -> bool {
    // SAFETY: four floats.
    unsafe { cvr_camera_plausible_unit_quaternion(q.as_ptr()) }
}
fn mul_quat(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    // VrCore.cpp MulQuat, operation for operation
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
/// The test clock when one is set (VRIK_ClockQpc, 1 MHz), QueryPerformanceCounter otherwise.
fn qpc() -> (i64, i64) {
    super::env::qpc()
}
fn rd_i32x3(at: usize) -> Option<[i32; 3]> {
    let mut p = [0i32; 3];
    for (i, v) in p.iter_mut().enumerate() {
        *v = read::<u32>(at + i * 4)? as i32;
    }
    Some(p)
}

/// SDK IComponent::owner+50, Entity::transformComponent+B0 and IPlacedComponent::worldTransform+E0, verified at the
/// MAIN write in34628.
#[derive(Clone, Copy, Default, PartialEq)]
struct CameraOwnerWorld {
    owner: usize,
    root: usize,
    position: [i32; 4],
    rotation: [f32; 4],
}
impl CameraOwnerWorld {
    fn same_bytes(&self, other: &CameraOwnerWorld) -> bool {
        self.owner == other.owner
            && self.root == other.root
            && self.position == other.position
            && self.rotation.iter().zip(other.rotation.iter()).all(|(a, b)| a.to_bits() == b.to_bits())
    }
}
fn read_camera_owner_world(camera: usize, expected_owner: usize) -> Option<CameraOwnerWorld> {
    let owner = read::<usize>(camera.wrapping_add(0x50))?;
    if owner == 0 || owner != expected_owner {
        return None;
    }
    let root = read::<usize>(owner + 0xB0)?;
    if root == 0 {
        return None;
    }
    Some(CameraOwnerWorld { owner, root, position: read::<[i32; 4]>(root + 0xE0)?, rotation: read::<[f32; 4]>(root + 0xF0)? })
}

/// Everything the composition mutex guards (s_cameraComposeMutex and the static locals under it).
struct Compose {
    fpp_comfort: SceneCameraComfort,
    fpp_comfort_qpc: i64,
    vehicle: VehicleStereoHeading,
    takeover: VehicleStereoHeading,
    surveillance_render_yaw: f32,
    surveillance_render_yaw_valid: bool,
    scene_yaw: f32,
    scene_prev: i64,
    yaw_held: f32,
    yaw_held_valid: bool,
    prev_yaw: f32,
    prev_yaw_valid: bool,
}
static COMPOSE: Mutex<Option<Compose>> = Mutex::new(None);
/// The census snapshots (last MAIN and VRCAM positions and the head delta each was patched against).
static CENSUS: Mutex<[[i32; 3]; 4]> = Mutex::new([[0; 3]; 4]);
/// The learned mount vector of the player's camera in the body frame.
static MOUNT: Mutex<(f32, f32, bool)> = Mutex::new((0.0, 0.0, false));

impl Compose {
    fn new() -> Self {
        Compose {
            fpp_comfort: SceneCameraComfort::new(),
            fpp_comfort_qpc: 0,
            vehicle: VehicleStereoHeading::default(),
            takeover: VehicleStereoHeading::default(),
            surveillance_render_yaw: 0.0,
            surveillance_render_yaw_valid: false,
            scene_yaw: 0.0,
            scene_prev: 0,
            yaw_held: 0.0,
            yaw_held_valid: false,
            prev_yaw: 0.0,
            prev_yaw_valid: false,
        }
    }
    /// THE SCENE YAW, INTEGRATED HERE BECAUSE THIS IS WHERE IT IS SPENT: against the time since the previous
    /// composition; closing the gate zeroes it, so a scene that ends hands the view back to the game's yaw.
    unsafe fn scene_stick_yaw_rad(&mut self) -> f32 {
        // SAFETY: the plugin's exports and C++ services.
        unsafe {
            let dps = CyberpunkVR_SceneStickYawDps;
            let armed = cvr_cpp_scene_stick_yaw_armed();
            let (now, freq) = qpc();
            if !armed {
                self.scene_yaw = 0.0;
                self.scene_prev = now;
                CyberpunkVR_DebugSceneStickYawDeg = 0.0;
                return 0.0;
            }
            let mut dt = 0.0f32;
            if self.scene_prev != 0 && freq > 0 {
                dt = ((now - self.scene_prev) as f64 / freq as f64) as f32;
            }
            self.scene_prev = now;
            if dt < 0.0 || dt > 0.2 {
                dt = 0.0; // a pause or a load screen is not a turn
            }
            let mut ax = f32::from_bits(g_lookStickX.load(Ordering::Relaxed));
            if ax > 1.0 {
                ax = 1.0;
            }
            if ax < -1.0 {
                ax = -1.0;
            }
            if ax.abs() < 0.12 {
                ax = 0.0; // the same dead zone the pad has
            }
            self.scene_yaw -= ax * dps * 0.017_453_292_52 * dt; // stick right turns the view right
            while self.scene_yaw > 6.283_185_31 {
                self.scene_yaw -= 6.283_185_31;
            }
            while self.scene_yaw < -6.283_185_31 {
                self.scene_yaw += 6.283_185_31;
            }
            CyberpunkVR_DebugSceneStickYawDeg = self.scene_yaw * 57.295_779_51;
            self.scene_yaw
        }
    }
    /// Tier 4 (FPP cinematic) and 5 (cinematic): the scene owns the player's camera.
    unsafe fn fpp_comfort_wanted() -> bool {
        // SAFETY: C++ service and the plugin's globals.
        unsafe { super::live_reads::scene_cam_comfort_deg() > 0.0 && !g_isInVehicle && g_sceneTier.load(Ordering::Relaxed) >= 4 }
    }
    fn fpp_comfort_dt(&mut self) -> f32 {
        let (now, freq) = qpc();
        let mut dt = 0.0f32;
        if self.fpp_comfort_qpc != 0 && freq > 0 {
            dt = ((now - self.fpp_comfort_qpc) as f64 / freq as f64) as f32;
        }
        self.fpp_comfort_qpc = now;
        dt
    }
    fn fpp_comfort_stop(&mut self) {
        self.fpp_comfort.reset();
        self.fpp_comfort_qpc = 0;
        // SAFETY: an exported float.
        unsafe { CyberpunkVR_DebugFppSceneComfortLagDeg = 0.0 };
    }
}

/// rcx = the component + 0xF0 (its camera state), rdx = the component.
#[no_mangle]
pub unsafe extern "C" fn OnPatchCameraCallback(camera_state: *mut f32, owner_state: usize) {
    // SAFETY: the engine's component (SEH reads and writes), the plugin's globals and C++ services.
    unsafe { patch(camera_state, owner_state) }
}

unsafe fn patch(camera_state: *mut f32, owner: usize) {
    // SAFETY (whole body): as OnPatchCameraCallback.
    unsafe {
        g_patchCameraHits = g_patchCameraHits.wrapping_add(1);
        let cam_kind = cvr_classify_patch_camera_owner(owner);
        if (1..=3).contains(&cam_kind) {
            cvr_pose_invalidate(owner.wrapping_add(0xE0));
        }
        let player_now = cvr_roomscale_player_identity();
        let camera_owner_before = if cam_kind == 1 || cam_kind == 2 { read_camera_owner_world(owner, player_now) } else { None };
        let have_camera_owner = camera_owner_before.is_some();
        let camera_owner_before = camera_owner_before.unwrap_or_default();

        // The lens the script handed over, re-read at the moment of the write.
        if g_lensComp.load(Ordering::Relaxed) != 0 {
            cvr_refresh_lens_from_component();
        }
        let state = camera_state as usize;
        if camera_state.is_null() || state < 0x10000 {
            return;
        }
        // THE DISCARD MOVES ABOVE THE READS: only the braindance camera hunt needs an unrecognised object's quaternion.
        let bd_hunt = cam_kind == 0 && g_bdActive.load(Ordering::Relaxed) != 0;
        if cam_kind == 0 && !bd_hunt {
            return;
        }
        let (Some(mut quat), Some(pos_a), Some(pos_b)) = (read::<[f32; 4]>(state), read::<[f32; 4]>(state + 16), read::<[f32; 4]>(state + 32)) else {
            return;
        };
        // BRAINDANCE: THE OBJECT THE SCENE RENDERS THROUGH, IDENTIFIED BEFORE IT IS DISCARDED.
        if cam_kind == 0 && g_bdActive.load(Ordering::Relaxed) != 0 && plausible_q(&quat) {
            cvr_braindance_camera_match(owner, quat.as_ptr());
        }
        if cam_kind == 0 {
            return;
        }
        if diagnostics() {
            let tid = GetCurrentThreadId();
            if tid != CyberpunkVR_DebugTidPatchCam {
                CyberpunkVR_DebugTidPatchCam = tid;
                diag!(CyberpunkVR_DebugCamThreadSwitches += 1);
            }
        }

        // The census line (diagnostics, MAIN only, one in 900 camera writes).
        if diagnostics() && CyberpunkVR_DebugPatchCamMain % 900 == 1 && cam_kind == 1 && cvr_cpp_verbose_log() != 0 {
            census_log();
        }

        // ---- ORIENTATION ----
        // The device camera's own aim, taken before we write over it, refreshed every frame the engine touches it.
        let dev_last = g_devCamLastWritten;
        let engine_wrote_dev_cam = cam_kind == 3 && (quat[0] != dev_last[0] || quat[1] != dev_last[1] || quat[2] != dev_last[2] || quat[3] != dev_last[3]);
        let mut comp_quat = [0.0f32; 4];
        let mut have_comp_quat = false;
        if cam_kind == 3 && owner >= 0x10000 {
            if let Some(q) = read::<[f32; 4]>(owner + 0xF0) {
                comp_quat = q;
                have_comp_quat = plausible_q(&comp_quat);
            }
        }
        if cam_kind == 3 && (have_comp_quat || (plausible_q(&quat) && engine_wrote_dev_cam)) {
            let src = if have_comp_quat { comp_quat } else { quat };
            g_devCamBase = src;
            // the same aim as yaw + pitch: X right, Y forward, Z up, so the second column is the game's forward
            let fx = 2.0 * (src[0] * src[1] - src[2] * src[3]);
            let fy = 1.0 - 2.0 * (src[0] * src[0] + src[2] * src[2]);
            let fz = 2.0 * (src[1] * src[2] + src[0] * src[3]);
            let fh = sqrt(fx * fx + fy * fy);
            g_devCamAimYaw = atan2(-fx, fy);
            g_devCamAimPitch = atan2(fz, fh);
            g_devCamAimValid.store(1, Ordering::Release);
            let first = g_devCamBaseValid.swap(1, Ordering::Release) == 0;
            if first && cvr_cpp_verbose_log() != 0 {
                super::env::log(&format!(
                    "PatchCamera: device camera aim q=({:.3},{:.3},{:.3},{:.3}) yaw={:.2} pitch={:.2} deg from={}\n",
                    src[0],
                    src[1],
                    src[2],
                    src[3],
                    g_devCamAimYaw * 57.29578,
                    g_devCamAimPitch * 57.29578,
                    if have_comp_quat { "component" } else { "state" }
                ));
            }
        }
        // THE FOV, the one thing a device camera never receives (+0x128, the camera component's fov).
        if cam_kind == 3 && owner >= 0x10000 && CyberpunkVR_DeviceCamOrient != 0 {
            let want = core::ptr::read_volatile(&raw const g_normalFovOverrideValue);
            if let Some(cur) = read::<f32>(owner + 0x128) {
                if cur > 1.0 && cur < 179.0 && want > 1.0 && want < 179.0 {
                    if g_devCamFovSaved.load(Ordering::Acquire) == 0 {
                        g_devCamFovOrig = cur;
                        g_devCamFovSaved.store(1, Ordering::Release);
                        if cvr_cpp_verbose_log() != 0 {
                            super::env::log(&format!("PatchCamera: device camera fov {cur:.3} -> {want:.3}\n"));
                        }
                    }
                    if (cur - want).abs() > 0.01 {
                        write(owner + 0x128, want);
                    }
                }
            }
        }

        // Native MAIN publication and shared composition use the same lock.
        let mut guard = COMPOSE.lock().unwrap_or_else(|p| p.into_inner());
        let c = guard.get_or_insert_with(Compose::new);
        let mut body_tracking_yaw = 0.0f32;
        let have_body_tracking_yaw = cvr_body_read_tracking_yaw(&mut body_tracking_yaw);
        let current_player = cvr_roomscale_player_identity();
        c.vehicle.set_source(current_player, g_camObjMain.load(Ordering::Acquire));
        if cam_kind == 1 && plausible_q(&quat) {
            if have_camera_owner && camera_owner_before.owner == current_player {
                c.vehicle.observe(owner, Some(&quat), Some(&camera_owner_before.rotation));
            }
            g_engineCamQuat = quat;
            core::ptr::write_volatile(&raw mut g_engineCamQuatValid, 1);
        }
        // A moving takeover has its own owner, separate from the player's seated FPP camera.
        let takeover_heading = CyberpunkVR_DeviceCamOrient != 0 && cvr_locate_owns_takeover() && g_devCamAimValid.load(Ordering::Acquire) != 0;
        let lens = if takeover_heading { g_lensComp.load(Ordering::Acquire) } else { 0 };
        let lens_owner = if takeover_heading { g_takeoverEntity.load(Ordering::Acquire) } else { 0 };
        let lens_owner_before = if lens != 0 && lens_owner != 0 { read_camera_owner_world(lens, lens_owner) } else { None };
        let have_lens_owner = lens_owner_before.is_some();
        let lens_owner_before = lens_owner_before.unwrap_or_default();
        c.takeover.set_source(lens_owner, lens);
        if cam_kind == 3 && owner == lens && have_lens_owner {
            c.takeover.observe(owner, Some(if have_comp_quat { &comp_quat } else { &quat }), Some(&lens_owner_before.rotation));
        }

        let mut hq = [0.0f32, 0.0, 0.0, 1.0];
        let mut have_write_quat = false;
        let mut written_pose = HeadPose::default();
        let mut written_pose_id = 0u64;
        let mut written_consumed = Vec2::default();
        let mut written_consumed_known = false;
        let mut orientation_written = false;
        let mut translation_label_known = false;
        let mut written_tracking_yaw = 0.0f32;
        let mut written_tracking_yaw_valid = false;

        let bd_push_owns = cvr_bd_push_owns_composition();
        let native_main_base = use_native_main_heading(
            g_isInVehicle,
            g_sceneTier.load(Ordering::Relaxed),
            cvr_cpp_device_cam_active(),
            g_bdActive.load(Ordering::Relaxed) != 0,
        );
        if CyberpunkVR_CamWriteInPatch != 0 && CyberpunkVR_CamComposeAtWrite != 0 {
            if core::ptr::read_volatile(&raw const g_headingValid) != 0 {
                // Claim, publish and read are one operation.
                let mut selected_pose = HeadPose::default();
                let mut selected_consumed = Vec2::default();
                let mut selected_sequence = 0u64;
                let selected = CyberpunkVR_OneSamplePerFrame != 0
                    && cvr_xr_acquire_camera_pose_frame(&mut selected_pose, &mut selected_consumed, &mut selected_sequence)
                    && selected_pose.valid;
                // GATE ON THE AIM EPOCH, NOT ON THE PRESENT COUNT.
                let epoch = if selected { selected_sequence | (1u64 << 62) } else { super::xr_calls::frame_aim_epoch() };
                let mut native_heading = Selection::default();
                let mut device_heading = Selection::default();
                let mut native_stick_yaw = 0.0f32;
                if native_main_base && CyberpunkVR_HeadingFromPreWrite != 0 {
                    let scene_yaw = c.scene_stick_yaw_rad();
                    native_stick_yaw = scene_yaw;
                    let entity = if have_camera_owner && camera_owner_before.owner == current_player { Some(&camera_owner_before.rotation) } else { None };
                    let hs = core::ptr::read_volatile(&raw const g_headingSy);
                    let hc = core::ptr::read_volatile(&raw const g_headingCy);
                    native_heading = c.vehicle.for_frame(epoch, entity, 2.0 * atan2(hs, hc) + scene_yaw, scene_yaw);
                } else {
                    c.fpp_comfort_stop(); // the next FPP cinematic starts on its own camera
                }
                if takeover_heading {
                    device_heading = c.takeover.for_frame(epoch, if have_lens_owner { Some(&lens_owner_before.rotation) } else { None }, g_devCamAimYaw, 0.0);
                }
                let mut driven_surveillance = false;
                if takeover_heading && have_lens_owner {
                    let q = lens_owner_before.rotation;
                    let owner_yaw = atan2(-2.0 * (q[0] * q[1] - q[2] * q[3]), 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]));
                    let mut view_yaw = 0.0f32;
                    let origin = if selected { selected_pose.origin_serial } else { cvr_cpp_xr_tracking_origin_serial() };
                    if cvr_cpp_surveillance_view_yaw(lens, lens_owner, origin, owner_yaw, &mut view_yaw) {
                        driven_surveillance = true;
                        device_heading.changed |= !c.surveillance_render_yaw_valid
                            || cvr_body::cmath::remainder(view_yaw - c.surveillance_render_yaw, 2.0 * 3.141_592_653_589_793_238_46).abs() > 1e-6;
                        device_heading.yaw = view_yaw;
                        c.surveillance_render_yaw = view_yaw;
                    }
                }
                c.surveillance_render_yaw_valid = driven_surveillance;
                // `epoch == 0`: the frame loop has not published an aim yet.
                let claimed = g_camComposedForPresent.load(Ordering::Acquire);
                let new_aim = epoch == 0
                    || (claimed != epoch && g_camComposedForPresent.compare_exchange(claimed, epoch, Ordering::AcqRel, Ordering::Acquire).is_ok());
                let mine = new_aim || native_heading.changed || device_heading.changed;
                if !new_aim && native_heading.changed {
                    if g_isInVehicle {
                        diag!(CyberpunkVR_DebugVehicleWorldRecompose += 1);
                    } else {
                        diag!(CyberpunkVR_DebugSceneWorldRecompose += 1);
                    }
                }
                if !new_aim && device_heading.changed {
                    diag!(CyberpunkVR_DebugTakeoverWorldRecompose += 1);
                }
                if mine {
                    // READ THE POSE HERE, FOR THIS FRAME -- the frame's sample LocateCamera placed the eye with.
                    let mut p = HeadPose::default();
                    let mut got = false;
                    if CyberpunkVR_OneSamplePerFrame != 0 {
                        p = selected_pose;
                        got = selected;
                        if got {
                            diag!(CyberpunkVR_DebugPoseLocatedAtWrite += 1);
                        }
                    } else if CyberpunkVR_PoseLocateAtWrite != 0 {
                        let aim = super::xr_calls::frame_aim_time();
                        if aim > 0 {
                            got = cvr_xr_locate_head_pose_at(aim, &mut p) && p.valid;
                            if got {
                                diag!(CyberpunkVR_DebugPoseLocatedAtWrite += 1);
                            }
                        }
                    }
                    if !got {
                        got = cvr_xr_get_head_pose(&mut p) && p.valid;
                        if got {
                            diag!(CyberpunkVR_DebugPoseFromCache += 1);
                        }
                    }
                    if got {
                        // THE HEADING OF THIS FRAME, NOT OF THE PREVIOUS ONE.
                        let mut h_sy = core::ptr::read_volatile(&raw const g_headingSy);
                        let mut h_cy = core::ptr::read_volatile(&raw const g_headingCy);
                        if have_body_tracking_yaw {
                            let yaw = body_tracking_yaw + c.scene_stick_yaw_rad();
                            g_viewYawUsedRad = yaw;
                            g_viewYawUsedValid = 1;
                            h_sy = sin(yaw * 0.5);
                            h_cy = cos(yaw * 0.5);
                        } else if CyberpunkVR_HeadingFromPreWrite != 0 && native_main_base {
                            let mut yaw = native_heading.yaw;
                            // AN FPP CINEMATIC TURNS THE PLAYER'S OWN CAMERA: the comfort filter on the scene's part.
                            if Compose::fpp_comfort_wanted() {
                                let dt = c.fpp_comfort_dt();
                                let settings = SceneComfortSettings { dead_deg: super::live_reads::scene_cam_comfort_deg(), ..SceneComfortSettings::default() };
                                yaw = c.fpp_comfort.step_yaw(yaw - native_stick_yaw, dt, &settings) + native_stick_yaw;
                                CyberpunkVR_DebugFppSceneComfortLagDeg = c.fpp_comfort.lag_deg();
                            } else {
                                c.fpp_comfort_stop();
                            }
                            g_viewYawUsedRad = yaw;
                            g_viewYawUsedValid = 1;
                            h_sy = sin(yaw * 0.5);
                            h_cy = cos(yaw * 0.5);
                        } else if CyberpunkVR_HeadingFromPreWrite != 0 && plausible_q(&quat) {
                            let fwd_x = 2.0 * (quat[0] * quat[1] - quat[2] * quat[3]);
                            let fwd_y = 1.0 - 2.0 * (quat[0] * quat[0] + quat[2] * quat[2]);
                            // A NEAR-VERTICAL FORWARD CARRIES NO YAW: hold the last real heading.
                            let mut yaw;
                            if fwd_x * fwd_x + fwd_y * fwd_y > 1.0e-6 {
                                yaw = atan2(-fwd_x, fwd_y);
                                c.yaw_held = yaw;
                                c.yaw_held_valid = true;
                            } else if c.yaw_held_valid {
                                yaw = c.yaw_held;
                            } else {
                                yaw = 2.0 * atan2(h_sy, h_cy);
                            }
                            // NO LEAD, NO KNOB: the step is kept as a census of the turn rate.
                            if c.prev_yaw_valid {
                                let mut d_yaw = yaw - c.prev_yaw;
                                while d_yaw > 3.141_592_65 {
                                    d_yaw -= 6.283_185_31;
                                }
                                while d_yaw < -3.141_592_65 {
                                    d_yaw += 6.283_185_31;
                                }
                                let degs = d_yaw.abs() * 57.295_779_5;
                                if degs > CyberpunkVR_DebugHeadingStepDeg {
                                    CyberpunkVR_DebugHeadingStepDeg = degs;
                                }
                            }
                            c.prev_yaw = yaw;
                            c.prev_yaw_valid = true;
                            // Native fallback outside the independent on-foot tracking frame.
                            if CyberpunkVR_ViewYawFromEngine != 0 && CyberpunkVR_EngineBodyYawValid != 0 && !g_isInVehicle {
                                let (z, w) = (CyberpunkVR_EngineBodyYawZ, CyberpunkVR_EngineBodyYawW);
                                if z != 0.0 || w != 0.0 {
                                    yaw = 2.0 * atan2(z, w);
                                }
                            }
                            yaw += c.scene_stick_yaw_rad();
                            g_viewYawUsedRad = yaw;
                            g_viewYawUsedValid = 1;
                            h_sy = sin(yaw * 0.5);
                            h_cy = cos(yaw * 0.5);
                        }
                        // XR (x, y, z) -> game (x, -z, y); yaw * pitch * HMD.
                        let mut head = mul_quat([0.0, 0.0, h_sy, h_cy], [g_headingPitchS, 0.0, 0.0, g_headingPitchC]);
                        // A BRAINDANCE HAS NO BODY HEADING: the scene camera's own (filtered) aim is the base.
                        if CyberpunkVR_BdSceneBaseInPatch != 0 && g_bdActive.load(Ordering::Relaxed) != 0 && g_bdScenePoseValid.load(Ordering::Acquire) != 0 {
                            let mut scene_base = [0.0f32; 4];
                            cvr_cpp_scene_comfort_apply(g_bdSceneQuat.as_ptr(), scene_base.as_mut_ptr());
                            head = scene_base;
                            diag!(CyberpunkVR_DebugBdSceneBaseInPatch += 1);
                        }
                        // A DEVICE CAMERA HAS NO BODY HEADING: the lens yaw, MAIN's pitch half, for every kind.
                        if CyberpunkVR_DeviceCamOrient != 0 && cvr_cpp_device_cam_active() && g_devCamAimValid.load(Ordering::Acquire) != 0 {
                            let ly = (if takeover_heading { device_heading.yaw } else { g_devCamAimYaw }) * 0.5;
                            head = mul_quat([0.0, 0.0, sin(ly), cos(ly)], [g_headingPitchS, 0.0, 0.0, g_headingPitchC]);
                        }
                        let mut r = mul_quat(head, [p.ori[0], -p.ori[2], p.ori[1], p.ori[3]]);
                        normalize_quat(&mut r);
                        let pose_consumed = if selected { selected_consumed } else { consumed_at(p.origin_serial) };
                        cvr_camera_cam_write_quat_publish(r[0], r[1], r[2], r[3], &p, &pose_consumed, if selected { selected_sequence } else { 0 });
                        // NOT while the push owns the composition.
                        if !bd_push_owns {
                            cvr_camera_cam_write_record_push(&r, &p);
                        }
                        diag!(CyberpunkVR_DebugCamComposed += 1);
                        if cam_kind == 2 {
                            diag!(CyberpunkVR_DebugCamVrcamFirst += 1);
                        }
                        // THE pose that is in the image, published at the instant it goes into the camera.
                        if !bd_push_owns {
                            cvr_cpp_xr_push_render_head_pose(&p);
                        }
                        g_headQuatComposed = r;
                        core::ptr::write_volatile(&raw mut g_headQuatValid, 1);
                    } else {
                        diag!(CyberpunkVR_DebugCamNoHmd += 1);
                    }
                }
                // Reuse the complete published quaternion.
                have_write_quat = cvr_camera_cam_write_quat_read(&mut hq, &mut written_pose, &mut written_consumed, &mut written_consumed_known, &mut written_pose_id);
                written_tracking_yaw = core::ptr::read_volatile(&raw const g_viewYawUsedRad);
                written_tracking_yaw_valid = have_write_quat && core::ptr::read_volatile(&raw const g_viewYawUsedValid) != 0;
            }
        } else if CyberpunkVR_CamWriteInPatch != 0 && core::ptr::read_volatile(&raw const g_headQuatValid) != 0 {
            hq = g_headQuatComposed;
            have_write_quat = true;
        }
        drop(guard);

        // ONE COMPOSITION FOR BOTH EYES -- only the value is taken over.
        if bd_push_owns {
            let b = g_bdPushQuat;
            let l2 = b[0] * b[0] + b[1] * b[1] + b[2] * b[2] + b[3] * b[3];
            if l2 > 0.9 && l2 < 1.1 {
                hq = b;
                have_write_quat = true;
            }
        }
        // BRAINDANCE: BOTH EYES FROM THE SCENE CAMERA AND THE FRAME'S OWN HEAD SAMPLE.
        let mut _bd_second_eye = false;
        if (cam_kind == 1 || cam_kind == 2) && g_bdActive.load(Ordering::Relaxed) != 0 && g_bdScenePoseValid.load(Ordering::Acquire) != 0 {
            let mut bp = HeadPose::default();
            let mut got_bd = CyberpunkVR_OneSamplePerFrame != 0 && super::xr_calls::acquire_frame_head_sample(&mut bp) && bp.valid;
            if !got_bd {
                got_bd = cvr_xr_get_head_pose(&mut bp) && bp.valid;
            }
            if got_bd {
                let mut scene_base = [0.0f32; 4];
                cvr_cpp_scene_comfort_apply(g_bdSceneQuat.as_ptr(), scene_base.as_mut_ptr());
                let mut o = mul_quat(scene_base, [bp.ori[0], -bp.ori[2], bp.ori[1], bp.ori[3]]);
                normalize_quat(&mut o);
                if plausible_q(&o) {
                    hq = o;
                    have_write_quat = true;
                    if cam_kind == 2 {
                        _bd_second_eye = true;
                    }
                }
            }
        }
        // THE SECOND EYE TAKES THE CAMERA'S OWN AIM when the head steering is off.
        if cam_kind == 2 && cvr_cpp_device_cam_active() && CyberpunkVR_DeviceCamOrient == 0 && g_devCamBaseValid.load(Ordering::Acquire) != 0 {
            hq = g_devCamBase;
            have_write_quat = true;
        }
        // TEST LEVER: LEAVE THE VEHICLE CAMERA'S ORIENTATION ALONE.
        if g_isInVehicle && CyberpunkVR_CamWriteOrientInVehicle == 0 {
            have_write_quat = false;
        }
        // A device camera is left as the engine wrote it unless the head steering is on.
        if cam_kind == 3 && CyberpunkVR_DeviceCamOrient == 0 {
            have_write_quat = false;
        }
        // AND WHEN THE LOCATED ROUTE OWNS THE TAKEOVER, NEITHER THE LENS NOR THE SECOND EYE IS WRITTEN HERE.
        if cam_kind == 3 && cvr_locate_owns_takeover() {
            have_write_quat = false;
        }
        if cam_kind == 2 && cvr_locate_owns_takeover() {
            have_write_quat = false;
        }
        if have_write_quat && plausible_q(&hq) {
            orientation_written = write(state, hq[0]);
            orientation_written &= write(state + 4, hq[1]);
            orientation_written &= write(state + 8, hq[2]);
            orientation_written &= write(state + 12, hq[3]);
            quat = hq;
            if cam_kind == 3 {
                g_devCamLastWritten = hq;
                g_devCamViewQuat = hq;
                g_devCamViewValid.store(1, Ordering::Release);
            }
        }

        // ---- WORLD POSITION: head translation, and the EYE SEPARATION ----
        if owner >= 0x10000 {
            let pos_addr = owner + 0xE0;
            let read_p = rd_i32x3(pos_addr);
            let ok = read_p.is_some();
            let mut p = read_p.unwrap_or_else(|| {
                // the C++ loop stops at the failing component, the earlier ones read
                let mut partial = [0i32; 3];
                for (i, v) in partial.iter_mut().enumerate() {
                    match read::<u32>(pos_addr + i * 4) {
                        Some(x) => *v = x as i32,
                        None => break,
                    }
                }
                partial
            });
            let mut dirty = false;
            let bd_active = g_bdActive.load(Ordering::Relaxed) != 0;
            // The braindance EDITOR's shared base, as one predicate.
            let bd_editor_align = cam_kind == 2
                && ok
                && CyberpunkVR_BdEditorAlign != 0
                && g_locatePosValid.load(Ordering::Acquire) != 0
                && bd_active
                && g_bdScenePoseValid.load(Ordering::Acquire) == 0;
            if cam_kind == 2 && ok && (CyberpunkVR_BdPushTransform & 1) == 0 && bd_active && g_bdScenePoseValid.load(Ordering::Acquire) != 0 {
                for (i, v) in p.iter_mut().enumerate() {
                    *v = g_bdScenePosFP[i].load(Ordering::Relaxed);
                }
                dirty = true;
            } else if bd_editor_align {
                for (i, v) in p.iter_mut().enumerate() {
                    *v = g_locateCenterFP[i].load(Ordering::Relaxed);
                }
                dirty = true;
                diag!(CyberpunkVR_DebugBdEditorAlign += 1);
            } else if cam_kind == 2 && ok && CyberpunkVR_VrcamPosFromMain == 3 && g_locatePosValid.load(Ordering::Acquire) != 0 {
                // MODE 3: THE SECOND VIEW SITS ON MAIN'S HEAD CENTRE.
                for (i, v) in p.iter_mut().enumerate() {
                    *v = g_locateCenterFP[i].load(Ordering::Relaxed);
                }
                dirty = true;
                diag!(CyberpunkVR_DebugVrcamPosFromMain += 1);
            } else if cam_kind == 2
                && ok
                && CyberpunkVR_EngineCamPosValid != 0
                && (CyberpunkVR_VrcamPosFromMain == 2 || (CyberpunkVR_VrcamPosFromMain == 1 && bd_active))
            {
                p = CyberpunkVR_EngineCamPosFP;
                dirty = true;
                diag!(CyberpunkVR_DebugVrcamPosFromMain += 1);
            }
            if cam_kind == 3 {
                for (i, v) in p.iter().enumerate() {
                    g_devCamPosFP[i].store(*v, Ordering::Relaxed);
                }
                // SET, NEVER CLEARED HERE.
                if ok {
                    g_devCamPosValid.store(1, Ordering::Release);
                }
            } else if cam_kind == 2 && ok && cvr_cpp_device_cam_active() && !cvr_locate_owns_takeover() && g_devCamPosValid.load(Ordering::Acquire) != 0 {
                for (i, v) in p.iter_mut().enumerate() {
                    *v = g_devCamPosFP[i].load(Ordering::Relaxed);
                }
                dirty = true;
            }
            // THE VIEWPOINT OFFSET FOR A TAKEN-OVER CAMERA (yaw-only, both kinds, not the pushed second eye).
            if ok && cvr_cpp_device_cam_active() && (cam_kind == 2 || cam_kind == 3) && !(cam_kind == 2 && cvr_locate_owns_takeover()) {
                let (ox, oy, oz) = (CyberpunkVR_DevCamOffsetX, CyberpunkVR_DevCamOffsetY, CyberpunkVR_DevCamOffsetZ);
                if ox != 0.0 || oy != 0.0 || oz != 0.0 {
                    let yaw = if g_devCamAimValid.load(Ordering::Acquire) != 0 { g_devCamAimYaw } else { 0.0 };
                    let (sy, cy) = (sin(yaw), cos(yaw));
                    let wx = ox * cy - oy * sy;
                    let wy = ox * sy + oy * cy;
                    p[0] = p[0].wrapping_add((wx * 131072.0) as i32);
                    p[1] = p[1].wrapping_add((wy * 131072.0) as i32);
                    p[2] = p[2].wrapping_add((oz * 131072.0) as i32);
                    dirty = true;
                }
            }
            // THE MOUNT SWEEP, CANCELLED WITHOUT EVER READING BACK WHAT WE WROTE.
            let bd_on_scene = (cam_kind == 1 || cam_kind == 2) && bd_active && g_bdScenePoseValid.load(Ordering::Acquire) != 0;
            let body_mount_applies = !(cvr_cpp_device_cam_active() && (cam_kind == 2 || cam_kind == 3)) && !bd_on_scene;
            if ok && body_mount_applies && CyberpunkVR_PlayerEntityValid != 0 && CyberpunkVR_BodyYawFinalValid != 0 {
                let mut mount = MOUNT.lock().unwrap_or_else(|q| q.into_inner());
                let k_fp = 1.0f32 / 131072.0;
                let h = CyberpunkVR_BodyYawFinalRad;
                let a = cvr_body_heading_offset();
                let rx = p[0] as f32 * k_fp - CyberpunkVR_PlayerEntityPos[0];
                let ry = p[1] as f32 * k_fp - CyberpunkVR_PlayerEntityPos[1];
                if a == 0.0 {
                    let (c_h, s_h) = (cos(-h), sin(-h));
                    let mx = rx * c_h - ry * s_h;
                    let my = rx * s_h + ry * c_h;
                    if !mount.2 {
                        *mount = (mx, my, true);
                    } else {
                        mount.0 += (mx - mount.0) * 0.05;
                        mount.1 += (my - mount.1) * 0.05;
                    }
                    CyberpunkVR_DebugCamMountM = sqrt(mount.0 * mount.0 + mount.1 * mount.1);
                } else if mount.2 && CyberpunkVR_CamMountCompensate != 0 {
                    let (c0, s0) = (cos(h - a), sin(h - a));
                    let (c1, s1) = (cos(h), sin(h));
                    let dx = (mount.0 * c0 - mount.1 * s0) - (mount.0 * c1 - mount.1 * s1);
                    let dy = (mount.0 * s0 + mount.1 * c0) - (mount.0 * s1 + mount.1 * c1);
                    p[0] = p[0].wrapping_add((dx * 131072.0) as i32);
                    p[1] = p[1].wrapping_add((dy * 131072.0) as i32);
                    dirty = true;
                }
            }
            // THE ENGINE'S OWN CAMERA POSITION, PUBLISHED BEFORE WE MOVE IT, whenever MAIN is patched.
            if ok && cam_kind == 1 {
                CyberpunkVR_EngineCamPosFP = p;
                CyberpunkVR_EngineCamPosValid = 1;
            }
            let body_base_before_vr = p;
            let want_main_here = cam_kind == 1 && CyberpunkVR_HeadTranslationInPatch != 0 && !bd_on_scene;
            let want_vrcam_here = cam_kind == 2
                && CyberpunkVR_VrcamHeadTranslation != 0
                && !cvr_cpp_device_cam_active()
                && !bd_on_scene
                && CyberpunkVR_VrcamPosFromMain != 3
                && !bd_push_owns
                && !bd_editor_align;
            if ok && (want_main_here || want_vrcam_here) && g_headDeltaValid.load(Ordering::Acquire) != 0 {
                // THE DELTA IS REBUILT HERE, NOT READ: from this eye's own head sample and the framed recipe.
                let mut d = [g_headDeltaFP[0].load(Ordering::Relaxed), g_headDeltaFP[1].load(Ordering::Relaxed), g_headDeltaFP[2].load(Ordering::Relaxed)];
                let mut translation_pose = written_pose;
                let mut translation_consumed = written_consumed;
                let translation_valid = (translation_pose.valid && translation_pose.origin_serial == cvr_cpp_xr_tracking_origin_serial())
                    || (!orientation_written
                        && cvr_xr_acquire_camera_pose_frame(&mut translation_pose, &mut translation_consumed, core::ptr::null_mut())
                        && translation_pose.valid);
                let mut anchor = AnchorRecipe::default();
                if CyberpunkVR_DeltaFromFreshSample != 0 && translation_valid && cvr_camera_anchor_recipe_read(&mut anchor) {
                    let sc = anchor.scale;
                    let consumed = if !orientation_written || written_consumed_known {
                        translation_consumed
                    } else {
                        consumed_at(translation_pose.origin_serial)
                    };
                    let tracking_yaw = if written_tracking_yaw_valid {
                        written_tracking_yaw
                    } else if core::ptr::read_volatile(&raw const g_viewYawUsedValid) != 0 {
                        core::ptr::read_volatile(&raw const g_viewYawUsedRad)
                    } else {
                        anchor.tracking_yaw
                    };
                    let tp = translation_pose.pos;
                    let delta = compose_view_translation(
                        av((tp[0] - consumed.x) * sc, (-tp[2] - consumed.y) * sc, tp[1] * sc),
                        &anchor,
                        tracking_yaw,
                        cvr_camera_body_anchor_yaw(anchor.model_yaw),
                    );
                    d = [(delta.x * 131072.0) as i32, (delta.y * 131072.0) as i32, (delta.z * 131072.0) as i32];
                    if written_pose.valid && written_pose.origin_serial == translation_pose.origin_serial {
                        // Orientation belongs to the published composition; position to this eye's sample.
                        written_pose.pos = translation_pose.pos;
                        translation_label_known = true;
                    }
                    diag!(CyberpunkVR_DebugDeltaRebuilt += 1);
                }
                for i in 0..3 {
                    p[i] = p[i].wrapping_add(d[i]);
                }
                // Full-body tracking: the camera at V's eyes, an absolute height over the entity.
                let mut eye_z = 0.0f32;
                if cvr_cpp_fbt_camera_eye_height(&mut eye_z) {
                    p[2] = ((CyberpunkVR_PlayerEntityPos[2] + eye_z) * 131072.0) as i32;
                }
                dirty = true;
                if cam_kind == 2 {
                    diag!(CyberpunkVR_DebugVrcamPosWrites += 1);
                } else {
                    diag!(CyberpunkVR_DebugMainPosWrites += 1);
                }
            }
            // Eye separation, symmetric about the head: MAIN the left eye, VRCAM the right.
            let centre_before_ipd = p;
            // The eye brought to the sight, on both eyes and only where the head translation is ours.
            if ok && (want_main_here || want_vrcam_here) {
                let a = CyberpunkVR_SightApproach;
                if a[0] != 0.0 || a[1] != 0.0 || a[2] != 0.0 {
                    for i in 0..3 {
                        p[i] = p[i].wrapping_add((a[i] * 131072.0) as i32);
                    }
                    dirty = true;
                }
            }
            if ok && CyberpunkVR_IpdInWorldPos != 0 && plausible_q(&quat) && !(cam_kind == 2 && (bd_push_owns || cvr_locate_owns_takeover())) {
                let half = cvr_camera_desired_half_ipd();
                if half != 0.0 {
                    let mut r = [0.0f32; 3];
                    cvr_camera_right_vector_from_quaternion(quat.as_ptr(), r.as_mut_ptr());
                    // MAIN'S HALF IS NOT WRITTEN HERE IN A BRAINDANCE (it goes into the located buffer).
                    let bd_main_skip = cam_kind == 1 && CyberpunkVR_BdIpdInLocate != 0 && bd_active && g_bdScenePoseValid.load(Ordering::Acquire) != 0;
                    if cvr_camera_plausible_unit_vector3(r.as_ptr()) && !bd_main_skip {
                        // AFW: MAIN is this frame's eye, the eyes taking turns; under a menu it stays.
                        if cam_kind == 1 && cvr_afw_active() {
                            let menu = GetMenuRectMode() != 0 || GetMenuMode() != 0;
                            let afw_eye = if menu { (CyberpunkVR_MainIsRightEye != 0) as i32 } else { cvr_afw_frame_eye() };
                            CyberpunkVR_MainIsRightEye = afw_eye;
                            written_pose.afw_eye = afw_eye as i8;
                        }
                        let sgn0 = if cam_kind == 2 { 1.0f32 } else { -1.0 };
                        let sign = if CyberpunkVR_MainIsRightEye != 0 { -sgn0 } else { sgn0 };
                        for i in 0..3 {
                            p[i] = p[i].wrapping_add((r[i] * half * sign * 131072.0) as i32);
                        }
                        dirty = true;
                        diag!(CyberpunkVR_DebugIpdWorldWrites += 1);
                    }
                }
            }
            if ok && dirty {
                let mut position_written = true;
                for (i, v) in p.iter().enumerate() {
                    position_written &= write(pos_addr + i * 4, *v as u32);
                }
                if position_written && orientation_written && translation_label_known && cvr_camera_placed_pose_enabled() && (cam_kind == 1 || cam_kind == 2) {
                    cvr_camera_placed_pose_push(cam_kind as u32, &hq, &p, &written_pose);
                }
                if position_written && orientation_written && translation_label_known && written_pose_id != 0 && (1..=3).contains(&cam_kind) {
                    cvr_pose_publish_setup(owner + 0xE0, owner, if cam_kind == 2 { 2 } else { 1 }, written_pose_id, &written_pose, false);
                }
                if position_written && cam_kind == 1 && !cvr_cpp_device_cam_active() && g_bdActive.load(Ordering::Relaxed) == 0 {
                    cvr_camera_main_eye_centre_publish(&p, &centre_before_ipd, &body_base_before_vr);
                    if !g_isInVehicle && have_camera_owner && orientation_written && written_pose.valid && translation_label_known {
                        let stable = read_camera_owner_world(owner, player_now_reread()).is_some_and(|after| camera_owner_before.same_bytes(&after));
                        cvr_body_native_pair_publish(
                            centre_before_ipd.as_ptr(),
                            body_base_before_vr.as_ptr(),
                            camera_owner_before.position.as_ptr(),
                            hq.as_ptr(),
                            camera_owner_before.rotation.as_ptr(),
                            camera_owner_before.owner,
                            written_pose.origin_serial,
                            stable,
                        );
                    }
                }
            }
            // AND THE SAME COMPOSITION INTO THE LENS, on the second eye's dispatch, once per frame.
            if cam_kind == 2 && g_lensComp.load(Ordering::Relaxed) != 0 && cvr_cpp_device_cam_active() && core::ptr::read_volatile(&raw const g_headQuatValid) != 0 {
                let lq = g_headQuatComposed;
                cvr_push_lens_head_transform(lq.as_ptr());
            }
            // The census snapshot AFTER the writes, with the head delta as it stood for this view.
            let mut census = CENSUS.lock().unwrap_or_else(|q| q.into_inner());
            for i in 0..3 {
                let hd = g_headDeltaFP[i].load(Ordering::Relaxed);
                if ok && cam_kind == 1 {
                    census[0][i] = p[i];
                    census[2][i] = hd;
                }
                if ok && cam_kind == 2 {
                    census[1][i] = p[i];
                    census[3][i] = hd;
                }
            }
        }

        // The posA/posB pair (CyberpunkVR_IpdInPosAB, off by default): the half IPD along the camera's right.
        let mut shift = if CyberpunkVR_IpdInPosAB != 0 { cvr_camera_desired_half_ipd() } else { 0.0 };
        if shift != 0.0 && plausible_q(&quat) && plausible_span(&pos_a, &pos_b) {
            let mut right = [0.0f32; 3];
            cvr_camera_right_vector_from_quaternion(quat.as_ptr(), right.as_mut_ptr());
            if cvr_camera_plausible_unit_vector3(right.as_ptr()) {
                shift = if cam_kind == 2 { shift } else { -shift }; // 1 = MAIN/left, 2 = VRCAM/right
                let (dx, dy, dz) = (right[0] * shift, right[1] * shift, right[2] * shift);
                write(state + 0x10, pos_a[0] + dx);
                write(state + 0x14, pos_a[1] + dy);
                write(state + 0x18, pos_a[2] + dz);
                write(state + 0x20, pos_b[0] + dx);
                write(state + 0x24, pos_b[1] + dy);
                write(state + 0x28, pos_b[2] + dz);
            }
        }
    }
}

/// cvr::roomscale::CameraConsumed, as the camera link's pair.
unsafe fn consumed_at(origin: u64) -> Vec2 {
    // SAFETY: the room-scale state.
    let c = unsafe { cvr_roomscale_camera_consumed_at(origin) };
    Vec2 { x: c.x, y: c.y }
}

/// ReadCameraOwnerWorld's default argument, evaluated at the call (the player can change between the two reads).
fn player_now_reread() -> usize {
    cvr_roomscale_player_identity()
}

/// VrCore.cpp IsPlausibleCameraSpan: two plausible positions less than five metres apart.
fn plausible_span(a: &[f32; 4], b: &[f32; 4]) -> bool {
    // SAFETY: two float vectors.
    if !unsafe { cvr_camera_plausible_position_vec4(a.as_ptr()) && cvr_camera_plausible_position_vec4(b.as_ptr()) } {
        return false;
    }
    let (dx, dy, dz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    dx * dx + dy * dy + dz * dz < 25.0
}

/// The census line: what the two eyes really differ by (MAIN's position from the located buffer in a braindance).
unsafe fn census_log() {
    // SAFETY: the plugin's globals.
    unsafe {
        let census = *CENSUS.lock().unwrap_or_else(|q| q.into_inner());
        let k = 1.0f32 / 131072.0;
        let bd_located = g_bdActive.load(Ordering::Relaxed) != 0 && g_bdScenePoseValid.load(Ordering::Acquire) != 0 && g_locatePosValid.load(Ordering::Acquire) != 0;
        let mut main = census[0];
        if bd_located {
            for (i, v) in main.iter_mut().enumerate() {
                *v = g_locatePosFP[i].load(Ordering::Relaxed);
            }
        }
        let no_subtract = bd_located || CyberpunkVR_VrcamPosFromMain == 3;
        let sub = if no_subtract { [0; 3] } else { census[3] };
        let vr = census[1];
        let r = [(vr[0] - main[0] - sub[0]) as f32 * k, (vr[1] - main[1] - sub[1]) as f32 * k, (vr[2] - main[2] - sub[2]) as f32 * k];
        let hd = [g_headDeltaFP[0].load(Ordering::Relaxed), g_headDeltaFP[1].load(Ordering::Relaxed), g_headDeltaFP[2].load(Ordering::Relaxed)];
        super::env::log(&format!(
            "PatchCamera: main={} | mainPos[{}]=({:.3},{:.3},{:.3}) vrcamPos=({:.3},{:.3},{:.3}) sep=({:.3},{:.3},{:.3}) headDelta=({:.3},{:.3},{:.3}) | resid=({:.4},{:.4},{:.4}) horiz={:.4} ipd={:.4}\n",
            CyberpunkVR_DebugPatchCamMain,
            if bd_located { "located" } else { "component" },
            main[0] as f32 * k,
            main[1] as f32 * k,
            main[2] as f32 * k,
            vr[0] as f32 * k,
            vr[1] as f32 * k,
            vr[2] as f32 * k,
            (vr[0] - main[0]) as f32 * k,
            (vr[1] - main[1]) as f32 * k,
            (vr[2] - main[2]) as f32 * k,
            hd[0] as f32 * k,
            hd[1] as f32 * k,
            hd[2] as f32 * k,
            r[0],
            r[1],
            r[2],
            sqrt(r[0] * r[0] + r[1] * r[1]),
            2.0 * cvr_camera_desired_half_ipd()
        ));
    }
}

// ---- the install: two sites, the fast reject, the slow path ----
pub struct PatchSite {
    name: &'static str,
    pattern: &'static [u8],
    mask: &'static [u8],
    replace: usize,
    store_at: usize,
    owner: u8,
    tail: &'static [u8],
}
const SITES: [PatchSite; 2] = [
    PatchSite {
        name: "A",
        pattern: b"\x0F\x11\x83\xF0\x00\x00\x00\x80\xBB\xB1\x00\x00\x00\x00\x75",
        mask: b"xxxxxxxxxxxxxxx",
        replace: 14,
        store_at: 0,
        owner: 3,
        tail: &[0x80, 0xBB, 0xB1, 0x00, 0x00, 0x00, 0x00], // cmp byte [rbx+0B1h],0
    },
    PatchSite {
        name: "B",
        pattern: b"\x48\x8B\xCF\x0F\x11\x87\xF0\x00\x00\x00\xE8",
        mask: b"xxxxxxxxxxx",
        replace: 10,
        store_at: 3,
        owner: 7,
        tail: &[0x48, 0x8B, 0xCF], // mov rcx,rdi
    },
];

/// The trampoline of one site (src/Hooks/PatchCamera.cpp InstallPatchCameraSite, byte for byte): the engine's own
/// store first and unconditional; the fast reject on the latched owners; the telemetry and the callback on the slow
/// path; the relocated tail last before either jump back. None when a rel8 is out of range.
pub fn stub(at: usize, found: usize, site: &PatchSite, store: &[u8; 7], fast: u64, telemetry: u64, callback: u64) -> Option<(cvr_engine::trampoline::Code, usize)> {
    use cvr_engine::trampoline::Code;
    let r = site.owner;
    let mut c = Code::new(at);
    c.bytes(store); // movups [owner+F0h],xmm0
    c.bytes(&[0x50]).mov_rax(fast); // push rax / mov rax,&g_patchFast
    let mut slow_jz = Vec::new();
    let base = at;
    let mark = |c: &mut Code, jz: &mut Vec<usize>| {
        c.bytes(&[0x74, 0x00]);
        jz.push(c.here() - 1 - base);
    };
    c.bytes(&[0x83, 0x78, 0x18, 0x00]); // cmp dword [rax+18h],0
    mark(&mut c, &mut slow_jz);
    c.bytes(&[0x48, 0x3B, r << 3]); // cmp owner,[rax]
    mark(&mut c, &mut slow_jz);
    c.bytes(&[0x48, 0x3B, 0x40 | r << 3, 0x08]); // cmp owner,[rax+8]
    mark(&mut c, &mut slow_jz);
    c.bytes(&[0x48, 0x3B, 0x40 | r << 3, 0x10]); // cmp owner,[rax+10h]
    mark(&mut c, &mut slow_jz);
    c.bytes(&[0x58]).bytes(site.tail).jmp(found + site.replace); // pop rax / tail / out
    let slow_at = c.here() - base;
    let mut bytes = c.as_slice().to_vec();
    for &j in &slow_jz {
        let rel = slow_at as i64 - (j as i64 + 1);
        if !(0..=127).contains(&rel) {
            return None;
        }
        bytes[j] = rel as u8;
    }
    let mut c = Code::new(at);
    c.bytes(&bytes);
    // slow path: the telemetry (inc [patchHits], state, owner, xmm0), then the callback
    c.mov_rax(telemetry).bytes(&[0xFF, 0x00]); // inc dword [rax]
    c.bytes(&[0x51, 0x48, 0x8D, 0x88 | r, 0xF0, 0x00, 0x00, 0x00]); // push rcx / lea rcx,[owner+F0h]
    c.bytes(&[0x48, 0x89, 0x48, 0x08, 0x59]); // mov [rax+8],rcx / pop rcx
    c.bytes(&[0x48, 0x89, 0x40 | r << 3, 0x10]); // mov [rax+10h],owner
    c.bytes(&[0x0F, 0x11, 0x40, 0x18, 0x58]); // movups [rax+18h],xmm0 / pop rax
    let mut arg = vec![0x48, 0x8D, 0x88 | r, 0xF0, 0x00, 0x00, 0x00]; // lea rcx,[owner+F0h]
    arg.extend_from_slice(&[0x48, 0x89, 0xC2 | r << 3]); // mov rdx,owner
    c.volatile_call(4, &arg, callback);
    c.bytes(site.tail).jmp(found + site.replace);
    Some((c, slow_at))
}

/// Installed from src/Hooks/PatchCamera.cpp's registration (Boot, 12). Both sites or neither.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_patch_camera_install() -> bool {
    use cvr_engine::trampoline::{allocate, patch_jump};
    let mut found = [0usize; 2];
    for (i, site) in SITES.iter().enumerate() {
        match find_pattern("Cyberpunk2077.exe", site.pattern, site.mask) {
            Some(f) => found[i] = f,
            None => {
                super::env::log(&format!("PatchCamera hook: site {} not found -- NOT installed\n", site.name));
                return false;
            }
        }
    }
    // SAFETY: C++ services: the fast-path block and the telemetry line, both alive for the process.
    let (fast, telemetry) = unsafe { (cvr_cpp_patch_fast_path() as u64, cvr_cpp_telemetry_patch_hits() as u64) };
    if fast == 0 || telemetry == 0 {
        return false;
    }
    for (i, site) in SITES.iter().enumerate() {
        let mut store = [0u8; 7];
        if !read_bytes(found[i] + site.store_at, &mut store) {
            return false;
        }
        let Some(tramp) = allocate(found[i], 512) else { return false };
        let Some((code, slow_at)) = stub(tramp, found[i], site, &store, fast, telemetry, OnPatchCameraCallback as *const () as u64) else {
            super::env::log("PatchCamera hook: fast-path branch out of rel8 range -- NOT installed\n");
            return false;
        };
        // SAFETY: fresh arena memory, then a site whose bytes the stub reproduces.
        unsafe {
            code.commit();
            patch_jump(found[i], site.replace, tramp);
        }
        super::env::log(&format!(
            "PatchCamera hook {}: site {:#x}, trampoline {:#x} ({} bytes, slow path at +0x{:X}), fast-path block {:#x} [owner+0x00, armed+0x18].\n",
            site.name,
            found[i],
            tramp,
            code.len(),
            slow_at,
            fast
        ));
    }
    let _ = core::ptr::null::<c_void>();
    true
}
