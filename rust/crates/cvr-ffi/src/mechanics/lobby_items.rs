//! modules/items.lua: the lobby's loose things (the scene's items and the engrams) -- spawned on their spots, their
//! physical bodies found on the `item` component; a closing hand takes the nearest within 10 cm of its surface: seated
//! in the hand slot where the plugin's grasp closes on its shape and carried attached (VRAttachToSlot, kinematic), or
//! pulled along by impulses when it cannot be; let go it is thrown at the carried motion's velocity. A hand aimed at
//! one (the palm's ray) marks it; closed it locks on, and a flick of the controller away pulls it flying to the hand.
//! Lost ones (fallen through, flown away) are put back on their spots.
use super::belt::{conj, qmul, qrot, Q};
use super::lobby::Lobby;
use super::lobby_hands::{len, Shape};
use super::lobby_spawn::{self as sp, add, dot, mul, native, sub, V3};
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::rc::Rc;

const TAG: &str = "VRLobbyItem";
const ITEM: &str = "item";
const REACH: f64 = 0.10;
const GAIN: f64 = 25.0;
const MAX_SPEED: f64 = 9.0;
const ACCEL: f64 = 80.0;
const SPIN: f64 = 12.0;
const RECOVER: f64 = 0.20;
const LOST_BELOW: f64 = 3.0;
const LOST_AWAY: f64 = 60.0;
const LOST_EVERY: f64 = 0.5;
const SOUND_DIR: &str = "CyberpunkVRPort_Menu/sounds/";
const HAPTIC_CLICK: i32 = 20;
const SLOTS: &str = "ItemAttachmentSlots";
const HAND_SLOT: [&str; 2] = ["WeaponLeft", "WeaponRight"];
const THROW_WINDOW: f64 = 0.06;
const THROW_FRAMES: i32 = 2;
const HOVER: f64 = 0.22;
const AIM_RANGE: f64 = 6.0;
const RAY_RADIUS: f64 = 0.06;
const FLICK: f64 = 1.1;
const PULL_SPEED: f64 = 7.0;
const PULL_ACCEL: f64 = 70.0;
const PULL_SLOW: f64 = 4.0;
const PULL_LONGEST: f64 = 2.0;
const WHO: &str = "items";
fn aim_cone() -> f64 {
    6f64.to_radians()
}

