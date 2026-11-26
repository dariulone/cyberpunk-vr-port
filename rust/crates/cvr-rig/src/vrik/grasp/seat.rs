//! SEAT: where a thing lies in the slot for its grasp, the slot pinned while the hand holds, and each pose pass's
//! palms and fingers (main's GraspPose.cpp: ApplySeatFit, Seat, PinSlot, GraspFingers, GraspHolding, GraspTake).
use super::*;
use super::close::solve;
use std::sync::OnceLock;

/// A seat the C++ fitter (tools/grip_generator/pose_baker.cpp) checked jointly with the solved fingers against the
/// visible FPP skin, for one mesh, hand, primitive and grasp: the seat moved along the palm (shift: along d, n, t) and
/// tilted about its normal, scaled by the palm against the one it was fitted on.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SeatFit {
    mesh: u64,
    side: i32,
    shape: i32,
    kind: i32,
    ends: i32,
    part: bool,
    axis_sign: i32,
    half: V3,
    palm_d: f32,
    palm_t: f32,
    shift: V3,
    tilt: f32,
}
const SEAT_FITS_TEXT: &str = include_str!("../../../../../../src/Anim/GraspSeatFits.inc");
/// One line of the fitter's table: `{0x...ull, side, shape, kind, ends, part, axisSign, {h f, h f, h f}, palmD f, palmT
/// f, {s f, s f, s f}, tilt f},`.
fn parse_seat_fit(line: &str) -> Option<SeatFit> {
    let body = line.trim().strip_prefix('{')?;
    let body = body.trim_end().trim_end_matches(',').strip_suffix('}')?;
    let mut it = body.split([',', '{', '}']).map(str::trim).filter(|s| !s.is_empty());
    let mesh = u64::from_str_radix(it.next()?.trim_start_matches("0x").trim_end_matches("ull"), 16).ok()?;
    let mut int = || -> Option<i32> { it.next()?.parse().ok() };
    let (side, shape, kind, ends) = (int()?, int()?, int()?, int()?);
    let part = match it.next()? {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    let axis_sign: i32 = it.next()?.parse().ok()?;
    let mut f = || -> Option<f32> { it.next()?.trim_end_matches('f').parse().ok() };
    let half = [f()?, f()?, f()?];
    let (palm_d, palm_t) = (f()?, f()?);
    let shift = [f()?, f()?, f()?];
    let tilt = f()?;
    Some(SeatFit { mesh, side, shape, kind, ends, part, axis_sign, half, palm_d, palm_t, shift, tilt })
}
fn seat_fits() -> &'static [SeatFit] {
    static TABLE: OnceLock<Vec<SeatFit>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v: Vec<SeatFit> = SEAT_FITS_TEXT.lines().filter(|l| l.trim_start().starts_with('{')).filter_map(parse_seat_fit).collect();
        // the generated table is sorted by mesh; a stable sort keeps its order among one mesh's rows
        v.sort_by_key(|f| f.mesh);
        v
    })
}
fn apply_seat_fit(mesh: u64, side: usize, shape: i32, kind: Kind, ends: i32, part: bool, half: &V3, palm: &Palm, position: &mut V3, rotation: &mut Q) -> bool {
    if mesh == 0 {
        return false;
    }
    let table = seat_fits();
    let first = table.partition_point(|f| f.mesh < mesh);
    let axis = rot(rotation, &[0.0, 0.0, 1.0]);
    for fit in table[first..].iter().take_while(|f| f.mesh == mesh) {
        if fit.side != side as i32 || fit.shape != shape || fit.kind != kind as i32 || fit.ends != ends || fit.part != part {
            continue;
        }
        let mut same = true;
        for k in 0..3 {
            same &= (half[k] - fit.half[k]).abs() <= max(0.0005, 0.02 * fit.half[k]);
        }
        if !same || dot(&axis, &palm.t) * (fit.axis_sign as f32) < 0.95 {
            continue;
        }
        let d = palm.half_d / fit.palm_d;
        let t = palm.half_t / fit.palm_t;
        if !((0.85..=1.15).contains(&d) && (0.85..=1.15).contains(&t)) {
            continue;
        }
        *position = mad(position, &palm.d, fit.shift[0] * d);
        *position = mad(position, &palm.n, fit.shift[1] * 0.5 * (d + t));
        *position = mad(position, &palm.t, fit.shift[2] * t);
        let turn = axis_angle(&palm.n, fit.tilt * K_DEG);
        let mut q = qmul(&turn, rotation);
        qnorm(&mut q);
        *rotation = q;
        return true;
    }
    false
}

