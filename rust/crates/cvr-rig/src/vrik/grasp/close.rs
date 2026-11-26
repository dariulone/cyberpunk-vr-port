//! CLOSE: the fingers closed on the thing one joint at a time, the thumb searched, and the solve that puts them
//! together (main's GraspPose.cpp: CloseFinger, SolveThumb, SpreadFor, LimitFor, Solve).
use super::*;
use cvr_body::cmath::{asin, atan2};

/// One finger closed: its three bones' rotations out of `start` into `out`, its angles (degrees) into `deg`; the
/// segments' contacts. `cap` caps each joint (radians), `spread_about` turns the spread about that way at the knuckle
/// after the bend (a plate's cover's normal), `curl` closes it already curled (a claw's).
pub(super) unsafe fn close_finger(
    buf: *const u8,
    side: i32,
    hand: i32,
    f: &Finger,
    th: &Thing,
    pm: &Palm,
    start: &[Q; SLOTS],
    limit: f32,
    spread: f32,
    out: &mut [Q; 3],
    deg: &mut [f32; 3],
    closed: &mut Closed,
    cap: Option<&[f32; 3]>,
    spread_about: Option<&V3>,
    curl: Option<&[f32; 3]>,
) -> i32 {
    let Some(c) = chain_of(buf, side, hand, &f.slot, start) else { return 0 };
    let mut rot0 = [c.local[0].q, c.local[1].q, c.local[2].q];
    let (mut jts, mut fr) = joints(&c, &rot0);
    // Spread: the proximal phalanx turned in the palm's plane, about the bone axis nearest the palm's normal; or
    // (spread_about, the hand's frame) about that way at the knuckle, after the bend: a plate's cover's normal.
    let mut pre = ID;
    if spread != 0.0 && spread_about.is_some() {
        let turn = axis_angle(spread_about.unwrap_or(&[0.0; 3]), spread);
        let base_inv = conj(&c.base.q);
        let m = qmul(&turn, &c.base.q);
        pre = qmul(&base_inv, &m);
        qnorm(&mut pre);
    } else if spread != 0.0 {
        let axis = local_hinge(&fr[0].q, &pm.n);
        let turn = axis_angle(&axis, spread);
        let mut m = qmul(&rot0[0], &turn);
        qnorm(&mut m);
        rot0[0] = m;
        (jts, fr) = joints(&c, &rot0);
    }
    let Some(hinge) = hinges(&jts, &fr, &pm.n) else { return 0 };
    let mut theta = [0.0f32; 3];
    let mut max_rad = [0.0f32; 3];
    let mut frozen = [false; 3];
    for j in 0..3 {
        max_rad[j] = min(f.max_deg[j] * K_DEG * limit, cap.map_or(1e9, |c| max(0.0, c[j])));
    }
    // Grasp flexion couples the two interphalangeal joints. Allow independent pad adjustment, but never fold the
    // fingertip to 80 degrees while the middle joint is stopped almost straight by a stem or rim.
    let distal_limit = |theta: &[f32; 3]| min(max_rad[2], 0.8 * theta[1] + 15.0 * K_DEG);
    let pose = |t: &[f32; 3]| -> [Q; 3] {
        let mut rotations = [ID; 3];
        for j in 0..3 {
            let turn = axis_angle(&hinge[j], t[j]);
            rotations[j] = qmul(&rot0[j], &turn);
            if j == 0 {
                rotations[0] = qmul(&pre, &rotations[0]);
            }
            qnorm(&mut rotations[j]);
        }
        rotations
    };
    // A segment's clearance: from the thing, and (but the first segment) from the palm -- along its axis, off the bone.
    let clearance = |s: usize, jts: &[V3; 4], fr: &[Pose; 3]| -> (f32, f32) {
        let (mut obj, mut palm) = (1e9f32, 1e9f32);
        let (a, b) = seg_axis(&jts[s], &jts[s + 1], &fr[s].q, &hinge[s], f.palm[s]);
        // the end phalanx over its pad only: the real fingertip narrows past it, turning to the nail
        for u in [0.25f32, 0.5, 0.75, if s == 2 { 0.85 } else { 1.0 }] {
            let mut p = [0.0f32; 3];
            for k in 0..3 {
                p[k] = a[k] + (b[k] - a[k]) * u;
            }
            obj = min(obj, th.dist(&p) - f.radius[s]);
            if s > 0 {
                palm = min(palm, palm_dist(pm, &p) - f.radius[s]);
            }
        }
        (obj, palm)
    };
    // closed already curled (a claw's): from there on as any
    if let Some(curl) = curl {
        for j in 0..3 {
            theta[j] = min(max(0.0, curl[j]), max_rad[j]);
        }
        theta[2] = min(theta[2], distal_limit(&theta));
        (jts, fr) = joints(&c, &pose(&theta));
        if th.shape == K_CAPSULE {
            // A moving limb can already occupy the pre-curl. Never start deeper than the open chain.
            let mut clear = true;
            for s in 0..3 {
                let (o, p) = clearance(s, &jts, &fr);
                clear &= o >= 0.0 && p >= 0.0;
            }
            if !clear {
                theta = [0.0; 3];
                (jts, fr) = joints(&c, &pose(&theta));
            }
        }
    }
    let (mut last_obj, mut last_palm) = ([0.0f32; 3], [0.0f32; 3]);
    for s in 0..3 {
        (last_obj[s], last_palm[s]) = clearance(s, &jts, &fr);
    }
    let mut contacts = 0;
    for _ in 0..240 {
        let prev = theta;
        let mut moved = false;
        for j in 0..3 {
            let maximum = if j == 2 { distal_limit(&theta) } else { max_rad[j] };
            if frozen[j] || theta[j] >= maximum {
                continue;
            }
            // A joint leads once the one before it is done; until then it only follows a little.
            let leads = j == 0 || frozen[j - 1] || theta[j - 1] >= max_rad[j - 1];
            theta[j] = min(maximum, theta[j] + STEP * f.speed[j] * (if leads { 1.0 } else { FOLLOW }));
            moved = true;
        }
        if !moved {
            break;
        }
        (jts, fr) = joints(&c, &pose(&theta));
        for s in 0..3 {
            let (o, p) = clearance(s, &jts, &fr);
            // Into the thing or the palm: a segment that was clear, or one already in that goes deeper.
            let hit = (o < 0.0 && o < last_obj[s] - 1e-5) || (p < 0.0 && p < last_palm[s] - 1e-5);
            if !hit {
                continue;
            }
            for j in 0..=s {
                if frozen[j] {
                    continue;
                }
                theta[j] = prev[j];
                frozen[j] = true;
            }
            theta[2] = min(theta[2], distal_limit(&theta));
            (jts, fr) = joints(&c, &pose(&theta));
            contacts += 1;
        }
        // The next step compares with the accepted pose, not a rejected trial whose penetration and downstream joints
        // belonged to another pose.
        for s in 0..3 {
            (last_obj[s], last_palm[s]) = clearance(s, &jts, &fr);
        }
    }
    let rotations = pose(&theta);
    let (jts, fr) = joints(&c, &rotations);
    for j in 0..3 {
        out[j] = rotations[j];
        deg[j] = theta[j] / K_DEG;
        closed.radius[j] = f.radius[j];
        (closed.a[j], closed.b[j]) = seg_axis(&jts[j], &jts[j + 1], &fr[j].q, &hinge[j], f.palm[j]);
    }
    closed.j = jts;
    closed.proximal = fr[0];
    closed.ok = true;
    contacts
}

