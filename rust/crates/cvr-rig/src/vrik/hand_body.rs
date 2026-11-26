//! The hands' and the body's natives (formerly src/Natives/HandBody.cpp), registered by rust/crates/cvr-ffi/src/natives.rs:
//! the smoking grip's controls and its recorded-grip dump, the model/world frame pair the hand collision works in
//! (palm, view, hand stops, rotation locks), the rig measurement channel, the player's engine transform, and the
//! physics body calls made with a hand-built CStack (entPhysicalBodyInterface's velocity bindings have empty
//! descriptors, so CET cannot call them; the natives behind them still run).
use super::rig::{VRIK_QuatConj, VRIK_QuatMul, VRIK_QuatNorm, VRIK_QuatRotateVec};
use core::ffi::{c_char, c_void, CStr};
use cvr_body::cmath::{cos, sin, sqrt};
use cvr_crt::{format as cformat, Arg};
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::function::{function_flags, property_flags, BaseFunction};
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, Class, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};
use std::io::Write;

type Frame = StackFrame;

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_AnimPoseTotalCalls: u64;
    static mut g_AnimPoseMatchCalls: u64;
    static mut g_AnimPoseLastBoneBuf: usize;
    // The smoking grip (src/Anim/CharacterRigState.cpp, Anim/VrikState.hpp).
    static mut g_VRSmokeFingerActive: i32;
    static mut g_VRSmokeFingerActiveL: i32;
    static mut g_VRSmokeFingerCapture: i32;
    static mut g_VRSmokeFingerCaptureL: i32;
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerHave: i32;
    static mut g_VRSmokeFingerHaveL: i32;
    static mut g_VRSmokeFingerName: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerNameL: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerRot: [[f32; 4]; 32];
    static mut g_VRSmokeFingerRotL: [[f32; 4]; 32];
    static mut g_VRSmokeFingerRotLC: [[f32; 4]; 32];
    static mut g_VRSmokeCigEnable: i32;
    static mut g_VRSmokeCigHave: i32;
    static mut g_VRSmokeCigIdx: i32;
    static mut g_VRSmokeCigLHave: i32;
    static mut g_VRSmokeCigLPos: [f32; 3];
    static mut g_VRSmokeCigLRot: [f32; 4];
    static mut g_VRSmokeCigOffP: [f32; 3];
    static mut g_VRSmokeCigOffQ: [f32; 4];
    static mut g_VRSmokeCigPos: [f32; 3];
    static mut g_VRSmokeCigRot: [f32; 4];
    static mut g_VRSmokeCigScaleY: f32;
    static mut g_VRSmokeLeftUseCig: i32;
    static mut g_VRSmokeRightLighter: i32;
    static mut g_VRSmokeLighterEnable: i32;
    static mut g_VRSmokeLighterHave: i32;
    static mut g_VRSmokeLighterIdx: i32;
    static mut g_VRSmokeLighterOffP: [f32; 3];
    static mut g_VRSmokeLighterOffQ: [f32; 4];
    static mut g_VRSmokeLighterPos: [f32; 3];
    static mut g_VRSmokeLighterRot: [f32; 4];
    static mut g_VRSmokeThumbFlickL: [f32; 4];
    static mut g_VRSmokeThumbPressManualL: f32;
    static mut g_VRSmokeMouthAnchor: i32;
    static mut g_VRSmokeMouthDist: f32;
    static mut g_VRSmokeMouthDistL: f32;
    static mut g_VRSmokeMouthPos: [f32; 3];
    static mut g_VRSmokeMouthRot: [f32; 4];
    static mut g_VRSmokeMouthWorldPos: [f32; 3];
    static mut g_VRSmokeMouthWorldRot: [f32; 4];
    static mut g_VRSmokeMouthWorldValid: i32;
    static mut g_VRSmokeAnchorValid: i32;
    static mut g_VRSmokeAltAnchorValid: i32;
    static mut g_VRSmokeAnchorBoneSel: i32;
    static mut g_VRSmokeAnchorBoneIdx: i32;
    static mut g_VRSmokeSmokePos: [f32; 3];
    static mut g_VRSmokeSmokeRot: [f32; 4];
    // The frame pair and the hands.
    static mut g_VRViewWorldPos: [f32; 3];
    static mut g_VRViewWorldRot: [f32; 4];
    static mut g_VRViewWorldValid: i32;
    static mut g_VRCamModelPos: [f32; 3];
    static mut g_VRCamModelRot: [f32; 4];
    static mut g_VRPalmModelValid: i32;
    static mut g_VRPalmModelR: [f32; 3];
    static mut g_VRPalmModelL: [f32; 3];
    static mut g_VRPalmModelRotR: [f32; 4];
    static mut g_VRPalmModelRotL: [f32; 4];
    static mut g_VRPalmRIdx: i32;
    static mut g_VRPalmLIdx: i32;
    static mut g_VRScaleR: f32;
    static mut g_VRScaleL: f32;
    static mut g_VRHandStopValid: [i32; 2];
    static mut g_VRHandStopModel: [[f32; 3]; 2];
    static mut g_VRHandStopDeadband: f32;
    static mut g_VRHandRawValid: [i32; 2];
    static mut g_VRHandRawModel: [[f32; 3]; 2];
    static mut g_VRHandRawRot: [[f32; 4]; 2];
    static mut g_VRHandRotLock: [[f32; 4]; 2];
    static mut g_VRHandRotLockOn: [i32; 2];
    static mut g_VRBodyBone: [[f32; 3]; 11];
    static mut g_VRBodyBoneOk: [i32; 11];
    static mut g_VRFKSnapCount: i32;
    static mut g_VRFKSnapPos: [[f32; 3]; super::MAX_BONES];
    static mut g_VRFKSnapRot: [[f32; 4]; super::MAX_BONES];
    // The player's transform at the body-yaw store site (src/Hooks/BodyYawCensus.cpp).
    static CyberpunkVR_PlayerEntityPos: [f32; 3];
    static CyberpunkVR_PlayerEntityQuat: [f32; 4];
    static CyberpunkVR_PlayerEntityValid: i32;
    // The C++ half (src/Anim/CharacterRigRust.cpp, src/Hooks/InputServices.cpp).
    fn cvr_anim_player_animated_object(component: *const c_char) -> *mut c_void;
    fn cvr_cpp_xr_hand_motion(hand: i32, linear: *mut f32, angular: *mut f32) -> bool;
}

/// RED4ext::Vector4 / Quaternion / Transform.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Quaternion {
    i: f32,
    j: f32,
    k: f32,
    r: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Transform {
    position: Vector4,
    orientation: Quaternion,
}
const _: () = assert!(core::mem::size_of::<Transform>() == 32);

const ZERO4: Vector4 = Vector4 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    w: 0.0,
};
fn v4(x: f32, y: f32, z: f32, w: f32) -> Vector4 {
    Vector4 { x, y, z, w }
}
fn q4(q: [f32; 4]) -> Quaternion {
    Quaternion {
        i: q[0],
        j: q[1],
        k: q[2],
        r: q[3],
    }
}

