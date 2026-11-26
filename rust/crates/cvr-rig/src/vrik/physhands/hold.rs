//! A BODY HELD BY THE HAND AT THE POINT IT CLOSED ON, AS A PISTOL IS (hold_body, the script's carry of a body lying
//! dead). The point held goes with the hand bone, at every step, exactly: it is made the body's centre of mass, the
//! body's mass and inertia raised so that the rest of the body pulling on it by its joints cannot drag it off the hand,
//! and each step it is given the velocity that brings it there by the step's end. Its turn is the hand's grip: steered
//! toward the turn the hand has it at by no more than the hand's torque (two hands on it: both), so a light part turns
//! with the wrist and a body hanging from a leg turns the leg further by its weight -- and the drawn hand is turned with
//! it (bear_load), so the leg stays in the hand as it was taken. Two hands on one body hold it together; two on two of its
//! parts each hold their own. Let go, the body has its own centre, mass, inertia, damping and spin ceiling back and goes
//! on as it moved. Its table and its (proxy, body) words are read before each touch: a ragdoll whose NPC streamed out is
//! freed into PhysX's pool, which keeps its slabs (HeldBody's Alive). Every call into a held actor is guarded, as main's
//! __try had them.
use super::blade::{held_alive, held_pose};
use super::grab::{held_contact_step, BODY_HOLDS, HOLD_LOADS};
use super::math::{cross, qconj, qmul, qnorm, qrot};
use super::pose::step_pose_inputs;
use super::px::{self, PxTransform, P};
use super::tick_ms;
use super::types::{BodyHold, HoldLoad};
use cvr_engine::code::{guarded_call4x, read};
use cvr_host::log;
use cvr_physhands::{held_body_mass, held_grip, held_grip_contact};
use cvr_settings::lc;

/// `object`'s function at `slot`, called (this, a, b, c) inside a guarded range: (rax, xmm0), or None on a fault.
pub(super) unsafe fn gcall(object: P, slot: usize, a: u64, b: u64, c: u64) -> Option<(u64, u64)> {
    let table = read::<usize>(object as usize)?;
    let f = read::<usize>(table + 8 * slot)?;
    guarded_call4x(f, [object as u64, a, b, c])
}
pub(super) fn fbits(x: f32) -> u64 {
    u64::from(x.to_bits())
}
pub(super) fn f_of(x: u64) -> f32 {
    f32::from_bits(x as u32)
}
fn ptr<T>(v: &T) -> u64 {
    v as *const T as u64
}
fn mptr<T>(v: &mut T) -> u64 {
    v as *mut T as u64
}
fn body_gone(proxy: u32, body: u32) -> bool {
    super::super::debug_view::body_gone(proxy, body)
}

