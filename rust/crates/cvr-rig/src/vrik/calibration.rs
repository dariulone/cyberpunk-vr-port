//! The solve's calibration and the camera pair (formerly src/Natives/Calibration.cpp and src/Natives/CameraPair.cpp),
//! natives registered by rust/crates/cvr-ffi/src/natives.rs.
//!
//! CALIBRATION: the bind mode and the per-hand reach, elbow pole and swing, wrist correction and bone overrides the CET
//! console sets; the diagnostic dump (`WriteVRDiagCore`, also the overlay's "Log VR Diag"), and the RTTI dumps of the
//! player's animated components.
//!
//! THE CAMERA PAIR (`SetVRPlayerYaw`): the FPP camera and the player entity's world poses pushed from Lua each frame.
//! The one stabilised camera-local offset (camera minus entity) is computed here from that coherent pair, slew-limited
//! in XY (the sprint start/stop dive), and published both to the solve (g_VRCamPairLocal*, the transform snapshot) and
//! to the shared slots the view reads; the entity's velocity and the push time go there too.
use super::camera_link::cvr_camera_main_eye_centre_read_world;
use super::env::*;
use super::hand_body::text_mode;
use super::rig::VRIK_QuatRotateVec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use cvr_body::cmath::{cos, sin, sqrt};
use cvr_crt::{format as cformat, Arg};
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::rtti::{object_class, RttiSystem};
use red4ext_abi::stack::StackFrame;
use std::io::Write;
use std::sync::Mutex;

type Frame = StackFrame;

/// Natives/NativeHelpers.hpp VR_CAMTRACE_CAP: the camera-trace ring's bound.
const CAMTRACE_CAP: usize = 256;

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_VRPlayerYaw: f32;
    static mut g_VRCamI: f32;
    static mut g_VRCamJ: f32;
    static mut g_VRCamK: f32;
    static mut g_VRCamR: f32;
    static mut g_VRCamPosX: f32;
    static mut g_VRCamPosY: f32;
    static mut g_VRCamPosZ: f32;
    static mut g_VREntityPosX: f32;
    static mut g_VREntityPosY: f32;
    static mut g_VREntityPosZ: f32;
    static mut g_VREntityQI: f32;
    static mut g_VREntityQJ: f32;
    static mut g_VREntityQK: f32;
    static mut g_VREntityQR: f32;
    static mut g_VRCamPairLocalX: f32;
    static mut g_VRCamPairLocalY: f32;
    static mut g_VRCamPairLocalZ: f32;
    static mut g_VRCamPairValid: i32;
    static mut g_VRCamPosValid: i32;
    static g_VRPairSlewRate: f32;
    static g_VRPairLeadTicks: f32;
    static mut g_VRPoseCapGen: i32;
    static mut g_VRScaleR: f32;
    static mut g_VRScaleL: f32;
    static mut g_VROffRX: f32;
    static mut g_VROffRY: f32;
    static mut g_VROffRZ: f32;
    static mut g_VROffLX: f32;
    static mut g_VROffLY: f32;
    static mut g_VROffLZ: f32;
    static mut g_VRElbowPoleR: f32;
    static mut g_VRElbowPoleL: f32;
    static mut g_VRElbowSwingR: f32;
    static mut g_VRElbowSwingL: f32;
    static mut g_VRWristR_I: f32;
    static mut g_VRWristR_J: f32;
    static mut g_VRWristR_K: f32;
    static mut g_VRWristR_R: f32;
    static mut g_VRWristL_I: f32;
    static mut g_VRWristL_J: f32;
    static mut g_VRWristL_K: f32;
    static mut g_VRWristL_R: f32;
    // The diagnostic dump's reads.
    static g_VRDiagBones: [f32; 32 * 7];
    static g_AnimPoseLastBoneBuf: usize;
    static g_AnimPoseTotalCalls: u64;
    static g_AnimPoseMatchCalls: u64;
    static g_VRIKDbgTarget: [f32; 3];
    static g_VRIKDbgShoulder: [f32; 3];
    static g_VRIKDbgElbow: [f32; 3];
    static g_VRIKDbgLocal: [f32; 4];
    static g_VRIKDbgLens: [f32; 2];
    static g_VRIKDbgTargetL: [f32; 3];
    static g_VRIKDbgShoulderL: [f32; 3];
    static g_VRIKDbgElbowL: [f32; 3];
    static g_VRIKDbgLocalL: [f32; 4];
    static g_VRIKDbgLensL: [f32; 2];
    static g_VRIKDbgClav: [[f32; 8]; 2];
    static g_VRIKDbgChestTgt: [f32; 3];
    static g_VRIKDbgChest: [f32; 3];
    static g_VRIKDbgHipsYaw: f32;
    static g_VRIKDbgTargetTrace: [f32; 3];
    static g_VRIKDbgHandFK: [f32; 3];
    static g_VRIKDbgShModel: [f32; 3];
    static g_VRUserArmLenR: f32;
    static g_VRUserArmLenL: f32;
    static g_VRUserEyeHeight: f32;
    static g_vrikViewScaleUsed: f32;
    static g_VRIKSolvesLastTick: i32;
    static g_VRIKSolvesMaxTick: i32;
    static g_VRIKLastBufA: usize;
    static g_VRIKLastBufB: usize;
    static g_VRIKReplayTotal: i32;
    static g_VRBodyUnderHMD: i32;
    static g_VRChestDrop: f32;
    static g_VRChestFwd: f32;
    static g_bdActive: AtomicI32;
    static CyberpunkVR_IpdInWorldPos: i32;
    static CyberpunkVR_HeadTranslationInPatch: i32;
    // The C++ half.
    fn cvr_ensure_shared_memory();
    fn cvr_cpp_pose_device_cam_active() -> bool;
    fn cvr_anim_player_entity() -> *mut c_void;
}

macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
/// `out << value` under std::fixed << std::setprecision(4).
fn f4(v: f32) -> String {
    cformat(c"%.4f", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}
fn shared(slot: usize) -> f32 {
    // SAFETY: the mapped shared block, read only when it is there.
    unsafe {
        if g_pSharedHands.is_null() {
            0.0
        } else {
            *g_pSharedHands.add(slot)
        }
    }
}

// ---- calibration ----------------------------------------------------------------------------------------------

/// SetVRBindMode(mode) -> Int32: the bind mode; a mode on re-arms the pose reference captures.
#[no_mangle]
pub unsafe extern "C" fn SetVRBindMode(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut mode = 0i32;
    params!(frame, mode);
    g_VRBind = mode;
    if mode > 0 {
        g_VRPoseCapGen += 1;
    }
    put(out, 1);
}
/// SetVRBindParams(scale, x, y, z, axis, hand) -> Int32: per-hand reach scale and offset (0 right, 1 left, else both,
/// which also drives the legacy globals of modes 1..3).
#[no_mangle]
pub unsafe extern "C" fn SetVRBindParams(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut scale, mut x, mut y, mut z) = (1.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut axis, mut hand) = (1i32, 2i32);
    params!(frame, scale, x, y, z, axis, hand);
    g_VRBindAxis = axis;
    if hand != 1 {
        g_VRScaleR = scale;
        g_VROffRX = x;
        g_VROffRY = y;
        g_VROffRZ = z;
    }
    if hand != 0 {
        g_VRScaleL = scale;
        g_VROffLX = x;
        g_VROffLY = y;
        g_VROffLZ = z;
    }
    if hand == 2 {
        g_VRBindScale = scale;
        g_VRBindOffX = x;
        g_VRBindOffY = y;
        g_VRBindOffZ = z;
    }
    put(out, 1);
}
/// SetVRElbowPole(degrees, hand) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRElbowPole(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut angle, mut hand) = (0.0f32, 2i32);
    params!(frame, angle, hand);
    if hand != 1 {
        g_VRElbowPoleR = angle;
    }
    if hand != 0 {
        g_VRElbowPoleL = angle;
    }
    put(out, 1);
}
/// SetVRElbowSwing(gain, hand) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRElbowSwing(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut gain, mut hand) = (1.0f32, 2i32);
    params!(frame, gain, hand);
    if hand != 1 {
        g_VRElbowSwingR = gain;
    }
    if hand != 0 {
        g_VRElbowSwingL = gain;
    }
    put(out, 1);
}
/// SetVRHandOffset(pitch, yaw, roll, hand) -> Int32: the constant wrist correction (hand-local degrees, XYZ).
#[no_mangle]
pub unsafe extern "C" fn SetVRHandOffset(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut pitch, mut yaw, mut roll, mut hand) = (0.0f32, 0.0f32, 0.0f32, 2i32);
    params!(frame, pitch, yaw, roll, hand);
    let d2r = 0.01745329252f32 * 0.5;
    let (cp, sp) = (cos(pitch * d2r), sin(pitch * d2r));
    let (cy, sy) = (cos(yaw * d2r), sin(yaw * d2r));
    let (cr, sr) = (cos(roll * d2r), sin(roll * d2r));
    let qi = sp * cy * cr + cp * sy * sr;
    let qj = cp * sy * cr - sp * cy * sr;
    let qk = cp * cy * sr + sp * sy * cr;
    let qr = cp * cy * cr - sp * sy * sr;
    if hand != 1 {
        g_VRWristR_I = qi;
        g_VRWristR_J = qj;
        g_VRWristR_K = qk;
        g_VRWristR_R = qr;
    }
    if hand != 0 {
        g_VRWristL_I = qi;
        g_VRWristL_J = qj;
        g_VRWristL_K = qk;
        g_VRWristL_R = qr;
    }
    put(out, 1);
}
/// SetVRBindBones(left, right) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRBindBones(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut left, mut right) = (23i32, 24i32);
    params!(frame, left, right);
    g_VRLeftBoneIdx = left;
    g_VRRightBoneIdx = right;
    put(out, 1);
}
/// SetVRHeadBone(index) -> Int32: the resolved head bone overridden (-1 disables head-relative).
#[no_mangle]
pub unsafe extern "C" fn SetVRHeadBone(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut idx = -1i32;
    params!(frame, idx);
    g_VRHeadBoneIdx = idx;
    put(out, g_VRHeadBoneIdx);
}
/// SetVRUseHeadRelative(on) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRUseHeadRelative(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 1i32;
    params!(frame, on);
    g_VRUseHeadRelative = (on != 0) as i32;
    put(out, g_VRUseHeadRelative);
}
/// SetVRDiagCapture's work.
///
/// # Safety
/// The diagnostic flag's write on the game's main thread.
pub unsafe fn set_diag_capture(on: bool) {
    g_VRDiagCapture = on as i32;
}
/// SetVRDiagCapture(on) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRDiagCapture(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    g_VRDiagCapture = (on != 0) as i32;
    put(out, g_VRDiagCapture);
}

// ---- the camera-kick trace and the diagnostic dump -------------------------------------------------------------

/// The camera-kick trace: per push, 0..2 raw local, 3..4 camera quaternion i/j, 5..7 the filtered pair, 8..10 the
/// eye bake, 11 the camera bake's y, 12..14 dead, 15 the hips' model yaw, 16..17 the right IK target, 18..19 the solved
/// right hand, 20..21 the right shoulder (model x/y).
struct CamTrace {
    rows: Vec<[f32; 22]>,
    n: i32,
    /// -1 live, > 0 the post-stop countdown, 0 frozen.
    freeze: i32,
    prev_speed2: f32,
}
static CAM_TRACE: Mutex<CamTrace> = Mutex::new(CamTrace {
    rows: Vec::new(),
    n: 0,
    freeze: -1,
    prev_speed2: 0.0,
});

