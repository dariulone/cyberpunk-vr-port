//! modules/hands.lua and grips.lua: the two hands in the lobby's world -- the palm's pose (VRPalmModelPos/Rot over the
//! player), its velocity, the tracked controller (the reach point), the menu pointer's ray and grip / trigger (read
//! against a plane through the player: VRMenuPointer's origin and hit), the grip closing past 0.6 and opening below 0.35,
//! who owns a closed hand; a face of buttons pressed by a fingertip or pulled by the beam; and the grasp the plugin's
//! fingers close on a held shape (VRGraspSeat / Set / Blend / Clear, eased in and out).
use super::belt::{qmul, qrot, Q};
use super::lobby_spawn::{add, mul, native, native_owned, native_v4s, sub, V3};
use crate::script_call::Value;

const GRIP_ON: f64 = 0.6;
const GRIP_OFF: f64 = 0.35;
const VEL_SMOOTH: f64 = 0.06;
const PALM_AT: [V3; 2] = [[0.0396, 0.0039, -0.0124], [-0.0396, -0.0039, 0.0124]];
const PALM_OUT: [V3; 2] = [[0.0775, -0.0077, -0.9970], [-0.0775, 0.0077, 0.9970]];

#[derive(Default, Clone)]
pub(super) struct Hand {
    pub pos: Option<V3>,
    pub rot: Q,
    pub vel: V3,
    pub palm: Option<(V3, V3)>,
    pub reach: Option<V3>,
    pub ray: Option<(V3, V3)>,
    rel: Option<V3>,
    pub cvel: Option<V3>,
    pub grip_prev: bool,
    pub grip: bool,
    pub trigger: f64,
    pub owner: Option<&'static str>,
}
#[derive(Default)]
pub(super) struct Hands {
    pub hands: [Hand; 2],
}
pub(super) fn len(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn f3(v: [f32; 4]) -> V3 {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}
pub(super) fn player_pose() -> Option<(V3, Q)> {
    let pl = super::belt::player();
    if pl.is_null() {
        return None;
    }
    let p = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).ok()?;
    let q = super::belt::m::<[f32; 4]>(&pl, "GetWorldOrientation", &[]).ok()?;
    Some((f3(p), q.map(f64::from)))
}
impl Hands {
    /// M.update: the hands this frame; false without a player.
    pub(super) fn update(&mut self, dt: f64) -> bool {
        let Some((pp, pq)) = player_pose() else { return false };
        let axes = [[pp[0] as f32, pp[1] as f32, pp[2] as f32, 1.0], [1.0f32, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 1.0, 0.0, 0.0]];
        let hits = native_v4s("VRMenuPointer", &[Value::Ptr(axes[0].as_ptr().cast()), Value::Ptr(axes[1].as_ptr().cast()), Value::Ptr(axes[2].as_ptr().cast()), Value::Ptr(axes[3].as_ptr().cast())]);
        let hits = (hits.len() >= 8).then_some(hits);
        for hand in 0..2 {
            let h = &mut self.hands[hand];
            let m = native::<[f32; 4]>("VRPalmModelPos", &[Value::Int32(hand as i32)]);
            let r = native::<[f32; 4]>("VRPalmModelRot", &[Value::Int32(hand as i32)]);
            match (m.filter(|m| m[3] > 0.5), r) {
                (Some(m), Some(r)) => {
                    let pos = add(pp, qrot(pq, f3(m)));
                    h.vel = match (h.pos, dt > 0.0) {
                        (Some(old), true) => mul(sub(pos, old), 1.0 / dt),
                        _ => [0.0; 3],
                    };
                    h.pos = Some(pos);
                    h.rot = qmul(pq, r.map(f64::from));
                    h.palm = Some((add(pos, qrot(h.rot, PALM_AT[hand])), qrot(h.rot, PALM_OUT[hand])));
                    h.reach = native::<[f32; 4]>("VRHandRawModel", &[Value::Int32(hand as i32)]).filter(|c| c[3] > 0.5).map(|c| add(pp, qrot(pq, f3(c))));
                }
                _ => (h.pos, h.reach, h.palm) = (None, None, None),
            }
            h.ray = None;
            let was = h.rel.take();
            if let Some(hs) = &hits {
                let (o, e) = (hs[4 * hand + 2], hs[4 * hand + 3]);
                let d = [f64::from(e[0] - o[0]), f64::from(e[1] - o[1]), f64::from(e[2] - o[2])];
                let l = len(d);
                if l > 1e-3 {
                    h.ray = Some((f3(o), mul(d, 1.0 / l)));
                }
                h.rel = Some(sub(f3(o), pp));
            }
            match (h.rel, was, dt > 1e-4) {
                (Some(rel), Some(was), true) => {
                    let v = mul(sub(rel, was), 1.0 / dt);
                    h.cvel = Some(match h.cvel {
                        Some(c) => add(c, mul(sub(v, c), (dt / VEL_SMOOTH).min(1.0))),
                        None => v,
                    });
                }
                _ => h.cvel = None,
            }
            let g = hits.as_ref().map_or(0.0, |hs| f64::from(hs[4 * hand + 3][3]));
            h.grip_prev = h.grip;
            h.grip = g >= GRIP_ON || (h.grip && g > GRIP_OFF);
            h.trigger = hits.as_ref().map_or(0.0, |hs| f64::from(hs[4 * hand + 2][3]));
            if !h.grip {
                h.owner = None;
            }
        }
        true
    }
    /// M.taking: the hand closes this frame and nobody has it.
    pub(super) fn taking(&self, hand: usize) -> bool {
        let h = &self.hands[hand - 1];
        h.pos.is_some() && h.grip && !h.grip_prev && h.owner.is_none()
    }
    pub(super) fn claim(&mut self, hand: usize, who: &'static str) {
        self.hands[hand - 1].owner = Some(who);
    }
    pub(super) fn release(&mut self, hand: usize, who: &'static str) {
        if self.hands[hand - 1].owner == Some(who) {
            self.hands[hand - 1].owner = None;
        }
    }
    pub(super) fn h(&self, hand: usize) -> &Hand {
        &self.hands[hand - 1]
    }
}