fn turn_onto(r: &mut Q, from: &V3, to: &V3) {
    let turn = from_to(from, to);
    let mut next = qmul(&turn, r);
    qnorm(&mut next);
    *r = next;
}
fn axis_of(r: &Q, k: usize) -> V3 {
    rot(r, &e_axis(k))
}
/// Turned about `about` (unit), the least, so that axis k lies along +-target projected off `about`.
fn align_about(r: &mut Q, k: usize, about: &V3, target: &V3) {
    let a0 = axis_of(r, k);
    let mut a = mad(&a0, about, -dot(&a0, about));
    let mut g = mad(target, about, -dot(target, about));
    if !unit(&mut a) || !unit(&mut g) {
        return;
    }
    if dot(&a, &g) < 0.0 {
        g = neg(&g);
    }
    turn_onto(r, &a, &g);
}
/// Turned about `about`, the least, so that one of the axes but `skip` lies along one of the targets.
fn edge_onto(r: &mut Q, skip: usize, about: &V3, t1: &V3, t2: &V3) {
    let mut best = -1.0f32;
    let mut from = [0.0f32; 3];
    let mut onto = [0.0f32; 3];
    for k in 0..3 {
        if k == skip {
            continue;
        }
        let a0 = axis_of(r, k);
        let mut a = mad(&a0, about, -dot(&a0, about));
        if !unit(&mut a) {
            continue;
        }
        for target in [t1, t2] {
            let mut g = mad(target, about, -dot(target, about));
            if !unit(&mut g) {
                continue;
            }
            let c = dot(&a, &g);
            if c.abs() <= best {
                continue;
            }
            best = c.abs();
            from = a;
            onto = if c >= 0.0 { g } else { neg(&g) };
        }
    }
    if best >= 0.0 {
        turn_onto(r, &from, &onto);
    }
}

