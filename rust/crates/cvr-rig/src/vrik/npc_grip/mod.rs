//! A HAND TAKING A LIVING PERSON (main's src/Anim/NpcArmGrip.cpp): by an arm, a leg, the head or the chest.
//!
//! There is no switch for one limb of a person's ragdoll: the "Animated" activation was tried (2026-11-04), its bodies
//! appear and the person goes on as before. What is drawn comes from the pose, though, and this port's pose hook sees
//! every animated object's completed update. So the part is solved there, as the player's own arm is:
//!
//!   an arm or a leg: two bones, the hand or foot to where the grip has it, elbow or knee in the plane they were in. An
//!   arm pulled past its stretch brings the shoulder along, and the spine less and less.
//!   the head: the neck to the grip, and the spine a little; it turns with the hand that holds it, as far as a neck lets it.
//!   the chest: the spine bends to the grip.
//!
//! The hips stay where they are, so the feet do too, and the person goes on walking and doing as they do: what they do
//! with what is held is overridden only while it is held. It blends in and out.
//!
//! IDENTIFIED BY THE INSTANCE'S POSE DESCRIPTOR (AnimatedObject+0x18). Reference float tracks belong to the shared
//! MetaRig and cannot identify an NPC instance. The buffer is indexed as the rig's bone names, when a pass has all of
//! them; a pass of a lower level of detail has fewer and is left alone.
//!
//! A PHYSICS HAND'S GRIP (VRNpcGripBind) binds the hold to the bone of the capsule it took: the native solver then drives
//! that bone-local contact from the paired controller and NPC poses itself (RefreshAnchor, SolveAnchor), and publishes
//! where the contact ended up for the physics hand (PublishAnchor).
//!
//! The animated object of a person is found by the caller (cvr-ffi's animation lookup, main's FindAnimatedObjectOf),
//! once, when a grip begins.
pub mod limits;

use self::limits as jl;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*};
use cvr_engine::code::{read, readable_range};
use cvr_host::log;
use cvr_physhands::person::{limit_grip_reach, GripControl, PersonAttachment, PersonContact};
use red4ext_abi::handle::{Handle, WeakHandle};
use std::sync::Mutex;

type V3 = [f32; 3];
type Q = [f32; 4];
const ID: Q = [0.0, 0.0, 0.0, 1.0];

const BLEND_IN: f32 = 0.15;
const BLEND_OUT: f32 = 0.2;
/// rad a held hand is turned from what it was doing
const TURN_MOST: f32 = 1.2;
/// of the hand's turn, of the neck's bend; rad about the neck, forward and back, ear to shoulder
const HEAD_FOLLOW: f32 = 0.7;
const HEAD_BEND: f32 = 0.5;
const HEAD_TWIST: f32 = 0.8;
const HEAD_NOD: f32 = 0.45;
const HEAD_ROLL: f32 = 0.3;
const STRETCH: f32 = 0.97;
const UNASKED_MS: u64 = 400;
const MOST_CHAIN: usize = 64;
const MOST_JOINTS: usize = 8;
const MOST_TOUCHED: usize = 16;
const MAX_BONES: usize = 2048;
const BONE: usize = 48;
const ARM_L: usize = 0;
const ARM_R: usize = 1;
const LEG_L: usize = 2;
const LEG_R: usize = 3;
const PART_HEAD: usize = 4;
const PART_COUNT: usize = 6;
const PART_NAMES: [&str; PART_COUNT] = ["left arm", "right arm", "left leg", "right leg", "head", "chest"];
// entEntity: transformComponent +0xB0, status +0x156 (4 Attached); entIPlacedComponent::worldTransform +0xE0.
const ENTITY_TRANSFORM: usize = 0xB0;
const ENTITY_STATUS: usize = 0x156;
const STATUS_ATTACHED: u8 = 4;
const PLACED_WORLD_TRANSFORM: usize = 0xE0;
// animMetaRig: boneTransforms +0 (QsTransform, 0x30: translation +0, rotation +0x10), parentIndeces +0x10, boneNames
// +0x20, referenceTracks +0x40 (DynArrays: the entries, then the size at +0xC).
const META_TRANSFORMS: usize = 0x00;
const META_PARENTS: usize = 0x10;
const META_NAMES: usize = 0x20;
const META_TRACKS: usize = 0x40;
const ARRAY_SIZE: usize = 0xC;

fn tick() -> u64 {
    // SAFETY: a plain OS call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn qmul(a: &Q, b: &Q) -> Q {
    jl::multiply(a, b)
}
fn qn(mut q: Q) -> Q {
    jl::normalize(&mut q);
    q
}
fn conj(q: &Q) -> Q {
    jl::conjugate(q)
}
fn rot(q: &Q, v: &V3) -> V3 {
    // VRIK_QuatRotateVec
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + (q[1] * tz - q[2] * ty), v[1] + q[3] * ty + (q[2] * tx - q[0] * tz), v[2] + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn apart(a: &V3, b: &V3) -> f32 {
    let d = sub(a, b);
    jl::dot(&d, &d).sqrt()
}
fn arc(a: &V3, b: &V3) -> Q {
    jl::arc(a, b).unwrap_or(ID)
}
fn angle_between(a: &Q, b: &Q) -> f32 {
    let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs();
    2.0 * d.min(1.0).acos()
}
fn blend(from: &Q, to: &Q, w: f32) -> Q {
    jl::slerp(from, to, w)
}

// ---- the pose buffer: 48 bytes a bone, the translation at +0 and the rotation at +16
unsafe fn local_t(buf: usize, i: i32) -> V3 {
    *((buf + i as usize * BONE) as *const V3)
}
unsafe fn local_r(buf: usize, i: i32) -> Q {
    *((buf + i as usize * BONE + 16) as *const Q)
}
unsafe fn set_local_r(buf: usize, i: i32, q: &Q) {
    *((buf + i as usize * BONE + 16) as *mut Q) = *q;
}
unsafe fn bone_bytes(buf: usize, i: i32) -> [u8; BONE] {
    *((buf + i as usize * BONE) as *const [u8; BONE])
}
unsafe fn set_bone_bytes(buf: usize, i: i32, b: &[u8; BONE]) {
    *((buf + i as usize * BONE) as *mut [u8; BONE]) = *b;
}

#[derive(Clone, Copy, Default)]
struct Joint {
    bone: i32,
    share: f32,
    most: f32,
}
#[derive(Clone, Copy)]
struct DonePose {
    buf: usize,
    count: usize,
    bone: [i32; MOST_CHAIN],
    before: [[u8; BONE]; MOST_CHAIN],
    after: [[u8; BONE]; MOST_CHAIN],
    target: V3,
    turn: Q,
    weight: f32,
}
impl DonePose {
    const fn new() -> DonePose {
        DonePose { buf: 0, count: 0, bone: [0; MOST_CHAIN], before: [[0; BONE]; MOST_CHAIN], after: [[0; BONE]; MOST_CHAIN], target: [0.0; 3], turn: [0.0; 4], weight: 0.0 }
    }
}
#[derive(Clone, Copy, Default)]
struct GripPose {
    point: [V3; 3],
    local: [Q; 3],
    at: u64,
}
#[derive(Clone, Copy)]
struct BoneGrip {
    on: bool,
    seeded: bool,
    live: bool,
    bone: i32,
    side: i32,
    owner: i32,
    serial: u64,
    capture: V3,
    capture_q: Q,
    local: V3,
    local_q: Q,
    point: V3,
    rotation: Q,
    control_offset: V3,
    control_rotation: Q,
    capsule_seed: V3,
    capsule_seed_q: Q,
    capsule_local: V3,
    capsule_local_q: Q,
    radius: f32,
    half: f32,
    entity_p: V3,
    entity_q: Q,
    world_point: V3,
    world_goal: V3,
    world_rotation: Q,
    control: GripControl,
    twist: [jl::TwistHistory; 3],
}
impl Default for BoneGrip {
    fn default() -> BoneGrip {
        BoneGrip {
            on: false,
            seeded: false,
            live: false,
            bone: -1,
            side: -1,
            owner: 0,
            serial: 0,
            capture: [0.0; 3],
            capture_q: ID,
            local: [0.0; 3],
            local_q: ID,
            point: [0.0; 3],
            rotation: ID,
            control_offset: [0.0; 3],
            control_rotation: ID,
            capsule_seed: [0.0; 3],
            capsule_seed_q: ID,
            capsule_local: [0.0; 3],
            capsule_local_q: ID,
            radius: 0.0,
            half: 0.0,
            entity_p: [0.0; 3],
            entity_q: ID,
            world_point: [0.0; 3],
            world_goal: [0.0; 3],
            world_rotation: ID,
            control: GripControl::default(),
            twist: [jl::TwistHistory::default(); 3],
        }
    }
}
struct Grip {
    on: bool,
    letting: bool,
    seeded: bool,
    side: i32,
    entity_key: usize,
    part: usize,
    track_a: usize,
    track_b: usize,
    pose_owner: usize,
    bones: u32,
    /// the two bones and what they carry; for a chain only `end`
    upper: i32,
    mid: i32,
    end: i32,
    /// from the root end: what the part's own bones cannot reach
    joints: [Joint; MOST_JOINTS],
    joint_count: usize,
    parents: Vec<i16>,
    target: V3,
    turn: Q,
    offset: V3,
    begun: u64,
    let_: u64,
    asked: u64,
    passes: u32,
    again: u32,
    miss: f32,
    /// the head is kept from the chest
    chest: i32,
    /// the head as the chest had it when the grip closed
    rest: Q,
    hinge: jl::Hinge,
    upper_rest: Q,
    end_rest: Q,
    bounded: bool,
    solved: GripPose,
    anchor: BoneGrip,
    done: [DonePose; 4],
    done_next: usize,
}
impl Grip {
    fn new() -> Grip {
        Grip {
            on: false,
            letting: false,
            seeded: false,
            side: -1,
            entity_key: 0,
            part: 0,
            track_a: 0,
            track_b: 0,
            pose_owner: 0,
            bones: 0,
            upper: -1,
            mid: -1,
            end: -1,
            joints: [Joint::default(); MOST_JOINTS],
            joint_count: 0,
            parents: vec![0; MAX_BONES],
            target: [0.0; 3],
            turn: ID,
            offset: [0.0; 3],
            begun: 0,
            let_: 0,
            asked: 0,
            passes: 0,
            again: 0,
            miss: 0.0,
            chest: -1,
            rest: ID,
            hinge: jl::Hinge::default(),
            upper_rest: ID,
            end_rest: ID,
            bounded: false,
            solved: GripPose::default(),
            anchor: BoneGrip::default(),
            done: [DonePose::new(); 4],
            done_next: 0,
        }
    }
    fn parent(&self, b: i32) -> i32 {
        if b < 0 || b as usize >= MAX_BONES {
            -1
        } else {
            i32::from(self.parents[b as usize])
        }
    }
}
/// Both hands may pull the same torso. One native input is kept around the complete pair of solves, otherwise each
/// hand mistakes the other's spine changes for a new animation and repeatedly adds its own reach correction.
struct NativePose {
    descriptor: usize,
    buf: usize,
    count: usize,
    bone: [i32; 2 * MOST_CHAIN],
    before: [[u8; BONE]; 2 * MOST_CHAIN],
    after: [[u8; BONE]; 2 * MOST_CHAIN],
    owner: WeakHandle,
    owner_key: usize,
    retired: bool,
}
impl NativePose {
    fn new() -> NativePose {
        NativePose {
            descriptor: 0,
            buf: 0,
            count: 0,
            bone: [0; 2 * MOST_CHAIN],
            before: [[0; BONE]; 2 * MOST_CHAIN],
            after: [[0; BONE]; 2 * MOST_CHAIN],
            owner: WeakHandle::null(),
            owner_key: 0,
            retired: false,
        }
    }
}
struct State {
    grips: [Grip; 2],
    entities: [WeakHandle; 2],
    native: Vec<NativePose>,
    next_native: usize,
}
// SAFETY: the weak handles are only locked, compared and reset under the state's lock.
unsafe impl Send for State {}
static STATE: Mutex<Option<Box<State>>> = Mutex::new(None);
static ANY_ON: AtomicBool = AtomicBool::new(false);
static ACTIVE_OWNERS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static RETIRED_OWNERS: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
static ANY_RETIRED: AtomicBool = AtomicBool::new(false);
static NEXT_CLEANUP_AT: AtomicU64 = AtomicU64::new(0);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| {
        Box::new(State { grips: [Grip::new(), Grip::new()], entities: [WeakHandle::null(), WeakHandle::null()], native: (0..8).map(|_| NativePose::new()).collect(), next_native: 0 })
    });
    f(s)
}
fn publish_activity(st: &State) {
    for side in 0..2 {
        let g = &st.grips[side];
        ACTIVE_OWNERS[side].store(if g.on { g.pose_owner } else { 0 }, Release);
    }
    ANY_ON.store(st.grips[0].on || st.grips[1].on, Release);
    ANY_RETIRED.store(st.native.iter().any(|p| p.retired), Release);
}