fn mul(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    let mut o = [0.0f32; 4];
    // SAFETY: four floats in, four out.
    unsafe { VRIK_QuatMul(a.as_ptr(), b.as_ptr(), o.as_mut_ptr()) };
    o
}
fn conj(q: &[f32; 4]) -> [f32; 4] {
    let mut o = [0.0f32; 4];
    // SAFETY: as mul.
    unsafe { VRIK_QuatConj(q.as_ptr(), o.as_mut_ptr()) };
    o
}
fn norm(mut q: [f32; 4]) -> [f32; 4] {
    // SAFETY: as mul.
    unsafe { VRIK_QuatNorm(q.as_mut_ptr()) };
    q
}
fn rotate(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let mut o = [0.0f32; 3];
    // SAFETY: as mul.
    unsafe { VRIK_QuatRotateVec(q.as_ptr(), v.as_ptr(), o.as_mut_ptr()) };
    o
}
/// Euler (pitch, yaw, roll) degrees to a quaternion, the XYZ convention of SetVRHandOffset.
fn euler_quat(pitch: f32, yaw: f32, roll: f32) -> [f32; 4] {
    let d2r = 0.01745329252f32 * 0.5;
    let (cp, sp) = (cos(pitch * d2r), sin(pitch * d2r));
    let (cy, sy) = (cos(yaw * d2r), sin(yaw * d2r));
    let (cr, sr) = (cos(roll * d2r), sin(roll * d2r));
    [
        sp * cy * cr + cp * sy * sr,
        cp * sy * cr - sp * cy * sr,
        cp * cy * sr + sp * sy * cr,
        cp * cy * cr - sp * sy * sr,
    ]
}

/// The arguments, read in order, then past ParamEnd.
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

// ---- the pose hook's counters, the arm resolve ---------------------------------------------------------------

/// ArmVRAnimPosePlayer() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn ArmVRAnimPosePlayer(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let r = super::vrik_arming::VRIK_DoArmPlayer();
    put(out, r);
}
/// GetVRAnimPoseStats(mode) -> Int32: 0 total calls, 1 player-match calls, 2 the last matched bone buffer (low 32).
#[no_mangle]
pub unsafe extern "C" fn GetVRAnimPoseStats(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut mode = 0i32;
    params!(frame, mode);
    let v: u64 = match mode {
        1 => g_AnimPoseMatchCalls,
        2 => g_AnimPoseLastBoneBuf as u64,
        _ => g_AnimPoseTotalCalls,
    };
    put(out, (v & 0xFFFF_FFFF) as u32 as i32);
}

fn log_file(name: &str) -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Log, name)
}
fn grip_file(name: &str) -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Grips, name)
}

/// DumpPlayerBoneNames() -> Int32: every metaRig bone name with its index (log\player_bone_names.txt); -1 without
/// the player's rig.
#[no_mangle]
pub unsafe extern "C" fn DumpPlayerBoneNames(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    put(out, 0);
    let file = std::fs::File::create(log_file("player_bone_names.txt")).ok();
    let object = cvr_anim_player_animated_object(c"root".as_ptr());
    // anim::AnimatedObject::metaRig (+0x08); anim::MetaRig::boneNames (+0x20).
    let meta_rig = if object.is_null() {
        core::ptr::null()
    } else {
        *((object as *const u8).add(0x08) as *const *const u8)
    };
    if meta_rig.is_null() {
        put(out, -1);
        return;
    }
    let names = &*(meta_rig.add(0x20) as *const DynArray<CName>);
    let names = names.as_slice();
    if let Some(mut f) = file {
        let mut text = format!("boneCount={}\n", names.len());
        for (i, name) in names.iter().enumerate() {
            let t = name.text();
            text.push_str(&format!("{}\t{}\n", i, if t.is_empty() && !name.is_none() { "<null>" } else { t }));
        }
        let _ = f.write_all(text_mode(&text).as_bytes());
    }
    put(out, names.len() as i32);
}

// ---- the smoking grip ----------------------------------------------------------------------------------------

/// SetVRSmokeFingers(active) -> Int32: replay the captured finger curl every player pose pass (1), or release (0).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeFingers(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut active = 0i32;
    params!(frame, active);
    g_VRSmokeFingerActive = (active != 0) as i32;
    put(out, g_VRSmokeFingerActive);
}
/// VRSmokeCaptureFingers() -> Int32: latch the right hand's finger locals on the next pose pass; the finger-bone
/// count (0: not resolved yet).
#[no_mangle]
pub unsafe extern "C" fn VRSmokeCaptureFingers(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    g_VRSmokeFingerCapture = 1;
    put(out, g_VRSmokeFingerCount);
}

