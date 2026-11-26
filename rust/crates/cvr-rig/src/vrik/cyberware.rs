//! Arm cyberware (formerly src/Anim/Cyberware.cpp): the gorilla arms' and mantis blades' skinned finger chain, the
//! mantis assembly mounted on the tracked wrist, the launcher's wrist binding, the muscle curves of an attack, the
//! monowire's line out of the forearm housing, and the frame the combat side reads. The C forms (VRIK_*) keep their
//! names; cvr::cyberware's functions are exported as cvr_cyberware_* and forwarded by src/Anim/CharacterRigRust.cpp.
use super::env::*;
use super::gestures::{cvr_gestures_apply, cvr_gestures_owns_hand, now_ms};
use super::rig::*;
use super::*;
use crate::data::{MantisDeploymentBone, MANTIS_DEPLOYMENT, MANTIS_DEPLOYMENT_REFERENCE_LENGTH, MANTIS_DEPLOYMENT_WRIST_ANCHOR};
use crate::data::{MUSCLE_CURVES, MUSCLE_SAMPLE_COUNT};
use crate::math::{cross3, dot3, norm3, quat_conj, quat_from_to, quat_mul, quat_norm, quat_rotate_vec};
use crate::skeleton::equal_bone_name;
use crate::wire::{finite, length, unit, v3, Plane, Vec3, Wire, COUNT as WIRE_COUNT, REACH};
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use cvr_body::cmath::{atan2, clamp, cos, min, sin, sqrt};
use std::sync::Mutex;

extern "C" {
    static g_VrikFrameEpoch: AtomicU32;
    fn VRIK_ReadCurrentBodyFrame(worldPosition: *mut f32, worldRotation: *mut f32, trackingYaw: *mut f32) -> bool;
}

/// cvr::cyberware::Frame, the combat side's read of the arm cyberware (Anim/Cyberware.hpp).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Frame {
    pub family: i32,
    pub count: [i32; 2],
    pub points: [[Vec3; WIRE_COUNT]; 2],
    pub body_origin: Vec3,
    pub wrists: [Vec3; 2],
    pub hand_rotation: [[f32; 4]; 2],
    pub deployment: [f32; 2],
    pub epoch: u32,
    pub stamp: u64,
}
const _: () = assert!(core::mem::size_of::<Frame>() == 1208);
const _: () = assert!(core::mem::size_of::<Plane>() == 28);
impl Frame {
    pub const ZERO: Frame = Frame {
        family: 0,
        count: [0; 2],
        points: [[v3(0.0, 0.0, 0.0); WIRE_COUNT]; 2],
        body_origin: v3(0.0, 0.0, 0.0),
        wrists: [v3(0.0, 0.0, 0.0); 2],
        hand_rotation: [[0.0; 4]; 2],
        deployment: [0.0; 2],
        epoch: 0,
        stamp: 0,
    };
}

#[derive(Clone, Copy)]
struct MantisSide {
    wrist: i32,
    index: i32,
    source_index: i32,
    forearm: i32,
    hand: i32,
    blade_joint: i32,
    dorsal_hand: V3,
    dorsal_forearm: V3,
    frame_valid: bool,
}
const MANTIS0: MantisSide = MantisSide {
    wrist: -1,
    index: -1,
    source_index: -1,
    forearm: -1,
    hand: -1,
    blade_joint: -1,
    dorsal_hand: [0.0; 3],
    dorsal_forearm: [0.0; 3],
    frame_valid: false,
};
#[derive(Clone, Copy)]
struct FingerLink {
    source: i32,
    target: i32,
    side: i32,
}
/// The monowire's own route out of the left hand in the unmodified game (fixed-point 1/131072 m): the root on the
/// forearm housing, across the wrist crease on the palm side, then along the palm under the curled fingers. The
/// right arm mirrors it.
struct LeadPoint {
    hand: bool,
    x: f32,
    y: f32,
    z: f32,
}
const WIRE_LEAD: [LeadPoint; 5] = [
    LeadPoint { hand: false, x: -3998.0, y: -317.0, z: -2910.0 },
    LeadPoint { hand: false, x: 622.0, y: 677.0, z: -3734.0 },
    LeadPoint { hand: false, x: 2085.0, y: 1115.0, z: -3663.0 },
    LeadPoint { hand: true, x: 2226.0, y: 264.0, z: -3619.0 },
    LeadPoint { hand: true, x: 5789.0, y: -581.0, z: -4246.0 },
];
#[derive(Clone, Copy)]
struct CombatSide {
    hand: i32,
    knuckle: i32,
    wire_anchor: i32,
    wire_root: i32,
    wire_control: i32,
    wire_source: i32,
    wire: [i32; WIRE_COUNT],
    wire_axis: [Vec3; WIRE_COUNT],
    length: f32,
    dynamics: Wire,
}
const COMBAT0: CombatSide = CombatSide {
    hand: -1,
    knuckle: -1,
    wire_anchor: -1,
    wire_root: -1,
    wire_control: -1,
    wire_source: -1,
    wire: [-1; WIRE_COUNT],
    wire_axis: [v3(0.0, 0.0, 0.0); WIRE_COUNT],
    length: 0.0,
    dynamics: Wire::ZERO,
};
#[derive(Clone, Copy)]
struct WristBinding {
    bone: i32,
    position: V3,
    rotation: Q,
}
const BINDING0: WristBinding = WristBinding { bone: -1, position: [0.0; 3], rotation: [0.0; 4] };

