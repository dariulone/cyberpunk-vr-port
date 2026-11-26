//! HAIR THAT THE HANDS MOVE (cvr-rig's hair_dangle, main's HairDangle.cpp): a person's hair swings by the animation's
//! dangle simulation, and a hand is no bone of that rig, so a hand in someone's hair is given to it as the simulation's
//! external force (world space, felt by every particle, gravity's company): the hair goes the way the hand goes through
//! it (its speed) and away from where the hand is in it (how deep). The force eases in and out over a few frames, and
//! is taken back to nothing the moment no hand is in that hair: the simulation is the hair resource's, worn by everyone
//! who wears that hair.
//!
//! WITH THE PHYSICS HANDS the hand is its palm as drawn, 7.5 cm on from the wrist, and its fingers: each finger two
//! points, its middle joint and its tip, each with its own depth in the hair and its own speed, so fingers combed
//! through it move it and a palm beside it no longer does. WHERE THE HAIR IS (hair_points): the hair dyngs' particles in
//! the head's frame, in chains down the hair, carried by the head slot; a hand is tried against those chains, not a line
//! down the middle of the head (a shaved side has no hair to move). EACH LOCK ITS OWN (hair_strands): each particle
//! weighted by how near a hand's point comes to its piece of lock, and down its lock from there; the others made heavy.
use super::people::{carry_slots, HairChains, NpcRef};
use super::{carry, data, len, mul, qrot, sub, Frame, State, V3};
use cvr_rig::vrik::hair_dangle;
use std::rc::Rc;

/// m: the wrist from the hair's line, how far down, how far back
const HAIR_R: f32 = 0.22;
const HAIR_DOWN: f32 = 0.28;
const HAIR_BACK: f32 = 0.06;
/// m: the palm from the hair's line, the line further back, the palm on from the wrist, down to the nape
const HAIR_R_PALM: f32 = 0.12;
const HAIR_BACK_PALM: f32 = 0.08;
const HAIR_PALM: f32 = 0.075;
const HAIR_DOWN_PALM: f32 = 0.20;
/// m/s^2 per m/s of the hand, at full depth, at most
const HAIR_FOLLOW: f32 = 25.0;
const HAIR_PUSH: f32 = 90.0;
const HAIR_MOST: f32 = 60.0;
const HAIR_EASE: f32 = 0.06;
/// shares of the force: the palm, a finger's middle joint, its tip
const HAIR_W_PALM: f32 = 0.35;
const HAIR_W_MID: f32 = 0.04;
const HAIR_W_TIP: f32 = 0.09;
/// m: a finger's axis from the hair's line
const HAIR_R_FINGER: f32 = 0.115;
/// m: the hair's surface from its bones; the palm, a finger, a wrist (the hand on past it) past its point
const HAIR_BONE_R: f32 = 0.045;
const HAIR_BONE_PALM: f32 = 0.03;
const HAIR_BONE_FINGER: f32 = 0.009;
const HAIR_BONE_WRIST: f32 = 0.10;
/// m: a hand this near the head has its hair carried to it
const HAIR_BONE_NEAR: f32 = 0.50;
/// m, weight
const HAIR_STRAND_SOFT: f32 = 0.03;
const HAIR_STRAND_STEP: f32 = 0.1;