/// The diagnostic dump appended to log\vrik_diag.txt (the right gizmo target against the bones the hook captured,
/// the IK chains' intermediates, the camera pair, the camera-kick trace). Also the overlay's "Log VR Diag".
#[no_mangle]
pub unsafe extern "C" fn WriteVRDiagCore(cam_x: f32, cam_y: f32, cam_z: f32, qi: f32, qj: f32, qk: f32, qr: f32) {
    cvr_ensure_shared_memory();
    let raw = [shared(9), shared(10), shared(11)];
    let local = [raw[0], -raw[2], raw[1]];
    let cam_quat = [qi, qj, qk, qr];
    let mut world_off = [0.0f32; 3];
    VRIK_QuatRotateVec(cam_quat.as_ptr(), local.as_ptr(), world_off.as_mut_ptr());
    let gizmo = [cam_x + world_off[0], cam_y + world_off[1], cam_z + world_off[2]];
    let mut o = String::new();
    macro_rules! w { ($($t:tt)*) => { o.push_str(&format!($($t)*)) }; }
    let v3 = |v: &[f32]| format!("({}, {}, {})", f4(v[0]), f4(v[1]), f4(v[2]));
    w!("==== LogVRDiag ====\n");
    w!("cam.pos  = ({}, {}, {})\n", f4(cam_x), f4(cam_y), f4(cam_z));
    w!("cam.quat = ({}, {}, {}, {})\n", f4(qi), f4(qj), f4(qk), f4(qr));
    w!("VR.rawR  = {}\n", v3(&raw));
    w!("gizmoWorld(R) = {}\n", v3(&gizmo));
    if !g_pSharedHands.is_null() {
        let h = [shared(16), shared(17), shared(18), shared(19)];
        let base_off = [
            (h[3] * h[3] - h[0] * h[0] - h[1] * h[1] - h[2] * h[2]) * raw[0]
                + 2.0 * (h[0] * h[1] - h[3] * h[2]) * raw[1]
                + 2.0 * (h[0] * h[2] + h[3] * h[1]) * raw[2],
            2.0 * (h[0] * h[1] + h[3] * h[2]) * raw[0]
                + (h[3] * h[3] - h[0] * h[0] + h[1] * h[1] - h[2] * h[2]) * raw[1]
                + 2.0 * (h[1] * h[2] - h[3] * h[0]) * raw[2],
            2.0 * (h[0] * h[2] - h[3] * h[1]) * raw[0]
                + 2.0 * (h[1] * h[2] + h[3] * h[0]) * raw[1]
                + (h[3] * h[3] - h[0] * h[0] - h[1] * h[1] + h[2] * h[2]) * raw[2],
        ];
        w!("hmdRel   = ({}, {}, {}, {})\n", f4(h[0]), f4(h[1]), f4(h[2]), f4(h[3]));
        w!("baseOff(hmdRel*raw) = {}\n", v3(&base_off));
        w!("mapLocal(x,-z,y)    = ({}, {}, {})\n", f4(base_off[0]), f4(-base_off[2]), f4(base_off[1]));
    }
    w!(
        "headIdx={} rightIdx={} leftIdx={} diagCapture={} lastBoneBuf=0x{:x}\n",
        g_VRHeadBoneIdx,
        g_VRRightBoneIdx,
        g_VRLeftBoneIdx,
        g_VRDiagCapture,
        g_AnimPoseLastBoneBuf
    );
    let bones: [(usize, &str); 12] = [
        (2, "Hips"),
        (13, "Spine3"),
        (16, "Neck"),
        (22, "Head"),
        (15, "RightShoulder"),
        (18, "RightArm"),
        (21, "RightForeArm"),
        (24, "RightHand"),
        (14, "LeftShoulder"),
        (17, "LeftArm"),
        (20, "LeftForeArm"),
        (23, "LeftHand"),
    ];
    let bone = |i: usize| -> [f32; 7] {
        let mut d = [0.0f32; 7];
        d.copy_from_slice(&g_VRDiagBones[i * 7..i * 7 + 7]);
        d
    };
    for (idx, name) in bones {
        let d = bone(idx);
        w!(
            "bone[{}] {} pos=({}, {}, {}) quat=({}, {}, {}, {})\n",
            idx,
            name,
            f4(d[0]),
            f4(d[1]),
            f4(d[2]),
            f4(d[3]),
            f4(d[4]),
            f4(d[5]),
            f4(d[6])
        );
    }
    let (head, hand, sh, fa) = (bone(22), bone(24), bone(15), bone(21));
    w!(
        "bufHand24 - bufHead22 = ({}, {}, {})\n",
        f4(hand[0] - head[0]),
        f4(hand[1] - head[1]),
        f4(hand[2] - head[2])
    );
    w!(
        "gizmoWorld  - cam.pos = ({}, {}, {})\n",
        f4(gizmo[0] - cam_x),
        f4(gizmo[1] - cam_y),
        f4(gizmo[2] - cam_z)
    );
    let l1 = sqrt((fa[0] - sh[0]) * (fa[0] - sh[0]) + (fa[1] - sh[1]) * (fa[1] - sh[1]) + (fa[2] - sh[2]) * (fa[2] - sh[2]));
    let l2 = sqrt((hand[0] - fa[0]) * (hand[0] - fa[0]) + (hand[1] - fa[1]) * (hand[1] - fa[1]) + (hand[2] - fa[2]) * (hand[2] - fa[2]));
    w!("restLen shoulder->forearm={} forearm->hand={}\n", f4(l1), f4(l2));
    w!(
        "IK chain: rArm={} rFore={} rHand={} boneCount={} bind={}\n",
        g_VRRightUpperArmIdx,
        g_VRRightForeArmIdx,
        g_VRRightBoneIdx,
        g_VRBoneCount,
        g_VRBind
    );
    w!("IK target(model)   = {}\n", v3(&g_VRIKDbgTarget));
    w!("IK shoulder(model) = {}\n", v3(&g_VRIKDbgShoulder));
    w!("IK elbow(model)    = {}\n", v3(&g_VRIKDbgElbow));
    let l = g_VRIKDbgLocal;
    w!("IK hand body(lx,ly,lz,cross) = ({}, {}, {}, {})\n", f4(l[0]), f4(l[1]), f4(l[2]), f4(l[3]));
    w!(
        "IK lens upper={} fore={} scale={} yaw={}\n",
        f4(g_VRIKDbgLens[0]),
        f4(g_VRIKDbgLens[1]),
        f4(g_VRBindScale),
        f4(g_VRPlayerYaw)
    );
    {
        // The gizmo-exact gate: the camera and the right gizmo hand reconstructed in model space from the hook's own
        // camera and entity transforms (world -> model = conj(entity)).
        let mut ent_q = [g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR];
        let en = sqrt(ent_q[0] * ent_q[0] + ent_q[1] * ent_q[1] + ent_q[2] * ent_q[2] + ent_q[3] * ent_q[3]);
        if en > 1e-4 {
            for q in &mut ent_q {
                *q /= en;
            }
        } else {
            ent_q = [0.0, 0.0, 0.0, 1.0];
        }
        let inv_ent = [-ent_q[0], -ent_q[1], -ent_q[2], ent_q[3]];
        let to_model = |x: f32, y: f32, z: f32| -> [f32; 3] {
            let v = [x, y, z];
            let mut o = [0.0f32; 3];
            VRIK_QuatRotateVec(inv_ent.as_ptr(), v.as_ptr(), o.as_mut_ptr());
            o
        };
        let cam_q2 = [g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR];
        let mut woff = [0.0f32; 3];
        VRIK_QuatRotateVec(cam_q2.as_ptr(), local.as_ptr(), woff.as_mut_ptr());
        let giz_r = [g_VRCamPosX + woff[0], g_VRCamPosY + woff[1], g_VRCamPosZ + woff[2]];
        let cam_model_p = to_model(g_VRCamPosX - g_VREntityPosX, g_VRCamPosY - g_VREntityPosY, g_VRCamPosZ - g_VREntityPosZ);
        let gizmo_model = to_model(giz_r[0] - g_VREntityPosX, giz_r[1] - g_VREntityPosY, giz_r[2] - g_VREntityPosZ);
        w!(
            "camPosValid={} camPos=({}, {}, {}) entityPos=({}, {}, {})\n",
            g_VRCamPosValid,
            f4(g_VRCamPosX),
            f4(g_VRCamPosY),
            f4(g_VRCamPosZ),
            f4(g_VREntityPosX),
            f4(g_VREntityPosY),
            f4(g_VREntityPosZ)
        );
        w!(
            "entityQuat = ({}, {}, {}, {})\n",
            f4(g_VREntityQI),
            f4(g_VREntityQJ),
            f4(g_VREntityQK),
            f4(g_VREntityQR)
        );
        w!("camModel(reconstructed) = {}\n", v3(&cam_model_p));
        if (0..256).contains(&g_VRHeadBoneIdx) {
            let hp = g_fkPos[g_VRHeadBoneIdx as usize];
            w!(
                "head bone(model FK)     = {}  delta camModel-head = ({}, {}, {})\n",
                v3(&hp),
                f4(cam_model_p[0] - hp[0]),
                f4(cam_model_p[1] - hp[1]),
                f4(cam_model_p[2] - hp[2])
            );
        }
        w!("gizmoModel(reconstructed) = {}\n", v3(&gizmo_model));
        w!(
            "GATE delta gizmoModel - IKtarget = ({}, {}, {})  (want ~0)\n",
            f4(gizmo_model[0] - g_VRIKDbgTarget[0]),
            f4(gizmo_model[1] - g_VRIKDbgTarget[1]),
            f4(gizmo_model[2] - g_VRIKDbgTarget[2])
        );
        w!(
            "userArmLen R/L={}/{} eyeHeight={}\n",
            f4(g_VRUserArmLenR),
            f4(g_VRUserArmLenL),
            f4(g_VRUserEyeHeight)
        );
        if !g_pSharedHands.is_null() {
            w!(
                "viewPose raw[108..110]=({}, {}, {}, flag={}) scaleUsed={}{}\n",
                f4(shared(108)),
                f4(shared(109)),
                f4(shared(110)),
                f4(shared(111)),
                f4(g_vrikViewScaleUsed),
                if g_vrikViewScaleUsed == 0.0 { " (REJECTED -> fallback)" } else { "" }
            );
            w!(
                "pairLocal[128..131]=({}, {}, {}, seq={}) entSeq[99]={} pairValid={}\n",
                f4(shared(128)),
                f4(shared(129)),
                f4(shared(130)),
                f4(shared(131)),
                f4(shared(99)),
                g_VRCamPairValid
            );
        }
        {
            let mut trace = CAM_TRACE.lock().unwrap_or_else(|p| p.into_inner());
            let n = (trace.n.max(0) as usize).min(CAMTRACE_CAP);
            w!(
                "camTrace freeze={}{}\n",
                trace.freeze,
                if trace.freeze == 0 {
                    " (FROZEN on sprint stop -- window preserved)"
                } else {
                    " (live)"
                }
            );
            if n > 8 && trace.rows.len() >= n {
                const COLUMNS: [&str; 22] = [
                    "rawX",
                    "rawY",
                    "rawZ",
                    "quatI",
                    "quatJ",
                    "fltX",
                    "fltY",
                    "fltZ",
                    "eyeBX",
                    "eyeBY",
                    "eyeBZ",
                    "camBY",
                    "locX",
                    "locY",
                    "locZ",
                    "hipsYawDeg",
                    "tgtX",
                    "tgtY",
                    "handX",
                    "handY",
                    "shX",
                    "shY",
                ];
                w!("camTrace n={} (per-channel min..max [range])\n", n);
                for (c, name) in COLUMNS.iter().enumerate() {
                    let (mut mn, mut mx) = (trace.rows[0][c], trace.rows[0][c]);
                    for row in &trace.rows[1..n] {
                        let v = row[c];
                        if v < mn {
                            mn = v;
                        }
                        if v > mx {
                            mx = v;
                        }
                    }
                    w!("  {} {} .. {}  [{}]\n", name, f4(mn), f4(mx), f4(mx - mn));
                }
                let newest = ((trace.n - 1) as usize) % CAMTRACE_CAP;
                w!("camTrace tail (oldest->newest):\n");
                let mut k = 176i32;
                while k >= 0 {
                    if (k as usize) < n {
                        let i = ((newest as i32 - k).rem_euclid(CAMTRACE_CAP as i32)) as usize;
                        let r = trace.rows[i];
                        w!(
                            "  raw=({}, {}, {}) hipsYaw={} tgt=({}, {}) hand=({}, {}) sh=({}, {})\n",
                            f4(r[0]),
                            f4(r[1]),
                            f4(r[2]),
                            f4(r[15]),
                            f4(r[16]),
                            f4(r[17]),
                            f4(r[18]),
                            f4(r[19]),
                            f4(r[20]),
                            f4(r[21])
                        );
                    }
                    k -= 4;
                }
            }
            trace.freeze = -1;
        }
        for (cs, side) in ["clavR", "clavL"].iter().enumerate() {
            let c = g_VRIKDbgClav[cs];
            w!(
                "{} desired=({}, {}, {}) joint=({}, {}, {}) need={}deg applied={}deg\n",
                side,
                f4(c[0]),
                f4(c[1]),
                f4(c[2]),
                f4(c[3]),
                f4(c[4]),
                f4(c[5]),
                f4(c[6]),
                f4(c[7])
            );
        }
        if !g_pSharedHands.is_null() {
            w!(
                "viewOffs eyeBake[116..119]=({}, {}, {}, valid={})\n",
                f4(shared(116)),
                f4(shared(117)),
                f4(shared(118)),
                f4(shared(119))
            );
            w!(
                "viewOffs dxgiTotal[120..123]=({}, {}, {}, valid={})\n",
                f4(shared(120)),
                f4(shared(121)),
                f4(shared(122)),
                f4(shared(123))
            );
            w!("viewOffs camBake[91..93]=({}, {}, {})\n", f4(shared(91)), f4(shared(92)), f4(shared(93)));
            w!(
                "laserDot gate[144]={} muzzleFwd[24..26]=({}, {}, {}) valid[27]={} zoom[28]={}\n",
                f4(shared(144)),
                f4(shared(24)),
                f4(shared(25)),
                f4(shared(26)),
                f4(shared(27)),
                f4(shared(28))
            );
        }
        w!(
            "solvesPerTick last={} max={} bufA=0x{:x} bufB=0x{:x} totalCalls={} matchCalls={} replays={}\n",
            g_VRIKSolvesLastTick,
            g_VRIKSolvesMaxTick,
            g_VRIKLastBufA,
            g_VRIKLastBufB,
            g_AnimPoseTotalCalls,
            g_AnimPoseMatchCalls,
            g_VRIKReplayTotal
        );
        w!(
            "bodyUnderHMD={} chestDrop={} chestFwd={}\n",
            g_VRBodyUnderHMD,
            f4(g_VRChestDrop),
            f4(g_VRChestFwd)
        );
        w!("chest target(model) = {}\n", v3(&g_VRIKDbgChestTgt));
        w!("chest actual(model) = {}\n", v3(&g_VRIKDbgChest));
        w!(
            "lowerbody idx: hips={} rUpLeg={} rLeg={} rFoot={} lUpLeg={} lLeg={} lFoot={} neck={}\n",
            g_VRHipsIdx,
            g_VRRightUpLegIdx,
            g_VRRightLegIdx,
            g_VRRightFootIdx,
            g_VRLeftUpLegIdx,
            g_VRLeftLegIdx,
            g_VRLeftFootIdx,
            g_VRNeckIdx
        );
        let fk = |i: i32| g_fkPos[i as usize];
        for (idx, name) in [
            (g_VRHipsIdx, "Hips"),
            (g_VRRightUpLegIdx, "RUpLeg"),
            (g_VRRightLegIdx, "RLeg"),
            (g_VRRightFootIdx, "RFoot"),
            (g_VRLeftUpLegIdx, "LUpLeg"),
            (g_VRLeftLegIdx, "LLeg"),
            (g_VRLeftFootIdx, "LFoot"),
        ] {
            if (0..256).contains(&idx) {
                w!("  {}[{}] fk={}\n", name, idx, v3(&fk(idx)));
            } else {
                w!("  {} = UNRESOLVED\n", name);
            }
        }
        let dist = |a: i32, b: i32| {
            let (p, q) = (fk(a), fk(b));
            let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
            sqrt(dx * dx + dy * dy + dz * dz)
        };
        let chain = |a: i32, b: i32, c: i32| -> f32 {
            if a < 0 || b < 0 || c < 0 || a >= 256 || b >= 256 || c >= 256 {
                return 0.0;
            }
            dist(b, a) + dist(c, b)
        };
        w!(
            "avatar arm FK R/L = {}/{}  target userArmLen={}/{}\n",
            f4(chain(g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx)),
            f4(chain(g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx)),
            f4(g_VRUserArmLenR),
            f4(g_VRUserArmLenL)
        );
        w!(
            "avatar leg FK R/L = {}/{}\n",
            f4(chain(g_VRRightUpLegIdx, g_VRRightLegIdx, g_VRRightFootIdx)),
            f4(chain(g_VRLeftUpLegIdx, g_VRLeftLegIdx, g_VRLeftFootIdx))
        );
    }
    w!("LK target(model)   = {}\n", v3(&g_VRIKDbgTargetL));
    w!("LK shoulder(model) = {}\n", v3(&g_VRIKDbgShoulderL));
    w!("LK elbow(model)    = {}\n", v3(&g_VRIKDbgElbowL));
    let l = g_VRIKDbgLocalL;
    w!("LK hand body(lx,ly,lz,cross) = ({}, {}, {}, {})\n", f4(l[0]), f4(l[1]), f4(l[2]), f4(l[3]));
    w!("LK lens upper={} fore={}\n\n", f4(g_VRIKDbgLensL[0]), f4(g_VRIKDbgLensL[1]));
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "vrik_diag.txt");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text_mode(&o).as_bytes());
    }
}
/// LogVRDiag(camX, camY, camZ, qi, qj, qk, qr) -> Int32: the dump, with the FPP camera's world pose.
#[no_mangle]
pub unsafe extern "C" fn LogVRDiag(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    let (mut qi, mut qj, mut qk, mut qr) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
    params!(frame, x, y, z, qi, qj, qk, qr);
    WriteVRDiagCore(x, y, z, qi, qj, qk, qr);
    put(out, 1);
}