/// BodySurface: the nearest point of a body's own shapes (a ragdoll's limb: its capsule, box or hull) to `point` (the
/// game's world; `origin` its scene's), and how far (0 inside) -- for the carry of a body lying dead: the flesh toward the
/// palm. None when it has no shape PxGeometryQuery measures, or a call into it faults.
pub unsafe fn body_surface(a: P, origin: &[f32; 3], point: &[f32; 3]) -> Option<([f32; 3], f32)> {
    let point_distance = px::point_distance()?;
    if a.is_null() {
        return None;
    }
    let mut ap = PxTransform::default();
    gcall(a, px::GET_GLOBAL_POSE, mptr(&mut ap), 0, 0)?;
    let mut list = [core::ptr::null_mut::<core::ffi::c_void>(); 16];
    let n = gcall(a, px::GET_SHAPES, list.as_mut_ptr() as u64, 16, 0)?.0 as u32;
    let pt = [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]];
    let (mut best, mut best_p) = (1e9f32, [0.0f32; 3]);
    for &sh in list.iter().take(n.min(16) as usize) {
        #[repr(C, align(16))]
        struct G([u8; 96]);
        let mut g = G([0; 96]);
        let Some((kind, _)) = gcall(sh, px::SHAPE_GET_GEOMETRY_TYPE, 0, 0, 0) else { return None };
        let slot = match kind as u32 as i32 {
            px::GEOM_BOX => px::SHAPE_GET_BOX,
            px::GEOM_SPHERE => px::SHAPE_GET_SPHERE,
            px::GEOM_CAPSULE => px::SHAPE_GET_CAPSULE,
            px::GEOM_CONVEX => px::SHAPE_GET_CONVEX,
            _ => continue, // what pointDistance does not measure
        };
        let Some((ok, _)) = gcall(sh, slot, g.0.as_mut_ptr() as u64, 0, 0) else { return None };
        if ok as u8 == 0 {
            continue;
        }
        let mut lp = PxTransform::default();
        gcall(sh, px::SHAPE_GET_LOCAL_POSE, mptr(&mut lp), 0, 0)?;
        let t = qrot(&ap.q, &lp.p);
        let mut wp = PxTransform { q: qmul(&ap.q, &lp.q), p: [ap.p[0] + t[0], ap.p[1] + t[1], ap.p[2] + t[2]] };
        if !qnorm(&mut wp.q) {
            continue;
        }
        let mut c = pt;
        let d2 = point_distance(pt.as_ptr(), g.0.as_ptr().cast(), (&wp as *const PxTransform).cast(), c.as_mut_ptr());
        if !d2.is_finite() || d2 < 0.0 {
            continue;
        }
        let m2: f32 = (0..3).map(|k| (c[k] - pt[k]) * (c[k] - pt[k])).sum();
        let d = if d2 > 0.0 { m2.sqrt() } else { 0.0 }; // (the square it returns, read from the point it gives)
        if d < best {
            best = d;
            best_p = if d2 > 0.0 { c } else { pt };
        }
    }
    if best > 1e8 {
        return None;
    }
    Some(([best_p[0] + origin[0], best_p[1] + origin[1], best_p[2] + origin[2]], best))
}

/// s: from where it lay in the hand to where the hold has it
const HOLD_EASE: f32 = 0.15;
/// m: a drawn hand this far from the last step's (a teleport) eases it in again
const HOLD_JUMP: f32 = 0.5;
/// the held body's mass and inertia while held, times its own...
const HOLD_MASS_SCALE: f32 = 20.0;
/// ...and its mass no less than this (kg)
const HOLD_LEAST_MASS: f32 = 100.0;
/// m/s: the most the point held is given to catch the hand
const HOLD_MOST_SPEED: f32 = 20.0;
/// rad/s: the most it is given to turn with the hand (its ceiling raised to it)
const HOLD_MOST_SPIN: f32 = 40.0;

#[derive(Clone, Copy)]
struct Held {
    actor: P,
    proxy: u32,
    body: u32,
    holders: i32,
    /// its own centre of mass, mass, inertia, damping and spin ceiling, put back when let go
    own_centre: PxTransform,
    own_mass: f32,
    own_damping: f32,
    own_inertia: [f32; 3],
    own_spin: f32,
    made_mass: f32,
    made_inertia: [f32; 3],
    /// its centre of mass while held (its frame): the point held, or between two
    centre: [f32; 3],
    steps: u32,
    off_sum: f32,
    off_most: f32,
    yield_most: f32,
    said_at: u64,
}
const HELD0: Held = Held {
    actor: core::ptr::null_mut(),
    proxy: 0,
    body: 0,
    holders: 0,
    own_centre: PxTransform::ZERO,
    own_mass: 0.0,
    own_damping: 0.0,
    own_inertia: [0.0; 3],
    own_spin: 0.0,
    made_mass: 0.0,
    made_inertia: [0.0; 3],
    centre: [0.0; 3],
    steps: 0,
    off_sum: 0.0,
    off_most: 0.0,
    yield_most: 0.0,
    said_at: 0,
};
static mut helds: [Held; 2] = [HELD0; 2];
#[derive(Clone, Copy)]
struct HoldRun {
    /// its Held, -1 none
    held: i32,
    actor: P,
    proxy: u32,
    body: u32,
    /// where the hold has it in the hand's frame (the script's, carried onto this body when taken)
    want_p: [f32; 3],
    want_q: [f32; 4],
    /// where it lay in the hand's frame when taken
    from_p: [f32; 3],
    from_q: [f32; 4],
    /// s since taken
    t: f32,
    /// last hand position in world space; an origin rebase is not a hand jump
    hand_p: [f32; 3],
    has_last: bool,
    /// the point held, in the body's frame
    grip: [f32; 3],
    /// the hand's grip torque (N m)
    torque: f32,
    /// where this hand has the body this step (PhysX's frame), and the body in the hand's frame
    to_on: bool,
    to: PxTransform,
    rel_p: [f32; 3],
    rel_q: [f32; 4],
    /// exact desired actual-body point for this step, stable across origin rebases
    target_grip_world: [f32; 3],
    target_grip_valid: bool,
    target_grip_scene: usize,
    target_grip_generation: u64,
}
const RUN0: HoldRun = HoldRun {
    held: -1,
    actor: core::ptr::null_mut(),
    proxy: 0,
    body: 0,
    want_p: [0.0; 3],
    want_q: [0.0; 4],
    from_p: [0.0; 3],
    from_q: [0.0; 4],
    t: 0.0,
    hand_p: [0.0; 3],
    has_last: false,
    grip: [0.0; 3],
    torque: 0.0,
    to_on: false,
    to: PxTransform::ZERO,
    rel_p: [0.0; 3],
    rel_q: [0.0; 4],
    target_grip_world: [0.0; 3],
    target_grip_valid: false,
    target_grip_scene: 0,
    target_grip_generation: 0,
};
static mut hold_run: [HoldRun; 2] = [RUN0; 2];
static mut hold_run_ready: bool = false;