/// The thumb: its metacarpal turned to a way given by its own anatomical angles -- its elevation off the palm (palmar
/// abduction) and its angle out from the index finger in the palm's plane (radial abduction; under 0 across the palm) --
/// by the least turn from the rig's way, then rolled about itself (pronation), and its two joints bent about their
/// hinges.
#[derive(Clone, Copy)]
struct ThumbAngles {
    elev: f32,
    away: f32,
    roll: f32,
    mcp: f32,
    ip: f32,
}
impl ThumbAngles {
    fn get(&self, i: usize) -> f32 {
        [self.elev, self.away, self.roll, self.mcp, self.ip][i]
    }
    fn set(&mut self, i: usize, v: f32) {
        match i {
            0 => self.elev = v,
            1 => self.away = v,
            2 => self.roll = v,
            3 => self.mcp = v,
            _ => self.ip = v,
        }
    }
}
/// THE THUMB AT EASE in each grasp (the metacarpal's elevation and angle out, its two joints' bend), which the comfort
/// term pulls toward, so that a goal out of reach leaves the thumb as a hand would hold it.
#[derive(Clone, Copy)]
struct ThumbRest {
    elev: f32,
    away: f32,
    mcp: f32,
    ip: f32,
}
fn rest_for(kind: Kind) -> ThumbRest {
    let r = |e: f32, a: f32, m: f32, i: f32| ThumbRest { elev: e * K_DEG, away: a * K_DEG, mcp: m * K_DEG, ip: i * K_DEG };
    match kind {
        Kind::Flat => r(0.0, 40.0, 10.0, 10.0),
        Kind::Ball => r(40.0, 40.0, 15.0, 25.0),
        // a crab's claw: bent at both joints
        Kind::Edge => r(55.0, 15.0, 25.0, 35.0),
        Kind::Pinch | Kind::Tripod => r(45.0, 15.0, 20.0, 20.0),
        Kind::Top | Kind::Cradle => r(35.0, 40.0, 15.0, 20.0),
        Kind::Wrap => r(45.0, 35.0, 20.0, 30.0),
    }
}
/// The roll that goes with opposition (per radian of the rise off the rig's elevation and half the turn in across the
/// palm) and its free play; the comfort term's weight (per radian squared); a real thumb's elevation and angle out, past
/// which a soft wall rises (THUMB_WALL per radian squared), and the most the whole thumb may point out sideways.
const ROLL_PER_LIFT: f32 = 0.45;
const ROLL_FREE: f32 = 15.0 * K_DEG;
const THUMB_COMFORT: f32 = 0.06;
const ELEV_EASE: [f32; 2] = [-5.0 * K_DEG, 70.0 * K_DEG];
const AWAY_EASE: [f32; 2] = [-25.0 * K_DEG, 65.0 * K_DEG];
const WHOLE_AWAY: f32 = 75.0 * K_DEG;
/// The pad's goal at most this share of the relaxed thumb's reach (base to pad) from its base.
const THUMB_REACH: f32 = 0.85;
const THUMB_WALL: f32 = 2.0;
fn past(v: f32, range: &[f32; 2]) -> f32 {
    if v < range[0] {
        range[0] - v
    } else if v > range[1] {
        v - range[1]
    } else {
        0.0
    }
}
/// Sparse first-web/thenar skin from the FPP mesh at the reference pose (GRASP_WEB_FIT): (n, d, t) from the palm's
/// skin, then the effective weights on the thumb's metacarpal, its MCP and the index MCP; the rest stays with the palm.
const WEB_POINTS: [[f32; 6]; 22] = [
    [0.02052, 0.02392, 0.04861, 0.08696, 0.82213, 0.00000],
    [0.01385, 0.02632, 0.04190, 0.26587, 0.37302, 0.00992],
    [0.02311, 0.02056, 0.04158, 0.19920, 0.60159, 0.00000],
    [0.03049, 0.00586, 0.04095, 0.36166, 0.27668, 0.00000],
    [0.01671, -0.02245, 0.02022, 0.27668, 0.00000, 0.00000],
    [0.01244, -0.02478, 0.02541, 0.25198, 0.00000, 0.00000],
    [0.01925, -0.01762, 0.01526, 0.31151, 0.00000, 0.00000],
    [0.02654, 0.01418, 0.03818, 0.31818, 0.36364, 0.00000],
    [0.01018, 0.00862, 0.01110, 0.33665, 0.00000, 0.00000],
    [0.01179, 0.01494, 0.02054, 0.44024, 0.00398, 0.00000],
    [0.02768, -0.00129, 0.04032, 0.45276, 0.09449, 0.00000],
    [0.02187, -0.00769, 0.03987, 0.47441, 0.02756, 0.00000],
    [0.02763, -0.00125, 0.03305, 0.47835, 0.03937, 0.00000],
    [0.02483, 0.00760, 0.03002, 0.46063, 0.07874, 0.00000],
    [0.02241, -0.01375, 0.02750, 0.42292, 0.00000, 0.00000],
    [0.01581, 0.00745, 0.01534, 0.43452, 0.00000, 0.00000],
    [0.01762, -0.00520, 0.04653, 0.44664, 0.10277, 0.00000],
    [0.02637, -0.00026, 0.02611, 0.48622, 0.00787, 0.00000],
    [0.01728, 0.01625, 0.02990, 0.44048, 0.11111, 0.00000],
    [0.01425, 0.02342, 0.03363, 0.39243, 0.16335, 0.00797],
    [0.02299, -0.00766, 0.01659, 0.41667, 0.00000, 0.00000],
    [0.01921, -0.00605, 0.01046, 0.35714, 0.00000, 0.00000],
];
#[derive(Clone, Copy, Default)]
struct WebLocal {
    fixed: V3,
    meta: V3,
    mcp: V3,
}

