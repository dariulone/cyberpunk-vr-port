//! What the hand's body touches (physics thread; the worker for the other hand's shapes): the world near, the people,
//! V's own body and the other hand -- by PxGeometryQuery against the lists the walks keep, never a scene query.
use super::body::{actor, hands_meet_on, made_centre, made_half, MOST_WEAPON_BOXES};
use super::loose::{global_pose, penetration};
use super::math::{compose, qrot};
use super::near::{near_lists, world_bounds_fn, NearShape, NEAR_FRONT};
use super::people::{person_meets, person_meets_hand, person_near, person_near_n, person_owner, person_simulates, SELF_OWNER};
use super::px::{self, BoxGeometry, CapsuleGeometry, PxTransform, SphereGeometry};
use super::seq::Seq;
use super::settings::Settings;
use super::tick_ms;
use super::types::TouchInfo;
use core::ffi::c_void;
use core::sync::atomic::Ordering::*;
use cvr_physhands::climb_support::{self, Feature, Hit, MAX_HITS};
use cvr_physhands::handspace::{length_squared, Vector};

/// THE BODY'S SHAPES AS POSED THIS STEP, in the hand bone's frame (physics thread writes, the animation thread reads):
/// the palm's box, the finger capsules, the forearm's -- what the drawn hand is kept out of the world by
/// (keep_drawn_out). part: 0 palm, 1 finger, 2 forearm, 3 gun.
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub struct BodyShape {
    pub geometry: [u8; 16],
    pub local: PxTransform,
    pub reach: f32,
    pub part: u8,
}
pub const BODY_SHAPES: usize = 18 + MOST_WEAPON_BOXES;
#[derive(Clone, Copy)]
pub struct BodyShapes {
    pub s: [BodyShape; BODY_SHAPES],
    pub n: usize,
    pub at: u64,
}
pub const BODY_SHAPE0: BodyShape = BodyShape { geometry: [0; 16], local: PxTransform::ZERO, reach: 0.0, part: 0 };
pub const BODY_SHAPES0: BodyShapes = BodyShapes { s: [BODY_SHAPE0; BODY_SHAPES], n: 0, at: 0 };
pub static BODY_SHAPES_SEQ: [Seq<BodyShapes>; 2] = [Seq::new(BODY_SHAPES0), Seq::new(BODY_SHAPES0)];
impl BodyShape {
    pub fn kind(&self) -> i32 {
        i32::from_ne_bytes([self.geometry[0], self.geometry[1], self.geometry[2], self.geometry[3]])
    }
    pub fn capsule(&self) -> CapsuleGeometry {
        // SAFETY: 12 of the 16 bytes, read as the capsule they hold when kind() says so.
        unsafe { core::ptr::read_unaligned(self.geometry.as_ptr().cast()) }
    }
}
/// The body's shapes as posed, fresh (200 ms), for this side.
pub fn body_shapes(side: usize) -> Option<BodyShapes> {
    BODY_SHAPES_SEQ[side].read().filter(|bs| tick_ms().wrapping_sub(bs.at) < 200)
}

/// what touches_world counts: the world alone; with the people near; with V's own body and the other hand as well
pub const TOUCH_WORLD: i32 = 0;
pub const TOUCH_PEOPLE: i32 = 1;
pub const TOUCH_ALL: i32 = 2;

fn apart(p: &[f32; 3], reach: f32, e: &NearShape) -> bool {
    (0..3).any(|k| p[k] + reach < e.bounds.lo[k] || p[k] - reach > e.bounds.hi[k])
}
fn gptr<T>(g: &T) -> *const c_void {
    (g as *const T).cast()
}
fn palm_pose(side: usize, at: &PxTransform) -> PxTransform {
    // SAFETY: physics-thread state, read.
    let r = qrot(&at.q, unsafe { &made_centre[side] });
    PxTransform { q: at.q, p: [at.p[0] + r[0], at.p[1] + r[1], at.p[2] + r[2]] }
}
unsafe fn grown_box(side: usize, grow: f32) -> BoxGeometry {
    BoxGeometry { kind: px::GEOM_BOX, half: [made_half[side][0] + grow, made_half[side][1] + grow, made_half[side][2] + grow] }
}
fn box_reach(b: &BoxGeometry) -> f32 {
    (b.half[0] * b.half[0] + b.half[1] * b.half[1] + b.half[2] * b.half[2]).sqrt()
}
fn deep(r: Option<([f32; 3], f32)>) -> Option<([f32; 3], f32)> {
    r.filter(|(_, d)| *d > 0.0)
}

