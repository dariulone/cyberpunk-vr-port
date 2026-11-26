//! The primary avatar's input pose for the physics step: begun on the animation thread with the frame (entity and
//! tracking), filled by note_target / note_forearm, committed after the final FK pass with V's own body and the drawn
//! wrists, read whole by the physics step (one snapshot for both hands), and the script's and the NPC grips' readers.
use super::math::{cross, qconj, qmul, qnorm, qrot};
use super::seq::Seq;
use super::tick_ms;
use super::types::{DrawnHand, Forearm, PoseFrame, PoseInputs, SelfCap, Shoulder, Target, MOST_SELF};
use core::sync::atomic::{AtomicU32, Ordering::*};
use cvr_host::log;
use cvr_physhands::handspace::{self, Rotation, Vector};
use cvr_physhands::person::GripControl;
use cvr_settings::lc;
use std::cell::{Cell, RefCell};
use std::sync::Mutex;

pub static TARGETS: [Seq<Target>; 2] = [Seq::new(Target { pos: [0.0; 3], rot: [0.0; 4], entity_pos: [0.0; 3], entity_rot: [0.0; 4], at: 0 }), Seq::new(Target { pos: [0.0; 3], rot: [0.0; 4], entity_pos: [0.0; 3], entity_rot: [0.0; 4], at: 0 })];
/// steps tracking was past the arm's reach (statistics)
pub static REACH_HELD: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
pub static SHOULDERS: [Seq<Shoulder>; 2] = [Seq::new(Shoulder { model: [0.0; 3], reach: 0.0, at: 0 }), Seq::new(Shoulder { model: [0.0; 3], reach: 0.0, at: 0 })];
pub static FOREARMS: [Seq<Forearm>; 2] = [Seq::new(Forearm { elbow: [0.0; 3], at: 0 }), Seq::new(Forearm { elbow: [0.0; 3], at: 0 })];
/// Compatibility / public display readers. Physics holds use the matching drawn wrist and raw controller target inside
/// the single step snapshot.
pub static DRAWN_HANDS: [Seq<DrawnHand>; 2] = [Seq::new(DrawnHand { p: [0.0; 3], q: [0.0; 4], at: 0 }), Seq::new(DrawnHand { p: [0.0; 3], q: [0.0; 4], at: 0 })];

static POSE_INPUTS: Mutex<Option<PoseInputs>> = Mutex::new(None);
thread_local! {
    static PENDING: RefCell<PoseInputs> = RefCell::new(PoseInputs::default());
    static PENDING_ON: Cell<bool> = const { Cell::new(false) };
    static PHYSICS: RefCell<PoseInputs> = RefCell::new(PoseInputs::default());
    static PHYSICS_DEPTH: Cell<u32> = const { Cell::new(0) };
}
/// The physics thread's one snapshot for the complete step.
pub static mut step_pose_inputs: PoseInputs = unsafe { core::mem::zeroed() };

