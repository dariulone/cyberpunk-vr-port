//! FINGERS ON THE WORLD (main's src/Anim/FingerContact.cpp). A hand with a body of its own (physhands) stops at a
//! table; its fingers bend on it, each joint about its own anatomical hinge and never past its range, the last joints
//! giving before the knuckles. The solve runs on the physics hands' worker against the world's shapes round the player;
//! this file only says where the fingers are and turns them by what comes back.
//!
//! WHERE THEY ARE: each finger's three bones in the hand bone's frame as every finger layer left them this pass, each
//! joint's hinge -- the bone axis across the bone nearest the one that takes its segment toward the palm -- and how far
//! it may turn from where it is now: its range measured from the straight finger. The chains are the ladder profile's
//! slots: index..little proximal 6-9, middle 11-14, distal 15-18; the thumb's metacarpal 0 (not turned here: it sits in
//! the palm), proximal 5, distal 10.
//!
//! WHAT COMES BACK is a turn per joint. The engine applies the player pose four or five times a tick and every pass of a
//! tick must leave the same fingers, so the turns are taken once a batch and written again in every pass of it.
use super::env::*;
use super::physhands::types::{FingerChain, FingerHand};
use super::rig::{VRIK_LadderFingerBone, VRIK_QuatConj, VRIK_QuatMul, VRIK_QuatNorm, VRIK_QuatRotateVec, VRIK_ReferenceFingerRotation};
use super::*;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use cvr_body::cmath::{atan2, clamp, cos, exp, max, min, sin, sqrt};
use cvr_host::log;

extern "C" {
    static g_hasWeaponEquipped: bool;
    static mut CyberpunkVR_VrikBatchGapMs: f32;
}
const SLOT_DUAL_WIELD_ACTIVE: usize = 165;

const CHAINS: [[i32; 3]; 5] = [[6, 11, 15], [7, 12, 16], [8, 13, 17], [9, 14, 18], [0, 5, 10]];
/// The segments' capsules (m), the grasp's; the thumb's metacarpal is in the palm and is not tested.
const RADIUS: [[f32; 3]; 5] = [[0.0095, 0.0085, 0.0075], [0.0095, 0.0085, 0.0075], [0.0095, 0.0085, 0.0075], [0.009, 0.008, 0.007], [0.0, 0.0105, 0.0095]];
/// EACH JOINT'S ANATOMICAL RANGE, degrees from the STRAIGHT finger, flexion positive: the knuckle (MCP) to 90 and back
/// 30 under a load, the middle joint (PIP) to 105 (the little finger's 110) and hardly past straight, the last (DIP) to 85
/// and back 20 pressed on its pad; the thumb's MCP 55 / -10, its IP 80 / -20 (Norkin & White 2009; AAOS 1965; Lin, Wu &
/// Huang 2000).
const LO: [[f32; 3]; 5] = [[-30.0, -8.0, -20.0], [-30.0, -8.0, -20.0], [-30.0, -8.0, -20.0], [-30.0, -8.0, -20.0], [0.0, -10.0, -20.0]];
const HI: [[f32; 3]; 5] = [[90.0, 105.0, 85.0], [90.0, 105.0, 85.0], [90.0, 105.0, 85.0], [90.0, 110.0, 85.0], [0.0, 55.0, 80.0]];
const DEG: f32 = 3.141_592_65 / 180.0;

#[derive(Clone, Copy)]
struct Pose {
    q: Q,
    p: V3,
}
const POSE0: Pose = Pose { q: ID, p: [0.0; 3] };