static mut OTHER_SCRATCH: BodyShapes = BODY_SHAPES0;
static mut OTHER: [NearShape; BODY_SHAPES] = [NearShape::ZERO; BODY_SHAPES];
/// The palm, grown by a centimetre and a half, in a static shape near the player (physics thread: the list the worker
/// reads is never written while it is the front one) -- or any of the body's other shapes, its fingers' and its
/// forearm's capsules grown by a centimetre: a hand held off a wall by its fingertips or its thumb is on the wall.
pub unsafe fn touches_world(side: usize, at_pose: &PxTransform, _s: &Settings, what: i32, mut info: Option<&mut TouchInfo>) -> bool {
    let front = NEAR_FRONT.load(Acquire);
    if front < 0 || px::penetration().is_none() {
        return false;
    }
    let l = &near_lists[front as usize];
    // THE OTHER HAND while the two meet is the world here too: pressed on, a hand is held soft as on a wall. Taken for a
    // loose thing, it was pushed at a loose thing's strength and the two went 4 cm into each other (2026-11-10).
    let mut other_n = 0usize;
    if what >= TOUCH_ALL && hands_meet_on && !actor[1 - side].is_null() {
        let op = global_pose(actor[1 - side]);
        let pose = [op.q[0], op.q[1], op.q[2], op.q[3], op.p[0], op.p[1], op.p[2]];
        other_n = other_hand_shapes(1 - side, &pose, &mut OTHER, &mut OTHER_SCRATCH, -1);
    }
    const GROW: f32 = 0.015;
    let geometry = grown_box(side, GROW);
    let pose = palm_pose(side, at_pose);
    let mut calls = 0;
    let mut touches = |g: *const c_void, at: &PxTransform, reach: f32, info: &mut Option<&mut TouchInfo>| -> bool {
        let mut i = 0;
        while i < l.n && calls < 32 {
            let e = &l.s[i];
            if !apart(&at.p, reach, e) {
                calls += 1;
                if let Some((_, depth)) = deep(penetration(g, at, e)) {
                    if let Some(inf) = info.as_deref_mut() {
                        *inf = TouchInfo { kind: 1, owner: 0, index: i as i32, simulates: 3, depth };
                    }
                    return true;
                }
            }
            i += 1;
        }
        // a person is the world here: pressed on, the hand is held soft and drawn still, as on a wall (V's own body too,
        // while the hand meets it)
        let mut i = 0;
        while what >= TOUCH_PEOPLE && i < person_near_n && calls < 48 {
            if person_meets_hand(i, side) && !(person_owner[i] == SELF_OWNER && what < TOUCH_ALL) {
                let e = &person_near[i];
                if !apart(&at.p, reach, e) {
                    calls += 1;
                    if let Some((_, depth)) = deep(penetration(g, at, e)) {
                        if let Some(inf) = info.as_deref_mut() {
                            *inf = TouchInfo { kind: 2, owner: person_owner[i], index: i as i32, simulates: i32::from(person_simulates[i]), depth };
                        }
                        return true;
                    }
                }
            }
            i += 1;
        }
        // the car's cabin (cabin.rs) is the world too, seated
        let cabin_n = super::cabin::cabin_near_n;
        let mut i = 0;
        while i < cabin_n && calls < 56 {
            let e = &super::cabin::cabin_near[i];
            if !apart(&at.p, reach, e) {
                calls += 1;
                if let Some((_, depth)) = deep(penetration(g, at, e)) {
                    if let Some(inf) = info.as_deref_mut() {
                        *inf = TouchInfo { kind: 5, owner: 0, index: i as i32, simulates: 3, depth };
                    }
                    return true;
                }
            }
            i += 1;
        }
        let mut i = 0;
        while i < other_n && calls < 64 {
            let e = &OTHER[i];
            if !apart(&at.p, reach, e) {
                calls += 1;
                if let Some((_, depth)) = deep(penetration(g, at, e)) {
                    if let Some(inf) = info.as_deref_mut() {
                        *inf = TouchInfo { kind: 3, owner: 1 - side as i32, index: i as i32, simulates: 3, depth };
                    }
                    return true;
                }
            }
            i += 1;
        }
        false
    };
    if touches(gptr(&geometry), &pose, box_reach(&geometry), &mut info) {
        return true;
    }
    if let Some(bs) = body_shapes(side) {
        for i in 1..bs.n.min(BODY_SHAPES) {
            if bs.s[i].kind() != px::GEOM_CAPSULE {
                continue;
            }
            let mut g = bs.s[i].capsule();
            g.radius += 0.01;
            let (wq, wp) = compose(&at_pose.q, &at_pose.p, &bs.s[i].local.q, &bs.s[i].local.p);
            let w = PxTransform { q: wq, p: wp };
            let reach = g.half_height + g.radius;
            let mut j = 0;
            while j < l.n && calls < 96 {
                let e = &l.s[j];
                if !apart(&w.p, reach, e) {
                    calls += 1;
                    if let Some((_, depth)) = deep(penetration(gptr(&g), &w, e)) {
                        if let Some(inf) = info.as_deref_mut() {
                            *inf = TouchInfo { kind: 4, owner: 0, index: j as i32, simulates: 3, depth };
                        }
                        return true;
                    }
                }
                j += 1;
            }
            let mut j = 0;
            while j < super::cabin::cabin_near_n && calls < 112 {
                let e = &super::cabin::cabin_near[j];
                if !apart(&w.p, reach, e) {
                    calls += 1;
                    if let Some((_, depth)) = deep(penetration(gptr(&g), &w, e)) {
                        if let Some(inf) = info.as_deref_mut() {
                            *inf = TouchInfo { kind: 5, owner: 0, index: j as i32, simulates: 3, depth };
                        }
                        return true;
                    }
                }
                j += 1;
            }
        }
    }
    // a gun's boxes meet nothing of the world (hand_filters): a gun in a wall is not the hand on it
    false
}