/// Searched -- a coarse grid, then finer steps one angle at a time -- for the least cost: the thumb out of the thing,
/// the palm and the closed fingers; its pad on the thing's surface, near the grasp's goal and facing it; the thumb near
/// its ease for the grasp and within a real thumb's range. `person`: a limb, whose flesh runs through the first web.
pub(super) unsafe fn solve_thumb(
    buf: *const u8,
    side: i32,
    hand: i32,
    kind: Kind,
    axis: usize,
    th: &Thing,
    pm: &Palm,
    start: &[Q; SLOTS],
    fingers: &[Closed; 4],
    out: &mut [Q; 3],
    deg: &mut [f32; 4],
    person: bool,
) -> bool {
    let Some(c) = chain_of(buf, side, hand, &THUMB_SLOTS, start) else { return false };
    let rot0 = [c.local[0].q, c.local[1].q, c.local[2].q];
    let (jts, fr) = joints(&c, &rot0);
    let toward = sub(&pm.skin, &jts[1]);
    let Some(hinge) = hinges(&jts, &fr, &toward) else { return false };
    let mut web = [WebLocal::default(); 22];
    if person {
        let index_ref = in_frame(buf, side, VRIK_LadderFingerBone(side, 6), hand, Some(start)).unwrap_or(POSE0);
        for (i, w) in WEB_POINTS.iter().enumerate() {
            let mut p = [0.0f32; 3];
            for k in 0..3 {
                p[k] = pm.skin[k] + pm.n[k] * w[0] + pm.d[k] * w[1] + pm.t[k] * w[2];
            }
            web[i].meta = to_frame(&fr[0], &p);
            web[i].mcp = to_frame(&fr[1], &p);
            let index_local = to_frame(&index_ref, &p);
            let posed = if fingers[0].ok { fingers[0].proximal } else { index_ref };
            let index_pos = rot(&posed.q, &index_local);
            for k in 0..3 {
                web[i].fixed[k] = p[k] * (1.0 - w[3] - w[4] - w[5]) + (index_pos[k] + posed.p[k]) * w[5];
            }
        }
    }
    // The metacarpal's way in the rig and its angles; the roll's axis, mirrored with the hand (the right hand's d x t is
    // against its palm's normal), so that a positive roll pronates either thumb.
    let dxt = cross(&pm.d, &pm.t);
    let front = dot(&dxt, &pm.n) > 0.0;
    let mut meta = sub(&jts[1], &jts[0]);
    unit(&mut meta);
    let roll_axis = if front { neg(&meta) } else { meta };
    let elev0 = asin(clamp(dot(&meta, &pm.n), -1.0, 1.0));
    let away0 = atan2(dot(&meta, &pm.t), dot(&meta, &pm.d));
    let base_model = qmul(&c.base.q, &rot0[0]);
    let base_inv = conj(&c.base.q);
    // The goal for the pad.
    let mut goal = match kind {
        Kind::Wrap => {
            // Round its side the other way from the fingers (a large-diameter wrap): on the cylinder's back and top as
            // the palm sees it, where the thumb meets the fingertips -- at the index finger's place along the axis, a
            // little toward the thumb. Not on its end.
            let a = rot(&th.pose.q, &e_axis(axis));
            let rel = sub(&pm.index, &th.pose.p);
            let length = if th.shape == K_BOX { th.half[axis] } else { th.half[2] };
            let along = clamp(dot(&rel, &a) + 0.01 * (if dot(&a, &pm.t) >= 0.0 { 1.0 } else { -1.0 }), -0.8 * length, 0.8 * length);
            let q = mad(&th.pose.p, &a, along);
            // over the front of it, a little toward the fingertips: the thumb crosses it to meet the fingers coming round
            let mut u = [0.0f32; 3];
            for k in 0..3 {
                u[k] = pm.n[k] * 0.85 + pm.d[k] * 0.35;
            }
            u = mad(&u, &a, -dot(&u, &a));
            if !unit(&mut u) {
                u = pm.n;
            }
            th.nearest(&mad(&q, &u, 0.2))
        }
        Kind::Ball => {
            let p = if th.shape == K_SPHERE && th.half[0] >= WRAP_RADIUS {
                // too big to close round: the palm lies on it, the thumb beside the index knuckle as on a face
                mad(&mad(&pm.index, &pm.t, 0.025), &pm.n, PALM_SKIN)
            } else {
                let mut dir = [0.0f32; 3];
                for k in 0..3 {
                    dir[k] = pm.t[k] * 0.75 + pm.n[k] * 0.6;
                }
                unit(&mut dir);
                mad(&th.pose.p, &dir, 1.0)
            };
            th.nearest(&p)
        }
        Kind::Pinch => th.nearest(&mad(&th.pose.p, &pm.pinch_n, 0.2)),
        Kind::Tripod => {
            // Its other side across from the two pads.
            let sideways = side_pinch(th.shape, &th.half);
            th.nearest(&mad(if sideways { &pm.side } else { &pm.tri }, if sideways { &pm.side_n } else { &pm.tri_n }, 0.2))
        }
        Kind::Top | Kind::Cradle => {
            // On its side, the other way from the fingers: from above just under the rim, toward the wrist; caught from
            // below a third of the way up, on the thumb's side.
            let (a, length) = long_axis(th.shape, &th.half, axis, &th.pose);
            let rel = sub(&pm.skin, &th.pose.p);
            // toward the end at the palm
            let s = if dot(&rel, &a) >= 0.0 { 1.0 } else { -1.0 };
            let mut u = [0.0f32; 3];
            let p = if kind == Kind::Top {
                for k in 0..3 {
                    u[k] = pm.t[k] * 0.6 - pm.d[k] * 0.8;
                }
                mad(&th.pose.p, &a, s * max(0.0, length - 0.015))
            } else {
                for k in 0..3 {
                    u[k] = pm.t[k] * 0.9 - pm.d[k] * 0.3;
                }
                mad(&th.pose.p, &a, s * length / 3.0)
            };
            u = mad(&u, &a, -dot(&u, &a));
            if !unit(&mut u) {
                u = pm.t;
            }
            th.nearest(&mad(&p, &u, 0.2))
        }
        Kind::Edge => {
            // The other cover right behind the index and middle fingers' pads (where they press it: a crab's claw), or,
            // without them, 6.5 cm out of the palm by the index finger.
            let p = if fingers[0].ok && fingers[1].ok {
                let mut p = [0.0f32; 3];
                for f in &fingers[..2] {
                    for k in 0..3 {
                        p[k] += 0.5 * (f.j[2][k] + 0.6 * (f.j[3][k] - f.j[2][k]));
                    }
                }
                mad(&p, &pm.d, -0.10)
            } else {
                mad(&mad(&pm.index, &pm.n, 0.065), &pm.d, -0.06)
            };
            th.nearest(&p)
        }
        Kind::Flat => {
            // Beside the index finger's knuckle, on the thumb's side: a big box's face, a narrow one's side.
            th.nearest(&mad(&mad(&pm.index, &pm.t, 0.025), &pm.n, PALM_SKIN))
        }
    };
    // the joints' hard ends: the elevation and the angle out a little past a real thumb's (the soft wall keeps it in
    // range unless nothing else is clear), the two joints never back more than a pressed pad gives
    let lo = [-20.0 * K_DEG, -50.0 * K_DEG, -60.0 * K_DEG, 0.0 * K_DEG, -5.0 * K_DEG];
    let hi = [80.0 * K_DEG, 85.0 * K_DEG, 60.0 * K_DEG, 60.0 * K_DEG, 80.0 * K_DEG];
    let roll_with = |e: f32, p: f32| clamp(ROLL_PER_LIFT * ((e - elev0) + 0.5 * (away0 - p)), lo[2], hi[2]);
    // A goal past the thumb's reach (a big part's far side) drawn in along the way to it from the thumb's base and back
    // onto the surface: the near side the thumb can get to, not a pull toward where it cannot.
    {
        let seg = sub(&jts[3], &jts[2]);
        let pad = mad(&jts[2], &seg, 0.7);
        let reach = THUMB_REACH * len(&sub(&pad, &jts[0]));
        let v = sub(&goal, &jts[0]);
        let dist = len(&v);
        if dist > reach && dist > 1e-4 {
            goal = th.nearest(&mad(&jts[0], &v, reach / dist));
        }
    }
    if person && kind == Kind::Wrap {
        // A limb wider than the hand is a partial wrap: seek the reachable near side, not its far crown.
        let mut pad = [0.0f32; 3];
        for k in 0..3 {
            pad[k] = jts[2][k] + (jts[3][k] - jts[2][k]) * 0.6;
        }
        goal = th.nearest(&pad);
    }
    let rest = rest_for(kind);
    // the thumb lying on what the palm lies on: a face, a ball too big to close round
    let lies = kind == Kind::Flat || (kind == Kind::Ball && th.shape == K_SPHERE && th.half[0] >= WRAP_RADIUS);
    // the pad itself opposed to the fingers' pads across a plate (a crab's claw): its roll free, the pad to face it
    let opposes = kind == Kind::Edge || (kind == Kind::Tripod && side_pinch(th.shape, &th.half));
    // (cost, overlap, the pad's gap, the pad from its goal), the rotations written to `rot`
    let cost = |a: &ThumbAngles, rot_out: &mut [Q; 3]| -> (f32, f32, f32, f32) {
        let (ce, se, ca, sa) = (cos(a.elev), sin(a.elev), cos(a.away), sin(a.away));
        let mut want = [0.0f32; 3];
        for k in 0..3 {
            want[k] = pm.n[k] * se + (pm.d[k] * ca + pm.t[k] * sa) * ce;
        }
        let qd = from_to(&meta, &want);
        let qr = axis_angle(&roll_axis, a.roll);
        let m2 = qmul(&qd, &qr);
        let m = qmul(&m2, &base_model);
        rot_out[0] = qmul(&base_inv, &m);
        qnorm(&mut rot_out[0]);
        let t1 = axis_angle(&hinge[1], a.mcp);
        let t2 = axis_angle(&hinge[2], a.ip);
        rot_out[1] = qmul(&rot0[1], &t1);
        rot_out[2] = qmul(&rot0[2], &t2);
        qnorm(&mut rot_out[1]);
        qnorm(&mut rot_out[2]);
        let (pp, g) = joints(&c, rot_out);
        let mut pen = 0.0f32;
        let mut nearest = 1e9f32;
        if person {
            for (i, w) in WEB_POINTS.iter().enumerate() {
                let a = rot(&g[0].q, &web[i].meta);
                let b = rot(&g[1].q, &web[i].mcp);
                let mut p = [0.0f32; 3];
                for k in 0..3 {
                    p[k] = web[i].fixed[k] + (a[k] + g[0].p[k]) * w[3] + (b[k] + g[1].p[k]) * w[4];
                }
                let o = 0.001 - th.dist(&p);
                if o > 0.0 {
                    pen += o * o;
                }
            }
        }
        // the pad's way: the way the distal joint bends it
        let seg = sub(&pp[3], &pp[2]);
        let axis_w = rot(&g[2].q, &hinge[2]);
        let mut pad_n = cross(&axis_w, &seg);
        unit(&mut pad_n);
        // the pad's half (the slices to two thirds of the way) by the real profile
        let mut pad_gap = 1e9f32;
        for s in 0..3 {
            // The metacarpal is in the thenar's flesh. Limb wraps check it and the web above; exposed phalanges use
            // offset capsules, with the real profile for the tip.
            if s == 0 {
                if person {
                    // The thenar follows the metacarpal. Unlike a loose item, a limb extends through the web, so its
                    // flesh must be checked too, not only the two exposed phalanges.
                    for u in [0.25f32, 0.5, 0.75, 1.0] {
                        let mut p = [0.0f32; 3];
                        for k in 0..3 {
                            p[k] = pp[0][k] + (pp[1][k] - pp[0][k]) * u;
                        }
                        let o = THUMB_RADIUS[0] - th.dist(&p);
                        if o > 0.0 {
                            pen += o * o;
                        }
                    }
                }
                continue;
            }
            let (seg_a, seg_b) = seg_axis(&pp[s], &pp[s + 1], &g[s].q, &hinge[s], THUMB_PALM[s]);
            if s == 2 {
                // the way to the thing's surface once, from the phalanx's middle (the slices span 2.5 cm)
                let mut mid = [0.0f32; 3];
                for k in 0..3 {
                    mid[k] = 0.5 * (pp[2][k] + pp[3][k]);
                }
                let gr = th.grad(&mid);
                for sl in &THUMB_TIP {
                    let mut p = [0.0f32; 3];
                    for k in 0..3 {
                        p[k] = pp[2][k] + (pp[3][k] - pp[2][k]) * sl.u;
                    }
                    let dist = th.dist(&p);
                    let toward = if dist >= 0.0 { neg(&gr) } else { gr };
                    let o = tip_extent(sl, &pad_n, &toward) - dist;
                    if o > 0.0 {
                        pen += o * o;
                    }
                    nearest = min(nearest, -o);
                    if sl.u <= 0.66 {
                        pad_gap = min(pad_gap, -o);
                    }
                    let q = THUMB_RADIUS[s] - palm_dist(pm, &p);
                    if q > 0.0 {
                        pen += q * q;
                    }
                }
            }
            for u in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                if s == 2 || (s == 1 && u < 0.25) {
                    continue;
                }
                let mut p = [0.0f32; 3];
                for k in 0..3 {
                    p[k] = seg_a[k] + (seg_b[k] - seg_a[k]) * u;
                }
                let o = THUMB_RADIUS[s] - th.dist(&p);
                if o > 0.0 {
                    pen += o * o;
                }
                nearest = min(nearest, -o);
                let q = THUMB_RADIUS[s] - palm_dist(pm, &p);
                if q > 0.0 {
                    pen += q * q;
                }
            }
            for f in fingers.iter().filter(|f| f.ok) {
                for gi in 0..3 {
                    let o = THUMB_RADIUS[s] + f.radius[gi] - seg_seg(&seg_a, &seg_b, &f.a[gi], &f.b[gi]);
                    if o > 0.0 {
                        pen += o * o;
                    }
                }
            }
        }
        // The pad facing the thing: the way the distal joint bends it, against the surface's normal; on the segment's
        // axis off the bone.
        let pad = mad(&mad(&pp[2], &seg, 0.7), &pad_n, THUMB_PALM[2]);
        // on a face the palm lies on the thumb lies too: touching with any part of it (its pad looks at the index finger)
        let gap = if lies { nearest } else { pad_gap };
        let off = sub(&pad, &goal);
        let gr = th.grad(&pad);
        let facing = 1.0 + dot(&pad_n, &gr);
        // comfort: off the thumb at ease for this grasp (the angle out, the one a thumb minds most, the more), the roll
        // off what the opposition brings past its free play; past a real thumb's range a wall, and on the whole thumb
        // out sideways past WHOLE_AWAY (an L with the index finger)
        let mut tv = sub(&pp[3], &pp[0]);
        unit(&mut tv);
        let whole_away = atan2(dot(&tv, &pm.t), dot(&tv, &pm.d));
        let (de, dp, dm, di) = (a.elev - rest.elev, a.away - rest.away, a.mcp - rest.mcp, a.ip - rest.ip);
        // (opposed to the fingers' pads across a plate the roll is the pad's to choose: turned to the cover, against
        // the lift's coupling)
        let dr = if opposes { 0.0 } else { max(0.0, (a.roll - roll_with(a.elev, a.away)).abs() - ROLL_FREE) };
        let (pe, pa) = (past(a.elev, &ELEV_EASE), past(a.away, &AWAY_EASE));
        let pw = max(0.0, whole_away - WHOLE_AWAY);
        // lying on a face the thumb follows it from its base, not bent at its joints (a sharp MCP reads as broken)
        let jw = if lies { 3.0 } else { 1.0 };
        let comfort = THUMB_COMFORT * (de * de + 1.5 * dp * dp + jw * (dm * dm + di * di)) + 0.5 * dr * dr + THUMB_WALL * (pe * pe + pa * pa + pw * pw);
        let total = 1e5 * pen
            + 2e3 * max(gap, 0.0) * max(gap, 0.0)
            + 30.0 * dot(&off, &off)
            + (if lies {
                0.0
            } else if opposes {
                0.4
            } else {
                0.01
            }) * facing
            + comfort;
        (total, sqrt(pen), gap, len(&off))
    };
    let mut scratch = [ID; 3];
    let mut best = ThumbAngles { elev: elev0, away: away0, roll: 0.0, mcp: 0.0, ip: 0.0 };
    let mut best_cost = cost(&best, &mut scratch).0;
    let mut e = -15.0 * K_DEG;
    while e <= 75.0 * K_DEG + 1e-4 {
        let mut p = -45.0 * K_DEG;
        while p <= 75.0 * K_DEG + 1e-4 {
            let mut m = 0.0f32;
            while m <= hi[3] + 1e-4 {
                let mut i = 0.0f32;
                while i <= hi[4] + 1e-4 {
                    for rk in 0..(if opposes { 3 } else { 1 }) {
                        // opposed across a plate also turned to it: the coupling's roll, then -60 and -20 deg
                        let roll = match rk {
                            0 => roll_with(e, p),
                            1 => lo[2],
                            _ => -20.0 * K_DEG,
                        };
                        let a = ThumbAngles { elev: e, away: p, roll, mcp: m, ip: i };
                        let v = cost(&a, &mut scratch).0;
                        if v < best_cost {
                            best_cost = v;
                            best = a;
                        }
                    }
                    i += 20.0 * K_DEG;
                }
                m += 20.0 * K_DEG;
            }
            p += 15.0 * K_DEG;
        }
        e += 15.0 * K_DEG;
    }
    for step in [8.0 * K_DEG, 4.0 * K_DEG, 2.0 * K_DEG, 1.0 * K_DEG] {
        for _ in 0..24 {
            let mut better = false;
            for k in 0..5 {
                for sgn in [-1.0f32, 1.0] {
                    let mut a = best;
                    a.set(k, clamp(a.get(k) + sgn * step, lo[k], hi[k]));
                    let v = cost(&a, &mut scratch).0;
                    if v < best_cost - 1e-7 {
                        best_cost = v;
                        best = a;
                        better = true;
                    }
                }
            }
            if !better {
                break;
            }
        }
    }
    let (v, pen, gap, off) = cost(&best, out);
    CyberpunkVR_DebugGraspThumb[side as usize] = [v, pen * 1000.0, gap * 1000.0, off * 1000.0];
    *deg = [best.elev / K_DEG, best.away / K_DEG, best.mcp / K_DEG, best.ip / K_DEG];
    true
}