/// BeginPoseFrame: one primary avatar pose, under the animation pose lock.
pub fn begin_pose_frame(frame: &PoseFrame) {
    PENDING.with(|p| *p.borrow_mut() = PoseInputs { frame: *frame, at: tick_ms(), ..Default::default() });
    // SAFETY: the live controls block, read.
    let on = frame.entity.valid && lc!(xrHandPhysics) != 0 && lc!(xrHandCollision) != 0;
    PENDING_ON.with(|c| c.set(on));
}
pub fn pose_frame_pending() -> bool {
    PENDING_ON.with(Cell::get)
}
/// The pending frame, while a primary avatar pose is being built on this thread.
pub fn pending_frame() -> Option<PoseFrame> {
    pose_frame_pending().then(|| PENDING.with(|p| p.borrow().frame))
}
/// The NPC grip applied to the drawn wrist, recorded in the pending pose (for the grip trace).
pub fn set_applied_grip(side: usize, attachment: &cvr_physhands::person::PersonAttachment, weight: f32) {
    if pose_frame_pending() {
        PENDING.with(|p| {
            let mut p = p.borrow_mut();
            p.applied_grip[side] = *attachment;
            p.applied_grip_weight[side] = weight;
        });
    }
}
/// ReadPoseInputs: the last published snapshot, none when it is older than 200 ms.
pub fn read_pose_inputs() -> PoseInputs {
    let g = POSE_INPUTS.lock().unwrap_or_else(|e| e.into_inner());
    match *g {
        Some(p) if p.at != 0 && tick_ms().wrapping_sub(p.at) <= 200 => p,
        _ => PoseInputs::default(),
    }
}
/// The published snapshot whatever its age (its readers check their own inputs' stamps).
pub fn published_pose_inputs() -> PoseInputs {
    POSE_INPUTS.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or_default()
}
/// The pending inputs on this (the animation) thread.
pub fn pending_inputs() -> PoseInputs {
    PENDING.with(|p| *p.borrow())
}
/// The pose a reader uses: the physics callback's own (begun by begin_physics_pose_frame), else the published one.
pub fn reader_pose() -> PoseInputs {
    if PHYSICS_DEPTH.with(Cell::get) != 0 {
        PHYSICS.with(|p| *p.borrow())
    } else {
        read_pose_inputs()
    }
}
/// Begin / EndPhysicsPoseFrame: one immutable avatar pose shared across a physics callback or an NPC grip batch.
pub fn begin_physics_pose_frame() {
    let d = PHYSICS_DEPTH.with(Cell::get);
    PHYSICS_DEPTH.with(|c| c.set(d + 1));
    if d == 0 {
        let p = read_pose_inputs();
        PHYSICS.with(|x| *x.borrow_mut() = p);
    }
}
pub fn end_physics_pose_frame() {
    PHYSICS_DEPTH.with(|c| c.set(c.get().saturating_sub(1)));
}

extern "C" {
    static CyberpunkVR_PlayerEntityPos: [f32; 3];
    static CyberpunkVR_PlayerEntityQuat: [f32; 4];
}
/// PoseEntity: the pending frame's entity, else the player's entity as the game thread wrote it.
pub fn pose_entity_now() -> ([f32; 3], [f32; 4]) {
    if pose_frame_pending() {
        return PENDING.with(|p| {
            let e = p.borrow().frame.entity;
            (e.position.to_array(), e.rotation.to_array())
        });
    }
    // SAFETY: the exported floats, read.
    unsafe { (core::ptr::read_volatile(&raw const CyberpunkVR_PlayerEntityPos), core::ptr::read_volatile(&raw const CyberpunkVR_PlayerEntityQuat)) }
}

/// The step's target / shoulder / forearm: the step snapshot's when there is one, else the last written.
pub unsafe fn step_target(side: usize) -> Option<Target> {
    if step_pose_inputs.at != 0 {
        let t = step_pose_inputs.target[side];
        return (t.at != 0).then_some(t);
    }
    TARGETS[side].read()
}
pub unsafe fn step_shoulder(side: usize) -> Option<Shoulder> {
    if step_pose_inputs.at != 0 {
        let t = step_pose_inputs.shoulder[side];
        return (t.at != 0).then_some(t);
    }
    SHOULDERS[side].read()
}
pub unsafe fn step_forearm(side: usize) -> Option<Forearm> {
    if step_pose_inputs.at != 0 {
        let t = step_pose_inputs.forearm[side];
        return (t.at != 0).then_some(t);
    }
    FOREARMS[side].read()
}