/// The seat of a thing in the slot's frame: `p`, `q` where it is in the slot when the hand takes it; `up` the world's up
/// in the hand's frame (None: unknown). Returns the seat and the grasp it is seated for; None when the palm is not
/// measured yet.
pub(super) unsafe fn seat(side: usize, shape: i32, half: &V3, p: &V3, q: &Q, up: Option<&V3>, ends: i32, part: bool, lathe: Option<&GraspLathe>, mesh_hash: u64) -> Option<(Pose, Kind)> {
    let st = lock(side);
    if !st.palm {
        return None;
    }
    let pm = &st.pm;
    let mut item = Pose { p: *p, q: *q };
    qnorm(&mut item.q);
    // in the hand's frame
    let held = compose(&pm.slot, &item);
    let (kind, axis) = if precision_stem(shape, half, ends, part) {
        (Kind::Tripod, 2)
    } else {
        approach(shape, half, &held, pm, up, Some(&mut *(&raw mut CyberpunkVR_DebugGraspApproach[side])), ends, part)
    };
    let mut r = held.q;
    let pos: V3;
    match kind {
        Kind::Wrap => {
            let mut w = [0.0f32; 3];
            for k in 0..3 {
                w[k] = pm.t[k] * cos(WRAP_TILT) + pm.d[k] * sin(WRAP_TILT);
            }
            unit(&mut w);
            let a = axis_of(&r, axis);
            let to = if dot(&a, &w) >= 0.0 { w } else { neg(&w) };
            turn_onto(&mut r, &a, &to);
            if shape == K_BOX {
                // A bar's flat side on the palm: its thinner cross axis onto the normal, about its length.
                let mut thin: i32 = -1;
                for k in 0..3 {
                    if k != axis && (thin < 0 || half[k] < half[thin as usize]) {
                        thin = k as i32;
                    }
                }
                align_about(&mut r, thin as usize, &to, &pm.n);
            }
            // A can's axis a little behind the knuckles; a flat bar's front edge just behind them, so the fingers fold
            // over it from the knuckles.
            let back = if shape == K_BOX { max(WRAP_BACK, support(shape, half, &r, &pm.d) - 0.004) } else { WRAP_BACK };
            let c = mad(&pm.skin, &pm.d, pm.half_d - back);
            let rel = sub(&held.p, &c);
            let length = if shape == K_BOX { half[axis] } else { half[2] };
            let room = max(0.0, length - 0.035);
            let mut along = clamp(dot(&rel, &w), -room, room);
            // Its end on the thumb's side out past the index finger, so that the thumb closes round its side and not
            // over its end: a can stands out of the top of the hand.
            let idx = sub(&pm.index, &c);
            along = max(along, min(dot(&idx, &w) + WRAP_OVERHANG - length, length));
            let at = mad(&c, &w, along);
            pos = mad(&at, &pm.n, support(shape, half, &r, &pm.n));
        }
        Kind::Edge => {
            // The thin axis square to the knuckles' line in the palm's plane, the longest out of the palm, the least
            // turns; the cover EDGE_GAP behind the knuckles' joints, so that each finger folds onto it, the edge on the
            // palm, its middle at the knuckles' middle.
            let line0 = sub(&pm.index, &pm.pinky);
            let mut line = mad(&line0, &pm.n, -dot(&line0, &pm.n));
            if !unit(&mut line) {
                line = pm.t;
            }
            let mut c = cross(&pm.n, &line);
            if dot(&c, &pm.d) < 0.0 {
                c = neg(&c);
            }
            unit(&mut c);
            let a = axis_of(&r, axis);
            let to = if dot(&a, &c) >= 0.0 { c } else { neg(&c) };
            turn_onto(&mut r, &a, &to);
            let mut longest: i32 = -1;
            for k in 0..3 {
                if k != axis && (longest < 0 || half[k] > half[longest as usize]) {
                    longest = k as i32;
                }
            }
            align_about(&mut r, longest as usize, &to, &pm.n);
            let rel0 = sub(&pm.knuckle, &pm.skin);
            let rel = mad(&rel0, &pm.n, -dot(&rel0, &pm.n));
            let mut at = [0.0f32; 3];
            for k in 0..3 {
                at[k] = pm.skin[k] + rel[k];
            }
            at = mad(&at, &c, -(EDGE_GAP + support(shape, half, &r, &c)));
            // its edge off the palm: held between the pads
            pos = mad(&at, &pm.n, support(shape, half, &r, &pm.n) + EDGE_LIFT);
        }
        Kind::Ball => {
            let radius = half[0];
            let at = mad(&pm.skin, &pm.d, if radius < WRAP_RADIUS { pm.half_d - WRAP_BACK } else { 0.008 });
            pos = mad(&at, &pm.n, radius + if radius < WRAP_RADIUS { 0.0 } else { PAD_RISE });
        }
        Kind::Pinch => {
            let mut thin = 0usize;
            for k in 1..3 {
                if half[k] < half[thin] {
                    thin = k;
                }
            }
            let a = axis_of(&r, thin);
            let to = if dot(&a, &pm.pinch_n) >= 0.0 { pm.pinch_n } else { neg(&pm.pinch_n) };
            turn_onto(&mut r, &a, &to);
            edge_onto(&mut r, thin, &pm.pinch_n, &pm.d, &pm.t);
            pos = mad(&pm.pinch, &pm.pinch_n, support(shape, half, &r, &pm.pinch_n));
        }
        Kind::Tripod => {
            // Its thinnest way between the pads and the thumb, its long side across the two fingers; a rod across them.
            if shape == K_BOX {
                let sideways = side_pinch(shape, half);
                let at = if sideways { pm.side } else { pm.tri };
                let at_n = if sideways { pm.side_n } else { pm.tri_n };
                let a = axis_of(&r, axis);
                let to = if dot(&a, &at_n) >= 0.0 { at_n } else { neg(&at_n) };
                turn_onto(&mut r, &a, &to);
                let mut longest: i32 = -1;
                for k in 0..3 {
                    if k != axis && (longest < 0 || half[k] > half[longest as usize]) {
                        longest = k as i32;
                    }
                }
                let longest = longest as usize;
                align_about(&mut r, longest, &at_n, &pm.t);
                // A bigger thing held near its edge, the rest of it out of the palm and out to the thumb's side: the pads
                // TRI_INSET in from that edge, its end on the little finger's side over the middle finger, so the ring
                // and little finger curl free under it.
                let away = 3 - axis - longest;
                let mut at_pos = mad(&at, &at_n, support(shape, half, &r, &at_n));
                if sideways {
                    let mut u = axis_of(&r, away);
                    if dot(&u, &pm.n) < 0.0 {
                        u = neg(&u);
                    }
                    at_pos = mad(&at_pos, &u, half[away] - TRI_INSET);
                    let mut w = axis_of(&r, longest);
                    let to_index = sub(&pm.index, &pm.pinky);
                    if dot(&w, &to_index) < 0.0 {
                        w = neg(&w);
                    }
                    at_pos = mad(&at_pos, &w, max(0.0, half[longest] - TRI_SIDE));
                }
                pos = at_pos;
            } else {
                if shape != K_SPHERE {
                    let a = axis_of(&r, 2);
                    let to = if dot(&a, &pm.t) >= 0.0 { pm.t } else { neg(&pm.t) };
                    turn_onto(&mut r, &a, &to);
                }
                pos = mad(&pm.tri, &pm.tri_n, support(shape, half, &r, &pm.tri_n));
            }
        }
        Kind::Top | Kind::Cradle => {
            // Its axis along the palm's normal, the end the hand came to toward the palm; a bar's faces along the
            // fingers.
            let a = axis_of(&r, axis);
            let rel = sub(&pm.skin, &held.p);
            let end = if dot(&rel, &a) >= 0.0 { a } else { neg(&a) };
            let to = neg(&pm.n);
            turn_onto(&mut r, &end, &to);
            if shape == K_BOX {
                edge_onto(&mut r, axis, &pm.n, &pm.d, &pm.t);
            }
            let length = if shape == K_BOX { half[axis] } else { half[2] };
            // From above: the end a little under the palm, toward the fingers, for the fingertips to go round its rim.
            // From below: standing on the palm's middle.
            let at = if kind == Kind::Top { mad(&pm.skin, &pm.d, TOP_AHEAD) } else { pm.skin };
            pos = mad(&at, &pm.n, length + if kind == Kind::Top { TOP_GAP } else { CRADLE_RISE });
        }
        Kind::Flat => {
            // The face most turned to the palm onto it, the least turn; then an edge along the fingers or across them.
            let mut best = [0.0f32; 3];
            let mut face = 0usize;
            for k in 0..3 {
                let a = axis_of(&r, k);
                if k == 0 || dot(&a, &pm.n).abs() > dot(&best, &pm.n).abs() {
                    face = k;
                    best = a;
                }
            }
            let to = if dot(&best, &pm.n) >= 0.0 { pm.n } else { neg(&pm.n) };
            turn_onto(&mut r, &best, &to);
            edge_onto(&mut r, face, &pm.n, &pm.d, &pm.t);
            // Where the hand took it, the palm's centre kept on the face.
            let rel = sub(&held.p, &pm.skin);
            let room_d = max(0.0, support(shape, half, &r, &pm.d) - FLAT_MARGIN);
            let room_t = max(0.0, support(shape, half, &r, &pm.t) - FLAT_MARGIN);
            let mut at = mad(&pm.skin, &pm.d, clamp(dot(&rel, &pm.d), -room_d, room_d));
            at = mad(&at, &pm.t, clamp(dot(&rel, &pm.t), -room_t, room_t));
            // Its far edge where the fingers reach over it: held with that edge just past the knuckles, the fingers
            // hooked round it, their pads on the side face. Not if that took the face off the palm's centre.
            {
                let off = sub(&at, &pm.skin);
                let sd = support(shape, half, &r, &pm.d);
                let ki = sub(&pm.index, &pm.skin);
                let knuckle = dot(&ki, &pm.d);
                let edge = dot(&off, &pm.d) + sd;
                let over = edge - knuckle;
                if over > FLAT_HOOK && over < FLAT_REACH {
                    let shift = FLAT_HOOK - over;
                    if dot(&off, &pm.d) + shift + sd >= FLAT_MARGIN && dot(&off, &pm.d) + shift - sd <= -FLAT_MARGIN {
                        at = mad(&at, &pm.d, shift);
                    }
                }
            }
            pos = mad(&at, &pm.n, support(shape, half, &r, &pm.n) + FLAT_RISE);
        }
    }
    let mut pos = pos;
    // A part wrapped (a bottle's neck) has the whole thing round it: the palm put to the neck would be in the shoulder
    // where the hand is wider than the neck is long. Off the palm, along its normal, until the palm's skin and the
    // knuckles (a proximal bone's radius round them) are out of the whole -- a neck is held in the fingers.
    if let Some(lathe) = lathe.filter(|l| l.knots >= 2 && kind == Kind::Wrap) {
        for _ in 0..4 {
            let mut worst = 0.0f32;
            let mut at = |pt: &V3, clear: f32| {
                let l = unrot(&r, &sub(pt, &pos));
                worst = min(worst, lathe_sdf(lathe, &l) - clear);
            };
            for i in -2..=2 {
                for j in -1..=1 {
                    let pt = mad(&mad(&pm.skin, &pm.d, 0.45 * i as f32 * pm.half_d), &pm.t, 0.9 * j as f32 * pm.half_t);
                    at(&pt, 0.0);
                }
            }
            for j in -2..=2 {
                let pt = mad(&pm.knuckle, &pm.t, 0.5 * j as f32 * pm.half_t);
                at(&pt, FINGERS[0].radius[0]);
            }
            if worst >= -0.0005 {
                break;
            }
            pos = mad(&pos, &pm.n, -worst + 0.001);
        }
    }
    // The C++ fitter jointly checked this seat and the solved fingers against the visible FPP skin. Fingers are still
    // solved for the current rig.
    apply_seat_fit(mesh_hash, side, shape, kind, ends, part, half, pm, &mut pos, &mut r);
    let mut out = POSE0;
    out.p = to_frame(&pm.slot, &pos);
    out.q = qmul(&conj(&pm.slot.q), &r);
    qnorm(&mut out.q);
    Some((out, kind))
}