/// "%.9g" of each value, as the C runtime prints them (the "C" locale's '.').
fn g9(values: &[f32]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|&v| cformat(c"%.9g", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string())
        .collect();
    parts.join(" ")
}
fn finger_name(name: &[c_char; 48]) -> String {
    // SAFETY: a NUL-terminated name in a 48-byte slot (the last byte is never written past).
    let bytes: Vec<u8> = name.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
/// What std::ofstream in text mode writes: each line feed as CR LF.
pub(crate) fn text_mode(text: &str) -> String {
    text.replace('\n', "\r\n")
}
fn write_file(path: std::path::PathBuf, text: &str) -> bool {
    std::fs::File::create(path).and_then(|mut f| f.write_all(text_mode(text).as_bytes())).is_ok()
}

/// VRSmokeDumpFingers() -> Int32: the captured grip poses written to the grips folder (name-keyed lines, so they reload
/// on any skeleton; the live cig nudge baked into the saved transform); the number of lines written.
#[no_mangle]
pub unsafe extern "C" fn VRSmokeDumpFingers(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let mut written = 0;
    // RIGHT hand (cigarette).
    if g_VRSmokeFingerHave != 0 || g_VRSmokeCigHave != 0 {
        let mut text = String::from("# CyberpunkVR RIGHT-hand cigarette grip v1 (auto-generated by VRSmokeDumpFingers)\n");
        text.push_str("# F <bone> qx qy qz qw    |    C WeaponRight px py pz qx qy qz qw\n");
        let mut lines = 0;
        if g_VRSmokeFingerHave != 0 {
            for k in 0..(g_VRSmokeFingerCount.clamp(0, 32) as usize) {
                if g_VRSmokeFingerName[k][0] == 0 {
                    continue;
                }
                text.push_str(&format!("F {} {}\n", finger_name(&g_VRSmokeFingerName[k]), g9(&g_VRSmokeFingerRot[k])));
                lines += 1;
            }
        }
        if g_VRSmokeCigHave != 0 && g_VRSmokeCigIdx >= 0 {
            let pos = [
                g_VRSmokeCigPos[0] + g_VRSmokeCigOffP[0],
                g_VRSmokeCigPos[1] + g_VRSmokeCigOffP[1],
                g_VRSmokeCigPos[2] + g_VRSmokeCigOffP[2],
            ];
            let rot = norm(mul(&g_VRSmokeCigRot, &g_VRSmokeCigOffQ));
            text.push_str(&format!("C WeaponRight {} {}\n", g9(&pos), g9(&rot)));
            lines += 1;
        }
        if write_file(grip_file("CyberpunkVR_SmokeGrip_right.ini"), &text) {
            written += lines;
        }
    }
    // LEFT hand (lighter).
    if g_VRSmokeFingerHaveL != 0 || g_VRSmokeLighterHave != 0 {
        let mut text = String::from("# CyberpunkVR LEFT-hand lighter grip v1 (auto-generated by VRSmokeDumpFingers)\n");
        text.push_str("# F <bone> qx qy qz qw | C WeaponLeft px py pz qx qy qz qw | K LeftThumbFlick qx qy qz qw\n");
        let mut lines = 0;
        if g_VRSmokeFingerHaveL != 0 {
            for k in 0..(g_VRSmokeFingerCountL.clamp(0, 32) as usize) {
                if g_VRSmokeFingerNameL[k][0] == 0 {
                    continue;
                }
                text.push_str(&format!("F {} {}\n", finger_name(&g_VRSmokeFingerNameL[k]), g9(&g_VRSmokeFingerRotL[k])));
                lines += 1;
            }
        }
        if g_VRSmokeLighterHave != 0 && g_VRSmokeLighterIdx >= 0 {
            let pos = [
                g_VRSmokeLighterPos[0] + g_VRSmokeLighterOffP[0],
                g_VRSmokeLighterPos[1] + g_VRSmokeLighterOffP[1],
                g_VRSmokeLighterPos[2] + g_VRSmokeLighterOffP[2],
            ];
            let rot = norm(mul(&g_VRSmokeLighterRot, &g_VRSmokeLighterOffQ));
            text.push_str(&format!("C WeaponLeft {} {}\n", g9(&pos), g9(&rot)));
            lines += 1;
        }
        if g_VRSmokeFingerHaveL != 0 {
            text.push_str(&format!("K LeftThumbFlick {}\n", g9(&g_VRSmokeThumbFlickL)));
            lines += 1;
        }
        if write_file(grip_file("CyberpunkVR_LighterGrip_Left.ini"), &text) {
            written += lines;
        }
    }
    // LEFT hand (cigarette).
    if g_VRSmokeCigLHave != 0 {
        let mut text = String::from("# CyberpunkVR LEFT-hand cigarette grip v1 (auto-generated by VRSmokeDumpFingers)\n");
        text.push_str("# F <bone> qx qy qz qw | C WeaponLeft px py pz qx qy qz qw\n");
        let mut lines = 0;
        for k in 0..(g_VRSmokeFingerCountL.clamp(0, 32) as usize) {
            if g_VRSmokeFingerNameL[k][0] == 0 {
                continue;
            }
            text.push_str(&format!("F {} {}\n", finger_name(&g_VRSmokeFingerNameL[k]), g9(&g_VRSmokeFingerRotLC[k])));
            lines += 1;
        }
        if g_VRSmokeLighterIdx >= 0 {
            text.push_str(&format!("C WeaponLeft {} {}\n", g9(&g_VRSmokeCigLPos), g9(&g_VRSmokeCigLRot)));
            lines += 1;
        }
        if write_file(grip_file("CyberpunkVR_SmokeGrip_Left.ini"), &text) {
            written += lines;
        }
    }
    put(out, written);
}
/// SetVRSmokeCig(enable) -> Int32: also place the WeaponRight (cig) slot when the grip is applied.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeCig(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut enable = 1i32;
    params!(frame, enable);
    g_VRSmokeCigEnable = (enable != 0) as i32;
    put(out, g_VRSmokeCigEnable);
}
/// VRSmokeMouthDist() -> Float: model-space distance from the cig slot to the mouth (the VRIK body solve's).
#[no_mangle]
pub unsafe extern "C" fn VRSmokeMouthDist(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    params!(frame);
    put(out, g_VRSmokeMouthDist);
}
/// VRSmokeMouthDistL() -> Float: the same for the left hand.
#[no_mangle]
pub unsafe extern "C" fn VRSmokeMouthDistL(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    params!(frame);
    put(out, g_VRSmokeMouthDistL);
}
/// x, y, z, pitch, yaw, roll: the live-tuning natives' six floats.
unsafe fn six(frame: *mut Frame) -> ([f32; 3], [f32; 4]) {
    let (mut x, mut y, mut z, mut pitch, mut yaw, mut roll) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z, pitch, yaw, roll);
    ([x, y, z], euler_quat(pitch, yaw, roll))
}
/// SetVRSmokeCigOffset(x, y, z, pitch, yaw, roll) -> Int32: live nudge of the cig in the hand (bone-local metres,
/// degrees).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeCigOffset(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (p, q) = six(frame);
    g_VRSmokeCigOffP = p;
    g_VRSmokeCigOffQ = q;
    put(out, 1);
}

// ---- the model/world frame pair -------------------------------------------------------------------------------