fn rotv(q: &Q, v: &V3) -> V3 {
    let mut o = [0.0f32; 3];
    // SAFETY: three and four floats.
    unsafe { VRIK_QuatRotateVec(q.as_ptr(), v.as_ptr(), o.as_mut_ptr()) };
    o
}
fn qmul(a: &Q, b: &Q) -> Q {
    let mut o = [0.0f32; 4];
    // SAFETY: four floats each.
    unsafe { VRIK_QuatMul(a.as_ptr(), b.as_ptr(), o.as_mut_ptr()) };
    o
}
fn qconj(q: &Q) -> Q {
    let mut o = [0.0f32; 4];
    // SAFETY: four floats each.
    unsafe { VRIK_QuatConj(q.as_ptr(), o.as_mut_ptr()) };
    o
}
fn qnorm(q: &mut Q) {
    // SAFETY: four floats.
    unsafe { VRIK_QuatNorm(q.as_mut_ptr()) };
}
unsafe fn valid(b: i32) -> bool {
    b >= 0 && b < g_VRBoneCount && b < MAXB
}
unsafe fn local(buf: *const u8, bone: i32) -> Pose {
    let base = buf.add(bone as usize * 48);
    let t = base as *const f32;
    let q = base.add(16) as *const f32;
    let mut o = Pose { q: [*q, *q.add(1), *q.add(2), *q.add(3)], p: [*t, *t.add(1), *t.add(2)] };
    qnorm(&mut o.q);
    o
}
fn compose(a: &Pose, b: &Pose) -> Pose {
    let r = rotv(&a.q, &b.p);
    let mut q = qmul(&a.q, &b.q);
    qnorm(&mut q);
    Pose { q, p: [a.p[0] + r[0], a.p[1] + r[1], a.p[2] + r[2]] }
}
/// Bone b in the frame of its ancestor `root`.
unsafe fn in_frame(buf: *const u8, bone: i32, root: i32) -> Option<Pose> {
    let mut chain = [0i32; 32];
    let mut n = 0;
    let mut b = bone;
    while b != root {
        if !valid(b) || n >= 32 {
            return None;
        }
        chain[n] = b;
        n += 1;
        b = g_VRBoneParent[b as usize] as i32;
    }
    let mut acc = POSE0;
    for i in (0..n).rev() {
        acc = compose(&acc, &local(buf, chain[i]));
    }
    Some(acc)
}
fn dot(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: &V3, b: &V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: &mut V3) -> bool {
    let l = sqrt(dot(a, a));
    if !(l > 1e-8) {
        return false;
    }
    for v in a.iter_mut() {
        *v /= l;
    }
    true
}
unsafe fn tip_offset(buf: *const u8, distal: i32) -> V3 {
    let mut i = 0;
    while i < g_VRBoneCount && i < MAXB {
        if g_VRBoneParent[i as usize] as i32 == distal {
            let c = local(buf, i);
            if sqrt(dot(&c.p, &c.p)) > 0.004 {
                return c.p;
            }
        }
        i += 1;
    }
    let d = local(buf, distal);
    [0.75 * d.p[0], 0.75 * d.p[1], 0.75 * d.p[2]]
}
/// THE HINGE IS ONE OF THE BONE'S OWN AXES: the one across the bone (y or z: the rig's finger bones run along x) nearest
/// k (k in the hand's frame, the bone turned by q there). A finger joint is a hinge, and the rig turns every finger bone
/// about its own z.
fn local_hinge(q: &Q, k: &V3) -> V3 {
    let out = rotv(&qconj(q), k);
    let best = if out[2].abs() >= out[1].abs() { 2 } else { 1 };
    let mut o = [0.0f32; 3];
    o[best] = if out[best] >= 0.0 { 1.0 } else { -1.0 };
    o
}
/// The palm's normal on the side the fingers close to, in the hand's frame (as the grasp's measure_palm finds it).
unsafe fn palm_normal(buf: *const u8, side: i32, hand: i32) -> Option<V3> {
    let index1 = VRIK_LadderFingerBone(side, 6);
    let middle1 = VRIK_LadderFingerBone(side, 7);
    let little1 = VRIK_LadderFingerBone(side, 9);
    let middle2 = VRIK_LadderFingerBone(side, 12);
    for b in [index1, middle1, little1, middle2] {
        if !valid(b) {
            return None;
        }
    }
    let i1 = in_frame(buf, index1, hand)?;
    let m1 = in_frame(buf, middle1, hand)?;
    let l1 = in_frame(buf, little1, hand)?;
    let mut d = m1.p;
    if !unit(&mut d) {
        return None;
    }
    let across = [i1.p[0] - l1.p[0], i1.p[1] - l1.p[1], i1.p[2] - l1.p[2]];
    let along = dot(&across, &d);
    let mut t = [across[0] - along * d[0], across[1] - along * d[1], across[2] - along * d[2]];
    if !unit(&mut t) {
        return None;
    }
    let mut n = cross(&d, &t);
    if !unit(&mut n) {
        return None;
    }
    // the rig's flexion turns the middle finger's proximal phalanx about its own -Z: its next joint goes to the palm
    // side. Asked of the phalanx as the reference hand turns it (straight): as the animation turns it, a fist's knuckle
    // past 90 deg sent the next joint back the other way, and the normal came out reversed (2026-11-09).
    let mut m1ref = m1;
    {
        let mut reference = [0.0f32; 4];
        if let Some(parent) = in_frame(buf, g_VRBoneParent[middle1 as usize] as i32, hand) {
            if VRIK_ReferenceFingerRotation(middle1, reference.as_mut_ptr()) {
                let mut l = local(buf, middle1);
                l.q = reference;
                qnorm(&mut l.q);
                m1ref = compose(&parent, &l);
            }
        }
    }
    let next = local(buf, middle2);
    let z = [0.0f32, 0.0, -1.0];
    let s = sin(10.0 * DEG);
    let bend = [z[0] * s, z[1] * s, z[2] * s, cos(10.0 * DEG)];
    let turned = qmul(&m1ref.q, &bend);
    let a = rotv(&m1ref.q, &next.p);
    let b = rotv(&turned, &next.p);
    let moved = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    if dot(&moved, &n) < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    Some(n)
}
/// EACH JOINT'S FLEXION FROM THE STRAIGHT FINGER, radians: the angle each bone makes with the one before about its hinge
/// -- the first with the palm's own line (the hand frame's z taken out), the last bone's line to the rig's own fingertip.
fn flexion(base: &[f32; 7], local_q: &[Q; 3], locals: &[[f32; 7]; 3], hinge: &[V3; 3], tip: &V3) -> [f32; 3] {
    let mut acc = Pose { q: [base[0], base[1], base[2], base[3]], p: [base[4], base[5], base[6]] };
    let mut f = [POSE0; 3];
    let mut jts = [[0.0f32; 3]; 4];
    for j in 0..3 {
        let l = Pose { q: local_q[j], p: [locals[j][4], locals[j][5], locals[j][6]] };
        acc = compose(&acc, &l);
        f[j] = acc;
        jts[j] = acc.p;
    }
    let r = rotv(&f[2].q, tip);
    for k in 0..3 {
        jts[3][k] = f[2].p[k] + r[k];
    }
    let mut out = [0.0f32; 3];
    let mut prev = [jts[0][0], jts[0][1], 0.0];
    for j in 0..3 {
        let seg = [jts[j + 1][0] - jts[j][0], jts[j + 1][1] - jts[j][1], jts[j + 1][2] - jts[j][2]];
        let axis = rotv(&f[j].q, &hinge[j]);
        let cr = cross(&prev, &seg);
        out[j] = atan2(dot(&cr, &axis), dot(&prev, &seg));
        if !out[j].is_finite() {
            out[j] = 0.0;
        }
        prev = seg;
    }
    out
}
/// for the trace's log: each joint's flexion as the animation has it and as the rig's reference hand has it, radians;
/// and the animation's turn of each joint off its hinge (the lean and twist the hinge-only pose drops), radians
static mut FLEX_ANIM: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut FLEX_REF: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut OFF_HINGE: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
fn quat_axis_angle(axis: &V3, a: f32) -> Q {
    let s = sin(0.5 * a);
    [axis[0] * s, axis[1] * s, axis[2] * s, cos(0.5 * a)]
}
/// THE JOINT AS A HINGE: the local turn `anim` of a finger bone taken from the rig's straight reference `reference` and
/// kept to its turn about the hinge k -- the swing-twist split of conj(ref)*anim about k. The knuckle (spread) keeps the
/// rest's part about the axis across the hinge and the bone's line: the fingers fan out as the animation spreads them.
/// Out: the pose, and the angle of what was dropped.
fn hinge_only(reference: &Q, anim: &Q, k: &V3, knuckle: bool) -> (Q, f32) {
    let rc = qconj(reference);
    let mut d = qmul(&rc, anim);
    if d[3] < 0.0 {
        d = [-d[0], -d[1], -d[2], -d[3]];
    }
    let pk = d[0] * k[0] + d[1] * k[1] + d[2] * k[2];
    let mut t = [k[0] * pk, k[1] * pk, k[2] * pk, d[3]];
    let tn = sqrt(t[0] * t[0] + t[1] * t[1] + t[2] * t[2] + t[3] * t[3]);
    if !(tn > 1e-6) {
        t = ID;
    } else {
        for v in t.iter_mut() {
            *v /= tn;
        }
    }
    let tc = qconj(&t);
    // D = S * T
    let mut s = qmul(&d, &tc);
    if s[3] < 0.0 {
        s = [-s[0], -s[1], -s[2], -s[3]];
    }
    let sn = sqrt(s[0] * s[0] + s[1] * s[1] + s[2] * s[2]);
    let dropped = 2.0 * atan2(sn, s[3]);
    let mut keep = ID;
    let mut left = dropped;
    if knuckle && sn > 1e-7 {
        let mut y = cross(k, &[1.0, 0.0, 0.0]);
        if unit(&mut y) {
            let rv = [s[0] / sn * dropped, s[1] / sn * dropped, s[2] / sn * dropped];
            let spread = dot(&rv, &y);
            keep = quat_axis_angle(&y, spread);
            left = sqrt(max(0.0, dropped * dropped - spread * spread));
        }
    }
    let a = qmul(reference, &keep);
    let mut b = qmul(&a, &t);
    qnorm(&mut b);
    (b, left)
}
/// The palm's middle in the hand's frame, on its face: halfway from the wrist to the four knuckles, moved off the bones
/// toward the palm's side -- what the thumb's joints bend toward.
unsafe fn palm_middle(buf: *const u8, side: i32, hand: i32, n: &V3) -> Option<V3> {
    let mut sum = [0.0f32; 3];
    for slot in 6..=9 {
        let b = VRIK_LadderFingerBone(side, slot);
        if !valid(b) {
            return None;
        }
        let p = in_frame(buf, b, hand)?;
        for k in 0..3 {
            sum[k] += p.p[k];
        }
    }
    Some([sum[0] * 0.125 + n[0] * 0.015, sum[1] * 0.125 + n[1] * 0.015, sum[2] * 0.125 + n[2] * 0.015])
}
unsafe fn reference_rotations(bone: &[i32; 3]) -> Option<[Q; 3]> {
    let mut r = [[0.0f32; 4]; 3];
    for j in 0..3 {
        if !VRIK_ReferenceFingerRotation(bone[j], r[j].as_mut_ptr()) {
            return None;
        }
    }
    Some(r)
}
/// One finger's chain as drawn, its hinges and its ranges from where it is now. With `pure`, each joint's local turn
/// kept to its hinge (hinge_only) goes there and into the chain the solve is given; the bool says whether it was made.
unsafe fn chain(buf: *const u8, side: i32, hand: i32, f: usize, n: &V3, palm_mid: Option<&V3>, c: &mut FingerChain, pure: Option<&mut [Q; 3]>) -> Option<bool> {
    let mut bone = [0i32; 3];
    for j in 0..3 {
        bone[j] = VRIK_LadderFingerBone(side, CHAINS[f][j]);
        if !valid(bone[j]) {
            return None;
        }
    }
    let base = in_frame(buf, g_VRBoneParent[bone[0] as usize] as i32, hand)?;
    c.base = [base.q[0], base.q[1], base.q[2], base.q[3], base.p[0], base.p[1], base.p[2]];
    for j in 0..3 {
        let l = local(buf, bone[j]);
        c.local[j] = [l.q[0], l.q[1], l.q[2], l.q[3], l.p[0], l.p[1], l.p[2]];
    }
    c.tip = tip_offset(buf, bone[2]);
    let rig_tip = c.tip;
    // THE SKIN: V's own hand -- each segment's width, its line off the bone, the fingertip
    let skin = super::physhands::skin_for(super::physhands::body_female(), side);
    if let Some(skin) = skin {
        c.tip = skin.tip[f];
    }
    let q_of = |v: &[f32; 7]| -> Q { [v[0], v[1], v[2], v[3]] };
    let p_of = |v: &[f32; 7]| -> V3 { [v[4], v[5], v[6]] };
    // THE HINGES FROM THE STRAIGHT FINGER: the chain posed by the rig's reference hand (its fingers straight), each
    // segment's line across the palm's normal, in that bone's own frame -- the same axis in the bone however the finger
    // is posed (2026-11-09).
    let refs = reference_rotations(&bone);
    let mut h = [[0.0f32; 3]; 4];
    let mut r = [POSE0; 3];
    {
        let mut a = base;
        for j in 0..3 {
            let mut l = Pose { q: refs.map_or(q_of(&c.local[j]), |rq| rq[j]), p: p_of(&c.local[j]) };
            qnorm(&mut l.q);
            a = compose(&a, &l);
            r[j] = a;
            h[j] = a.p;
        }
        let rr = rotv(&r[2].q, if refs.is_some() { &rig_tip } else { &c.tip });
        for k in 0..3 {
            h[3][k] = r[2].p[k] + rr[k];
        }
    }
    // THE THUMB'S joints bend toward the palm's middle (as the grasp's thumb does), not toward the palm's normal: the
    // thumb flexes and extends on its nail's plane.
    let mut toward = *n;
    if f == 4 {
        if let Some(pm) = palm_mid {
            toward = [pm[0] - h[1][0], pm[1] - h[1][1], pm[2] - h[1][2]];
            if !unit(&mut toward) {
                toward = *n;
            }
        }
    }
    for j in 0..3 {
        let seg = [h[j + 1][0] - h[j][0], h[j + 1][1] - h[j][1], h[j + 1][2] - h[j][2]];
        let mut k = cross(&seg, &toward);
        if !unit(&mut k) {
            return None;
        }
        c.hinge[j] = local_hinge(&r[j].q, &k);
        c.radius[j] = skin.map_or(RADIUS[f][j], |s| s.radius[f][j]);
        c.offset[j] = skin.map_or([0.0; 3], |s| s.offset[f][j]);
        c.moves[j] = if f == 4 && j == 0 { 0 } else { 1 };
    }
    // THE JOINTS AS HINGES (physhands.ini finger_hinge_only): a straightened finger keeps none of the curled hand's lean
    // or twist off its hinges. The thumb's first bone (in the palm) stays as the animation has it.
    let mut pured = false;
    {
        let mut made = [ID; 3];
        for j in 0..3 {
            let lq = q_of(&c.local[j]);
            let p = match refs {
                Some(mut rq) if !(f == 4 && j == 0) => {
                    qnorm(&mut rq[j]);
                    let (p, left) = hinge_only(&rq[j], &lq, &c.hinge[j], j == 0 || (f == 4 && j == 1));
                    OFF_HINGE[side as usize][f][j] = left;
                    p
                }
                _ => {
                    OFF_HINGE[side as usize][f][j] = 0.0;
                    lq
                }
            };
            made[j] = p;
        }
        if let (Some(pure), true) = (pure, refs.is_some()) {
            *pure = made;
            for j in 0..3 {
                c.local[j][..4].copy_from_slice(&made[j]);
            }
            pured = true;
        }
    }
    // WHERE EACH JOINT IS IN ITS RANGE: its flexion from the straight finger now, measured on the bones' own lines about
    // the hinges above, so the range is the hand's anatomical one whatever pose the animation left the finger in.
    let q = [q_of(&c.local[0]), q_of(&c.local[1]), q_of(&c.local[2])];
    let flex = flexion(&c.base, &q, &c.local, &c.hinge, &rig_tip);
    for j in 0..3 {
        c.lo[j] = min(0.0, LO[f][j] * DEG - flex[j]);
        c.hi[j] = max(0.0, HI[f][j] * DEG - flex[j]);
        c.flex[j] = flex[j];
        FLEX_ANIM[side as usize][f][j] = flex[j];
    }
    // the reference hand the same way, for the trace's log
    FLEX_REF[side as usize][f] = refs.map_or([0.0; 3], |rq| flexion(&c.base, &rq, &c.local, &c.hinge, &rig_tip));
    Some(pured)
}