/// The fingers' spread for a ball: wider the bigger it is (radians, the index finger's; the others by their share).
/// Taken from above the fingertips go round the rim, spread by its size; caught from below they go up its sides.
fn spread_for(kind: Kind, half: &V3) -> f32 {
    match kind {
        Kind::Ball => clamp(half[0] * 1.6, 0.08, 0.22),
        Kind::Flat => 0.05,
        // a cover's fingers: solve's COVER_SPREAD
        Kind::Edge => 0.0,
        Kind::Top => clamp(max(half[0], half[1]) * 2.4, 0.05, 0.16),
        Kind::Cradle => 0.04,
        _ => 0.0,
    }
}
/// How far a finger may close (a share of its joints' range): three fingers leave the ring and little finger curled out
/// of the way, a pinch the three after the index.
fn limit_for(kind: Kind, finger: usize) -> f32 {
    if kind == Kind::Pinch && finger >= 1 {
        return 0.6;
    }
    if kind == Kind::Tripod && finger >= 2 {
        return 0.75;
    }
    1.0
}
/// on a cover each finger its own (radians): the rig's bent fingers run together unevenly, so these leave the tips some
/// 16 mm apart
const COVER_SPREAD: [f32; 4] = [0.125, 0.0, -0.025, -0.165];
/// a thick plate's side: the index and middle fingers a crab's claw, the ring and little finger tucked a little apart
const TUCK_SPREAD: [f32; 4] = [0.0, 0.0, -0.12, -0.15];