/// BodySolverMass: uncached PhysX getMass, for on-demand diagnostics; negative if no live dynamic actor.
pub unsafe fn body_solver_mass(a: P, proxy: u32, body: u32) -> f32 {
    if !held_alive(a, proxy, body) || body_gone(proxy, body) {
        return -1.0;
    }
    match gcall(a, px::GET_MASS, 0, 0, 0) {
        Some((_, x)) if f_of(x).is_finite() && f_of(x) > 0.0 => f_of(x),
        _ => -1.0,
    }
}
/// HeldTake: its centre of mass moved to `centre`, its mass and inertia raised, no linear damping, its spin ceiling
/// raised.
unsafe fn held_take(a: P, centre: &[f32; 3], h: &mut Held) -> Option<()> {
    gcall(a, px::GET_CMASS_LOCAL_POSE, mptr(&mut h.own_centre), 0, 0)?;
    h.own_mass = f_of(gcall(a, px::GET_MASS, 0, 0, 0)?.1);
    h.own_damping = f_of(gcall(a, px::GET_LINEAR_DAMPING, 0, 0, 0)?.1);
    h.own_spin = f_of(gcall(a, px::GET_MAX_ANGULAR_VELOCITY, 0, 0, 0)?.1);
    gcall(a, px::GET_INERTIA, h.own_inertia.as_mut_ptr() as u64, 0, 0)?;
    if !h.own_mass.is_finite() || h.own_mass <= 0.0 || !(h.own_inertia[0] + h.own_inertia[1] + h.own_inertia[2]).is_finite() {
        return None;
    }
    let scale = HOLD_MASS_SCALE.max(HOLD_LEAST_MASS / h.own_mass);
    h.made_mass = h.own_mass * scale;
    for k in 0..3 {
        h.made_inertia[k] = h.own_inertia[k].max(1e-4) * scale;
    }
    let c = PxTransform { q: h.own_centre.q, p: *centre };
    h.centre = *centre;
    gcall(a, px::SET_CMASS_LOCAL_POSE, ptr(&c), 0, 0)?;
    gcall(a, px::SET_MASS, fbits(h.made_mass), 0, 0)?;
    gcall(a, px::SET_INERTIA, h.made_inertia.as_ptr() as u64, 0, 0)?;
    gcall(a, px::SET_LINEAR_DAMPING, fbits(0.0), 0, 0)?;
    gcall(a, px::SET_MAX_ANGULAR_VELOCITY, fbits(h.own_spin.max(HOLD_MOST_SPIN)), 0, 0)?;
    Some(())
}
/// HeldCentre: its centre moved while held (a second hand on it, or one let go).
unsafe fn held_centre(h: &mut Held, centre: &[f32; 3]) -> Option<()> {
    let c = PxTransform { q: h.own_centre.q, p: *centre };
    gcall(h.actor, px::SET_CMASS_LOCAL_POSE, ptr(&c), 0, 0)?;
    h.centre = *centre;
    Some(())
}
/// HeldGive: let go -- its own centre, mass, inertia, damping and spin ceiling, its velocity carried from its centre
/// while held to its own.
unsafe fn held_give(h: &Held) -> Option<()> {
    let a = h.actor;
    let mut pose = PxTransform::default();
    let mut v = [0.0f32; 3];
    let mut w = [0.0f32; 3];
    gcall(a, px::GET_GLOBAL_POSE, mptr(&mut pose), 0, 0)?;
    gcall(a, px::GET_LINEAR_VELOCITY, v.as_mut_ptr() as u64, 0, 0)?;
    gcall(a, px::GET_ANGULAR_VELOCITY, w.as_mut_ptr() as u64, 0, 0)?;
    let d = [h.own_centre.p[0] - h.centre[0], h.own_centre.p[1] - h.centre[1], h.own_centre.p[2] - h.centre[2]];
    let off = qrot(&pose.q, &d);
    let add = cross(&w, &off);
    for k in 0..3 {
        v[k] += add[k];
    }
    gcall(a, px::SET_CMASS_LOCAL_POSE, ptr(&h.own_centre), 0, 0)?;
    gcall(a, px::SET_MASS, fbits(h.own_mass), 0, 0)?;
    gcall(a, px::SET_INERTIA, h.own_inertia.as_ptr() as u64, 0, 0)?;
    gcall(a, px::SET_LINEAR_DAMPING, fbits(h.own_damping), 0, 0)?;
    gcall(a, px::SET_MAX_ANGULAR_VELOCITY, fbits(if h.own_spin > 0.0 { h.own_spin } else { 7.0 }), 0, 0)?;
    if (v[0] + v[1] + v[2]).is_finite() {
        gcall(a, px::SET_LINEAR_VELOCITY, v.as_ptr() as u64, 1, 0)?;
    }
    Some(())
}
/// HeldDrive: one step of a held body -- velocity toward the requested grip point and a damped angular drive. Grip
/// torque is measured against the original inertia; the solver's artificial inertia multiplier must not weaken it.
/// (the grip-position error, the current rotation error)
unsafe fn held_drive(h: &Held, to: &PxTransform, torque: f32, step: f32) -> Option<(f32, [f32; 4])> {
    let a = h.actor;
    let mut pose = PxTransform::default();
    let mut w = [0.0f32; 3];
    gcall(a, px::GET_GLOBAL_POSE, mptr(&mut pose), 0, 0)?;
    gcall(a, px::GET_ANGULAR_VELOCITY, w.as_mut_ptr() as u64, 0, 0)?;
    let d = qrot(&pose.q, &h.centre);
    let at = [pose.p[0] + d[0], pose.p[1] + d[1], pose.p[2] + d[2]];
    let d = qrot(&to.q, &h.centre);
    let mut v = [0.0f32; 3];
    let mut o2 = 0.0f32;
    for k in 0..3 {
        let want = to.p[k] + d[k];
        v[k] = (want - at[k]) / step;
        o2 += (want - at[k]) * (want - at[k]);
    }
    let off = o2.sqrt();
    let vn = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if vn > HOLD_MOST_SPEED {
        for c in &mut v {
            *c *= HOLD_MOST_SPEED / vn;
        }
    }
    v[2] += 9.81 * step;
    let yield_ = qmul(&pose.q, &qconj(&to.q)); // it now, off the hand's turn
    let mut dq = qmul(&to.q, &qconj(&pose.q));
    if dq[3] < 0.0 {
        for c in &mut dq {
            *c = -*c;
        }
    }
    let sn = (dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]).sqrt();
    let mut wt = [0.0f32; 3];
    if sn > 1e-7 {
        let angle = 2.0 * sn.atan2(dq[3]);
        for k in 0..3 {
            wt[k] = dq[k] / sn * angle;
        }
    }
    // The grip follows the controller against the original physical inertia. The artificial mass / inertia increase
    // stabilizes connected ragdoll bodies; applying it here too weakens the wrist by 20x or more.
    let w = held_grip::drive_angular(&wt, &w, &h.own_inertia, torque, step, HOLD_MOST_SPIN);
    if !(v[0] + v[1] + v[2] + w[0] + w[1] + w[2]).is_finite() {
        return None;
    }
    gcall(a, px::SET_LINEAR_VELOCITY, v.as_ptr() as u64, 1, 0)?;
    gcall(a, px::SET_ANGULAR_VELOCITY, w.as_ptr() as u64, 1, 0)?;
    Some((off, yield_))
}

