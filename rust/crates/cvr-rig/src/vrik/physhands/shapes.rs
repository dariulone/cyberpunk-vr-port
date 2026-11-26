//! The hand's body's own shapes posed each step (physics thread), the arm's reach on tracking, and the hand put back
//! when it is stuck behind something an arm does not reach round.
use super::body::{
    actor, box_with_margin, finger_half, finger_radius, finger_shape, forearm_half, forearm_radius, forearm_shape, forearm_upper, forearm_upper_half, made_centre, made_half, made_skin, on_world, shape,
    simulating, weapon_equipped, weapon_half, weapon_local, weapon_made, weapon_on, weapon_shape, applied_mass, made_along, MOST_WEAPON_BOXES,
};
use super::census::player_pos;
use super::fingers::{BENDS, FINGER_HANDS};
use super::loose::{global_pose, palm_in_loose};
use super::math::{axis_angle, compose, qmul, qnorm, qrot};
use super::near::{near_lists, NEAR_FRONT};
use super::pose::{step_forearm, step_shoulder, step_target, REACH_HELD};
use super::px::{self, at, BoxGeometry, CapsuleGeometry, PxTransform, P};
use super::seq::Seq;
use super::settings::Settings;
use super::tick_ms;
use super::touch::{path_free, touches_world, BodyShapes, BODY_SHAPES, BODY_SHAPES0, BODY_SHAPES_SEQ, TOUCH_ALL};
use core::sync::atomic::Ordering::*;
use cvr_host::log;

/// The gun in a hand (script thread writes, physics thread reads): its boxes in its own frame, the gun's frame in the
/// hand bone's (rotation x y z w, then position).
#[derive(Clone, Copy)]
pub struct Weapon {
    pub n: i32,
    pub frame: [f32; 7],
    pub lo: [[f32; 3]; MOST_WEAPON_BOXES],
    pub hi: [[f32; 3]; MOST_WEAPON_BOXES],
    pub at: u64,
}
pub const WEAPON0: Weapon = Weapon { n: 0, frame: [0.0; 7], lo: [[0.0; 3]; MOST_WEAPON_BOXES], hi: [[0.0; 3]; MOST_WEAPON_BOXES], at: 0 };
pub static WEAPONS: [Seq<Weapon>; 2] = [Seq::new(WEAPON0), Seq::new(WEAPON0)];
/// the script's thread only: parts come one by one, the frame commits them
pub static mut weapon_staged: [Weapon; 2] = [WEAPON0; 2];

/// m the player's entity moves in one step: a teleport, not a walk or a fall
pub const TELEPORT_STEP: f32 = 1.0;
/// the hands put where tracking is at least that long after it...
pub const TELEPORT_PUT_MS: u64 = 300;
/// ...and on while tracking's place still jumps, up to this
pub const TELEPORT_PUT_MOST_MS: u64 = 2000;
/// m a body is held off where tracking is...
const STUCK_GAP: f32 = 0.25;
/// ...for this long before it is asked whether it is stuck
const STUCK_MS: u64 = 400;
/// m: closer, held off by something thin between it and tracking...
const BEHIND_GAP: f32 = 0.03;
/// ...this long, asked as well
const BEHIND_MS: u64 = 250;
pub static mut far_since: [u64; 2] = [0; 2];
static mut stuck_asked: [u64; 2] = [0; 2];
static mut STUCK_TOLD_AT: [u64; 2] = [0; 2];

/// THE ARM'S REACH: tracking farther from the shoulder (as the arm solve placed it this frame) than the arm's length is
/// brought back along the shoulder's line to that length. Nothing is limited without a fresh solve.
pub unsafe fn arm_reach(side: usize, to: &mut PxTransform, origin: &[f32; 3], tick: u64, s: &Settings) {
    let Some(sh) = step_shoulder(side) else { return };
    if tick.wrapping_sub(sh.at) > 300 || sh.reach <= 0.0 {
        return;
    }
    let Some(t) = step_target(side) else { return };
    let r = qrot(&t.entity_rot, &sh.model);
    let mut d = [0.0f32; 3];
    let mut n = 0.0f32;
    for k in 0..3 {
        let shoulder = t.entity_pos[k] + r[k] - origin[k];
        d[k] = to.p[k] - shoulder;
        n += d[k] * d[k];
    }
    n = n.sqrt();
    let most = sh.reach + s.arm_reach_spare;
    if !(n > most) || !n.is_finite() {
        return;
    }
    let k1 = most / n;
    for k in 0..3 {
        to.p[k] -= d[k] * (1.0 - k1);
    }
    REACH_HELD[side].fetch_add(1, Relaxed);
}

