//! The part a hand closes on of a round long thing, the grasp's ease and its end, and A PERSON'S PART in a physics hand
//! (main's GraspPose.cpp: GraspPartOf, GraspEase, GraspLetGo, PersonSeat, PersonOnPalm, GraspPerson).
use super::*;
use cvr_body::cmath::acos;
use cvr_host::log;

/// THE PART A HAND CLOSES ON, of a round long thing given by the points of its hull (or of its drawn mesh's profile):
/// `points` x y z each, in its own frame (the axis Z through the origin, `half_length` along it), the hand at `hand`.
/// The radius round the axis is read in sixteen slices along it (an empty slice is the straight line between the slices
/// round it); a NECK is a run of slices under 62% of the widest at one end, 2 cm long at least and no more than half the
/// thing, while the other end's three slices keep over 85%. Its SHOULDER is where, coming from the neck's end, the radius
/// first passes 85% of the widest. The neck is then a cylinder from its end to the shoulder, of the radius of its end's
/// two slices, and the body a cylinder from the other end to the shoulder, of the widest radius. A hand past the middle
/// of the shoulder's slope, toward the neck, closes on the neck; any other on the body. An internal STEM (a wine glass's:
/// wider at both ends) is a part of its own. False: no neck, the whole thing (whose lathe is still given when its profile
/// could be read: out.lathe.knots > 0).
pub fn grasp_part_of(points: &[f32], half_length: f32, hand: &V3, out: &mut GraspPart) -> bool {
    const SLICES: usize = 16;
    *out = GraspPart::default();
    let n = points.len() / 3;
    if n < 6 || !(half_length > 0.01) || !half_length.is_finite() || !hand[0].is_finite() || !hand[1].is_finite() || !hand[2].is_finite() {
        return false;
    }
    let mut r = [-1.0f32; SLICES];
    let w = 2.0 * half_length / SLICES as f32;
    for i in 0..n {
        let p = &points[3 * i..3 * i + 3];
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return false;
        }
        let k = clamp((p[2] + half_length) / w, 0.0, (SLICES - 1) as f32) as usize;
        r[k] = max(r[k], sqrt(p[0] * p[0] + p[1] * p[1]));
    }
    let (mut first, mut last) = (-1i32, -1i32);
    for k in 0..SLICES {
        if r[k] >= 0.0 {
            if first < 0 {
                first = k as i32;
            }
            last = k as i32;
        }
    }
    if first < 0 {
        return false;
    }
    let (first, last) = (first as usize, last as usize);
    for k in 0..first {
        r[k] = r[first];
    }
    for k in last + 1..SLICES {
        r[k] = r[last];
    }
    for k in first + 1..last {
        if r[k] >= 0.0 {
            continue;
        }
        let mut b = k + 1;
        while r[b] < 0.0 {
            b += 1;
        }
        let a = k - 1;
        r[k] = r[a] + (r[b] - r[a]) * (k - a) as f32 / (b - a) as f32;
    }
    let widest = r.iter().copied().fold(0.0f32, max);
    if !(widest > 0.005) {
        return false;
    }
    // the whole thing as a lathe: its ends, and each slice's radius at the slice's middle
    out.lathe.knots = SLICES as i32 + 2;
    out.lathe.z[0] = -half_length;
    out.lathe.r[0] = r[0];
    for k in 0..SLICES {
        out.lathe.z[k + 1] = -half_length + w * (k as f32 + 0.5);
        out.lathe.r[k + 1] = r[k];
    }
    out.lathe.z[SLICES + 1] = half_length;
    out.lathe.r[SLICES + 1] = r[SLICES - 1];
    {
        // a knot on the straight line between the last one kept and the next (within half a millimetre) is dropped: a
        // bottle's body is one straight run, and the fingers' distances to the lathe are read thousands of times a take
        let l = &mut out.lathe;
        let knots = l.knots as usize;
        let mut kept = 1usize;
        for k in 1..knots - 1 {
            let t = (l.z[k] - l.z[kept - 1]) / max(1e-6, l.z[k + 1] - l.z[kept - 1]);
            let line = l.r[kept - 1] + (l.r[k + 1] - l.r[kept - 1]) * t;
            if (l.r[k] - line).abs() <= 0.0005 {
                continue;
            }
            l.z[kept] = l.z[k];
            l.r[kept] = l.r[k];
            kept += 1;
        }
        l.z[kept] = l.z[knots - 1];
        l.r[kept] = l.r[knots - 1];
        l.knots = kept as i32 + 1;
    }
    out.z = 0.0;
    // An internal stem has wider material at both ends. The old end-only neck test missed wine glasses and seated the
    // whole bowl as one cylinder.
    let stem_limit = min(0.01, 0.25 * widest);
    let mut begin = 1usize;
    while begin + 1 < SLICES {
        if r[begin] > stem_limit {
            begin += 1;
            continue;
        }
        let mut end = begin;
        let mut radius = r[begin];
        while end + 1 < SLICES && r[end + 1] <= stem_limit {
            end += 1;
            radius = max(radius, r[end]);
        }
        if end + 1 >= SLICES {
            break;
        }
        let low = -half_length + w * (begin as f32 + 0.5);
        let high = -half_length + w * (end as f32 + 0.5);
        let shoulders = r[begin - 1] > max(0.012, 2.5 * radius) && r[end + 1] > max(0.012, 2.5 * radius);
        if shoulders && high - low >= 0.025 && hand[2] >= low - 0.015 && hand[2] <= high + 0.015 {
            out.z = 0.5 * (low + high);
            out.half = 0.5 * (high - low);
            out.radius = radius;
            out.ends = 0;
            out.neck = false;
            out.stem = true;
            for k in 0..out.lathe.knots as usize {
                out.lathe.z[k] -= out.z;
            }
            return true;
        }
        begin = end + 1;
    }
    // the neck's end: +1 the +Z end, -1 the -Z end; its thin run's slices
    let (mut s, mut thin) = (0i32, 0usize);
    let mut end = 1i32;
    while end >= -1 && s == 0 {
        let mut run = 0usize;
        while run < SLICES && r[if end > 0 { SLICES - 1 - run } else { run }] < 0.62 * widest {
            run += 1;
        }
        let other = if end > 0 { max(r[0], max(r[1], r[2])) } else { max(r[SLICES - 1], max(r[SLICES - 2], r[SLICES - 3])) };
        if run >= 1 && run as f32 * w >= 0.02 && run <= SLICES / 2 && other > 0.85 * widest {
            s = end;
            thin = run;
        }
        end -= 2;
    }
    if s == 0 {
        return false;
    }
    // from the neck's end: the shoulder, the first slice past 85% of the widest
    let mut shoulder = -1i32;
    for i in 0..SLICES {
        let k = if s > 0 { SLICES - 1 - i } else { i };
        if r[k] >= 0.85 * widest {
            shoulder = k as i32;
            break;
        }
    }
    if shoulder < 0 {
        return false;
    }
    let (mouth, next) = if s > 0 { (SLICES - 1, SLICES - 2) } else { (0, 1) };
    let neck_r = max(0.008, max(r[mouth], r[next]));
    // the shoulder's z (its slice's edge on the neck's side), and where its slope is half way down to the neck
    let z_shoulder = -half_length + w * (if s > 0 { shoulder + 1 } else { shoulder }) as f32;
    let mut slope = shoulder;
    for i in 0..SLICES as i32 {
        let k = shoulder + s * i;
        if k < 0 || k >= SLICES as i32 {
            break;
        }
        if r[k as usize] <= 0.5 * (widest + neck_r) {
            slope = k;
            break;
        }
    }
    let z_slope = -half_length + w * (slope as f32 + 0.5);
    let z_end = s as f32 * half_length;
    let z_base = -(s as f32) * half_length;
    // the neck the palm is put to: its thin run from the mouth (the lathe has the shoulder beyond it, for the fingers)
    let z_thin = z_end - s as f32 * w * thin as f32;
    if s as f32 * (hand[2] - z_slope) >= 0.0 {
        out.z = 0.5 * (z_end + z_thin);
        out.half = 0.5 * (z_end - z_thin).abs();
        out.radius = neck_r;
        // over the mouth only; never stood on the neck
        out.ends = if s > 0 { 2 } else { 1 };
        out.neck = true;
    } else {
        out.z = 0.5 * (z_base + z_shoulder);
        out.half = 0.5 * (z_shoulder - z_base).abs();
        out.radius = widest;
        // its base: from above or stood on the palm; not its shoulder
        out.ends = if s > 0 { 1 | 4 } else { 2 | 8 };
        out.neck = false;
    }
    // into the part's frame
    for k in 0..out.lathe.knots as usize {
        out.lathe.z[k] -= out.z;
    }
    out.half > 0.005
}

