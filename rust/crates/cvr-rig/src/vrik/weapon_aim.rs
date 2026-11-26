//! WeaponAim (formerly src/Hooks/WeaponAim.cpp): the native hitscan from the muzzle, and the two engine functions the
//! camera code needs this family to own.
//!
//!   PhysicalRay   gameEffectObjectProvider_PhysicalRay::Execute's origin and forward: bullets from the muzzle, along
//!                 the barrel, the sight line with the eye on a sight, the left gun of two, the hitscan hand recoil, and
//!                 the cast ray published back into the shot's shared data. The shot path itself.
//!   0x21297C      a transform copy the camera pose bookkeeping (Camera/PoseIdentity) follows across.
//!   0x19C75C      the targeting update: re-aimed at the device camera the player looks through, and the head gaze
//!                 centred on the binocular view, after the engine's own update.
//!
//! The long notes on every decision (whose ray it is, the check order that once cost 50 fps near turrets, the cone
//! centre, one shot one impact) are at the C++ original's lines; this port keeps the decisions and their order.
use super::env::g_pSharedHands;
use super::pose_identity::{cvr_pose_capture, cvr_pose_transfer, PoseReceipt};
use super::roomscale_move::cvr_roomscale_player_identity;
use core::ffi::c_void;
use cvr_body::cmath::{atan2, hypot, sqrt};
use cvr_engine::code::{image_base, read, read_bytes};
use cvr_engine::hooks::{install_chain, Detour};
use cvr_engine::trampoline::{allocate, patch_call, write_code};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI8, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

const TARGETING_UPDATE: usize = 0x19C75C;
const POSE_COPY: usize = 0x21297C;
const PHYSICAL_RAY_EVAL: usize = 0x474BCC;
const PHYSICAL_RAY_ORIGIN_CALLSITE: usize = 0x9507B7;
const PHYSICAL_RAY_FORWARD_CALLSITE: usize = 0x9507EC;
const PLAYER_OWNED_WEAPON: u64 = 0x8CB4_C089_1BD2_55ED; // FNV1a64
const WEAPON_ITEM_RECORD: u64 = 0x8DBD_D925_7C33_612F; // FNV1a64
/// The blackboard's typed Vector4 setter, sub_140475078 (blackboard, &key, &value, flag); refused if a game update
/// moved it.
const SHARED_VECTOR4_SET: usize = 0x475078;
const SHARED_VECTOR4_SET_BYTES: [u8; 7] = [0x4C, 0x8B, 0xDC, 0x49, 0x89, 0x5B, 0x08];
const POSITION: u64 = 0x4CBF_3A26_FCA1_D74A; // FNV1a64("position")
const FORWARD: u64 = 0x1E65_942E_132F_4F7A; // FNV1a64("forward")
// Utils/SharedSlots.hpp
const K_DUAL_WIELD_ACTIVE: usize = 165;
const K_DUAL_WIELD_LEFT_FIRES: i32 = 2;
const K_DUAL_WIELD_RIGHT_FIRES: i32 = 4;
const K_LEFT_MUZZLE_POS: usize = 166;
const K_LEFT_MUZZLE_FWD: usize = 169;
const K_LEFT_MUZZLE_VALID: usize = 172;
// Camera/TakeoverTargeting.hpp
const TARGETING_CAMERA_TRANSFORM: usize = 0x31100;
const TARGETING_DEFAULT_POSITION: usize = 0x311F0;
const TARGETING_DEFAULT_FORWARD: usize = 0x31200;
const TARGETING_DEFAULT_ANGLES: usize = 0x31210;
const TARGETING_SOURCE_CACHE: usize = 0x30CC8;
const SOURCE_CACHE_FORWARD: usize = 0x280;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugWaInstalled: i32;
    static mut CyberpunkVR_GazeTargetState: usize;
    static mut CyberpunkVR_GazeTargetUpdates: u64;
    static CyberpunkVR_HitscanFromMuzzle: i32;
    static mut CyberpunkVR_DebugPhysicalRayPatched: i32;
    static CyberpunkVR_DebugPhysicalRayCalls: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayMine: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayOriginWrites: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayForwardWrites: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayConeRays: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayRecoil: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayForeign: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayLeftGun: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayStaleMuzzle: AtomicU64;
    static CyberpunkVR_DebugPhysicalRaySightLine: AtomicU64;
    static CyberpunkVR_DebugPhysicalRayShared: AtomicU64;
    static CyberpunkVR_DebugPhysicalRaySharedMissing: AtomicU64;
    static CyberpunkVR_DebugConeFirstRay: AtomicU64;
    static mut CyberpunkVR_DebugDualShot: [f32; 4];
    static mut CyberpunkVR_DebugDualShotRecord: [u32; 3];
    static mut CyberpunkVR_DebugPhysicalRaySightLift: [f32; 4];
    static mut CyberpunkVR_DebugPhysicalRayRawForward: [f32; 4];
    static mut CyberpunkVR_DebugPhysicalRayOutForward: [f32; 4];
    static mut CyberpunkVR_DebugPhysicalRayKeys: [u64; 64];
    static CyberpunkVR_ConeCentreFromCamera: i32;
    static CyberpunkVR_HitscanSharedRay: i32;
    static CyberpunkVR_DualWieldRecords: [u32; 2];
    static CyberpunkVR_SightLineOffset: [f32; 3];
    static CyberpunkVR_SightLineTick: u64;
    static CyberpunkVR_HitscanFromSightLine: i32;
    static g_lastLocateQuat: [f32; 4];
    static g_provMuzzleSeq: u32;
    static g_provRecoilSeqSeen: AtomicI32;
    fn RecoilOnShot();
    fn RecoilOnShotLeft();
    fn cvr_cpp_remote_cam_on() -> bool;
    fn cvr_cpp_takeover_entity_id() -> u64;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_vrui_captures_input() -> bool;
    fn cvr_cpp_gaze_tangent_y() -> f32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
/// A counter read from six threads is incremented from six threads (one PhysicalRay per pellet, each on its own
/// worker): an interlocked increment, only with diagnostics on.
fn count(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}
fn volatile<T: Copy>(p: *const T) -> T {
    // SAFETY: the plugin's own globals.
    unsafe { core::ptr::read_volatile(p) }
}