// ---------------------------------------------------------------- a face of buttons (M.face / pointer / pressed)
pub(super) struct Face {
    pub c: V3,
    pub r: V3,
    pub u: V3,
    pub w: f64,
    pub h: f64,
    pressed: [bool; 2],
    pulled: [bool; 2],
}
const TOUCH: f64 = 0.006;
const PULL: f64 = 0.75;
const LET_GO: f64 = 0.35;
pub(super) fn yaw_quat(deg: f64) -> Q {
    let a = deg.to_radians() / 2.0;
    [0.0, 0.0, a.sin(), a.cos()]
}
pub(super) fn to_world(pos: V3, yaw: f64, p: V3) -> V3 {
    add(pos, qrot(yaw_quat(yaw), p))
}
pub(super) fn face(pos: V3, yaw: f64, centre: V3, right: V3, up: V3, w: f64, h: f64) -> Face {
    let yq = yaw_quat(yaw);
    Face { c: to_world(pos, yaw, centre), r: qrot(yq, right), u: qrot(yq, up), w, h, pressed: [false; 2], pulled: [false; 2] }
}
pub(super) fn pointer(f: &Face) -> Option<Vec<[f32; 4]>> {
    let v = [[f.c[0] as f32, f.c[1] as f32, f.c[2] as f32, 1.0], [f.r[0] as f32, f.r[1] as f32, f.r[2] as f32, 0.0], [f.u[0] as f32, f.u[1] as f32, f.u[2] as f32, 0.0], [f.w as f32, f.h as f32, 0.0, 0.0]];
    let hits = native_v4s("VRMenuPointer", &[Value::Ptr(v[0].as_ptr().cast()), Value::Ptr(v[1].as_ptr().cast()), Value::Ptr(v[2].as_ptr().cast()), Value::Ptr(v[3].as_ptr().cast())]);
    (hits.len() >= 8).then_some(hits)
}
/// M.pressed: the hand (1, 2) that pressed the face this frame -- a fingertip in, or the beam's trigger pulled.
pub(super) fn pressed(f: &mut Face, hits: Option<&[[f32; 4]]>) -> Option<usize> {
    let hits = hits?;
    let mut fired = None;
    for hand in 0..2 {
        let (tip, ray, origin) = (hits[4 * hand], hits[4 * hand + 1], hits[4 * hand + 2]);
        let on = |p: [f32; 4]| p[0] >= -0.2 && p[0] <= 1.2 && p[1] >= -0.2 && p[1] <= 1.2;
        let pr = tip[3] > 0.5 && on(tip) && f64::from(tip[2]) <= TOUCH && tip[2] > -0.05;
        if pr && !f.pressed[hand] {
            fired = Some(hand + 1);
        }
        f.pressed[hand] = pr;
        let aimed = ray[3] > 0.5 && on(ray);
        let pulled = aimed && if f.pulled[hand] { f64::from(origin[3]) > LET_GO } else { f64::from(origin[3]) >= PULL };
        if pulled && !f.pulled[hand] {
            fired = Some(hand + 1);
        }
        f.pulled[hand] = pulled;
    }
    fired
}

