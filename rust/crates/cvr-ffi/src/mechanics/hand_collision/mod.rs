//! mods/cet/CyberpunkVRPort_HandCollision as main has it (2026-11): the PhysX hands' script side. Collision response
//! and finger contacts belong to the physics hands (cvr-rig's physhands); this publishes what they meet and maintains
//! what they hold, every frame:
//!   * the people near the player as capsules on their bones (people.rs), and the person's part a physics hand took
//!     pulled by that part's own bones (carry.rs, npc_grip);
//!   * the gun or the blade in a hand as boxes on that hand's body (weapon_body: weapon_shapes.json, cut from the game's
//!     own vertices, with a mounted muzzle and scope);
//!   * V's own trunk padded by what V wears over the chest (self_pad);
//!   * a loose thing a closing grip meets, carried by its own body or by velocity, thrown on letting go, a body lying
//!     dead taken by its limb and its ragdoll put back to rest, the weight slowing the player (carry.rs);
//!   * hair the hands move (hair.rs, cvr-rig's hair_dangle);
//!   * the collision view (view.rs).
//!
//! Lua never displaces a wrist to resolve a capsule or ray collision; the reload's and the grenade's wrist locks keep
//! their own owner (shared [162]). What the mod has switched off by its own constants is not ported: the body capsules'
//! physics (CAPS_PHYS_ON false: they stay off everywhere, toggled once), pulling a held person along the floor
//! (NPC_TUG), a woman's breasts (BREAST_ON: the breasts' graph overrides a written force), the gravity gloves for loose
//! things and their marks (GLOVES_LOOSE), and the carried things' Debug Draw boxes (a CET mod's DebugDraw, not in
//! this build). Main's test hooks (GetMod(...).traceGrasp, canGrabNear, graspState, tune) have no caller here.
mod carry;
mod data;
mod hair;
mod people;
mod view;