/// STUCK. Bonelab's hands hang on a jointed arm, which does not reach round a wall, and its menu has "unstick
/// controllers" for when they hang anyway; these hang on nothing, so it is done here, by itself. A body the world holds
/// more than STUCK_GAP off where tracking is for STUCK_MS (or more than BEHIND_GAP for BEHIND_MS, with something between
/// it and tracking) is put there -- when the palm there is in nothing and a straight arm from the shoulder gets there
/// through nothing. A hand pushed into a wall, under a table or through a thin wall fails one of the two and stays
/// where it is stopped. Physics thread, between steps.
pub unsafe fn unstick(side: usize, to: &PxTransform, origin: &[f32; 3], holding: i32, tick: u64, s: &Settings) -> bool {
    let a = actor[side];
    if a.is_null() || !simulating[side] || holding & (1 << side) != 0 {
        far_since[side] = 0;
        return false;
    }
    let now = global_pose(a);
    let gap2: f32 = (0..3).map(|k| (to.p[k] - now.p[k]) * (to.p[k] - now.p[k])).sum();
    if !(gap2 > BEHIND_GAP * BEHIND_GAP) {
        far_since[side] = 0;
        return false;
    }
    // FAR, or close with something between: a body put by a teleport's passing frames on the far side of a thin wall
    // sat 23 cm off tracking behind it, under the far gap, for good (2026-11-09)
    let distant = gap2 > STUCK_GAP * STUCK_GAP;
    if far_since[side] == 0 {
        far_since[side] = tick;
    }
    if tick - far_since[side] < (if distant { STUCK_MS } else { BEHIND_MS }) || tick.wrapping_sub(stuck_asked[side]) < (if distant { 200 } else { 100 }) {
        return false;
    }
    stuck_asked[side] = tick;
    let refuse = |why: &str| {
        if distant && tick.wrapping_sub(STUCK_TOLD_AT[side]) >= 1000 {
            STUCK_TOLD_AT[side] = tick;
            log!("[physhands] {} hand {:.2} m off tracking for {} ms, left there: {}\n", if side == 1 { "right" } else { "left" }, gap2.sqrt(), tick - far_since[side], why);
        }
        false
    };
    // the world list asked must be the one round where the player is now
    let front = NEAR_FRONT.load(Acquire);
    if front < 0 {
        return refuse("no world list yet");
    }
    {
        let p = player_pos();
        let mut c = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
        c[2] += 1.1;
        let center = near_lists[front as usize].center;
        let d2: f32 = (0..3).map(|k| (c[k] - center[k]) * (c[k] - center[k])).sum();
        if d2 > 0.5 * 0.5 {
            return refuse("the world list is still the old place's");
        }
    }
    let (Some(sh), Some(t)) = (step_shoulder(side), step_target(side)) else { return refuse("no shoulder from the arm solve") };
    if tick.wrapping_sub(sh.at) > 300 {
        return refuse("no shoulder from the arm solve");
    }
    let r = qrot(&t.entity_rot, &sh.model);
    let mut shoulder = [t.entity_pos[0] + r[0] - origin[0], t.entity_pos[1] + r[1] - origin[1], t.entity_pos[2] + r[2] - origin[2]];
    let off = made_centre[side];
    let r = qrot(&to.q, &off);
    let palm = [to.p[0] + r[0], to.p[1] + r[1], to.p[2] + r[2]];
    // held off by the world only: a hand held back by a crate it pushes is not stuck, and put inside the crate it
    // would throw it
    if !on_world[side] {
        return refuse("held off by something loose, not by the world");
    }
    if touches_world(side, to, s, TOUCH_ALL, None) {
        return refuse("tracking's place is in the world (pushed into it)");
    }
    if palm_in_loose(side, to) {
        return refuse("tracking's place is in a loose thing");
    }
    {
        // from 6 cm out of the shoulder: the shoulder is inside the player, and a wall V stands with its back to met the
        // first ball and refused every hand (2026-11-09)
        let d = [palm[0] - shoulder[0], palm[1] - shoulder[1], palm[2] - shoulder[2]];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if n > 0.12 {
            for k in 0..3 {
                shoulder[k] += d[k] * (0.06 / n);
            }
        }
    }
    match path_free(&shoulder, &palm, 0.03) {
        Some(Ok(())) => {}
        other => {
            let met = match other {
                Some(Err(m)) => m,
                _ => String::new(),
            };
            let why = format!(
                "something between the shoulder ({:.2} {:.2} {:.2}) and tracking's palm ({:.2} {:.2} {:.2}): {} (PhysX's space, the world's minus {:.2} {:.2} {:.2})",
                shoulder[0] + origin[0],
                shoulder[1] + origin[1],
                shoulder[2] + origin[2],
                palm[0] + origin[0],
                palm[1] + origin[1],
                palm[2] + origin[2],
                met,
                origin[0],
                origin[1],
                origin[2]
            );
            return refuse(&why);
        }
    }
    if !distant {
        // close: only when the straight way from the body's palm to tracking's crosses something (a thin ball, so a palm
        // merely pressed on a wall with tracking just off it is not "behind" it)
        let r = qrot(&now.q, &off);
        let body_palm = [now.p[0] + r[0], now.p[1] + r[1], now.p[2] + r[2]];
        if matches!(path_free(&body_palm, &palm, 0.008), Some(Ok(()))) {
            return false;
        }
    }
    log!(
        "[physhands] {} hand stuck {:.2} m off tracking for {} ms behind something: put back where tracking is\n",
        if side == 1 { "right" } else { "left" },
        gap2.sqrt(),
        tick - far_since[side]
    );
    far_since[side] = 0;
    true
}