/// An item of the scene's list (or an engram): its template, spot, shape and mass.
pub(super) struct Entry {
    pub name: String,
    pub ent: String,
    pub position: V3,
    /// (r, i, j, k), as the scene list writes it
    pub rotation: [f64; 4],
    pub radius: f64,
    pub mass: f64,
    pub shape: Option<String>,
    pub half: Option<V3>,
    pub grasp_half: Option<V3>,
    pub grasp_at: Option<V3>,
    pub centre: Option<V3>,
    pub look: Option<Rc<super::lobby_engrams::Look>>,
}
pub(super) type EntryRef = Rc<Entry>;
impl Entry {
    fn shape(&self) -> Shape {
        Shape { shape: self.shape.clone().unwrap_or_default(), half: self.half.unwrap_or([0.0; 3]), grasp_half: self.grasp_half, grasp_at: self.grasp_at }
    }
}
struct Item {
    entry: EntryRef,
    id: Option<u64>,
    home: V3,
    entity: Handle,
    body: Handle,
    gone: bool,
}
type ItemRef = Rc<RefCell<Item>>;
#[derive(Default)]
struct Held {
    item: Option<usize>,
    offset: V3,
    rel_rot: Q,
    attached: bool,
    trail: Vec<(f64, V3, Q)>,
    clock: f64,
    lp: Option<V3>,
    lq: Option<Q>,
}
#[derive(Default)]
struct ReachS {
    aim: Option<usize>,
    lock: Option<usize>,
    pull: Option<usize>,
    pull_for: f64,
    snatch: f64,
}
struct Throw {
    it: ItemRef,
    v: V3,
    w: V3,
    frames: i32,
}
#[derive(Default)]
pub(super) struct Items {
    anchor: Option<(V3, f64)>,
    items: Vec<ItemRef>,
    held: [Held; 2],
    giving: [Option<usize>; 2],
    resets: u32,
    throws: Vec<Throw>,
    reach: [ReachS; 2],
    pulls: u32,
    lost_at: f64,
    last_throw: f64,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Items {}

fn play(name: &str) {
    let f = sp::Str::new(&format!("{SOUND_DIR}{name}.wav"));
    native::<bool>("VRMenuPlayWav", &[f.v(), Value::Float(0.8)]);
}
fn haptic(hand: usize) {
    sp::haptic(hand, HAPTIC_CLICK);
}
fn world_pos(e: &Handle) -> Option<V3> {
    super::belt::m::<[f32; 4]>(e, "GetWorldPosition", &[]).ok().map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
}
fn world_rot(e: &Handle) -> Option<Q> {
    super::belt::m::<[f32; 4]>(e, "GetWorldOrientation", &[]).ok().map(|q| q.map(f64::from))
}
fn yaw_q(yaw: f64) -> Q {
    super::lobby_hands::yaw_quat(yaw)
}
fn spawn_item(a: (V3, f64), entry: EntryRef, at: Option<V3>) -> ItemRef {
    let p = super::lobby_hands::to_world(a.0, a.1, entry.position);
    let r = entry.rotation;
    let q = qmul(yaw_q(a.1), [r[1], r[2], r[3], r[0]]);
    let w = at.unwrap_or([p[0], p[1], p[2] + 0.01]);
    let id = sp::spawn(&entry.ent, w, q, TAG);
    Rc::new(RefCell::new(Item { entry, id, home: p, entity: Handle::null(), body: Handle::null(), gone: false }))
}
fn despawn_item(it: &mut Item) {
    sp::delete(it.id);
    (it.id, it.entity, it.body) = (None, Handle::null(), Handle::null());
}
fn resolve(it: &mut Item) -> bool {
    if !it.body.is_null() {
        return true;
    }
    if !sp::spawned(it.id) {
        return false;
    }
    it.entity = sp::find_entity(it.id);
    if !it.entity.is_null() {
        let c = sp::find_comp(&it.entity, ITEM);
        if !c.is_null() {
            it.body = super::lobby_ink::child(&c, "CreatePhysicalBodyInterface", &[Value::Int32(0)]);
        }
    }
    !it.body.is_null()
}
fn body_call(b: &Handle, name: &str, values: &[Value]) {
    super::lobby_ink::call(b, name, values);
}
fn v4(v: V3, w: f32) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w]
}
fn throw_velocity(trail: &[(f64, V3, Q)]) -> (V3, V3) {
    let Some(last) = trail.last() else { return ([0.0; 3], [0.0; 3]) };
    let mut first = last;
    for n in (0..trail.len().saturating_sub(1)).rev() {
        first = &trail[n];
        if last.0 - first.0 >= THROW_WINDOW {
            break;
        }
    }
    let span = last.0 - first.0;
    if span <= 0.0 {
        return ([0.0; 3], [0.0; 3]);
    }
    let qd = qmul(last.2, conj(first.2));
    let s = if qd[3] < 0.0 { -2.0 } else { 2.0 } / span;
    (mul(sub(last.1, first.1), 1.0 / span), [qd[0] * s, qd[1] * s, qd[2] * s])
}
/// surface: the signed distance from a point to the item's shape.
fn surface(it: &Item, p: V3) -> Option<f64> {
    let e = &it.entry;
    let mut l = qrot(conj(world_rot(&it.entity)?), sub(p, world_pos(&it.entity)?));
    if let Some(c) = e.centre {
        l = sub(l, c);
    }
    let (Some(shape), Some(h)) = (e.shape.as_deref(), e.half) else { return Some(len(l) - e.radius) };
    if shape == "sphere" {
        return Some(len(l) - h[0]);
    }
    let q: Vec<f64> = if shape == "cylinder" || shape == "cone" {
        vec![(l[0] * l[0] + l[1] * l[1]).sqrt() - h[0].max(h[1]), l[2].abs() - h[2]]
    } else {
        vec![l[0].abs() - h[0], l[1].abs() - h[1], l[2].abs() - h[2]]
    };
    let (mut out, mut inside) = (0.0, f64::NEG_INFINITY);
    for c in q {
        out += c.max(0.0).powi(2);
        inside = inside.max(c);
    }
    Some(out.sqrt() + inside.min(0.0))
}
fn slots() -> Handle {
    sp::find_comp(&super::belt::player(), SLOTS)
}
fn attach(hand: usize, it: &Item) -> bool {
    let s = slots();
    if s.is_null() {
        return false;
    }
    body_call(&it.body, "SetIsKinematic", &[Value::Bool(true)]);
    if native::<bool>("VRAttachToSlot", &[Value::Handle(&it.entity), Value::Handle(&s), sp::cn(HAND_SLOT[hand - 1])]) == Some(true) {
        return true;
    }
    body_call(&it.body, "SetIsKinematic", &[Value::Bool(false)]);
    false
}
fn body_to(it: &Item, pos: Option<V3>, rot: Option<Q>) {
    let (Some(p), Some(q)) = (pos.or_else(|| world_pos(&it.entity)), rot.or_else(|| world_rot(&it.entity))) else { return };
    let (pv, qv) = (v4(p, 1.0), q.map(|v| v as f32));
    native::<bool>("VRBodySetPos", &[Value::Handle(&it.body), Value::Ptr(pv.as_ptr().cast()), Value::Ptr(qv.as_ptr().cast())]);
}
fn slot_pose(hand: usize) -> Option<(V3, Q)> {
    super::reload_core::slot_world(&slots(), HAND_SLOT[hand - 1])
}
fn carried(hand: usize, lp: V3, lq: Q) -> Option<(V3, Q)> {
    let (sp_, sq) = slot_pose(hand)?;
    Some((add(sp_, qrot(sq, lp)), qmul(sq, lq)))
}
fn throw_on(t: &Throw) {
    let it = t.it.borrow();
    let (v, w) = (v4(t.v, 0.0), v4(t.w, 0.0));
    native::<bool>("VRBodySetVel", &[Value::Handle(&it.body), Value::Ptr(v.as_ptr().cast())]);
    native::<bool>("VRBodySetAngVel", &[Value::Handle(&it.body), Value::Ptr(w.as_ptr().cast())]);
}
fn release(l: &mut Lobby, hand: usize) {
    let s = &mut l.items;
    let p = &mut s.held[hand - 1];
    if let Some(itr) = p.item.and_then(|i| s.items.get(i)).cloned() {
        let it = itr.borrow();
        if !it.body.is_null() {
            if p.attached {
                let (v, w) = throw_velocity(&p.trail);
                let pose = world_pos(&it.entity).zip(world_rot(&it.entity));
                native::<bool>("VRDetachFromSlot", &[Value::Handle(&it.entity)]);
                body_call(&it.body, "SetIsKinematic", &[Value::Bool(false)]);
                if let Some((pos, rot)) = pose {
                    super::reload_core::set_local(&sp::find_comp(&it.entity, ITEM), pos, rot);
                    body_to(&it, Some(pos), Some(rot));
                }
                drop(it);
                let t = Throw { it: itr.clone(), v, w, frames: THROW_FRAMES };
                s.last_throw = len(v);
                throw_on(&t);
                s.throws.push(t);
            } else {
                drop(it);
            }
            body_call(&itr.borrow().body, "SetIsQueryable", &[Value::Bool(true)]);
        }
    }
    let p = &mut s.held[hand - 1];
    (p.item, p.attached, p.trail, p.lp, p.lq) = (None, false, Vec::new(), None, None);
    l.hands.release(hand, WHO);
    l.grips.release(hand);
}
fn put_back(l: &mut Lobby, i: usize) {
    for hand in 1..=2 {
        if l.items.held[hand - 1].item == Some(i) {
            release(l, hand);
        }
    }
    let Some(a) = l.items.anchor else { return };
    let entry = {
        let mut it = l.items.items[i].borrow_mut();
        despawn_item(&mut it);
        it.entry.clone()
    };
    l.items.items[i] = spawn_item(a, entry, None);
}
fn seat(l: &Lobby, hand: usize, it: &Item, snap: bool, at: V3, turn: Q) -> Option<(V3, Q)> {
    let (sp_, sq) = slot_pose(hand)?;
    let inv = conj(sq);
    let lq = qmul(inv, turn);
    let lp = qrot(inv, sub(at, sp_));
    if let Some(r) = l.grips.seat(hand, &it.entry.shape(), lp, lq) {
        return Some(r);
    }
    let h = l.hands.h(hand);
    if let (true, Some(reach), Some(pos)) = (snap, h.reach, h.pos) {
        let mut off = sub(at, reach);
        let (ln, most) = (len(off), it.entry.radius + 0.02);
        if ln > most {
            off = mul(off, most / ln);
        }
        return Some((qrot(inv, sub(add(pos, off), sp_)), lq));
    }
    Some((lp, lq))
}
fn hold(l: &mut Lobby, hand: usize, i: usize, snap: bool) {
    let itr = l.items.items[i].clone();
    let (Some(hpos), hrot) = (l.hands.h(hand).pos, l.hands.h(hand).rot) else { return };
    let inv = conj(hrot);
    let it = itr.borrow();
    let (Some(at), Some(turn)) = (world_pos(&it.entity), world_rot(&it.entity)) else { return };
    {
        let p = &mut l.items.held[hand - 1];
        p.item = Some(i);
        p.offset = qrot(inv, sub(at, hpos));
        p.rel_rot = qmul(inv, turn);
    }
    l.hands.claim(hand, WHO);
    body_call(&it.body, "SetIsQueryable", &[Value::Bool(false)]);
    let seated = seat(l, hand, &it, snap, at, turn);
    let p = &mut l.items.held[hand - 1];
    (p.lp, p.lq) = (seated.map(|s| s.0), seated.map(|s| s.1));
    let wpose = seated.and_then(|(lp, lq)| carried(hand, lp, lq));
    if let Some((wp, wr)) = wpose {
        body_to(&it, Some(wp), Some(wr));
    }
    let attached = attach(hand, &it);
    let p = &mut l.items.held[hand - 1];
    (p.attached, p.trail, p.clock) = (attached, Vec::new(), 0.0);
    if let (true, Some(lp), Some(lq)) = (attached, p.lp, p.lq) {
        super::reload_core::set_local(&sp::find_comp(&it.entity, ITEM), lp, lq);
    } else if let Some((wp, wr)) = wpose {
        p.offset = qrot(inv, sub(wp, hpos));
        p.rel_rot = qmul(inv, wr);
    }
    let (gp, gq) = if attached { (p.lp, p.lq) } else { (None, None) };
    let shape = it.entry.shape();
    drop(it);
    l.grips.hold(hand, &shape, gp, gq);
    haptic(hand);
}
fn claimed(s: &Items, hand: usize, i: usize) -> bool {
    let other = 3 - hand;
    s.held[other - 1].item == Some(i) || s.reach[other - 1].pull == Some(i) || s.items[i].borrow().gone
}
fn nearest(l: &Lobby, hand: usize, within: f64) -> (Option<usize>, bool) {
    let h = l.hands.h(hand);
    let Some(hpos) = h.pos else { return (None, false) };
    let (mut best, mut best_d, mut best_far) = (None, None::<f64>, false);
    for i in 0..l.items.items.len() {
        if claimed(&l.items, hand, i) || !resolve(&mut l.items.items[i].borrow_mut()) {
            continue;
        }
        let it = l.items.items[i].borrow();
        let mut d = surface(&it, hpos);
        let far = d.is_some_and(|d| d >= within);
        if far {
            if let Some(r) = h.reach {
                d = surface(&it, r);
            }
        }
        if let Some(d) = d.filter(|d| *d < within) {
            if best_d.is_none_or(|b| d < b) {
                (best, best_d, best_far) = (Some(i), Some(d), far);
            }
        }
    }
    (best, best_far)
}
fn grab(l: &mut Lobby, hand: usize) -> bool {
    let (best, far) = nearest(l, hand, REACH);
    if let Some(b) = best {
        hold(l, hand, b, far);
    }
    best.is_some()
}
fn body_vel(b: &Handle) -> Option<V3> {
    native::<[f32; 4]>("VRBodyGetVel", &[Value::Handle(b)]).filter(|v| v[3] >= 0.5).map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
}
fn impulse(b: &Handle, dv: V3, m: f64) {
    let v = v4(mul(dv, m), 0.0);
    super::lobby_ink::call(b, "AddLinearImpulse", &[Value::Ptr(v.as_ptr().cast()), Value::Bool(true)]);
}
fn carry(l: &mut Lobby, hand: usize, dt: f64) {
    let Some(itr) = l.items.held[hand - 1].item.and_then(|i| l.items.items.get(i)).cloned() else {
        release(l, hand);
        return;
    };
    if !resolve(&mut itr.borrow_mut()) {
        release(l, hand);
        return;
    }
    let it = itr.borrow();
    if l.items.held[hand - 1].attached {
        let p = &mut l.items.held[hand - 1];
        p.clock += dt;
        let mut pose = match (p.lp, p.lq) {
            (Some(lp), Some(lq)) => carried(hand, lp, lq),
            _ => None,
        };
        if pose.is_none() {
            pose = world_pos(&it.entity).zip(world_rot(&it.entity));
        }
        if let Some((pos, rot)) = pose {
            body_to(&it, Some(pos), Some(rot));
            p.trail.push((p.clock, pos, rot));
            while p.trail.len() > 2 && p.clock - p.trail[1].0 > THROW_WINDOW {
                p.trail.remove(0);
            }
        }
        return;
    }
    let h = l.hands.h(hand).clone();
    let (Some(cur), Some(hpos)) = (world_pos(&it.entity), h.pos) else { return };
    let p = &l.items.held[hand - 1];
    let target = add(hpos, qrot(h.rot, p.offset));
    let miss = len(sub(target, cur));
    let far = if miss > RECOVER { (miss / RECOVER).min(12.0) } else { 1.0 };
    let mut want = add(add(h.vel, mul(sub(target, cur), GAIN)), [0.0, 0.0, 9.81 * dt]);
    let spd = len(want);
    if spd > MAX_SPEED * far {
        want = mul(want, MAX_SPEED * far / spd);
    }
    body_call(&it.body, "SetIsKinematic", &[Value::Bool(false)]);
    body_call(&it.body, "SetIsQueryable", &[Value::Bool(false)]);
    match body_vel(&it.body) {
        Some(v) => {
            let mut dv = sub(want, v);
            let cap = ACCEL * far * dt;
            let ln = len(dv);
            if ln > cap && ln > 1e-4 {
                dv = mul(dv, cap / ln);
            }
            impulse(&it.body, dv, it.entry.mass);
        }
        None => {
            let w = v4(want, 0.0);
            native::<bool>("VRBodySetVel", &[Value::Handle(&it.body), Value::Ptr(w.as_ptr().cast())]);
        }
    }
    if let Some(r) = world_rot(&it.entity) {
        let qt = qmul(h.rot, p.rel_rot);
        let qe = qmul(qt, conj(r));
        let s = if qe[3] < 0.0 { -1.0 } else { 1.0 };
        let w = [(2.0 * SPIN * qe[0] * s) as f32, (2.0 * SPIN * qe[1] * s) as f32, (2.0 * SPIN * qe[2] * s) as f32, 0.0];
        native::<bool>("VRBodySetAngVel", &[Value::Handle(&it.body), Value::Ptr(w.as_ptr().cast())]);
    }
}
fn centre(it: &Item) -> Option<V3> {
    let p = world_pos(&it.entity)?;
    match it.entry.centre {
        None => Some(p),
        Some(c) => Some(add(p, qrot(world_rot(&it.entity)?, c))),
    }
}
fn bound(e: &Entry) -> f64 {
    let (Some(shape), Some(h)) = (e.shape.as_deref(), e.half) else { return e.radius + 0.01 };
    match shape {
        "sphere" => h[0] + 0.01,
        "cylinder" | "cone" => (h[0].max(h[1]).powi(2) + h[2] * h[2]).sqrt() + 0.01,
        _ => (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt() + 0.01,
    }
}
fn mark(l: &Lobby, hand: usize, i: usize, kind: super::reach::Kind, ready: bool) {
    let it = l.items.items[i].borrow();
    let Some(pos) = centre(&it) else { return };
    super::reach::mark(hand, super::reach::Mark { pos, radius: Some(it.entry.radius), kind, ready, from: l.hands.h(hand).pos, lift: Some(bound(&it.entry)) });
}
fn pointed(l: &mut Lobby, hand: usize, ui: Option<f64>) -> Option<usize> {
    let (o, d) = l.hands.h(hand).palm?;
    let (mut best, mut best_score) = (None, None::<f64>);
    for i in 0..l.items.items.len() {
        if claimed(&l.items, hand, i) || !resolve(&mut l.items.items[i].borrow_mut()) {
            continue;
        }
        let it = l.items.items[i].borrow();
        let Some(c) = centre(&it) else { continue };
        let v = sub(c, o);
        let along = dot(v, d);
        if along > 0.05 && along < AIM_RANGE && ui.is_none_or(|u| along < u) {
            let off = (len(sub(v, mul(d, along))) - it.entry.radius - RAY_RADIUS).max(0.0);
            let score = off.atan2(along);
            if score < aim_cone() && best_score.is_none_or(|b| score < b) {
                (best, best_score) = (Some(i), Some(score));
            }
        }
    }
    best
}
fn fly(l: &Lobby, hand: usize, i: usize, dt: f64) {
    let h = l.hands.h(hand);
    let it = l.items.items[i].borrow();
    let (Some(c), Some(hpos)) = (centre(&it), h.pos) else { return };
    let way = sub(hpos, c);
    let dist = len(way);
    if dist < 1e-3 {
        return;
    }
    let speed = PULL_SPEED.min(dist * PULL_SLOW + 1.0);
    let want = add(add(mul(way, speed / dist), h.vel), [0.0, 0.0, 9.81 * dt]);
    body_call(&it.body, "SetIsKinematic", &[Value::Bool(false)]);
    if let Some(v) = body_vel(&it.body) {
        let mut dv = sub(want, v);
        let (ln, cap) = (len(dv), PULL_ACCEL * dt);
        if ln > cap && ln > 1e-4 {
            dv = mul(dv, cap / ln);
        }
        impulse(&it.body, dv, it.entry.mass);
    }
}
fn end_pull(l: &mut Lobby, hand: usize) {
    let r = &mut l.items.reach[hand - 1];
    (r.pull, r.pull_for, r.lock) = (None, 0.0, None);
    l.hands.release(hand, WHO);
}
fn reach(l: &mut Lobby, hand: usize, dt: f64, ui: Option<f64>, closed: bool) {
    use super::reach::Kind;
    if let Some(pi) = l.items.reach[hand - 1].pull {
        l.items.reach[hand - 1].pull_for += dt;
        let h = l.hands.h(hand).clone();
        let ok = l.items.items.get(pi).is_some_and(|it| resolve(&mut it.borrow_mut()));
        if !ok || !h.grip || h.pos.is_none() || l.items.reach[hand - 1].pull_for > PULL_LONGEST {
            end_pull(l, hand);
        } else {
            let d = surface(&l.items.items[pi].borrow(), h.pos.unwrap_or([0.0; 3]));
            if d.is_some_and(|d| d < REACH) {
                end_pull(l, hand);
                hold(l, hand, pi, false);
                return;
            }
            fly(l, hand, pi, dt);
            mark(l, hand, pi, Kind::Pull, false);
            return;
        }
    }
    let h = l.hands.h(hand).clone();
    if h.pos.is_none() || l.items.held[hand - 1].item.is_some() || h.owner.is_some_and(|o| o != WHO) {
        (l.items.reach[hand - 1].aim, l.items.reach[hand - 1].lock) = (None, None);
        return;
    }
    if !h.grip {
        l.items.reach[hand - 1].lock = None;
    }
    if let Some(lk) = l.items.reach[hand - 1].lock {
        if claimed(&l.items, hand, lk) {
            l.items.reach[hand - 1].lock = None;
        }
    }
    if let (Some(lk), Some(cvel), Some(hpos)) = (l.items.reach[hand - 1].lock, h.cvel, h.pos) {
        let c = centre(&l.items.items[lk].borrow());
        let away = c.map(|c| sub(hpos, c));
        let ln = away.map_or(0.0, len);
        let speed = if ln > 1e-3 { dot(cvel, mul(away.unwrap_or([0.0; 3]), 1.0 / ln)) } else { 0.0 };
        let r = &mut l.items.reach[hand - 1];
        r.snatch = r.snatch.max(speed);
        if speed >= FLICK {
            (r.pull, r.pull_for) = (Some(lk), 0.0);
            l.items.pulls += 1;
            l.hands.claim(hand, WHO);
            haptic(hand);
            return;
        }
    }
    let (take, _) = nearest(l, hand, REACH);
    let near = take.or_else(|| nearest(l, hand, HOVER).0);
    if let (Some(n), None) = (near, l.items.reach[hand - 1].lock) {
        l.items.reach[hand - 1].aim = None;
        mark(l, hand, n, Kind::Near, take.is_some());
        return;
    }
    let aim = l.items.reach[hand - 1].lock.or_else(|| pointed(l, hand, ui));
    l.items.reach[hand - 1].aim = aim;
    if closed && l.items.reach[hand - 1].lock.is_none() && aim.is_some() {
        (l.items.reach[hand - 1].lock, l.items.reach[hand - 1].snatch) = (aim, 0.0);
        haptic(hand);
    }
    if let Some(a) = aim {
        let kind = if l.items.reach[hand - 1].lock.is_some() { Kind::Lock } else { Kind::Aim };
        mark(l, hand, a, kind, false);
    }
}
pub(super) fn open(l: &mut Lobby, anchor: Option<(V3, f64)>, list: Vec<EntryRef>) {
    close(l);
    let Some(a) = anchor else { return };
    l.items.anchor = Some(a);
    for e in list {
        let it = spawn_item(a, e, None);
        l.items.items.push(it);
    }
}
pub(super) fn close(l: &mut Lobby) {
    for hand in 1..=2 {
        if l.items.held[hand - 1].item.is_some() {
            release(l, hand);
        }
        l.hands.release(hand, WHO);
    }
    l.grips.close();
    sp::clear(TAG);
    let resets = l.items.resets;
    l.items = Items { resets, ..Default::default() };
}
/// M.reset: every item back on its spot.
pub(super) fn reset(l: &mut Lobby) {
    for i in 0..l.items.items.len() {
        if !l.items.items[i].borrow().gone {
            put_back(l, i);
        }
    }
    l.items.resets += 1;
    play("click");
}
pub(super) fn update(l: &mut Lobby, dt: f64, ui: Option<[Option<f64>; 2]>) {
    let Some(a) = l.items.anchor else { return };
    if dt <= 0.0 || dt > 0.2 {
        return;
    }
    for hand in 1..=2 {
        let h = l.hands.h(hand).clone();
        if l.items.held[hand - 1].item.is_some() && (!h.grip || h.pos.is_none()) {
            release(l, hand);
        }
        if let Some(i) = l.items.giving[hand - 1] {
            if l.items.held[hand - 1].item.is_none() && l.items.items.get(i).is_some_and(|it| resolve(&mut it.borrow_mut())) {
                l.items.giving[hand - 1] = None;
                let h = l.hands.h(hand);
                if h.grip && h.pos.is_some() && h.owner.is_none_or(|o| o == WHO) {
                    hold(l, hand, i, false);
                }
            }
        }
        let mut closed = false;
        if l.items.held[hand - 1].item.is_none() && l.hands.taking(hand) {
            closed = !grab(l, hand);
        }
        if l.items.held[hand - 1].item.is_some() {
            carry(l, hand, dt);
        }
        if l.items.held[hand - 1].item.is_none() && l.items.giving[hand - 1].is_none() {
            reach(l, hand, dt, ui.and_then(|u| u[hand - 1]), closed);
        }
    }
    l.grips.update(dt);
    let throws = std::mem::take(&mut l.items.throws);
    for mut t in throws {
        let has = !t.it.borrow().body.is_null();
        if has {
            throw_on(&t);
        }
        t.frames -= 1;
        if t.frames > 0 && has {
            l.items.throws.push(t);
        }
    }
    l.items.lost_at -= dt;
    if l.items.lost_at > 0.0 {
        return;
    }
    l.items.lost_at = LOST_EVERY;
    for i in 0..l.items.items.len() {
        let lost = {
            let it = l.items.items[i].borrow();
            !it.entity.is_null() && !it.gone && world_pos(&it.entity).is_some_and(|p| p[2] < a.0[2] - LOST_BELOW || len(sub(p, it.home)) > LOST_AWAY)
        };
        if lost {
            put_back(l, i);
        }
    }
}
/// M.held: the entry in that hand and where it is.
pub(super) fn held(l: &Lobby, hand: usize) -> Option<(EntryRef, V3)> {
    let it = l.items.items.get(l.items.held[hand - 1].item?)?.borrow();
    if it.entity.is_null() {
        return None;
    }
    Some((it.entry.clone(), world_pos(&it.entity)?))
}
/// M.take: the held one taken away (an engram into the neck).
pub(super) fn take(l: &mut Lobby, hand: usize) -> Option<EntryRef> {
    let i = l.items.held[hand - 1].item?;
    let itr = l.items.items.get(i)?.clone();
    release(l, hand);
    let mut it = itr.borrow_mut();
    despawn_item(&mut it);
    it.gone = true;
    Some(it.entry.clone())
}
pub(super) fn take_entry(l: &mut Lobby, entry: &EntryRef) -> bool {
    for itr in &l.items.items {
        let mut it = itr.borrow_mut();
        if Rc::ptr_eq(&it.entry, entry) && !it.gone {
            despawn_item(&mut it);
            it.gone = true;
            return true;
        }
    }
    false
}
pub(super) fn restore(l: &mut Lobby, entry: &EntryRef) {
    for i in 0..l.items.items.len() {
        let gone = {
            let it = l.items.items[i].borrow();
            Rc::ptr_eq(&it.entry, entry) && it.gone
        };
        if gone {
            put_back(l, i);
        }
    }
}
/// M.give: a taken one back into the hand.
pub(super) fn give(l: &mut Lobby, entry: &EntryRef, hand: usize) -> bool {
    let (Some(hpos), Some(a)) = (l.hands.h(hand).pos, l.items.anchor) else { return false };
    for i in 0..l.items.items.len() {
        let gone = {
            let it = l.items.items[i].borrow();
            Rc::ptr_eq(&it.entry, entry) && it.gone
        };
        if gone {
            l.items.items[i] = spawn_item(a, entry.clone(), Some(hpos));
            l.items.giving[hand - 1] = Some(i);
            l.hands.claim(hand, WHO);
            return true;
        }
    }
    false
}
pub(super) fn state(l: &Lobby) -> String {
    let resolved = l.items.items.iter().filter(|it| !it.borrow().body.is_null()).count();
    format!("items {} resolved {} resets {} pulls {} throw {:.2}", l.items.items.len(), resolved, l.items.resets, l.items.pulls, l.items.last_throw)
}