#[derive(Clone, Copy)]
struct Tr {
    p: V3,
    q: Q,
}
fn child(parent: &Tr, lp: &V3, lq: &Q) -> Tr {
    let q = qn(qmul(&parent.q, lq));
    let r = rot(&parent.q, lp);
    Tr { p: [parent.p[0] + r[0], parent.p[1] + r[1], parent.p[2] + r[2]], q }
}
/// A bone's place in the model, as this pass's buffer has it now.
unsafe fn model_of(g: &Grip, buf: usize, bone: i32) -> Option<Tr> {
    let mut chain = [0i32; MOST_CHAIN];
    let mut n = 0;
    let mut b = bone;
    while b >= 0 && n < MOST_CHAIN {
        if b as u32 >= g.bones {
            return None;
        }
        chain[n] = b;
        n += 1;
        b = g.parent(b);
    }
    if n == 0 || n >= MOST_CHAIN {
        return None;
    }
    let mut at = Tr { p: [0.0; 3], q: ID };
    for i in (0..n).rev() {
        at = child(&at, &local_t(buf, chain[i]), &local_r(buf, chain[i]));
    }
    Some(at)
}
/// What was changed, as it was: the animation it is blended with at the end.
struct Touched {
    count: usize,
    bone: [i32; MOST_TOUCHED],
    was: [Q; MOST_TOUCHED],
}
impl Touched {
    fn new() -> Touched {
        Touched { count: 0, bone: [0; MOST_TOUCHED], was: [ID; MOST_TOUCHED] }
    }
    unsafe fn note(&mut self, buf: usize, b: i32) -> Option<Q> {
        for i in 0..self.count {
            if self.bone[i] == b {
                return Some(self.was[i]);
            }
        }
        if self.count >= MOST_TOUCHED {
            return None;
        }
        self.bone[self.count] = b;
        self.was[self.count] = local_r(buf, b);
        self.count += 1;
        Some(self.was[self.count - 1])
    }
}
/// A bone to have this rotation in the model.
unsafe fn set_model_rot(g: &Grip, buf: usize, bone: i32, model: &Q, t: &mut Touched) {
    if t.note(buf, bone).is_none() {
        return;
    }
    let Some(parent) = model_of(g, buf, g.parent(bone)) else { return };
    set_local_r(buf, bone, &qn(qmul(&conj(&parent.q), model)));
}
/// The joints turn so `tip` comes to `to`, the nearest first, each its share of the way and no further from what the
/// animation had than it can go.
unsafe fn reach_with(g: &Grip, buf: usize, tip: i32, to: &V3, t: &mut Touched) {
    for _ in 0..3 {
        for i in (0..g.joint_count).rev() {
            let j = g.joints[i];
            let (Some(at), Some(end), Some(parent)) = (model_of(g, buf, j.bone), model_of(g, buf, tip), model_of(g, buf, g.parent(j.bone))) else { return };
            let from = sub(&end.p, &at.p);
            let want = sub(to, &at.p);
            let part = blend(&ID, &arc(&from, &want), j.share);
            let q = qn(qmul(&part, &at.q));
            let mut local = qn(qmul(&conj(&parent.q), &q));
            let Some(was) = t.note(buf, j.bone) else { return };
            let a = angle_between(&was, &local);
            if a > j.most {
                local = blend(&was, &local, j.most / a);
            }
            set_local_r(buf, j.bone, &local);
        }
    }
}
/// A chain to `to`, the nearest joint first. A joint turns only the way that takes the tip across to the target, its
/// axis from where the tip has to go: never round the far way, whichever side of the joint the target has passed.
unsafe fn bend_chain(g: &Grip, buf: usize, tip: i32, to: &V3, t: &mut Touched) {
    for _ in 0..3 {
        for i in (0..g.joint_count).rev() {
            let j = g.joints[i];
            let (Some(at), Some(end), Some(parent)) = (model_of(g, buf, j.bone), model_of(g, buf, tip), model_of(g, buf, g.parent(j.bone))) else { return };
            let arm = sub(&end.p, &at.p);
            let gap = sub(to, &end.p);
            let length = jl::dot(&arm, &arm).sqrt();
            if length < 1e-3 {
                continue;
            }
            let along = jl::dot(&gap, &arm) / length;
            let across = [gap[0] - arm[0] * along / length, gap[1] - arm[1] * along / length, gap[2] - arm[2] * along / length];
            let size = jl::dot(&across, &across).sqrt();
            if size < 1e-4 {
                continue;
            }
            let axis = jl::cross(&arm, &across);
            let axis_size = jl::dot(&axis, &axis).sqrt();
            if axis_size < 1e-7 {
                continue;
            }
            let depth = if length + along > length * 0.25 { length + along } else { length * 0.25 };
            let half = size.atan2(depth) * j.share * 0.5;
            let sine = half.sin() / axis_size;
            let part = [axis[0] * sine, axis[1] * sine, axis[2] * sine, half.cos()];
            let q = qn(qmul(&part, &at.q));
            let mut local = qn(qmul(&conj(&parent.q), &q));
            let Some(was) = t.note(buf, j.bone) else { return };
            let a = angle_between(&was, &local);
            if a > j.most {
                local = blend(&was, &local, j.most / a);
            }
            set_local_r(buf, j.bone, &local);
        }
    }
}
/// A turn kept within what a neck does: so far about the neck's own line, and away from it less to the side than
/// forward. The model faces +Y: about X is a nod, about Y an ear to a shoulder.
fn limit_turn(q: &mut Q, axis: &V3, twist_most: f32, nod_most: f32, roll_most: f32) {
    let along = q[0] * axis[0] + q[1] * axis[1] + q[2] * axis[2];
    let mut twist = [axis[0] * along, axis[1] * along, axis[2] * along, q[3]];
    if twist[0] * twist[0] + twist[1] * twist[1] + twist[2] * twist[2] + twist[3] * twist[3] < 1e-6 {
        twist = ID;
    } else {
        jl::normalize(&mut twist);
    }
    let swing = qn(qmul(q, &conj(&twist)));
    let a = angle_between(&ID, &twist);
    let b = angle_between(&ID, &swing);
    let kept_twist = blend(&ID, &twist, if a > twist_most { twist_most / a } else { 1.0 });
    let size_of = (swing[0] * swing[0] + swing[1] * swing[1] + swing[2] * swing[2]).sqrt();
    let mut over = 0.0f32;
    if size_of > 1e-5 {
        let ux = swing[0] / size_of / nod_most;
        let uy = swing[1] / size_of / roll_most;
        let uz = swing[2] / size_of / nod_most;
        over = b * (ux * ux + uy * uy + uz * uz).sqrt();
    }
    let kept_swing = blend(&ID, &swing, if over > 1.0 { 1.0 / over } else { 1.0 });
    *q = qn(qmul(&kept_swing, &kept_twist));
}
/// How the hand has turned since it closed, as far as a wrist goes along.
fn held_turn(turn: &Q) -> Q {
    let angle = angle_between(&ID, turn);
    blend(&ID, turn, if angle > TURN_MOST { TURN_MOST / angle } else { 1.0 })
}
/// An arm or a leg.
unsafe fn solve_limb(g: &mut Grip, buf: usize, t: &V3, touched: &mut Touched) {
    if !g.bounded {
        return;
    }
    let (Some(mut up), Some(mut mid), Some(end)) = (model_of(g, buf, g.upper), model_of(g, buf, g.mid), model_of(g, buf, g.end)) else { return };
    let end_was = end.q;
    let la = apart(&mid.p, &up.p);
    let lb = apart(&end.p, &mid.p);
    if la < 1e-3 || lb < 1e-3 {
        return;
    }
    // THE FALLOFF: past the limb's stretch the shoulder is brought along so far as it goes, and the spine less and less.
    let stretch = (la + lb) * STRETCH;
    if g.joint_count > 0 && apart(t, &up.p) > stretch {
        let d = apart(t, &up.p);
        let shoulder = [t[0] + (up.p[0] - t[0]) * stretch / d, t[1] + (up.p[1] - t[1]) * stretch / d, t[2] + (up.p[2] - t[2]) * stretch / d];
        reach_with(g, buf, g.upper, &shoulder, touched);
        let (Some(u2), Some(m2), Some(_)) = (model_of(g, buf, g.upper), model_of(g, buf, g.mid), model_of(g, buf, g.end)) else { return };
        up = u2;
        mid = m2;
    }
    let _ = mid;
    // Solve with the rig's actual hinge. An unconstrained two-bone arc can fold a knee sideways, and keeping the old
    // world hand rotation breaks the wrist.
    let to_t = sub(t, &up.p);
    let d = jl::dot(&to_t, &to_t).sqrt();
    if !d.is_finite() {
        return;
    }
    let dir = if d < 1e-5 {
        let Some(e) = model_of(g, buf, g.end) else { return };
        let mut dv = sub(&e.p, &up.p);
        if !jl::unit(&mut dv) {
            return;
        }
        dv
    } else {
        [to_t[0] / d, to_t[1] / d, to_t[2] / d]
    };
    let flex = ((d * d - la * la - lb * lb) / (2.0 * la * lb)).clamp(-1.0, 1.0).acos();
    let mid_local = jl::pose(&g.hinge, flex, &local_r(buf, g.mid), None);
    let fore = rot(&mid_local, &local_t(buf, g.end));
    let lm = local_t(buf, g.mid);
    let local_end = [lm[0] + fore[0], lm[1] + fore[1], lm[2] + fore[2]];
    let world_end = rot(&up.q, &local_end);
    let aim = arc(&world_end, &dir);
    let up_q = qn(qmul(&aim, &up.q));
    // Transport the animated upper bone's roll with the aim rotation. Its rig hinge already chooses the bend plane.
    // Projecting the old upper segment onto the NEW target instead invents opposite poles either side of a straight
    // limb: a sub-millimetre movement could flip the elbow 180 degrees.
    let leg = g.part == LEG_L || g.part == LEG_R;
    let Some(parent) = model_of(g, buf, g.parent(g.upper)) else { return };
    let local = qmul(&conj(&parent.q), &up_q);
    let Some(bounded) = jl::clamp(&g.upper_rest, &local, &local_t(buf, g.mid), if leg { jl::HIP } else { jl::SHOULDER }, None) else { return };
    if touched.note(buf, g.upper).is_none() || touched.note(buf, g.mid).is_none() {
        return;
    }
    set_local_r(buf, g.upper, &bounded);
    set_local_r(buf, g.mid, &mid_local);
    let Some(mid) = model_of(g, buf, g.mid) else { return };
    // Keep the requested world turn only as far as the wrist/ankle can follow its NEW parent. Position and orientation
    // share the constrained chain.
    let end_q = if leg { end_was } else { qn(qmul(&held_turn(&g.turn), &end_was)) };
    let local = qmul(&conj(&mid.q), &end_q);
    if let Some(bounded) = jl::clamp(&g.end_rest, &local, &local_t(buf, g.end), if leg { jl::ANKLE } else { jl::WRIST }, None) {
        if touched.note(buf, g.end).is_some() {
            set_local_r(buf, g.end, &bounded);
        }
    }
    if let Some(end) = model_of(g, buf, g.end) {
        g.miss = apart(t, &end.p);
    }
}
/// The head or the chest: a chain, no bones of its own.
unsafe fn solve_chain(g: &mut Grip, buf: usize, t: &V3, touched: &mut Touched) {
    let Some(was) = model_of(g, buf, g.end) else { return };
    let end_was = was.q;
    let below = model_of(g, buf, g.parent(g.end));
    bend_chain(g, buf, g.end, t, touched);
    let Some(now) = model_of(g, buf, g.end) else { return };
    g.miss = apart(t, &now.p);
    if g.part != PART_HEAD {
        return;
    }
    // half of how the neck bent it, so a head brought down looks down, and most of how the hand turned; all of that no
    // further than a neck goes
    let bend = qn(qmul(&now.q, &conj(&end_was)));
    let half = blend(&ID, &bend, HEAD_BEND);
    let turn = blend(&ID, &g.turn, HEAD_FOLLOW);
    let mut total = qn(qmul(&turn, &half));
    if let Some(below) = below {
        let mut axis = sub(&was.p, &below.p);
        if jl::dot(&axis, &axis).sqrt() > 1e-4 && jl::unit(&mut axis) {
            limit_turn(&mut total, &axis, HEAD_TWIST, HEAD_NOD, HEAD_ROLL);
        }
    }
    let mut q = qn(qmul(&total, &end_was));
    // AND NO FURTHER FROM THE CHEST, whatever the pose it started from had in it: the limit above is on what this pass adds.
    if let (Some(below), true) = (below, g.chest >= 0) {
        if let Some(chest) = model_of(g, buf, g.chest) {
            let mut axis = sub(&was.p, &below.p);
            if jl::dot(&axis, &axis).sqrt() > 1e-4 && jl::unit(&mut axis) {
                let carried = qn(qmul(&chest.q, &g.rest));
                let mut away = qn(qmul(&q, &conj(&carried)));
                limit_turn(&mut away, &axis, HEAD_TWIST, HEAD_NOD, HEAD_ROLL);
                q = qn(qmul(&away, &carried));
            }
        }
    }
    set_model_rot(g, buf, g.end, &q, touched);
}
fn ancestor(g: &Grip, bone: i32, mut descendant: i32) -> bool {
    let mut depth = 0;
    while descendant >= 0 && (descendant as u32) < g.bones && depth < MOST_CHAIN {
        if bone == descendant {
            return true;
        }
        descendant = g.parent(descendant);
        depth += 1;
    }
    false
}
unsafe fn anchor_pose(g: &Grip, buf: usize) -> Option<(V3, Q)> {
    let at = model_of(g, buf, g.anchor.bone)?;
    let o = rot(&at.q, &g.anchor.local);
    Some(([at.p[0] + o[0], at.p[1] + o[1], at.p[2] + o[2]], qn(qmul(&at.q, &g.anchor.local_q))))
}
unsafe fn bound_local(g: &Grip, buf: usize, bone: i32, wanted: &Q, touched: &mut Touched) -> Option<Q> {
    let native = touched.note(buf, bone)?;
    let leg = g.part == LEG_L || g.part == LEG_R;
    let index = if bone == g.upper {
        0
    } else if bone == g.mid {
        1
    } else if bone == g.end {
        2
    } else {
        usize::MAX
    };
    let winding = (index != usize::MAX && g.anchor.on && g.anchor.twist[index].valid).then(|| g.anchor.twist[index].angle);
    if bone == g.upper {
        return jl::clamp(&g.upper_rest, wanted, &local_t(buf, g.mid), if leg { jl::HIP } else { jl::SHOULDER }, winding);
    }
    if bone == g.end {
        return jl::clamp(&g.end_rest, wanted, &local_t(buf, g.end), if leg { jl::ANKLE } else { jl::WRIST }, winding);
    }
    if bone == g.mid {
        let direction = rot(wanted, &g.hinge.outgoing);
        let c = jl::cross(&g.hinge.incoming, &direction);
        let flex = jl::dot(&c, &g.hinge.axis).atan2(jl::dot(&g.hinge.incoming, &direction)) * g.hinge.sign;
        return Some(jl::pose(&g.hinge, flex, wanted, winding));
    }
    for i in 0..g.joint_count {
        if g.joints[i].bone == bone {
            let a = angle_between(&native, wanted);
            return Some(blend(&native, wanted, if a > g.joints[i].most { g.joints[i].most / a } else { 1.0 }));
        }
    }
    None
}
unsafe fn bound_model(g: &Grip, buf: usize, bone: i32, wanted: &Q, touched: &mut Touched) -> bool {
    let Some(parent) = model_of(g, buf, g.parent(bone)) else { return false };
    let local = qn(qmul(&conj(&parent.q), wanted));
    let Some(bounded) = bound_local(g, buf, bone, &local, touched) else { return false };
    set_local_r(buf, bone, &bounded);
    true
}
/// The hold given up: the physics hand told (released), the grip blending out from now.
fn stop_anchor(g: &mut Grip, entities: &mut [WeakHandle; 2]) {
    if (0..2).contains(&g.anchor.side) {
        let a = &g.anchor;
        let mut ended = PersonAttachment { owner: a.owner, serial: a.serial, release: true, ..PersonAttachment::default() };
        if ended.serial == 0 {
            if let Some(c) = crate::vrik::physhands::grab::read_person_contact(a.side as usize).filter(|c| c.owner == a.owner) {
                ended.serial = c.serial;
            }
        }
        crate::vrik::physhands::grab::publish_person_attachment(a.side as usize, &ended);
    }
    g.anchor.live = false;
    g.letting = true;
    if g.let_ == 0 {
        g.let_ = tick();
    }
    if g.anchor.on && !g.anchor.seeded {
        g.on = false;
        if (0..2).contains(&g.side) {
            entities[g.side as usize] = WeakHandle::null();
        }
    }
}
/// An actual wrist-to-bone attachment, not a fixed offset to the limb's distal endpoint. Moving a forearm grip must
/// solve that point even when the elbow bends; the wrist/foot can then continue to move naturally beyond it.
unsafe fn seed_anchor(g: &mut Grip, buf: usize, entities: &mut [WeakHandle; 2]) -> bool {
    if !g.bounded || !g.anchor.on || !ancestor(g, g.upper, g.anchor.bone) {
        return false;
    }
    if !g.anchor.seeded {
        let Some(bone) = model_of(g, buf, g.anchor.bone) else { return false };
        // A mismatched entity/model basis must never create a metre-long lever on a limb. A normal capsule grip is close
        // to its selected bone.
        if apart(&g.anchor.capture, &bone.p) > 0.8 {
            if g.anchor.live {
                log!("[npcgrip] rejected anchor {:.3} m from bone {}\n", apart(&g.anchor.capture, &bone.p), g.anchor.bone);
                stop_anchor(g, entities);
                g.on = false;
            }
            return false;
        }
        let inverse = conj(&bone.q);
        let a = &mut g.anchor;
        a.local = rot(&inverse, &sub(&a.capture, &bone.p));
        a.local_q = qn(qmul(&inverse, &a.capture_q));
        a.seeded = true;
        if a.live {
            a.capsule_local = rot(&inverse, &sub(&a.capsule_seed, &bone.p));
            a.capsule_local_q = qn(qmul(&inverse, &a.capsule_seed_q));
        }
    }
    true
}
unsafe fn solve_anchor(g: &mut Grip, buf: usize, touched: &mut Touched, entities: &mut [WeakHandle; 2]) {
    if !seed_anchor(g, buf, entities) {
        return;
    }
    let wanted = qn(qmul(&g.turn, &conj(&g.anchor.local_q)));
    // Observe each external requested twist once, before CCD changes any parent. Iterative trial poses are not new
    // tracking input and must not wind up the history. The held bone follows the controller; other joints keep their
    // native animation's requested twist.
    let limb = [g.upper, g.mid, g.end];
    for (i, &bone) in limb.iter().enumerate() {
        let mut requested = local_r(buf, bone);
        if bone == g.anchor.bone {
            let Some(parent) = model_of(g, buf, g.parent(bone)) else { return };
            requested = qn(qmul(&conj(&parent.q), &wanted));
        }
        let reference = match i {
            0 => g.upper_rest,
            1 => g.hinge.rest,
            _ => g.end_rest,
        };
        let axis = match i {
            0 => local_t(buf, g.mid),
            1 => g.hinge.outgoing,
            _ => local_t(buf, g.end),
        };
        let Some(angle) = jl::reference_twist(&reference, &requested, &axis) else { return };
        g.anchor.twist[i].observe(angle);
    }
    let mut chain = [0i32; MOST_TOUCHED];
    let mut count = 0;
    for bone in [g.end, g.mid, g.upper] {
        if ancestor(g, bone, g.anchor.bone) {
            chain[count] = bone;
            count += 1;
        }
    }
    for i in (0..g.joint_count).rev() {
        if ancestor(g, g.joints[i].bone, g.anchor.bone) && count < MOST_TOUCHED {
            chain[count] = g.joints[i].bone;
            count += 1;
        }
    }
    // Establish each limit's native reference once before any CCD pass.
    for &b in &chain[..count] {
        touched.note(buf, b);
    }
    for bone in [g.upper, g.mid, g.end] {
        if let Some(bounded) = bound_local(g, buf, bone, &local_r(buf, bone), touched) {
            set_local_r(buf, bone, &bounded);
        }
    }
    bound_model(g, buf, g.anchor.bone, &wanted, touched);
    let mut best = [ID; MOST_TOUCHED];
    let mut best_cost = f32::INFINITY;
    let save = |g: &Grip, touched: &Touched, best: &mut [Q; MOST_TOUCHED], best_cost: &mut f32| -> f32 {
        let Some((p, q)) = anchor_pose(g, buf) else { return f32::INFINITY };
        let distance = apart(&p, &g.target);
        let angle = angle_between(&q, &g.turn);
        // Position is the hard contact goal. Orientation selects among nearby solutions and yields to the anatomical
        // limits when both cannot fit.
        let cost = distance * distance + angle * angle * 1e-6;
        if cost < *best_cost {
            *best_cost = cost;
            for i in 0..touched.count {
                best[i] = local_r(buf, touched.bone[i]);
            }
        }
        distance
    };
    save(g, touched, &mut best, &mut best_cost);
    for _ in 0..32 {
        let before = best_cost;
        for &joint_bone in &chain[..count] {
            let (Some(joint), Some((p, _))) = (model_of(g, buf, joint_bone), anchor_pose(g, buf)) else { return };
            let from = sub(&p, &joint.p);
            let to = sub(&g.target, &joint.p);
            let model = qn(qmul(&arc(&from, &to), &joint.q));
            bound_model(g, buf, joint_bone, &model, touched);
            save(g, touched, &mut best, &mut best_cost);
        }
        let reached = anchor_pose(g, buf).is_some_and(|(p, _)| apart(&p, &g.target) < 0.00025);
        if reached || before - best_cost < 1e-11 {
            break;
        }
    }
    if best_cost.is_finite() {
        for i in 0..touched.count {
            set_local_r(buf, touched.bone[i], &best[i]);
        }
    }
    if let Some((p, q)) = anchor_pose(g, buf) {
        g.anchor.point = p;
        g.anchor.rotation = q;
    }
    g.miss = apart(&g.anchor.point, &g.target);
}
unsafe fn solve(g: &mut Grip, buf: usize, weight: f32, entities: &mut [WeakHandle; 2]) {
    // Where the grip took it is where it is: the slots the script goes by are not quite the bones.
    if !g.seeded && !g.anchor.on {
        let Some(end) = model_of(g, buf, g.end) else { return };
        g.offset = sub(&end.p, &g.target);
        if g.chest >= 0 {
            if let Some(chest) = model_of(g, buf, g.chest) {
                g.rest = qn(qmul(&conj(&chest.q), &end.q));
            }
        }
        g.seeded = true;
    }
    let t = [g.target[0] + g.offset[0], g.target[1] + g.offset[1], g.target[2] + g.offset[2]];
    let mut touched = Touched::new();
    if g.anchor.on {
        solve_anchor(g, buf, &mut touched, entities);
    } else if g.upper >= 0 {
        solve_limb(g, buf, &t, &mut touched);
    } else {
        solve_chain(g, buf, &t, &mut touched);
    }
    if weight < 0.999 {
        for i in 0..touched.count {
            let now = local_r(buf, touched.bone[i]);
            set_local_r(buf, touched.bone[i], &blend(&touched.was[i], &now, weight));
        }
    }
    if g.upper >= 0 {
        let bones = [g.upper, g.mid, g.end];
        let mut ok = true;
        for (i, &b) in bones.iter().enumerate() {
            let Some(model) = model_of(g, buf, b) else {
                ok = false;
                break;
            };
            g.solved.point[i] = model.p;
            g.solved.local[i] = local_r(buf, b);
        }
        g.solved.at = if ok { tick() } else { 0 };
        if ok {
            if g.anchor.on {
                if let Some((p, q)) = anchor_pose(g, buf) {
                    g.anchor.point = p;
                    g.anchor.rotation = q;
                }
                g.miss = apart(&g.target, &g.anchor.point);
            } else {
                g.miss = apart(&t, &g.solved.point[2]);
            }
        }
    }
}
/// Keep the native input as well as the solved output. The same animation pose can be reused, or copied to another
/// buffer, while a controller keeps moving. Re-solve from its native input on a new command; never add a turn to our
/// own output, and never wait for the idle NPC's next animation to move the grip.
unsafe fn solve_once(g: &mut Grip, buf: usize, weight: f32, entities: &mut [WeakHandle; 2]) -> bool {
    let mut slot = None;
    let mut previous = None;
    for (k, d) in g.done.iter().enumerate() {
        if d.buf == buf {
            slot = Some(k);
        }
        let mut same = d.count > 0;
        for i in 0..d.count {
            if !same {
                break;
            }
            same = bone_bytes(buf, d.bone[i]) == d.after[i];
        }
        if same {
            previous = Some(k);
            break;
        }
    }
    if let Some(p) = previous {
        let d = &g.done[p];
        let command_same = d.target == g.target && d.turn == g.turn && d.weight == weight;
        if command_same {
            g.again += 1;
            if g.bounded {
                g.solved.at = tick();
            }
            return false;
        }
        for i in 0..d.count {
            set_bone_bytes(buf, d.bone[i], &d.before[i]);
        }
    }
    let slot = match slot {
        Some(s) => s,
        None => {
            let s = g.done_next;
            g.done_next = (g.done_next + 1) % 4;
            g.done[s].buf = buf;
            s
        }
    };
    g.done[slot].count = 0;
    let mut b = g.end;
    while b >= 0 && (b as u32) < g.bones && g.done[slot].count < MOST_CHAIN {
        let i = g.done[slot].count;
        g.done[slot].count += 1;
        g.done[slot].bone[i] = b;
        g.done[slot].before[i] = bone_bytes(buf, b);
        b = g.parent(b);
    }
    solve(g, buf, weight, entities);
    let d = &mut g.done[slot];
    for i in 0..d.count {
        d.after[i] = bone_bytes(buf, d.bone[i]);
    }
    d.target = g.target;
    d.turn = g.turn;
    d.weight = weight;
    true
}
/// A graph pass can refresh only some channels of this buffer. A changed translation, scale, or neighboring joint does
/// not turn our retained rotation into native animation. Only rotations we changed and can still identify exactly are
/// restored; every freshly written channel is preserved.
unsafe fn restore_retained_rotations(pose: &NativePose, buf: usize, slots: u32) {
    for i in 0..pose.count {
        let bone = pose.bone[i];
        if bone as u32 >= slots {
            continue;
        }
        let (before, after) = (&pose.before[i][16..32], &pose.after[i][16..32]);
        let now = core::slice::from_raw_parts((buf + bone as usize * BONE + 16) as *const u8, 16);
        if before != after && now == after {
            core::ptr::copy_nonoverlapping(before.as_ptr(), (buf + bone as usize * BONE + 16) as *mut u8, 16);
        }
    }
}
unsafe fn begin_native_pose(st: &mut State, descriptor: usize, buf: usize, slots: u32) -> Option<usize> {
    let applies = st.grips.iter().any(|g| g.on && descriptor == g.pose_owner && slots >= g.bones);
    if !applies {
        return None;
    }
    let mut own = None;
    let mut previous = None;
    for (k, p) in st.native.iter().enumerate() {
        if p.descriptor != descriptor {
            continue;
        }
        if p.buf == buf {
            own = Some(k);
        }
        let mut same = p.count > 0;
        for i in 0..p.count {
            if !same {
                break;
            }
            same = (p.bone[i] as u32) < slots && bone_bytes(buf, p.bone[i]) == p.after[i];
        }
        if same {
            previous = Some(k);
        }
    }
    if let Some(k) = previous {
        let p = &st.native[k];
        for i in 0..p.count {
            set_bone_bytes(buf, p.bone[i], &p.before[i]);
        }
    } else if let Some(k) = own {
        restore_retained_rotations(&st.native[k], buf, slots);
    }
    let own = match own {
        Some(k) => k,
        None => {
            let mut k = st.next_native;
            for _ in 0..8 {
                k = st.next_native;
                st.next_native = (st.next_native + 1) % 8;
                if !st.native[k].retired {
                    break;
                }
            }
            k
        }
    };
    let p = &mut st.native[own];
    p.retired = false;
    RETIRED_OWNERS[own].store(0, Release);
    p.owner = WeakHandle::null();
    p.owner_key = 0;
    p.descriptor = descriptor;
    p.buf = buf;
    p.count = 0;
    for side in 0..2 {
        let g = &st.grips[side];
        if !g.on || descriptor != g.pose_owner || slots < g.bones {
            continue;
        }
        if g.side >= 0 && g.entity_key != 0 {
            let strong = st.entities[g.side as usize].lock();
            st.native[own].owner = WeakHandle::from_handle(&strong);
            st.native[own].owner_key = g.entity_key;
        }
        let p = &mut st.native[own];
        let mut depth = 0;
        let mut b = g.end;
        while b >= 0 && (b as u32) < g.bones && depth < MOST_CHAIN {
            depth += 1;
            if !p.bone[..p.count].contains(&b) {
                if p.count >= 2 * MOST_CHAIN {
                    break;
                }
                let i = p.count;
                p.count += 1;
                p.bone[i] = b;
                p.before[i] = bone_bytes(buf, b);
            }
            b = g.parent(b);
        }
    }
    Some(own)
}
/// The entity's world frame, read twice and the same both times (not mid-write), only while it is attached.
pub(crate) fn entity_frame(entity: usize) -> Option<(V3, Q)> {
    if entity == 0 || read::<u8>(entity + ENTITY_STATUS)? != STATUS_ATTACHED {
        return None;
    }
    let tc = read::<usize>(entity + ENTITY_TRANSFORM).filter(|t| *t != 0)?;
    let first = read::<[u32; 8]>(tc + PLACED_WORLD_TRANSFORM)?;
    let second = read::<[u32; 8]>(tc + PLACED_WORLD_TRANSFORM)?;
    if first != second {
        return None;
    }
    let p = [0, 1, 2].map(|i| first[i] as i32 as f32 / 131072.0);
    let mut q = [4, 5, 6, 7].map(|i| f32::from_bits(first[i]));
    (p.iter().all(|v| v.is_finite()) && jl::normalize(&mut q)).then_some((p, q))
}
fn entity_attached(entity: usize) -> bool {
    entity != 0 && read::<u8>(entity + ENTITY_STATUS) == Some(STATUS_ATTACHED)
}
fn clear_retired(p: &mut NativePose, k: usize) {
    p.retired = false;
    RETIRED_OWNERS[k].store(0, Release);
    p.owner = WeakHandle::null();
    p.owner_key = 0;
    p.count = 0;
}
unsafe fn restore_retired(st: &mut State, descriptor: usize, buf: usize, slots: u32) {
    for k in 0..st.native.len() {
        let p = &mut st.native[k];
        if !p.retired {
            continue;
        }
        if p.owner_key != 0 {
            let owner = p.owner.lock();
            if owner.is_null() || !entity_attached(owner.instance as usize) {
                clear_retired(p, k);
                continue;
            }
        }
        if descriptor == 0 || buf == 0 || p.descriptor != descriptor {
            continue;
        }
        let mut same = p.count > 0;
        for i in 0..p.count {
            if !same {
                break;
            }
            same = (p.bone[i] as u32) < slots && bone_bytes(buf, p.bone[i]) == p.after[i];
        }
        if same {
            for i in 0..p.count {
                set_bone_bytes(buf, p.bone[i], &p.before[i]);
            }
        } else if p.buf == buf {
            // fresh or partially refreshed native pose
            restore_retained_rotations(p, buf, slots);
            clear_retired(p, k);
        }
    }
}
fn reap_grips(st: &mut State, current_owner: usize, slots: u32) {
    let now = tick();
    for side in 0..2 {
        if !st.grips[side].on {
            continue;
        }
        let g = &st.grips[side];
        let mut detached = false;
        if g.side >= 0 && g.entity_key != 0 {
            let owner = st.entities[g.side as usize].lock();
            detached = owner.is_null() || !entity_attached(owner.instance as usize);
        }
        if !g.letting && now.wrapping_sub(g.asked) <= UNASKED_MS && !detached {
            continue;
        }
        let (letting, asked) = (g.letting, g.asked);
        let g = &mut st.grips[side];
        if g.let_ == 0 {
            g.let_ = if letting { now } else { asked + UNASKED_MS };
        }
        if !g.letting {
            stop_anchor(g, &mut st.entities);
        }
        let g = &st.grips[side];
        let this_pose = current_owner == g.pose_owner && slots >= g.bones;
        if !detached && (now.wrapping_sub(g.let_) < (BLEND_OUT * 1000.0) as u64 || this_pose) {
            continue;
        }
        // No matching pose callbacks after a menu/streaming/LOD transition: free the hand now and restore any reused
        // output when that actor next submits a pose. Never dereference an old bone-buffer pointer here.
        let owner = g.pose_owner;
        for k in 0..st.native.len() {
            let p = &mut st.native[k];
            if p.count != 0 && p.descriptor == owner {
                if detached {
                    clear_retired(p, k);
                } else {
                    p.retired = true;
                    RETIRED_OWNERS[k].store(p.descriptor, Release);
                }
            }
        }
        let g = &mut st.grips[side];
        stop_anchor(g, &mut st.entities);
        g.on = false;
        if g.side >= 0 {
            st.entities[g.side as usize] = WeakHandle::null();
        }
        log!("[npcgrip] released inactive {} ({})\n", PART_NAMES[g.part], if detached { "entity detached" } else { "pose stopped arriving" });
    }
}
fn refresh_anchor(g: &mut Grip, entities: &mut [WeakHandle; 2]) -> bool {
    if !g.anchor.live {
        return true;
    }
    if g.letting || tick().wrapping_sub(g.asked) > UNASKED_MS {
        stop_anchor(g, entities);
        return g.anchor.seeded;
    }
    let side = g.anchor.side as usize;
    let entity = entities[side].lock();
    let frame = (!entity.is_null()).then(|| entity_frame(entity.instance as usize)).flatten();
    let contact: Option<PersonContact> = crate::vrik::physhands::grab::read_person_contact(side);
    let contact_ready = contact.is_some_and(|c| c.owner == g.anchor.owner);
    let control = crate::vrik::physhands::pose::read_grip_control(side);
    let (Some((ep, eq)), true, Some(control)) = (frame, contact_ready, control) else {
        log!(
            "[npcgrip] bone drive unavailable: owner {} entity {} contact {} (owner {}) control {}\n",
            g.anchor.owner,
            i32::from(frame.is_some()),
            i32::from(contact_ready),
            contact.map_or(0, |c| c.owner),
            i32::from(control.is_some())
        );
        stop_anchor(g, entities);
        return g.anchor.seeded;
    };
    let contact = contact.expect("checked");
    let a = &mut g.anchor;
    a.entity_p = ep;
    a.entity_q = eq;
    a.control = control;
    // let the initial finger seat settle first
    if !contact.ready && !a.seeded {
        return false;
    }
    let inverse = conj(&a.entity_q);
    if a.serial != contact.serial || !a.seeded {
        a.serial = contact.serial;
        a.seeded = false;
        a.capture = rot(&inverse, &sub(&contact.position, &a.entity_p));
        a.capture_q = qn(qmul(&inverse, &contact.rotation));
        a.capsule_seed = rot(&inverse, &sub(&contact.capsule, &a.entity_p));
        a.capsule_seed_q = qn(qmul(&inverse, &contact.capsule_rotation));
        a.radius = contact.radius;
        a.half = contact.half;
        let control_inverse = conj(&a.control.rotation);
        a.control_offset = rot(&control_inverse, &sub(&contact.position, &a.control.position));
        a.control_rotation = qn(qmul(&control_inverse, &contact.rotation));
        log!(
            "[npcgrip] bone contact ready: owner {} bone {} serial {} wrist {:.3} {:.3} {:.3} reach {:.3}\n",
            a.owner,
            a.bone,
            a.serial,
            contact.position[0],
            contact.position[1],
            contact.position[2],
            a.control.reach
        );
        for d in g.done.iter_mut() {
            *d = DonePose::new();
        }
        g.done_next = 0;
    }
    let a = &mut g.anchor;
    let o = rot(&a.control.rotation, &a.control_offset);
    a.world_goal = [o[0] + a.control.position[0], o[1] + a.control.position[1], o[2] + a.control.position[2]];
    let control = a.control;
    if !limit_grip_reach(&control, &mut a.world_goal) {
        stop_anchor(g, entities);
        return g.anchor.seeded;
    }
    let a = &g.anchor;
    g.target = rot(&inverse, &sub(&a.world_goal, &a.entity_p));
    let world_q = qmul(&a.control.rotation, &a.control_rotation);
    g.turn = qn(qmul(&inverse, &world_q));
    true
}
unsafe fn publish_anchor(g: &mut Grip, buf: usize) {
    if !g.anchor.live || !g.anchor.seeded {
        return;
    }
    let Some(bone) = model_of(g, buf, g.anchor.bone) else { return };
    let a = &mut g.anchor;
    let mut result = PersonAttachment { owner: a.owner, serial: a.serial, on: true, ..PersonAttachment::default() };
    let p = rot(&a.entity_q, &a.point);
    result.position = [p[0] + a.entity_p[0], p[1] + a.entity_p[1], p[2] + a.entity_p[2]];
    a.world_point = result.position;
    result.rotation = qn(qmul(&a.entity_q, &a.rotation));
    a.world_rotation = result.rotation;
    let local_cap = rot(&bone.q, &a.capsule_local);
    let model_cap = [bone.p[0] + local_cap[0], bone.p[1] + local_cap[1], bone.p[2] + local_cap[2]];
    let c = rot(&a.entity_q, &model_cap);
    result.capsule = [c[0] + a.entity_p[0], c[1] + a.entity_p[1], c[2] + a.entity_p[2]];
    let model_q = qmul(&bone.q, &a.capsule_local_q);
    result.capsule_rotation = qn(qmul(&a.entity_q, &model_q));
    result.radius = a.radius;
    result.half = a.half;
    result.control_at = a.control.at;
    if apart(&result.position, &a.control.shoulder) > a.control.reach + 0.03 {
        result.on = false;
        result.release = true;
    }
    crate::vrik::physhands::grab::publish_person_attachment(a.side as usize, &result);
}
unsafe fn apply_inner(st: &mut State, owner: usize, buf: usize, slots: u32) {
    let native = begin_native_pose(st, owner, buf, slots);
    let mut ready = [true, true];
    for side in 0..2 {
        let g = &st.grips[side];
        if !g.on || owner != g.pose_owner || slots < g.bones {
            continue;
        }
        let (grips, entities) = (&mut st.grips, &mut st.entities);
        let g = &mut grips[side];
        if g.anchor.live {
            ready[side] = refresh_anchor(g, entities);
        }
        // Simultaneous contacts share the same source pose. The first hand's solve must not change where the second
        // contact is captured on a bone.
        if ready[side] && g.anchor.on {
            ready[side] = seed_anchor(g, buf, entities);
        }
    }
    // What moves the trunk first: a limb hangs from it.
    for order in 0..2 {
        for side in 0..2 {
            let (grips, entities) = (&mut st.grips, &mut st.entities);
            let g = &mut grips[side];
            if !g.on || owner != g.pose_owner || slots < g.bones {
                continue;
            }
            if (g.upper < 0) != (order == 0) {
                continue;
            }
            if !ready[side] {
                continue;
            }
            let now = tick();
            let mut finished = false;
            let weight = if g.letting || now.wrapping_sub(g.asked) > UNASKED_MS {
                if g.let_ == 0 {
                    g.let_ = now;
                }
                let w = 1.0 - now.wrapping_sub(g.let_) as f32 * 0.001 / BLEND_OUT;
                if w <= 0.0 {
                    finished = true;
                    0.0
                } else {
                    w
                }
            } else {
                (now.wrapping_sub(g.begun) as f32 * 0.001 / BLEND_IN).min(1.0)
            };
            if solve_once(g, buf, weight, entities) {
                g.passes += 1;
            }
            if finished {
                stop_anchor(g, entities);
                g.on = false;
                if g.side >= 0 {
                    entities[g.side as usize] = WeakHandle::null();
                }
                log!("[npcgrip] let go of the {} after {} passes, and {} more on a pose already solved\n", PART_NAMES[g.part], g.passes, g.again);
            }
        }
    }
    // The other hand can still move a shared ancestor (or the same bone). Attachments are published only from the final
    // combined pose, never from the intermediate pose produced by the first grip.
    for side in 0..2 {
        let g = &mut st.grips[side];
        if !g.on || owner != g.pose_owner || slots < g.bones {
            continue;
        }
        if g.anchor.on && g.anchor.seeded {
            if let Some((p, q)) = anchor_pose(g, buf) {
                g.anchor.point = p;
                g.anchor.rotation = q;
            }
            g.miss = apart(&g.target, &g.anchor.point);
            let bones = [g.upper, g.mid, g.end];
            for (i, &b) in bones.iter().enumerate() {
                if let Some(model) = model_of(g, buf, b) {
                    g.solved.point[i] = model.p;
                    g.solved.local[i] = local_r(buf, b);
                }
            }
        }
        publish_anchor(g, buf);
    }
    if let Some(k) = native {
        let p = &mut st.native[k];
        for i in 0..p.count {
            p.after[i] = bone_bytes(buf, p.bone[i]);
        }
    }
}
fn bone_named(names: usize, count: u32, name: &str) -> i32 {
    let wanted = red4ext_abi::cname::CName::hashed(name).0;
    for i in 0..count as usize {
        if read::<u64>(names + i * 8) == Some(wanted) {
            return i as i32;
        }
    }
    -1
}
fn begin(st: &mut State, side: usize, part: usize, entity: usize, object: usize, target: &V3) -> bool {
    if object == 0 {
        return false;
    }
    let Some(rig) = read::<usize>(object + 8).filter(|r| *r != 0) else { return false };
    let (Some(parents_at), Some(parents_n), Some(names_at), Some(names_n)) =
        (read::<usize>(rig + META_PARENTS), read::<u32>(rig + META_PARENTS + ARRAY_SIZE), read::<usize>(rig + META_NAMES), read::<u32>(rig + META_NAMES + ARRAY_SIZE))
    else {
        return false;
    };
    if parents_n != names_n || names_n as usize > MAX_BONES {
        return false;
    }
    let owner_a = rig;
    let Some(owner_b) = read::<usize>(object + 0x18) else { return false };
    let g = &mut st.grips[side];
    if g.on {
        return false;
    }
    g.side = side as i32;
    g.entity_key = entity;
    g.track_a = read::<usize>(owner_a + META_TRACKS).unwrap_or(0);
    g.track_b = if owner_b != 0 { read::<usize>(owner_b + 0x18).unwrap_or(0) } else { 0 };
    g.pose_owner = owner_b;
    if g.pose_owner == 0 {
        return false;
    }
    g.bones = names_n;
    for i in 0..names_n as usize {
        let Some(p) = read::<i16>(parents_at + i * 2) else { return false };
        g.parents[i] = p;
    }
    g.part = part;
    g.bounded = false;
    g.solved = GripPose::default();
    g.anchor = BoneGrip::default();
    g.upper = -1;
    g.mid = -1;
    g.end = -1;
    g.joint_count = 0;
    g.chest = -1;
    for d in g.done.iter_mut() {
        *d = DonePose::new();
    }
    g.done_next = 0;
    g.again = 0;
    let add = |g: &mut Grip, name: &str, share: f32, most: f32| {
        let b = bone_named(names_at, names_n, name);
        if b >= 0 && g.joint_count < MOST_JOINTS {
            g.joints[g.joint_count] = Joint { bone: b, share, most };
            g.joint_count += 1;
        }
    };
    if part <= LEG_R {
        const NAMES: [[&str; 3]; 4] =
            [["LeftArm", "LeftForeArm", "LeftHand"], ["RightArm", "RightForeArm", "RightHand"], ["LeftUpLeg", "LeftLeg", "LeftFoot"], ["RightUpLeg", "RightLeg", "RightFoot"]];
        g.upper = bone_named(names_at, names_n, NAMES[part][0]);
        g.mid = bone_named(names_at, names_n, NAMES[part][1]);
        g.end = bone_named(names_at, names_n, NAMES[part][2]);
        if g.upper < 0 || g.mid < 0 || g.end < 0 {
            log!("[npcgrip] this rig has no {} ({} {} {} of {} bones)\n", PART_NAMES[part], g.upper, g.mid, g.end, g.bones);
            return false;
        }
        if g.parent(g.mid) != g.upper || g.parent(g.end) != g.mid {
            log!("[npcgrip] bones stand between those of the {}: not solved\n", PART_NAMES[part]);
            return false;
        }
        let (Some(transforms), Some(transforms_n)) = (read::<usize>(rig + META_TRANSFORMS), read::<u32>(rig + META_TRANSFORMS + ARRAY_SIZE)) else { return false };
        if transforms_n != g.bones || transforms == 0 {
            return false;
        }
        let qs = |b: i32| read::<[f32; 12]>(transforms + b as usize * 0x30);
        let (Some(upper), Some(mid), Some(end)) = (qs(g.upper), qs(g.mid), qs(g.end)) else { return false };
        g.upper_rest = [upper[4], upper[5], upper[6], upper[7]];
        g.end_rest = [end[4], end[5], end[6], end[7]];
        let leg = part == LEG_L || part == LEG_R;
        g.bounded = jl::normalize(&mut g.upper_rest)
            && jl::normalize(&mut g.end_rest)
            && jl::configure(&mut g.hinge, &[mid[4], mid[5], mid[6], mid[7]], &[mid[0], mid[1], mid[2]], &[end[0], end[1], end[2]], leg);
        if !g.bounded {
            static LAST_UNSUPPORTED: AtomicU64 = AtomicU64::new(0);
            let now = tick();
            if now.wrapping_sub(LAST_UNSUPPORTED.load(Relaxed)) > 3000 {
                LAST_UNSUPPORTED.store(now, Relaxed);
                log!(
                    "[npcgrip] unsupported reference hinge for {}: mid p({:.5} {:.5} {:.5}) q({:.5} {:.5} {:.5} {:.5}), end p({:.5} {:.5} {:.5}); not solved\n",
                    PART_NAMES[part],
                    mid[0],
                    mid[1],
                    mid[2],
                    mid[4],
                    mid[5],
                    mid[6],
                    mid[7],
                    end[0],
                    end[1],
                    end[2]
                );
            }
            return false;
        }
        if part == ARM_L || part == ARM_R {
            add(g, "Spine", 0.08, 0.08);
            add(g, "Spine1", 0.12, 0.12);
            add(g, "Spine2", 0.18, 0.15);
            add(g, "Spine3", 0.25, 0.2);
            add(g, if part == ARM_L { "LeftShoulder" } else { "RightShoulder" }, 0.6, 0.45);
        }
    } else if part == PART_HEAD {
        g.end = bone_named(names_at, names_n, "Head");
        add(g, "Spine", 0.02, 0.02);
        add(g, "Spine1", 0.03, 0.03);
        add(g, "Spine2", 0.05, 0.05);
        add(g, "Spine3", 0.08, 0.07);
        add(g, "Neck", 0.5, 0.25);
        add(g, "Neck1", 0.7, 0.3);
        g.chest = bone_named(names_at, names_n, "Spine3");
    } else {
        g.end = bone_named(names_at, names_n, "Spine3");
        add(g, "Spine", 0.3, 0.3);
        add(g, "Spine1", 0.5, 0.35);
        add(g, "Spine2", 0.7, 0.4);
    }
    if g.end < 0 || (g.upper < 0 && g.joint_count == 0) {
        log!("[npcgrip] this rig has no {} ({} bones)\n", PART_NAMES[part], g.bones);
        return false;
    }
    g.target = *target;
    g.turn = ID;
    let now = tick();
    g.begun = now;
    g.asked = now;
    g.let_ = 0;
    g.letting = false;
    g.passes = 0;
    g.miss = 0.0;
    g.seeded = false;
    g.on = true;
    log!(
        "[npcgrip] holding the {}: {} bones, end {}, {} joints above it; pose owner {:#x} reference tracks {:#x} live tracks {:#x}\n",
        PART_NAMES[part],
        g.bones,
        g.end,
        g.joint_count,
        g.pose_owner,
        g.track_a,
        g.track_b
    );
    publish_activity(st);
    true
}