// ---- the RTTI dumps of the player's components -----------------------------------------------------------------

/// A handle as stored in an array (instance, reference count), read without touching the count.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawHandle {
    instance: *mut u8,
    ref_count: *mut c_void,
}
/// The player's components (ent::Entity::components, +0xA0) with their class names.
unsafe fn player_components() -> Option<Vec<(*mut u8, CName)>> {
    let player = cvr_anim_player_entity();
    if player.is_null() {
        return None;
    }
    let list = &*((player as *const u8).add(0xA0) as *const DynArray<RawHandle>);
    Some(
        list.as_slice()
            .iter()
            .filter(|h| !h.instance.is_null())
            .filter_map(|h| object_class(h.instance as *const c_void).map(|c| (h.instance, c.class_name())))
            .collect(),
    )
}
fn log_file(name: &str) -> Option<std::fs::File> {
    std::fs::File::create(cvr_host::paths::file(cvr_host::paths::Folder::Log, name)).ok()
}
/// DumpAnimVTable(): the first 60 vtable slots of the player's entAnimatedComponent (log\anim_vtable_dump.txt).
#[no_mangle]
pub unsafe extern "C" fn DumpAnimVTable(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    params!(frame);
    let mut file = log_file("anim_vtable_dump.txt");
    let mut o = String::new();
    match player_components() {
        None => o.push_str("No player\n"),
        Some(components) => {
            if let Some((component, _)) = components.into_iter().find(|(_, n)| *n == CName::hashed("entAnimatedComponent")) {
                o.push_str(&format!("Found entAnimatedComponent at: {:x}\n", component as usize));
                let vtable = *(component as *const *const usize);
                o.push_str(&format!("VTable Address: {:x}\n", vtable as usize));
                let base = cvr_engine::code::image_base();
                for i in 0..60usize {
                    let func = *vtable.add(i);
                    o.push_str(&format!("  [{}] Func: {:x} (Cyberpunk2077.exe+{:x})\n", i, func, func.wrapping_sub(base)));
                }
            }
        }
    }
    if let Some(f) = file.as_mut() {
        let _ = f.write_all(text_mode(&o).as_bytes());
    }
}
/// DumpAnimControllerComponents() -> Int32: the player's animation controllers and their IK targets
/// (log\anim_controller_dump.txt); -1 without a player.
#[no_mangle]
pub unsafe extern "C" fn DumpAnimControllerComponents(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let Some(components) = player_components() else {
        put(out, -1);
        return;
    };
    let mut o = String::new();
    let mut dumped = 0;
    for (c, name) in components {
        if name != CName::hashed("entAnimationControllerComponent") {
            continue;
        }
        // ent::IComponent: name (+0x40), isEnabled (+0x8B); entAnimationControllerComponent: lookAtController
        // (+0x120), ikTargetController (+0x1A8: the component +0, targetData +0x10, ikParams +0x20).
        let read_ptr = |at: usize| *(c.add(at) as *const usize);
        o.push_str("==================================================\n");
        o.push_str(&format!("AnimationControllerComponent ptr=0x{:x}\n", c as usize));
        o.push_str(&format!(
            "name={} enabled={}\n",
            (*(c.add(0x40) as *const CName)).text(),
            (*(c.add(0x8B) as *const u8) != 0) as i32
        ));
        o.push_str(&format!("lookAtController ptr(backref)=0x{:x}\n", read_ptr(0x120)));
        o.push_str(&format!("ikTargetController ptr(backref)=0x{:x}\n", read_ptr(0x1A8)));
        let targets = (c.add(0x1B8) as *const DynArray<[u8; 0x30]>).as_ref().map(|a| a.as_slice()).unwrap_or(&[]);
        o.push_str(&format!("ikTargetController.targetData.size={}\n", targets.len()));
        for (i, t) in targets.iter().enumerate() {
            let id = i32::from_le_bytes(t[0..4].try_into().unwrap());
            let part = CName(u64::from_le_bytes(t[8..16].try_into().unwrap()));
            let pos = usize::from_le_bytes(t[0x10..0x18].try_into().unwrap());
            let ori = usize::from_le_bytes(t[0x20..0x28].try_into().unwrap());
            o.push_str(&format!(
                "  [{}] id={} part={} posProvider=0x{:x} orientProvider=0x{:x}\n",
                i,
                id,
                part.text(),
                pos,
                ori
            ));
        }
        o.push_str(&format!("ikParams=0x{:x}\n", read_ptr(0x1C8)));
        dumped += 1;
    }
    if let Some(mut f) = log_file("anim_controller_dump.txt") {
        let _ = f.write_all(text_mode(&o).as_bytes());
    }
    put(out, dumped);
}
/// DumpRuntimeClassFunctions() -> Int32: the member and static functions of three classes
/// (log\runtime_class_functions.txt).
#[no_mangle]
pub unsafe extern "C" fn DumpRuntimeClassFunctions(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let mut file = log_file("runtime_class_functions.txt");
    let mut o = String::new();
    let mut dumped = 0;
    if let Some(rtti) = RttiSystem::get() {
        for class_name in ["entAnimationControllerComponent", "entEntity", "entAnimatedComponent"] {
            let Some(class) = rtti.get_class(CName::hashed(class_name)) else { continue };
            o.push_str("==================================================\n");
            o.push_str(&format!("CLASS {}\n", class_name));
            o.push_str("==================================================\n");
            o.push_str("Member functions:\n");
            for f in class.funcs().iter().filter_map(|&f| f.as_ref()) {
                o.push_str(&format!("  - {}\n", f.full_name.text()));
            }
            o.push_str("Static functions:\n");
            for f in class.static_funcs().iter().filter_map(|&f| f.as_ref()) {
                o.push_str(&format!("  - {}\n", f.full_name.text()));
            }
            o.push('\n');
            dumped += 1;
        }
    }
    if let Some(f) = file.as_mut() {
        let _ = f.write_all(text_mode(&o).as_bytes());
    }
    put(out, dumped);
}