/// The slot a held thing hangs from back at the rig's reference pose under the hand.
unsafe fn pin_slot(buf: *mut u8, slot: i32) {
    let mut p = [0.0f32; 3];
    let mut q = [0.0f32; 4];
    if !VRIK_ReferenceLocal(slot, p.as_mut_ptr(), q.as_mut_ptr()) {
        return;
    }
    put3(bt(buf, slot), &p);
    put4(br(buf, slot), &q);
}

/// GraspFingers: each player pose pass, both palms measured, and the grasp of a hand that holds something written (a
/// person's part eased in and out, its fingers eased from one solve to the next).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_grasp_fingers(buf: *mut u8) {
    if buf.is_null() || g_VRBoneCount <= 0 {
        return;
    }
    for side in 0..2i32 {
        let hand = if side != 0 { g_VRRightBoneIdx } else { g_VRLeftBoneIdx };
        let slot_bone = if side != 0 { g_VRSmokeCigIdx } else { g_VRSmokeLighterIdx };
        if !valid_bone(hand) || !valid_bone(slot_bone) {
            continue;
        }
        let sd = side as usize;
        note_up(buf, sd, hand);
        let holding = {
            let st = lock(sd);
            st.active && st.pin
        };
        if holding {
            pin_slot(buf, slot_bone);
        }
        let mut pm = PALM0;
        if !measure_palm(buf, side, hand, slot_bone, &mut pm) {
            continue;
        }
        let mut guard = lock(sd);
        let st = &mut *guard;
        st.pm = pm;
        st.palm = true;
        CyberpunkVR_DebugGrasp[2] = CyberpunkVR_DebugGrasp[2].wrapping_add(1);
        {
            let v = [to_frame(&pm.slot, &pm.skin), unrot(&pm.slot.q, &pm.n), unrot(&pm.slot.q, &pm.d), unrot(&pm.slot.q, &pm.t)];
            for i in 0..4 {
                for k in 0..3 {
                    CyberpunkVR_DebugGraspPalm[sd][i * 3 + k] = v[i][k];
                }
            }
        }
        if !st.active {
            continue;
        }
        if st.solved != st.rev {
            solve(buf, side, hand, st);
            st.solved = st.rev;
        }
        if st.person {
            let now = now_s();
            let dt = if st.eased_at > 0.0 { (now - st.eased_at).clamp(0.0, 0.1) as f32 } else { 0.0 };
            st.eased_at = now;
            st.blend = if st.person_on { min(1.0, st.blend + dt / PERSON_IN) } else { max(0.0, st.blend - dt / PERSON_OUT) };
            if !st.person_on && st.blend <= 0.0 {
                st.active = false;
                st.person = false;
                st.shown_on = false;
                continue;
            }
            // the fingers drawn eased toward the last solve (the first is taken as it is)
            let k = if st.shown_on { 1.0 - cvr_body::cmath::exp(-dt / PERSON_TURN_TAU) } else { 1.0 };
            for s in 0..SLOTS {
                if !st.have[s] {
                    continue;
                }
                let t = st.rot[s];
                let a = &mut st.shown[s];
                if k >= 0.999 {
                    *a = t;
                    continue;
                }
                let w = if a[0] * t[0] + a[1] * t[1] + a[2] * t[2] + a[3] * t[3] < 0.0 { -k } else { k };
                for c in 0..4 {
                    a[c] = a[c] * (1.0 - k) + t[c] * w;
                }
                qnorm(a);
            }
            st.shown_on = true;
        }
        let b = st.blend;
        if b <= 0.001 {
            continue;
        }
        for s in 0..SLOTS {
            if !st.have[s] {
                continue;
            }
            let bone = VRIK_LadderFingerBone(side, s as i32);
            if !valid_bone(bone) {
                continue;
            }
            let q = br(buf, bone);
            let t = if st.person { st.shown[s] } else { st.rot[s] };
            if b >= 0.999 {
                put4(q, &t);
                continue;
            }
            let cur = a4(q);
            let d = cur[0] * t[0] + cur[1] * t[1] + cur[2] * t[2] + cur[3] * t[3];
            let w = if d < 0.0 { -b } else { b };
            let mut m = [0.0f32; 4];
            for k in 0..4 {
                m[k] = cur[k] * (1.0 - b) + t[k] * w;
            }
            qnorm(&mut m);
            put4(q, &m);
        }
        CyberpunkVR_DebugGrasp[1] = CyberpunkVR_DebugGrasp[1].wrapping_add(1);
    }
}