/// Between: a capsule's pose (its axis x) and half height between two points.
pub fn between(a: &[f32; 3], b: &[f32; 3], out: &mut PxTransform) -> f32 {
    let mut d = [0.0f32; 3];
    for k in 0..3 {
        d[k] = b[k] - a[k];
        out.p[k] = 0.5 * (a[k] + b[k]);
    }
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-4 {
        out.q = [0.0, 0.0, 0.0, 1.0];
        return 0.0;
    }
    for v in &mut d {
        *v /= len;
    }
    if d[0] < -0.9999 {
        out.q = [0.0, 0.0, 1.0, 0.0];
    } else {
        out.q = [0.0, -d[2], d[1], 1.0 + d[0]];
        qnorm(&mut out.q);
    }
    0.5 * len
}
pub unsafe fn set_capsule(sh: P, radius: f32, half: f32, made: &mut f32, made_radius: &mut f32, local: &PxTransform) {
    if (half - *made).abs() > 0.002 || (radius - *made_radius).abs() > 0.0003 {
        let g = CapsuleGeometry { kind: px::GEOM_CAPSULE, radius, half_height: half };
        at::<unsafe extern "C" fn(P, *const CapsuleGeometry)>(sh, px::SHAPE_SET_GEOMETRY)(sh, &g);
        *made = half;
        *made_radius = radius;
    }
    at::<unsafe extern "C" fn(P, *const PxTransform)>(sh, px::SHAPE_SET_LOCAL_POSE)(sh, local);
}

static mut PUB: BodyShapes = BODY_SHAPES0;
unsafe fn keep(geometry: &[u8], local: &PxTransform, reach: f32, part: u8) {
    let b = &mut PUB;
    if b.n >= BODY_SHAPES {
        return;
    }
    let e = &mut b.s[b.n];
    b.n += 1;
    e.geometry = [0; 16];
    e.geometry[..geometry.len()].copy_from_slice(geometry);
    e.local = *local;
    e.reach = reach;
    e.part = part;
}
fn bytes<T>(v: &T) -> &[u8] {
    // SAFETY: a plain repr(C) geometry viewed as its bytes.
    unsafe { core::slice::from_raw_parts((v as *const T).cast::<u8>(), core::mem::size_of::<T>()) }
}
unsafe fn capsule_kept(sh: P, radius: f32, half: f32, made: &mut f32, made_radius: &mut f32, local: &PxTransform, part: u8) {
    set_capsule(sh, radius, half, made, made_radius, local);
    let g = CapsuleGeometry { kind: px::GEOM_CAPSULE, radius: *made_radius, half_height: *made };
    keep(bytes(&g), local, *made + *made_radius, part);
}