fn now_ms() -> f64 {
    let (t, f) = super::physhands::qpc();
    t as f64 * 1000.0 / f as f64
}

static mut LAST_PASS_MS: f64 = 0.0;
static mut LATCHED: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut LATCHED_FLEX: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut LATCHED_ON: [bool; 2] = [false; 2];
static mut LATCHED_FLEX_KNOWN: [bool; 2] = [false; 2];
static BENDS_REBASED: AtomicU32 = AtomicU32::new(0);
static BENDS_CLAMPED: AtomicU32 = AtomicU32::new(0);
static mut WEIGHT: [f32; 2] = [0.0; 2];
static mut SPENT_W: [f32; 2] = [0.0; 2];
/// THE TURNS AS DRAWN: eased toward what is solved, at most TURN_RATE rad/s and over TURN_TAU s, once a batch. Kept as
/// they were while the hand holds on (a person's part, a climb) once the grip has had FREEZE_MS to close on it.
const TURN_RATE: f32 = 9.0;
const TURN_TAU: f32 = 0.03;
const FREEZE_MS: f64 = 150.0;
const BACK_MOST_DEG: f32 = 20.0;
static mut DRAWN_TURN: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut HOLD_SINCE: [f64; 2] = [0.0; 2];
static mut LAST_BATCH_MS: f64 = 0.0;
static mut FROZEN: [bool; 2] = [false; 2];
static TURNS_EASED: AtomicU32 = AtomicU32::new(0);
static FRAMES_FROZEN: AtomicU32 = AtomicU32::new(0);
static mut TOLD_AT: f64 = 0.0;
static mut PAST_TOLD_AT: [u64; 2] = [0; 2];