/// VRViewWorldPos() -> Vector4: the rendered view pose in world space (W = valid).
#[no_mangle]
pub unsafe extern "C" fn VRViewWorldPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    params!(frame);
    put(
        out,
        v4(g_VRViewWorldPos[0], g_VRViewWorldPos[1], g_VRViewWorldPos[2], g_VRViewWorldValid as f32),
    );
}
/// VRViewWorldPos's answer: the rendered view pose in world space (W = valid).
///
/// # Safety
/// Reads the view pose the frame publishes.
pub unsafe fn view_world_pos() -> [f32; 4] {
    [g_VRViewWorldPos[0], g_VRViewWorldPos[1], g_VRViewWorldPos[2], g_VRViewWorldValid as f32]
}
/// model -> world: viewWorldRot * conj(camModelRot), anchored at viewWorldPos / camModelPos.
unsafe fn model_to_world() -> [f32; 4] {
    norm(mul(&g_VRViewWorldRot, &conj(&g_VRCamModelRot)))
}
/// A model-space point in world space through the frame pair.
unsafe fn world_of(model: [f32; 3]) -> Vector4 {
    let rel = [model[0] - g_VRCamModelPos[0], model[1] - g_VRCamModelPos[1], model[2] - g_VRCamModelPos[2]];
    let w = rotate(&model_to_world(), &rel);
    v4(g_VRViewWorldPos[0] + w[0], g_VRViewWorldPos[1] + w[1], g_VRViewWorldPos[2] + w[2], 1.0)
}
unsafe fn palm(right: bool) -> [f32; 3] {
    if right {
        g_VRPalmModelR
    } else {
        g_VRPalmModelL
    }
}
/// VRPalmWorldPos(right) -> Vector4: the solved palm in world space (W = 1 once the skeleton was solved).
#[no_mangle]
pub unsafe extern "C" fn VRPalmWorldPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    if g_VRPalmModelValid == 0 || g_VRViewWorldValid == 0 {
        return;
    }
    let idx = if right != 0 { g_VRPalmRIdx } else { g_VRPalmLIdx };
    if idx < 0 {
        return;
    }
    *out = world_of(palm(right != 0));
}
/// VRHandStop(right, active, world) -> Bool: a physics-resolved hand position (world) for the arm solve to stop at.
#[no_mangle]
pub unsafe extern "C" fn VRHandStop(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut right, mut active, mut world) = (1i32, false, Vector4::default());
    params!(frame, right, active, world);
    let side = if right != 0 { 1 } else { 0 };
    put(out, false);
    if !active || g_VRPalmModelValid == 0 || g_VRViewWorldValid == 0 {
        g_VRHandStopValid[side] = 0;
        return;
    }
    let w2m = conj(&model_to_world());
    let rel = [world.x - g_VRViewWorldPos[0], world.y - g_VRViewWorldPos[1], world.z - g_VRViewWorldPos[2]];
    let m = rotate(&w2m, &rel);
    g_VRHandStopModel[side] = [g_VRCamModelPos[0] + m[0], g_VRCamModelPos[1] + m[1], g_VRCamModelPos[2] + m[2]];
    g_VRHandStopValid[side] = 1;
    put(out, true);
}
/// World -> model with the same frame pair: (rotation, world anchor, model anchor), or None before it is ready.
unsafe fn world_model_frame() -> Option<([f32; 4], [f32; 3], [f32; 3])> {
    if g_VRPalmModelValid == 0 || g_VRViewWorldValid == 0 {
        return None;
    }
    Some((conj(&model_to_world()), g_VRViewWorldPos, g_VRCamModelPos))
}
/// VRWorldToModel(world) -> Vector4 (W = 1 when the frame pair is ready).
#[no_mangle]
pub unsafe extern "C" fn VRWorldToModel(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut world = Vector4::default();
    params!(frame, world);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let Some((w2m, aw, am)) = world_model_frame() else { return };
    let m = rotate(&w2m, &[world.x - aw[0], world.y - aw[1], world.z - aw[2]]);
    *out = v4(am[0] + m[0], am[1] + m[1], am[2] + m[2], 1.0);
}
/// VRWorldDirToModel(dir) -> Vector4: a direction, rotation only.
#[no_mangle]
pub unsafe extern "C" fn VRWorldDirToModel(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut dir = Vector4::default();
    params!(frame, dir);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let Some((w2m, _, _)) = world_model_frame() else { return };
    let m = rotate(&w2m, &[dir.x, dir.y, dir.z]);
    *out = v4(m[0], m[1], m[2], 1.0);
}
/// VRHandRawModel(right) -> Vector4: where tracking alone puts the hand, model space.
#[no_mangle]
pub unsafe extern "C" fn VRHandRawModel(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let side = if right != 0 { 1 } else { 0 };
    if g_VRHandRawValid[side] == 0 {
        return;
    }
    let p = g_VRHandRawModel[side];
    *out = v4(p[0], p[1], p[2], 1.0);
}
/// VRHandRawRot(right) -> Quaternion: the tracked hand rotation, model space.
#[no_mangle]
pub unsafe extern "C" fn VRHandRawRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    let side = if right != 0 { 1 } else { 0 };
    put(out, q4(g_VRHandRawRot[side]));
}
/// VRHandMotion(right, spin) -> Vector4: the controller's velocity (m/s at the hand's scale) or angular velocity
/// (rad/s) as the runtime measures it, in the model's axes (W = 1 when valid).
#[no_mangle]
pub unsafe extern "C" fn VRHandMotion(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let (mut right, mut spin) = (1i32, false);
    params!(frame, right, spin);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let (mut linear, mut angular) = ([0.0f32; 3], [0.0f32; 3]);
    if g_VRPalmModelValid == 0 || !cvr_cpp_xr_hand_motion(if right != 0 { 1 } else { 0 }, linear.as_mut_ptr(), angular.as_mut_ptr()) {
        return;
    }
    let v = if spin { angular } else { linear };
    let scale = if spin {
        1.0
    } else if right != 0 {
        g_VRScaleR
    } else {
        g_VRScaleL
    };
    let game = [v[0] * scale, -v[2] * scale, v[1] * scale];
    let m = rotate(&g_VRCamModelRot, &game);
    *out = v4(m[0], m[1], m[2], 1.0);
}
/// VRHandStopModel(right, active, model) -> Bool: hold the hand at a model-space position.
#[no_mangle]
pub unsafe extern "C" fn VRHandStopModel(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut right, mut active, mut model) = (1i32, false, Vector4::default());
    params!(frame, right, active, model);
    let side = if right != 0 { 1 } else { 0 };
    if !active {
        g_VRHandStopValid[side] = 0;
        put(out, true);
        return;
    }
    g_VRHandStopModel[side] = [model.x, model.y, model.z];
    g_VRHandStopValid[side] = 1;
    put(out, true);
}
/// VRHandStopRot(side, active, qi, qj, qk, qr) -> Bool: the wrist rotation lock (model space) the arm solve takes
/// instead of the controller's twist.
#[no_mangle]
pub unsafe extern "C" fn VRHandStopRot(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut side, mut active) = (0i32, 0i32);
    let (mut qi, mut qj, mut qk, mut qr) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
    params!(frame, side, active, qi, qj, qk, qr);
    put(out, false);
    if !(0..=1).contains(&side) {
        return;
    }
    let s = side as usize;
    if active == 0 {
        g_VRHandRotLockOn[s] = 0;
        put(out, true);
        return;
    }
    let len = sqrt(qi * qi + qj * qj + qk * qk + qr * qr);
    if len < 1e-4 {
        return;
    }
    g_VRHandRotLock[s] = [qi / len, qj / len, qk / len, qr / len];
    g_VRHandRotLockOn[s] = 1;
    put(out, true);
}
/// VRHandRawWorld(right) -> Vector4: where tracking alone would put the hand, world space (W = 0 before the frame
/// pair is ready).
#[no_mangle]
pub unsafe extern "C" fn VRHandRawWorld(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let side = if right != 0 { 1 } else { 0 };
    if g_VRHandRawValid[side] == 0 || g_VRPalmModelValid == 0 || g_VRViewWorldValid == 0 {
        return;
    }
    *out = world_of(g_VRHandRawModel[side]);
}
/// VRHandStopDeadband(metres) -> Bool (0..0.30).
#[no_mangle]
pub unsafe extern "C" fn VRHandStopDeadband(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let mut v = 0.02f32;
    params!(frame, v);
    if v < 0.0 {
        v = 0.0;
    }
    if v > 0.30 {
        v = 0.30;
    }
    g_VRHandStopDeadband = v;
    put(out, true);
}
/// VRPalmCamLocal(right) -> Vector4: the palm's offset from the camera in the camera's own frame (model space).
#[no_mangle]
pub unsafe extern "C" fn VRPalmCamLocal(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    if g_VRPalmModelValid == 0 {
        return;
    }
    let idx = if right != 0 { g_VRPalmRIdx } else { g_VRPalmLIdx };
    if idx < 0 {
        return;
    }
    let pm = palm(right != 0);
    let rel = [pm[0] - g_VRCamModelPos[0], pm[1] - g_VRCamModelPos[1], pm[2] - g_VRCamModelPos[2]];
    let local = rotate(&conj(&g_VRCamModelRot), &rel);
    *out = v4(local[0], local[1], local[2], 1.0);
}