/// HoldRelease: this hand lets go -- the body it held is its own again once no hand holds it.
unsafe fn hold_release(side: usize) {
    let r = &mut hold_run[side];
    if r.held >= 0 {
        let h = &mut helds[r.held as usize];
        h.holders -= 1;
        if h.holders <= 0 {
            let still = held_alive(h.actor, h.proxy, h.body) && !body_gone(h.proxy, h.body);
            let given = still && held_give(h).is_some();
            log!(
                "[physhands] {} hand lets go of the body it held (proxy {} body {}) after {} steps: {}; its centre ended a step {:.3} m off the hands on average, {:.3} at the most; turned off the hands' turn by its weight {:.0} deg at the most\n",
                if side == 1 { "right" } else { "left" },
                h.proxy,
                h.body,
                h.steps,
                if !still {
                    "its body is gone"
                } else if given {
                    "its own again"
                } else {
                    "PhysX refused"
                },
                if h.steps != 0 { h.off_sum / h.steps as f32 } else { 0.0 },
                h.off_most,
                h.yield_most * 57.2958
            );
            *h = HELD0;
        } else {
            log!("[physhands] {} hand lets go of the body both held (proxy {} body {}): the other holds it alone\n", if side == 1 { "right" } else { "left" }, h.proxy, h.body);
        }
    }
    HOLD_LOADS[side].write(&HoldLoad { at: tick_ms(), ..Default::default() });
    *r = RUN0;
}