// ---- the camera pair ------------------------------------------------------------------------------------------

/// The transform snapshot the solve reads (VRIK_ReadTransformSnapshot, src/Natives/CameraPair.cpp forwarding): a
/// sequence lock over atomic lanes -- even stable, odd a write in progress.
struct Snapshot {
    seq: AtomicU32,
    lanes: [AtomicU32; 14],
    valid: AtomicU32,
    published_ms: AtomicU64,
}
static SNAPSHOT: Snapshot = Snapshot {
    seq: AtomicU32::new(0),
    lanes: [const { AtomicU32::new(0) }; 14],
    valid: AtomicU32::new(0),
    published_ms: AtomicU64::new(0),
};
fn tick_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { GetTickCount64() }
}
unsafe fn publish_snapshot(body_camera_minus_entity: [f32; 3]) {
    SNAPSHOT.seq.fetch_add(1, Ordering::AcqRel);
    let values = [
        g_VRCamI,
        g_VRCamJ,
        g_VRCamK,
        g_VRCamR,
        g_VREntityQI,
        g_VREntityQJ,
        g_VREntityQK,
        g_VREntityQR,
        g_VRCamPairLocalX,
        g_VRCamPairLocalY,
        g_VRCamPairLocalZ,
        body_camera_minus_entity[0],
        body_camera_minus_entity[1],
        body_camera_minus_entity[2],
    ];
    for (lane, v) in SNAPSHOT.lanes.iter().zip(values) {
        lane.store(v.to_bits(), Ordering::Relaxed);
    }
    SNAPSHOT.valid.store(1, Ordering::Relaxed);
    SNAPSHOT.published_ms.store(tick_ms(), Ordering::Relaxed);
    SNAPSHOT.seq.fetch_add(1, Ordering::Release);
}
fn invalidate_snapshot() {
    SNAPSHOT.seq.fetch_add(1, Ordering::AcqRel);
    SNAPSHOT.valid.store(0, Ordering::Relaxed);
    SNAPSHOT.seq.fetch_add(1, Ordering::Release);
}
/// The last complete snapshot (four tries past a write in progress); a valid one older than 250 ms reads as
/// unavailable.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrik_read_transform_snapshot(out: *mut VrikTransformSnapshot) -> bool {
    let Some(out) = out.as_mut() else { return false };
    for _ in 0..4 {
        let s0 = SNAPSHOT.seq.load(Ordering::Acquire);
        if s0 == 0 || s0 & 1 != 0 {
            continue;
        }
        let lane = |i: usize| f32::from_bits(SNAPSHOT.lanes[i].load(Ordering::Relaxed));
        let mut tmp = VrikTransformSnapshot::default();
        for i in 0..4 {
            tmp.camQuat[i] = lane(i);
            tmp.entityQuat[i] = lane(4 + i);
        }
        for i in 0..3 {
            tmp.cameraMinusEntity[i] = lane(8 + i);
            tmp.bodyCameraMinusEntity[i] = lane(11 + i);
        }
        tmp.valid = SNAPSHOT.valid.load(Ordering::Relaxed);
        let published = SNAPSHOT.published_ms.load(Ordering::Relaxed);
        if SNAPSHOT.seq.load(Ordering::Acquire) == s0 {
            if tmp.valid != 0 && (published == 0 || tick_ms().wrapping_sub(published) > 250) {
                tmp.valid = 0;
                tmp.unavailable = 1;
            }
            *out = tmp;
            return true;
        }
    }
    false
}