const N_DEPLOY: usize = MANTIS_DEPLOYMENT.len();
const N_MUSCLE: usize = MUSCLE_CURVES.len();
static mut SIDES: [MantisSide; 2] = [MANTIS0; 2];
static mut SKIN_FINGERS: [FingerLink; 38] = [FingerLink { source: -1, target: -1, side: 0 }; 38];
static mut SKIN_FINGER_COUNT: usize = 0;
static mut DEPLOYMENT_INDICES: [i32; N_DEPLOY] = [0; N_DEPLOY];
static mut DEPLOYMENT_SCALE: [f32; 2] = [1.0, 1.0];
static mut COMBAT: [CombatSide; 2] = [COMBAT0; 2];
static mut WIRE_TRACK: [i32; 2] = [-1, -1];
static mut WIRE_TRACK_REFERENCE: *const f32 = core::ptr::null();
static mut LAUNCHER_AMMO: i32 = -1;
static mut LAUNCHER_MUZZLE: i32 = -1;
static mut LAUNCHER_BASE: i32 = -1;
static mut LAUNCHER_WRIST: [WristBinding; 2] = [BINDING0; 2];
static mut WIRE_EPOCH: u32 = !0;
static mut WIRE_STAMP: u64 = 0;
static mut WIRE_ORIGIN: Vec3 = v3(0.0, 0.0, 0.0);
static mut WIRE_ORIGIN_VALID: bool = false;
static FRAME_MUTEX: Mutex<()> = Mutex::new(());
static mut FRAME: Frame = Frame::ZERO;
const PLANE0: Plane = Plane { position: v3(0.0, 0.0, 0.0), normal: v3(0.0, 0.0, 0.0), valid: false };
static mut COLLISION_PLANES: [[Plane; WIRE_COUNT]; 2] = [[PLANE0; WIRE_COUNT]; 2];
static mut COLLISION_STAMP: [u64; 2] = [0; 2];
/// One atomic packet per side: start milliseconds in the high bits, duration in the low 16.
static MUSCLE_PULSE: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static mut MUSCLE_INDICES: [i32; N_MUSCLE] = [-1; N_MUSCLE];
static mut MUSCLE_REFERENCE_SCALE: [[f32; 3]; N_MUSCLE] = [[0.0; 3]; N_MUSCLE];
static SELECTED_KIND: AtomicI32 = AtomicI32::new(0);
static SELECTED_STAMP: AtomicU64 = AtomicU64::new(0);
static RAW_GRIP: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
// configure scratch (static rather than stack: VRIK_MAX_BONES entries)
static mut RP: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
static mut RQ: [[f32; 4]; MAX_BONES] = [[0.0; 4]; MAX_BONES];
static mut RVALID: [bool; MAX_BONES] = [false; MAX_BONES];

#[inline(always)]
unsafe fn valid(i: i32) -> bool {
    i >= 0 && i < g_VRBoneCount
}
#[inline(always)]
unsafe fn position(i: i32) -> Vec3 {
    let p = g_fkPos[i as usize];
    v3(p[0], p[1], p[2])
}
#[inline(always)]
fn rotated(q: &Q, v: Vec3) -> Vec3 {
    let b = quat_rotate_vec(q, &[v.x, v.y, v.z]);
    v3(b[0], b[1], b[2])
}
#[inline(always)]
fn grip(side: usize) -> bool {
    RAW_GRIP[side].load(Ordering::Relaxed)
}
unsafe fn descends_from(mut bone: i32, parent: i32) -> bool {
    let mut steps = 0;
    while valid(bone) && {
        let s = steps;
        steps += 1;
        s < g_VRBoneCount
    } {
        if bone == parent {
            return true;
        }
        bone = g_VRBoneParent[bone as usize] as i32;
    }
    false
}
unsafe fn model_position(buffer: *mut u8, bone: i32, p: &V3) {
    let parent = g_VRBoneParent[bone as usize] as i32;
    if valid(parent) {
        write_local_pos(buffer, bone, &g_fkPos[parent as usize], &g_fkRot[parent as usize], p);
    }
}
unsafe fn mount_frame(s: &MantisSide) -> Option<Q> {
    if !s.frame_valid || !valid(s.hand) || !valid(s.forearm) {
        return None;
    }
    let mut axis = sub3(&g_fkPos[s.hand as usize], &g_fkPos[s.forearm as usize]);
    if dot3(&axis, &axis) < 1e-6 {
        return None;
    }
    norm3(&mut axis);
    let mut dorsal = quat_rotate_vec(&g_fkRot[s.hand as usize], &s.dorsal_hand);
    let project = |dorsal: &mut V3| {
        let d = dot3(dorsal, &axis);
        for k in 0..3 {
            dorsal[k] -= axis[k] * d;
        }
    };
    project(&mut dorsal);
    if dot3(&dorsal, &dorsal) < 1e-5 {
        dorsal = quat_rotate_vec(&g_fkRot[s.forearm as usize], &s.dorsal_forearm);
        project(&mut dorsal);
    }
    if dot3(&dorsal, &dorsal) < 1e-5 {
        return None;
    }
    norm3(&mut dorsal);
    let swing = quat_from_to(&[0.0, 1.0, 0.0], &axis);
    let normal = quat_rotate_vec(&swing, &[0.0, 0.0, 1.0]);
    let cross = cross3(&normal, &dorsal);
    let half = 0.5 * atan2(dot3(&cross, &axis), dot3(&normal, &dorsal));
    let turn = [axis[0] * sin(half), axis[1] * sin(half), axis[2] * sin(half), cos(half)];
    let mut mount = quat_mul(&turn, &swing);
    quat_norm(&mut mount);
    Some(mount)
}