/// THE WAYS OUT OF WHAT THE HAND TOUCHES (physics thread): the palm's box grown by 1.5 cm and the fingers' and the
/// forearm's capsules by a centimetre, against the world and the people near (not the other hand: the two ride
/// together), each one's way out as PhysX gives it; ways within 25 degrees of one already found are the same face.
pub unsafe fn ways_out(side: usize, at_pose: &PxTransform, ways: &mut [[f32; 3]]) -> usize {
    let front = NEAR_FRONT.load(Acquire);
    let cap = ways.len();
    if front < 0 || px::penetration().is_none() || cap == 0 {
        return 0;
    }
    let l = &near_lists[front as usize];
    let mut n = 0usize;
    let mut calls = 0;
    let add = |d: &[f32; 3], ways: &mut [[f32; 3]], n: &mut usize| {
        let m = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if !(m > 1e-6) || *n >= cap {
            return;
        }
        let u = [d[0] / m, d[1] / m, d[2] / m];
        if ways[..*n].iter().any(|w| u[0] * w[0] + u[1] * w[1] + u[2] * w[2] > 0.906) {
            return;
        }
        ways[*n] = u;
        *n += 1;
    };
    let mut against = |g: *const c_void, p: &PxTransform, reach: f32, ways: &mut [[f32; 3]], n: &mut usize| {
        let mut i = 0;
        while i < l.n && calls < 64 && *n < cap {
            let e = &l.s[i];
            if !apart(&p.p, reach, e) {
                calls += 1;
                if let Some((dir, _)) = deep(penetration(g, p, e)) {
                    add(&dir, ways, n);
                }
            }
            i += 1;
        }
        let mut i = 0;
        while i < person_near_n && calls < 80 && *n < cap {
            if person_meets_hand(i, side) && person_owner[i] != SELF_OWNER {
                let e = &person_near[i];
                if !apart(&p.p, reach, e) {
                    calls += 1;
                    if let Some((dir, _)) = deep(penetration(g, p, e)) {
                        add(&dir, ways, n);
                    }
                }
            }
            i += 1;
        }
    };
    {
        let geometry = grown_box(side, 0.015);
        let pose = palm_pose(side, at_pose);
        against(gptr(&geometry), &pose, box_reach(&geometry), ways, &mut n);
    }
    if let Some(bs) = body_shapes(side) {
        for i in 1..bs.n.min(BODY_SHAPES) {
            if n >= cap {
                break;
            }
            if bs.s[i].kind() != px::GEOM_CAPSULE {
                continue;
            }
            let mut g = bs.s[i].capsule();
            g.radius += 0.01;
            let (wq, wp) = compose(&at_pose.q, &at_pose.p, &bs.s[i].local.q, &bs.s[i].local.p);
            let w = PxTransform { q: wq, p: wp };
            against(gptr(&g), &w, g.half_height + g.radius, ways, &mut n);
        }
    }
    n
}

