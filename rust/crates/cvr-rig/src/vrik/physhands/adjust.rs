//! The drawn hand (animation thread, VRIK_ApplyHandStop): moved to where its body is, the hold on the world or a
//! person's part drawn where it was taken, the held body's weight turning the wrist, and the drawn hand kept out of what
//! is near.
use super::fingers::damped;
use super::grab::{read_person_attachment, HOLD_LOADS, HOLD_POSES};
use super::math::{qconj, qmul, qnorm, qrot};
use super::near::{near_lists, NEAR_FRONT, NEAR_LOCK};
use super::people::{PeopleNear, MOST_BODIES, PEOPLE_NEAR, SELF_OWNER};
use super::pose::{pending_frame, pose_entity_now, set_applied_grip, DRAWN_HANDS};
use super::loose::penetration;
use super::stats::{note_drawn, note_slip, now_s};
use super::tick_ms;
use super::touch::{BodyShape, BodyShapes, BODY_SHAPES, BODY_SHAPES_SEQ};
use super::trace::{trace_frame, DRAWN_IN};
use super::types::{DrawnHand, Offset, PoseFrame};
use super::worker::OFFSETS;
use super::px::PxTransform;
use super::body::SELF_ON_SHOWN;
use super::near::NearList;
use core::sync::atomic::{AtomicU64, Ordering::*};
use cvr_host::log;
use cvr_physhands::handspace::{self, Motion, Rotation, Vector};
use cvr_physhands::held_grip;
use cvr_settings::lc;
use std::cell::RefCell;
use std::sync::Mutex;

extern "C" {
    static g_hasWeaponEquipped: bool;
    static g_isInVehicle: bool;
}
fn seated() -> bool {
    // SAFETY: the C++ flag, read.
    unsafe { core::ptr::read_volatile(&raw const g_isInVehicle) }
}
/// RideMotion: how what one frame (or step) keeps rides on to the next -- by tracking's frame where either has one;
/// SEATED, with none, by the seat's (the entity's): the car carries the hands, their cabin and what is drawn of them.
/// Kept in the world, a moving car's drawn hand was eased from where the car had been a frame before -- 6 cm off as a
/// palm left the dashboard at 15 km/h (2026-11-26). On foot without a tracking frame nothing rides, as before.
pub fn ride_motion(before: &PoseFrame, after: &PoseFrame, most: f32) -> Motion {
    if before.tracking.valid || after.tracking.valid {
        return handspace::between(&before.tracking, &after.tracking, most);
    }
    if seated() {
        return handspace::between(&before.entity, &after.entity, most);
    }
    Motion::default()
}
fn weapon_equipped() -> bool {
    // SAFETY: the C++ flag, read.
    unsafe { core::ptr::read_volatile(&raw const g_hasWeaponEquipped) }
}
fn v3(p: &[f32; 3]) -> Vector {
    Vector::new(p[0], p[1], p[2])
}
fn r4(q: &[f32; 4]) -> Rotation {
    Rotation::new(q[0], q[1], q[2], q[3])
}

/// THE DRAWN HAND STAYS OUT OF THE WORLD: how often the drawn place was cut back, and by how much (um).
pub static DRAWN_CUT: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
pub static DRAWN_CUT_UM: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// ShapeDepth: one of the body's shapes where the hand is drawn (at, its world turn), how deep in what is near.
unsafe fn shape_depth(b: &BodyShape, at: &[f32; 3], world_rot: &[f32; 4], origin: &[f32; 3], l: Option<&NearList>, p: Option<&PeopleNear>, side: usize) -> f32 {
    let r = qrot(world_rot, &b.local.p);
    let mut pose = PxTransform { q: qmul(world_rot, &b.local.q), p: [at[0] + r[0] - origin[0], at[1] + r[1] - origin[1], at[2] + r[2] - origin[2]] };
    qnorm(&mut pose.q);
    let mut deepest = 0.0f32;
    let mut test = |e: &super::near::NearShape| {
        if (0..3).any(|k| pose.p[k] + b.reach < e.bounds.lo[k] || pose.p[k] - b.reach > e.bounds.hi[k]) {
            return;
        }
        #[repr(C, align(8))]
        struct G([u8; 16]);
        let g = G(b.geometry);
        if let Some((_, depth)) = penetration(g.0.as_ptr().cast(), &pose, e) {
            if depth.is_finite() {
                deepest = deepest.max(depth);
            }
        }
    };
    if let Some(l) = l {
        for e in &l.s[..l.n.min(super::near::MOST_NEAR)] {
            test(e);
        }
    }
    // V's own body: the palm's and the fingers' alone, of the parts this hand meets while it does (the forearm lies
    // along the trunk, and a forearm "in" it held the drawn hand back)
    let own = b.part != 2 && SELF_ON_SHOWN[side].load(Relaxed);
    if let Some(p) = p {
        for i in 0..p.n.min(MOST_BODIES) {
            if (p.meets[i] >> side) & 1 != 0 && (p.owner[i] != SELF_OWNER || own) {
                test(&p.s[i]);
            }
        }
    }
    deepest
}
fn qslerp(a: &[f32; 4], b0: &[f32; 4], t: f32) -> [f32; 4] {
    let mut b = *b0;
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if d < 0.0 {
        for c in &mut b {
            *c = -*c;
        }
        d = -d;
    }
    let (mut wa, mut wb) = (1.0 - t, t);
    if d < 0.9995 {
        let th = d.min(1.0).acos();
        let sn = th.sin();
        wa = ((1.0 - t) * th).sin() / sn;
        wb = (t * th).sin() / sn;
    }
    let mut o = [0.0f32; 4];
    for k in 0..4 {
        o[k] = a[k] * wa + b[k] * wb;
    }
    qnorm(&mut o);
    o
}
/// RebasePeopleForDraw: these shapes and the hand's physics result need not have been published in the same step: both
/// put in the hand result's PhysX origin first, and V's own body carried by the tracking frame's motion since.
fn rebase_people_for_draw(people: &mut PeopleNear, origin: &[f32; 3]) {
    for k in 0..3 {
        let shift = people.origin[k] - origin[k];
        if shift == 0.0 {
            continue;
        }
        for i in 0..people.n.min(MOST_BODIES) {
            people.s[i].pose.p[k] += shift;
            people.s[i].bounds.lo[k] += shift;
            people.s[i].bounds.hi[k] += shift;
        }
    }
    let Some(frame) = pending_frame() else { return };
    let current = if frame.tracking.valid { frame.tracking } else { frame.entity };
    let motion = handspace::between(&people.self_frame, &current, 1.0);
    let space_origin = v3(origin);
    for i in 0..people.n.min(MOST_BODIES) {
        if people.owner[i] != SELF_OWNER {
            continue;
        }
        if !motion.valid {
            people.meets[i] = 0;
            continue;
        }
        let shape = &mut people.s[i];
        let p = motion.point(v3(&shape.pose.p) + space_origin) - space_origin;
        let q = motion.orientation(r4(&shape.pose.q));
        shape.pose.p = p.to_array();
        shape.pose.q = q.to_array();
        let old = shape.bounds;
        shape.bounds.lo = [f32::MAX; 3];
        shape.bounds.hi = [-f32::MAX; 3];
        // A transformed old AABB is conservative. Narrow phase still uses the actual PhysX capsule at its transformed
        // pose, never an invented box.
        for corner in 0..8 {
            let c = motion.point(
                Vector::new(
                    if corner & 1 != 0 { old.hi[0] } else { old.lo[0] },
                    if corner & 2 != 0 { old.hi[1] } else { old.lo[1] },
                    if corner & 4 != 0 { old.hi[2] } else { old.lo[2] },
                ) + space_origin,
            ) - space_origin;
            let v = c.to_array();
            for k in 0..3 {
                shape.bounds.lo[k] = shape.bounds.lo[k].min(v[k]);
                shape.bounds.hi[k] = shape.bounds.hi[k].max(v[k]);
            }
        }
    }
}