/// PoseShapes: each step, before it, the finger capsules where the fingers are drawn (the layers' pose and the
/// worker's bends), the forearm's from the wrist toward the elbow; parked in the palm when there is nothing to pose them
/// by. What is posed is published as it leaves (whichever way).
pub unsafe fn pose_shapes(side: usize, s: &Settings) {
    PUB.n = 0;
    pose_shapes_inner(side, s);
    PUB.at = tick_ms();
    BODY_SHAPES_SEQ[side].write(&PUB);
}
unsafe fn pose_shapes_inner(side: usize, s: &Settings) {
    let now = tick_ms();
    // THE BOX, V'S OWN: the body is known some time after the hands are made (the rig's policy), and a change of it (a
    // new rig, the skin setting) is taken at once -- the box's size and place, and the mass's spread with it
    {
        let (half, centre, skin_of) = box_with_margin(side, s);
        if (skin_of != made_skin[side] || (half[0] - made_half[side][0]).abs() > 1e-5) && !shape[side].is_null() {
            let g = BoxGeometry { kind: px::GEOM_BOX, half };
            at::<unsafe extern "C" fn(P, *const BoxGeometry)>(shape[side], px::SHAPE_SET_GEOMETRY)(shape[side], &g);
            let at_pose = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: centre };
            at::<unsafe extern "C" fn(P, *const PxTransform)>(shape[side], px::SHAPE_SET_LOCAL_POSE)(shape[side], &at_pose);
            made_half[side] = half;
            made_centre[side] = centre;
            made_along[side] = centre[0].abs();
            made_skin[side] = skin_of;
            applied_mass[side] = -1.0; // the inertia again, for the new box
            log!(
                "[physhands] {} hand's palm fitted to {} skin: box {:.3} x {:.3} x {:.3} m about ({:.3} {:.3} {:.3})\n",
                if side == 1 { "right" } else { "left" },
                match skin_of {
                    1 => "a woman's",
                    0 => "a man's",
                    _ => "no (the ini's box)",
                },
                2.0 * half[0],
                2.0 * half[1],
                2.0 * half[2],
                centre[0],
                centre[1],
                centre[2]
            );
        }
    }
    let palm = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: made_centre[side] };
    {
        let g = BoxGeometry { kind: px::GEOM_BOX, half: made_half[side] };
        let h = g.half;
        keep(bytes(&g), &palm, (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt(), 0);
    }
    let hand = if s.finger_shapes != 0 { FINGER_HANDS[side].read().filter(|h| now.wrapping_sub(h.at) < 200) } else { None };
    let chains = hand.is_some();
    let h = hand.unwrap_or_default();
    let b = if chains { BENDS[side].read().filter(|b| b.on && now.wrapping_sub(b.at) < 250) } else { None };
    for f in 0..5 {
        let c = &h.f[f];
        let mut j_ = [[0.0f32; 3]; 4];
        let mut fq = [[0.0f32, 0.0, 0.0, 1.0]; 3];
        if chains {
            let mut pq = [c.base[0], c.base[1], c.base[2], c.base[3]];
            let mut pp = [c.base[4], c.base[5], c.base[6]];
            let mut last = [0.0f32, 0.0, 0.0, 1.0];
            for j in 0..3 {
                let turn = axis_angle(&c.hinge[j], b.map_or(0.0, |b| b.y[f][j]));
                let lq = qmul(&[c.local[j][0], c.local[j][1], c.local[j][2], c.local[j][3]], &turn);
                let (nq, np) = compose(&pq, &pp, &lq, &[c.local[j][4], c.local[j][5], c.local[j][6]]);
                pq = nq;
                pp = np;
                j_[j] = np;
                last = nq;
                fq[j] = nq;
            }
            let tip = qrot(&last, &c.tip);
            for k in 0..3 {
                j_[3][k] = pp[k] + tip[k];
            }
        }
        for j in 0..3 {
            let sh = finger_shape[side][f][j];
            if sh.is_null() {
                continue;
            }
            let mut local = palm;
            let mut half = finger_half[side][f][j];
            let mut radius = 0.008f32;
            if chains && c.radius[j] > 0.0 {
                // the segment's line, moved off the bone to the skin's centre (in the bone's frame)
                let o = qrot(&fq[j], &c.offset[j]);
                let a = [j_[j][0] + o[0], j_[j][1] + o[1], j_[j][2] + o[2]];
                let e = [j_[j + 1][0] + o[0], j_[j + 1][1] + o[1], j_[j + 1][2] + o[2]];
                half = between(&a, &e, &mut local);
                radius = c.radius[j] - 0.0005;
            }
            capsule_kept(sh, radius, half.max(0.003), &mut finger_half[side][f][j], &mut finger_radius[side][f][j], &local, 1);
        }
    }
    // THE GUN: its boxes where the script measured it in this hand; parked in the palm without one (or a stale one)
    {
        let wp = if s.weapon != 0 && weapon_equipped() { WEAPONS[side].read().filter(|w| w.n > 0 && now.wrapping_sub(w.at) < 1500) } else { None };
        let armed = wp.is_some();
        let wp = wp.unwrap_or(WEAPON0);
        let mut on = 0usize;
        for i in 0..MOST_WEAPON_BOXES {
            let sh = weapon_shape[side][i];
            if sh.is_null() {
                continue;
            }
            let mut local = palm;
            let mut half = [0.004f32; 3];
            if armed && (i as i32) < wp.n {
                let mut c = [0.0f32; 3];
                for k in 0..3 {
                    c[k] = 0.5 * (wp.lo[i][k] + wp.hi[i][k]);
                    half[k] = (0.5 * (wp.hi[i][k] - wp.lo[i][k])).max(0.005);
                }
                let (lq, lp) = compose(&[wp.frame[0], wp.frame[1], wp.frame[2], wp.frame[3]], &[wp.frame[4], wp.frame[5], wp.frame[6]], &[0.0, 0.0, 0.0, 1.0], &c);
                local = PxTransform { q: lq, p: lp };
                weapon_local[side][on] = local;
                weapon_half[side][on] = half;
                on += 1;
                let kept = BoxGeometry { kind: px::GEOM_BOX, half };
                keep(bytes(&kept), &local, (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt(), 3);
            }
            let made = &mut weapon_made[side][i];
            let changed = (0..3).any(|k| (half[k] - made[k]).abs() > 0.001);
            if changed {
                let g = BoxGeometry { kind: px::GEOM_BOX, half };
                at::<unsafe extern "C" fn(P, *const BoxGeometry)>(sh, px::SHAPE_SET_GEOMETRY)(sh, &g);
                *made = half;
            }
            at::<unsafe extern "C" fn(P, *const PxTransform)>(sh, px::SHAPE_SET_LOCAL_POSE)(sh, &local);
        }
        weapon_on[side] = on as i32;
    }
    let (fa, fu) = (forearm_shape[side], forearm_upper[side]);
    if fa.is_null() || fu.is_null() {
        return;
    }
    if let Some(arm) = (s.forearm != 0).then(|| step_forearm(side)).flatten().filter(|a| now.wrapping_sub(a.at) < 200) {
        let len = (arm.elbow[0] * arm.elbow[0] + arm.elbow[1] * arm.elbow[1] + arm.elbow[2] * arm.elbow[2]).sqrt();
        if len > 0.15 {
            let k = if s.skin != 0 { super::skin_for(super::body_female(), side as i32) } else { None };
            let mut local = PxTransform::default();
            if let Some(k) = k {
                // V's own forearm, which tapers to the wrist: 3-11 cm from it, and from 11 cm to 4 cm short of the elbow
                let mut a = [0.0f32; 3];
                let mut m = [0.0f32; 3];
                let mut e = [0.0f32; 3];
                for i in 0..3 {
                    a[i] = arm.elbow[i] * (0.03 / len);
                    m[i] = arm.elbow[i] * (0.11 / len);
                    e[i] = arm.elbow[i] * ((len - 0.04) / len);
                }
                let half = between(&a, &m, &mut local);
                capsule_kept(fa, k.fore_lower + s.skin_margin, half.max(0.01), &mut forearm_half[side], &mut forearm_radius[side][0], &local, 2);
                let half = between(&m, &e, &mut local);
                capsule_kept(fu, k.fore_upper + s.skin_margin, half.max(0.01), &mut forearm_upper_half[side], &mut forearm_radius[side][1], &local, 2);
                return;
            }
            // from 6 cm up the forearm (the palm's box has the wrist) to 5 cm short of the elbow
            let mut a = [0.0f32; 3];
            let mut e = [0.0f32; 3];
            for i in 0..3 {
                a[i] = arm.elbow[i] * (0.06 / len);
                e[i] = arm.elbow[i] * ((len - 0.05) / len);
            }
            let half = between(&a, &e, &mut local);
            capsule_kept(fa, 0.035, half.max(0.01), &mut forearm_half[side], &mut forearm_radius[side][0], &local, 2);
            capsule_kept(fu, 0.01, 0.01, &mut forearm_upper_half[side], &mut forearm_radius[side][1], &palm, 2);
            return;
        }
    }
    let fh = forearm_half[side];
    capsule_kept(fa, 0.035, fh, &mut forearm_half[side], &mut forearm_radius[side][0], &palm, 2);
    capsule_kept(fu, 0.01, 0.01, &mut forearm_upper_half[side], &mut forearm_radius[side][1], &palm, 2);
}