// ---- the original functions (MinHook trampolines and the evaluator), and the image the setter is checked in ----
static mut TARGETING_ORIG: *mut c_void = core::ptr::null_mut();
static mut POSE_COPY_ORIG: *mut c_void = core::ptr::null_mut();
static EVAL: AtomicUsize = AtomicUsize::new(0);
static EXE: AtomicUsize = AtomicUsize::new(0);
/// -1 unknown, 0 no, 1 yes: the setter's prologue, checked on its first use.
static SETTER_MATCHES: AtomicI8 = AtomicI8::new(-1);
static POSE_COPY_INSTALLED: AtomicBool = AtomicBool::new(false);
static TARGETING_INSTALLED: AtomicBool = AtomicBool::new(false);
type Fn4 = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;
type EvalFn = unsafe extern "C" fn(usize, usize, *mut f32) -> i8;

/// For tools/vrik_tests: the evaluator, the image the setter lives in, and the two originals the detours call.
#[no_mangle]
pub unsafe extern "C" fn cvr_weapon_aim_bind(eval: usize, exe: usize, pose_copy: usize, targeting: usize) {
    EVAL.store(eval, Ordering::Relaxed);
    EXE.store(exe, Ordering::Relaxed);
    SETTER_MATCHES.store(-1, Ordering::Relaxed);
    // SAFETY: the slots the detours read, set before any call.
    unsafe {
        POSE_COPY_ORIG = pose_copy as *mut c_void;
        TARGETING_ORIG = targeting as *mut c_void;
    }
}

/// Smart guns lock along the barrel while drawn (0 = along the head gaze, as before main's 2026-11-16).
#[no_mangle]
pub static CyberpunkVR_SmartLockBarrel: AtomicI32 = AtomicI32::new(1);
#[no_mangle]
pub static CyberpunkVR_SmartBarrelUpdates: AtomicU64 = AtomicU64::new(0);
/// Last frame's outcome: -20 the gate closed (a menu, the VR UI's input, no session), -21 no player, 0 no smart gun
/// drawn, 2 a bad muzzle quaternion, otherwise AimAlong.
#[no_mangle]
pub static CyberpunkVR_SmartBarrelState: AtomicI32 = AtomicI32::new(0);
const TARGETING_FRUSTUM: usize = 0x31130;
const TARGETING_FRUSTUM_PLANES: usize = 6;
/// cvr::camera::AimAlong.
const AIM_APPLIED: i32 = 1;
const AIM_BAD_INPUT: i32 = -1;
const AIM_NOT_PLAYER: i32 = -2;
const AIM_BAD_POSE: i32 = -3;
const AIM_SCRIPTED_RAY: i32 = -4;
const AIM_OPPOSITE: i32 = -5;
const AIM_BAD_FRUSTUM: i32 = -6;