thread_local! {
    static KEEP_BS: RefCell<Box<BodyShapes>> = RefCell::new(Box::new(super::touch::BODY_SHAPES0));
    static KEEP_PN: RefCell<Box<PeopleNear>> = RefCell::new(Box::new(unsafe { core::mem::zeroed() }));
}
static KEEP_TOLD_AT: Mutex<[u64; 2]> = Mutex::new([0; 2]);
/// KeepDrawnOut: the body's shapes as last posed (the palm's box, the finger capsules, the forearm's, grown by the
/// skin's margin) are tried where the hand is drawn against what is near the hands -- the world's shapes and the
/// people's -- and the way from the body to it (its place and its turn, together) is cut back, halved a few times, until
/// no shape is deeper in than it is at the body itself (and a millimetre's grace). Animation thread; the lists are the
/// worker's, read under its lock.
unsafe fn keep_drawn_out(side: usize, o: &Offset, at: &mut [f32; 3], model_rot: Option<&mut [f32; 4]>) {
    let (_entity_position, entity_rotation) = pose_entity_now();
    DRAWN_IN[side].store(0, Relaxed);
    let cut = !weapon_equipped();
    let Some(model_rot) = model_rot else { return };
    if !cut && o.trace == 0 {
        return;
    }
    let Some(bs) = BODY_SHAPES_SEQ[side].read().filter(|bs| bs.n > 0 && tick_ms().wrapping_sub(bs.at) <= 200) else { return };
    KEEP_BS.with(|b| **b.borrow_mut() = bs);
    let mut wq = qmul(&entity_rotation, model_rot);
    let mut bq = o.raw_rot;
    if !qnorm(&mut wq) || !qnorm(&mut bq) {
        return;
    }
    // from the body as PhysX has it
    let d = [at[0] - o.raw[0], at[1] - o.raw[1], at[2] - o.raw[2]];
    let dn = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let turn_dot = (wq[0] * bq[0] + wq[1] * bq[1] + wq[2] * bq[2] + wq[3] * bq[3]).abs();
    let moved = dn >= 0.001 * 0.001 || turn_dot < 0.99998; // a millimetre, or about a degree
    let people = PEOPLE_NEAR.read().filter(|p| p.n > 0 && tick_ms().wrapping_sub(p.at) < 300);
    let mut pn_box = KEEP_PN.with(|p| core::mem::replace(&mut *p.borrow_mut(), Box::new(core::mem::zeroed())));
    let p: Option<&PeopleNear> = match people {
        Some(people) => {
            *pn_box = people;
            rebase_people_for_draw(&mut pn_box, &o.origin);
            Some(&*pn_box)
        }
        None => None,
    };
    let _shared = NEAR_LOCK.read().unwrap_or_else(|e| e.into_inner());
    let front = NEAR_FRONT.load(Acquire);
    let l: Option<&NearList> = (front >= 0).then(|| &near_lists[front as usize]);
    let grace = o.margin + 0.001;
    // the shapes deeper where drawn than at the body, and how deep each may be
    let mut bad: Vec<(usize, f32)> = Vec::new();
    if cut && moved {
        for i in 0..bs.n.min(BODY_SHAPES) {
            let there = shape_depth(&bs.s[i], at, &wq, &o.origin, l, p, side);
            if !(there > grace) {
                continue;
            }
            let at_body = shape_depth(&bs.s[i], &o.raw, &bq, &o.origin, l, p, side);
            let most = grace.max(at_body + 0.0005);
            if there > most {
                bad.push((i, most));
            }
        }
    }
    let mut cut_to = 1.0f32;
    if !bad.is_empty() {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..6 {
            let mid = 0.5 * (lo + hi);
            let pp = [o.raw[0] + d[0] * mid, o.raw[1] + d[1] * mid, o.raw[2] + d[2] * mid];
            let q = qslerp(&bq, &wq, mid);
            let inside = bad.iter().any(|&(j, allowed)| shape_depth(&bs.s[j], &pp, &q, &o.origin, l, p, side) > allowed);
            if inside {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        for k in 0..3 {
            at[k] = o.raw[k] + d[k] * lo;
        }
        wq = qslerp(&bq, &wq, lo);
        {
            let mut m = qmul(&qconj(&entity_rotation), &wq);
            if qnorm(&mut m) {
                *model_rot = m;
            }
        }
        cut_to = lo;
        DRAWN_CUT[side].fetch_add(1, Relaxed);
        DRAWN_CUT_UM[side].fetch_add((1e6 * dn.sqrt() * (1.0 - lo)) as u64, Relaxed);
    }
    if o.trace != 0 {
        let (mut worst, mut which) = (0.0f32, None);
        for i in 0..bs.n.min(BODY_SHAPES) {
            let e = shape_depth(&bs.s[i], at, &wq, &o.origin, l, p, side);
            if e > worst {
                worst = e;
                which = Some(i);
            }
        }
        DRAWN_IN[side].store((worst - o.margin).max(0.0).to_bits(), Relaxed);
        // a drawn shape over a centimetre in: which, and how the cut saw it (rate-limited)
        let mut told = KEEP_TOLD_AT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(which) = which.filter(|_| worst - o.margin > 0.01 && tick_ms().wrapping_sub(told[side]) > 200) {
            told[side] = tick_ms();
            let pp = [o.raw[0] + d[0], o.raw[1] + d[1], o.raw[2] + d[2]];
            let before = shape_depth(&bs.s[which], &pp, &wq, &o.origin, l, p, side);
            let at_body = shape_depth(&bs.s[which], &o.raw, &bq, &o.origin, l, p, side);
            log!(
                "[physhands] {} hand drawn {:.1} mm in by its shape {} (type {}, of {}): {:.1} mm before the cut, {:.1} at the body; {} shapes cut for, to {:.2} of {:.1} mm; pressed {}\n",
                if side == 1 { "right" } else { "left" },
                1000.0 * (worst - o.margin),
                which,
                bs.s[which].kind(),
                bs.n,
                1000.0 * before,
                1000.0 * at_body,
                bad.len(),
                cut_to,
                1000.0 * dn.sqrt(),
                i32::from(o.pressed)
            );
        }
    }
    drop(_shared);
    KEEP_PN.with(|p| *p.borrow_mut() = pn_box);
}

/// The animation thread's per-hand state of the drawn hand (C++ function statics; per hand, not per thread: the
/// animation jobs that call this run on more than one thread).
struct Drawn {
    hold_w: [f32; 2],
    hold_p: [[f32; 3]; 2],
    hold_q: [[f32; 4]; 2],
    hold_at: [f64; 2],
    load_w: [f32; 2],
    load_at: [f64; 2],
    load_last: [super::types::HoldLoad; 2],
    load_epochs: [u64; 2],
    move_w: [f32; 2],
    move_at: [f64; 2],
    rel_at: [[f32; 3]; 2],
    rel_v: [[f32; 3]; 2],
    rel_goal: [[f32; 3]; 2],
    rel_e: [[f32; 3]; 2],
    rel_ev: [[f32; 3]; 2],
    rel_track: [[f32; 3]; 2],
    rel_t: [f64; 2],
    rel_from: [f64; 2],
    rel_have: [bool; 2],
    rel_pressed: [bool; 2],
    rel_touch: [bool; 2],
    rel_start: [bool; 2],
    rel_frame: [PoseFrame; 2],
    rel_riding: [bool; 2],
    out_n: [[f32; 3]; 2],
    out_at: [f64; 2],
    off_v: [[f32; 3]; 2],
    off_x: [[f32; 3]; 2],
    off_at: [f64; 2],
}
static DRAWN: Mutex<Drawn> = Mutex::new(Drawn {
    hold_w: [0.0; 2],
    hold_p: [[0.0; 3]; 2],
    hold_q: [[0.0, 0.0, 0.0, 1.0]; 2],
    hold_at: [0.0; 2],
    load_w: [0.0; 2],
    load_at: [0.0; 2],
    load_last: [super::types::HoldLoad { wrist: [0.0; 4], seat: [0.0; 3], on: false, at: 0, proxy: 0, body: 0, own_mass: 0.0, contact_error_um: cvr_physhands::held_grip_contact::INVALID }; 2],
    load_epochs: [0; 2],
    move_w: [1.0; 2],
    move_at: [0.0; 2],
    rel_at: [[0.0; 3]; 2],
    rel_v: [[0.0; 3]; 2],
    rel_goal: [[0.0; 3]; 2],
    rel_e: [[0.0; 3]; 2],
    rel_ev: [[0.0; 3]; 2],
    rel_track: [[0.0; 3]; 2],
    rel_t: [0.0; 2],
    rel_from: [-1.0; 2],
    rel_have: [false; 2],
    rel_pressed: [false; 2],
    rel_touch: [false; 2],
    rel_start: [false; 2],
    rel_frame: [PoseFrame {
        entity: handspace::Reference { position: Vector { x: 0.0, y: 0.0, z: 0.0 }, rotation: Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, owner: 0, origin: 0, valid: false },
        tracking: handspace::Reference { position: Vector { x: 0.0, y: 0.0, z: 0.0 }, rotation: Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, owner: 0, origin: 0, valid: false },
    }; 2],
    rel_riding: [false; 2],
    out_n: [[0.0; 3]; 2],
    out_at: [-1.0; 2],
    off_v: [[0.0; 3]; 2],
    off_x: [[0.0; 3]; 2],
    off_at: [0.0; 2],
});

/// THE HAND THAT HOLDS ON (climb) IS DRAWN WHERE AND AS IT TOOK HOLD, as the grenade's pin holds the hand: the hold's
/// place and turn in the game's world, brought into the model's space each frame, whatever the controller does. Eased
/// in and out over 30 ms.
unsafe fn adjust_hold(d: &mut Drawn, side: usize, model_pos: &mut [f32; 3], mut model_rot: Option<&mut [f32; 4]>) -> bool {
    let (entity_position, entity_rotation) = pose_entity_now();
    const HOLD_TAU: f32 = 0.03;
    if !adjust_body(d, side, model_pos, model_rot.as_deref_mut()) {
        d.hold_w[side] = 0.0;
        return false;
    }
    let read_hold = HOLD_POSES[side].read();
    let mut hp = read_hold.unwrap_or_default();
    let attached = read_person_attachment(side).filter(|a| a.on);
    if let Some(a) = &attached {
        hp.p = a.position;
        hp.q = a.rotation;
        hp.on = true;
        hp.at = a.at;
    }
    let on = (read_hold.is_some() || hp.on) && hp.on && tick_ms().wrapping_sub(hp.at) < 200 && (hp.p[0] + hp.p[1] + hp.p[2] + hp.q[0] + hp.q[1] + hp.q[2] + hp.q[3]).is_finite();
    let now = now_s();
    let dt = (now - d.hold_at[side]).clamp(0.0, 0.1);
    if now - d.hold_at[side] > 0.004 {
        d.hold_at[side] = now;
    }
    if on {
        // taken now: at once at the hold's weight so far (none), eased in from where the body had it
        d.hold_p[side] = hp.p;
        d.hold_q[side] = hp.q;
    }
    d.hold_w[side] += ((if on { 1.0 } else { 0.0 }) - d.hold_w[side]) * (1.0 - (-dt / f64::from(HOLD_TAU)).exp()) as f32;
    if d.hold_w[side] < 0.001 {
        return true;
    }
    let inv = qconj(&entity_rotation);
    let dd = [d.hold_p[side][0] - entity_position[0], d.hold_p[side][1] - entity_position[1], d.hold_p[side][2] - entity_position[2]];
    let m = qrot(&inv, &dd);
    let mut q = qmul(&inv, &d.hold_q[side]);
    if !qnorm(&mut q) || !(m[0] + m[1] + m[2]).is_finite() {
        return true;
    }
    let w = if d.hold_w[side] > 0.999 { 1.0 } else { d.hold_w[side] };
    if let Some(a) = &attached {
        set_applied_grip(side, a, w);
    }
    for k in 0..3 {
        model_pos[k] += (m[k] - model_pos[k]) * w;
    }
    if let Some(model_rot) = model_rot {
        if q[0] * model_rot[0] + q[1] * model_rot[1] + q[2] * model_rot[2] + q[3] * model_rot[3] < 0.0 {
            for c in &mut q {
                *c = -*c;
            }
        }
        let mut r = [0.0f32; 4];
        for k in 0..4 {
            r[k] = model_rot[k] + (q[k] - model_rot[k]) * w;
        }
        if qnorm(&mut r) {
            *model_rot = r;
        }
    }
    true
}

/// The completed physics pose supplies the held wrist's orientation. The controller still drives the grip point and
/// the weighted body; publishing the pre-load wrist avoids feeding the correction back into that drive.
const LOAD_TAU: f32 = 0.08;
unsafe fn bear_load(d: &mut Drawn, side: usize, model_pos: &mut [f32; 3], model_rot: Option<&mut [f32; 4]>) {
    let (entity_position, entity_rotation) = pose_entity_now();
    let epoch = cvr_input::scanner::SCANNER_HAND_PHYSICS.epoch();
    if d.load_epochs[side] != epoch {
        d.load_epochs[side] = epoch;
        d.load_w[side] = 0.0;
        d.load_last[side] = Default::default();
        d.load_at[side] = now_s();
    }
    // Fallback for a caller outside the primary pose scope. Its normal publication is the completed FK wrist in
    // publish_pose_frame.
    if pending_frame().is_none() {
        let r = qrot(&entity_rotation, model_pos);
        let mut dh = DrawnHand { p: [entity_position[0] + r[0], entity_position[1] + r[1], entity_position[2] + r[2]], q: entity_rotation, at: tick_ms() };
        if let Some(m) = model_rot.as_deref() {
            dh.q = qmul(&entity_rotation, m);
        }
        if qnorm(&mut dh.q) && (dh.p[0] + dh.p[1] + dh.p[2]).is_finite() {
            DRAWN_HANDS[side].write(&dh);
        }
    }
    let Some(model_rot) = model_rot else { return };
    let fresh = lc!(xrHandPhysics) != 0 && HOLD_LOADS[side].read().filter(|l| l.on && tick_ms().wrapping_sub(l.at) < 200).inspect(|l| d.load_last[side] = *l).is_some();
    let now = now_s();
    let dt = (now - d.load_at[side]).clamp(0.0, 0.1);
    if now - d.load_at[side] > 0.004 {
        d.load_at[side] = now;
    }
    d.load_w[side] += ((if fresh { 1.0 } else { 0.0 }) - d.load_w[side]) * (1.0 - (-dt / f64::from(LOAD_TAU)).exp()) as f32;
    if d.load_w[side] < 0.001 {
        return;
    }
    if fresh && d.load_w[side] > 0.99 {
        d.load_w[side] = 1.0;
    }
    let u = d.load_last[side];
    let r = qrot(&entity_rotation, model_pos);
    let mut wp = [entity_position[0] + r[0], entity_position[1] + r[1], entity_position[2] + r[2]];
    let mut wq = qmul(&entity_rotation, model_rot);
    if !qnorm(&mut wq) || !held_grip::seat_rotation(&mut wp, &mut wq, &u.wrist, &u.seat, d.load_w[side]) {
        return;
    }
    let inv = qconj(&entity_rotation);
    let m = qrot(&inv, &[wp[0] - entity_position[0], wp[1] - entity_position[1], wp[2] - entity_position[2]]);
    if !(m[0] + m[1] + m[2]).is_finite() {
        return;
    }
    *model_pos = m;
    let mut mq = qmul(&inv, &wq);
    if qnorm(&mut mq) {
        *model_rot = mq;
    }
}

/// HoldsBody: a fresh weighted body grip owns the wrist pose; arm IK must not shorten it after load compensation.
pub fn holds_body(side: usize) -> bool {
    // SAFETY: the live controls block, read.
    if side > 1 || lc!(xrHandPhysics) == 0 || lc!(xrHandCollision) == 0 {
        return false;
    }
    HOLD_LOADS[side].read().is_some_and(|l| l.on && tick_ms().wrapping_sub(l.at) < 200)
}
/// HeldBodyState: the diagnostic snapshot of the completed physics hold, not the script's request. IDs are the
/// engine's integer proxy / body identifiers, never actor pointers.
pub fn held_body_state(side: usize) -> [u64; 5] {
    let mut fields = [0u64; 5];
    fields[4] = cvr_physhands::held_grip_contact::INVALID;
    if side > 1 {
        return fields;
    }
    let Some(load) = HOLD_LOADS[side].read().filter(|l| l.at != 0) else { return fields };
    let age = tick_ms().wrapping_sub(load.at);
    // SAFETY: the live controls block, read.
    fields[0] = u64::from(lc!(xrHandPhysics) != 0 && lc!(xrHandCollision) != 0 && load.on && age < 200);
    fields[1] = u64::from(load.proxy);
    fields[2] = u64::from(load.body);
    fields[3] = age;
    if fields[0] != 0 {
        fields[4] = load.contact_error_um;
    }
    fields
}
/// OriginalHeldMass: the mass of a currently held actor before its solver-only override.
pub fn original_held_mass(proxy: u32, body: u32) -> Option<f32> {
    let now = tick_ms();
    (0..2).find_map(|side| HOLD_LOADS[side].read().and_then(|l| cvr_physhands::held_body_mass::original(proxy, body, l.proxy, l.body, l.own_mass, l.on, l.at, now)))
}

/// Adjust (VRIK_ApplyHandStop): the drawn hand moved to where its body is (true: it did, and the script's stops are not
/// applied on top), and the held body's weight on the wrist.
pub fn adjust(side: usize, model_pos: &mut [f32; 3], mut model_rot: Option<&mut [f32; 4]>) -> bool {
    if side > 1 {
        return false;
    }
    let mut d = DRAWN.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the animation thread's call; the lists read under the worker's lock.
    unsafe {
        let drawn = adjust_hold(&mut d, side, model_pos, model_rot.as_deref_mut());
        bear_load(&mut d, side, model_pos, model_rot);
        drawn
    }
}

unsafe fn adjust_body(d: &mut Drawn, side: usize, model_pos: &mut [f32; 3], model_rot: Option<&mut [f32; 4]>) -> bool {
    let (entity_position, entity_rotation) = pose_entity_now();
    if !super::on(side) {
        return false;
    }
    let Some(mut o) = OFFSETS[side].read() else { return false };
    if !o.on || tick_ms().wrapping_sub(o.at) > 200 || !super::scanner_accepts(o.scanner_epoch) {
        return false;
    }
    let observed = o; // trace the actual physics publication, not its transported view
    let mut transported = false;
    let pending = pending_frame();
    if let Some(pf) = &pending {
        if o.frame.entity.valid && (o.frame.entity.owner != pf.entity.owner || o.frame.entity.origin != pf.entity.origin) {
            return false;
        }
    }
    if let (true, Some(pf)) = (o.riding, &pending) {
        let motion = ride_motion(&o.frame, pf, 4.0);
        if motion.valid {
            // the body on something rides as the step rode it: along what refused it and away, not into it (the way in
            // is where it was sent less where it ended, as below)
            let mut into = [0.0f32; 3];
            let mut in_n = 0.0f32;
            let (body0, raw0) = (o.body, o.raw);
            // (seated, what it is on -- the cabin -- rides with it: none of the ride is taken out)
            if (o.touching != 0 || o.on_world != 0) && !seated() {
                for k in 0..3 {
                    into[k] = o.sent[k] - o.raw[k];
                    in_n += into[k] * into[k];
                }
                in_n = in_n.sqrt();
            }
            for p in [&mut o.body, &mut o.raw, &mut o.sent, &mut o.sent_track] {
                *p = motion.point(v3(p)).to_array();
            }
            if in_n > 0.001 {
                for c in &mut into {
                    *c /= in_n;
                }
                for (p, p0) in [(&mut o.body, &body0), (&mut o.raw, &raw0)] {
                    let dd: f32 = (0..3).map(|k| (p[k] - p0[k]) * into[k]).sum();
                    if dd > 0.0 {
                        for k in 0..3 {
                            p[k] -= into[k] * dd;
                        }
                    }
                }
            }
            for q in [&mut o.body_rot, &mut o.raw_rot, &mut o.sent_rot] {
                *q = motion.orientation(r4(q)).to_array();
            }
            o.vel = motion.direction(v3(&o.vel)).to_array();
            o.spin = motion.direction(v3(&o.spin)).to_array();
            o.sent_entity = entity_position;
            o.frame = *pf;
            transported = true;
        }
    }
    let inv = qconj(&entity_rotation);
    if o.body_mode {
        // THE BODY, MOVED ON BY WHAT TRACKING HAS DONE SINCE THE STEP THAT SENT IT. A free body ends its step where it was
        // sent, so this is tracking itself, frame for frame; a body something held back is that much short of it.
        // Touching, the part of tracking's move into what refused the body is left out; pressed on the world, the body
        // is held where it is (the latch) and none of it is added.
        let tr = qrot(&entity_rotation, model_pos);
        let track = [tr[0] + entity_position[0], tr[1] + entity_position[1], tr[2] + entity_position[2]];
        let mut mv = [0.0f32; 3];
        // THE PLAYER'S OWN MOVE SINCE THE STEP rides along whole (a jump past a metre is a teleport)
        let mut pm = [0.0f32; 3];
        for k in 0..3 {
            pm[k] = if o.riding && !transported { entity_position[k] - o.sent_entity[k] } else { 0.0 };
        }
        if !(pm[0] * pm[0] + pm[1] * pm[1] + pm[2] * pm[2] < 1.0) {
            pm = [0.0; 3];
        }
        // eased out as the body is pressed on the world and in as it leaves, so the hand neither stops dead nor jumps by
        // a frame's move
        let now = now_s();
        let frame_dt = (now - d.move_at[side]).clamp(0.0, 0.1);
        if now - d.move_at[side] > 0.004 {
            d.move_at[side] = now;
        }
        let move_goal = if o.pressed { 0.0 } else { 1.0 };
        d.move_w[side] += (move_goal - d.move_w[side]) * (1.0 - (-frame_dt / 0.04).exp()) as f32;
        let move_w = d.move_w[side];
        if move_w > 0.001 {
            // tracking's move since the step, from where tracking was then
            for k in 0..3 {
                mv[k] = (track[k] - o.sent_track[k] - pm[k]) * move_w;
            }
            if o.touching != 0 || o.on_world != 0 {
                let b = [o.sent[0] - o.raw[0], o.sent[1] - o.raw[1], o.sent[2] - o.raw[2]];
                let bn = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
                if bn > 0.001 {
                    let into = (mv[0] * b[0] + mv[1] * b[1] + mv[2] * b[2]) / bn;
                    if into > 0.0 {
                        for k in 0..3 {
                            mv[k] -= b[k] / bn * into;
                        }
                    }
                }
            }
        }
        // PRESSED, TRACKING'S MOVE ALONG THE SURFACE STILL PASSES; only the part into it and out of it is held
        if o.pressed || move_w < 0.999 {
            let r = [o.raw[0] - o.sent[0], o.raw[1] - o.sent[1], o.raw[2] - o.sent[2]];
            let rn = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
            if rn > 0.003 {
                let full = [track[0] - o.sent_track[0] - pm[0], track[1] - o.sent_track[1] - pm[1], track[2] - o.sent_track[2] - pm[2]];
                let fn_: f32 = (0..3).map(|k| full[k] * r[k] / rn).sum();
                let w = 1.0 - move_w;
                for k in 0..3 {
                    mv[k] += (full[k] - r[k] / rn * fn_) * w;
                }
            }
        }
        let mut at = [o.body[0] + mv[0] + pm[0], o.body[1] + mv[1] + pm[1], o.body[2] + mv[2] + pm[2]];
        self::soften(d, side, &o, &pending, &mut at, &track, now);
        let dd = [at[0] - entity_position[0], at[1] - entity_position[1], at[2] - entity_position[2]];
        let m = qrot(&inv, &dd);
        if !(m[0] + m[1] + m[2]).is_finite() {
            return false;
        }
        *model_pos = m;
        note_drawn(side, &at);
        note_slip(side, &at, &track, o.touching == 0 && o.on_world == 0 && move_w > 0.99);
        let trace_lead = (mv[0] * mv[0] + mv[1] * mv[1] + mv[2] * mv[2]).sqrt();
        // THE TURN. An empty hand's is its body's, turned on by tracking's turn since the step the same way. A gun's
        // hand's is tracking's in the air (the aim, not a step behind it) and its body's as the gun meets something,
        // eased between: a barrel on a wall turns away as the body is turned.
        let mut track_rot = [0.0f32, 0.0, 0.0, 1.0];
        let mut model_rot = model_rot;
        if let Some(mr) = model_rot.as_deref_mut() {
            track_rot = *mr;
            let mut step = [0.0f32, 0.0, 0.0, 1.0];
            if move_w > 0.001 {
                let tr = qmul(&entity_rotation, mr);
                step = qmul(&tr, &qconj(&o.sent_rot));
                if step[3] < 0.0 {
                    for c in &mut step {
                        *c = -*c;
                    }
                }
                let unit = qnorm(&mut step);
                let sn = (step[0] * step[0] + step[1] * step[1] + step[2] * step[2]).sqrt();
                if unit && sn > 1e-7 {
                    let angle = 2.0 * sn.atan2(step[3]) * move_w;
                    let h = (0.5 * angle).sin() / sn;
                    step = [step[0] * h, step[1] * h, step[2] * h, (0.5 * angle).cos()];
                } else {
                    step = [0.0, 0.0, 0.0, 1.0];
                }
            }
            let q = qmul(&step, &o.body_rot);
            let mut r = qmul(&inv, &q);
            if !qnorm(&mut r) {
                return true;
            }
            let t = if weapon_equipped() { o.contact.clamp(0.0, 1.0) } else { 1.0 };
            let mut from = *mr;
            if from[0] * r[0] + from[1] * r[1] + from[2] * r[2] + from[3] * r[3] < 0.0 {
                for c in &mut from {
                    *c = -*c;
                }
            }
            for k in 0..4 {
                r[k] = from[k] + (r[k] - from[k]) * t;
            }
            if qnorm(&mut r) {
                *mr = r;
            }
            // THE TURN OFF TRACKING'S, EASED: in the air it is none and the hand turns as tracking does, frame for frame;
            // a body that meets a wall is turned by it at once and that turn is followed critically damped (30 ms, at
            // most 4 rad/s) instead of shown in one frame
            {
                let mut off = qmul(mr, &qconj(&track_rot));
                if off[3] < 0.0 {
                    for c in &mut off {
                        *c = -*c;
                    }
                }
                let on = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
                let mut ov = [0.0f32; 3];
                if on > 1e-7 {
                    let a = 2.0 * on.atan2(off[3]);
                    for k in 0..3 {
                        ov[k] = off[k] / on * a;
                    }
                }
                let odt = now - d.off_at[side];
                if odt > 0.004 {
                    if odt > 0.2 {
                        d.off_x[side] = ov;
                        d.off_v[side] = [0.0; 3];
                    } else {
                        for k in 0..3 {
                            let (x, v) = (&mut d.off_x[side][k], &mut d.off_v[side][k]);
                            damped(x, v, ov[k], 1.0 / 0.03, odt as f32, 4.0);
                        }
                    }
                    d.off_at[side] = now;
                }
                let x = d.off_x[side];
                let xn = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
                let mut shown_off = [0.0f32, 0.0, 0.0, 1.0];
                if xn > 1e-7 {
                    let h = (0.5 * xn).sin() / xn;
                    shown_off = [x[0] * h, x[1] * h, x[2] * h, (0.5 * xn).cos()];
                }
                let mut fin = qmul(&shown_off, &track_rot);
                if qnorm(&mut fin) {
                    *mr = fin;
                }
            }
        }
        {
            // the drawn palm kept out of what is near (above), and the place given again
            let was = at;
            keep_drawn_out(side, &o, &mut at, model_rot.as_deref_mut());
            if at != was {
                let mm = qrot(&inv, &[at[0] - entity_position[0], at[1] - entity_position[1], at[2] - entity_position[2]]);
                if (mm[0] + mm[1] + mm[2]).is_finite() {
                    *model_pos = mm;
                }
            }
        }
        if o.trace != 0 {
            trace_frame(side, &at, &observed, trace_lead, model_rot.as_deref(), move_w, if model_rot.is_some() { Some(&track_rot) } else { None });
        }
        return true;
    }
    for k in 0..3 {
        model_pos[k] += o.pos[k];
    }
    if o.held > 0.0 {
        // the body's place in model space, by the entity as it is this frame
        let m = qrot(&inv, &[o.body[0] - entity_position[0], o.body[1] - entity_position[1], o.body[2] - entity_position[2]]);
        if (m[0] + m[1] + m[2]).is_finite() {
            for k in 0..3 {
                model_pos[k] += (m[k] - model_pos[k]) * o.held;
            }
        }
    }
    if let Some(mr) = model_rot {
        let mut r = qmul(&o.rot, mr);
        if qnorm(&mut r) {
            *mr = r;
        }
    }
    true
}

/// THE CHANGE OF STATE, SOFTENED. Pressed, the hand stands still; let go it moves at tracking's pace at once. From the
/// press or the touch changing, the drawn hand is where it would be (the goal) plus an OFFSET that dies away critically
/// damped (REL_TAU): at the change the offset is what the drawn hand would jump by and its rate what its velocity would
/// jump by (at most REL_RATE_MOST), so neither jumps, and it only ever shrinks toward the goal. And NEVER DRAWN DEEPER
/// THAN TRACKING, OUT OF WHAT IT WAS PRESSED ON.
fn soften(d: &mut Drawn, side: usize, o: &Offset, pending: &Option<PoseFrame>, at: &mut [f32; 3], track: &[f32; 3], now: f64) {
    const REL_MOST: f64 = 0.4;
    const REL_TAU: f32 = 0.03;
    const REL_RATE_MOST: f32 = 1.0;
    const REL_OFF_MOST: f32 = 0.06;
    if let Some(next) = pending {
        if now - d.rel_t[side] >= 0.004 {
            if d.rel_frame[side].entity.valid && next.entity.valid && (d.rel_frame[side].entity.owner != next.entity.owner || d.rel_frame[side].entity.origin != next.entity.origin) {
                d.rel_have[side] = false;
                d.rel_from[side] = -1.0;
                d.rel_start[side] = false;
                d.out_at[side] = -1.0;
            }
            let motion = if d.rel_riding[side] && o.riding { ride_motion(&d.rel_frame[side], next, 4.0) } else { Motion::default() };
            if motion.valid && d.rel_have[side] {
                for p in [&mut d.rel_at[side], &mut d.rel_goal[side], &mut d.rel_track[side]] {
                    *p = motion.point(v3(p)).to_array();
                }
                for v in [&mut d.rel_v[side], &mut d.rel_e[side], &mut d.rel_ev[side], &mut d.out_n[side]] {
                    *v = motion.direction(v3(v)).to_array();
                }
            }
            d.rel_frame[side] = *next;
            d.rel_riding[side] = o.riding;
        }
    }
    let touch = o.touching != 0 || o.on_world != 0;
    let was_held = d.rel_pressed[side] || d.rel_touch[side];
    // letting go only: a landing stops dead on what it lands on, and a hand lagging as it lifts off is still above what
    // it left
    if d.rel_have[side] && ((d.rel_pressed[side] && !o.pressed) || (d.rel_touch[side] && !touch)) {
        d.rel_from[side] = now;
        d.rel_start[side] = true;
    }
    d.rel_pressed[side] = o.pressed;
    d.rel_touch[side] = touch;
    let dt_s = now - d.rel_t[side];
    if d.rel_have[side] && dt_s < 0.004 {
        *at = d.rel_at[side]; // a second solve of the same frame: the same place
        return;
    }
    let dt = dt_s as f32;
    let recent = d.rel_have[side] && dt_s < 0.1;
    let mut goal_v = [0.0f32; 3];
    if recent {
        for k in 0..3 {
            goal_v[k] = (at[k] - d.rel_goal[side][k]) / dt;
        }
    }
    // ...and seen here, in the frame it happens: on something last frame, and the place it would be drawn now moves off
    // tracking by more than 0.25 m/s at once
    if recent && was_held && d.rel_from[side] < 0.0 {
        let (mut g, mut v, mut off2) = (0.0f32, 0.0f32, 0.0f32);
        for k in 0..3 {
            let off = (at[k] - track[k]) - (d.rel_goal[side][k] - d.rel_track[side][k]);
            g += goal_v[k] * goal_v[k];
            v += d.rel_v[side][k] * d.rel_v[side][k];
            off2 += off * off;
        }
        if g.sqrt() > v.sqrt() + 0.25 && off2.sqrt() > 0.25 * dt {
            d.rel_from[side] = now;
            d.rel_start[side] = true;
        }
    }
    d.rel_goal[side] = *at;
    d.rel_track[side] = *track;
    if !recent {
        d.rel_from[side] = -1.0;
        d.rel_start[side] = false;
    }
    if d.rel_start[side] {
        // the offset and its rate at the change: last frame's drawn place carried on by its own velocity, and that
        // velocity, against the goal's
        d.rel_start[side] = false;
        let (mut en, mut vn) = (0.0f32, 0.0f32);
        for k in 0..3 {
            d.rel_e[side][k] = d.rel_at[side][k] + d.rel_v[side][k] * dt - at[k];
            d.rel_ev[side][k] = d.rel_v[side][k] - goal_v[k];
            en += d.rel_e[side][k] * d.rel_e[side][k];
            vn += d.rel_ev[side][k] * d.rel_ev[side][k];
        }
        en = en.sqrt();
        vn = vn.sqrt();
        if en > REL_OFF_MOST {
            for c in &mut d.rel_e[side] {
                *c *= REL_OFF_MOST / en;
            }
        }
        if vn > REL_RATE_MOST {
            for c in &mut d.rel_ev[side] {
                *c *= REL_RATE_MOST / vn;
            }
        }
        if !(en + vn).is_finite() {
            d.rel_from[side] = -1.0;
        }
    }
    if d.rel_from[side] >= 0.0 && now - d.rel_from[side] < REL_MOST {
        // the exact step of a critically damped offset: x'' = -2w x' - w^2 x
        let w = 1.0 / REL_TAU;
        let ex = (-w * dt).exp();
        let (mut e2, mut v2) = (0.0f32, 0.0f32);
        for k in 0..3 {
            let (x, v) = (d.rel_e[side][k], d.rel_ev[side][k]);
            let c = v + w * x;
            d.rel_e[side][k] = (x + c * dt) * ex;
            d.rel_ev[side][k] = (v - w * c * dt) * ex;
            e2 += d.rel_e[side][k] * d.rel_e[side][k];
            v2 += d.rel_ev[side][k] * d.rel_ev[side][k];
        }
        for k in 0..3 {
            at[k] += d.rel_e[side][k];
            d.rel_v[side][k] = goal_v[k] + d.rel_ev[side][k];
        }
        // caught up: tracking's own pace from here on
        if e2 < 0.0005 * 0.0005 && v2 < 0.03 * 0.03 {
            d.rel_from[side] = -1.0;
        }
    } else {
        d.rel_from[side] = -1.0;
        d.rel_v[side] = goal_v;
    }
    // NEVER DRAWN DEEPER THAN TRACKING, OUT OF WHAT IT WAS PRESSED ON: along the way out the press last had, a drawn hand
    // short of where a free body would be is brought out to it at once. It only ever moves the hand toward tracking.
    {
        let r = [o.raw[0] - o.sent[0], o.raw[1] - o.sent[1], o.raw[2] - o.sent[2]];
        let rn = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        if o.pressed && rn > 0.003 {
            for k in 0..3 {
                d.out_n[side][k] = r[k] / rn;
            }
            d.out_at[side] = now;
        }
        if d.out_at[side] >= 0.0 && now - d.out_at[side] < 0.4 {
            // where a free body would be: tracking, less what the arm's reach took off it at the step
            let n = d.out_n[side];
            let e: f32 = (0..3).map(|k| (track[k] - o.sent_track[k] + o.sent[k] - at[k]) * n[k]).sum();
            if e > 0.0 && e.is_finite() {
                for k in 0..3 {
                    at[k] += n[k] * e;
                }
                if d.rel_from[side] >= 0.0 {
                    // the follower's offset loses that part too, and its rate back in
                    let mut back = 0.0f32;
                    for k in 0..3 {
                        d.rel_e[side][k] += n[k] * e;
                        back += d.rel_ev[side][k] * n[k];
                    }
                    if back < 0.0 {
                        for k in 0..3 {
                            d.rel_ev[side][k] -= n[k] * back;
                        }
                    }
                }
            }
        }
    }
    d.rel_at[side] = *at;
    d.rel_t[side] = now;
    d.rel_have[side] = (at[0] + at[1] + at[2]).is_finite();
}