/// The descriptor's bone buffer, track buffer and slot count; None for one whose slots pass its capacity.
fn pose_arguments(desc: usize) -> Option<(usize, usize, u32)> {
    if desc == 0 {
        return None;
    }
    let buf = read::<usize>(desc)?;
    let track = read::<usize>(desc + 3 * 8)?;
    let capacity = read::<u32>(desc + 8)?;
    let slots = read::<u32>(desc + 12)?;
    (slots <= capacity && buf != 0).then_some((buf, track, slots))
}
/// NpcArmGripApply: the held parts solved on this completed pose (only an owner a grip has, or every 125 ms to reap the
/// ones whose poses stopped coming).
fn apply(desc: usize) {
    if !ANY_ON.load(Acquire) && !ANY_RETIRED.load(Acquire) {
        return;
    }
    let args = pose_arguments(desc);
    let owner = if args.is_some() { desc } else { 0 };
    let interested = owner != 0 && (ACTIVE_OWNERS.iter().any(|v| v.load(Acquire) == owner) || RETIRED_OWNERS.iter().any(|v| v.load(Acquire) == owner));
    let now = tick();
    let next = NEXT_CLEANUP_AT.load(Relaxed);
    let cleanup = now >= next && NEXT_CLEANUP_AT.compare_exchange(next, now + 125, AcqRel, Relaxed).is_ok();
    if !interested && !cleanup {
        return;
    }
    let (buf, track, slots) = args.unwrap_or((0, 0, 0));
    // the buffer as the pass has it, all of it, or nothing is touched
    let readable = buf != 0 && readable_range(buf, slots as usize * BONE);
    with(|st| {
        if !interested && crate::vrik::physhands::api::tracing() {
            static REPORTED: AtomicUsize = AtomicUsize::new(0);
            for g in &st.grips {
                if g.on && owner != 0 && owner != g.pose_owner && track != 0 && (track == g.track_a || track == g.track_b) && REPORTED.fetch_add(1, Relaxed) < 8 {
                    log!("[npcgrip] rejected shared-track pose {:#x} (held owner {:#x}), track {:#x}\n", owner, g.pose_owner, track);
                }
            }
        }
        reap_grips(st, owner, slots);
        let mut scope = false;
        // SAFETY: the pass's own bone buffer, its whole range readable; the grips' parents copied at their start.
        unsafe {
            if readable {
                restore_retired(st, owner, buf, slots);
            } else {
                restore_retired(st, 0, 0, 0);
            }
            if ANY_ON.load(Acquire) && owner != 0 && readable {
                crate::vrik::physhands::pose::begin_physics_pose_frame();
                scope = true;
                apply_inner(st, owner, buf, slots);
            }
        }
        // Begin publishes on/anyOn under this same lock. Clearing the gate after unlocking could erase a new grab
        // between the empty check and exchange.
        publish_activity(st);
        if scope {
            crate::vrik::physhands::pose::end_physics_pose_frame();
        }
    });
}
/// NpcArmGripPoseCompleted: an animated object's pose descriptor completed (`before_model_conversion`: at the final
/// LocalToModel, else at the object update's return).
pub fn pose_completed(descriptor: usize, before_model_conversion: bool) {
    if !ANY_ON.load(Acquire) && !ANY_RETIRED.load(Acquire) {
        return;
    }
    let active = descriptor != 0 && ACTIVE_OWNERS.iter().any(|v| v.load(Acquire) == descriptor);
    static REPORTED: AtomicUsize = AtomicUsize::new(0);
    let bit = if before_model_conversion { 1 } else { 2 };
    if active && REPORTED.load(Relaxed) & bit == 0 && crate::vrik::physhands::api::tracing() && REPORTED.fetch_or(bit, Relaxed) & bit == 0 {
        log!(
            "[npcgrip] completed held pose {:#x} at {}\n",
            descriptor,
            if before_model_conversion { "local-to-model boundary" } else { "object update return" }
        );
    }
    apply(descriptor);
}
/// NpcArmGripObjectUpdated: CP2077 2.5's AnimatedObject update (exe+0x1B3E00) runs its linked graph controllers before
/// returning. Each controller's pose-copy call is only an intermediate result. The instance owns the completed
/// descriptor here.
pub fn object_updated(object: usize) {
    if !ANY_ON.load(Acquire) && !ANY_RETIRED.load(Acquire) {
        return;
    }
    let Some(descriptor) = read::<usize>(object + 0x18) else { return };
    pose_completed(descriptor, false);
}