/// One point of a hand this frame (world space), its speed from where it was the frame before.
#[derive(Clone, Copy, Default)]
struct Probe {
    p: V3,
    v: V3,
    w: f32,
    r: f32,
    rc: f32,
    gen: u32,
    set: bool,
}
#[derive(Clone, Copy, Default)]
struct HairHand {
    at: Option<V3>,
    v: V3,
}
#[derive(Default)]
pub(super) struct Hair {
    gen: u32,
    t: f32,
    hands: [HairHand; 2],
    /// each hand's points: 1 the palm, 2 + 2 digit its middle joint, 3 + 2 digit its tip
    probes: [[Probe; 12]; 2],
    /// the people given a force (K.HAIR)
    forced: Vec<NpcRef>,
    say: f32,
}
fn probe(p: &mut Probe, at: V3, w: f32, r: f32, rc: f32, dt: f32, gen: u32) {
    p.v = if p.set && p.gen == gen.wrapping_sub(1) {
        let mut v = mul(&sub(&at, &p.p), 1.0 / dt);
        let s = len(&v);
        if s > 5.0 {
            v = mul(&v, 5.0 / s);
        }
        v
    } else {
        [0.0; 3]
    };
    *p = Probe { p: at, v: p.v, w, r, rc, gen, set: true };
}
/// A point of a hand d from the hair's nearest q: the force it adds, its share `w` of a hand's, away from it into the
/// hair (how deep within r) and the way it goes.
fn push_at(f: &mut V3, x: &V3, q: &V3, d: f32, v: &V3, r: f32, w: f32) {
    if d < r {
        let depth = (r - d) / r;
        if d > 0.005 {
            // a hand just in moves the hair already: the push rises fast at first
            let k = w * HAIR_PUSH * depth.powf(0.6) / d;
            for i in 0..3 {
                f[i] += (q[i] - x[i]) * k;
            }
        }
        let g = w * HAIR_FOLLOW * (0.3 + 0.7 * depth);
        for i in 0..3 {
            f[i] += v[i] * g;
        }
    }
}
/// The nearest point of someone's hair chains to x, and how far; each piece of lock (a particle's, from its parent to
/// it) weighted by how near it comes within reach `rc`.
fn chain_near(ch: &mut HairChains, x: &V3, rc: f32) -> Option<(V3, f32)> {
    let mut best = 1e9f32;
    let mut at = None;
    for i in 0..ch.n {
        let mut q = ch.w[i];
        let j = ch.p[i];
        if j > 0 {
            let a = sub(&ch.w[j - 1], &q);
            let aa = super::len2(&a);
            if aa > 1e-8 {
                let u = (super::dot(&sub(x, &q), &a) / aa).clamp(0.0, 1.0);
                q = super::add(&q, &mul(&a, u));
            }
        }
        let dd = super::len2(&sub(&q, x));
        if dd < best {
            best = dd;
            at = Some(q);
        }
        if dd < (rc + HAIR_STRAND_SOFT) * (rc + HAIR_STRAND_SOFT) {
            let s = (1.0 - (dd.sqrt() - rc) / HAIR_STRAND_SOFT).min(1.0);
            if s > ch.s[i] {
                ch.s[i] = s;
            }
        }
    }
    at.map(|q| (q, best.sqrt()))
}
/// A point in someone's hair: its chains when known (rc how near counts), else the line from c along a (aa its length
/// squared, r): the force it adds, and how far it is from the hair.
#[allow(clippy::too_many_arguments)]
fn push(f: &mut V3, x: &V3, v: &V3, r: f32, rc: f32, w: f32, ch: Option<&mut HairChains>, c: &V3, a: &V3, aa: f32) -> f32 {
    if let Some(ch) = ch {
        let Some((q, d)) = chain_near(ch, x, rc) else { return 9.0 };
        push_at(f, x, &q, d, v, rc, w);
        return d;
    }
    let p = sub(x, c);
    let t = if aa > 1e-6 { (super::dot(&p, a) / aa).clamp(0.0, 1.0) } else { 0.0 };
    let q = super::add(c, &mul(a, t));
    let d = len(&sub(&q, x));
    push_at(f, x, &q, d, v, r, w);
    d
}
/// One person's hair chains (the head's frame), read once; none when they have none, tried again in 3 s (a hair not
/// loaded yet).
fn chains(rec: &mut super::people::NpcRec, t: f32) -> bool {
    match &rec.hair_pts {
        Some(Some(_)) => return true,
        Some(None) if t < rec.hair_retry => return false,
        _ => {}
    }
    let text = hair_dangle::hair_points(rec.e.instance.cast());
    let mut c = HairChains::default();
    for item in text.split(';').filter(|s| !s.is_empty()) {
        let parts: Vec<&str> = item.split(' ').collect();
        // a place that did not read: far off, keeping the others' places
        let num = |k: usize| parts.get(k).and_then(|v| v.parse::<f32>().ok());
        c.l.push([num(0).unwrap_or(1e3), num(1).unwrap_or(1e3), num(2).unwrap_or(1e3)]);
        c.p.push((num(3).unwrap_or(-1.0) as i32 + 1).max(0) as usize);
        c.n += 1;
    }
    c.w = vec![[0.0; 3]; c.n];
    c.s = vec![0.0; c.n];
    let found = c.n > 0;
    rec.hair_pts = Some(found.then_some(c));
    if !found {
        rec.hair_retry = t + 3.0;
    }
    found
}
fn give_back(rec: &NpcRef) {
    let mut r = rec.borrow_mut();
    hair_dangle::hair_force(r.e.instance.cast(), &[0.0; 3]);
    r.hair_force = None;
    if r.hair_strands.take().is_some() {
        hair_dangle::hair_strands(r.e.instance.cast(), &[]);
    }
}
pub(super) fn hair(s: &mut State, dt: f32, f: &Frame) {
    let dt = if dt > 1e-4 { dt } else { 0.016 };
    let palm_only = s.phys;
    s.hair.gen = s.hair.gen.wrapping_add(1);
    s.hair.t += dt;
    let gen = s.hair.gen;
    let (hair_r, hair_back, hair_down) = if palm_only { (HAIR_R_PALM, HAIR_BACK_PALM, HAIR_DOWN_PALM) } else { (HAIR_R, HAIR_BACK, HAIR_DOWN) };
    let d = data::data();
    for side in 0..2 {
        let at = if palm_only {
            d.hand[side].and_then(|h| {
                let (hp, hq) = s.bone(d.caps[h].index)?;
                let sgn = if d.caps[h].c[0] >= 0.0 { HAIR_PALM } else { -HAIR_PALM };
                Some(f.to_world(&super::add(&hp, &qrot(&hq, &[sgn, 0.0, 0.0]))))
            })
        } else {
            carry::hand_world(s, side, f).map(|(p, _)| p)
        };
        let h = &mut s.hair.hands[side];
        h.v = match (at, h.at) {
            (Some(x), Some(was)) => {
                let mut v = mul(&sub(&x, &was), 1.0 / dt);
                let sp = len(&v);
                if sp > 5.0 {
                    v = mul(&v, 5.0 / sp);
                }
                v
            }
            _ => [0.0; 3],
        };
        h.at = at;
        // the palm and each finger's middle joint and tip
        if palm_only {
            if let Some(x) = at {
                probe(&mut s.hair.probes[side][1], x, HAIR_W_PALM, hair_r, HAIR_BONE_R + HAIR_BONE_PALM, dt, gen);
                for digit in 0..5 {
                    let Some(ends) = d.fingers[side][digit].and_then(|fi| carry::finger_ends(s, &fi)) else { continue };
                    let (mid, tip) = (f.to_world(&ends.0), f.to_world(&ends.1));
                    probe(&mut s.hair.probes[side][2 + digit * 2], mid, HAIR_W_MID, HAIR_R_FINGER, HAIR_BONE_R + HAIR_BONE_FINGER, dt, gen);
                    probe(&mut s.hair.probes[side][3 + digit * 2], tip, HAIR_W_TIP, HAIR_R_FINGER, HAIR_BONE_R + HAIR_BONE_FINGER, dt, gen);
                }
            }
        }
    }
    let ease = (dt / HAIR_EASE).min(1.0);
    let mut nearest = 9.0f32;
    let mut fingers_in = 0;
    let mut hair_by = String::from("nobody near");
    let mut light_n = 0;
    let wrist_r = HAIR_BONE_R + HAIR_BONE_WRIST;
    let mut seen: Vec<NpcRef> = Vec::new();
    for i in 0..s.people.n {
        let Some(rec_ref) = s.people.at[i].clone() else { continue };
        let Some(nw) = s.people.nw[i] else { continue };
        if rec_ref.borrow().n < 8 || rec_ref.borrow().e.is_null() {
            continue;
        }
        carry_slots(&mut s.people.carried, &rec_ref.borrow(), &nw, s.people.nf[i]);
        let c = &s.people.carried;
        let mut force = [0.0f32; 3];
        let mut strands_any = false;
        let mut rec = rec_ref.borrow_mut();
        if let (Some(w1), Some(x1)) = (c.w[0], c.x[0]) {
            let y1 = c.y[0].unwrap_or([0.0; 3]);
            // the crown, and the hair's lower end behind the nape
            let cb = if palm_only { 0.03 } else { 0.01 };
            let cc = [w1[0] + x1[0] * 0.10 + y1[0] * cb, w1[1] + x1[1] * 0.10 + y1[1] * cb, w1[2] + x1[2] * 0.10 + y1[2] * cb];
            let tt = [w1[0] - x1[0] * hair_down + y1[0] * hair_back, w1[1] - x1[1] * hair_down + y1[1] * hair_back, w1[2] - x1[2] * hair_down + y1[2] * hair_back];
            let a = sub(&tt, &cc);
            let aa = super::len2(&a);
            // where this person's hair is, carried by their head to this frame, when a hand is near their head
            let near = s.hair.hands.iter().any(|h| h.at.is_some_and(|x| super::len2(&sub(&x, &w1)) < HAIR_BONE_NEAR * HAIR_BONE_NEAR));
            let mut use_chains = false;
            if near {
                use_chains = chains(&mut rec, s.hair.t) && c.z[0].is_some();
                if use_chains {
                    let z1 = c.z[0].expect("checked");
                    if let Some(Some(ch)) = rec.hair_pts.as_mut() {
                        for k in 0..ch.n {
                            let l = ch.l[k];
                            ch.w[k] = [
                                w1[0] + x1[0] * l[0] + y1[0] * l[1] + z1[0] * l[2],
                                w1[1] + x1[1] * l[0] + y1[1] * l[1] + z1[1] * l[2],
                                w1[2] + x1[2] * l[0] + y1[2] * l[1] + z1[2] * l[2],
                            ];
                            ch.s[k] = 0.0;
                        }
                        hair_by = format!("its {} bones", ch.n);
                    }
                } else {
                    hair_by = "the line".into();
                }
            }
            for side in 0..2 {
                let h = s.hair.hands[side];
                let Some(hx) = h.at else { continue };
                let mut ch = if use_chains { rec.hair_pts.as_mut().and_then(|c| c.as_mut()) } else { None };
                if palm_only {
                    // the palm first: a hand far from this hair has no finger in it either
                    let p = s.hair.probes[side][1];
                    if p.set && p.gen == gen {
                        let dd = push(&mut force, &p.p, &p.v, p.r, p.rc, p.w, ch.as_deref_mut(), &cc, &a, aa);
                        nearest = nearest.min(dd);
                        if dd < if use_chains { p.rc } else { hair_r } + 0.25 {
                            for slot in 2..12 {
                                let q = s.hair.probes[side][slot];
                                if q.set && q.gen == gen {
                                    let dq = push(&mut force, &q.p, &q.v, q.r, q.rc, q.w, ch.as_deref_mut(), &cc, &a, aa);
                                    nearest = nearest.min(dq);
                                    if dq < if use_chains { q.rc } else { q.r } {
                                        fingers_in += 1;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let dd = push(&mut force, &hx, &h.v, hair_r, wrist_r, 1.0, ch, &cc, &a, aa);
                    nearest = nearest.min(dd);
                }
            }
            // each lock its own: a particle as touched as the most touched piece of its lock from it up to the scalp
            if use_chains {
                let mut key = String::new();
                let mut any = false;
                light_n = 0;
                if let Some(Some(ch)) = rec.hair_pts.as_ref() {
                    for k in 0..ch.n {
                        let (mut w, mut j, mut up) = (ch.s[k], ch.p[k], 0);
                        while j > 0 && up < 16 {
                            w = w.max(ch.s[j - 1]);
                            j = ch.p[j - 1];
                            up += 1;
                        }
                        let w = (w / HAIR_STRAND_STEP + 0.5).floor() * HAIR_STRAND_STEP;
                        if w > 0.0 {
                            any = true;
                            light_n += 1;
                        }
                        if k > 0 {
                            key.push(' ');
                        }
                        key.push_str(&format!("{w:.1}"));
                    }
                }
                // none touched: the last weights kept while the force eases out, given back with it
                strands_any = any;
                if any && rec.hair_strands.as_deref() != Some(key.as_str()) {
                    hair_dangle::hair_strands(rec.e.instance.cast(), &hair_dangle::parse_weights(&key));
                    rec.hair_strands = Some(key);
                }
            }
        }
        let fl = len(&force);
        if fl > HAIR_MOST {
            force = mul(&force, HAIR_MOST / fl);
        }
        if rec.hair_force.is_some() || fl > 0.0 {
            let mut g = rec.hair_force.unwrap_or([0.0; 3]);
            for k in 0..3 {
                g[k] += (force[k] - g[k]) * ease;
            }
            if fl == 0.0 && len(&g) < 0.3 {
                drop(rec);
                give_back(&rec_ref);
                s.hair.forced.retain(|r| !Rc::ptr_eq(r, &rec_ref));
                rec = rec_ref.borrow_mut();
            } else {
                hair_dangle::hair_force(rec.e.instance.cast(), &g);
                rec.hair_force = Some(g);
                if !s.hair.forced.iter().any(|r| Rc::ptr_eq(r, &rec_ref)) {
                    s.hair.forced.push(rec_ref.clone());
                }
                seen.push(rec_ref.clone());
            }
        }
        // weighted without a force (a hand near a lock, not in it), and now none near: the hair's own masses back
        if rec.hair_strands.is_some() && rec.hair_force.is_none() && !strands_any {
            hair_dangle::hair_strands(rec.e.instance.cast(), &[]);
            rec.hair_strands = None;
        }
    }
    // once a second while someone is near: how near the nearest hand came to their hair
    s.hair.say += dt;
    if s.hair.say >= 1.0 && s.people.n > 0 {
        s.hair.say = 0.0;
        if super::dbg_on() {
            cvr_host::log!(
                "[HandColl] hair: {} near, the nearest hand {:.2} m from their hair (by {}), {} given a force, {} finger points in it, {} particles light\n",
                s.people.n,
                nearest,
                hair_by,
                s.hair.forced.len(),
                fingers_in,
                light_n
            );
        }
    }
    // whoever left the list with a force on their hair: given back
    let forced = std::mem::take(&mut s.hair.forced);
    for rec in forced {
        if seen.iter().any(|r| Rc::ptr_eq(r, &rec)) {
            s.hair.forced.push(rec);
        } else {
            give_back(&rec);
        }
    }
}
/// suspend's part: every hair given a force given back.
pub(super) fn release_all(s: &mut State) {
    for rec in std::mem::take(&mut s.hair.forced) {
        give_back(&rec);
    }
}