// ---- TARGETING UPDATE @0x19C75C, per frame (this = rcx) ----
#[no_mangle]
pub unsafe extern "C" fn Hooked_WaTargetingUpdate(rcx: usize, rdx: usize, r8: usize, r9: usize) -> usize {
    // SAFETY (whole body): the engine's original, the targeting state it just refreshed, C++ services.
    unsafe {
        let original: Fn4 = core::mem::transmute::<*mut c_void, Fn4>(TARGETING_ORIG);
        let result = original(rcx, rdx, r8, r9);
        if cvr_cpp_remote_cam_on()
            && cvr_cpp_takeover_entity_id() != 0
            && cvr_cpp_device_cam_active()
            && cvr_cpp_menu_mode() == 0
            && !cvr_cpp_vrui_captures_input()
            && super::xr_calls::session_running()
        {
            let player = cvr_roomscale_player_identity();
            if player != 0 {
                if let Some(entity) = read::<u64>(player + 0x48) {
                    refresh_takeover_targeting(rcx, player, entity);
                }
            }
        }
        CyberpunkVR_SmartBarrelState.store(-20, Ordering::Relaxed);
        if cvr_cpp_menu_mode() == 0 && !cvr_cpp_vrui_captures_input() && super::xr_calls::session_running() {
            let player = cvr_roomscale_player_identity();
            CyberpunkVR_SmartBarrelState.store(-21, Ordering::Relaxed);
            if player != 0 {
                if let Some(entity) = read::<u64>(player + 0x48) {
                    // A DRAWN SMART GUN LOCKS ALONG ITS BARREL (aim_targeting_along). The flag and the muzzle both come
                    // from the weapon mechanic once a frame; a stale pair falls back to the head gaze, so holstering or
                    // a paused mechanic never leaves the cone on the gun.
                    let mut centered = false;
                    let mut smart_state = 0;
                    let smart_at = super::provider::SMART_WEAPON_STAMP_MS.load(Ordering::Relaxed);
                    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
                    if CyberpunkVR_SmartLockBarrel.load(Ordering::Relaxed) != 0 && smart_at != 0 && now.wrapping_sub(smart_at) < 250 {
                        let q = super::provider::muzzle_q();
                        smart_state = 2;
                        if q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3] > 0.5 {
                            let barrel = [2.0 * (q[0] * q[1] - q[2] * q[3]), 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]), 2.0 * (q[1] * q[2] + q[0] * q[3])];
                            let along = aim_targeting_along(rcx, player, entity, &barrel);
                            smart_state = along;
                            centered = along == AIM_APPLIED;
                            if centered {
                                CyberpunkVR_SmartBarrelUpdates.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    CyberpunkVR_SmartBarrelState.store(smart_state, Ordering::Relaxed);
                    if !centered {
                        centered = center_head_gaze(rcx, player, entity, cvr_cpp_gaze_tangent_y());
                    }
                    if diagnostics() {
                        CyberpunkVR_GazeTargetState = rcx;
                        CyberpunkVR_GazeTargetUpdates += centered as u64;
                    }
                }
            }
        }
        result
    }
}
unsafe fn rd<T: Copy>(at: usize) -> T {
    // SAFETY: the caller's live object (the C++ memcpy's from it unguarded).
    unsafe { core::ptr::read_unaligned(at as *const T) }
}
unsafe fn wr<T: Copy>(at: usize, v: T) {
    // SAFETY: as rd.
    unsafe { core::ptr::write_unaligned(at as *mut T, v) }
}
const DEGREES: f32 = 180.0 / core::f32::consts::PI;
/// The targeting state's owner (+0), its entity (+8) and its active flag (+24) name this player.
unsafe fn targeting_owned(state: usize, player: usize, entity: u64) -> bool {
    // SAFETY: the targeting state.
    unsafe { rd::<usize>(state) == player && rd::<u64>(state + 8) == entity && rd::<u32>(state + 24) != 0 }
}
/// Camera/TakeoverTargeting.hpp IsSourceCacheForward.
fn is_source_cache_forward(cached: &[f32; 4], f: &[f32; 3], w: f32) -> bool {
    if cached.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let length = cached[0] * cached[0] + cached[1] * cached[1] + cached[2] * cached[2];
    cached[3] == w && (length - 1.0).abs() < 0.01 && cached[0] * f[0] + cached[1] * f[1] + cached[2] * f[2] > 0.5
}
/// Camera/TakeoverTargeting.hpp CenterHeadGaze: re-aim the native head-gaze cache without moving its origin.
unsafe fn center_head_gaze(state: usize, player: usize, entity: u64, tangent_y: f32) -> bool {
    if state == 0 || player == 0 || entity == 0 || !tangent_y.is_finite() || tangent_y.abs() > 1.0 || tangent_y == 0.0 {
        return false;
    }
    // SAFETY (whole body): the targeting state the original just refreshed.
    unsafe {
        if !targeting_owned(state, player, entity) {
            return false;
        }
        let pose: [f32; 8] = rd(state + TARGETING_CAMERA_TRANSFORM);
        let old: [f32; 4] = rd(state + TARGETING_DEFAULT_FORWARD);
        if pose.iter().any(|v| !v.is_finite()) || old.iter().any(|v| !v.is_finite()) {
            return false;
        }
        let n = pose[4] * pose[4] + pose[5] * pose[5] + pose[6] * pose[6] + pose[7] * pose[7];
        if n < 0.5 || n > 1.5 {
            return false;
        }
        let inv = 1.0 / sqrt(n);
        let (x, y, z, w) = (pose[4] * inv, pose[5] * inv, pose[6] * inv, pose[7] * inv);
        let f = [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)];
        if old[0] * f[0] + old[1] * f[1] + old[2] * f[2] < 0.98 {
            return false;
        }
        let up = [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)];
        let length = sqrt(1.0 + tangent_y * tangent_y);
        let forward = [(f[0] + tangent_y * up[0]) / length, (f[1] + tangent_y * up[1]) / length, (f[2] + tangent_y * up[2]) / length, old[3]];
        let angles = [0.0f32, -atan2(forward[2], hypot(forward[0], forward[1])) * DEGREES, atan2(-forward[0], forward[1]) * DEGREES];
        wr(state + TARGETING_DEFAULT_FORWARD, forward);
        wr(state + TARGETING_DEFAULT_ANGLES, angles);
        let source_cache: usize = rd(state + TARGETING_SOURCE_CACHE);
        if source_cache != 0 {
            let cached_at = source_cache + SOURCE_CACHE_FORWARD;
            let cached: [f32; 4] = rd(cached_at);
            if is_source_cache_forward(&cached, &f, old[3]) {
                wr(cached_at, forward);
            }
        }
        true
    }
}
/// Camera/TakeoverTargeting.hpp AimTargetingAlong (main's 7b830fd6c): smart guns lock onto targets in a cone around the
/// targeting system's default ray (broadphase stores each target's yaw and pitch against the default angles, +264FD4;
/// the smart handler scores and locks by those, +9115E8). In flat play that ray is the camera, which is also the gun; in
/// VR they are apart, so a smart gun locked onto whatever the head looked at. While one is drawn, the default ray and the
/// targeting frustum point down the barrel. Only directions move: the origin stays the camera's. The frustum turns by the
/// shortest arc from the camera ray to the barrel about the camera position, so the targets the barrel points at
/// survive broadphase even when the head looks elsewhere. An AimAlong code.
unsafe fn aim_targeting_along(state: usize, player: usize, entity: u64, barrel: &[f32; 3]) -> i32 {
    if state == 0 || player == 0 || entity == 0 {
        return AIM_BAD_INPUT;
    }
    if barrel.iter().any(|v| !v.is_finite()) {
        return AIM_BAD_INPUT;
    }
    let length = sqrt(barrel[0] * barrel[0] + barrel[1] * barrel[1] + barrel[2] * barrel[2]);
    if !(0.5..=1.5).contains(&length) {
        return AIM_BAD_INPUT;
    }
    // SAFETY (whole body): the targeting state the original just refreshed (the C++ memcpy's it unguarded).
    unsafe {
        if !targeting_owned(state, player, entity) {
            return AIM_NOT_PLAYER;
        }
        let pose: [f32; 8] = rd(state + TARGETING_CAMERA_TRANSFORM);
        let old: [f32; 4] = rd(state + TARGETING_DEFAULT_FORWARD);
        if pose.iter().any(|v| !v.is_finite()) || old.iter().any(|v| !v.is_finite()) {
            return AIM_BAD_POSE;
        }
        let n = pose[4] * pose[4] + pose[5] * pose[5] + pose[6] * pose[6] + pose[7] * pose[7];
        if !(0.5..=1.5).contains(&n) {
            return AIM_BAD_POSE;
        }
        let inv = 1.0 / sqrt(n);
        let (x, y, z, w) = (pose[4] * inv, pose[5] * inv, pose[6] * inv, pose[7] * inv);
        let f = [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)];
        // A non-camera ray (a scripted aiming override) is left alone, exactly as center_head_gaze does.
        if old[0] * f[0] + old[1] * f[1] + old[2] * f[2] < 0.98 {
            return AIM_SCRIPTED_RAY;
        }
        let b = [barrel[0] / length, barrel[1] / length, barrel[2] / length];
        // The shortest arc camera ray -> barrel: q = (f x b, 1 + f.b), normalised.
        let dot = f[0] * b[0] + f[1] * b[1] + f[2] * b[2];
        if dot < -0.95 {
            return AIM_OPPOSITE;
        }
        let mut q = [f[1] * b[2] - f[2] * b[1], f[2] * b[0] - f[0] * b[2], f[0] * b[1] - f[1] * b[0], 1.0 + dot];
        let qn = 1.0 / sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
        for v in q.iter_mut() {
            *v *= qn;
        }
        let rotate = |v: &[f32; 4]| -> [f32; 3] {
            let t = [2.0 * (q[1] * v[2] - q[2] * v[1]), 2.0 * (q[2] * v[0] - q[0] * v[2]), 2.0 * (q[0] * v[1] - q[1] * v[0])];
            [
                v[0] + q[3] * t[0] + q[1] * t[2] - q[2] * t[1],
                v[1] + q[3] * t[1] + q[2] * t[0] - q[0] * t[2],
                v[2] + q[3] * t[2] + q[0] * t[1] - q[1] * t[0],
            ]
        };
        let mut planes: [[f32; 4]; TARGETING_FRUSTUM_PLANES] = rd(state + TARGETING_FRUSTUM);
        if planes.iter().flatten().any(|v| !v.is_finite()) {
            return AIM_BAD_FRUSTUM;
        }
        // n' = R n; the plane keeps passing through the camera position p: d' = d + n.p - n'.p.
        let p = &pose;
        for plane in planes.iter_mut() {
            let turned = rotate(plane);
            plane[3] += plane[0] * p[0] + plane[1] * p[1] + plane[2] * p[2] - (turned[0] * p[0] + turned[1] * p[1] + turned[2] * p[2]);
            plane[0] = turned[0];
            plane[1] = turned[1];
            plane[2] = turned[2];
        }
        let forward = [b[0], b[1], b[2], old[3]];
        let angles = [0.0f32, -atan2(forward[2], hypot(forward[0], forward[1])) * DEGREES, atan2(-forward[0], forward[1]) * DEGREES];
        wr(state + TARGETING_FRUSTUM, planes);
        wr(state + TARGETING_DEFAULT_FORWARD, forward);
        wr(state + TARGETING_DEFAULT_ANGLES, angles);
        let source_cache: usize = rd(state + TARGETING_SOURCE_CACHE);
        if source_cache != 0 {
            let cached_at = source_cache + SOURCE_CACHE_FORWARD;
            let cached: [f32; 4] = rd(cached_at);
            // Validated against the camera ray it still holds; written with the barrel.
            if is_source_cache_forward(&cached, &f, old[3]) {
                wr(cached_at, forward);
            }
        }
        AIM_APPLIED
    }
}
/// Camera/TakeoverTargeting.hpp RefreshTakeoverTargeting: the default ray from the active (device) camera.
unsafe fn refresh_takeover_targeting(state: usize, player: usize, entity: u64) -> bool {
    if state == 0 || player == 0 || entity == 0 {
        return false;
    }
    // SAFETY (whole body): as center_head_gaze.
    unsafe {
        if !targeting_owned(state, player, entity) {
            return false;
        }
        let pose: [f32; 8] = rd(state + TARGETING_CAMERA_TRANSFORM);
        if pose.iter().any(|v| !v.is_finite()) {
            return false;
        }
        let n = pose[4] * pose[4] + pose[5] * pose[5] + pose[6] * pose[6] + pose[7] * pose[7];
        if n < 0.5 || n > 1.5 {
            return false;
        }
        let inv = 1.0 / sqrt(n);
        let (x, y, z, w) = (pose[4] * inv, pose[5] * inv, pose[6] * inv, pose[7] * inv);
        let position = [pose[0], pose[1], pose[2], 1.0f32];
        let forward = [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w), 1.0f32];
        let angles = [0.0f32, -atan2(forward[2], hypot(forward[0], forward[1])) * DEGREES, atan2(-forward[0], forward[1]) * DEGREES];
        wr(state + TARGETING_DEFAULT_POSITION, position);
        wr(state + TARGETING_DEFAULT_FORWARD, forward);
        wr(state + TARGETING_DEFAULT_ANGLES, angles);
        true
    }
}