/// FingerContact: each player pose pass, after every finger layer: both hands' fingers published as drawn and their
/// joints turned by the latest solve. `busy` per side (0 left, 1 right): the fingers belong to something else this pass
/// (a carried body, a native reload's hand-off), so they are left alone.
pub unsafe fn finger_contact(buf: *mut u8, busy: &[bool; 2]) {
    if buf.is_null() || g_VRBoneCount <= 0 {
        return;
    }
    let now = now_ms();
    let new_batch = now - LAST_PASS_MS > f64::from(CyberpunkVR_VrikBatchGapMs);
    LAST_PASS_MS = now;
    let holding = super::physhands::heldbody::holding_mask();
    for side in 0..2i32 {
        let sd = side as usize;
        let hand = if side != 0 { g_VRRightBoneIdx } else { g_VRLeftBoneIdx };
        if !valid(hand) {
            continue;
        }
        let on = super::physhands::on(sd);
        let mut h = FingerHand { at: super::physhands::tick_ms(), ..Default::default() };
        // a gun out takes the hand that holds it (both, wielding two), not the other: that one lies on the gun's boxes
        // and on the world as an empty hand does
        let dual = !g_pSharedHands.is_null() && *g_pSharedHands.add(SLOT_DUAL_WIELD_ACTIVE) > 0.5;
        let gun_hand = g_hasWeaponEquipped && (dual || side == (if super::weapon_arm::cvr_anim_weapon_in_left_hand() { 0 } else { 1 }));
        let free = on && !busy[sd] && !super::grasp::grasp_holding(sd) && !gun_hand && (holding & (1 << side)) == 0 && !super::rig::VRIK_UsesNativeCyberware();
        // the chains always (the hand's body poses its finger capsules by them, a weapon's grip or not); the solve only
        // for a free hand
        let n = palm_normal(buf, side, hand);
        let mut chained = n.is_some();
        let n = n.unwrap_or([0.0; 3]);
        let middle = if chained { palm_middle(buf, side, hand, &n) } else { None };
        // the joints as hinges: the chain the solve is given and the pose drawn under the bends, both from the hinge-only
        // turns, so what is solved is what is drawn
        let hinge_only_on = super::physhands::api::finger_hinge_only();
        let mut pure = [[ID; 3]; 5];
        let mut pured = [false; 5];
        for f in 0..5 {
            if !chained {
                break;
            }
            match chain(buf, side, hand, f, &n, middle.as_ref(), &mut h.f[f], hinge_only_on.then_some(&mut pure[f])) {
                Some(p) => pured[f] = p,
                None => chained = false,
            }
        }
        h.free = free && chained;
        if chained {
            super::physhands::fingers::publish_fingers(sd, &h);
        }
        let chained = h.free;
        // the turns: taken once a batch, eased in and out over a few
        if new_batch {
            let got = if chained { super::physhands::fingers::finger_bends(sd) } else { None };
            let holds = chained && super::physhands::grab::holds_on(sd);
            if !holds {
                HOLD_SINCE[sd] = 0.0;
            } else if HOLD_SINCE[sd] == 0.0 {
                HOLD_SINCE[sd] = now;
            }
            FROZEN[sd] = holds && LATCHED_ON[sd] && now - HOLD_SINCE[sd] > FREEZE_MS;
            if FROZEN[sd] {
                FRAMES_FROZEN.fetch_add(1, Relaxed);
            }
            if let Some((b, fx)) = got.filter(|_| !FROZEN[sd]) {
                LATCHED[sd] = b;
                LATCHED_FLEX[sd] = fx;
                LATCHED_FLEX_KNOWN[sd] = true;
            }
            LATCHED_ON[sd] = got.is_some() || FROZEN[sd];
            WEIGHT[sd] += ((if LATCHED_ON[sd] { 1.0 } else { 0.0 }) - WEIGHT[sd]) * 0.35;
            if WEIGHT[sd] < 0.002 {
                WEIGHT[sd] = 0.0;
            }
            SPENT_W[sd] += ((if chained && super::physhands::grab::grip_spent_shown(sd) { 1.0 } else { 0.0 }) - SPENT_W[sd]) * 0.3;
            if SPENT_W[sd] < 0.002 {
                SPENT_W[sd] = 0.0;
            }
        }
        // trace=1, every two seconds: the right hand's fingers, each joint's flexion from straight (degrees) as the
        // animation has it, as the reference hand has it, its range, the turn taken and the result
        if side == 1 && chained && new_batch && now - TOLD_AT > 2000.0 && super::physhands::api::tracing() {
            TOLD_AT = now;
            const NAMES: [&str; 5] = ["index", "middle", "ring", "little", "thumb"];
            let rr = 1.0 / DEG;
            let w = WEIGHT[1];
            let mut line = String::from("[physhands] fingers (MCP PIP DIP from straight, deg; anim / ref / range / turn = drawn):");
            for f in 0..5 {
                let c = &h.f[f];
                let a = FLEX_ANIM[1][f];
                let r = FLEX_REF[1][f];
                let b = LATCHED[1][f];
                line += &format!(
                    " {} {:.0} {:.0} {:.0} / {:.0} {:.0} {:.0} / {:.0}..{:.0} {:.0}..{:.0} {:.0}..{:.0} / {:.0} {:.0} {:.0} = {:.0} {:.0} {:.0};",
                    NAMES[f],
                    a[0] * rr,
                    a[1] * rr,
                    a[2] * rr,
                    r[0] * rr,
                    r[1] * rr,
                    r[2] * rr,
                    (a[0] + c.lo[0]) * rr,
                    (a[0] + c.hi[0]) * rr,
                    (a[1] + c.lo[1]) * rr,
                    (a[1] + c.hi[1]) * rr,
                    (a[2] + c.lo[2]) * rr,
                    (a[2] + c.hi[2]) * rr,
                    b[0] * w * rr,
                    b[1] * w * rr,
                    b[2] * w * rr,
                    (a[0] + b[0] * w) * rr,
                    (a[1] + b[1] * w) * rr,
                    (a[2] + b[2] * w) * rr
                );
            }
            line += &format!(
                " bends re-based {}, clamped {}, eased {}, frames held still {};",
                BENDS_REBASED.load(Relaxed),
                BENDS_CLAMPED.load(Relaxed),
                TURNS_EASED.load(Relaxed),
                FRAMES_FROZEN.load(Relaxed)
            );
            // the animation's turn off each hinge (the hinge-only pose drops it), degrees
            line += &format!(" off hinge{}", if hinge_only_on { " (dropped)" } else { "" });
            for f in 0..5 {
                line += &format!(" {:.0}/{:.0}/{:.0}", OFF_HINGE[1][f][0] / DEG, OFF_HINGE[1][f][1] / DEG, OFF_HINGE[1][f][2] / DEG);
            }
            // each joint's hinge, the bone's own axis: y / z, upper case for +
            line += " hinges";
            for f in 0..5 {
                let mut code = String::new();
                for j in 0..3 {
                    let x = h.f[f].hinge[j];
                    let ax = if x[2].abs() >= x[1].abs() { 2 } else { 1 };
                    let ch = if ax == 2 { 'z' } else { 'y' };
                    code.push(if x[ax] > 0.0 { ch.to_ascii_uppercase() } else { ch });
                }
                line += &format!(" {}", code);
            }
            line.truncate(1499);
            log!("{}\n", line);
        }
        if (WEIGHT[sd] <= 0.0 && SPENT_W[sd] <= 0.0) || !chained {
            DRAWN_TURN[sd] = [[0.0; 3]; 5];
            continue;
        }
        let dt_b = ((now - LAST_BATCH_MS) / 1000.0).clamp(0.001, 0.05) as f32;
        for f in 0..5 {
            for j in 0..3 {
                let mut a = LATCHED[sd][f][j] * WEIGHT[sd];
                // A BEND THAT HELD A FINGER OFF SOMETHING, ON AN ANIMATION THAT HAS OPENED SINCE: what it kept was the
                // finger's angle, so it is the less by how far the animation opened, and never past straight the other
                // way (2026-11-10).
                if a < 0.0 && LATCHED_FLEX_KNOWN[sd] {
                    let opened = LATCHED_FLEX[sd][f][j] - h.f[f].flex[j];
                    if opened > 0.0 {
                        a = min(0.0, a + opened);
                        BENDS_REBASED.fetch_add(1, Relaxed);
                    }
                }
                // A HOLD PULLED OFF BY ITSELF (a person's part the controller was drawn away from): the grip counts as
                // opened, and the fingers open as if it were, however the grip is still held
                if SPENT_W[sd] > 0.0 && f < 4 {
                    a += (-0.85 * h.f[f].flex[j] - a) * SPENT_W[sd];
                }
                {
                    let shown = &mut DRAWN_TURN[sd][f][j];
                    if new_batch {
                        let diff = a - *shown;
                        let most = TURN_RATE * dt_b;
                        let mut step = diff * (1.0 - exp(-dt_b / TURN_TAU));
                        if step > most || step < -most {
                            step = clamp(step, -most, most);
                            TURNS_EASED.fetch_add(1, Relaxed);
                        }
                        *shown += step;
                    }
                    a = *shown;
                }
                if a < h.f[f].lo[j] || a > h.f[f].hi[j] {
                    a = clamp(a, h.f[f].lo[j], h.f[f].hi[j]);
                    BENDS_CLAMPED.fetch_add(1, Relaxed);
                }
                // and a knuckle or a middle joint never more than 20 degrees back past straight (a fingertip pressed on
                // its pad gives that much) (2026-11-10)
                if f < 4 && j < 2 && h.f[f].flex[j] + a < -BACK_MOST_DEG * DEG {
                    a = -BACK_MOST_DEG * DEG - h.f[f].flex[j];
                    BENDS_CLAMPED.fetch_add(1, Relaxed);
                }
                // a diagnostic, always on: a finger's knuckle or middle joint drawn more than 22 degrees back past
                // straight, rate-limited
                if f < 4 && j < 2 && h.f[f].flex[j] + a < -22.0 * DEG && h.at - PAST_TOLD_AT[sd] > 250 {
                    PAST_TOLD_AT[sd] = h.at;
                    log!(
                        "[physhands] {} finger {} joint {} drawn {:.0} deg past straight (animation {:.0}, turn {:.0}, latched {:.0} at {:.0})\n",
                        if side != 0 { "right" } else { "left" },
                        f,
                        j,
                        -(h.f[f].flex[j] + a) / DEG,
                        h.f[f].flex[j] / DEG,
                        a / DEG,
                        LATCHED[sd][f][j] / DEG,
                        LATCHED_FLEX[sd][f][j] / DEG
                    );
                }
                if a == 0.0 && !pured[f] {
                    continue;
                }
                let bone = VRIK_LadderFingerBone(side, CHAINS[f][j]);
                if !valid(bone) {
                    continue;
                }
                let axis = h.f[f].hinge[j];
                let s = sin(0.5 * a);
                let turn = [axis[0] * s, axis[1] * s, axis[2] * s, cos(0.5 * a)];
                let qp = br(buf, bone);
                let mut from = a4(qp);
                if pured[f] {
                    // eased onto the hinge-only pose as the bends are (a hand that leaves a wall gets its lean back)
                    let p = pure[f][j];
                    let d = from[0] * p[0] + from[1] * p[1] + from[2] * p[2] + from[3] * p[3];
                    let w = WEIGHT[sd];
                    let sgn = if d < 0.0 { -1.0 } else { 1.0 };
                    for i in 0..4 {
                        from[i] = from[i] * (1.0 - w) + p[i] * sgn * w;
                    }
                    qnorm(&mut from);
                }
                let mut r = qmul(&from, &turn);
                qnorm(&mut r);
                put4(qp, &r);
            }
        }
    }
    if new_batch {
        LAST_BATCH_MS = now;
    }
}