unsafe fn find(names: *const *const c_char, count: i32, name: &str) -> i32 {
    for i in 0..count {
        let p = *names.offset(i as isize);
        if !p.is_null() && equal_bone_name(CStr::from_ptr(p).to_bytes(), name.as_bytes()) {
            return i;
        }
    }
    -1
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ConfigureCyberwareRig(names: *const *const c_char, count: i32, reference: *const u8, referenceCount: i32) {
    WIRE_TRACK = [-1, -1];
    WIRE_TRACK_REFERENCE = core::ptr::null();
    SIDES = [MANTIS0; 2];
    DEPLOYMENT_INDICES = [-1; N_DEPLOY];
    SKIN_FINGER_COUNT = 0;
    DEPLOYMENT_SCALE = [1.0, 1.0];
    COMBAT = [COMBAT0; 2];
    LAUNCHER_AMMO = -1;
    LAUNCHER_MUZZLE = -1;
    LAUNCHER_BASE = -1;
    WIRE_EPOCH = !0;
    WIRE_STAMP = 0;
    WIRE_ORIGIN_VALID = false;
    LAUNCHER_WRIST = [BINDING0; 2];
    for pulse in MUSCLE_PULSE.iter() {
        pulse.store(0, Ordering::Relaxed);
    }
    MUSCLE_INDICES = [-1; N_MUSCLE];
    {
        let _lock = FRAME_MUTEX.lock();
        FRAME = Frame::ZERO;
        COLLISION_STAMP = [0, 0];
    }
    if g_VRCyberwareFamily == 0 {
        return;
    }
    let family = g_VRCyberwareFamily;
    if family == 1 || family == 2 {
        for side in 0..2 {
            let hand = if side != 0 { "Right" } else { "Left" };
            let prefix = if side != 0 { "r_" } else { "l_" };
            for joint in 0..4 {
                for digit in ["Thumb", "Index", "Middle", "Ring", "Pinky"] {
                    if joint == 3 && digit == "Thumb" {
                        continue;
                    }
                    let canonical =
                        if joint == 0 { format!("{hand}InHand{digit}") } else { format!("{hand}Hand{digit}{joint}") };
                    let actual = if family == 1 {
                        format!("{canonical}_setup")
                    } else {
                        format!("{prefix}hand{digit}0{}_JNT", joint + 1)
                    };
                    let (source, target) = (find(names, count, &canonical), find(names, count, &actual));
                    if valid(source) && valid(target) && SKIN_FINGER_COUNT < SKIN_FINGERS.len() {
                        SKIN_FINGERS[SKIN_FINGER_COUNT] = FingerLink { source, target, side };
                        SKIN_FINGER_COUNT += 1;
                    }
                }
            }
        }
    }
    // Arming cannot borrow the animation job's working FK arrays.
    RP = [[0.0; 3]; MAX_BONES];
    RQ = [[0.0; 4]; MAX_BONES];
    RVALID = [false; MAX_BONES];
    let (rp, rq, rvalid) = (&mut RP, &mut RQ, &mut RVALID);
    let limit = count.min(referenceCount).min(MAXB);
    if !reference.is_null() {
        for i in 0..limit {
            let iu = i as usize;
            let p = reference.offset(i as isize * 48) as *const f32;
            let parent = g_VRBoneParent[iu] as i32;
            if parent >= i || (parent >= 0 && !rvalid[parent as usize]) {
                continue;
            }
            let mut fin = true;
            for k in 0..8 {
                fin = fin && (*p.add(k)).is_finite();
            }
            let norm = *p.add(4) * *p.add(4) + *p.add(5) * *p.add(5) + *p.add(6) * *p.add(6) + *p.add(7) * *p.add(7);
            if !fin || norm < 0.5 || norm > 1.5 {
                continue;
            }
            let t = a3(p);
            let r = a4(p.add(4));
            if parent >= 0 {
                let pu = parent as usize;
                let mut pos = quat_rotate_vec(&rq[pu], &t);
                for k in 0..3 {
                    pos[k] += rp[pu][k];
                }
                rp[iu] = pos;
                rq[iu] = quat_mul(&rq[pu], &r);
            } else {
                rp[iu] = t;
                rq[iu] = r;
            }
            quat_norm(&mut rq[iu]);
            rvalid[iu] = true;
        }
    }
    let ok = |i: i32| valid(i) && rvalid[i as usize];
    for side in 0..2usize {
        let p = if side != 0 { "r_" } else { "l_" };
        let h = if side != 0 { "Right" } else { "Left" };
        let c = &mut COMBAT[side];
        c.hand = find(names, count, &format!("{h}Hand"));
        c.knuckle = find(names, count, &format!("{h}HandMiddle1_setup"));
        if !valid(c.knuckle) {
            c.knuckle = find(names, count, &format!("{h}HandMiddle1"));
        }
        // The outlet light is bound to InHandThumb; nanoWire01_off is an animation control in the forearm.
        c.wire_anchor = find(names, count, &format!("{h}InHandThumb"));
        c.wire_root = find(names, count, &format!("{p}nanoWire01_grp"));
        c.wire_control = find(names, count, &format!("{p}nanoWire01_off"));
        // The housing on the inner forearm is skinned ~70% to the distal twist joint.
        c.wire_source = find(names, count, &format!("{p}Wrist_2_JNT"));
        for k in 0..WIRE_COUNT {
            c.wire[k] = find(names, count, &format!("{p}nanoWire{:02}_jnt", k + 1));
        }
        for k in 0..WIRE_COUNT {
            let bone = c.wire[k];
            let next = c.wire[if k + 1 < WIRE_COUNT { k + 1 } else { k - 1 }];
            if !ok(bone) || !ok(next) {
                continue;
            }
            let sign = if k + 1 < WIRE_COUNT { 1.0f32 } else { -1.0 };
            let (bu, nu) = (bone as usize, next as usize);
            let mut axis = [(rp[nu][0] - rp[bu][0]) * sign, (rp[nu][1] - rp[bu][1]) * sign, (rp[nu][2] - rp[bu][2]) * sign];
            if k + 1 < WIRE_COUNT {
                c.length += sqrt(dot3(&axis, &axis));
            }
            norm3(&mut axis);
            let inverse = quat_conj(&rq[bu]);
            let local = quat_rotate_vec(&inverse, &axis);
            c.wire_axis[k] = v3(local[0], local[1], local[2]);
        }
        let s = &mut SIDES[side];
        s.wrist = find(names, count, &format!("{p}innerWrist01_JNT"));
        s.index = find(names, count, &format!("{p}handIndex01_JNT"));
        s.source_index = find(names, count, &format!("{h}InHandIndex"));
        s.forearm = find(names, count, &format!("{h}ForeArm"));
        s.hand = find(names, count, &format!("{h}Hand"));
        s.blade_joint = find(names, count, &format!("{p}blade02_JNT"));
        if !descends_from(s.index, s.wrist) || !descends_from(s.source_index, s.hand) {
            *s = MANTIS0;
        }
        let index = find(names, count, &format!("{h}HandIndex1"));
        let pinky = find(names, count, &format!("{h}HandPinky1"));
        if ok(index) && ok(pinky) && ok(s.hand) && ok(s.forearm) {
            let (iu, pu, hu, fu) = (index as usize, pinky as usize, s.hand as usize, s.forearm as usize);
            let a = sub3(&rp[iu], &rp[hu]);
            let b = sub3(&rp[pu], &rp[hu]);
            let mut dorsal = cross3(&a, &b);
            if side != 0 {
                for v in dorsal.iter_mut() {
                    *v = -*v;
                }
            }
            if dot3(&dorsal, &dorsal) > 1e-10 {
                norm3(&mut dorsal);
                s.dorsal_hand = quat_rotate_vec(&quat_conj(&rq[hu]), &dorsal);
                s.dorsal_forearm = quat_rotate_vec(&quat_conj(&rq[fu]), &dorsal);
                s.frame_valid = true;
            }
        }
        if !reference.is_null() && valid(s.blade_joint) && s.blade_joint < referenceCount {
            // The sampled rig's native blade hinge length scales the deployed translations.
            let hinge = a3(reference.offset(s.blade_joint as isize * 48) as *const f32);
            let len = sqrt(dot3(&hinge, &hinge));
            if len.is_finite() {
                DEPLOYMENT_SCALE[side] = clamp(len / MANTIS_DEPLOYMENT_REFERENCE_LENGTH[side], 0.5, 2.0);
            }
        }
    }
    // Both arms pay out the same authored line.
    if COMBAT[0].length > 0.2 && COMBAT[1].length > 0.2 {
        let len = min(REACH, min(COMBAT[0].length, COMBAT[1].length));
        COMBAT[0].length = len;
        COMBAT[1].length = len;
    }
    for (i, bone) in MANTIS_DEPLOYMENT.iter().enumerate() {
        DEPLOYMENT_INDICES[i] = find(names, count, bone.name);
    }
    LAUNCHER_AMMO = find(names, count, "launcher_ammo_base");
    LAUNCHER_MUZZLE = find(names, count, "launcher_fragment_fire");
    LAUNCHER_BASE = find(names, count, "launcher_base");
    if family == 4 && ok(COMBAT[0].hand) {
        let hand = COMBAT[0].hand as usize;
        let inverse = quat_conj(&rq[hand]);
        for (i, root) in ["LeftHand_setup", "l_Wrist_2_JNT_setup"].iter().enumerate() {
            let bone = find(names, count, root);
            if !ok(bone) {
                continue;
            }
            let binding = &mut LAUNCHER_WRIST[i];
            binding.bone = bone;
            let offset = sub3(&rp[bone as usize], &rp[hand]);
            binding.position = quat_rotate_vec(&inverse, &offset);
            binding.rotation = quat_mul(&inverse, &rq[bone as usize]);
        }
    }
    for (i, curve) in MUSCLE_CURVES.iter().enumerate() {
        if curve.family != family {
            continue;
        }
        let bone = find(names, count, curve.name);
        MUSCLE_INDICES[i] = bone;
        for k in 0..3 {
            MUSCLE_REFERENCE_SCALE[i][k] = if !reference.is_null() && bone >= 0 && bone < referenceCount {
                *(reference.offset(bone as isize * 48) as *const f32).add(8 + k)
            } else {
                1.0
            };
        }
    }
}

/// Cyberware's second, skinned finger chain takes the controlled model rotations, keeping its authored lengths.
unsafe fn apply_skin_fingers(buffer: *mut u8, count: i32) {
    if SKIN_FINGER_COUNT == 0 {
        return;
    }
    for i in 0..SKIN_FINGER_COUNT {
        let link = SKIN_FINGERS[i];
        let parent = g_VRBoneParent[link.target as usize] as i32;
        if !cvr_gestures_owns_hand(link.side) || !valid(parent) {
            continue;
        }
        let r = g_fkRot[link.source as usize];
        write_local_rot(buffer, link.target, &g_fkRot[parent as usize], &r);
        g_fkRot[link.target as usize] = r;
    }
    VRIK_ComputeFK(buffer, count);
}

/// The launcher skin's wrist (a sibling of LeftHand) driven from the tracked hand with its own binding.
unsafe fn apply_launcher_wrist(buffer: *mut u8) {
    let hand = COMBAT[0].hand;
    if g_VRCyberwareFamily != 4 || !valid(hand) {
        return;
    }
    let hu = hand as usize;
    for binding in LAUNCHER_WRIST {
        let bone = binding.bone;
        if !valid(bone) || !valid(g_VRBoneParent[bone as usize] as i32) {
            continue;
        }
        let mut pos = quat_rotate_vec(&g_fkRot[hu], &binding.position);
        for k in 0..3 {
            pos[k] += g_fkPos[hu][k];
        }
        let rotation = quat_mul(&g_fkRot[hu], &binding.rotation);
        model_position(buffer, bone, &pos);
        let parent = g_VRBoneParent[bone as usize] as usize;
        write_local_rot(buffer, bone, &g_fkRot[parent], &rotation);
    }
    VRIK_ComputeFK(buffer, VRIK_FKCount());
}

/// The mantis assembly mounted round the tracked wrist socket, its native shape kept.
unsafe fn apply_mantis_pose(buffer: *mut u8) {
    if buffer.is_null() || !VRIK_UsesNativeCyberware() || g_VRCyberwareFamily != 2 || g_VRBind != 4 {
        return;
    }
    let count = VRIK_FKCount();
    VRIK_ComputeFK(buffer, count);
    let m0 = mount_frame(&SIDES[0]);
    let m1 = mount_frame(&SIDES[1]);
    let mounted = [m0.is_some(), m1.is_some()];
    let mounts = [m0.unwrap_or([0.0; 4]), m1.unwrap_or([0.0; 4])];
    let mut wrist_position = [[0.0f32; 3]; 2];
    let mut wrist_rotation = [[0.0f32; 4]; 2];
    let mut mount_origin = [[0.0f32; 3]; 2];
    let mut tracked_wrist = [false; 2];
    for side in 0..2 {
        let s = SIDES[side];
        if !mounted[side] {
            continue;
        }
        mount_origin[side] = g_fkPos[s.hand as usize];
        if !valid(s.wrist) || !valid(s.index) || !valid(s.source_index) {
            continue;
        }
        let (wu, iu, su) = (s.wrist as usize, s.index as usize, s.source_index as usize);
        let inverse = quat_conj(&g_fkRot[iu]);
        let mut delta = quat_mul(&g_fkRot[su], &inverse);
        quat_norm(&mut delta);
        let offset = sub3(&g_fkPos[wu], &g_fkPos[iu]);
        let mut wp = quat_rotate_vec(&delta, &offset);
        for k in 0..3 {
            wp[k] += g_fkPos[su][k];
        }
        wrist_position[side] = wp;
        wrist_rotation[side] = quat_mul(&delta, &g_fkRot[wu]);
        let anchor = quat_rotate_vec(&mounts[side], &MANTIS_DEPLOYMENT_WRIST_ANCHOR[side]);
        // Mount around the actual tracked wrist socket, not the canonical Hand origin.
        for k in 0..3 {
            mount_origin[side][k] = wp[k] - anchor[k] * DEPLOYMENT_SCALE[side];
        }
        tracked_wrist[side] = true;
    }
    for (i, pose) in MANTIS_DEPLOYMENT.iter().enumerate() {
        let bone = DEPLOYMENT_INDICES[i];
        if !valid(bone) {
            continue;
        }
        let pose: &MantisDeploymentBone = pose;
        let side = (pose.name.as_bytes()[0] == b'r') as usize;
        let parent = g_VRBoneParent[bone as usize] as i32;
        if !mounted[side] || !valid(parent) {
            continue;
        }
        let mut pos = quat_rotate_vec(&mounts[side], &pose.position);
        for k in 0..3 {
            pos[k] = mount_origin[side][k] + pos[k] * DEPLOYMENT_SCALE[side];
        }
        let mut rotation = quat_mul(&mounts[side], &pose.rotation);
        quat_norm(&mut rotation);
        model_position(buffer, bone, &pos);
        write_local_rot(buffer, bone, &g_fkRot[parent as usize], &rotation);
        // Profile order is parent-before-child: keep FK current for each dependent joint.
        g_fkPos[bone as usize] = pos;
        g_fkRot[bone as usize] = rotation;
    }
    VRIK_ComputeFK(buffer, count);
    for side in 0..2 {
        let s = SIDES[side];
        if !tracked_wrist[side] {
            continue;
        }
        // The skin and mechanism share the index metacarpal bind: transport the complete mechanical wrist.
        let parent = g_VRBoneParent[s.wrist as usize] as i32;
        if !valid(parent) {
            continue;
        }
        model_position(buffer, s.wrist, &wrist_position[side]);
        write_local_rot(buffer, s.wrist, &g_fkRot[parent as usize], &wrist_rotation[side]);
        VRIK_ComputeFK(buffer, count);
    }
}

/// An attack's muscle curves (sampled from the native attacks), played over a pulse.
unsafe fn apply_muscles(buffer: *mut u8) {
    let now = now_ms();
    for (i, curve) in MUSCLE_CURVES.iter().enumerate() {
        let bone = MUSCLE_INDICES[i];
        let side = (curve.name.as_bytes()[0] == b'r') as usize;
        if curve.family != g_VRCyberwareFamily || !valid(bone) {
            continue;
        }
        let pulse = MUSCLE_PULSE[side].load(Ordering::Acquire);
        let (start, duration) = (pulse >> 16, pulse & 0xffff);
        let active = duration != 0 && now >= start && now - start < duration && cvr_cyberware_selected_kind() == curve.family;
        let transform = bone12(buffer, bone);
        if curve.family == 1 {
            for k in 0..3 {
                (*transform)[8 + k] = MUSCLE_REFERENCE_SCALE[i][k];
            }
        }
        if !active {
            continue;
        }
        let phase = (now - start) as f32 / duration as f32 * (MUSCLE_SAMPLE_COUNT - 1) as f32;
        let a = (phase as i32).clamp(0, MUSCLE_SAMPLE_COUNT as i32 - 2) as usize;
        let weight = phase - a as f32;
        let (first, next) = (&curve.samples[a], &curve.samples[a + 1]);
        let fr = &first.rotation;
        let nr = &next.rotation;
        let sign = if dot3(&[fr[0], fr[1], fr[2]], &[nr[0], nr[1], nr[2]]) + fr[3] * nr[3] < 0.0 { -1.0f32 } else { 1.0 };
        let mut delta = [0.0f32; 4];
        for k in 0..4 {
            delta[k] = fr[k] * (1.0 - weight) + nr[k] * weight * sign;
        }
        quat_norm(&mut delta);
        let t = &mut *transform;
        if curve.family == 2 {
            for k in 0..3 {
                t[k] += (first.position[k] * (1.0 - weight) + next.position[k] * weight) * DEPLOYMENT_SCALE[side];
            }
            let mut rotation = quat_mul(&delta, &[t[4], t[5], t[6], t[7]]);
            quat_norm(&mut rotation);
            t[4..8].copy_from_slice(&rotation);
        }
        for k in 0..3 {
            t[8 + k] = MUSCLE_REFERENCE_SCALE[i][k] * (first.scale[k] * (1.0 - weight) + next.scale[k] * weight);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ConfigureCyberwareTracks(names: *const *const c_char, count: i32, reference: *const f32) {
    WIRE_TRACK = [-1, -1];
    WIRE_TRACK_REFERENCE = reference;
    if names.is_null() || !(0..=8192).contains(&count) {
        return;
    }
    for i in 0..count {
        let p = *names.offset(i as isize);
        if p.is_null() {
            continue;
        }
        let n = CStr::from_ptr(p).to_bytes();
        if n == b"leftWireSlideFactor" {
            WIRE_TRACK[0] = i;
        }
        if n == b"rightWireSlideFactor" {
            WIRE_TRACK[1] = i;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ApplyWireTracks(tracks: *mut f32, count: u32) {
    if tracks.is_null() || tracks as *const f32 == WIRE_TRACK_REFERENCE || count > 8192 || g_VRBind != 4 || g_VRCyberwareFamily != 3 {
        return;
    }
    // Geometry owns the payout: render the full current curve, never a second shortened one.
    for side in 0..2 {
        let t = WIRE_TRACK[side];
        if t >= 0 && (t as u32) < count {
            *tracks.add(t as usize) = if COMBAT[side].dynamics.deployed > 0.0001 { 1.0 } else { 0.0 };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ApplyCyberwarePose(buffer: *mut u8, tracks: *mut f32, trackCount: u32) {
    if buffer.is_null() || !VRIK_UsesNativeCyberware() || g_VRBind != 4 {
        return;
    }
    cvr_gestures_apply(buffer);
    if g_VRCyberwareFamily == 2 {
        apply_mantis_pose(buffer);
    } else {
        VRIK_ComputeFK(buffer, VRIK_FKCount());
    }
    apply_launcher_wrist(buffer);
    apply_skin_fingers(buffer, VRIK_FKCount());
    apply_muscles(buffer);
    if g_VRCyberwareFamily == 2 {
        VRIK_ComputeFK(buffer, VRIK_FKCount());
    }
    let mut world_pos = [0.0f32; 3];
    let mut world_rot = [0.0f32; 4];
    let mut tracking_yaw = 0.0f32;
    if !VRIK_ReadCurrentBodyFrame(world_pos.as_mut_ptr(), world_rot.as_mut_ptr(), &mut tracking_yaw) {
        return;
    }
    let origin = v3(world_pos[0], world_pos[1], world_pos[2]);
    let inverse = quat_conj(&world_rot);
    let now = now_ms();
    let epoch = g_VrikFrameEpoch.load(Ordering::Relaxed);
    let advance = epoch != WIRE_EPOCH;
    let dt = if WIRE_STAMP != 0 && now >= WIRE_STAMP { (now - WIRE_STAMP) as f32 * 0.001 } else { 0.0 };
    // Locomotion carries the wire instead of dragging it.
    let carry = if WIRE_ORIGIN_VALID { origin - WIRE_ORIGIN } else { Vec3::default() };
    let mut output = Frame::ZERO;
    output.family = g_VRCyberwareFamily;
    output.epoch = epoch;
    output.stamp = now;
    output.body_origin = origin;
    let world = |bone: i32| origin + rotated(&world_rot, position(bone));
    for side in 0..2usize {
        let hand = COMBAT[side].hand;
        if !valid(hand) {
            continue;
        }
        let raw = v3(g_VRHandRawModel[side][0], g_VRHandRawModel[side][1], g_VRHandRawModel[side][2]);
        output.wrists[side] = origin + rotated(&world_rot, raw);
        let raw_rotation = g_VRHandRawRot[side];
        let mut hr = quat_mul(&world_rot, &raw_rotation);
        quat_norm(&mut hr);
        output.hand_rotation[side] = hr;
        let c = &mut COMBAT[side];
        if g_VRCyberwareFamily == 3 && valid(c.wire[0]) && valid(c.wire[WIRE_COUNT - 1]) && c.length > 0.2 {
            // The simulated first joint is an OUTPUT, never its own anchor.
            let anchor_bone = c.wire_anchor;
            if !valid(anchor_bone) {
                continue;
            }
            // The native light_slots offset in fixed-point units: the glowing opening of the link housing.
            let sign = if side != 0 { -1.0f32 } else { 1.0 };
            let outlet = v3(sign * 3128.0 / 131072.0, -sign * 3244.0 / 131072.0, (if side != 0 { -3218.0 } else { 3217.0 }) / 131072.0);
            let anchor = world(anchor_bone) + rotated(&world_rot, rotated(&g_fkRot[anchor_bone as usize], outlet));
            let direction = if valid(c.knuckle) { world(c.knuckle) - world(c.hand) } else { rotated(&world_rot, v3(0.0, 1.0, 0.0)) };
            for root in [c.wire_root, c.wire_control] {
                if valid(root) {
                    let scale = bone12(buffer, root);
                    (*scale)[8] = 1.0;
                    (*scale)[9] = 1.0;
                    (*scale)[10] = 1.0;
                }
            }
            if advance {
                {
                    let _lock = FRAME_MUTEX.lock();
                    if now >= COLLISION_STAMP[side] && now - COLLISION_STAMP[side] < 100 {
                        c.dynamics.planes = COLLISION_PLANES[side];
                    } else {
                        c.dynamics.planes = [PLANE0; WIRE_COUNT];
                    }
                }
                c.dynamics.step(anchor, direction, grip(side), dt, c.length, carry);
            }
            if !c.dynamics.valid {
                continue;
            }
            output.deployment[side] = c.dynamics.deployed;
            output.count[side] = if c.dynamics.deployed > 0.1 && grip(side) { WIRE_COUNT as i32 } else { 0 };
            // THE LINE LEAVES THE FOREARM HOUSING, THEN THE FIST: the first five rendered joints follow the game's own
            // route, the rest the simulated line resampled by arc length; shown where the body is now.
            let mut points = c.dynamics.point;
            if !advance && finite(carry) {
                for p in points.iter_mut() {
                    *p = *p + carry;
                }
            }
            let mut shown = points;
            if valid(c.wire_source) {
                const N: usize = WIRE_COUNT;
                let lead = WIRE_LEAD.len();
                let mut fin = true;
                for k in 0..lead {
                    let p = &WIRE_LEAD[k];
                    let bone = if p.hand { c.hand } else { c.wire_source };
                    let offset = v3(sign * p.x / 131072.0, sign * p.y / 131072.0, sign * p.z / 131072.0);
                    shown[k] = world(bone) + rotated(&world_rot, rotated(&g_fkRot[bone as usize], offset));
                    fin = fin && finite(shown[k]);
                }
                let reach = length(shown[0] - anchor);
                if !fin || reach < 0.005 || reach > 0.25 {
                    shown = points;
                } else {
                    let point = &points;
                    let mut arc = [0.0f32; N];
                    for i in 1..N {
                        arc[i] = arc[i - 1] + length(point[i] - point[i - 1]);
                    }
                    let mut i = 1usize;
                    for k in lead..N {
                        let s = arc[N - 1] * (k - lead) as f32 / (N - 1 - lead) as f32;
                        while i < N - 1 && arc[i] < s {
                            i += 1;
                        }
                        let span = arc[i] - arc[i - 1];
                        let f = if span > 1e-6 { clamp((s - arc[i - 1]) / span, 0.0, 1.0) } else { 0.0 };
                        shown[k] = point[i - 1] + (point[i] - point[i - 1]) * f;
                    }
                }
            }
            for k in 0..WIRE_COUNT {
                let bone = c.wire[k];
                if !valid(bone) || !valid(g_VRBoneParent[bone as usize] as i32) {
                    continue;
                }
                let model = rotated(&inverse, shown[k] - origin);
                let a = shown[if k != 0 { k - 1 } else { k }];
                let b = shown[if k + 1 < WIRE_COUNT { k + 1 } else { k }];
                let direction_model = rotated(&inverse, unit(b - a));
                let native_axis = rotated(&g_fkRot[bone as usize], c.wire_axis[k]);
                let delta = quat_from_to(&[native_axis.x, native_axis.y, native_axis.z], &[direction_model.x, direction_model.y, direction_model.z]);
                let mut rotation = quat_mul(&delta, &g_fkRot[bone as usize]);
                quat_norm(&mut rotation);
                let pos = [model.x, model.y, model.z];
                model_position(buffer, bone, &pos);
                let parent = g_VRBoneParent[bone as usize] as usize;
                write_local_rot(buffer, bone, &g_fkRot[parent], &rotation);
                let scale = bone12(buffer, bone);
                (*scale)[8] = 1.0;
                (*scale)[9] = 1.0;
                (*scale)[10] = 1.0;
                g_fkPos[bone as usize] = pos;
                g_fkRot[bone as usize] = rotation;
                output.points[side][k] = c.dynamics.point[k];
            }
        } else if g_VRCyberwareFamily == 2 {
            let s = SIDES[side];
            let mut tip = -1;
            let name = if side != 0 { "r_blade05_JNT" } else { "l_blade05_JNT" };
            for (i, bone) in MANTIS_DEPLOYMENT.iter().enumerate() {
                if bone.name == name {
                    tip = DEPLOYMENT_INDICES[i];
                }
            }
            if valid(s.blade_joint) && valid(tip) {
                output.count[side] = 2;
                output.points[side][0] = world(s.blade_joint);
                output.points[side][1] = world(tip);
            }
        } else if g_VRCyberwareFamily == 1 && valid(c.knuckle) {
            output.count[side] = 2;
            output.points[side][0] = world(c.hand);
            output.points[side][1] = world(c.knuckle);
        } else if g_VRCyberwareFamily == 4 && side == 0 && valid(LAUNCHER_AMMO) && valid(LAUNCHER_MUZZLE) && valid(LAUNCHER_BASE) {
            output.count[side] = 2;
            output.points[side][0] = world(LAUNCHER_MUZZLE);
            // The housing's +Z: the housing lies along the forearm.
            let direction = rotated(&world_rot, rotated(&g_fkRot[LAUNCHER_BASE as usize], v3(0.0, 0.0, 1.0)));
            output.points[side][1] = output.points[side][0] + direction * 0.4;
        }
    }
    if advance {
        WIRE_EPOCH = epoch;
        WIRE_STAMP = now;
        WIRE_ORIGIN = origin;
        WIRE_ORIGIN_VALID = true;
    }
    VRIK_ApplyWireTracks(tracks, trackCount);
    if g_VRCyberwareFamily == 3 {
        VRIK_ComputeFK(buffer, VRIK_FKCount());
    }
    let _lock = FRAME_MUTEX.lock();
    FRAME = output;
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_read_frame(out: *mut Frame) -> bool {
    let _lock = FRAME_MUTEX.lock();
    let now = now_ms();
    if g_VRBind != 4 || FRAME.family == 0 || FRAME.family != g_VRCyberwareFamily || now < FRAME.stamp || now - FRAME.stamp > 150 {
        return false;
    }
    *out = FRAME;
    true
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_launcher_line(origin: *mut f32, direction: *mut f32) -> bool {
    // The rig keeps the launcher's bones while it is installed: selected is what makes the left trigger fire it.
    let mut value = Frame::ZERO;
    if cvr_cyberware_selected_kind() != 4
        || super::weapon_arm::cvr_anim_is_head_aim_weapon_active()
        || !cvr_cyberware_read_frame(&mut value)
        || value.family != 4
        || value.count[0] != 2
    {
        return false;
    }
    let (start, end) = (value.points[0][0], value.points[0][1]);
    let d = [end.x - start.x, end.y - start.y, end.z - start.z];
    let len = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if !(len > 0.01) {
        return false;
    }
    *origin = start.x;
    *origin.add(1) = start.y;
    *origin.add(2) = start.z;
    for k in 0..3 {
        *direction.add(k) = d[k] / len;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_wire_deployment(side: i32) -> f32 {
    let mut value = Frame::ZERO;
    if (0..2).contains(&side) && cvr_cyberware_read_frame(&mut value) && value.family == 3 {
        value.deployment[side as usize]
    } else {
        0.0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_set_wire_planes(side: i32, epoch: u32, planes: *const Plane) {
    if !(0..=1).contains(&side) || planes.is_null() {
        return;
    }
    let _lock = FRAME_MUTEX.lock();
    if FRAME.family != 3 || FRAME.epoch.wrapping_sub(epoch) > 8 {
        return;
    }
    COLLISION_PLANES[side as usize] = *(planes as *const [Plane; WIRE_COUNT]);
    COLLISION_STAMP[side as usize] = now_ms();
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_muscle_pulse(side: i32, duration: f32) {
    if !(0..=1).contains(&side) || !duration.is_finite() {
        return;
    }
    let milliseconds = (clamp(duration, 0.0, 1.5) * 1000.0) as u64;
    MUSCLE_PULSE[side as usize].store((now_ms() << 16) | milliseconds, Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_select(kind: i32) {
    SELECTED_KIND.store(if (0..=5).contains(&kind) { kind } else { 0 }, Ordering::Relaxed);
    SELECTED_STAMP.store(now_ms(), Ordering::Release);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_selected_kind() -> i32 {
    let stamp = SELECTED_STAMP.load(Ordering::Acquire);
    let now = now_ms();
    if g_VRBind == 4 && now >= stamp && now - stamp < 250 {
        SELECTED_KIND.load(Ordering::Relaxed)
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_publish_grips(left: bool, right: bool) {
    RAW_GRIP[0].store(left, Ordering::Relaxed);
    RAW_GRIP[1].store(right, Ordering::Relaxed);
}

#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_collision_info(values: *mut f32) {
    let _lock = FRAME_MUTEX.lock();
    *values = 0.0;
    *values.add(1) = 0.0;
    for side in 0..2 {
        for plane in COLLISION_PLANES[side].iter() {
            if plane.valid {
                *values.add(side) += 1.0;
            }
        }
    }
    let stamp = COLLISION_STAMP[0].max(COLLISION_STAMP[1]);
    *values.add(2) = if stamp != 0 { now_ms().wrapping_sub(stamp) as f32 } else { -1.0 };
    *values.add(3) = ((if grip(0) { 1 } else { 0 }) | (if grip(1) { 2 } else { 0 })) as f32;
}

/// One step of a line the caller holds (cvr::cyberware::Wire's layout): the parity trace steps the C++ and the Rust
/// simulation on the same inputs.
#[no_mangle]
pub unsafe extern "C" fn cvr_cyberware_wire_step(
    wire: *mut Wire,
    anchor: *const f32,
    direction: *const f32,
    grip: bool,
    dt: f32,
    length: f32,
    carry: *const f32,
) {
    let v = |p: *const f32| v3(*p, *p.add(1), *p.add(2));
    (*wire).step(v(anchor), v(direction), grip, dt, length, v(carry));
}