/// NoteTarget (VRIK_ApplyHandStop): where tracking puts the hand bone this frame, model space (the rotation as it is
/// solved).
pub fn note_target(side: usize, model_pos: &[f32; 3], model_rot: &[f32; 4]) {
    if side > 1 {
        return;
    }
    let (entity_pos, entity_rot) = pose_entity_now();
    let pending = pose_frame_pending();
    let t = Target { pos: *model_pos, rot: *model_rot, entity_pos, entity_rot, at: if pending { PENDING.with(|p| p.borrow().at) } else { tick_ms() } };
    if pending {
        PENDING.with(|p| p.borrow_mut().target[side] = t);
    } else {
        TARGETS[side].write(&t);
    }
    super::stats::note_tracking(side, model_pos);
}
/// NoteForearm (VRIK_SolveArm, once the arm is solved): the elbow, the wrist and the hand's turn as drawn, model space.
/// The forearm's capsule on the hand's body is posed by them; the shoulder says whether an arm reaches where tracking
/// is, and `reach` (m) is the farthest the drawn hand gets from that shoulder (the solve's stretch caps), 0 for none.
pub fn note_forearm(side: usize, elbow_model: &[f32; 3], wrist_model: &[f32; 3], hand_model_rot: &[f32; 4], shoulder_model: Option<&[f32; 3]>, reach: f32) {
    if side > 1 {
        return;
    }
    let pending = pose_frame_pending();
    if let Some(sm) = shoulder_model.filter(|s| (s[0] + s[1] + s[2]).is_finite()) {
        let mut sh = Shoulder { model: *sm, reach, at: tick_ms() };
        if !sh.reach.is_finite() || sh.reach < 0.3 || sh.reach > 1.2 {
            sh.reach = 0.0; // not an arm: no limit from it
        }
        if pending {
            PENDING.with(|p| p.borrow_mut().shoulder[side] = sh);
        } else {
            SHOULDERS[side].write(&sh);
        }
    }
    let d = [elbow_model[0] - wrist_model[0], elbow_model[1] - wrist_model[1], elbow_model[2] - wrist_model[2]];
    let mut q = qconj(hand_model_rot);
    if !qnorm(&mut q) {
        return;
    }
    let f = Forearm { elbow: qrot(&q, &d), at: tick_ms() };
    if pending {
        PENDING.with(|p| p.borrow_mut().forearm[side] = f);
    } else {
        FOREARMS[side].write(&f);
    }
}