/// HoldSide: one hand -- its asking read, taken or let go, and where it has the body this step.
unsafe fn hold_side(side: usize, sc: P, origin: &[f32; 3], step: f32) {
    hold_run[side].to_on = false;
    hold_run[side].target_grip_valid = false;
    let h: BodyHold = BODY_HOLDS[side].read().unwrap_or_default();
    let asked = !cvr_input::scanner::SCANNER_HAND_PHYSICS.active() && lc!(xrHandPhysics) != 0 && lc!(xrHandCollision) != 0 && h.on && h.actor != 0 && h.scene == sc as usize && tick_ms().wrapping_sub(h.at) < 300;
    let ha = h.actor as P;
    let alive = asked && !body_gone(h.proxy, h.body) && held_alive(ha, h.proxy, h.body);
    {
        let r = &hold_run[side];
        if r.held >= 0 && (!alive || !held_body_mass::same_actor(h.actor, h.proxy, h.body, r.actor as usize, r.proxy, r.body)) {
            hold_release(side);
        }
    }
    if !alive {
        return;
    }
    let r = &mut hold_run[side];
    r.torque = h.torque;
    let dh = step_pose_inputs.drawn[side];
    if step_pose_inputs.at == 0 || dh.at == 0 || tick_ms().wrapping_sub(dh.at) > 200 {
        return;
    }
    let Some(control) = super::body::target_now(side, origin) else { return };
    let hp = [dh.p[0] - origin[0], dh.p[1] - origin[1], dh.p[2] - origin[2]];
    let hq = dh.q;
    let mut take = r.held < 0;
    if !take && r.has_last {
        let j2: f32 = (0..3).map(|k| (dh.p[k] - r.hand_p[k]) * (dh.p[k] - r.hand_p[k])).sum();
        if j2 > HOLD_JUMP * HOLD_JUMP {
            take = true;
            log!("[physhands] {} hand jumped {:.2} m holding a body: eased in again from where the body is\n", if side == 1 { "right" } else { "left" }, j2.sqrt());
        }
    }
    if take {
        let Some(a) = held_pose(ha) else { return };
        let hi = qconj(&hq);
        let d = [a.p[0] - hp[0], a.p[1] - hp[1], a.p[2] - hp[2]];
        r.from_p = qrot(&hi, &d);
        r.from_q = qmul(&hi, &a.q);
        if !qnorm(&mut r.from_q) {
            return;
        }
        if r.held < 0 {
            // given in another body's frame (the one the script asked of): carried onto this one as the two lie now
            r.grip = h.g;
            r.want_p = h.p;
            r.want_q = h.q;
            let href = h.reference as P;
            if !href.is_null() && href != ha {
                if !held_alive(href, h.proxy, h.ref_body) {
                    return;
                }
                let Some(b) = held_pose(href) else { return };
                // T = b^-1 a (this body in the asked one's frame); the hold's pose times T; the point held by T^-1
                let bi = qconj(&b.q);
                let t = [a.p[0] - b.p[0], a.p[1] - b.p[1], a.p[2] - b.p[2]];
                let tp = qrot(&bi, &t);
                let mut tq = qmul(&bi, &a.q);
                if !qnorm(&mut tq) {
                    return;
                }
                let t = qrot(&h.q, &tp);
                for k in 0..3 {
                    r.want_p[k] = h.p[k] + t[k];
                }
                r.want_q = qmul(&h.q, &tq);
                if !qnorm(&mut r.want_q) {
                    return;
                }
                let ti = qconj(&tq);
                let t = [h.g[0] - tp[0], h.g[1] - tp[1], h.g[2] - tp[2]];
                r.grip = qrot(&ti, &t);
            }
            // the other hand's body already: held by both
            let mut at_i: i32 = -1;
            for i in 0..2 {
                if helds[i].holders > 0 && held_body_mass::same_actor(helds[i].actor as usize, helds[i].proxy, helds[i].body, h.actor, h.proxy, h.body) {
                    at_i = i as i32;
                }
            }
            if at_i >= 0 {
                helds[at_i as usize].holders += 1;
                log!("[physhands] {} hand holds the body the other holds too (proxy {} body {}): held by both, the pull shared\n", if side == 1 { "right" } else { "left" }, h.proxy, h.body);
            } else {
                for i in 0..2 {
                    if at_i < 0 && helds[i].holders <= 0 {
                        at_i = i as i32;
                    }
                }
                if at_i < 0 {
                    return;
                }
                let n = &mut helds[at_i as usize];
                *n = HELD0;
                if held_take(ha, &r.grip, n).is_none() {
                    return;
                }
                n.actor = ha;
                n.proxy = h.proxy;
                n.body = h.body;
                n.holders = 1;
                let g2: f32 = (0..3).map(|k| (r.want_p[k] - r.from_p[k]) * (r.want_p[k] - r.from_p[k])).sum();
                log!(
                    "[physhands] {} hand holds a body (proxy {} body {}{}) at the point it closed on, as a pistol: {:.1} kg carried as {:.0}, {:.3} m from where the hold has it, eased in over {:.0} ms; turned by a grip of {:.0} N m\n",
                    if side == 1 { "right" } else { "left" },
                    h.proxy,
                    h.body,
                    if !href.is_null() && href != ha { ", the part's own, asked of another" } else { "" },
                    n.own_mass,
                    n.made_mass,
                    g2.sqrt(),
                    HOLD_EASE * 1000.0,
                    h.torque
                );
            }
            r.held = at_i;
            r.actor = ha;
            r.proxy = h.proxy;
            r.body = h.body;
        }
        r.t = 0.0;
        r.has_last = false;
    }
    r.t += step;
    let s = (r.t / HOLD_EASE).min(1.0);
    let e = s * s * (3.0 - 2.0 * s);
    let sgn = if r.from_q[0] * r.want_q[0] + r.from_q[1] * r.want_q[1] + r.from_q[2] * r.want_q[2] + r.from_q[3] * r.want_q[3] < 0.0 { -1.0 } else { 1.0 };
    for k in 0..3 {
        r.rel_p[k] = r.from_p[k] + (r.want_p[k] - r.from_p[k]) * e;
    }
    for k in 0..4 {
        r.rel_q[k] = r.from_q[k] + (r.want_q[k] * sgn - r.from_q[k]) * e;
    }
    if !qnorm(&mut r.rel_q) {
        return;
    }
    let Some((tp, tq)) = held_grip::drive_target(&hp, &hq, &control.q, &r.rel_p, &r.rel_q, &r.grip) else { return };
    r.to = PxTransform { q: tq, p: tp };
    match held_grip_contact::world_point(&r.to.p, &r.to.q, &r.grip, origin) {
        Some(w) => {
            r.target_grip_world = w;
            r.target_grip_valid = true;
        }
        None => r.target_grip_valid = false,
    }
    r.target_grip_scene = sc as usize;
    r.target_grip_generation = held_contact_step.generation;
    r.hand_p = dh.p;
    r.has_last = true;
    r.to_on = true;
}