/// HoldFeature: only current static geometry may establish a climb hold. The worker bends drawn fingers on people and
/// loose things too; its curl and its animation hysteresis are not contact evidence.
pub unsafe fn hold_feature(side: usize, at_pose: &PxTransform, origin: &[f32; 3]) -> Feature {
    let front = NEAR_FRONT.load(Acquire);
    if front < 0 || px::penetration().is_none() {
        return Feature::None;
    }
    let l = &near_lists[front as usize];
    if l.origin != *origin || tick_ms().wrapping_sub(l.at) >= 5000 {
        return Feature::None;
    }
    let mut hits: Vec<Hit> = Vec::with_capacity(MAX_HITS);
    let mut calls = 0;
    // Reserve 16 of the pre-existing 160 queries for verifying seams and rail tops. The grown probe's extreme along the
    // MTD, translated out by its depth, locates the static plane.
    let mut probe = |g: *const c_void, pose: &PxTransform, reach: f32, extent: &dyn Fn(&[f32; 3]) -> f32, close_depth: &dyn Fn(&[f32; 3]) -> f32, hits: &mut Vec<Hit>| {
        let mut i = 0;
        while i < l.n && calls < 144 && hits.len() < MAX_HITS {
            let e = &l.s[i];
            if !apart(&pose.p, reach, e) {
                calls += 1;
                if let Some((dir, depth)) = penetration(g, pose, e) {
                    if depth > 0.0 && depth.is_finite() {
                        let n = Vector::new(dir[0], dir[1], dir[2]);
                        let n2 = length_squared(n);
                        if n2 > 0.99 && n2 < 1.01 {
                            let p = Vector::new(pose.p[0], pose.p[1], pose.p[2]) + n * (depth - extent(&dir));
                            if cvr_physhands::handspace::finite(p) {
                                hits.push(Hit { point: p, normal: n, shape: i as i32, close: depth > close_depth(&dir) });
                            }
                        }
                    }
                }
            }
            i += 1;
        }
    };
    {
        let geometry = grown_box(side, 0.02);
        let pose = palm_pose(side, at_pose);
        let axes: [[f32; 3]; 3] = core::array::from_fn(|a| qrot(&at_pose.q, &[f32::from(u8::from(a == 0)), f32::from(u8::from(a == 1)), f32::from(u8::from(a == 2))]));
        let projected = |a: usize, n: &[f32; 3]| (axes[a][0] * n[0] + axes[a][1] * n[1] + axes[a][2] * n[2]).abs();
        let half = geometry.half;
        let extent = |n: &[f32; 3]| projected(0, n) * half[0] + projected(1, n) * half[1] + projected(2, n) * half[2];
        // A hold must actually touch its support at the old 1.5 cm palm / 1 cm finger reach, not just touch a different
        // wall with the forearm while reaching towards a top.
        let close_depth = |n: &[f32; 3]| 0.005 * (projected(0, n) + projected(1, n) + projected(2, n));
        probe(gptr(&geometry), &pose, box_reach(&geometry), &extent, &close_depth, &mut hits);
    }
    if let Some(bs) = body_shapes(side) {
        for i in 0..bs.n.min(BODY_SHAPES) {
            if bs.s[i].part != 1 {
                continue;
            }
            let mut g = bs.s[i].capsule();
            if g.kind != px::GEOM_CAPSULE {
                continue;
            }
            g.radius += 0.012;
            let (wq, wp) = compose(&at_pose.q, &at_pose.p, &bs.s[i].local.q, &bs.s[i].local.p);
            let w = PxTransform { q: wq, p: wp };
            let ax = qrot(&w.q, &[1.0, 0.0, 0.0]);
            let (radius, half) = (g.radius, g.half_height);
            let extent = move |n: &[f32; 3]| radius + (n[0] * ax[0] + n[1] * ax[1] + n[2] * ax[2]).abs() * half;
            probe(gptr(&g), &w, g.half_height + g.radius, &extent, &|_| 0.002, &mut hits);
        }
    }
    drop(probe);
    let seam_limit = calls + 14;
    let mut calls_seam = calls;
    climb_support::classify(
        &hits,
        |a, b, seam| {
            if calls_seam + 2 > seam_limit {
                return false;
            }
            let ball = SphereGeometry { kind: px::GEOM_SPHERE, radius: climb_support::SEAM_RADIUS };
            let p = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: seam.to_array() };
            for index in [a, b] {
                let e = &l.s[index as usize];
                if (0..3).any(|k| p.p[k] + ball.radius < e.bounds.lo[k] || p.p[k] - ball.radius > e.bounds.hi[k]) {
                    return false;
                }
            }
            for index in [a, b] {
                calls_seam += 1;
                match penetration(gptr(&ball), &p, &l.s[index as usize]) {
                    Some((_, depth)) if depth > 0.0 && depth.is_finite() => {}
                    _ => return false,
                }
            }
            true
        },
        |shape, center| {
            let ball = SphereGeometry { kind: px::GEOM_SPHERE, radius: climb_support::CAP_RADIUS };
            let p = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: center.to_array() };
            // A pad outside a top gets a shallow, upward MTD. A pad buried inside a solid or simply crossing a vertical
            // wall does not demonstrate top support.
            penetration(gptr(&ball), &p, &l.s[shape as usize]).is_some_and(|(d, depth)| depth > 0.0 && depth <= ball.radius && d[2] >= 0.65)
        },
    )
}

