//! THE FINGERS (worker). Each step the worker solves every finger of a free hand against the world near (the walked
//! list, the people near and the loose things by the hands, V's own body while a hand meets it, the other hand while the
//! two meet), eases the bends and publishes them: `a` as drawn (yielding, then wrapped round what the palm is on), `y`
//! only what the fingers yielded, for their capsules on the body. Wrapped capsules pressed into the table they curled
//! toward and lifted the hand off it: the fingers then found nothing, hung further and lifted it more.
use super::body::{made_centre, made_half, LIVE, HANDS_MEET_SHOWN, SELF_ON_SHOWN};
use super::loose::{penetration, LooseShapesNear, LOOSE_SHAPES_NEAR};
use super::math::{axis_angle, compose, cross, qmul, qnorm, qrot};
use super::near::{near_lists, NearList, NearShape, MOST_NEAR, NEAR_FRONT, NEAR_LOCK};
use super::people::{PeopleNear, MOST_BODIES, PEOPLE_NEAR, SELF_OWNER};
use super::px::{self, BoxGeometry, CapsuleGeometry, PxTransform};
use super::seq::Seq;
use super::settings::Settings;
use super::touch::{other_hand_shapes, BodyShapes, BODY_SHAPES, BODY_SHAPES0};
use super::types::{FingerChain, FingerHand, Step};
use super::{qpc, tick_ms};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use cvr_host::log;

pub static FINGER_HANDS: [Seq<FingerHand>; 2] = [Seq::new(FINGER_HAND0), Seq::new(FINGER_HAND0)];
const CHAIN0: FingerChain = FingerChain { base: [0.0; 7], local: [[0.0; 7]; 3], tip: [0.0; 3], hinge: [[0.0; 3]; 3], lo: [0.0; 3], hi: [0.0; 3], flex: [0.0; 3], radius: [0.0; 3], offset: [[0.0; 3]; 3], moves: [0; 3] };
const FINGER_HAND0: FingerHand = FingerHand { f: [CHAIN0; 5], free: false, at: 0 };

/// A CRITICALLY DAMPED FOLLOWER: x chases `goal` with velocity v, rate w (1/s), solved exactly over dt (no overshoot, no
/// jump in velocity, stable at any step). The speed it reaches is held to `most`.
pub fn damped(x: &mut f32, v: &mut f32, goal: f32, w: f32, dt: f32, most: f32) {
    let e = *x - goal;
    let k = *v + w * e;
    let ex = (-w * dt).exp();
    *x = goal + (e + k * dt) * ex;
    *v = (*v - w * k * dt) * ex;
    *v = v.clamp(-most, most);
}

#[derive(Clone, Copy, Default)]
pub struct Bends {
    pub a: [[f32; 3]; 5],
    pub y: [[f32; 3]; 5],
    pub flex: [[f32; 3]; 5],
    pub on: bool,
    pub at: u64,
    pub scanner_epoch: u64,
}
const BENDS0: Bends = Bends { a: [[0.0; 3]; 5], y: [[0.0; 3]; 5], flex: [[0.0; 3]; 5], on: false, at: 0, scanner_epoch: 0 };
pub static BENDS: [Seq<Bends>; 2] = [Seq::new(BENDS0), Seq::new(BENDS0)];
pub static FINGER_CALLS: AtomicU64 = AtomicU64::new(0);
pub static FINGER_CONTACTS: AtomicU64 = AtomicU64::new(0);
pub static FINGER_US: AtomicU64 = AtomicU64::new(0);
pub static FINGER_SOLVES: AtomicU64 = AtomicU64::new(0);
/// solves posed down onto the palm; solves with the palm on something
pub static PALM_SHIFTED: AtomicU64 = AtomicU64::new(0);
pub static PALM_WRAPS: AtomicU64 = AtomicU64::new(0);
/// eased fingers found in something and put out of it at once...
pub static FINGERS_OUT: AtomicU64 = AtomicU64::new(0);
/// ...and out toward the open hand, the solve being in it too
pub static FINGERS_OPENED: AtomicU64 = AtomicU64::new(0);
/// solves with the back of the hand pressed on something
pub static BACK_PRESSED: AtomicU64 = AtomicU64::new(0);
/// worker: the hand holds on (climb), so its thumb lies on what it holds too
pub static HOLDING_ON: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
const Z3: AtomicU32 = AtomicU32::new(0);
const Z3X: [AtomicU32; 3] = [Z3; 3];
const Z15: [[AtomicU32; 3]; 5] = [Z3X; 5];
/// the eased bends last given, for the statistics (f32 bits)
pub static SHOWN_BENDS: [[[AtomicU32; 3]; 5]; 2] = [Z15; 2];
const Z5: [AtomicU32; 5] = [Z3; 5];
/// trace=1: each finger as drawn (b.a at the body), m deep in what is near (f32 bits)
pub static SHOWN_DEPTH: [[AtomicU32; 5]; 2] = [Z5; 2];

/// trace=1: the next solve_finger logs its finger's depth along the whole wrap (worker thread)
static mut wrap_probe: bool = false;
/// The skin's margin as the fingers' solve has it: a capsule grown by it may be that far in a thing with the skin itself
/// still out, and the checks for a finger left in something allow it.
static mut finger_slack: f32 = 0.002;