/// The pair's own state across pushes.
struct PairState {
    slew: [f32; 2],
    pvel: [f32; 2],
    slew_init: bool,
    ent_seq: f32,
    qpf: i64,
    prev_t: i64,
    prev_ent: [f32; 3],
    vel: [f32; 3],
    vel_init: bool,
}
static PAIR: Mutex<PairState> = Mutex::new(PairState {
    slew: [0.0; 2],
    pvel: [0.0; 2],
    slew_init: false,
    ent_seq: 0.0,
    qpf: 0,
    prev_t: 0,
    prev_ent: [0.0; 3],
    vel: [0.0; 3],
    vel_init: false,
});

/// SetVRPlayerYaw(yaw, camera quaternion (4), camera position (3), entity position (3), entity quaternion (4)) ->
/// Int32: the camera pair pushed from Lua.
#[no_mangle]
pub unsafe extern "C" fn SetVRPlayerYaw(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut p_yaw = 0.0f32;
    let (mut ci, mut cj, mut ck, mut cr) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
    let (mut cam_x, mut cam_y, mut cam_z) = (0.0f32, 0.0f32, 0.0f32);
    let (mut ent_x, mut ent_y, mut ent_z) = (0.0f32, 0.0f32, 0.0f32);
    let (mut eqi, mut eqj, mut eqk, mut eqr) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
    params!(frame, p_yaw, ci, cj, ck, cr, cam_x, cam_y, cam_z, ent_x, ent_y, ent_z, eqi, eqj, eqk, eqr);
    put(out, 0);
    let mut body_camera_world = [cam_x, cam_y, cam_z];
    // The Lua path samples the same FPP component; it must mean the same position as the native pair (the centre,
    // not MAIN's eye), resolved before the pair filter.
    if (CyberpunkVR_VrikNativeFramePair != 0 || CyberpunkVR_VrikTransformsFromPlugin != 0)
        && (CyberpunkVR_IpdInWorldPos != 0 || CyberpunkVR_HeadTranslationInPatch != 0)
        && !cvr_cpp_pose_device_cam_active()
        && g_bdActive.load(Ordering::Relaxed) == 0
    {
        let supplied = [cam_x, cam_y, cam_z];
        let mut centre = [0.0f32; 3];
        if !cvr_camera_main_eye_centre_read_world(&supplied, &mut centre, &mut body_camera_world) {
            // The previous complete snapshot is kept briefly; a raw eye never replaces it.
            return;
        }
        cam_x = centre[0];
        cam_y = centre[1];
        cam_z = centre[2];
    }
    g_VRPlayerYaw = p_yaw;
    g_VRCamI = ci;
    g_VRCamJ = cj;
    g_VRCamK = ck;
    g_VRCamR = cr;
    let degenerate = cam_x == 0.0 && cam_y == 0.0 && cam_z == 0.0 && ent_x == 0.0 && ent_y == 0.0 && ent_z == 0.0;
    if !degenerate {
        g_VRCamPosX = cam_x;
        g_VRCamPosY = cam_y;
        g_VRCamPosZ = cam_z;
        g_VREntityPosX = ent_x;
        g_VREntityPosY = ent_y;
        g_VREntityPosZ = ent_z;
        g_VREntityQI = eqi;
        g_VREntityQJ = eqj;
        g_VREntityQK = eqk;
        g_VREntityQR = eqr;
        g_VRCamPosValid = 1;
    }
    let mut pair_accepted = false;
    {
        // The camera-kick trace, with its sprint-stop auto-freeze: when the entity's speed EMA falls through 3.5 m/s,
        // 90 more pushes are recorded and the ring freezes (the dump un-freezes it).
        let mut trace = CAM_TRACE.lock().unwrap_or_else(|p| p.into_inner());
        if !g_pSharedHands.is_null() {
            let (tvx, tvy) = (shared(132), shared(133));
            let sp2 = tvx * tvx + tvy * tvy;
            if trace.freeze < 0 && trace.prev_speed2 > 12.25 && sp2 <= 12.25 {
                trace.freeze = 90;
            }
            trace.prev_speed2 = sp2;
        }
        if trace.freeze > 0 {
            trace.freeze -= 1;
        }
        let (lx, ly, lz) = (cam_x - ent_x, cam_y - ent_y, cam_z - ent_z);
        if trace.freeze != 0 && lx * lx + ly * ly + lz * lz < 9.0 {
            if trace.rows.len() < CAMTRACE_CAP {
                trace.rows.resize(CAMTRACE_CAP, [0.0; 22]);
            }
            let w = (trace.n as usize) % CAMTRACE_CAP;
            let s = |i: usize| if g_pSharedHands.is_null() { 0.0 } else { shared(i) };
            trace.rows[w] = [
                lx,
                ly,
                lz,
                ci,
                cj,
                g_VRCamPairLocalX,
                g_VRCamPairLocalY,
                g_VRCamPairLocalZ,
                s(116),
                s(117),
                s(118),
                s(92),
                0.0,
                0.0,
                0.0,
                g_VRIKDbgHipsYaw,
                g_VRIKDbgTargetTrace[0],
                g_VRIKDbgTargetTrace[1],
                g_VRIKDbgHandFK[0],
                g_VRIKDbgHandFK[1],
                g_VRIKDbgShModel[0],
                g_VRIKDbgShModel[1],
            ];
            trace.n += 1;
        }
    }
    let mut pair = PAIR.lock().unwrap_or_else(|p| p.into_inner());
    {
        // The XY slew limiter: steady states pass exactly, the swing rate is capped (~0.5 m/s by default, live via
        // SetVRPairSlew); Z passes raw; more than 0.35 m snaps (a teleport).
        let (lx, ly, lz) = (cam_x - ent_x, cam_y - ent_y, cam_z - ent_z);
        let l2 = lx * lx + ly * ly + lz * lz;
        if degenerate || l2 < 1.0e-4 {
            // Ignored: the previous pair state stands.
        } else if l2 < 9.0 {
            if !pair.slew_init {
                pair.slew = [lx, ly];
                pair.slew_init = true;
            }
            let (rx, ry) = (lx - pair.slew[0], ly - pair.slew[1]);
            if rx * rx + ry * ry > 0.1225 {
                pair.slew = [lx, ly];
                pair.pvel = [0.0, 0.0];
            } else {
                let max_step = g_VRPairSlewRate * 0.0166;
                let clamp = |v: f32| {
                    if v > max_step {
                        max_step
                    } else if v < -max_step {
                        -max_step
                    } else {
                        v
                    }
                };
                let (sx, sy) = (clamp(rx), clamp(ry));
                pair.slew[0] += sx;
                pair.slew[1] += sy;
                pair.pvel[0] += (sx - pair.pvel[0]) * 0.4;
                pair.pvel[1] += (sy - pair.pvel[1]) * 0.4;
            }
            let lead = g_VRPairLeadTicks;
            g_VRCamPairLocalX = pair.slew[0] + pair.pvel[0] * lead;
            g_VRCamPairLocalY = pair.slew[1] + pair.pvel[1] * lead;
            g_VRCamPairLocalZ = lz;
            g_VRCamPairValid = 1;
            pair_accepted = true;
        } else {
            // A cinematic or detached camera: consumers fall back.
            g_VRCamPairValid = 0;
        }
    }
    if !degenerate && g_VRCamPosValid != 0 {
        if pair_accepted {
            publish_snapshot([body_camera_world[0] - ent_x, body_camera_world[1] - ent_y, body_camera_world[2] - ent_z]);
        } else if g_VRCamPairValid == 0 {
            invalidate_snapshot();
        }
    }
    if !g_pSharedHands.is_null() && !degenerate {
        let s = g_pSharedHands;
        pair.ent_seq += 1.0;
        if pair.ent_seq > 1.0e6 {
            pair.ent_seq = 1.0;
        }
        // The entity's velocity and the push time ([132..136]) for the view's extrapolation.
        {
            use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
            if pair.qpf == 0 {
                QueryPerformanceFrequency(&mut pair.qpf);
            }
            let mut now = 0i64;
            QueryPerformanceCounter(&mut now);
            if pair.vel_init && pair.qpf != 0 {
                let dt = (now - pair.prev_t) as f64 / pair.qpf as f64;
                if dt > 1.0e-4 && dt < 0.25 {
                    let ivx = ((ent_x - pair.prev_ent[0]) as f64 / dt) as f32;
                    let ivy = ((ent_y - pair.prev_ent[1]) as f64 / dt) as f32;
                    let ivz = ((ent_z - pair.prev_ent[2]) as f64 / dt) as f32;
                    if ivx * ivx + ivy * ivy + ivz * ivz < 400.0 {
                        pair.vel[0] += (ivx - pair.vel[0]) * 0.3;
                        pair.vel[1] += (ivy - pair.vel[1]) * 0.3;
                        pair.vel[2] += (ivz - pair.vel[2]) * 0.3;
                    } else {
                        pair.vel = [0.0; 3];
                    }
                }
            }
            pair.prev_ent = [ent_x, ent_y, ent_z];
            pair.prev_t = now;
            pair.vel_init = true;
            *s.add(132) = pair.vel[0];
            *s.add(133) = pair.vel[1];
            *s.add(134) = pair.vel[2];
            let ts = now as f64;
            core::ptr::copy_nonoverlapping(&ts as *const f64 as *const u8, s.add(135) as *mut u8, 8);
        }
        // The clean pair on [128..131], then the entity and its sequence on [96..99].
        if g_VRCamPairValid != 0 {
            *s.add(128) = g_VRCamPairLocalX;
            *s.add(129) = g_VRCamPairLocalY;
            *s.add(130) = g_VRCamPairLocalZ;
            *s.add(131) = pair.ent_seq;
        } else {
            *s.add(131) = 0.0;
        }
        *s.add(96) = ent_x;
        *s.add(97) = ent_y;
        *s.add(98) = ent_z;
        // [CAMWRITE] the entity's world yaw (degrees) for the view's heading source.
        *s.add(153) = p_yaw;
        *s.add(99) = pair.ent_seq;
    }
    put(out, 1);
}