/// PersonAt: the part of a person a hand closes on (physics thread): of the people's capsules the palm's box grown by 2
/// cm or a finger's capsule grown by 1 cm is in, the one it is deepest in, of a part the script named (owner > 0).
pub unsafe fn person_at(side: usize, at_pose: &PxTransform) -> Option<usize> {
    let mut index: Option<usize> = None;
    let mut best = 0.0f32;
    let mut probe = |g: *const c_void, pose: &PxTransform, reach: f32| {
        for i in 0..person_near_n {
            if person_owner[i] <= 0 {
                continue;
            }
            let e = &person_near[i];
            if apart(&pose.p, reach, e) {
                continue;
            }
            if let Some((_, depth)) = penetration(g, pose, e) {
                if depth > best {
                    best = depth;
                    index = Some(i);
                }
            }
        }
    };
    {
        let geometry = grown_box(side, 0.02);
        probe(gptr(&geometry), &palm_pose(side, at_pose), box_reach(&geometry));
    }
    if let Some(bs) = body_shapes(side) {
        for i in 0..bs.n.min(BODY_SHAPES) {
            if bs.s[i].part != 1 {
                continue;
            }
            let mut g = bs.s[i].capsule();
            if g.kind != px::GEOM_CAPSULE {
                continue;
            }
            g.radius += 0.01;
            let (wq, wp) = compose(&at_pose.q, &at_pose.p, &bs.s[i].local.q, &bs.s[i].local.p);
            probe(gptr(&g), &PxTransform { q: wq, p: wp }, g.half_height + g.radius);
        }
    }
    index
}
/// PersonPose: where a person's part is this step (physics thread), by the script's id for it.
pub unsafe fn person_pose(owner: i32) -> Option<PxTransform> {
    (0..person_near_n).find(|&i| person_owner[i] == owner).map(|i| person_near[i].pose)
}
/// InSelf: a hand in V's own body (physics thread) -- its palm's box or a finger's capsule within 5 mm of a part that
/// hand would meet.
pub unsafe fn in_self(side: usize) -> bool {
    if actor[side].is_null() {
        return false;
    }
    let at_pose = global_pose(actor[side]);
    let shapes = body_shapes(side);
    for i in 0..person_near_n {
        if person_owner[i] != SELF_OWNER || (person_meets[i] >> side) & 1 == 0 {
            continue;
        }
        let e = &person_near[i];
        let geometry = grown_box(side, 0.005);
        if deep(penetration(gptr(&geometry), &palm_pose(side, &at_pose), e)).is_some() {
            return true;
        }
        if let Some(bs) = &shapes {
            for j in 0..bs.n.min(BODY_SHAPES) {
                if bs.s[j].part != 1 {
                    continue;
                }
                let mut g = bs.s[j].capsule();
                if g.kind != px::GEOM_CAPSULE {
                    continue;
                }
                g.radius += 0.005;
                let (wq, wp) = compose(&at_pose.q, &at_pose.p, &bs.s[j].local.q, &bs.s[j].local.p);
                if deep(penetration(gptr(&g), &PxTransform { q: wq, p: wp }, e)).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

static mut APART_SCRATCH: [BodyShapes; 2] = [BODY_SHAPES0; 2];
static mut APART_A: [NearShape; BODY_SHAPES] = [NearShape::ZERO; BODY_SHAPES];
static mut APART_B: [NearShape; BODY_SHAPES] = [NearShape::ZERO; BODY_SHAPES];
/// HandsApart: the two hands apart (physics thread) -- no shape of one within 1.5 cm of the other's (the palms, the
/// fingers, the forearms, a gun's boxes, which reach 30 cm and more past the palm), and the palms' middles 12 cm apart.
/// Without the shapes to go by, the palms 26 cm apart.
pub unsafe fn hands_apart() -> bool {
    if actor[0].is_null() || actor[1].is_null() {
        return false;
    }
    let mut c = [[0.0f32; 3]; 2];
    let mut pose = [[0.0f32; 7]; 2];
    for side in 0..2 {
        let at_pose = global_pose(actor[side]);
        let r = qrot(&at_pose.q, &made_centre[side]);
        for k in 0..3 {
            c[side][k] = at_pose.p[k] + r[k];
        }
        pose[side][..4].copy_from_slice(&at_pose.q);
        pose[side][4..].copy_from_slice(&at_pose.p);
    }
    let d2: f32 = (0..3).map(|k| (c[0][k] - c[1][k]) * (c[0][k] - c[1][k])).sum();
    let na = other_hand_shapes(0, &pose[0], &mut APART_A, &mut APART_SCRATCH[0], -1);
    let nb = other_hand_shapes(1, &pose[1], &mut APART_B, &mut APART_SCRATCH[1], -1);
    if na == 0 || nb == 0 {
        return d2 > 0.26 * 0.26;
    }
    if !(d2 > 0.12 * 0.12) {
        return false;
    }
    const CLEAR: f32 = 0.015;
    for i in 0..na {
        let mut g = [0u8; 16];
        g.copy_from_slice(&APART_A[i].geometry.0[..16]);
        let kind = i32::from_ne_bytes([g[0], g[1], g[2], g[3]]);
        let mut f: [f32; 3] = core::ptr::read_unaligned(g.as_ptr().add(4).cast());
        if kind == px::GEOM_BOX {
            f[0] += CLEAR;
            f[1] += CLEAR;
            f[2] += CLEAR;
        } else if kind == px::GEOM_CAPSULE {
            f[0] += CLEAR;
        } else {
            continue;
        }
        core::ptr::write_unaligned(g.as_mut_ptr().add(4).cast(), f);
        #[repr(C, align(8))]
        struct G([u8; 16]);
        let geometry = G(g);
        for j in 0..nb {
            let (a, b) = (&APART_A[i].bounds, &APART_B[j].bounds);
            if (0..3).any(|k| a.hi[k] + CLEAR < b.lo[k] || a.lo[k] - CLEAR > b.hi[k]) {
                continue;
            }
            if deep(penetration(geometry.0.as_ptr().cast(), &APART_A[i].pose, &APART_B[j])).is_some() {
                return false;
            }
        }
    }
    true
}

/// OtherHandShapes: THE OTHER HAND for a hand's fingers (the worker) -- its palm's box, its fingers' and its forearm's
/// capsules and its gun's boxes, as its body was posed after the step (`pose`, PhysX's space). Without the forearm a
/// hand went through the other arm; without the gun the free hand could not lie on it.
pub fn other_hand_shapes(side: usize, pose: &[f32; 7], out: &mut [NearShape], scratch: &mut BodyShapes, part_only: i32) -> usize {
    let Some(world_bounds) = world_bounds_fn() else { return 0 };
    let Some(bs) = BODY_SHAPES_SEQ[side].read() else { return 0 };
    if tick_ms().wrapping_sub(bs.at) > 200 {
        return 0;
    }
    *scratch = bs;
    let q = [pose[0], pose[1], pose[2], pose[3]];
    let p = [pose[4], pose[5], pose[6]];
    let mut n = 0usize;
    for b in scratch.s.iter().take(scratch.n.min(BODY_SHAPES)) {
        if n >= out.len() {
            break;
        }
        if part_only >= 0 && i32::from(b.part) != part_only {
            continue;
        }
        let e = &mut out[n];
        n += 1;
        e.geometry.0 = [0; 64];
        e.geometry.0[..16].copy_from_slice(&b.geometry);
        e.kind = b.kind();
        let (oq, op) = compose(&q, &p, &b.local.q, &b.local.p);
        e.pose = PxTransform { q: oq, p: op };
        // SAFETY: PhysX3Common's geometry query on a copied geometry.
        e.bounds = unsafe { world_bounds(e.geometry_ptr(), &e.pose, 1.0) };
    }
    n
}

static mut OTHER_OUT_SCRATCH: BodyShapes = BODY_SHAPES0;
static mut OTHER_OUT: [NearShape; BODY_SHAPES] = [NearShape::ZERO; BODY_SHAPES];
/// OtherHandOut: THE WAYS OUT OF THE OTHER HAND for this one (physics thread) -- of this hand's palm box and its
/// fingers' capsules, grown by 4 and 2 cm, against the other hand's palm and fingers, each pair's way out and how far
/// the shapes still are from touching along it (4 floats each: the way, then the room). Every pair, not the deepest's
/// alone. Grown, so a hand sent at the other from 2 cm is let on those 2 cm and no more (2026-11-10).
pub unsafe fn other_hand_out(side: usize, at_pose: &PxTransform, out: &mut [[f32; 4]]) -> usize {
    if !hands_meet_on || actor[1 - side].is_null() {
        return 0;
    }
    let op = global_pose(actor[1 - side]);
    let pose = [op.q[0], op.q[1], op.q[2], op.q[3], op.p[0], op.p[1], op.p[2]];
    let n = other_hand_shapes(1 - side, &pose, &mut OTHER_OUT, &mut OTHER_OUT_SCRATCH, -1);
    if n == 0 {
        return 0;
    }
    let most = out.len();
    let mut found = 0usize;
    let mut probe = |g: *const c_void, w: &PxTransform, grown: f32, out: &mut [[f32; 4]]| {
        for i in 0..n {
            if found >= most {
                break;
            }
            if let Some((dir, depth)) = penetration(g, w, &OTHER_OUT[i]) {
                if depth > 0.0 && (dir[0] + dir[1] + dir[2]).is_finite() {
                    out[found] = [dir[0], dir[1], dir[2], (grown - depth).max(0.0)];
                    found += 1;
                }
            }
        }
    };
    {
        const GROWN: f32 = 0.04;
        let geometry = grown_box(side, GROWN);
        probe(gptr(&geometry), &palm_pose(side, at_pose), GROWN, out);
    }
    if let Some(mine) = body_shapes(side) {
        for i in 0..mine.n.min(BODY_SHAPES) {
            if mine.s[i].part != 1 {
                continue;
            }
            let mut g = mine.s[i].capsule();
            if g.kind != px::GEOM_CAPSULE {
                continue;
            }
            g.radius += 0.02;
            let (wq, wp) = compose(&at_pose.q, &at_pose.p, &mine.s[i].local.q, &mine.s[i].local.p);
            probe(gptr(&g), &PxTransform { q: wq, p: wp }, 0.02, out);
        }
    }
    found
}

static mut GUN_SCRATCH: BodyShapes = BODY_SHAPES0;
static mut GUN: [NearShape; BODY_SHAPES] = [NearShape::ZERO; BODY_SHAPES];
/// OtherInGun: THE OTHER HAND IN THIS HAND'S GUN (physics thread) -- its palm box and its fingers' capsules, grown by 1
/// cm, against the gun's boxes as posed last step. The gun's collision with it is taken up again only once this is
/// false: given the boxes while in them, PhysX throws the hand out.
pub unsafe fn other_in_gun(side: usize) -> bool {
    let other = 1 - side;
    if actor[side].is_null() || actor[other].is_null() {
        return false;
    }
    let gp = global_pose(actor[side]);
    let op = global_pose(actor[other]);
    let pose = [gp.q[0], gp.q[1], gp.q[2], gp.q[3], gp.p[0], gp.p[1], gp.p[2]];
    let n = other_hand_shapes(side, &pose, &mut GUN, &mut GUN_SCRATCH, 3);
    if n == 0 {
        return false;
    }
    let probe = |g: *const c_void, w: &PxTransform| (0..n).any(|i| deep(penetration(g, w, &GUN[i])).is_some());
    {
        let geometry = grown_box(other, 0.01);
        if probe(gptr(&geometry), &palm_pose(other, &op)) {
            return true;
        }
    }
    if let Some(mine) = body_shapes(other) {
        for i in 0..mine.n.min(BODY_SHAPES) {
            if mine.s[i].part != 1 {
                continue;
            }
            let mut g = mine.s[i].capsule();
            if g.kind != px::GEOM_CAPSULE {
                continue;
            }
            g.radius += 0.01;
            let (wq, wp) = compose(&op.q, &op.p, &mine.s[i].local.q, &mine.s[i].local.p);
            if probe(gptr(&g), &PxTransform { q: wq, p: wp }) {
                return true;
            }
        }
    }
    false
}

/// PathFree: nothing static near the player on the straight line from a to b (PhysX's space): a ball walked along it.
/// Physics thread, like touches_world. None when there is no list to ask; Some(Err(what was met)) when it is not free.
pub unsafe fn path_free(a: &[f32; 3], b: &[f32; 3], radius: f32) -> Option<Result<(), String>> {
    let front = NEAR_FRONT.load(Acquire);
    if front < 0 || px::penetration().is_none() {
        return None;
    }
    let l = &near_lists[front as usize];
    let ball = SphereGeometry { kind: px::GEOM_SPHERE, radius };
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let n = ((len / radius.max(0.01)) as i32 + 1).clamp(2, 96);
    for i in 0..=n {
        let mut at_pose = PxTransform::default();
        for k in 0..3 {
            at_pose.p[k] = a[k] + d[k] * (i as f32 / n as f32);
        }
        // the world near and, seated, the car's cabin (a hand stopped on the dashboard was put back beyond it)
        let cabin = &super::cabin::cabin_near[..super::cabin::cabin_near_n];
        for e in l.s[..l.n].iter().chain(cabin.iter()) {
            if apart(&at_pose.p, ball.radius, e) {
                continue;
            }
            if let Some((_, depth)) = deep(penetration(gptr(&ball), &at_pose, e)) {
                return Some(Err(format!(
                    "at {}/{} of the line a shape of type {}, {:.1} cm in, its bounds {:.2} {:.2} {:.2} .. {:.2} {:.2} {:.2}",
                    i,
                    n,
                    e.kind,
                    100.0 * depth,
                    e.bounds.lo[0],
                    e.bounds.lo[1],
                    e.bounds.lo[2],
                    e.bounds.hi[0],
                    e.bounds.hi[1],
                    e.bounds.hi[2]
                )));
            }
        }
    }
    Some(Ok(()))
}