// ---- POSE COPY @0x21297C (dest = rcx, source = rdx) ----
#[no_mangle]
pub unsafe extern "C" fn Hooked_WaPoseCopy(rcx: usize, rdx: usize, r8: usize, r9: usize) -> usize {
    let mut source = PoseReceipt::default();
    // SAFETY: the pose ledger and the engine's original copy.
    unsafe {
        cvr_pose_capture(rdx, &mut source);
        let original: Fn4 = core::mem::transmute::<*mut c_void, Fn4>(POSE_COPY_ORIG);
        let result = original(rcx, rdx, r8, r9);
        cvr_pose_transfer(&source, rcx);
        result
    }
}

// ---- NATIVE HITSCAN, FROM THE MUZZLE ----
/// The barrel as the rest of the port publishes it: the muzzle slot's world position (shared[200..202], valid at
/// [203]) and the muzzle transform's world +Y axis (shared[24..26], valid at [27]).
unsafe fn muzzle() -> Option<([f32; 3], [f32; 3])> {
    // SAFETY: the shared block.
    unsafe {
        let sh = g_pSharedHands;
        if sh.is_null() || *sh.add(203) < 0.5 || *sh.add(27) < 0.5 {
            return None;
        }
        let pos = [*sh.add(200), *sh.add(201), *sh.add(202)];
        let (fx, fy, fz) = (*sh.add(24), *sh.add(25), *sh.add(26));
        let l2 = fx * fx + fy * fy + fz * fz;
        if l2 < 0.25 {
            return None;
        }
        let inv = 1.0 / sqrt(l2);
        Some((pos, [fx * inv, fy * inv, fz * inv]))
    }
}
#[derive(Clone, Copy)]
struct Ray {
    mine: bool,
    muzzle_seq: u32,
    context: usize,
    origin: [f32; 3],
    muzzle_forward: [f32; 3],
    cone_centre: [f32; 3],
    have_centre: bool,
}
thread_local! {
    static RAY: Cell<Ray> = const {
        Cell::new(Ray {
            mine: false,
            muzzle_seq: u32::MAX,
            context: 0,
            origin: [0.0; 3],
            muzzle_forward: [0.0; 3],
            cone_centre: [0.0; 3],
            have_centre: false,
        })
    };
}
/// The left gun's round latch while two guns are in hand.
static RECOIL_SEEN_LEFT: AtomicI32 = AtomicI32::new(-1);
/// FNV1a64("attackData"): the shot's AttackData, its weapon the firing entity (EffectPreAction_PreAttack writes it
/// from EffectScriptContext.GetWeapon before PhysicalRay runs).
const ATTACK_DATA: u64 = red4ext_abi::fnv::fnv1a64(b"attackData");
/// The RTTI type a typed AttackData handle carries in the shared data (a typed handle, not a pointer guessed at).
fn attack_data_type() -> usize {
    static TYPE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *TYPE.get_or_init(|| {
        red4ext_abi::rtti::RttiSystem::get()
            .and_then(|r| r.get_type(red4ext_abi::cname::CName::hashed("handle:gamedamageAttackData")))
            .map_or(0, |t| t as *const _ as usize)
    })
}
/// The shot's first ray, keyed by the muzzle sequence (the cone centre).
static CONE: Mutex<(u32, [f32; 3])> = Mutex::new((u32::MAX, [0.0; 3]));