/// HoldSteps (before the step): each hand's hold read, and each held body driven toward where its hands have it.
pub unsafe fn hold_steps(sc: P, origin: &[f32; 3], dt: f32) {
    held_contact_step.prepared(sc as usize);
    if !hold_run_ready {
        hold_run = [RUN0; 2];
        hold_run_ready = true;
    }
    let step = if dt > 1e-4 && dt < 0.1 { dt } else { 1.0 / 60.0 };
    for side in 0..2 {
        hold_side(side, sc, origin, step);
    }
    let now = tick_ms();
    for i in 0..2 {
        if helds[i].actor.is_null() || helds[i].holders <= 0 {
            continue;
        }
        let sides: Vec<usize> = (0..2).filter(|&s| hold_run[s].held == i as i32 && hold_run[s].to_on).collect();
        if sides.is_empty() {
            continue;
        }
        let a = hold_run[sides[0]];
        let mut to = a.to;
        let mut centre = a.grip;
        if sides.len() == 2 {
            // Both grip points steer the body; wrist turns supply the remaining twist.
            let b = hold_run[sides[1]];
            let Some((p, q, c)) = held_grip::two_hand_target(&a.to.p, &a.to.q, &a.grip, &b.to.p, &b.to.q, &b.grip) else { continue };
            to = PxTransform { q, p };
            centre = c;
        }
        let h = &mut helds[i];
        let c2: f32 = (0..3).map(|k| (centre[k] - h.centre[k]) * (centre[k] - h.centre[k])).sum();
        if c2 > 1e-6 && held_centre(h, &centre).is_none() {
            continue;
        }
        let torque: f32 = sides.iter().map(|&s| hold_run[s].torque).sum();
        let Some((off, yield_)) = held_drive(h, &to, torque, step) else { continue };
        let least = sides.iter().map(|&s| hold_run[s].t).fold(1e9f32, f32::min);
        let eased = least > HOLD_EASE + 0.05;
        let ys = (yield_[0] * yield_[0] + yield_[1] * yield_[1] + yield_[2] * yield_[2]).sqrt();
        if eased {
            h.steps += 1;
            h.off_sum += off;
            h.off_most = h.off_most.max(off);
            h.yield_most = h.yield_most.max(2.0 * ys.atan2(yield_[3].abs()));
        }
        if now.wrapping_sub(h.said_at) > 2000 && eased {
            h.said_at = now;
            log!(
                "[physhands] a body held by {} (proxy {} body {}): turned {:.0} deg off the hand{} by its weight (a grip of {:.0} N m), its centre {:.3} m off\n",
                if sides.len() == 2 {
                    "both hands"
                } else if sides[0] == 1 {
                    "the right hand"
                } else {
                    "the left hand"
                },
                h.proxy,
                h.body,
                2.0 * ys.atan2(yield_[3].abs()) * 57.2958,
                if sides.len() == 2 { "s" } else { "" },
                torque,
                off
            );
        }
    }
}