// ---- the rig measurement channel ------------------------------------------------------------------------------

/// VRBoneSnapCount() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRBoneSnapCount(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    put(out, g_VRFKSnapCount);
}
/// VRBoneModelPos(index) -> Vector4: a solved bone, model space (only while SetVRDiagCapture(1) is on).
#[no_mangle]
pub unsafe extern "C" fn VRBoneModelPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut idx = -1i32;
    params!(frame, idx);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    if idx < 0 || idx >= g_VRFKSnapCount || idx as usize >= super::MAX_BONES {
        return;
    }
    let p = g_VRFKSnapPos[idx as usize];
    *out = v4(p[0], p[1], p[2], 1.0);
}
/// VRBoneModelRot(index) -> Quaternion (identity when out of range).
#[no_mangle]
pub unsafe extern "C" fn VRBoneModelRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    let mut idx = -1i32;
    params!(frame, idx);
    let Some(out) = out.as_mut() else { return };
    *out = Quaternion {
        i: 0.0,
        j: 0.0,
        k: 0.0,
        r: 1.0,
    };
    if idx < 0 || idx >= g_VRFKSnapCount || idx as usize >= super::MAX_BONES {
        return;
    }
    *out = q4(g_VRFKSnapRot[idx as usize]);
}
/// VRPalmModelPos's answer: the raw model-space palm (W = valid).
///
/// # Safety
/// The palm the frame publishes.
pub unsafe fn palm_model_pos(right: bool) -> [f32; 4] {
    let pm = palm(right);
    [pm[0], pm[1], pm[2], if g_VRPalmModelValid != 0 { 1.0 } else { 0.0 }]
}
/// VRPalmModelRot's answer: the solved hand's orientation, model space (i, j, k, r).
///
/// # Safety
/// As above.
pub unsafe fn palm_model_rot(right: bool) -> [f32; 4] {
    if right {
        g_VRPalmModelRotR
    } else {
        g_VRPalmModelRotL
    }
}
/// VRPalmModelPos(right) -> Vector4: the raw model-space palm (W = valid).
#[no_mangle]
pub unsafe extern "C" fn VRPalmModelPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    let pm = palm(right != 0);
    put(out, v4(pm[0], pm[1], pm[2], if g_VRPalmModelValid != 0 { 1.0 } else { 0.0 }));
}
/// VRPalmModelRot(right) -> Quaternion: the solved hand's orientation, model space.
#[no_mangle]
pub unsafe extern "C" fn VRPalmModelRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    let mut right = 1i32;
    params!(frame, right);
    put(out, q4(if right != 0 { g_VRPalmModelRotR } else { g_VRPalmModelRotL }));
}
/// VRBodyBonePos(slot) -> Vector4: 0 hips, 1 spine, 2 chest, 3 neck, 4 head, 5-7 left leg, 8-10 right leg.
#[no_mangle]
pub unsafe extern "C" fn VRBodyBonePos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut slot = 0i32;
    params!(frame, slot);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    if !(0..=10).contains(&slot) || g_VRBodyBoneOk[slot as usize] == 0 {
        return;
    }
    let p = g_VRBodyBone[slot as usize];
    *out = v4(p[0], p[1], p[2], 1.0);
}
/// VRCamModelPos() -> Vector4 (W = the palm model is valid).
#[no_mangle]
pub unsafe extern "C" fn VRCamModelPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    params!(frame);
    put(
        out,
        v4(
            g_VRCamModelPos[0],
            g_VRCamModelPos[1],
            g_VRCamModelPos[2],
            if g_VRPalmModelValid != 0 { 1.0 } else { 0.0 },
        ),
    );
}

// ---- the player's own transform, as the engine has it this frame ---------------------------------------------

/// VRPlayerEnginePos() -> Vector4 (W = 1 once the store site identified the player).
#[no_mangle]
pub unsafe extern "C" fn VRPlayerEnginePos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    params!(frame);
    let p = CyberpunkVR_PlayerEntityPos;
    put(out, v4(p[0], p[1], p[2], if CyberpunkVR_PlayerEntityValid != 0 { 1.0 } else { 0.0 }));
}
/// VRPlayerEngineRot() -> Quaternion: the same instant's body orientation (yaw only).
#[no_mangle]
pub unsafe extern "C" fn VRPlayerEngineRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    params!(frame);
    put(out, q4(CyberpunkVR_PlayerEntityQuat));
}
/// VRViewWorldRot() -> Quaternion.
#[no_mangle]
pub unsafe extern "C" fn VRViewWorldRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    params!(frame);
    put(out, q4(g_VRViewWorldRot));
}
/// VRSmokeMouthWorldPos() -> Vector4: the exhale smoke's world mouth point (W = valid).
#[no_mangle]
pub unsafe extern "C" fn VRSmokeMouthWorldPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    params!(frame);
    let p = g_VRSmokeMouthWorldPos;
    put(out, v4(p[0], p[1], p[2], g_VRSmokeMouthWorldValid as f32));
}
/// VRSmokeMouthWorldRot() -> Quaternion.
#[no_mangle]
pub unsafe extern "C" fn VRSmokeMouthWorldRot(_this: *mut c_void, frame: *mut Frame, out: *mut Quaternion, _unused: i64) {
    params!(frame);
    put(out, q4(g_VRSmokeMouthWorldRot));
}
/// SetVRSmokeSmokeOffset(x, y, z, pitch, yaw, roll) -> Int32: the exhale smoke, HMD-local.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeSmokeOffset(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (p, q) = six(frame);
    g_VRSmokeSmokePos = p;
    g_VRSmokeSmokeRot = q;
    put(out, 1);
}
/// SetVRSmokeCigScaleY(y) -> Int32: the cig slot bone's long-axis scale (burn-down), at least 0.02.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeCigScaleY(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut y = 1.0f32;
    params!(frame, y);
    if y < 0.02 {
        y = 0.02;
    }
    g_VRSmokeCigScaleY = y;
    put(out, 1);
}

// ---- an entity's mesh components ------------------------------------------------------------------------------