/// Range and alignment only (ProvPlausiblePtr in src/Natives/OrientationProvider.cpp).
fn plausible_ptr(p: usize) -> bool {
    (0x10000..0x0000_7FFF_FFFF_FFFF).contains(&p) && p & 7 == 0
}
/// The shot's shared-data container: its keys, count and values (blackboard vtable +0x108's fields).
fn container(bb: usize) -> Option<(usize, u32, usize)> {
    Some((read::<usize>(bb + 0x48)?, read::<u32>(bb + 0x54)?, read::<usize>(bb + 0x58)?))
}
/// WHOSE RAY IS THIS: the effect's own playerOwnedWeapon flag, next to weaponItemRecord.
fn player_owned_weapon(bb: usize) -> bool {
    if bb == 0 || !plausible_ptr(bb) {
        return false;
    }
    let Some((keys, count, vals)) = container(bb) else { return false };
    if keys == 0 || vals == 0 || count == 0 || count > 256 || !plausible_ptr(keys) || !plausible_ptr(vals) {
        return false;
    }
    let (mut from_weapon, mut player_owned) = (false, false);
    for i in 0..count as usize {
        let Some(key) = read::<u64>(keys + i * 8) else { return false };
        if key != PLAYER_OWNED_WEAPON && key != WEAPON_ITEM_RECORD {
            continue;
        }
        let entry = vals + i * 24;
        let Some(tag) = read::<u64>(entry) else { return false };
        if tag & !1 == 0 {
            continue; // empty slot
        }
        if key == WEAPON_ITEM_RECORD {
            from_weapon = true;
        } else {
            let Some(v) = read::<u8>(entry + 8) else { return false };
            player_owned = v != 0;
        }
    }
    from_weapon && player_owned
}
/// WHICH GUN FIRED, WITH TWO IN HAND (main's 66872b5a7): the shot's AttackData weapon against the two published guns,
/// then its weaponItemRecord against the two records (two guns of one model have the same record); unknown is the right
/// gun. The triggers and alternation no longer decide: Hurricane fires after the triggers are released.
unsafe fn left_gun_fired(bb: usize) -> Option<([f32; 3], [f32; 3])> {
    // SAFETY (whole body): the shared block, the blackboard through SEH reads, the exported diagnostics.
    unsafe {
        let sh = g_pSharedHands;
        if sh.is_null() || *sh.add(K_DUAL_WIELD_ACTIVE) < 0.5 || *sh.add(K_LEFT_MUZZLE_VALID) < 0.5 {
            return None;
        }
        let (keys, count, vals) = container(bb)?;
        if keys == 0 || vals == 0 || count == 0 || count > 256 {
            return None;
        }
        let mut record = 0u32;
        let mut side: Option<bool> = None;
        for i in 0..count as usize {
            let key = read::<u64>(keys + i * 8)?;
            if key != WEAPON_ITEM_RECORD && key != ATTACK_DATA {
                continue;
            }
            let entry = vals + i * 24;
            let tagged = read::<u64>(entry)?;
            if key == ATTACK_DATA {
                // a Variant: its type (low bit: the value inline), then the value or a pointer to it
                let ty = (tagged & !1) as usize;
                let expected = attack_data_type();
                if ty != 0 && ty == expected {
                    let data = if tagged & 1 != 0 { entry + 8 } else { read::<usize>(entry + 8)? };
                    let attack = read::<usize>(data)?;
                    if plausible_ptr(attack) {
                        // AttackData.weapon, a weak handle at +0x68: its instance
                        side = super::provider::shot_weapon_side(read::<usize>(attack + 0x68)?);
                    }
                }
                continue;
            }
            let instance = read::<usize>(entry + 8)?;
            if tagged & !1 != 0 && plausible_ptr(instance) {
                record = read::<u32>(instance + 0x40)?;
            }
        }
        let records = volatile(&raw const CyberpunkVR_DualWieldRecords);
        let (left_record, right_record) = (records[0], records[1]);
        let hands = *sh.add(K_DUAL_WIELD_ACTIVE) as i32;
        let left_trigger = hands & K_DUAL_WIELD_LEFT_FIRES != 0;
        let right_trigger = hands & K_DUAL_WIELD_RIGHT_FIRES != 0;
        let (mut left, mut path) = (false, 0);
        if let Some(l) = side {
            (left, path) = (l, 4);
        } else if record != 0 && left_record != 0 && right_record != 0 && left_record != right_record {
            left = record == left_record;
            path = 1;
        }
        if diagnostics() {
            CyberpunkVR_DebugDualShot =
                [if left { 1.0 } else { 0.0 }, path as f32, if left_trigger { 1.0 } else { 0.0 }, if right_trigger { 1.0 } else { 0.0 }];
            CyberpunkVR_DebugDualShotRecord = [record, left_record, right_record];
        }
        if !left {
            return None;
        }
        let m = [*sh.add(K_LEFT_MUZZLE_POS), *sh.add(K_LEFT_MUZZLE_POS + 1), *sh.add(K_LEFT_MUZZLE_POS + 2)];
        let f = [*sh.add(K_LEFT_MUZZLE_FWD), *sh.add(K_LEFT_MUZZLE_FWD + 1), *sh.add(K_LEFT_MUZZLE_FWD + 2)];
        let l2 = f[0] * f[0] + f[1] * f[1] + f[2] * f[2];
        if !(l2 > 0.25) {
            return None;
        }
        let inv = 1.0 / sqrt(l2);
        Some((m, [f[0] * inv, f[1] * inv, f[2] * inv]))
    }
}
/// Hooks/SightLine.hpp SightLineOffset: the bore-to-sight-line offset for a shot along `forward`.
pub(crate) unsafe fn sight_line_offset(forward: &[f32; 3]) -> Option<[f32; 3]> {
    // SAFETY: the exported offset and its tick.
    unsafe {
        if CyberpunkVR_HitscanFromSightLine == 0 {
            return None;
        }
        let tick = volatile(&raw const CyberpunkVR_SightLineTick);
        if tick == 0 || GetTickCount64().wrapping_sub(tick) > 250 {
            return None;
        }
        let o = volatile(&raw const CyberpunkVR_SightLineOffset);
        let (mut x, mut y, mut z) = (o[0], o[1], o[2]);
        let fl2 = forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2];
        if !fl2.is_finite() || fl2 < 0.25 {
            return None;
        }
        let along = (x * forward[0] + y * forward[1] + z * forward[2]) / fl2;
        x -= along * forward[0];
        y -= along * forward[1];
        z -= along * forward[2];
        let l2 = x * x + y * y + z * z;
        if !l2.is_finite() || l2 > 0.3 * 0.3 {
            return None;
        }
        Some([x, y, z])
    }
}
/// The cone centre: the game camera's forward as the locate site published it (game forward is +Y).
fn camera_forward(out: &mut [f32; 3]) -> bool {
    let q = volatile(&raw const g_lastLocateQuat);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let n = x * x + y * y + z * z + w * w;
    if !n.is_finite() || n < 0.98 || n > 1.02 {
        return false;
    }
    out[0] = 2.0 * (x * y - z * w);
    out[1] = 1.0 - 2.0 * (x * x + z * z);
    out[2] = 2.0 * (y * z + x * w);
    out.iter().all(|v| v.is_finite())
}
fn rotation_between(from: &[f32; 3], to: &[f32; 3]) -> Option<[f32; 4]> {
    let (mut ax, mut ay, mut az) = (from[0], from[1], from[2]);
    let (mut bx, mut by, mut bz) = (to[0], to[1], to[2]);
    let al2 = ax * ax + ay * ay + az * az;
    let bl2 = bx * bx + by * by + bz * bz;
    if al2 < 1e-6 || bl2 < 1e-6 {
        return None;
    }
    let ai = 1.0 / sqrt(al2);
    let bi = 1.0 / sqrt(bl2);
    ax *= ai;
    ay *= ai;
    az *= ai;
    bx *= bi;
    by *= bi;
    bz *= bi;
    // std::clamp(v, -1, 1)
    let d = ax * bx + ay * by + az * bz;
    let dot = if d < -1.0 {
        -1.0
    } else if 1.0 < d {
        1.0
    } else {
        d
    };
    if dot > 0.999_999 {
        return Some([0.0, 0.0, 0.0, 1.0]);
    }
    if dot < -0.999_999 {
        let rx = if ax.abs() < 0.9 { 1.0f32 } else { 0.0 };
        let ry = if rx == 0.0 { 1.0f32 } else { 0.0 };
        let rz = 0.0f32;
        let qx = ay * rz - az * ry;
        let qy = az * rx - ax * rz;
        let qz = ax * ry - ay * rx;
        let qi = 1.0 / sqrt(qx * qx + qy * qy + qz * qz);
        return Some([qx * qi, qy * qi, qz * qi, 0.0]);
    }
    let mut q = [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx, 1.0 + dot];
    let qi = 1.0 / sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]);
    for v in q.iter_mut() {
        *v *= qi;
    }
    Some(q)
}
fn rotate_vector(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + (q[1] * tz - q[2] * ty), v[1] + q[3] * ty + (q[2] * tx - q[0] * tz), v[2] + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
/// Whether the shot's container already holds both keys (nothing is ever ADDED to it).
unsafe fn shared_has_position_forward(bb: usize) -> bool {
    let Some((keys, count, _)) = container(bb) else { return false };
    if keys == 0 || count == 0 || count > 256 || !plausible_ptr(keys) {
        return false;
    }
    let mut list = [0u64; 256];
    for (i, k) in list.iter_mut().take(count as usize).enumerate() {
        let Some(v) = read::<u64>(keys + i * 8) else { return false };
        *k = v;
    }
    if diagnostics() {
        let n = count.min(63) as usize;
        // SAFETY: the exported key census.
        unsafe {
            for i in 0..n {
                CyberpunkVR_DebugPhysicalRayKeys[1 + i] = list[i];
            }
            CyberpunkVR_DebugPhysicalRayKeys[0] = count as u64;
        }
    }
    let list = &list[..count as usize];
    list.contains(&POSITION) && list.contains(&FORWARD)
}
#[repr(C, align(16))]
struct Vector4([f32; 4]);
/// Through the engine's own typed setter, only while its prologue is the one the port was written against.
unsafe fn write_shared_vector4(bb: usize, key: u64, value: &[f32; 3], w: f32) -> bool {
    let exe = EXE.load(Ordering::Relaxed);
    if bb == 0 || exe == 0 {
        return false;
    }
    if !value.iter().all(|v| v.is_finite()) {
        return false;
    }
    if SETTER_MATCHES.load(Ordering::Relaxed) < 0 {
        let mut bytes = [0u8; 7];
        let matches = read_bytes(exe + SHARED_VECTOR4_SET, &mut bytes) && bytes == SHARED_VECTOR4_SET_BYTES;
        SETTER_MATCHES.store(matches as i8, Ordering::Relaxed);
    }
    if SETTER_MATCHES.load(Ordering::Relaxed) == 0 {
        return false;
    }
    let v = Vector4([value[0], value[1], value[2], w]);
    type Setter = unsafe extern "C" fn(usize, *const u64, *const f32, u8) -> bool;
    // SAFETY: the engine's setter, checked above, with the blackboard its own evaluator has just used.
    unsafe { core::mem::transmute::<usize, Setter>(exe + SHARED_VECTOR4_SET)(bb, &key, v.0.as_ptr(), 0) }
}
unsafe fn evaluate(rcx: usize, rdx: usize, out: *mut f32) -> i8 {
    let eval = EVAL.load(Ordering::Relaxed);
    if eval == 0 {
        return 0;
    }
    // SAFETY: the engine's evaluator with its own arguments.
    unsafe { core::mem::transmute::<usize, EvalFn>(eval)(rcx, rdx, out) }
}

/// The origin evaluator's callsite: the camera-space origin becomes the muzzle (or the sight line).
#[no_mangle]
pub unsafe extern "C" fn Hooked_WaPhysicalRayOrigin(rcx: usize, rdx: usize, out: *mut f32) -> i8 {
    let mut ray = RAY.with(Cell::get);
    ray.mine = false;
    ray.muzzle_seq = u32::MAX;
    ray.context = 0;
    ray.have_centre = false;
    ray.muzzle_forward = [0.0; 3];
    RAY.with(|r| r.set(ray));
    // SAFETY (whole body): the engine's evaluator and its out-buffer, the shared block, the plugin's globals.
    unsafe {
        let result = evaluate(rcx, rdx, out);
        count(&CyberpunkVR_DebugPhysicalRayCalls);
        if result == 0 || out.is_null() || CyberpunkVR_HitscanFromMuzzle == 0 {
            return result;
        }
        // THE FREE CHECKS COME FIRST: the muzzle and the staleness bound, then whose ray it is.
        let Some((mut muzzle, mut forward)) = muzzle() else { return result };
        let dx = *out - muzzle[0];
        let dy = *out.add(1) - muzzle[1];
        let dz = *out.add(2) - muzzle[2];
        let distance_sq = dx * dx + dy * dy + dz * dz;
        if !distance_sq.is_finite() || distance_sq > 3.0 * 3.0 {
            count(&CyberpunkVR_DebugPhysicalRayStaleMuzzle);
            return result;
        }
        if !player_owned_weapon(rdx) {
            count(&CyberpunkVR_DebugPhysicalRayForeign);
            return result;
        }
        let left = left_gun_fired(rdx);
        let left_gun = left.is_some();
        if let Some((m, f)) = left {
            muzzle = m;
            forward = f;
            count(&CyberpunkVR_DebugPhysicalRayLeftGun);
        }
        ray.mine = true;
        ray.muzzle_seq = volatile(&raw const g_provMuzzleSeq);
        // Both bases sampled HERE, on the same instant, and carried to the forward callsite.
        ray.muzzle_forward = forward;
        ray.have_centre = camera_forward(&mut ray.cone_centre);
        count(&CyberpunkVR_DebugPhysicalRayMine);
        let mut lift = [0.0f32; 3];
        if !left_gun {
            if let Some(l) = sight_line_offset(&forward) {
                lift = l;
                count(&CyberpunkVR_DebugPhysicalRaySightLine);
                CyberpunkVR_DebugPhysicalRaySightLift = [l[0], l[1], l[2], sqrt(l[0] * l[0] + l[1] * l[1] + l[2] * l[2])];
            }
        }
        *out = muzzle[0] + lift[0];
        *out.add(1) = muzzle[1] + lift[1];
        *out.add(2) = muzzle[2] + lift[2];
        *out.add(3) = 1.0;
        ray.context = rdx;
        ray.origin = [*out, *out.add(1), *out.add(2)];
        RAY.with(|r| r.set(ray));
        count(&CyberpunkVR_DebugPhysicalRayOriginWrites);
        let recoil_seq = ray.muzzle_seq as i32;
        let recoil_seen = g_provRecoilSeqSeen.load(Ordering::SeqCst);
        if !left_gun
            && recoil_seen != recoil_seq
            && g_provRecoilSeqSeen.compare_exchange(recoil_seen, recoil_seq, Ordering::SeqCst, Ordering::SeqCst).is_ok()
        {
            RecoilOnShot();
            count(&CyberpunkVR_DebugPhysicalRayRecoil);
        }
        // ...and a round out of the LEFT gun kicks the left hand.
        let left_seen = RECOIL_SEEN_LEFT.load(Ordering::SeqCst);
        if left_gun && left_seen != recoil_seq && RECOIL_SEEN_LEFT.compare_exchange(left_seen, recoil_seq, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            RecoilOnShotLeft();
            count(&CyberpunkVR_DebugPhysicalRayRecoil);
        }
        result
    }
}

/// The forward evaluator's callsite: the ray rotated from the cone axis onto the barrel, then published back.
#[no_mangle]
pub unsafe extern "C" fn Hooked_WaPhysicalRayForward(rcx: usize, rdx: usize, out: *mut f32) -> i8 {
    // SAFETY (whole body): as Hooked_WaPhysicalRayOrigin.
    unsafe {
        let result = evaluate(rcx, rdx, out);
        let mut ray = RAY.with(Cell::get);
        let (redirect, have_centre, centre, forward, shared_context, final_origin) =
            (ray.mine, ray.have_centre, ray.cone_centre, ray.muzzle_forward, ray.context, ray.origin);
        ray.mine = false;
        ray.muzzle_seq = u32::MAX;
        ray.context = 0;
        ray.have_centre = false;
        RAY.with(|r| r.set(ray));
        if result == 0 || !redirect || out.is_null() || CyberpunkVR_HitscanFromMuzzle == 0 {
            return result;
        }
        let raw = [*out, *out.add(1), *out.add(2)];
        CyberpunkVR_DebugPhysicalRayRawForward = [raw[0], raw[1], raw[2], *out.add(3)];
        // THE CENTRE OF THE CONE IS THE SHOT'S FIRST RAY, not the camera (keyed by the muzzle sequence).
        let mut cone_centre = centre;
        let mut have_cone = have_centre;
        if CyberpunkVR_ConeCentreFromCamera == 0 {
            let shot = volatile(&raw const g_provMuzzleSeq);
            let mut cone = CONE.lock().unwrap_or_else(|p| p.into_inner());
            if cone.0 != shot {
                cone.0 = shot;
                cone.1 = raw;
                count(&CyberpunkVR_DebugConeFirstRay);
            }
            cone_centre = cone.1;
            have_cone = cone_centre[0] * cone_centre[0] + cone_centre[1] * cone_centre[1] + cone_centre[2] * cone_centre[2] > 1e-6;
        }
        // ONE ROTATION, CONE AXIS -> BARREL AXIS, APPLIED TO THE PELLET AS IT CAME.
        let spread_delta = if have_cone {
            let Some(q) = rotation_between(&cone_centre, &forward) else { return result };
            count(&CyberpunkVR_DebugPhysicalRayConeRays);
            q
        } else {
            let Some(q) = rotation_between(&raw, &forward) else { return result };
            q
        };
        let redirected = rotate_vector(&spread_delta, &raw);
        let length_sq = redirected[0] * redirected[0] + redirected[1] * redirected[1] + redirected[2] * redirected[2];
        if length_sq < 1e-6 || !length_sq.is_finite() {
            return result;
        }
        let inv_length = 1.0 / sqrt(length_sq);
        *out = redirected[0] * inv_length;
        *out.add(1) = redirected[1] * inv_length;
        *out.add(2) = redirected[2] * inv_length;
        *out.add(3) = 0.0;
        CyberpunkVR_DebugPhysicalRayOutForward = [*out, *out.add(1), *out.add(2), *out.add(3)];
        count(&CyberpunkVR_DebugPhysicalRayForwardWrites);
        // The same effect, the same ray: what the impact consumers read is what was cast.
        if CyberpunkVR_HitscanSharedRay != 0 && shared_context != 0 && shared_context == rdx {
            if !shared_has_position_forward(shared_context) {
                count(&CyberpunkVR_DebugPhysicalRaySharedMissing);
            } else if write_shared_vector4(shared_context, POSITION, &final_origin, 1.0)
                && write_shared_vector4(shared_context, FORWARD, &[*out, *out.add(1), *out.add(2)], 0.0)
            {
                count(&CyberpunkVR_DebugPhysicalRayShared);
            }
        }
        result
    }
}

// ---- installation ----
/// The evaluator's callsite redirected to `hook` through a relay (mov rax, hook / jmp rax) in the trampoline arena,
/// only while the call there is still the evaluator's.
unsafe fn patch_physical_ray_at(exe: usize, site_rva: usize, hook: usize) -> bool {
    let callsite = exe + site_rva;
    let mut call = [0u8; 5];
    if !read_bytes(callsite, &mut call) || call[0] != 0xE8 {
        return false;
    }
    let rel = i32::from_le_bytes([call[1], call[2], call[3], call[4]]);
    if (callsite as i64 + 5 + rel as i64) as usize != exe + PHYSICAL_RAY_EVAL {
        return false;
    }
    let Some(relay) = allocate(callsite, 16) else { return false };
    let delta = relay as i64 - (callsite as i64 + 5);
    if delta < i32::MIN as i64 || delta > i32::MAX as i64 {
        return false;
    }
    let mut stub = [0x48u8, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xE0];
    stub[2..10].copy_from_slice(&(hook as u64).to_le_bytes());
    // SAFETY: fresh arena memory, then the evaluator's own call instruction.
    unsafe {
        write_code(relay, &stub);
        patch_call(callsite, relay)
    }
}
unsafe fn install_physical_ray(exe: usize) -> bool {
    // SAFETY (whole body): the exported state, the game's code.
    unsafe {
        if CyberpunkVR_DebugPhysicalRayPatched == 1 {
            return true;
        }
        EVAL.store(exe + PHYSICAL_RAY_EVAL, Ordering::Relaxed);
        if !patch_physical_ray_at(exe, PHYSICAL_RAY_ORIGIN_CALLSITE, Hooked_WaPhysicalRayOrigin as *const () as usize) {
            CyberpunkVR_DebugPhysicalRayPatched = -1;
            return false;
        }
        if !patch_physical_ray_at(exe, PHYSICAL_RAY_FORWARD_CALLSITE, Hooked_WaPhysicalRayForward as *const () as usize) {
            CyberpunkVR_DebugPhysicalRayPatched = -2;
            return false;
        }
        CyberpunkVR_DebugPhysicalRayPatched = 1;
        true
    }
}
/// The camera pose bookkeeping asks for the copy hook itself (Hooks/CameraPoseTransport), so it must not depend on
/// the rest of this family having installed.
#[no_mangle]
pub unsafe extern "C" fn cvr_weapon_aim_ensure_pose_copy_hook() -> bool {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if POSE_COPY_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let detour = Detour {
        name: "PoseCopy",
        target: POSE_COPY,
        absolute: false,
        detour: Hooked_WaPoseCopy as *const c_void,
        original: &raw mut POSE_COPY_ORIG,
        expect: None,
    };
    // SAFETY: the detour has the target's signature and reads its trampoline from POSE_COPY_ORIG.
    let ok = unsafe { install_chain("WeaponAimPoseCopy", &[detour]) }.is_ok();
    POSE_COPY_INSTALLED.store(ok, Ordering::Release);
    ok
}
/// Installed at boot (src/Hooks/WeaponAim.cpp's registration), and again harmlessly from the CET native.
#[no_mangle]
pub unsafe extern "C" fn cvr_weapon_aim_install() -> bool {
    let exe = image_base();
    if exe == 0 {
        return false;
    }
    EXE.store(exe, Ordering::Relaxed);
    // SAFETY (whole body): the hooks above, each with its target's signature.
    unsafe {
        let ok = cvr_weapon_aim_ensure_pose_copy_hook();
        install_physical_ray(exe); // reports itself: CyberpunkVR_DebugPhysicalRayPatched
        if !TARGETING_INSTALLED.load(Ordering::Acquire) {
            let detour = Detour {
                name: "TargetingUpdate",
                target: TARGETING_UPDATE,
                absolute: false,
                detour: Hooked_WaTargetingUpdate as *const c_void,
                original: &raw mut TARGETING_ORIG,
                expect: None,
            };
            TARGETING_INSTALLED.store(install_chain("WeaponAimTargeting", &[detour]).is_ok(), Ordering::Release);
        }
        CyberpunkVR_DebugWaInstalled = ok as i32;
        ok
    }
}