/// ActorPoint: the point `local` of an actor's frame in the game's world (`origin` its scene's).
pub unsafe fn actor_point(a: P, origin: &[f32; 3], local: &[f32; 3]) -> Option<[f32; 3]> {
    if a.is_null() {
        return None;
    }
    let p = held_pose(a)?;
    let d = qrot(&p.q, local);
    let w = [p.p[0] + d[0] + origin[0], p.p[1] + d[1] + origin[1], p.p[2] + d[2] + origin[2]];
    (w[0] + w[1] + w[2]).is_finite().then_some(w)
}

static mut HOLD_SAID: [bool; 2] = [false; 2];
/// HoldBody: a body held by the hand at the point it closed on, as a pistol is (the script's carry of a body lying
/// dead): its PhysX actor and its scene, its pose in the hand bone's frame (`pos` xyz, `rot` x y z w), `grip` xyz the
/// point held in the body's frame, and `pos` w the hand's grip torque (N m). Asked every frame it is held: unasked 0.3
/// s, or with a null actor, it is let go. False when its scene is not the hands'. Script thread (one writer).
#[allow(clippy::too_many_arguments)]
pub unsafe fn hold_body(side: usize, a: P, actor_scene: P, proxy: u32, body: u32, pos: Option<&[f32; 4]>, rot: Option<&[f32; 4]>, grip: Option<&[f32; 3]>, reference: P, ref_body: u32) -> bool {
    if side > 1 {
        return false;
    }
    let scene_now = super::body::scene;
    let mut h = BodyHold::default();
    if let (false, Some(pos), Some(rot), Some(grip)) = (a.is_null(), pos, rot, grip) {
        if actor_scene == scene_now {
            h.actor = a as usize;
            h.scene = actor_scene as usize;
            h.proxy = proxy;
            h.body = body;
            h.reference = reference as usize;
            h.ref_body = ref_body;
            h.p = [pos[0], pos[1], pos[2]];
            h.q = *rot;
            h.g = *grip;
            h.torque = pos[3];
            h.on = qnorm(&mut h.q) && (h.p[0] + h.p[1] + h.p[2] + h.g[0] + h.g[1] + h.g[2] + h.torque).is_finite();
        }
    }
    h.at = tick_ms();
    BODY_HOLDS[side].write(&h);
    if !a.is_null() && !h.on && !HOLD_SAID[side] {
        HOLD_SAID[side] = true;
        log!(
            "[physhands] {} hand cannot hold a body fast (proxy {} body {}): {}\n",
            if side == 1 { "right" } else { "left" },
            proxy,
            body,
            if actor_scene != scene_now { "not the hands' scene" } else { "no pose for it in the hand" }
        );
    }
    if a.is_null() {
        true
    } else {
        h.on
    }
}