/// GraspEase: how far the fingers are on the grasp: 0 tracking, 1 the grasp.
pub fn grasp_ease(side: usize, blend: f32) {
    if side > 1 {
        return;
    }
    lock(side).blend = clamp(blend, 0.0, 1.0);
}
/// GraspLetGo: the hand holds nothing: its fingers are the tracking's again.
pub fn grasp_let_go(side: usize) {
    if side > 1 {
        return;
    }
    let mut st = lock(side);
    st.active = false;
    st.person = false;
    st.person_on = false;
    st.shown_on = false;
    st.blend = 0.0;
}

fn dot4(a: &Q, b: &Q) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}
/// THE SEAT OF A PERSON'S PART (its capsule's pose in the hand bone's frame, the axis z, `half` its segment's half
/// length):
///  - held round it (styles 1-3): as a can -- its axis turned (SEAT_TURN_ROUND at most) onto the palm's width tilted
///    toward the fingers, through a point a little behind the knuckles, its surface on the palm's pads; its place along
///    its axis kept. Not when its axis points into the palm (an end of it): None, the palm instead;
///  - the palm (4): the hand turned (SEAT_TURN_PALM at most) for the palm's normal to point at the part's axis, its skin
///    SEAT_PALM_GAP off the part's surface.
fn person_seat(pm: &Palm, style: i32, cap: &Pose, radius: f32, half: f32) -> Option<Pose> {
    let mut out = *cap;
    let ez = [0.0f32, 0.0, 1.0];
    let a = rot(&cap.q, &ez);
    if style != 4 {
        if dot(&a, &pm.n).abs() > 0.6 {
            return None;
        }
        let thick = clamp((radius - PART_THIN) / (PART_THICK - PART_THIN), 0.0, 1.0);
        let tilt = WRAP_TILT + (PART_TILT_THICK - WRAP_TILT) * thick;
        let mut w = [0.0f32; 3];
        for k in 0..3 {
            w[k] = pm.t[k] * cos(tilt) + pm.d[k] * sin(tilt);
        }
        unit(&mut w);
        if dot(&a, &w) < 0.0 {
            w = neg(&w);
        }
        let mut about = cross(&a, &w);
        let angle = acos(clamp(dot(&a, &w), -1.0, 1.0));
        if unit(&mut about) && angle > 1e-3 {
            let turn = axis_angle(&about, min(angle, SEAT_TURN_ROUND));
            out.q = qmul(&turn, &cap.q);
            qnorm(&mut out.q);
        }
        let a2 = rot(&out.q, &ez);
        let c0 = mad(&pm.skin, &pm.d, pm.half_d - WRAP_BACK);
        // Seat on the real palm pads, not the bone-plane skin estimate through the thenar/web.
        let c0 = mad(&c0, &pm.n, radius + FLAT_RISE);
        let rel = sub(&cap.p, &c0);
        out.p = mad(&c0, &a2, dot(&rel, &a2));
        return Some(out);
    }
    // the way from the palm's skin to the part's axis
    let rel = sub(&pm.skin, &cap.p);
    let on_axis = mad(&cap.p, &a, clamp(dot(&rel, &a), -half, half));
    let mut v = sub(&on_axis, &pm.skin);
    if !unit(&mut v) {
        return None;
    }
    // the hand turned about the skin by r (n onto v): the part, in the turned hand, turned back by it about the skin
    let mut about = cross(&pm.n, &v);
    let angle = acos(clamp(dot(&pm.n, &v), -1.0, 1.0));
    let mut r = ID;
    if unit(&mut about) && angle > 1e-3 {
        r = axis_angle(&about, min(angle, SEAT_TURN_PALM));
    }
    let ri = conj(&r);
    let off = sub(&cap.p, &pm.skin);
    let back = rot(&ri, &off);
    for k in 0..3 {
        out.p[k] = pm.skin[k] + back[k];
    }
    out.q = qmul(&ri, &cap.q);
    qnorm(&mut out.q);
    // then along the palm's normal, the skin SEAT_PALM_GAP off its surface
    let a2 = rot(&out.q, &ez);
    let rel = sub(&pm.skin, &out.p);
    let on_axis = mad(&out.p, &a2, clamp(dot(&rel, &a2), -half, half));
    let v = sub(&on_axis, &pm.skin);
    out.p = mad(&out.p, &pm.n, radius + SEAT_PALM_GAP - dot(&v, &pm.n));
    Some(out)
}
/// ON THE PALM ALONG ITS NORMAL (grasp_person's palm_gap): the part moved across its own axis only, for the point of its
/// axis nearest the palm's skin to lie `radius + gap` out along the palm's normal from that skin. Its way and its place
/// along it are kept, and there is no limit to how far: the caller placed it by an estimate of the palm, this is the
/// palm itself.
fn person_on_palm(pm: &Palm, cap: &Pose, radius: f32, half: f32, gap: f32) -> Option<Pose> {
    let mut out = *cap;
    let a = rot(&cap.q, &[0.0, 0.0, 1.0]);
    let rel = sub(&pm.skin, &cap.p);
    let on_axis = mad(&cap.p, &a, clamp(dot(&rel, &a), -half, half));
    let want = mad(&pm.skin, &pm.n, radius + gap);
    let mv = sub(&want, &on_axis);
    let along = dot(&mv, &a);
    let mv = mad(&mv, &a, -along);
    for k in 0..3 {
        out.p[k] = cap.p[k] + mv[k];
    }
    (out.p[0] + out.p[1] + out.p[2]).is_finite().then_some(out)
}