/// m V's clothes add round the trunk (the script, from what is worn)
pub static SELF_PAD: AtomicU32 = AtomicU32::new(0);
static SELF_SAID: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// V'S OWN BODY for the hands, the game's world, from the pose the arm solve left (the FK snapshot, model space) and the
/// player's place: the trunk as two columns side by side from the waist (the first spine joint) to the top of the chest
/// (the last), a little ahead of the spine, which is near the back; the crown of the head round the head's joint, by
/// the head's own up and forward -- not the face, so a hand brought to the mouth or the eyes is not stopped; and each
/// upper arm, from below the shoulder to short of the elbow, for the other hand. No hips and no thighs: the holsters
/// are there. Called only at the primary avatar's completed FK pass, under its pose lock.
unsafe fn build_self_capsules(out: &mut [SelfCap; MOST_SELF], entity_position: &[f32; 3], entity_rotation: &[f32; 4]) -> i32 {
    use super::super::env::*;
    use super::super::shared_globals::{g_fkPos, g_fkRot};
    if !super::census::player_valid() {
        return 0;
    }
    let pad = f32::from_bits(SELF_PAD.load(Relaxed)).clamp(0.0, 0.04);
    let count = super::super::rig::VRIK_FKCount().min(super::super::MAX_BONES as i32);
    let have = |i: i32| i >= 0 && i < count;
    let spines = g_VRSpineCount.clamp(0, 8);
    let (hips, neck, head) = (g_VRHipsIdx, g_VRNeckIdx, g_VRHeadBoneIdx);
    let (la, ra, lf, rf) = (g_VRLeftUpperArmIdx, g_VRRightUpperArmIdx, g_VRLeftForeArmIdx, g_VRRightForeArmIdx);
    if spines < 2 || !have(hips) || !have(neck) || !have(la) || !have(ra) || !have(g_VRSpineIdx[0]) || !have(g_VRSpineIdx[(spines - 1) as usize]) {
        return 0;
    }
    let at = |i: i32| -> Option<[f32; 3]> {
        let p = g_fkPos[i as usize];
        (p[0] + p[1] + p[2]).is_finite().then_some(p)
    };
    let len = |v: &[f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dot = |a: &[f32; 3], b: &[f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let unit = |v: &mut [f32; 3]| {
        let n = len(v);
        if !(n > 1e-5) {
            return false;
        }
        for c in v.iter_mut() {
            *c /= n;
        }
        true
    };
    let (Some(h), Some(nk), Some(lap), Some(rap), Some(s0), Some(st)) = (at(hips), at(neck), at(la), at(ra), at(g_VRSpineIdx[0]), at(g_VRSpineIdx[(spines - 1) as usize])) else { return 0 };
    let mut up = [nk[0] - h[0], nk[1] - h[1], nk[2] - h[2]];
    let mut lat = [rap[0] - lap[0], rap[1] - lap[1], rap[2] - lap[2]];
    let (up_len, lat_len) = (len(&up), len(&lat));
    if !(up_len > 0.25 && up_len < 1.0) || !(lat_len > 0.15 && lat_len < 0.7) {
        return 0;
    }
    unit(&mut up);
    let along = dot(&lat, &up);
    for k in 0..3 {
        lat[k] -= up[k] * along;
    }
    if !unit(&mut lat) {
        return 0;
    }
    let fwd = cross(&up, &lat); // up x right: forward
    let mut n = 0usize;
    for s in [-1.0f32, 1.0] {
        let c = &mut out[n];
        n += 1;
        for k in 0..3 {
            c.a[k] = s0[k] + lat[k] * (s * 0.06) + fwd[k] * 0.05;
            c.b[k] = st[k] + lat[k] * (s * 0.065) + fwd[k] * 0.05;
        }
        c.r = 0.105 + pad;
        c.meets = 3;
    }
    if have(head) {
        if let Some(hd) = at(head) {
            let mut q = g_fkRot[head as usize];
            let mut hu = [hd[0] - nk[0], hd[1] - nk[1], hd[2] - nk[2]];
            if !unit(&mut hu) {
                hu = up;
            }
            let mut hf = fwd;
            if qnorm(&mut q) {
                // the head's own axes: of its frame's three, the one nearest the neck's line is its up, of the other two
                // the one nearest the body's forward its forward
                let axes: [[f32; 3]; 3] = core::array::from_fn(|a| {
                    let e = [f32::from(u8::from(a == 0)), f32::from(u8::from(a == 1)), f32::from(u8::from(a == 2))];
                    qrot(&q, &e)
                });
                let mut iu = 0;
                for a in 1..3 {
                    if dot(&axes[a], &hu).abs() > dot(&axes[iu], &hu).abs() {
                        iu = a;
                    }
                }
                let mut jf = if iu == 0 { 1 } else { 0 };
                for a in 0..3 {
                    if a != iu && dot(&axes[a], &fwd).abs() > dot(&axes[jf], &fwd).abs() {
                        jf = a;
                    }
                }
                if dot(&axes[iu], &hu).abs() > 0.6 && dot(&axes[jf], &fwd).abs() > 0.5 {
                    let su = if dot(&axes[iu], &hu) < 0.0 { -1.0 } else { 1.0 };
                    let sf = if dot(&axes[jf], &fwd) < 0.0 { -1.0 } else { 1.0 };
                    for k in 0..3 {
                        hu[k] = axes[iu][k] * su;
                        hf[k] = axes[jf][k] * sf;
                    }
                }
            }
            let c = &mut out[n];
            n += 1;
            for k in 0..3 {
                let centre = hd[k] + hu[k] * 0.095 - hf[k] * 0.01;
                c.a[k] = centre - lat[k] * 0.012;
                c.b[k] = centre + lat[k] * 0.012;
            }
            c.r = 0.075;
            c.meets = 3;
        }
    }
    for s in 0..2 {
        let (a, f) = if s == 1 { (ra, rf) } else { (la, lf) };
        if !have(f) {
            continue;
        }
        let (Some(ap), Some(fp)) = (at(a), at(f)) else { continue };
        let d = [fp[0] - ap[0], fp[1] - ap[1], fp[2] - ap[2]];
        let l = len(&d);
        if !(l > 0.15 && l < 0.5) {
            continue;
        }
        let c = &mut out[n];
        n += 1;
        for k in 0..3 {
            c.a[k] = ap[k] + d[k] * 0.22;
            c.b[k] = ap[k] + d[k] * 0.85;
        }
        c.r = 0.045 + 0.6 * pad;
        c.meets = if s == 1 { 1 } else { 2 }; // the right upper arm for the left hand, the left for the right
    }
    // SEATED, THE THIGHS: a hand laid on the lap lies on them (on foot they would stand between a hand and the hip's
    // holsters, so only in a vehicle)
    if core::ptr::read_volatile(&raw const g_isInVehicle) {
        for (up, knee) in [(g_VRLeftUpLegIdx, g_VRLeftLegIdx), (g_VRRightUpLegIdx, g_VRRightLegIdx)] {
            if n >= MOST_SELF || !have(up) || !have(knee) {
                continue;
            }
            let (Some(hp), Some(kp)) = (at(up), at(knee)) else { continue };
            let d = [kp[0] - hp[0], kp[1] - hp[1], kp[2] - hp[2]];
            let l = len(&d);
            if !(l > 0.25 && l < 0.7) {
                continue;
            }
            let c = &mut out[n];
            n += 1;
            for k in 0..3 {
                c.a[k] = hp[k] + d[k] * 0.2;
                c.b[k] = hp[k] + d[k] * 0.9;
            }
            c.r = 0.075 + pad;
            c.meets = 3;
        }
    }
    // model space to the game's world
    let mut eq = *entity_rotation;
    if !qnorm(&mut eq) {
        return 0;
    }
    for c in out.iter_mut().take(n) {
        for p in [&mut c.a, &mut c.b] {
            let w = qrot(&eq, p);
            for k in 0..3 {
                p[k] = entity_position[k] + w[k];
            }
        }
    }
    if !SELF_SAID.swap(true, Relaxed) {
        log!(
            "[physhands] V's own body for the hands: {} capsules (trunk {:.2} m, shoulders {:.2} m apart; forward in the model {:.2} {:.2} {:.2})\n",
            n,
            len(&up) * up_len,
            lat_len,
            fwd[0],
            fwd[1],
            fwd[2]
        );
    }
    n as i32
}

/// CommitPoseInputs (PublishPoseFrame): V's own body and the drawn wrists added, the whole snapshot published. Held
/// bodies follow the completed wrist, just like an attachment on the weapon bone: publishing the pre-IK target here
/// used to put the object ahead of the visible hand whenever arm reach or a later pose layer moved it.
pub unsafe fn publish_pose_frame() {
    use super::super::env::{g_VRLeftBoneIdx, g_VRRightBoneIdx};
    use super::super::shared_globals::{g_fkPos, g_fkRot};
    if !pose_frame_pending() {
        return;
    }
    let (position, rotation) = pose_entity_now();
    let mut pending = PENDING.with(|p| *p.borrow());
    pending.body.n = build_self_capsules(&mut pending.body.c, &position, &rotation);
    pending.body.at = pending.at;
    let count = super::super::rig::VRIK_FKCount();
    for side in 0..2 {
        let mut hand = DrawnHand::default();
        let bone = if side == 1 { g_VRRightBoneIdx } else { g_VRLeftBoneIdx };
        if pending.target[side].at != 0 && bone >= 0 && bone < count {
            let p = qrot(&rotation, &g_fkPos[bone as usize]);
            hand.p = [p[0] + position[0], p[1] + position[1], p[2] + position[2]];
            hand.q = qmul(&rotation, &g_fkRot[bone as usize]);
            if qnorm(&mut hand.q) && (hand.p[0] + hand.p[1] + hand.p[2]).is_finite() {
                hand.at = pending.at;
            }
        }
        pending.drawn[side] = hand;
        PENDING.with(|p| p.borrow_mut().drawn[side] = hand);
        super::trace::trace_final_grip(side, &pending);
        if hand.at != 0 {
            let palm = &mut pending.palm[side];
            match super::super::grasp::grasp_palm_frame(side) {
                Some((skin, normal, along)) => {
                    palm.skin = skin;
                    palm.normal = normal;
                    palm.along = along;
                    palm.valid = true;
                }
                None => palm.valid = false,
            }
        }
    }
    *POSE_INPUTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(pending);
    // Compatibility / freshness readers do not use these to pose the step. Its targets, arm reach and body geometry
    // come from the one snapshot above.
    for side in 0..2 {
        DRAWN_HANDS[side].write(&pending.drawn[side]);
        TARGETS[side].write(&pending.target[side]);
        SHOULDERS[side].write(&pending.shoulder[side]);
        FOREARMS[side].write(&pending.forearm[side]);
    }
    PENDING.with(|p| *p.borrow_mut() = pending);
    PENDING_ON.with(|c| c.set(false));
}
/// SelfCapsules: V's own body as the step snapshot has it.
pub unsafe fn self_capsules(out: &mut [SelfCap; MOST_SELF]) -> i32 {
    if step_pose_inputs.at == 0 || !step_pose_inputs.frame.entity.valid {
        return 0;
    }
    let n = step_pose_inputs.body.n.clamp(0, MOST_SELF as i32) as usize;
    out[..n].copy_from_slice(&step_pose_inputs.body.c[..n]);
    n as i32
}

fn v3(p: &[f32; 3]) -> Vector {
    Vector::new(p[0], p[1], p[2])
}
fn r4(q: &[f32; 4]) -> Rotation {
    Rotation::new(q[0], q[1], q[2], q[3])
}
/// DrawnWorld: the fresh constrained wrist in world space, for contact interactions.
pub fn drawn_world(side: usize) -> Option<[f32; 3]> {
    if side > 1 {
        return None;
    }
    if let Some((p, _)) = solved_world_pose(side) {
        return Some(p);
    }
    DRAWN_HANDS[side].read().filter(|h| tick_ms().wrapping_sub(h.at) <= 200).map(|h| h.p)
}
/// SolvedWorldPose: the completed primary-avatar wrist, with position and orientation from the same entity/pose
/// sample. Never reads the mutable FK diagnostic arrays.
pub fn solved_world_pose(side: usize) -> Option<([f32; 3], [f32; 4])> {
    if side > 1 {
        return None;
    }
    let pose = reader_pose();
    let hand = pose.drawn[side];
    if pose.at == 0 || !pose.frame.entity.valid || hand.at == 0 || tick_ms().wrapping_sub(hand.at) > 200 {
        return None;
    }
    Some((hand.p, hand.q))
}
/// ReadGripControl: the controller's place for a held person's part and the arm that reaches it.
pub fn read_grip_control(side: usize) -> Option<GripControl> {
    if side > 1 {
        return None;
    }
    let pose = reader_pose();
    let (target, shoulder) = (pose.target[side], pose.shoulder[side]);
    if pose.at == 0 || !pose.frame.entity.valid || target.at == 0 || shoulder.at == 0 || !(shoulder.reach > 0.01) {
        return None;
    }
    let position = handspace::point(&pose.frame.entity, v3(&target.pos));
    let mut rotation = handspace::multiply(pose.frame.entity.rotation, r4(&target.rot));
    let center = handspace::point(&pose.frame.entity, v3(&shoulder.model));
    if !handspace::finite(position) || !handspace::finite(center) || !handspace::normalize(&mut rotation) {
        return None;
    }
    Some(GripControl { position: position.to_array(), rotation: rotation.to_array(), shoulder: center.to_array(), reach: shoulder.reach, at: pose.at })
}
/// ScriptHandFrame: the script's rows -- solved world position / rotation, control world position / rotation, palm
/// skin in the wrist, world palm normal / along, solved model wrist, world palm.
pub fn script_hand_frame(side: usize) -> Option<[[f32; 4]; 9]> {
    if side > 1 {
        return None;
    }
    let pose = reader_pose();
    let (hand, target, palm) = (pose.drawn[side], pose.target[side], pose.palm[side]);
    if pose.at == 0 || !pose.frame.entity.valid || hand.at == 0 || target.at == 0 || !palm.valid || tick_ms().wrapping_sub(hand.at) > 200 {
        return None;
    }
    let entity = pose.frame.entity;
    let drawn = r4(&hand.q);
    let mut control = handspace::multiply(entity.rotation, r4(&target.rot));
    let raw = handspace::point(&entity, v3(&target.pos));
    if !handspace::normalize(&mut control) || !handspace::finite(raw) {
        return None;
    }
    let row3 = |v: Vector| [v.x, v.y, v.z, 1.0];
    let row4 = |q: Rotation| [q.x, q.y, q.z, q.w];
    Some([
        row3(v3(&hand.p)),
        row4(drawn),
        row3(raw),
        row4(control),
        row3(v3(&palm.skin)),
        row3(handspace::rotate(drawn, v3(&palm.normal))),
        row3(handspace::rotate(drawn, v3(&palm.along))),
        row3(handspace::rotate(handspace::conjugate(entity.rotation), v3(&hand.p) - entity.position)),
        row3(v3(&hand.p) + handspace::rotate(drawn, v3(&palm.skin))),
    ])
}