const MESH_COMPONENT: CName = CName::hashed("entMeshComponent");
const SKINNED_MESH_COMPONENT: CName = CName::hashed("entSkinnedMeshComponent");
const GARMENT_SKINNED_MESH_COMPONENT: CName = CName::hashed("entGarmentSkinnedMeshComponent");
/// entMeshComponent::visualScale (+0x178, Vector3), chunkMask (+0x198); entSkinnedMeshComponent::chunkMask (+0x248).
const MESH_VISUAL_SCALE_Y: usize = 0x178 + 4;
const MESH_CHUNK_MASK: usize = 0x198;
const SKINNED_CHUNK_MASK: usize = 0x248;

/// A handle as stored in an array (instance, reference count), read without touching the count.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawHandle {
    instance: *mut u8,
    ref_count: *mut c_void,
}
/// An entity's components with their classes (ent::Entity::components, +0xA0).
unsafe fn components(entity: *mut c_void) -> Vec<(*mut u8, &'static Class)> {
    let list = &*((entity as *const u8).add(0xA0) as *const DynArray<RawHandle>);
    list.as_slice()
        .iter()
        .filter(|h| !h.instance.is_null())
        .filter_map(|h| object_class(h.instance as *const c_void).map(|c| (h.instance, c)))
        .collect()
}
/// SetVRSmokeCigChunks(cig, count) -> Int32: only the low `count` mesh chunks shown (<= 0 or >= 64: all); the mesh
/// components touched, -1 without an entity.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeCigChunks(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut cig, mut count) = (Handle::null(), 0i32);
    params!(frame, cig, count);
    let mask: u64 = if count <= 0 || count >= 64 { u64::MAX } else { (1u64 << count) - 1 };
    if cig.instance.is_null() {
        put(out, -1);
        return;
    }
    let mut hit = 0;
    for (component, class) in components(cig.instance as *mut c_void) {
        if class.is_a(SKINNED_MESH_COMPONENT) || class.is_a(GARMENT_SKINNED_MESH_COMPONENT) {
            *(component.add(SKINNED_CHUNK_MASK) as *mut u64) = mask;
            hit += 1;
        } else if class.is_a(MESH_COMPONENT) {
            *(component.add(MESH_CHUNK_MASK) as *mut u64) = mask;
            hit += 1;
        }
    }
    put(out, hit);
}
/// SetVRSmokeCigVisualScale(cig, y) -> Int32: visualScale.Y of every entMeshComponent (X/Z kept); the components hit,
/// -1 without an entity.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeCigVisualScale(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut cig, mut y) = (Handle::null(), 1.0f32);
    params!(frame, cig, y);
    if y < 0.02 {
        y = 0.02;
    }
    if cig.instance.is_null() {
        put(out, -1);
        return;
    }
    let mut hit = 0;
    for (component, class) in components(cig.instance as *mut c_void) {
        if class.is_a(MESH_COMPONENT) {
            *(component.add(MESH_VISUAL_SCALE_Y) as *mut f32) = y;
            hit += 1;
        }
    }
    put(out, hit);
}
/// GetVRSmokeCigVisualScaleY(cig) -> Float: the first entMeshComponent's visualScale.Y, or -1.
#[no_mangle]
pub unsafe extern "C" fn GetVRSmokeCigVisualScaleY(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let mut cig = Handle::null();
    params!(frame, cig);
    let mut result = -1.0f32;
    if !cig.instance.is_null() {
        if let Some((component, _)) = components(cig.instance as *mut c_void)
            .into_iter()
            .find(|(_, class)| class.is_a(MESH_COMPONENT))
        {
            result = *(component.add(MESH_VISUAL_SCALE_Y) as *const f32);
        }
    }
    put(out, result);
}

// ---- the mouth anchor and the left hand -----------------------------------------------------------------------

/// SetVRSmokeMouthAnchor(on) -> Int32: pin the cig to the mouth (1) or back to the hand grip (0).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeMouthAnchor(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    g_VRSmokeMouthAnchor = (on != 0) as i32;
    if g_VRSmokeMouthAnchor == 0 {
        g_VRSmokeAnchorValid = 0;
        g_VRSmokeAltAnchorValid = 0;
    }
    put(out, g_VRSmokeMouthAnchor);
}
/// SetVRSmokeAnchorBone(sel) -> Int32: which bone the mouth-pin drives (0 off, 1 Neck1, 2 Head, 3 Neck).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeAnchorBone(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut sel = 0i32;
    params!(frame, sel);
    g_VRSmokeAnchorBoneSel = sel;
    if sel == 0 {
        g_VRSmokeAnchorBoneIdx = -1;
        g_VRSmokeAltAnchorValid = 0;
    }
    put(out, sel);
}
/// SetVRSmokeMouthOffset(x, y, z, pitch, yaw, roll) -> Int32: the mouth-anchored cig (model space from the head bone).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeMouthOffset(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (p, q) = six(frame);
    g_VRSmokeMouthPos = p;
    g_VRSmokeMouthRot = q;
    put(out, 1);
}
/// SetVRSmokeFingersL(active) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeFingersL(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut active = 0i32;
    params!(frame, active);
    g_VRSmokeFingerActiveL = (active != 0) as i32;
    put(out, g_VRSmokeFingerActiveL);
}
/// SetVRSmokeLeftCig(on) -> Int32: the left hand holds the cigarette (1) or the lighter (0).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeLeftCig(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    g_VRSmokeLeftUseCig = (on != 0) as i32;
    put(out, g_VRSmokeLeftUseCig);
}
/// SetVRSmokeRightLighter(on) -> Int32: the right hand holds the lighter (left-handed smoking).
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeRightLighter(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    g_VRSmokeRightLighter = (on != 0) as i32;
    put(out, g_VRSmokeRightLighter);
}
/// VRSmokeCaptureFingersL() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRSmokeCaptureFingersL(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    g_VRSmokeFingerCaptureL = 1;
    put(out, g_VRSmokeFingerCountL);
}
/// SetVRSmokeLighter(enable) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeLighter(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut enable = 1i32;
    params!(frame, enable);
    g_VRSmokeLighterEnable = (enable != 0) as i32;
    put(out, g_VRSmokeLighterEnable);
}
/// SetVRSmokeLighterOffset(x, y, z, pitch, yaw, roll) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeLighterOffset(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (p, q) = six(frame);
    g_VRSmokeLighterOffP = p;
    g_VRSmokeLighterOffQ = q;
    put(out, 1);
}
/// SetVRSmokeThumbFlickL(pitch, yaw, roll) -> Int32: the left thumb's full-press rotation delta.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeThumbFlickL(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut pitch, mut yaw, mut roll) = (0.0f32, 0.0f32, 0.0f32);
    params!(frame, pitch, yaw, roll);
    g_VRSmokeThumbFlickL = euler_quat(pitch, yaw, roll);
    put(out, 1);
}
/// SetVRSmokeThumbPressL(amount) -> Int32: the manual press override, 0..1.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmokeThumbPressL(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut amount = 0.0f32;
    params!(frame, amount);
    if amount < 0.0 {
        amount = 0.0;
    } else if amount > 1.0 {
        amount = 1.0;
    }
    g_VRSmokeThumbPressManualL = amount;
    put(out, 1);
}