pub(super) unsafe fn solve(buf: *const u8, side: i32, hand: i32, st: &mut Side) {
    let sd = side as usize;
    let mut start = [ID; SLOTS];
    reference_fingers(buf, side, &mut start, Some(&mut st.have));
    st.rot = start;
    if st.req.baked {
        for s in 0..SLOTS {
            st.rot[s] = qmul(&start[s], &st.req.baked_delta[s]);
            qnorm(&mut st.rot[s]);
        }
        CyberpunkVR_DebugGraspAngles[sd][0] = st.req.baked_kind as f32;
        CyberpunkVR_DebugGrasp[0] = CyberpunkVR_DebugGrasp[0].wrapping_add(1);
        return;
    }
    let th = Thing { shape: st.req.shape, half: st.req.half, pose: compose(&st.pm.slot, &st.req.obj), lathe: st.req.lathe };
    let (kind, axis) = if st.req.force >= 0 {
        (Kind::from_i32(st.req.force), 2)
    } else if precision_stem(st.req.shape, &st.req.half, st.req.ends, st.req.part) {
        (Kind::Tripod, 2)
    } else {
        seated(st.req.shape, &st.req.half, &th.pose, &st.pm, st.req.part)
    };
    let pm = st.pm;
    // Spread toward the thumb (index) or away from it (ring, pinky), whichever way that turns about the normal.
    let nxd = cross(&pm.n, &pm.d);
    let to_thumb = if dot(&nxd, &pm.t) >= 0.0 { 1.0 } else { -1.0 };
    let spread = if st.req.spread >= 0.0 { st.req.spread } else { spread_for(kind, &st.req.half) };
    let mut contacts = 0;
    let mut closed = [CLOSED0; 4];
    let mut degs = [[0.0f32; 3]; 4];
    let mut outs = [[[0.0f32; 4]; 3]; 4];
    let mut gots = [0i32; 4];
    let req_limit = clamp(st.req.limit, 0.05, 1.0);
    let limit_of = |i: usize| limit_for(kind, i) * req_limit;
    // a plate's fingers spread on its cover: about n x t, signed so that with the spread's own sign (to_thumb) the
    // index finger goes toward the thumb
    let mut about = cross(&pm.n, &pm.t);
    for v in about.iter_mut() {
        *v *= to_thumb;
    }
    let spread_about = (kind == Kind::Edge).then_some(&about);
    let claw = kind == Kind::Tripod && side_pinch(th.shape, &th.half);
    let spread_of = |i: usize| {
        (if kind == Kind::Edge {
            COVER_SPREAD[i]
        } else if claw {
            TUCK_SPREAD[i]
        } else {
            FINGERS[i].spread * spread
        }) * to_thumb
    };
    let person = st.person;
    let curl_of = |i: usize| -> Option<&'static [f32; 3]> {
        if claw {
            Some(if i < 2 { &SIDE_CURL } else { &TUCK_CURL })
        } else if person && kind == Kind::Wrap {
            Some(&SIDE_CURL)
        } else {
            None
        }
    };
    for i in 0..4 {
        gots[i] = close_finger(buf, side, hand, &FINGERS[i], &th, &pm, &start, limit_of(i), spread_of(i), &mut outs[i], &mut degs[i], &mut closed[i], None, spread_about, curl_of(i));
    }
    // THE FINGERS AS ONE HAND: no joint more than COUPLE_DEG[pair][joint] more bent than the same joint of the finger
    // beside it. A finger that closed further is closed again only so far: opened back along the way it closed.
    for _pass in 0..2 {
        for i in 0..4 {
            if !closed[i].ok {
                continue;
            }
            let mut cap = [1e9f32; 3];
            let mut over = false;
            for k in [i as i32 - 1, i as i32 + 1] {
                if !(0..=3).contains(&k) || !closed[k as usize].ok {
                    continue;
                }
                let k = k as usize;
                let most = &COUPLE_DEG[i.min(k)];
                for j in 0..3 {
                    cap[j] = min(cap[j], (degs[k][j] + most[j]) * K_DEG);
                    over |= degs[i][j] > degs[k][j] + most[j] + 0.5;
                }
            }
            if !over {
                continue;
            }
            gots[i] = close_finger(buf, side, hand, &FINGERS[i], &th, &pm, &start, limit_of(i), spread_of(i), &mut outs[i], &mut degs[i], &mut closed[i], Some(&cap), spread_about, curl_of(i));
            CyberpunkVR_DebugGraspCoupled = CyberpunkVR_DebugGraspCoupled.wrapping_add(1);
        }
    }
    for i in 0..4 {
        if !closed[i].ok {
            continue;
        }
        contacts += gots[i];
        for j in 0..3 {
            st.rot[FINGERS[i].slot[j]] = outs[i][j];
            CyberpunkVR_DebugGraspAngles[sd][5 + i * 3 + j] = degs[i][j];
        }
    }
    let mut thumb = [ID; 3];
    let mut deg = [0.0f32; 4];
    if solve_thumb(buf, side, hand, kind, axis, &th, &pm, &start, &closed, &mut thumb, &mut deg, person && kind == Kind::Wrap) {
        for j in 0..3 {
            st.rot[THUMB_SLOTS[j]] = thumb[j];
        }
        for j in 0..4 {
            CyberpunkVR_DebugGraspAngles[sd][1 + j] = deg[j];
        }
    }
    CyberpunkVR_DebugGraspAngles[sd][0] = kind as i32 as f32;
    CyberpunkVR_DebugGrasp[3] = contacts as u64;
    CyberpunkVR_DebugGrasp[0] = CyberpunkVR_DebugGrasp[0].wrapping_add(1);
}