/// PublishHeldWrists (after the fetch): each hand's held body's measured turn off the hand and the point held, its mass
/// before the solver's override, and how far the point held is from where the hand had it (this step's contact).
pub unsafe fn publish_held_wrists(now: u64, origin: &[f32; 3], contact: &held_grip_contact::Fetch) {
    if !hold_run_ready {
        return;
    }
    for side in 0..2 {
        let r = hold_run[side];
        let mut load = HoldLoad { at: now, ..Default::default() };
        if r.held >= 0 && r.to_on {
            let held = helds[r.held as usize];
            if !held.actor.is_null() && held_alive(held.actor, held.proxy, held.body) && !body_gone(held.proxy, held.body) {
                if let Some(pose) = held_pose(held.actor) {
                    if let Some(w) = held_grip::wrist_rotation(&pose.q, &r.rel_q) {
                        load.wrist = w;
                        let g = qrot(&r.rel_q, &r.grip);
                        for k in 0..3 {
                            load.seat[k] = r.rel_p[k] + g[k];
                        }
                        load.proxy = held.proxy;
                        load.body = held.body;
                        load.own_mass = held.own_mass;
                        load.contact_error_um = held_grip_contact::error_micrometres(
                            &pose.p,
                            &pose.q,
                            &r.grip,
                            origin,
                            &r.target_grip_world,
                            r.target_grip_valid && contact.valid && r.target_grip_scene == contact.scene && r.target_grip_generation == contact.generation,
                        );
                        load.on = true;
                    }
                }
            }
        }
        HOLD_LOADS[side].write(&load);
    }
}