// ---------------------------------------------------------------- grips.lua
const EASE_IN: f64 = 0.12;
const EASE_OUT: f64 = 0.18;
#[derive(Default, Clone)]
struct GripS {
    blend: f64,
    target: f64,
    shape: Option<String>,
    applied: bool,
}
#[derive(Default)]
pub(super) struct Grips {
    s: [GripS; 2],
}
/// What the grasp closes on: the entry's shape and its half sizes (the grasp's own when given), and where.
pub(super) struct Shape {
    pub shape: String,
    pub half: V3,
    pub grasp_half: Option<V3>,
    pub grasp_at: Option<V3>,
}
fn code(shape: &str) -> Option<i32> {
    match shape {
        "box" => Some(0),
        "sphere" => Some(1),
        "cylinder" => Some(2),
        "cone" => Some(3),
        _ => None,
    }
}
fn part(e: &Shape, p: V3, q: Q) -> V3 {
    e.grasp_at.map_or(p, |g| add(p, qrot(q, g)))
}
fn args(hand: usize, c: i32, h: V3, at: V3, q: Q) -> ([f32; 4], [f32; 4], Vec<Value>) {
    let pv = [at[0] as f32, at[1] as f32, at[2] as f32, 1.0];
    let qv = q.map(|v| v as f32);
    (pv, qv, vec![Value::Int32(hand as i32 - 1), Value::Int32(c), Value::Float(h[0] as f32), Value::Float(h[1] as f32), Value::Float(h[2] as f32)])
}
impl Grips {
    /// M.seat: where the held thing must be for the hand's grasp to close on it (None: no grasp for this shape).
    pub(super) fn seat(&self, hand: usize, e: &Shape, p: V3, q: Q) -> Option<(V3, Q)> {
        let c = code(&e.shape)?;
        let h = e.grasp_half.unwrap_or(e.half);
        let at = part(e, p, q);
        let (pv, qv, mut vals) = args(hand, c, h, at, q);
        vals.push(Value::Ptr(pv.as_ptr().cast()));
        vals.push(Value::Ptr(qv.as_ptr().cast()));
        let t = native_owned("VRGraspSeat", &vals)?;
        // SAFETY: a Transform: position Vector4, orientation Quaternion.
        let tr = unsafe { *(t.as_ptr() as *const [[f32; 4]; 2]) };
        if tr[0][3] < 0.5 {
            return None;
        }
        let tq = tr[1].map(f64::from);
        let mut tp = f3(tr[0]);
        if let Some(g) = e.grasp_at {
            tp = sub(tp, qrot(tq, g));
        }
        Some((tp, tq))
    }
    pub(super) fn hold(&mut self, hand: usize, e: &Shape, pos: Option<V3>, rot: Option<Q>) {
        let (Some(c), Some(pos), Some(rot)) = (code(&e.shape), pos, rot) else { return };
        let h = e.grasp_half.unwrap_or(e.half);
        let s = &mut self.s[hand - 1];
        let at = part(e, pos, rot);
        native::<bool>("VRGraspBlend", &[Value::Int32(hand as i32 - 1), Value::Float(s.blend as f32)]);
        let (pv, qv, mut vals) = args(hand, c, h, at, rot);
        vals.push(Value::Ptr(pv.as_ptr().cast()));
        vals.push(Value::Ptr(qv.as_ptr().cast()));
        if native::<bool>("VRGraspSet", &vals).is_none() {
            return;
        }
        (s.applied, s.shape, s.target) = (true, Some(e.shape.clone()), 1.0);
    }
    pub(super) fn release(&mut self, hand: usize) {
        self.s[hand - 1].target = 0.0;
    }
    pub(super) fn update(&mut self, dt: f64) {
        for (i, s) in self.s.iter_mut().enumerate() {
            if !s.applied {
                continue;
            }
            let rate = if s.target > s.blend { 1.0 / EASE_IN } else { 1.0 / EASE_OUT };
            s.blend = if s.blend < s.target { s.target.min(s.blend + rate * dt) } else { s.target.max(s.blend - rate * dt) };
            if s.blend <= 0.001 && s.target == 0.0 {
                native::<bool>("VRGraspClear", &[Value::Int32(i as i32)]);
                (s.applied, s.shape) = (false, None);
            } else {
                native::<bool>("VRGraspBlend", &[Value::Int32(i as i32), Value::Float(s.blend as f32)]);
            }
        }
    }
    pub(super) fn close(&mut self) {
        for (i, s) in self.s.iter_mut().enumerate() {
            if s.applied {
                native::<bool>("VRGraspClear", &[Value::Int32(i as i32)]);
            }
            *s = GripS::default();
        }
    }
}