// main's additions: the body a ray hit, the near and pointed-at bodies, a body lying dead, a body held at a point
#[path = "hand_body_physics.rs"]
mod physics;

// ---- the physics body's bindings, called with a hand-built CStack ---------------------------------------------

/// entPhysicalBodyInterface (`PhysicalBodyInterface` is only the redscript alias).
fn body_class(rtti: &RttiSystem) -> Option<&'static Class> {
    rtti.get_class(CName::hashed("entPhysicalBodyInterface"))
        .or_else(|| rtti.get_class(CName::hashed("PhysicalBodyInterface")))
}
/// The class's member function by short name.
fn body_func(name: &str) -> Option<&'static BaseFunction> {
    let rtti = RttiSystem::get()?;
    let want = CName::hashed(name);
    body_class(rtti)?
        .funcs()
        .iter()
        .filter_map(|&f| unsafe { f.as_ref() })
        .find(|f| f.short_name == want)
}
/// `f->Execute(stack)` under SEH: a call whose descriptor says "no parameters" made with one can fault inside the
/// engine's marshaller, and the probe reports that instead of taking the game down.
unsafe fn safe_execute(f: &BaseFunction, stack: &mut CStack) -> bool {
    let Some(address) = f.execute_address() else { return false };
    cvr_engine::code::guarded_call3(
        address,
        f as *const BaseFunction as *mut c_void,
        stack as *mut CStack as *mut c_void,
        core::ptr::null_mut(),
    )
    .unwrap_or(false)
}
/// A getter called on `body`, its result (of the RTTI type `ret`) copied out of an oversized buffer.
unsafe fn body_call_get(body: *mut c_void, name: &str, ret: &CStr, out: &mut [u8]) -> bool {
    if body.is_null() {
        return false;
    }
    if physics::body_broken(body) {
        return false;
    }
    let (Some(f), Some(rtti)) = (body_func(name), RttiSystem::get()) else {
        return false;
    };
    #[repr(C, align(16))]
    struct Buffer([u8; 64]);
    let mut buffer = Buffer([0; 64]);
    let ty = rtti
        .get_type(CName::hashed(ret.to_str().unwrap_or("")))
        .map_or(core::ptr::null(), |t| t as *const _);
    let mut result = StackType {
        ty,
        value: buffer.0.as_mut_ptr() as *mut c_void,
    };
    let Some(mut stack) = CStack::new(rtti, body, &mut [], &mut result) else {
        return false;
    };
    if !safe_execute(f, &mut stack) {
        return false;
    }
    let n = out.len().min(64);
    out[..n].copy_from_slice(&buffer.0[..n]);
    true
}
/// A setter called on `body` with one argument of the RTTI type `ty`.
unsafe fn body_call_set<T>(body: *mut c_void, name: &str, ty: &CStr, value: &mut T) -> bool {
    if body.is_null() {
        return false;
    }
    if physics::body_broken(body) {
        return false;
    }
    let (Some(f), Some(rtti)) = (body_func(name), RttiSystem::get()) else {
        return false;
    };
    let ty = rtti
        .get_type(CName::hashed(ty.to_str().unwrap_or("")))
        .map_or(core::ptr::null(), |t| t as *const _);
    let mut args = [StackType {
        ty,
        value: value as *mut T as *mut c_void,
    }];
    let Some(mut stack) = CStack::new(rtti, body, &mut args, core::ptr::null_mut()) else {
        return false;
    };
    safe_execute(f, &mut stack)
}
fn as_bytes<T>(value: &mut T) -> &mut [u8] {
    // SAFETY: plain data viewed as its bytes.
    unsafe { core::slice::from_raw_parts_mut(value as *mut T as *mut u8, core::mem::size_of::<T>()) }
}
/// A Vector4 getter on the body handle in the frame (W = 1 when the call went through).
unsafe fn body_vector(frame: *mut Frame, out: *mut Vector4, name: &str) {
    let mut body = Handle::null();
    params!(frame, body);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let mut v = Vector4::default();
    if body_call_get(body.instance as *mut c_void, name, c"Vector4", as_bytes(&mut v)) {
        *out = v4(v.x, v.y, v.z, 1.0);
    }
}
/// VRBodyGetVel(body) -> Vector4: the body's own linear velocity.
#[no_mangle]
pub unsafe extern "C" fn VRBodyGetVel(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    body_vector(frame, out, "GetLinearVelocity");
}
/// VRBodyGetAngVel(body) -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBodyGetAngVel(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    body_vector(frame, out, "GetAngularVelocity");
}
/// VRBodyGetDims(body) -> Vector4: the body's shape.
#[no_mangle]
pub unsafe extern "C" fn VRBodyGetDims(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    body_vector(frame, out, "GetDimensions");
}
/// VRBodyGetCOM(body) -> Vector4: the bounds' centre.
#[no_mangle]
pub unsafe extern "C" fn VRBodyGetCOM(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    body_vector(frame, out, "GetBoundsCenter");
}
/// VRBodySetVel(body, velocity) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBodySetVel(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut body, mut v) = (Handle::null(), Vector4::default());
    params!(frame, body, v);
    let ok = body_call_set(body.instance as *mut c_void, "SetLinearVelocity", c"Vector4", &mut v);
    put(out, ok);
}
/// VRBodySetAngVel(body, velocity) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBodySetAngVel(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut body, mut v) = (Handle::null(), Vector4::default());
    params!(frame, body, v);
    let ok = body_call_set(body.instance as *mut c_void, "SetAngularVelocity", c"Vector4", &mut v);
    put(out, ok);
}
/// VRBodySetKinematic(body, kinematic) -> Bool: the body between solver-owned and driven, live.
#[no_mangle]
pub unsafe extern "C" fn VRBodySetKinematic(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut body, mut kinematic) = (Handle::null(), true);
    params!(frame, body, kinematic);
    put(out, false);
    if body.instance.is_null() {
        return;
    }
    let ok = body_call_set(body.instance as *mut c_void, "SetIsKinematic", c"Bool", &mut kinematic);
    put(out, ok);
}
/// VRBodyIsKinematic(body) -> Int32: 1/0, or -1 when the call did not go through.
#[no_mangle]
pub unsafe extern "C" fn VRBodyIsKinematic(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut body = Handle::null();
    params!(frame, body);
    put(out, -1);
    let mut k = false;
    if body_call_get(body.instance as *mut c_void, "IsKinematic", c"Bool", as_bytes(&mut k)) {
        put(out, k as i32);
    }
}
/// VRBodyGetPos(body) -> Vector4: the body's own pose (GetTransform: { position, orientation }).
#[no_mangle]
pub unsafe extern "C" fn VRBodyGetPos(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut body = Handle::null();
    params!(frame, body);
    let Some(out) = out.as_mut() else { return };
    *out = ZERO4;
    let mut t = Transform::default();
    if body_call_get(body.instance as *mut c_void, "GetTransform", c"Transform", as_bytes(&mut t)) {
        *out = v4(t.position.x, t.position.y, t.position.z, 1.0);
    }
}
/// VRBodySetPos(body, position, orientation) -> Bool: teleport.
#[no_mangle]
pub unsafe extern "C" fn VRBodySetPos(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut body, mut position, mut orientation) = (Handle::null(), Vector4::default(), Quaternion::default());
    params!(frame, body, position, orientation);
    put(out, false);
    if body.instance.is_null() {
        return;
    }
    let mut t = Transform { position, orientation };
    let ok = body_call_set(body.instance as *mut c_void, "SetTransform", c"Transform", &mut t);
    put(out, ok);
}
/// VRBodySetSimMasks(body, mask1, mask2) -> Bool: the simulation filter (physicsSimulationFilter: two Uint64).
#[no_mangle]
pub unsafe extern "C" fn VRBodySetSimMasks(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut body, mut m1, mut m2) = (Handle::null(), 0u64, 0u64);
    params!(frame, body, m1, m2);
    put(out, false);
    if body.instance.is_null() {
        return;
    }
    let mut filter = [m1, m2];
    let ok = body_call_set(body.instance as *mut c_void, "SetSimulationFilter", c"physicsSimulationFilter", &mut filter);
    put(out, ok);
}