use super::lobby_rtti::{self as rt, A};
use crate::script_call::{call_native, method, Ret, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub(super) type V3 = [f32; 3];
pub(super) type Q = [f32; 4];

// ---------------------------------------------------------------- math
pub(super) fn qrot(q: &Q, v: &V3) -> V3 {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + (q[1] * tz - q[2] * ty), v[1] + q[3] * ty + (q[2] * tx - q[0] * tz), v[2] + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
pub(super) fn qmul(a: &Q, b: &Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub(super) fn conj(q: &Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
pub(super) fn add(a: &V3, b: &V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(super) fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(super) fn mul(a: &V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub(super) fn dot(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn len2(a: &V3) -> f32 {
    dot(a, a)
}
pub(super) fn len(a: &V3) -> f32 {
    len2(a).sqrt()
}
pub(super) fn f3(v: &[f32; 4]) -> V3 {
    [v[0], v[1], v[2]]
}
pub(super) fn v4(v: &V3, w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

// ---------------------------------------------------------------- calls
/// One of the plugin's natives, as the script VM calls it.
pub(super) fn nat<T: Ret>(name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: a registered global function, on the main thread where the mechanics tick.
    unsafe { call_native::<T>(name, values) }
}
/// A native returning a handle.
pub(super) fn nat_handle(name: &str, values: &[Value]) -> Handle {
    // SAFETY: as above.
    unsafe { crate::script_call::call_global_handle(core::ptr::null_mut(), name, values) }.unwrap_or_else(Handle::null)
}
/// `o:name(values...)` with a plain result.
pub(super) fn m<T: Ret>(o: &Handle, name: &str, values: &[Value]) -> Option<T> {
    if o.is_null() {
        return None;
    }
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }
}
/// A Vector4 or Quaternion argument, in place.
pub(super) fn vp(v: &[f32; 4]) -> Value {
    Value::Ptr(v.as_ptr().cast())
}
pub(super) fn clock() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}
pub(super) fn shared(i: i32) -> f32 {
    // SAFETY: the shared block's read.
    unsafe { cvr_rig::vrik::provider::shared_slot(i) }
}
/// DEBUG LOGGING, one flag for every bridge: the plugin publishes the launcher's DEBUG state to shared slot [156].
/// Without it the per-second reports produced megabytes a session for players who never asked for them.
pub(super) fn dbg_on() -> bool {
    shared(156) > 0.5
}
pub(super) fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
pub(super) fn entity_hash(e: &Handle) -> Option<u64> {
    m::<u64>(e, "GetEntityID", &[])
}
pub(super) fn world_pos(e: &Handle) -> Option<V3> {
    m::<[f32; 4]>(e, "GetWorldPosition", &[]).map(|v| f3(&v))
}
pub(super) fn world_rot(e: &Handle) -> Option<Q> {
    m::<[f32; 4]>(e, "GetWorldOrientation", &[])
}
pub(super) fn world_fwd(e: &Handle) -> Option<V3> {
    m::<[f32; 4]>(e, "GetWorldForward", &[]).map(|v| f3(&v))
}
pub(super) fn is_a(e: &Handle, class: &str) -> bool {
    super::menu_rt::is_a(e, class)
}
pub(super) fn name_text(n: u64) -> String {
    CName(n).text().to_string()
}
pub(super) fn component_name(c: &Handle) -> String {
    m::<u64>(c, "GetName", &[]).map(name_text).unwrap_or_default()
}
#[repr(C, align(16))]
struct WorldTransform([u32; 8]);
/// `comp:GetSlotTransform(name)`: the slot's world place and turn, None when it does not answer.
pub(super) fn slot_transform(comp: &Handle, name: CName) -> Option<(V3, Q)> {
    let mut wt = WorldTransform([0; 8]);
    if m::<bool>(comp, "GetSlotTransform", &[Value::U64(name.0), Value::Out((&mut wt as *mut WorldTransform).cast())]) != Some(true) {
        return None;
    }
    let p = [0, 1, 2].map(|i| wt.0[i] as i32 as f32 / 131072.0);
    let q = [4, 5, 6, 7].map(|i| f32::from_bits(wt.0[i]));
    Some((p, q))
}
/// The NPCs and other entities the targeting system finds within `max` of the player (TSQ_ALL, the complete set,
/// the player left out), each once.
pub(super) fn targets_near(pl: &Handle, max: f32) -> Vec<Handle> {
    let mut out = Vec::new();
    let Some(mut q) = rt::global_call("TSQ_ALL", &[]) else { return out };
    let qv = super::menu_rt::val(&q);
    super::menu_rt::set(&qv, "maxDistance", f64::from(max));
    super::menu_rt::set(&qv, "testedSet", super::menu_rt::ev("TargetingSet", "Complete"));
    super::menu_rt::set(&qv, "ignoreInstigator", 1.0);
    let targeting = super::game_system("GetTargetingSystem");
    let Some((_, outs)) = rt::call_outs(&targeting, "GetTargetParts", &[A::H(pl), A::P(q.as_mut_ptr().cast_const().cast())], 1) else { return out };
    let Some(parts) = outs.first() else { return out };
    let mut seen = HashSet::new();
    for part in super::menu_rt::items(&rt::Val::of(parts)) {
        let comp = rt::static_call("TS_TargetPartInfo", "GetComponent", &[A::P(part.ptr.cast_const())]).map(|o| rt::Val::of(&o).handle()).unwrap_or_else(Handle::null);
        let e = super::menu_rt::o(&comp, "GetEntity", &[]);
        if e.is_null() {
            continue;
        }
        let Some(id) = entity_hash(&e) else { continue };
        if seen.insert(id) {
            out.push(e);
        }
    }
    out
}

/// The world frame: the player's place and the world -> model turn (the conjugate of its orientation). World -> model
/// through the PLAYER's transform, not VRWorldToModel: that is anchored on the render-view pair, and VRViewWorldPos sits
/// about a metre below the real camera.
#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub b: V3,
    pub i: Q,
}
impl Frame {
    pub fn to_model(&self, w: &V3) -> V3 {
        qrot(&self.i, &sub(w, &self.b))
    }
    pub fn to_world(&self, m: &V3) -> V3 {
        add(&qrot(&conj(&self.i), m), &self.b)
    }
    pub fn dir_to_world(&self, m: &V3) -> V3 {
        qrot(&conj(&self.i), m)
    }
}
fn world_frame() -> Option<Frame> {
    let pl = player();
    let b = world_pos(&pl)?;
    let q = world_rot(&pl)?;
    Some(Frame { b, i: conj(&q) })
}

// ---------------------------------------------------------------- the state
/// THE WEAPON'S BODY on the holding hand (K.weaponBody): what was sent, and the gun's frame in the hand bone's.
#[derive(Default)]
struct WeaponBody {
    key: Option<u64>,
    parts: Option<Vec<[f32; 6]>>,
    parts_at: f64,
    frame: Option<[f32; 7]>,
    frame_at: f64,
    side: i32,
    sent_at: f64,
    said: Option<String>,
    last: Option<V3>,
}
/// WHAT A FRAME COSTS, by the part of this mod that spent it, every 10 s; and at once when a frame took long.
#[derive(Default)]
struct Cost {
    now: HashMap<&'static str, f64>,
    sum: HashMap<&'static str, f64>,
    most: HashMap<&'static str, f64>,
    ticks: u32,
    said: f64,
    spikes: u32,
    last: Option<f64>,
}
const PF_LONG: f64 = 0.04;
struct State {
    pub gen: u32,
    cap_on: bool,
    owner_cleared: bool,
    bones: Vec<(u32, Option<(V3, Q)>)>,
    frame: (u32, Option<Frame>),
    /// the tracked wrists this frame (model space), and the last place each was seen (PX, PY, PZ)
    pub raw: [Option<V3>; 2],
    pub px: [Option<V3>; 2],
    /// a hand this frame's carrying must leave alone
    pub skip: [bool; 2],
    /// the hands have PhysX bodies of their own (VRHandCollision's 64)
    pub phys: bool,
    // the weapon
    pub last_w: Handle,
    w_miss: u32,
    slot_comp: Handle,
    slot_ent: Option<u64>,
    wgn: usize,
    slot_holder: Option<i32>,
    slot_holder_for: Option<u64>,
    holder_side: Option<i32>,
    holder_ent: Option<u64>,
    dbg_dl: f32,
    dbg_dr: f32,
    ranged_key: Option<u64>,
    ranged_answer: bool,
    ranged_wait: u32,
    wb: WeaponBody,
    // the body's capsule components, kept off
    vb_comps: Vec<Handle>,
    vb_scan_at: f64,
    vb_wanted: Option<bool>,
    vb_empty_at: f64,
    template_player: Option<u64>,
    self_pad_at: Option<f64>,
    self_pad: Option<f32>,
    state_key: Option<i64>,
    // the tracer and the once-a-second report
    dbg_acc: f32,
    trace_on: bool,
    stage: &'static str,
    note: String,
    log_acc: f32,
    ms_acc: f64,
    ms_max: f64,
    ms_n: u32,
    cost: Cost,
    said: HashSet<String>,
    people: people::People,
    carry: carry::Carry,
    hair: hair::Hair,
    view: view::View,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

impl State {
    fn new() -> State {
        State {
            gen: 0,
            cap_on: false,
            owner_cleared: false,
            bones: vec![(u32::MAX, None); 256],
            frame: (u32::MAX, None),
            raw: [None; 2],
            px: [None; 2],
            skip: [true; 2],
            phys: false,
            last_w: Handle::null(),
            w_miss: 0,
            slot_comp: Handle::null(),
            slot_ent: None,
            wgn: 0,
            slot_holder: None,
            slot_holder_for: None,
            holder_side: None,
            holder_ent: None,
            dbg_dl: -1.0,
            dbg_dr: -1.0,
            ranged_key: None,
            ranged_answer: false,
            ranged_wait: 0,
            wb: WeaponBody { parts_at: -10.0, frame_at: -10.0, side: -1, sent_at: -10.0, ..WeaponBody::default() },
            vb_comps: Vec::new(),
            vb_scan_at: -1.0,
            vb_wanted: None,
            vb_empty_at: -10.0,
            template_player: None,
            self_pad_at: None,
            self_pad: None,
            state_key: None,
            dbg_acc: 0.0,
            trace_on: false,
            stage: "never-ran",
            note: String::new(),
            log_acc: 0.0,
            ms_acc: 0.0,
            ms_max: 0.0,
            ms_n: 0,
            cost: Cost::default(),
            said: HashSet::new(),
            people: people::People::new(),
            carry: carry::Carry::new(),
            hair: hair::Hair::default(),
            view: view::View::default(),
        }
    }
    /// sayOnce: a first in the log, once a session.
    pub fn say_once(&mut self, what: &str) {
        if self.said.insert(what.to_string()) {
            log!("[HandColl] first: {what}\n");
        }
    }
    /// The world frame, read once a frame.
    pub fn frame(&mut self) -> Option<Frame> {
        if self.frame.0 != self.gen {
            self.frame = (self.gen, world_frame());
        }
        self.frame.1
    }
    /// A bone's model-space pose, read once per bone per frame (the bone snapshot VRBoneModelPos/Rot reads).
    pub fn bone(&mut self, idx: i32) -> Option<(V3, Q)> {
        if !(0..256).contains(&idx) {
            return None;
        }
        let slot = &mut self.bones[idx as usize];
        if slot.0 == self.gen {
            return slot.1;
        }
        slot.0 = self.gen;
        let p = nat::<[f32; 4]>("VRBoneModelPos", &[Value::Int32(idx)]);
        let q = nat::<[f32; 4]>("VRBoneModelRot", &[Value::Int32(idx)]);
        slot.1 = match (p, q) {
            (Some(p), Some(q)) if p[3] >= 0.5 => Some((f3(&p), q)),
            _ => None,
        };
        slot.1
    }
    /// A capsule's two ends in model space (its bone's frame: the centre c, half its length along the bone's X).
    pub fn endpoints(&mut self, c: usize) -> Option<(V3, V3)> {
        let cap = &data::data().caps[c];
        let (p, q) = self.bone(cap.index)?;
        let x = qrot(&q, &[1.0, 0.0, 0.0]);
        let centre = add(&p, &qrot(&q, &cap.c));
        Some((sub(&centre, &mul(&x, cap.half)), add(&centre, &mul(&x, cap.half))))
    }
    fn pf(&mut self, name: &'static str) {
        let c = clock();
        let spent = c - self.cost.last.unwrap_or(c);
        *self.cost.now.entry(name).or_insert(0.0) += spent;
        self.cost.last = Some(c);
    }
    fn pf_begin(&mut self) {
        let c = clock();
        let total: f64 = self.cost.now.values().sum();
        for (name, spent) in &self.cost.now {
            *self.cost.sum.entry(name).or_insert(0.0) += spent;
            let most = self.cost.most.entry(name).or_insert(0.0);
            if *spent > *most {
                *most = *spent;
            }
        }
        if total > PF_LONG && self.cost.spikes < 3 {
            self.cost.spikes += 1;
            if dbg_on() {
                let rows: Vec<String> = self.cost.now.iter().filter(|(_, s)| **s > 0.002).map(|(n, s)| format!("{n} {:.0}", s * 1000.0)).collect();
                log!("[cost] a long frame, {:.0} ms: {}\n", total * 1000.0, rows.join(", "));
            }
        }
        for v in self.cost.now.values_mut() {
            *v = 0.0;
        }
        self.cost.ticks += 1;
        if c - self.cost.said > 10.0 {
            let (ticks, over) = (f64::from(self.cost.ticks), c - self.cost.said);
            if dbg_on() {
                let mut rows: Vec<String> = self
                    .cost
                    .sum
                    .iter()
                    .filter(|(n, s)| **s * 1000.0 / ticks > 0.05 || self.cost.most.get(*n).copied().unwrap_or(0.0) > 0.01)
                    .map(|(n, s)| format!("{n} {:.2} ({:.0})", s * 1000.0 / ticks, self.cost.most.get(n).copied().unwrap_or(0.0) * 1000.0))
                    .collect();
                rows.sort();
                log!("[cost] {:.0} ticks a second; ms a tick (most): {}\n", ticks / over, rows.join(", "));
            }
            self.cost.sum.clear();
            self.cost.most.clear();
            self.cost.ticks = 0;
            self.cost.said = c;
            self.cost.spikes = 0;
        }
        self.cost.last = Some(c);
    }

    // ------------------------------------------------------------ the weapon
    /// heldWeapon(): GetActiveWeapon() is not reliable frame to frame, so the last weapon is kept and re-validated
    /// cheaply -- can its world position still be read -- and only given up after a run of failures (W_GIVE_UP
    /// frames), which is what holstering actually looks like.
    pub fn held_weapon(&mut self) -> Handle {
        const W_GIVE_UP: u32 = 30;
        let pl = player();
        if pl.is_null() {
            self.last_w = Handle::null();
            self.w_miss = 0;
            return Handle::null();
        }
        let w = active_weapon(&pl);
        if !w.is_null() {
            self.last_w = w.clone();
            self.w_miss = 0;
            return w;
        }
        if !self.last_w.is_null() {
            if world_pos(&self.last_w).is_some() {
                self.w_miss += 1;
                if self.w_miss < W_GIVE_UP {
                    return self.last_w.clone();
                }
            }
            self.last_w = Handle::null();
            self.w_miss = 0;
        }
        Handle::null()
    }
    /// readWeapon(full): how many of the held weapon's part slots (weapon_silverhand.json: one capsule per moving part,
    /// a vrp_<bone> slot each) answer; `full` false stops at the first. The slot component is found by asking: a weapon
    /// carries several, and only the one that answers the first part's slot is it.
    fn read_weapon(&mut self, full: bool) -> usize {
        self.wgn = 0;
        let d = data::data();
        if d.silver.is_empty() || player().is_null() {
            return 0;
        }
        let w = self.held_weapon();
        if w.is_null() {
            self.slot_comp = Handle::null();
            self.slot_ent = None;
            return 0;
        }
        if self.frame().is_none() {
            return 0;
        }
        let key = entity_hash(&w);
        if key != self.slot_ent {
            self.slot_comp = Handle::null();
            self.slot_ent = key;
        }
        if self.slot_comp.is_null() {
            for c in crate::engine_objects::entity_components(&w) {
                let class = m::<u64>(&c, "GetClassName", &[]).map(name_text).unwrap_or_default();
                if class.contains("SlotComponent") && slot_transform(&c, d.silver[0].slot).is_some() {
                    self.slot_comp = c;
                    break;
                }
            }
            if self.slot_comp.is_null() {
                return 0;
            }
        }
        let mut n = 0;
        for c in &d.silver {
            if slot_transform(&self.slot_comp, c.slot).is_some() {
                n += 1;
                if !full {
                    break;
                }
            }
        }
        self.wgn = n;
        n
    }
    /// holderFromSlot(): which hand holds the gun, asked of the game: a held weapon is attached to WeaponRight or
    /// WeaponLeft. Cached per weapon, because it cannot change without the weapon changing.
    fn holder_from_slot(&mut self) -> Option<i32> {
        let pl = player();
        let w = if pl.is_null() { Handle::null() } else { active_weapon(&pl) };
        if w.is_null() {
            self.slot_holder = None;
            self.slot_holder_for = None;
            return None;
        }
        let wid = entity_hash(&w)?;
        if self.slot_holder_for == Some(wid) {
            return self.slot_holder;
        }
        let mut found = None;
        for (side, slot) in [(1, "AttachmentSlots.WeaponRight"), (0, "AttachmentSlots.WeaponLeft")] {
            let item = super::belt::in_slot(&pl, slot);
            if !item.is_null() && entity_hash(&item) == Some(wid) {
                found = Some(side);
                break;
            }
        }
        if found.is_some() {
            self.slot_holder = found;
            self.slot_holder_for = Some(wid);
        }
        found
    }
    /// holderOf(): the game's own answer wins; without one, the right hand is (the left is only ever a support hand).
    fn holder_of(&mut self) -> Option<i32> {
        const HOLDER_DROP: f32 = 0.30;
        // the grip's model place is no longer written (the weapon's capsules moved to their own bones): these distances
        // are from the player's feet, as main's
        let dist = |v: Option<V3>| v.map_or(-1.0, |v| len(&v));
        if let Some(s) = self.holder_from_slot() {
            self.holder_side = Some(s);
            self.holder_ent = self.slot_ent;
            self.dbg_dl = dist(self.raw[0]);
            self.dbg_dr = dist(self.raw[1]);
            return Some(s);
        }
        if self.slot_ent != self.holder_ent {
            self.holder_side = None;
            self.holder_ent = self.slot_ent;
        }
        self.dbg_dl = dist(self.raw[0]);
        self.dbg_dr = dist(self.raw[1]);
        if let Some(h) = self.holder_side {
            let d = if h == 0 { self.dbg_dl } else { self.dbg_dr };
            if (0.0..=HOLDER_DROP).contains(&d) {
                return Some(h);
            }
            self.holder_side = None;
        }
        if self.raw[1].is_some() {
            self.holder_side = Some(1);
        } else if self.raw[0].is_some() {
            self.holder_side = Some(0);
        }
        self.holder_side
    }
    /// Whether the weapon in the hand is a gun. Asked again until it says yes: one just drawn answers no for a few frames.
    pub fn weapon_ranged(&mut self, w: &Handle) -> bool {
        let key = entity_hash(w);
        if key != self.ranged_key {
            self.ranged_key = key;
            self.ranged_answer = false;
            self.ranged_wait = 0;
        }
        if self.ranged_answer {
            return true;
        }
        if self.ranged_wait > 0 {
            self.ranged_wait -= 1;
            return false;
        }
        self.ranged_wait = 20;
        self.ranged_answer = m::<bool>(w, "IsRanged", &[]) == Some(true);
        self.ranged_answer
    }
    fn weapon_body_clear(&mut self) {
        if self.wb.side >= 0 {
            // SAFETY: the physics hands' script-side staging.
            unsafe { cvr_rig::vrik::physhands::api::set_weapon(self.wb.side as usize, 0, Some(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])) };
        }
        self.wb.side = -1;
    }
    /// THE GUN'S BODY: a gun in a hand is a few boxes (weapon_shapes.json, the blades and batons the same way) on the
    /// PhysX body of that hand: the other hand stops on them and lies on them; the world does not meet them. The boxes
    /// are in the gun's frame (origin at the grip, +Y along the barrel, +Z up); the gun's frame is measured in the hand
    /// bone's, a hand still enough that the bone and the entity, read at different moments, agree.
    fn weapon_body(&mut self, holder: Option<i32>) {
        let w = self.held_weapon();
        let hand = holder.and_then(|h| data::data().hand[h as usize]);
        let (Some(holder), Some(hand), false) = (holder, hand, w.is_null()) else {
            self.weapon_body_clear();
            self.wb.key = None;
            return;
        };
        let now = clock();
        let key = entity_hash(&w);
        if key != self.wb.key {
            self.wb = WeaponBody { key, parts_at: -10.0, frame_at: -10.0, side: self.wb.side, sent_at: self.wb.sent_at, said: self.wb.said.take(), last: self.wb.last, ..WeaponBody::default() };
        }
        // the outline: asked until it answers, then again every couple of seconds (what is mounted can change)
        if now - self.wb.parts_at > if self.wb.parts.is_some() { 2.0 } else { 0.3 } {
            self.wb.parts_at = now;
            if let Some((parts, family)) = weapon_parts(&w) {
                if self.wb.said.as_deref() != Some(&family) {
                    log!("[HandColl] gun body: {}, {} boxes\n", family, parts.len());
                    self.wb.said = Some(family);
                }
                self.wb.parts = Some(parts);
            }
        }
        if self.wb.parts.is_none() {
            return;
        }
        // the gun's frame in the hand bone's: measured while the hand is still (under 3 mm since the last frame)
        let Some((hp, hq)) = self.bone(data::data().caps[hand].index) else { return };
        let still = self.wb.last.is_some_and(|l| (hp[0] - l[0]).abs() + (hp[1] - l[1]).abs() + (hp[2] - l[2]).abs() < 0.003);
        self.wb.last = Some(hp);
        if (self.wb.frame.is_none() && now - self.wb.frame_at > 0.1) || (still && now - self.wb.frame_at > 1.0) {
            if let (Some(f), Some(ep), Some(eq)) = (self.frame(), world_pos(&w), world_rot(&w)) {
                let r = sub(&f.to_model(&ep), &hp);
                if len2(&r) < 0.25 {
                    let g = qrot(&conj(&hq), &r);
                    let mq = qmul(&f.i, &eq);
                    let fq = qmul(&conj(&hq), &mq);
                    self.wb.frame = Some([g[0], g[1], g[2], fq[0], fq[1], fq[2], fq[3]]);
                }
            }
            self.wb.frame_at = now;
        }
        let Some(f) = self.wb.frame else { return };
        if self.wb.side != holder {
            self.weapon_body_clear();
        }
        if self.wb.side != holder || now - self.wb.sent_at > 0.5 {
            let parts = self.wb.parts.as_ref().expect("checked");
            for (i, b) in parts.iter().enumerate() {
                // SAFETY: the physics hands' script-side staging.
                unsafe { cvr_rig::vrik::physhands::api::set_weapon_part(holder as usize, i, &[b[0], b[2], b[4]], &[b[1], b[3], b[5]]) };
            }
            // SAFETY: as above; the frame as VRPhysHandWeapon passes it (turn x y z w, then the place).
            unsafe { cvr_rig::vrik::physhands::api::set_weapon(holder as usize, parts.len() as i32, Some(&[f[3], f[4], f[5], f[6], f[0], f[1], f[2]])) };
            self.wb.side = holder;
            self.wb.sent_at = now;
        }
    }
    /// Old player templates may still contain the previous kinematic push capsules: kept disabled, the dynamic PhysX
    /// hand owns every contact.
    fn disable_template_hands(&mut self) {
        let pl = player();
        let Some(id) = entity_hash(&pl) else {
            self.template_player = None;
            return;
        };
        if self.template_player == Some(id) {
            return;
        }
        for c in crate::engine_objects::entity_components(&pl) {
            let name = component_name(&c);
            if name == "VRPortHand_l" || name == "VRPortHand_r" {
                m::<()>(&c, "Toggle", &[Value::Bool(false)]);
            }
        }
        self.template_player = Some(id);
    }
    /// V'S OWN BODY AS THE PHYSICS HANDS MEET IT is padded by what V wears over the chest: a coat or a jacket 2.5 cm,
    /// other outer wear 1.8 (a vest 1.2), a shirt alone 0.6, nothing 0. By the items in the torso's two slots (the outer
    /// and the inner), every two seconds.
    fn self_pad(&mut self) {
        let now = clock();
        if self.self_pad_at.is_some_and(|t| now - t < 2.0) {
            return;
        }
        self.self_pad_at = Some(now);
        let p = player();
        if p.is_null() {
            return;
        }
        let worn = |slot: &str| -> Option<String> {
            let item = super::belt::in_slot(&p, slot);
            if item.is_null() {
                return None;
            }
            let id = rt::call(&item, "GetItemID", &[]).map(|o| {
                // SAFETY: an ItemID result: its TweakDBID first.
                unsafe { *(o.as_ptr() as *const u64) & 0xFF_FFFF_FFFF }
            });
            // TDBID.ToStringDEBUG: the name, else the id as the debug string has it
            let id = id.unwrap_or(0);
            Some(crate::tdb_names::name_of(id).unwrap_or_else(|| format!("<TDBID:{:08X}:{:02X}>", id as u32, (id >> 32) as u8)).to_lowercase())
        };
        let (outer, inner) = (worn("AttachmentSlots.Torso"), worn("AttachmentSlots.Chest"));
        let pad = match (&outer, &inner) {
            (Some(o), _) if ["coat", "jacket", "bomber", "puffer"].iter().any(|w| o.contains(w)) => 0.025,
            (Some(o), _) if o.contains("vest") => 0.012,
            (Some(_), _) => 0.018,
            (None, Some(_)) => 0.006,
            (None, None) => 0.0,
        };
        cvr_rig::vrik::physhands::people::set_self_pad(pad);
        if self.self_pad != Some(pad) {
            self.self_pad = Some(pad);
            let show = |s: &Option<String>| s.clone().unwrap_or_else(|| "nil".into());
            log!("[HandColl] V's own body padded {:.1} cm (outer {}, inner {})\n", pad * 100.0, show(&outer), show(&inner));
        }
    }
    /// bodyCapsulesForVehicle(): the body's capsule components (VRPortBody_*, from the archive) kept off: their physics
    /// is off everywhere (CAPS_PHYS_ON false), toggled once; rescanned a second after none were found or when the scan is
    /// 5 s old.
    fn body_capsules(&mut self, now: f64) {
        let want = false;
        if self.vb_wanted == Some(want) {
            return;
        }
        if self.vb_comps.is_empty() && now - self.vb_empty_at < 1.0 {
            return;
        }
        if self.vb_comps.is_empty() || now - self.vb_scan_at > 5.0 {
            let pl = player();
            let mut found = Vec::new();
            if !pl.is_null() && nat::<bool>("VRShouldScanBodyCapsules", &[Value::Handle(&pl)]) != Some(false) {
                for c in crate::engine_objects::entity_components(&pl) {
                    if component_name(&c).contains("VRPortBody_") {
                        found.push(c);
                    }
                }
            }
            if found.is_empty() {
                self.vb_empty_at = now;
                return;
            }
            self.vb_comps = found;
            self.vb_scan_at = now;
        }
        for c in &self.vb_comps {
            m::<()>(c, "Toggle", &[Value::Bool(want)]);
        }
        self.vb_wanted = Some(want);
        log!("[BodyCaps] OFF -- mounted ({} capsules)\n", self.vb_comps.len());
    }
}
fn active_weapon(pl: &Handle) -> Handle {
    crate::script_call::static_function("ScriptedPuppet", "GetActiveWeapon")
        // SAFETY: the scripted static with the player held.
        .and_then(|f| unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(pl)]) })
        .unwrap_or_else(Handle::null)
}
/// The gun's family as its mesh components name it: w_handgun__arasaka_kenshin__base1_01 -> w_handgun__arasaka_kenshin
/// (main's '^(w_[%w_]-__[%w_]-)__').
fn family_of(name: &str) -> Option<&str> {
    if !name.starts_with("w_") {
        return None;
    }
    let first = name[2..].find("__")? + 2;
    let second = name[first + 2..].find("__")? + first + 2;
    let fam = &name[..second];
    fam.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_').then_some(fam)
}
/// A melee weapon's handle: how far its boxes (weapon_shapes.json, in the weapon's frame: the origin at the grip, the
/// blade along +Z) reach toward the pommel, by the mesh family its components name; None for a family without boxes.
pub(in crate::mechanics) fn melee_handle(e: &Handle) -> Option<f64> {
    let d = data::data();
    for c in crate::engine_objects::entity_components(e) {
        let name = component_name(&c).to_lowercase();
        if let Some(shapes) = family_of(&name).and_then(|f| d.shapes.get(f)) {
            let lowest = shapes.iter().map(|b| b[4]).fold(0.0f32, f32::min);
            return (lowest < -0.02).then_some(f64::from(-lowest));
        }
    }
    None
}
/// An attachment's key as its mesh components name it: w_att__scope_long_01_... -> w_att__scope_long_01,
/// w_att__power_03_01 -> w_att__power_03 (main's '^(w_att_+%a+_%a+_%d+)' or '^(w_att_+%a+_%d+)').
fn attachment_of(name: &str) -> Option<&str> {
    let b = name.as_bytes();
    if !name.starts_with("w_att_") {
        return None;
    }
    let n = b.len();
    let mut i = 5;
    while i < n && b[i] == b'_' {
        i += 1;
    }
    let s1 = i;
    while i < n && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == s1 || i >= n || b[i] != b'_' {
        return None;
    }
    i += 1;
    let mut j = i;
    while j < n && b[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j > i && j < n && b[j] == b'_' {
        let mut k = j + 1;
        while k < n && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > j + 1 {
            return Some(&name[..k]);
        }
    }
    let mut k = i;
    while k < n && b[k].is_ascii_digit() {
        k += 1;
    }
    (k > i).then(|| &name[..k])
}
/// weaponParts(w): the gun's boxes, with a mounted muzzle on from the front of the foremost box and a scope standing on
/// top of the middle; 14 at the most (the first ones dropped).
fn weapon_parts(w: &Handle) -> Option<(Vec<[f32; 6]>, String)> {
    let d = data::data();
    let mut family = None;
    let (mut reach, mut up, mut wide) = (0.0f32, 0.0f32, 0.0f32);
    for c in crate::engine_objects::entity_components(w) {
        let name = component_name(&c).to_lowercase();
        if let Some(fam) = family_of(&name).filter(|f| d.shapes.contains_key(*f)) {
            family = Some(fam.to_string());
        }
        if let Some(att) = attachment_of(&name).and_then(|k| d.atts.get(k).map(|a| (k.contains("power"), a))) {
            let on = m::<bool>(&c, "IsEnabled", &[]);
            if on != Some(false) {
                let (power, a) = att;
                if power {
                    reach = reach.max(a[3]);
                } else {
                    up = up.max(a[5]);
                    wide = wide.max(a[1]).max(-a[0]);
                }
            }
        }
    }
    let family = family?;
    let shapes = &d.shapes[&family];
    let first = *shapes.first()?;
    let mut parts = Vec::with_capacity(shapes.len() + 2);
    let (mut front, mut lo, mut hi) = (first, first[2], first[3]);
    for s in shapes {
        parts.push(*s);
        if s[3] > front[3] {
            front = *s;
        }
        lo = lo.min(s[2]);
        hi = hi.max(s[3]);
    }
    if reach > 0.0 {
        let (cx, cz) = ((front[0] + front[1]) * 0.5, (front[4] + front[5]) * 0.5);
        parts.push([cx - 0.025, cx + 0.025, front[3], front[3] + reach, cz - 0.025, cz + 0.025]);
    }
    if up > 0.0 {
        let (mid, half) = ((lo + hi) * 0.5, wide.max(0.02));
        let top = shapes.iter().fold(front[5], |t, s| t.max(s[5]));
        parts.push([-half, half, mid - 0.08, mid + 0.08, top, top + up]);
    }
    while parts.len() > 14 {
        parts.remove(0);
    }
    Some((parts, family))
}
/// The collision-off release of a free hand's legacy stop.
fn release_hand(side: i32) {
    let zero = [0f32; 4];
    nat::<()>("VRHandStopModel", &[Value::Int32(side), Value::Bool(false), vp(&zero)]);
}

/// canGrabNear (main's HandCollision API, asked by Reach): the hand (0 left, 1 right) has a physical body it could
/// grip at its palm right now -- its PhysX hand on (the mask's side, carry, phys and own-body bits), not holding, not
/// left alone this frame, and the carry's probe finding something.
pub(in crate::mechanics) fn can_grab_near(side: usize) -> bool {
    if side > 1 {
        return false;
    }
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let Some(s) = g.as_mut() else { return false };
    if !s.phys || s.carry.hold[side].is_some() || s.skip[side] {
        return false;
    }
    let mask = nat::<i32>("VRHandCollision", &[]).unwrap_or(0);
    if (mask >> side) & 1 != 1 || (mask >> 3) & 1 != 1 || (mask >> 5) & 1 != 1 || (mask >> 6) & 1 != 1 {
        return false;
    }
    let Some(f) = world_frame() else { return false };
    carry::probe(s, side, &f)
}

/// The mod's onUpdate.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(State::new);
    let dt = if dt > 0.0 { dt } else { 0.016 };
    s.dbg_acc += dt;
    if s.dbg_acc >= 1.0 {
        s.dbg_acc = 0.0;
        s.trace_on = dbg_on();
        if s.trace_on {
            log!("[Tick] stage={} {} ownHand={} collOn={}\n", s.stage, s.note, shared(162), s.carry.coll_on);
        }
    }
    let t0 = clock();
    s.stage = "natives";
    s.note.clear();
    if !s.cap_on && nat::<i32>("SetVRDiagCapture", &[Value::Int32(1)]).is_some() {
        s.cap_on = true;
    }
    s.gen = s.gen.wrapping_add(1);
    // THE SLOT STARTS AT 0, WHICH READS AS "THE LEFT WRIST IS TAKEN": the reload writes it only on a change.
    if !s.owner_cleared && nat::<()>("SetVRReloadOwnedHand", &[Value::Int32(-1)]).is_some() {
        s.owner_cleared = true;
    }
    let coll_mask = nat::<i32>("VRHandCollision", &[]).unwrap_or(0);
    let coll_on = coll_mask % 4 != 0;
    s.carry.coll_on = coll_on;
    if !coll_on {
        carry::suspend(s);
    }
    let world_on = (coll_mask / 4) % 2 == 1;
    let push_allowed = (coll_mask / 8) % 2 == 1;
    // 64: the hands have PhysX bodies of their own: they push, and the template's capsules stay off
    let hand_bodies = (coll_mask / 64) % 2 == 1;
    s.phys = hand_bodies;
    s.disable_template_hands();
    if hand_bodies {
        s.self_pad();
    }
    let read = |side: i32| nat::<[f32; 4]>("VRHandRawModel", &[Value::Int32(side)]);
    let (rl, rr) = (read(0), read(1));
    s.stage = "hands";
    if s.trace_on {
        s.note = format!("Lw={:.1} Rw={:.1}", rl.map_or(-1.0, |v| v[3]), rr.map_or(-1.0, |v| v[3]));
    }
    let rl = rl.filter(|v| v[3] >= 0.5).map(|v| f3(&v));
    let rr = rr.filter(|v| v[3] >= 0.5).map(|v| f3(&v));
    if rl.is_none() && rr.is_none() {
        return;
    }
    s.pf_begin();
    s.raw = [rl, rr];
    for side in 0..2 {
        if let Some(r) = s.raw[side] {
            s.px[side] = Some(r);
        }
    }
    let mut holder = None;
    // With the collision off the slots, and the holder, only feed the tracer's note. All of the weapon's part slots
    // are counted only for the once-a-second report that shows the count.
    let have_gun = s.read_weapon(coll_on && dbg_on()) > 0;
    nat::<()>("VRPoseCensus", &[Value::Int32(-4), Value::Int32(i32::from(have_gun))]);
    // Only the Silverhand has capsules (have_gun); any weapon in hand has a holder, who must not count as an empty hand.
    if (have_gun || (coll_on && !s.held_weapon().is_null())) && (coll_on || s.trace_on) {
        holder = s.holder_of();
        if holder.is_some_and(|h| s.raw[h as usize].is_none() || data::data().hand[h as usize].is_none()) {
            holder = None;
        }
    }
    if hand_bodies {
        s.weapon_body(holder);
    }
    // COLLISION OFF: release both hands so nothing clamps them -- except the wrist the reload holds (shared [162]).
    let v = shared(162);
    let own = (v == 0.0 || v == 1.0).then_some(v as i32);
    if !coll_on {
        s.stage = "colloff";
        if s.trace_on {
            s.note = format!("gun={have_gun} holder={holder:?} slotComp={}", !s.slot_comp.is_null());
        }
        for side in 0..2 {
            if own != Some(side) {
                release_hand(side);
            }
        }
        s.skip = [true, true];
        s.body_capsules(clock());
        return;
    }
    // THE HAND THE RELOAD OWNS: while it glues a wrist to a weapon part that wrist has exactly one writer.
    s.skip[0] = own == Some(0) || coll_mask % 2 == 0;
    s.skip[1] = own == Some(1) || (coll_mask / 2) % 2 == 0;
    // WHAT THE SOLVE IS WORKING WITH, once per change.
    let state_key = i64::from(coll_mask) * 1000
        + i64::from(holder.unwrap_or(2) + 1) * 100
        + i64::from(own.unwrap_or(2) + 1) * 10
        + i64::from(s.skip[0])
        + 2 * i64::from(s.skip[1])
        + 4 * i64::from(!s.last_w.is_null());
    if s.state_key != Some(state_key) {
        s.state_key = Some(state_key);
        cvr_host::vlog!(
            "[HandColl] state mask={} holder={} owned={} skipL={} skipR={} weapon={}\n",
            coll_mask,
            holder.map_or("nil".into(), |h| h.to_string()),
            own.map_or("nil".into(), |h| h.to_string()),
            s.skip[0],
            s.skip[1],
            !s.last_w.is_null()
        );
    }
    // People, on foot, with empty hands or a gun.
    s.people.n = 0;
    let mut npc_on = world_on;
    if npc_on && holder.is_some() && !s.last_w.is_null() {
        let w = s.last_w.clone();
        npc_on = s.weapon_ranged(&w);
    }
    s.pf("reading");
    if npc_on {
        people::place(s, dt);
    }
    people::publish(s);
    s.pf("people list");
    if let Some(f) = s.frame() {
        carry::track_hands(s, dt, &f);
        hair::hair(s, dt, &f);
    }
    s.pf("tracking");
    // PhysX owns collision response: a previous legacy stop cleared on every free hand.
    for side in 0..2 {
        if own != Some(side) {
            release_hand(side);
        }
    }
    s.pf("publishing");
    if let Some(f) = s.frame() {
        carry::carry(s, hand_bodies && push_allowed && (coll_mask / 32) % 2 == 1, holder, dt, &f);
        view::debug_draw(s, &f);
        s.pf("the view");
    }
    // Body capsules are switched at the component level.
    s.body_capsules(clock());
    let ms = (clock() - t0) * 1000.0;
    s.ms_acc += ms;
    s.ms_n += 1;
    s.ms_max = s.ms_max.max(ms);
    s.log_acc += dt;
    if s.log_acc >= 1.0 && dbg_on() {
        s.log_acc = 0.0;
        log!(
            "[HandColl] PhysX={} holder={} (grip L={:.3} R={:.3}) caps={} gun={} miss={} update {:.2}/{:.2} ms\n",
            hand_bodies,
            holder.map_or("nil".into(), |h| h.to_string()),
            s.dbg_dl,
            s.dbg_dr,
            data::data().caps.len(),
            s.wgn,
            s.w_miss,
            if s.ms_n > 0 { s.ms_acc / f64::from(s.ms_n) } else { -1.0 },
            s.ms_max
        );
        // THE CENSUS: the weapon's two rigs, nominated by the (bones, tracks) pair the pose descriptor carries.
        let st = |i: i32| nat::<i32>("VRRigStatus", &[Value::Int32(i)]).unwrap_or(0);
        let rows: Vec<String> = (0..2)
            .map(|w| format!("{} seen={} bones={} tracks={} buf={:08X}{:08X}", if w == 0 { "mag" } else { "frame" }, st(w), st(2 + w), st(4 + w), st(8 + w) as u32, st(6 + w) as u32))
            .collect();
        log!("[Rig] weapon={} | {}\n", if s.wgn > 0 { "yes" } else { "no" }, rows.join(" | "));
        s.ms_acc = 0.0;
        s.ms_max = 0.0;
        s.ms_n = 0;
    }
}
/// The mod's onShutdown: what it holds let go, the carrying state taken off, the plugin's wrist stops as it ships.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        carry::suspend(s);
        carry::carry_state(s, false);
    }
    let zero = [0f32; 4];
    for side in 0..2 {
        nat::<()>("VRHandStopModel", &[Value::Int32(side), Value::Bool(false), vp(&zero)]);
        // the wrist rotation lock is plugin-side state; left on through a reload it would freeze the hand's aim
        nat::<bool>("VRHandStopRot", &[Value::Int32(side), Value::Int32(0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(1.0)]);
    }
}

/// graspState(side).held (main's HandCollision API, asked by the car's panels): that hand carries a body.
pub(in crate::mechanics) fn holding(side: usize) -> bool {
    let g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    side < 2 && g.as_ref().is_some_and(|s| s.carry.hold[side].is_some())
}