/// VRNpcGrip's work: the part 0 left arm 1 right arm 2 left leg 3 right leg 4 head 5 chest taken by that hand, where the
/// grip has it in the entity's model space (w < 0.5 lets go), how the hand has turned since it closed, in that space
/// -> how far the grip is from what the part can reach, in m. Below 0: it could not be held. `object` finds the
/// entity's animated object, asked only when a grip begins.
pub fn grip(entity: &Handle, side: i32, part: i32, target: &[f32; 4], turn: &[f32; 4], object: &dyn Fn() -> usize) -> f32 {
    if !(0..2).contains(&side) || !(0..PART_COUNT as i32).contains(&part) || !target[3].is_finite() {
        return -1.0;
    }
    let t = [target[0], target[1], target[2]];
    let mut rotation = *turn;
    if target[3] >= 0.5 && (!t.iter().all(|v| v.is_finite()) || !jl::normalize(&mut rotation)) {
        return -1.0;
    }
    let (side, part) = (side as usize, part as usize);
    with(|st| {
        reap_grips(st, 0, 0);
        let mut answer = -1.0;
        let g = &mut st.grips[side];
        if target[3] < 0.5 {
            if g.on {
                stop_anchor(g, &mut st.entities);
                st.grips[side].letting = true;
            }
            answer = 0.0;
        } else if g.on && !g.letting && g.part == part {
            g.target = t;
            g.turn = rotation;
            g.asked = tick();
            answer = g.miss;
        } else if !entity.is_null() && !g.on {
            if begin(st, side, part, entity.instance as usize, object(), &t) {
                st.entities[side] = WeakHandle::from_handle(entity);
                answer = 0.0;
            }
        }
        publish_activity(st);
        answer
    })
}
/// VRNpcGripBind's work: a PhysX grasp bound to the selected segment's bone (0 upper, 1 middle, 2 end of the limb); all
/// driving and world conversion then happens natively from paired controller and NPC poses. The miss, -1 when not held.
pub fn grip_bind(entity: &Handle, side: i32, part: i32, segment: i32, owner: i32, object: &dyn Fn() -> usize) -> f32 {
    if entity.is_null() || !(0..2).contains(&side) || !(0..=3).contains(&part) || !(0..=2).contains(&segment) || owner <= 0 {
        return -1.0;
    }
    let (side, part) = (side as usize, part as usize);
    with(|st| {
        reap_grips(st, 0, 0);
        let mut answer = -1.0;
        let current = st.entities[side].lock();
        let g = &mut st.grips[side];
        if g.on && !g.letting && g.anchor.live && g.anchor.owner == owner && g.part == part && current.instance == entity.instance {
            g.asked = tick();
            answer = g.miss;
        } else if !g.on && begin(st, side, part, entity.instance as usize, object(), &[0.0; 3]) {
            let g = &mut st.grips[side];
            g.anchor.on = true;
            g.anchor.live = true;
            g.anchor.side = side as i32;
            g.anchor.owner = owner;
            g.anchor.bone = match segment {
                0 => g.upper,
                1 => g.mid,
                _ => g.end,
            };
            st.entities[side] = WeakHandle::from_handle(entity);
            answer = 0.0;
            log!("[npcgrip] {} hand binds owner {} to bone {} (segment {})\n", if side == 1 { "right" } else { "left" }, owner, st.grips[side].anchor.bone, segment);
        }
        publish_activity(st);
        answer
    })
}
/// VRNpcGripState's rows: one locked snapshot of the actually drawn limb, including its reference frames (no actor or
/// rig pointers). Empty when that hand holds no bounded part solved in the last 250 ms.
pub fn grip_state(side: i32) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    if !(0..2).contains(&side) {
        return out;
    }
    with(|st| {
        let g = &st.grips[side as usize];
        if !(g.on && g.bounded && g.solved.at != 0 && tick().wrapping_sub(g.solved.at) < 250) {
            return;
        }
        out.push([g.part as f32, g.miss, g.passes as f32, 1.0]);
        for p in &g.solved.point {
            out.push([p[0], p[1], p[2], 1.0]);
        }
        for q in &g.solved.local {
            out.push(*q);
        }
        for q in [g.upper_rest, g.hinge.rest, g.end_rest] {
            out.push(q);
        }
        let off = if g.anchor.on { [0.0; 3] } else { g.offset };
        out.push([g.target[0] + off[0], g.target[1] + off[1], g.target[2] + off[2], g.hinge.rest_angle]);
        out.push([g.hinge.axis[0], g.hinge.axis[1], g.hinge.axis[2], g.hinge.sign]);
        out.push([g.hinge.outgoing[0], g.hinge.outgoing[1], g.hinge.outgoing[2], g.hinge.maximum]);
        out.push([g.hinge.incoming[0], g.hinge.incoming[1], g.hinge.incoming[2], g.hinge.twist]);
        if g.anchor.on && g.anchor.seeded {
            let a = &g.anchor;
            out.push([a.bone as f32, a.owner as f32, a.serial as f32, if a.live { 1.0 } else { 0.0 }]);
            out.push([a.point[0], a.point[1], a.point[2], g.miss]);
            out.push([a.local[0], a.local[1], a.local[2], 1.0]);
            out.push([a.world_point[0], a.world_point[1], a.world_point[2], 1.0]);
            out.push([a.world_goal[0], a.world_goal[1], a.world_goal[2], 1.0]);
            out.push(a.rotation);
            out.push(a.world_rotation);
        }
    });
    out
}