/// GraspHolding: whether a hand (0 left, 1 right) holds a thing its fingers are closed on.
pub fn grasp_holding(side: usize) -> bool {
    side <= 1 && lock(side).active
}
#[no_mangle]
pub extern "C" fn cvr_anim_grasp_holding(side: i32) -> bool {
    (0..=1).contains(&side) && grasp_holding(side as usize)
}

/// GraspTake: a hand (0 left, 1 right) takes a thing -- `shape` 0 box, 1 sphere, 2 cylinder, 3 cone, `half` its half
/// sizes, at `p`, `q` (x y z w) in the HAND bone's frame -- the way it came to it: where the thing lies for its grasp,
/// in the hand bone's frame, and the grasp's kind; its fingers close on it from the next pass, at the blend grasp_ease
/// gives. `ends` and `part` are a cylinder's (grasp_part_of), `lathe` what the fingers rest on. None when the palm is not
/// measured yet.
pub fn grasp_take(side: usize, shape: i32, half: &V3, p: &V3, q: &Q, ends: i32, part: bool, lathe: Option<&GraspLathe>, mesh_hash: u64) -> Option<(V3, Q, i32)> {
    if side > 1 || !(K_BOX..=K_CONE).contains(&shape) {
        return None;
    }
    let slot = {
        let st = lock(side);
        if !st.palm {
            return None;
        }
        st.pm.slot
    };
    // the hand's frame to the slot's, the seat, and back
    let mut in_hand = Pose { p: *p, q: *q };
    qnorm(&mut in_hand.q);
    let mut in_slot = Pose { p: to_frame(&slot, &in_hand.p), q: qmul(&conj(&slot.q), &in_hand.q) };
    qnorm(&mut in_slot.q);
    let up = up_in_hand(side);
    // SAFETY: the debug read-out written under the side's lock, as main's.
    let (seat_pose, kind) = unsafe { seat(side, shape, half, &in_slot.p, &in_slot.q, up.as_ref(), ends, part, lathe, mesh_hash) }?;
    let back = compose(&slot, &seat_pose);
    let mut st = lock(side);
    // a baked grasp is only the hold grasp_take_baked began
    st.req.baked = false;
    st.req.shape = shape;
    st.req.half = *half;
    st.req.obj = seat_pose;
    st.req.ends = ends;
    st.req.part = part;
    st.req.lathe = lathe.filter(|l| l.knots >= 2).copied().unwrap_or_default();
    // a thing's own grasp: none of a person's part's forced kind, limit or spread left from the last hold
    st.req.force = -1;
    st.req.limit = 1.0;
    st.req.spread = -1.0;
    st.active = true;
    st.pin = true;
    st.rev = st.rev.wrapping_add(1);
    Some((back.p, back.q, kind as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seat_fits_parse() {
        let rows = SEAT_FITS_TEXT.lines().filter(|l| l.trim_start().starts_with('{')).count();
        assert_eq!(seat_fits().len(), rows);
    }
}