/// GraspPerson with the palm's gap at its default: seated as the grasp wants it.
pub fn grasp_person(side: usize, style: i32, p: Option<&V3>, q: Option<&Q>, half_length: f32, radius: f32) -> Option<(V3, Q, bool)> {
    grasp_person_gap(side, style, p, q, half_length, radius, -1.0)
}
/// A PHYSICS HAND HOLDS A PERSON'S PART (any thread; None: not now, the pose pass busy or a thing held): the part's
/// capsule -- its segment along its own Z, `half_length` each way, `radius` -- at `p`, `q` (x y z w) in the HAND bone's
/// frame, closed on by `style`: 1 a can (WRAP round the capsule, every joint as far as it goes), 2 a can with the fingers
/// spread wide (a neck), 3 gently (WRAP to 60 % of the range, a little spread), 4 the palm (FLAT, the fingers lying on
/// it to 60 %). As the hold is taken the part is SEATED for that grasp -- the returned bool true, the pose where the part
/// should lie in the hand (the same frame): the caller moves the hand so. Given every step after: solved again only once
/// the part has been 2.5 cm or 20 degrees off its seat for half a second, the fingers eased from one solve to the next.
/// Style 0: let go, eased out. palm_gap < 0: seated as the grasp wants it (turned for it, 6 cm at the most); palm_gap >=
/// 0: under the palm (4) along the palm's normal, its surface palm_gap off the palm's skin.
pub fn grasp_person_gap(side: usize, style: i32, p: Option<&V3>, q: Option<&Q>, half_length: f32, radius: f32, palm_gap: f32) -> Option<(V3, Q, bool)> {
    let asked = style;
    let mut style = style;
    if side > 1 {
        return None;
    }
    let mut guard = match SIDES[side].try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        // the pose pass solving: the next step
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    let st = &mut *guard;
    let (Some(p), Some(q)) = (p.filter(|_| style > 0), q) else {
        if st.person && st.person_on {
            st.person_on = false;
            log!(
                "[grasp] {} hand lets go of a person's part ({} solves, {} of them as it moved)\n",
                if side != 0 { "right" } else { "left" },
                st.taken + st.resolved,
                st.resolved
            );
        }
        return Some(([0.0; 3], ID, false));
    };
    // a thing's grasp
    if st.active && !st.person {
        return None;
    }
    if !st.palm {
        return None;
    }
    let mut in_hand = Pose { p: *p, q: *q };
    qnorm(&mut in_hand.q);
    let slot = st.pm.slot;
    let to_slot = |h: &Pose| {
        let mut o = Pose { p: to_frame(&slot, &h.p), q: qmul(&conj(&slot.q), &h.q) };
        qnorm(&mut o.q);
        o
    };
    let mut in_slot = to_slot(&in_hand);
    let now = now_s();
    let fresh = !st.person || !st.person_on || st.style != style;
    let mut seat_out = (in_hand.p, in_hand.q, false);
    if !fresh {
        // off its seat (the hold keeps it there: only a part that went somewhere else is solved again, where it is now)
        // by its geometry: its axis's way (either sense) and how far it is across it; along it only past 5 cm
        let ez = [0.0f32, 0.0, 1.0];
        let d = sub(&in_slot.p, &st.req.obj.p);
        let a0 = rot(&st.req.obj.q, &ez);
        let a1 = rot(&in_slot.q, &ez);
        let along = dot(&d, &a0);
        let across = mad(&d, &a0, -along);
        let moved = len(&across) + max(0.0, along.abs() - 0.05);
        let turned = acos(min(1.0, dot(&a0, &a1).abs()));
        if moved < PERSON_MOVED && turned < PERSON_TURNED {
            // the time it has been off its seat counts from here
            st.solved_at = now;
            return Some(seat_out);
        }
        if now - st.solved_at < PERSON_EVERY {
            return Some(seat_out);
        }
        st.resolved += 1;
    } else {
        st.taken += 1;
        // seated as the grasp wants it, the physics hand moved to it (the caller); a part held round by an end of it is
        // under the palm instead
        let mut as_style = style;
        // a part held round (an arm, a leg, a neck) lies across the palm, the fingers round it: on the palm instead, the
        // hand only lay on a limb (2026-11-11)
        let seat = if palm_gap >= 0.0 && style == 4 {
            person_on_palm(&st.pm, &in_hand, radius, half_length, palm_gap)
        } else {
            match person_seat(&st.pm, as_style, &in_hand, radius, half_length) {
                Some(s) => Some(s),
                None if as_style != 4 => {
                    as_style = 4;
                    if palm_gap >= 0.0 {
                        person_on_palm(&st.pm, &in_hand, radius, half_length, palm_gap)
                    } else {
                        person_seat(&st.pm, as_style, &in_hand, radius, half_length)
                    }
                }
                None => None,
            }
        };
        if let Some(mut seat) = seat {
            // at most SEAT_MOST: the rest of the way not taken (position and turn alike); on the palm along its normal, all
            let t = sub(&seat.p, &in_hand.p);
            let dist = len(&t);
            if palm_gap < 0.0 && dist > SEAT_MOST {
                let k = SEAT_MOST / dist;
                seat.p = mad(&in_hand.p, &t, k);
                let s = if dot4(&in_hand.q, &seat.q) < 0.0 { -1.0 } else { 1.0 };
                for c in 0..4 {
                    seat.q[c] = in_hand.q[c] + (seat.q[c] * s - in_hand.q[c]) * k;
                }
                qnorm(&mut seat.q);
            }
            style = as_style;
            in_hand = seat;
            in_slot = to_slot(&seat);
            seat_out = (seat.p, seat.q, true);
        }
    }
    st.req = REQUEST0;
    // a baked grasp is only the hold grasp_take_baked began
    st.req.baked = false;
    st.req.shape = K_CAPSULE;
    st.req.half = [max(0.005, radius), max(0.005, radius), max(0.0, half_length)];
    st.req.obj = in_slot;
    st.req.part = true;
    match style {
        2 => {
            st.req.force = Kind::Wrap as i32;
            st.req.spread = 0.15;
        }
        3 => {
            st.req.force = Kind::Wrap as i32;
            st.req.limit = 0.6;
            st.req.spread = 0.04;
        }
        4 => {
            st.req.force = Kind::Flat as i32;
            st.req.limit = 0.6;
            st.req.spread = 0.06;
        }
        _ => st.req.force = Kind::Wrap as i32,
    }
    if fresh {
        const NAMES: [&str; 5] = ["", "a can", "a can, the fingers wide", "gently", "the palm"];
        // the palm's skin in the part's frame
        let skin = to_frame(&in_hand, &st.pm.skin);
        log!(
            "[grasp] {} hand closes on a person's part (capsule r {:.3}, {:.3} long, {:.3} from the palm's skin) as {}\n",
            if side != 0 { "right" } else { "left" },
            radius,
            2.0 * half_length,
            sdf(K_CAPSULE, &st.req.half, &skin),
            NAMES[style.clamp(1, 4) as usize]
        );
        if !st.person {
            st.blend = 0.0;
        }
        st.eased_at = 0.0;
    }
    st.style = asked;
    st.person = true;
    st.person_on = true;
    st.active = true;
    st.pin = false;
    st.solved_at = now;
    st.rev = st.rev.wrapping_add(1);
    Some(seat_out)
}