type Chain7 = [[f32; 7]; 3];
/// PoseChain: a finger's chain posed with joint turns b from the hand's frame (bq, bp in the world): each joint and the
/// tip (J), each bone's frame (F), each joint's axis in the world.
pub fn pose_chain(c: &FingerChain, bq: &[f32; 4], bp: &[f32; 3], b: &[f32; 3]) -> ([[f32; 3]; 4], Chain7, [[f32; 3]; 3]) {
    let mut j_ = [[0.0f32; 3]; 4];
    let mut f_ = [[0.0f32; 7]; 3];
    let mut axis = [[0.0f32; 3]; 3];
    let (mut pq, mut pp) = (*bq, *bp);
    for j in 0..3 {
        let turn = axis_angle(&c.hinge[j], b[j]);
        let lq = qmul(&[c.local[j][0], c.local[j][1], c.local[j][2], c.local[j][3]], &turn);
        let (q, p) = compose(&pq, &pp, &lq, &[c.local[j][4], c.local[j][5], c.local[j][6]]);
        f_[j][..4].copy_from_slice(&q);
        f_[j][4..].copy_from_slice(&p);
        pq = q;
        pp = p;
        j_[j] = p;
        axis[j] = qrot(&q, &c.hinge[j]);
    }
    let tip = qrot(&[f_[2][0], f_[2][1], f_[2][2], f_[2][3]], &c.tip);
    for k in 0..3 {
        j_[3][k] = f_[2][4 + k] + tip[k];
    }
    (j_, f_, axis)
}
/// Segments: each segment of a posed chain against what is near -- the deepest of all.
pub unsafe fn segments(c: &FingerChain, j0: &[[f32; 3]; 4], f_: Option<&Chain7>, l: &NearList, calls: &mut i32) -> f32 {
    let mut deepest = 0.0f32;
    for s in 0..3 {
        if c.radius[s] <= 0.0 {
            continue;
        }
        // the segment's line, off the bone to the skin's centre (hand_skin)
        let o = f_.map_or([0.0; 3], |f| qrot(&[f[s][0], f[s][1], f[s][2], f[s][3]], &c.offset[s]));
        let ja = [j0[s][0] + o[0], j0[s][1] + o[1], j0[s][2] + o[2]];
        let jb = [j0[s + 1][0] + o[0], j0[s + 1][1] + o[1], j0[s + 1][2] + o[2]];
        let mut d = [jb[0] - ja[0], jb[1] - ja[1], jb[2] - ja[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-4 {
            continue;
        }
        for v in &mut d {
            *v /= len;
        }
        let cap = CapsuleGeometry { kind: px::GEOM_CAPSULE, radius: c.radius[s], half_height: 0.5 * len };
        let mut pose = PxTransform::default();
        // the capsule's axis is its x: the shortest turn from x to the segment
        if d[0] < -0.9999 {
            pose.q = [0.0, 0.0, 1.0, 0.0];
        } else {
            pose.q = [0.0, -d[2], d[1], 1.0 + d[0]];
            qnorm(&mut pose.q);
        }
        let mut lo = [0.0f32; 3];
        let mut hi = [0.0f32; 3];
        for k in 0..3 {
            pose.p[k] = 0.5 * (ja[k] + jb[k]);
            lo[k] = ja[k].min(jb[k]) - c.radius[s];
            hi[k] = ja[k].max(jb[k]) + c.radius[s];
        }
        let mut i = 0;
        while i < l.n && *calls < 1400 {
            let e = &l.s[i];
            i += 1;
            if hi[0] < e.bounds.lo[0] || lo[0] > e.bounds.hi[0] || hi[1] < e.bounds.lo[1] || lo[1] > e.bounds.hi[1] || hi[2] < e.bounds.lo[2] || lo[2] > e.bounds.hi[2] {
                continue;
            }
            *calls += 1;
            let Some((_, depth)) = penetration((&cap as *const CapsuleGeometry).cast(), &pose, e) else { continue };
            if !(depth > 0.0005) || !depth.is_finite() {
                continue;
            }
            deepest = deepest.max(depth);
        }
    }
    deepest
}

/// THE OPEN HAND from the chain as the animation has it: each joint turned back about its hinge by the angle its
/// segment makes with the one before (the first with the palm's own line), within its range.
pub fn open_angles(c: &FingerChain, hq: &[f32; 4], hp: &[f32; 3], bq: &[f32; 4], bp: &[f32; 3]) -> [f32; 3] {
    let mut a = [0.0f32; 3];
    let (j_, _, axis) = pose_chain(c, bq, bp, &a);
    // the palm's own line: wrist to this finger's knuckle, laid in the plane of the palm's face (the box's thickness
    // axis, the hand frame's z, taken out): the wrist to knuckle line leans off it, so the fingers opened along it stood
    // off a wall the palm lay flat on and only their curled tips touched it (2026-11-09)
    let mut prev = [j_[0][0] - hp[0], j_[0][1] - hp[1], j_[0][2] - hp[2]];
    {
        let n = qrot(hq, &[0.0, 0.0, 1.0]);
        let d = prev[0] * n[0] + prev[1] * n[1] + prev[2] * n[2];
        for k in 0..3 {
            prev[k] -= d * n[k];
        }
    }
    for j in 0..3 {
        let seg = [j_[j + 1][0] - j_[j][0], j_[j + 1][1] - j_[j][1], j_[j + 1][2] - j_[j][2]];
        let cr = cross(&prev, &seg);
        let angle = (cr[0] * axis[j][0] + cr[1] * axis[j][1] + cr[2] * axis[j][2]).atan2(prev[0] * seg[0] + prev[1] * seg[1] + prev[2] * seg[2]);
        // only ever straightened: a joint the measure finds bent backward (the fingertip's point is not always on the
        // bone's line) is left as the animation has it rather than curled by that much
        if c.moves[j] != 0 && angle.is_finite() {
            a[j] = (-angle).min(0.0).clamp(c.lo[j], c.hi[j]);
        }
        prev = seg;
    }
    a
}

/// rad at the full wrap: knuckle, middle joint, last joint
const WRAP: [f32; 3] = [1.1, 1.5, 0.9];
/// how far a finger pressed on its pad bends back past straight at most (each within its anatomical range): the knuckle
/// and the last joint give, the middle joint hardly (its volar plate)
const BACK: [f32; 3] = [0.35, 0.10, 0.30];

/// SolveFinger: one finger against what is near -- (as drawn, only what it yielded, the open hand).
#[allow(clippy::too_many_arguments)]
unsafe fn solve_finger(c: &FingerChain, hq: &[f32; 4], hp: &[f32; 3], l: &NearList, wrap: bool, calls: &mut i32, contacts: &mut i32, squeeze: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (bq, bp) = compose(hq, hp, &[c.base[0], c.base[1], c.base[2], c.base[3]], &[c.base[4], c.base[5], c.base[6]]);
    let mut a = [0.0f32; 3];
    let open = open_angles(c, hq, hp, &bq, &bp);
    let opened = open;
    let mut depth_of = |b: &[f32; 3], calls: &mut i32| {
        let (j_, f_, _) = pose_chain(c, &bq, &bp, b);
        segments(c, &j_, Some(&f_), l, calls)
    };
    let in_most = finger_slack + 0.0015;
    // A FINGER MOVES ON ONE CURL, never joint by joint (HIGGS's FingerAnimator: one value per finger, each joint on the
    // way between the open and the closed pose): from the open hand (straight) to the animation's pose, and past the
    // open hand bent back a little.
    let back = |v: f32| -> [f32; 3] { core::array::from_fn(|j| if c.moves[j] != 0 { c.lo[j].max(open[j] - BACK[j] * v) } else { 0.0 }) };
    let pressed_out = |calls: &mut i32, depth_of: &mut dyn FnMut(&[f32; 3], &mut i32) -> f32| -> [f32; 3] {
        // the open hand in it too: bent back by the least that has it out, or all the way
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let t = back(1.0);
        if depth_of(&t, calls) > in_most {
            return t;
        }
        for _ in 0..5 {
            let mid = 0.5 * (lo + hi);
            if depth_of(&back(mid), calls) > in_most {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        back(hi)
    };
    // ...and the other way, curled on from the animation's pose toward a fist by the least that has it out: a finger met
    // on its back (its nail's side) gives toward the palm, as a finger does (2026-11-10). None when no curl within the
    // joints' ranges does it.
    let curl_at = |v: f32| -> [f32; 3] { core::array::from_fn(|j| if c.moves[j] != 0 { (WRAP[j] * v).clamp(c.lo[j], c.hi[j]) } else { 0.0 }) };
    let curled_out = |calls: &mut i32, depth_of: &mut dyn FnMut(&[f32; 3], &mut i32) -> f32| -> Option<[f32; 3]> {
        let (mut lo, mut hi) = (0.0f32, -1.0f32);
        let mut v = 0.1f32;
        while v < 1.25 {
            if !(depth_of(&curl_at(v), calls) > in_most) {
                hi = v;
                break;
            }
            lo = v;
            v += 0.1;
        }
        if hi < 0.0 {
            return None;
        }
        for _ in 0..4 {
            let mid = 0.5 * (lo + hi);
            if depth_of(&curl_at(mid), calls) > in_most {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(curl_at(hi))
    };
    if wrap {
        a = open;
        if depth_of(&open, calls) > in_most {
            a = pressed_out(calls, &mut depth_of);
            *contacts += 1;
        }
    } else if depth_of(&[0.0; 3], calls) > in_most {
        *contacts += 1;
        if depth_of(&open, calls) > in_most {
            // the open hand in it too: bent back past straight (a fingertip pressed on its pad) or curled on (a finger met
            // on its back), whichever is out, and of both the one that turns the finger less
            let back_b = pressed_out(calls, &mut depth_of);
            let curl_b = curled_out(calls, &mut depth_of);
            let back_ok = !(depth_of(&back_b, calls) > in_most);
            let back_cost: f32 = back_b.iter().map(|v| v.abs()).sum();
            let curl = match curl_b {
                Some(cb) => !back_ok || cb.iter().map(|v| v.abs()).sum::<f32>() < back_cost,
                None => false,
            };
            a = if curl { curl_b.unwrap_or(back_b) } else { back_b };
        } else {
            // the most curl of the way from the open hand that is out of what is near (HIGGS keeps the least curled first
            // contact of its three probes): halved toward the animation's pose
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..6 {
                let mid = 0.5 * (lo + hi);
                let t: [f32; 3] = core::array::from_fn(|j| open[j] * (1.0 - mid));
                if depth_of(&t, calls) > in_most {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            for j in 0..3 {
                a[j] = open[j] * (1.0 - lo);
            }
        }
    }
    let yielded = a;
    if wrap {
        // THE WRAP'S PATH, knuckle first: over t 0..1 the knuckle alone (a straight finger tips onto a wall or a table it
        // lies on, so the hand lies flat), over 1..2 the middle and last joints (a finger whose straight reach meets
        // nothing, over an edge, curls on round it).
        let base = a;
        let bend_at = |t: f32| -> [f32; 3] {
            let w = [t.min(1.0), (t - 1.0).max(0.0), (t - 1.0).max(0.0)];
            core::array::from_fn(|j| if c.moves[j] != 0 { (base[j] + w[j] * WRAP[j]).clamp(c.lo[j], c.hi[j]) } else { base[j] })
        };
        let touches_at = |t: f32, calls: &mut i32| {
            let (j_, f_, _) = pose_chain(c, &bq, &bp, &bend_at(t));
            segments(c, &j_, Some(&f_), l, calls) > 0.0015
        };
        if wrap_probe {
            wrap_probe = false;
            let mut line = format!(
                "[physhands] right middle finger along its wrap from the open hand ({:.2} {:.2} {:.2}, hi {:.2} {:.2} {:.2}), mm in at t 0..2 (knuckle to 1):",
                a[0], a[1], a[2], c.hi[0], c.hi[1], c.hi[2]
            );
            for i in 0..=20 {
                if line.len() >= 512 {
                    break;
                }
                let (j_, f_, _) = pose_chain(c, &bq, &bp, &bend_at(0.1 * i as f32));
                let mut dummy = 0;
                line.push_str(&format!(" {:.1}", 1000.0 * segments(c, &j_, Some(&f_), l, &mut dummy)));
            }
            log!("{}\n", line);
        }
        if !touches_at(0.06, calls) {
            // THE FIRST TOUCH ON THE WAY IN, stepped for and then halved between the last step clear and the first that
            // touches. A finger that meets nothing however far it curls (past the edge of a step, over a drop) hangs: a
            // share of the wrap, as a relaxed finger droops over an edge. Past the wrap, on to the joints' anatomical
            // ends. Steps short enough that a fingertip moves less than its capsule's radius.
            const DRAPE: f32 = 0.35;
            const STEP: f32 = 0.03;
            const MOST: f32 = 2.0;
            let (mut lo, mut hi) = (0.06f32, -1.0f32);
            let mut t = 0.06 + STEP;
            while t < MOST + 0.5 * STEP {
                let at = t.min(MOST);
                if touches_at(at, calls) {
                    hi = at;
                    break;
                }
                lo = at;
                t += STEP;
            }
            if hi < 0.0 {
                // nothing met: the relaxed droop, all three joints a share of the wrap
                for j in 0..3 {
                    if c.moves[j] != 0 {
                        a[j] = (a[j] + DRAPE * WRAP[j]).clamp(c.lo[j], c.hi[j]);
                    }
                }
            } else {
                for _ in 0..4 {
                    let mid = 0.5 * (lo + hi);
                    if touches_at(mid, calls) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                a = bend_at(lo);
            }
        }
        if squeeze > 0.0 {
            // THE GRIP SHOWS: closed on what the palm is on, the fingers press on -- on a flat face the fingertips dig in
            // (the knuckle lifts a little, the middle and last joints bend), round an edge they curl tighter
            const SQUEEZE: [f32; 3] = [-0.2, 0.6, 0.35];
            for j in 0..3 {
                if c.moves[j] != 0 {
                    a[j] = (a[j] + SQUEEZE[j] * squeeze).clamp(c.lo[j], c.hi[j]);
                }
            }
        }
        // THE LAST WORD: a finger the wrap left in something is turned back toward the open hand (laid in the palm's
        // plane, clear of what the palm is on) by the least of the way that has it out; or, the open hand in it too,
        // toward the animation's own pose
        let deep = depth_of(&a, calls);
        if deep > in_most {
            let zero = [0.0f32; 3];
            let open_deep = depth_of(&open, calls);
            let mut to: Option<[f32; 3]> = (open_deep < deep).then_some(open);
            if open_deep > in_most && depth_of(&zero, calls) < deep.min(open_deep) {
                to = Some(zero);
            }
            if let Some(to) = to {
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..6 {
                    let mid = 0.5 * (lo + hi);
                    let b: [f32; 3] = core::array::from_fn(|j| a[j] + (to[j] - a[j]) * mid);
                    if depth_of(&b, calls) > in_most {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                for j in 0..3 {
                    a[j] += (to[j] - a[j]) * hi;
                }
            }
        }
    }
    (a, yielded, opened)
}

fn palm_box(side: usize, grow: f32, hq: &[f32; 4], hp: &[f32; 3]) -> (BoxGeometry, PxTransform, f32) {
    // SAFETY: the palm's box as made (physics thread writes it once a body is made or refitted).
    let (half, centre) = unsafe { (made_half[side], made_centre[side]) };
    let g = BoxGeometry { kind: px::GEOM_BOX, half: [half[0] + grow, half[1] + grow, half[2] + grow] };
    let r = qrot(hq, &centre);
    let pose = PxTransform { q: *hq, p: [hp[0] + r[0], hp[1] + r[1], hp[2] + r[2]] };
    let reach = (g.half[0] * g.half[0] + g.half[1] * g.half[1] + g.half[2] * g.half[2]).sqrt();
    (g, pose, reach)
}
/// PalmDepth: how deep the palm's own box is in what is near, the hand at (hq, hp).
unsafe fn palm_depth(side: usize, hq: &[f32; 4], hp: &[f32; 3], l: &NearList, calls: &mut i32) -> f32 {
    let (g, pose, reach) = palm_box(side, 0.0, hq, hp);
    let mut deepest = 0.0f32;
    let mut i = 0;
    while i < l.n && *calls < 1400 {
        let e = &l.s[i];
        i += 1;
        if (0..3).any(|k| pose.p[k] + reach < e.bounds.lo[k] || pose.p[k] - reach > e.bounds.hi[k]) {
            continue;
        }
        *calls += 1;
        if let Some((_, depth)) = penetration((&g as *const BoxGeometry).cast(), &pose, e) {
            if depth.is_finite() {
                deepest = deepest.max(depth);
            }
        }
    }
    deepest
}
/// THE PALM ON SOMETHING, on its own side: the palm's box grown by a centimetre in a shape near, the way out of it
/// backward against the palm's facing (the fingers' curl). The back of the hand on a wall is not the palm on it. The palm
/// moved on a centimetre and a half the way it faces goes deeper into what is there; HELD OFF BY ITS FINGERTIPS, with
/// the body held by something, what is in front of the palm within 4.5 cm counts as the palm on it.
unsafe fn palm_on(side: usize, hq: &[f32; 4], hp: &[f32; 3], facing: &[f32; 3], l: &NearList, calls: &mut i32, held: bool) -> bool {
    {
        let here = palm_depth(side, hq, hp, l, calls);
        let reach = [0.015f32, 0.03, 0.045];
        for &r in reach.iter().take(if held { 3 } else { 1 }) {
            let on = [hp[0] + facing[0] * r, hp[1] + facing[1] * r, hp[2] + facing[2] * r];
            if palm_depth(side, hq, &on, l, calls) > here + 0.004 {
                return true;
            }
        }
    }
    let (g, pose, reach) = palm_box(side, 0.01, hq, hp);
    let mut i = 0;
    while i < l.n && *calls < 1400 {
        let e = &l.s[i];
        i += 1;
        if (0..3).any(|k| pose.p[k] + reach < e.bounds.lo[k] || pose.p[k] - reach > e.bounds.hi[k]) {
            continue;
        }
        *calls += 1;
        if let Some((dir, depth)) = penetration((&g as *const BoxGeometry).cast(), &pose, e) {
            if depth > 0.0 && dir[0] * facing[0] + dir[1] * facing[1] + dir[2] * facing[2] < -0.4 {
                return true;
            }
        }
    }
    false
}

// ---- the worker's own statics
static mut SHOWN: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut SHOWN_YIELD: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut SHOWN_V: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut SHOWN_YIELD_V: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut SHOWN_FLEX: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
static mut SHOWN_FLEX_KNOWN: [bool; 2] = [false; 2];
static mut TOUCHED_AT: [u64; 2] = [0; 2];
static mut WRAP_UNTIL: [u64; 2] = [0; 2];
static mut WRAP_PROBE_AT: u64 = 0;
static mut PROBE_AT: u64 = 0;
static mut TOLD_AT: u64 = 0;
static mut MERGED: [NearList; 2] = [NearList::NEW, NearList::NEW];
static mut PN: PeopleNear = unsafe { core::mem::zeroed() };
static mut LN: LooseShapesNear = unsafe { core::mem::zeroed() };
static mut EXTRA: [NearShape; MOST_BODIES + BODY_SHAPES] = [NearShape::ZERO; MOST_BODIES + BODY_SHAPES];
static mut OTHER_SCRATCH: BodyShapes = BODY_SHAPES0;
static mut COPIED_FROM: [usize; 2] = [usize::MAX; 2];
static mut COPIED_AT: [u64; 2] = [0; 2];
static mut COPIED_N: [i64; 2] = [-1; 2];

fn chain_base(c: &FingerChain) -> ([f32; 4], [f32; 3]) {
    ([c.base[0], c.base[1], c.base[2], c.base[3]], [c.base[4], c.base[5], c.base[6]])
}

/// SolveFingers: both hands' fingers this step (the worker), eased and published.
pub unsafe fn solve_fingers(st: &Step, s: &Settings, dt_s: f64) {
    let (t0, freq) = qpc();
    let _shared = NEAR_LOCK.read().unwrap_or_else(|e| e.into_inner());
    let front = if s.fingers != 0 { NEAR_FRONT.load(Acquire) } else { -1 };
    let l0: Option<&NearList> = (front >= 0).then(|| &near_lists[front as usize]);
    // ...and the people near and the loose things by the hands, posed this step, V's own body while a hand meets it and
    // the other hand while the two meet: the world's list and theirs, one list a hand
    let mut lists: [Option<&NearList>; 2] = [l0, l0];
    if let Some(l0) = l0 {
        let now_t = tick_ms();
        let people_ok = match PEOPLE_NEAR.read() {
            Some(p) if p.n > 0 && now_t.wrapping_sub(p.at) < 300 => {
                PN = p;
                true
            }
            _ => false,
        };
        let loose_count = match LOOSE_SHAPES_NEAR.read() {
            Some(x) if x.n > 0 && now_t.wrapping_sub(x.at) < 300 => {
                LN = x;
                x.n
            }
            _ => 0,
        };
        for side in 0..2 {
            let mut ne = 0usize;
            if people_ok {
                for i in 0..PN.n.min(MOST_BODIES) {
                    if (PN.meets[i] >> side) & 1 != 0 && (PN.owner[i] != SELF_OWNER || SELF_ON_SHOWN[side].load(Relaxed)) {
                        EXTRA[ne] = PN.s[i];
                        ne += 1;
                    }
                }
            }
            if HANDS_MEET_SHOWN.load(Relaxed) && st.valid[1 - side] {
                ne += other_hand_shapes(1 - side, &st.pose[1 - side], &mut EXTRA[ne..ne + BODY_SHAPES], &mut OTHER_SCRATCH, -1);
            }
            if ne + loose_count == 0 {
                continue;
            }
            let m = &mut MERGED[side];
            let mut at = l0.n.min(MOST_NEAR.saturating_sub(ne + loose_count));
            // the world's part copied only when it is another list (a list is never written while it is the front one):
            // V's own body is always there, and the whole list a hand a step was 2 x 200 kB
            let from = l0 as *const NearList as usize;
            if COPIED_FROM[side] != from || COPIED_AT[side] != l0.at || COPIED_N[side] != at as i64 {
                m.s[..at].copy_from_slice(&l0.s[..at]);
                COPIED_FROM[side] = from;
                COPIED_AT[side] = l0.at;
                COPIED_N[side] = at as i64;
            }
            let nl = loose_count.min(MOST_NEAR - at);
            m.s[at..at + nl].copy_from_slice(&LN.s[..nl]);
            at += nl;
            let nx = ne.min(MOST_NEAR - at);
            m.s[at..at + nx].copy_from_slice(&EXTRA[..nx]);
            m.n = at + nx;
            m.center = l0.center;
            m.at = l0.at;
            lists[side] = Some(&MERGED[side]);
        }
    }
    let (mut total_calls, mut contacts) = (0i32, 0i32);
    finger_slack = s.skin_margin;
    let dt = (dt_s.clamp(1e-4, 0.1)) as f32;
    for side in 0..2 {
        let l = lists[side];
        // A BUDGET OF QUERIES PER HAND, AND ONE OF ITS OWN FOR PUTTING THE EASED FINGERS OUT. A query past the budget
        // answers "clear" (segments, palm_depth: at most 1400 a call count).
        let (mut calls, mut put_calls) = (0i32, 0i32);
        let mut want = [[0.0f32; 3]; 5];
        let mut want_yield = [[0.0f32; 3]; 5];
        let mut want_open = [[0.0f32; 3]; 5];
        let hand = FINGER_HANDS[side].read();
        let h = hand.unwrap_or(FINGER_HAND0);
        let on = s.fingers != 0
            && px::penetration().is_some()
            && l.is_some()
            && st.valid[side]
            && LIVE[side].load(Acquire)
            && hand.is_some()
            && h.free
            && tick_ms().wrapping_sub(h.at) < 200
            && l.is_some_and(|l| tick_ms().wrapping_sub(l.at) < 5000);
        if on {
            let l = l.unwrap_or(&near_lists[0]);
            let hq = [st.pose[side][0], st.pose[side][1], st.pose[side][2], st.pose[side][3]];
            let mut hp = [st.pose[side][4], st.pose[side][5], st.pose[side][6]];
            // PRESSED, THE FINGERS GIVE: solved where the hand would be held by its palm alone -- moved on toward where
            // tracking sent it (at most 5 cm) until the palm's box meets what is there
            {
                let mut r = [st.sent[side][4] - hp[0], st.sent[side][5] - hp[1], st.sent[side][6] - hp[2]];
                let rn = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
                if rn > 0.002 {
                    let most = rn.min(0.05) / rn;
                    for v in &mut r {
                        *v *= most;
                    }
                    let palm_in = |t: f32, calls: &mut i32| {
                        let p = [hp[0] + r[0] * t, hp[1] + r[1] * t, hp[2] + r[2] * t];
                        palm_depth(side, &hq, &p, l, calls) > 0.0015
                    };
                    // the FIRST place on the way where the palm meets something: a mesh answers only where the box
                    // crosses its triangles
                    let (mut lo, mut hi) = (0.0f32, 1.0f32);
                    if !palm_in(0.0, &mut calls) {
                        const STEPS: i32 = 8;
                        let mut met = 0;
                        for i in 1..=STEPS {
                            if palm_in(i as f32 / STEPS as f32, &mut calls) {
                                met = i;
                                break;
                            }
                        }
                        if met == 0 {
                            lo = hi;
                        } else {
                            lo = (met - 1) as f32 / STEPS as f32;
                            hi = met as f32 / STEPS as f32;
                            for _ in 0..4 {
                                let mid = 0.5 * (lo + hi);
                                if palm_in(mid, &mut calls) {
                                    hi = mid;
                                } else {
                                    lo = mid;
                                }
                            }
                        }
                    }
                    for k in 0..3 {
                        hp[k] += r[k] * lo;
                    }
                    if lo > 0.0 {
                        PALM_SHIFTED.fetch_add(1, Relaxed);
                    }
                }
            }
            // THE PALM'S FACING: the plane of the palm (the wrist to the middle knuckle, the little knuckle to the index
            // knuckle), turned to the side a straight middle finger curls toward about its knuckle's hinge
            let mut wrap = false;
            let mut palm_facing = [0.0f32; 3];
            let mut have_facing = false;
            if s.conform != 0 {
                let which = [1usize, 0, 3]; // middle, index, little
                let mut jj = [[[0.0f32; 3]; 4]; 3];
                let mut axes = [[[0.0f32; 3]; 3]; 3];
                for i in 0..3 {
                    let c = &h.f[which[i]];
                    let (cq, cp) = chain_base(c);
                    let (bq, bp) = compose(&hq, &hp, &cq, &cp);
                    let (j_, _, axis) = pose_chain(c, &bq, &bp, &[0.0; 3]);
                    jj[i] = j_;
                    axes[i] = axis;
                }
                let along = [jj[0][0][0] - hp[0], jj[0][0][1] - hp[1], jj[0][0][2] - hp[2]];
                let across = [jj[1][0][0] - jj[2][0][0], jj[1][0][1] - jj[2][0][1], jj[1][0][2] - jj[2][0][2]];
                let mut facing = cross(&along, &across);
                let mv = cross(&axes[0][0], &along);
                let n = (facing[0] * facing[0] + facing[1] * facing[1] + facing[2] * facing[2]).sqrt();
                let d = facing[0] * mv[0] + facing[1] * mv[1] + facing[2] * mv[2];
                if n > 1e-6 && d.is_finite() {
                    let sign = if d < 0.0 { -1.0 } else { 1.0 };
                    for v in &mut facing {
                        *v *= sign / n;
                    }
                    palm_facing = facing;
                    have_facing = true;
                    // HELD A MOMENT: the touch flickers off for a step as a hand rests on a wall; the body's touch counts
                    // for 100 ms, and a palm found on something stays on it 150 ms
                    let now_t = tick_ms();
                    if st.touching[side] {
                        TOUCHED_AT[side] = now_t;
                    }
                    wrap = palm_on(side, &hq, &hp, &facing, l, &mut calls, now_t.wrapping_sub(TOUCHED_AT[side]) < 100);
                    if wrap {
                        WRAP_UNTIL[side] = now_t + 150;
                    } else {
                        wrap = now_t < WRAP_UNTIL[side];
                    }
                    if wrap {
                        PALM_WRAPS.fetch_add(1, Relaxed);
                    }
                }
            }
            let gripping = super::body::shared_hand(if side == 1 { 49 } else { 155 }).is_some_and(|v| v > 0.5);
            for f in 0..5 {
                if s.trace != 0 && side == 1 && f == 1 && wrap && tick_ms().wrapping_sub(WRAP_PROBE_AT) > 2000 {
                    WRAP_PROBE_AT = tick_ms();
                    wrap_probe = true;
                }
                // THE THUMB OF A HAND THAT HOLDS ON lies on what it holds as the fingers do (2026-11-16); otherwise it
                // stays as it is, on top of a railing the fingers wrap
                let thumb_wraps = f == 4 && HOLDING_ON[side].load(Relaxed);
                let (a, y, o) = solve_finger(&h.f[f], &hq, &hp, l, wrap && (f != 4 || thumb_wraps), &mut calls, &mut contacts, if gripping && wrap && f != 4 { 1.0 } else { 0.0 });
                want[f] = a;
                want_yield[f] = y;
                want_open[f] = o;
                wrap_probe = false;
            }
            // THE BACK OF THE HAND PRESSED ON SOMETHING (the knuckles on a wall, a table): the fingers straighten a little
            // with the press -- from 4 mm held off to 4.4 cm, up to 35% of the way to the open hand
            if have_facing && !wrap && st.on_world[side] {
                let r = [st.pose[side][4] - st.sent[side][4], st.pose[side][5] - st.sent[side][5], st.pose[side][6] - st.sent[side][6]];
                let rn = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
                let back = if rn > 1e-4 { (r[0] * palm_facing[0] + r[1] * palm_facing[1] + r[2] * palm_facing[2]) / rn } else { 0.0 };
                if back > 0.5 && rn > 0.004 {
                    let k = 0.35 * ((rn - 0.004) / 0.04).clamp(0.0, 1.0) * ((back - 0.5) / 0.3).clamp(0.0, 1.0);
                    for f in 0..5 {
                        for j in 0..3 {
                            if want_open[f][j] < 0.0 {
                                want[f][j] = want[f][j].min(want_open[f][j] * k);
                            }
                        }
                    }
                    BACK_PRESSED.fetch_add(1, Relaxed);
                }
            }
            // trace=1: every 2 s, how deep each finger of the right hand is in what is near, curled 0, half and all the
            // way from the animation's pose, and how far the hand was posed down onto its palm
            if s.trace != 0 && side == 1 && tick_ms().wrapping_sub(PROBE_AT) > 2000 {
                PROBE_AT = tick_ms();
                let mut d = [[0.0f32; 3]; 4];
                for f in 0..4 {
                    let c = &h.f[f];
                    let (cq, cp) = chain_base(c);
                    let (bq, bp) = compose(&hq, &hp, &cq, &cp);
                    for i in 0..3 {
                        let t = 0.5 * i as f32;
                        let b: [f32; 3] = core::array::from_fn(|j| if c.moves[j] != 0 { (t * WRAP[j]).clamp(c.lo[j], c.hi[j]) } else { 0.0 });
                        let (j_, f_, _) = pose_chain(c, &bq, &bp, &b);
                        let mut dummy = 0;
                        d[f][i] = 1000.0 * segments(c, &j_, Some(&f_), l, &mut dummy);
                    }
                }
                let moved: f32 = (0..3).map(|k| (hp[k] - st.pose[side][4 + k]) * (hp[k] - st.pose[side][4 + k])).sum();
                let mut dummy = 0;
                log!(
                    "[physhands] right fingers probe: posed down {:.1} mm, palm {:.1} mm in, wrap {}; mm in at curl 0/half/all: index {:.1}/{:.1}/{:.1}, middle {:.1}/{:.1}/{:.1}, ring {:.1}/{:.1}/{:.1}, little {:.1}/{:.1}/{:.1}; near shapes {}\n",
                    1000.0 * moved.sqrt(),
                    1000.0 * palm_depth(side, &hq, &hp, l, &mut dummy),
                    i32::from(wrap),
                    d[0][0], d[0][1], d[0][2], d[1][0], d[1][1], d[1][2], d[2][0], d[2][1], d[2][2], d[3][0], d[3][1], d[3][2],
                    l.n
                );
            }
        }
        // quick into a bend, slower out of it: a finger lifted off a table eases back
        let mut b = BENDS0;
        let mut bent = false;
        // THE ANIMATION OPENED UNDER A BEND THAT KEPT A FINGER OUT OF SOMETHING: the bend is the finger's turn from the
        // animation's pose, and what it held was the finger's angle; so it is the less at once by how far the animation
        // opened, never past straight the other way (2026-11-10).
        if on {
            for f in 0..5 {
                for j in 0..3 {
                    let now = h.f[f].flex[j];
                    let v = &mut SHOWN[side][f][j];
                    if SHOWN_FLEX_KNOWN[side] && *v < 0.0 {
                        let opened = SHOWN_FLEX[side][f][j] - now;
                        if opened > 0.0 {
                            *v = (*v + opened).min(0.0);
                            if *v == 0.0 {
                                SHOWN_V[side][f][j] = 0.0;
                            } else {
                                SHOWN_V[side][f][j] = SHOWN_V[side][f][j].max(0.0);
                            }
                        }
                    }
                    SHOWN_FLEX[side][f][j] = now;
                    b.flex[f][j] = now;
                }
            }
            SHOWN_FLEX_KNOWN[side] = true;
        } else {
            SHOWN_FLEX_KNOWN[side] = false;
            b.flex = SHOWN_FLEX[side];
        }
        for f in 0..5 {
            for j in 0..3 {
                // critically damped, quicker into a bend than out of it, at most 6 rad/s
                let tau = if want[f][j].abs() > SHOWN[side][f][j].abs() { 0.035 } else { 0.07 };
                damped(&mut SHOWN[side][f][j], &mut SHOWN_V[side][f][j], want[f][j], 1.0 / tau, dt, 6.0);
                if SHOWN[side][f][j].abs() < 1e-4 && want[f][j].abs() < 1e-4 {
                    SHOWN[side][f][j] = 0.0;
                    SHOWN_V[side][f][j] = 0.0;
                }
            }
            // THE EASED FINGER IN SOMETHING: out of it at once, by the least of the way to the solve that does it, eased
            // on from there
            if on {
                let l = l.unwrap_or(&near_lists[0]);
                let in_most = finger_slack + 0.0006;
                let c = &h.f[f];
                let (cq, cp) = chain_base(c);
                let sq = [st.pose[side][0], st.pose[side][1], st.pose[side][2], st.pose[side][3]];
                let sp = [st.pose[side][4], st.pose[side][5], st.pose[side][6]];
                let (bq, bp) = compose(&sq, &sp, &cq, &cp);
                let chain_depth = |a: &[f32; 3], calls: &mut i32| {
                    let (j_, f_, _) = pose_chain(c, &bq, &bp, a);
                    segments(c, &j_, Some(&f_), l, calls)
                };
                let depth_at = |t: f32, calls: &mut i32| {
                    let a: [f32; 3] = core::array::from_fn(|j| SHOWN[side][f][j] + (want[f][j] - SHOWN[side][f][j]) * t);
                    chain_depth(&a, calls)
                };
                let now = depth_at(0.0, &mut put_calls);
                if now > in_most {
                    if depth_at(1.0, &mut put_calls) < now {
                        let (mut lo, mut hi) = (0.0f32, 1.0f32);
                        for _ in 0..6 {
                            let mid = 0.5 * (lo + hi);
                            if depth_at(mid, &mut put_calls) > in_most {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        let _ = lo;
                        for j in 0..3 {
                            SHOWN[side][f][j] += (want[f][j] - SHOWN[side][f][j]) * hi;
                            SHOWN_V[side][f][j] = 0.0;
                        }
                        FINGERS_OUT.fetch_add(1, Relaxed);
                    }
                    // the solve in it too, here where the hand is (it was solved where the palm would be held): toward
                    // the open hand, which lies in the palm's plane
                    let left = depth_at(0.0, &mut put_calls);
                    if left > in_most {
                        want_open[f] = open_angles(c, &sq, &sp, &bq, &bp);
                        {
                            // the open hand in it as well: the animation's own pose, when that is clear of it
                            let open_in = chain_depth(&want_open[f], &mut put_calls);
                            if !(open_in < left) && open_in > finger_slack + 0.0015 && chain_depth(&[0.0; 3], &mut put_calls) < left {
                                want_open[f] = [0.0; 3];
                            }
                        }
                        let open_at = |t: f32, calls: &mut i32| {
                            let a: [f32; 3] = core::array::from_fn(|j| SHOWN[side][f][j] + (want_open[f][j] - SHOWN[side][f][j]) * t);
                            chain_depth(&a, calls)
                        };
                        if open_at(1.0, &mut put_calls) < left {
                            let (mut lo, mut hi) = (0.0f32, 1.0f32);
                            for _ in 0..6 {
                                let mid = 0.5 * (lo + hi);
                                if open_at(mid, &mut put_calls) > in_most {
                                    lo = mid;
                                } else {
                                    hi = mid;
                                }
                            }
                            let _ = lo;
                            for j in 0..3 {
                                SHOWN[side][f][j] += (want_open[f][j] - SHOWN[side][f][j]) * hi;
                                SHOWN_V[side][f][j] = 0.0;
                            }
                            FINGERS_OPENED.fetch_add(1, Relaxed);
                        }
                        // ...and the open hand in it as well (a wrist turned fingertips first toward a wall the palm is
                        // on): bent back past straight, as a finger pressed on its pad gives at the knuckle and the last
                        // joint, by the least that has it out
                        if depth_at(0.0, &mut put_calls) > in_most {
                            const BACK_MOST: [f32; 3] = [0.35, 0.10, 0.30];
                            let wo = want_open[f];
                            let back_at = |v: f32| -> [f32; 3] { core::array::from_fn(|j| if c.moves[j] != 0 { c.lo[j].max(wo[j] - BACK_MOST[j] * v) } else { 0.0 }) };
                            let now0 = depth_at(0.0, &mut put_calls);
                            if chain_depth(&back_at(1.0), &mut put_calls) < now0 {
                                let (mut lo2, mut hi2) = (0.0f32, 1.0f32);
                                if chain_depth(&back_at(1.0), &mut put_calls) <= in_most {
                                    for _ in 0..5 {
                                        let mid = 0.5 * (lo2 + hi2);
                                        if chain_depth(&back_at(mid), &mut put_calls) > in_most {
                                            lo2 = mid;
                                        } else {
                                            hi2 = mid;
                                        }
                                    }
                                }
                                let _ = lo2;
                                let bb = back_at(hi2);
                                for j in 0..3 {
                                    SHOWN[side][f][j] = bb[j];
                                    SHOWN_V[side][f][j] = 0.0;
                                }
                            }
                        }
                        // trace=1: a finger left in something, and why (rate-limited)
                        let after = depth_at(0.0, &mut put_calls);
                        if s.trace != 0 && after > 0.002 && tick_ms().wrapping_sub(TOLD_AT) > 300 {
                            TOLD_AT = tick_ms();
                            let d_want = chain_depth(&want[f], &mut put_calls);
                            let d_open = chain_depth(&want_open[f], &mut put_calls);
                            let d_anim = chain_depth(&[0.0; 3], &mut put_calls);
                            let palm = palm_depth(side, &sq, &sp, l, &mut put_calls);
                            log!(
                                "[physhands] {} finger {} left {:.1} mm in: shown {:.2} {:.2} {:.2}, solve {:.2} {:.2} {:.2} ({:.1} mm), open {:.2} {:.2} {:.2} ({:.1} mm), animation's ({:.1} mm), palm {:.1} mm in\n",
                                if side == 1 { "right" } else { "left" },
                                f,
                                1000.0 * after,
                                SHOWN[side][f][0], SHOWN[side][f][1], SHOWN[side][f][2],
                                want[f][0], want[f][1], want[f][2],
                                1000.0 * d_want,
                                want_open[f][0], want_open[f][1], want_open[f][2],
                                1000.0 * d_open,
                                1000.0 * d_anim,
                                1000.0 * palm
                            );
                        }
                    }
                }
            }
            for j in 0..3 {
                let v = SHOWN[side][f][j];
                b.a[f][j] = v;
                SHOWN_BENDS[side][f][j].store(v.to_bits(), Relaxed);
                let y = &mut SHOWN_YIELD[side][f][j];
                damped(y, &mut SHOWN_YIELD_V[side][f][j], want_yield[f][j], if want_yield[f][j].abs() > y.abs() { 1.0 / 0.035 } else { 1.0 / 0.07 }, dt, 6.0);
                if y.abs() < 1e-4 && want_yield[f][j].abs() < 1e-4 {
                    *y = 0.0;
                    SHOWN_YIELD_V[side][f][j] = 0.0;
                }
                b.y[f][j] = *y;
                bent |= v != 0.0 || *y != 0.0;
            }
        }
        b.on = bent;
        b.scanner_epoch = st.scanner_epoch;
        b.at = tick_ms();
        BENDS[side].write(&b);
        // trace=1, WHAT IS DRAWN, MEASURED: each finger turned as the animation is given it (b.a), on the hand where its
        // body is (where it is drawn, not where the fingers were solved), against what is near
        for f in 0..5 {
            let mut deep = 0.0f32;
            if on && s.trace != 0 {
                if let Some(l) = l {
                    let c = &h.f[f];
                    let (cq, cp) = chain_base(c);
                    let (bq, bp) = compose(&[st.pose[side][0], st.pose[side][1], st.pose[side][2], st.pose[side][3]], &[st.pose[side][4], st.pose[side][5], st.pose[side][6]], &cq, &cp);
                    let (j_, f_, _) = pose_chain(c, &bq, &bp, &b.a[f]);
                    let mut dummy = 0;
                    deep = segments(c, &j_, Some(&f_), l, &mut dummy);
                }
            }
            SHOWN_DEPTH[side][f].store(deep.to_bits(), Relaxed);
        }
        total_calls += calls + put_calls;
    }
    drop(_shared);
    if total_calls != 0 {
        FINGER_CALLS.fetch_add(total_calls as u64, Relaxed);
        FINGER_CONTACTS.fetch_add(contacts as u64, Relaxed);
        FINGER_SOLVES.fetch_add(1, Relaxed);
        let (t1, _) = qpc();
        FINGER_US.fetch_add(((t1 - t0) * 1_000_000 / freq) as u64, Relaxed);
    }
}

/// PublishFingers: the finger layer's chains as drawn this pass (the animation thread), their capsules grown by the
/// skin's margin.
pub fn publish_fingers(side: usize, hand: &FingerHand) {
    if side > 1 {
        return;
    }
    let s = super::settings::current();
    let mut h = *hand;
    for c in &mut h.f {
        for r in &mut c.radius {
            if *r > 0.0 {
                *r += s.skin_margin;
            }
        }
    }
    FINGER_HANDS[side].write(&h);
}
/// FingerBends: each joint's turn about its hinge, radians ([finger][joint] in the chains' order), and the flexion the
/// animation had when they were solved. None when there is none to make.
pub fn finger_bends(side: usize) -> Option<([[f32; 3]; 5], [[f32; 3]; 5])> {
    if side > 1 || !super::on(side) {
        return None;
    }
    let b = BENDS[side].read()?;
    if !b.on || tick_ms().wrapping_sub(b.at) > 250 || !super::scanner_accepts(b.scanner_epoch) {
        return None;
    }
    Some((b.a, b.flex))
}