/// `ostream << float` (precision 6, %g).
fn g(v: f32) -> String {
    cformat(c"%g", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}
fn type_name(ty: *const red4ext_abi::rtti::IType) -> Option<&'static str> {
    // SAFETY: an RTTI type of a registered function, or null.
    unsafe { ty.as_ref() }.map(|t| t.name().text())
}
/// VRBodyProbe(body) -> String: everything needed to decide whether the velocity API is usable.
#[no_mangle]
pub unsafe extern "C" fn VRBodyProbe(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _unused: i64) {
    let mut body = Handle::null();
    params!(frame, body);
    if out.is_null() {
        return;
    }
    let object = body.instance as *mut c_void;
    if object.is_null() {
        CString::assign(out, "null body handle");
        return;
    }
    let mut os = String::new();
    for n in ["GetMass", "GetLinearVelocity", "GetAngularVelocity", "SetLinearVelocity", "SetAngularVelocity"] {
        os.push_str(n);
        os.push_str(": ");
        let Some(f) = body_func(n) else {
            os.push_str("NOT IN RTTI\n");
            continue;
        };
        let ret = f.return_type.as_ref().and_then(|p| type_name(p.ty)).unwrap_or("none");
        os.push_str(&format!(
            "native={} static={} params={} ret={} reg={}\n",
            (f.flags & function_flags::IS_NATIVE != 0) as i32,
            (f.flags & function_flags::IS_STATIC != 0) as i32,
            f.params.len(),
            ret,
            f.reg_index()
        ));
    }
    let mut mass = -1.0f32;
    let mass_ok = body_call_get(object, "GetMass", c"Float", as_bytes(&mut mass));
    os.push_str(&format!("GetMass -> ok={} value={}  (mesh authors 0.624)\n", mass_ok as i32, g(mass)));
    let (mut lin, mut ang) = (Vector4::default(), Vector4::default());
    let lin_ok = body_call_get(object, "GetLinearVelocity", c"Vector4", as_bytes(&mut lin));
    let ang_ok = body_call_get(object, "GetAngularVelocity", c"Vector4", as_bytes(&mut ang));
    os.push_str(&format!(
        "GetLinearVelocity  -> ok={} ({}, {}, {}, {})\n",
        lin_ok as i32,
        g(lin.x),
        g(lin.y),
        g(lin.z),
        g(lin.w)
    ));
    os.push_str(&format!(
        "GetAngularVelocity -> ok={} ({}, {}, {}, {})\n",
        ang_ok as i32,
        g(ang.x),
        g(ang.y),
        g(ang.z),
        g(ang.w)
    ));
    CString::assign(out, &os);
}
/// VRDumpBodyFuncs() -> String: the class's whole RTTI listing.
#[no_mangle]
pub unsafe extern "C" fn VRDumpBodyFuncs(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _unused: i64) {
    params!(frame);
    if out.is_null() {
        return;
    }
    let Some(class) = RttiSystem::get().and_then(body_class) else {
        CString::assign(out, "entPhysicalBodyInterface class not found");
        return;
    };
    let mut os = format!("funcs={} staticFuncs={}\n", class.funcs().len(), class.static_funcs().len());
    for f in class.funcs().iter().filter_map(|&f| f.as_ref()) {
        os.push_str(f.short_name.text());
        os.push('(');
        for (p, &param) in f.params.as_slice().iter().enumerate() {
            let Some(param) = param.as_ref() else { continue };
            if p != 0 {
                os.push_str(", ");
            }
            os.push_str(type_name(param.ty).unwrap_or("?"));
            if param.flags & property_flags::IS_OUT != 0 {
                os.push('&');
            }
        }
        let ret = f.return_type.as_ref().and_then(|p| type_name(p.ty)).unwrap_or("void");
        os.push_str(&format!(") -> {}\n", ret));
    }
    CString::assign(out, &os);
}
/// VRProbeBody(body) -> String: the object's first qwords, the ones pointing into readable memory followed, with the
/// executable-relative address of a vtable they lead to.
#[no_mangle]
pub unsafe extern "C" fn VRProbeBody(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _unused: i64) {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS};
    let mut body = Handle::null();
    params!(frame, body);
    if out.is_null() {
        return;
    }
    let object = body.instance as *const usize;
    if object.is_null() {
        CString::assign(out, "null handle");
        return;
    }
    let readable = |p: *const c_void| -> bool {
        if p.is_null() {
            return false;
        }
        let mut mbi: MEMORY_BASIC_INFORMATION = core::mem::zeroed();
        if VirtualQuery(p, &mut mbi, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
            return false;
        }
        mbi.State == MEM_COMMIT && mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
    };
    let exe = cvr_engine::code::image_base();
    let mut os = format!("obj={:x} exe={:x}\n", object as usize, exe);
    for i in 0..24usize {
        let slot = object.add(i);
        if !readable(slot as *const c_void) {
            break;
        }
        let v = *slot;
        os.push_str(&format!("[{}] {:x}", i * 8, v));
        if v > 0x10000 && readable(v as *const c_void) {
            let inner = *(v as *const usize);
            os.push_str(&format!(" -> {:x}", inner));
            if inner > exe && inner < exe + 0x800_0000 {
                os.push_str(&format!(" (vtbl rva {:x})", inner - exe));
            }
        }
        os.push('\n');
    }
    CString::assign(out, &os);
}
