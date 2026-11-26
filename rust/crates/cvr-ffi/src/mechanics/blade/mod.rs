//! main's handheld blade contact, in Rust: mods/redscript/CyberpunkVRPort_Melee's vrport_blade_contact.reds,
//! vrport_blade_rpg.reds, vrport_blade_visible_cut.reds, vrport_blade_marker.reds, vrport_contact_stroke.reds,
//! vrport_npc_melee_reaction.reds and vrport_blade_surface_mark.reds, with mods/cet/CyberpunkVRPort_Weapon's
//! blade_contact_pose.lua (pose.rs).
//!
//! A knife or a long blade (VRBladeKind 1 / 2) strikes only where the visible weapon's swept segment, grip to tip,
//! meets a person's native hit shape: a "Melee" physics sweep finds the puppet, the blade natives measure the crossing
//! against its hit representation (VRBladeEntrySweep), and one native melee effect is started along that very
//! segment (an impact, `Impact`). Its damage pipeline accepts only the traced person and the one shape the blade met
//! (ProcessPipeline); a thrust from behind into the neck of an unaware enemy takes the rest of its health, a thrust
//! deals 1.5x and bleeds; the health it took publishes a receipt (`Receipt`): the blade lodged in the shape's slot,
//! a D6 joint between the hand and the body (the blade natives' joint), the person held in place, a cut along the
//! edge continuing the wound (the visible cut, cut.rs), the struck mesh marked (mark.rs). Nothing here aims at a
//! target, names a body region or simulates a wound the game would not deal itself.
//!
//! The redscript fields of WeaponObject, Attack_GameEffect, NPCPuppet and gameHitEvent are side tables here, keyed by
//! the weapon's and the person's entity, the attack's and the hit event's object; the redscript classes are
//! Rc<RefCell> records (their identity is the scripts' `==`).
mod cut;
mod hold;
mod hooks;
mod mark;
mod pose;
mod rpg;

pub(super) use hooks::HOOKS;
pub(super) use pose::{reset as pose_reset, update as pose_update};
pub(super) use cut::box_half_extent;
pub(super) use rpg::publish_hit;

use super::lobby_rtti::{self as rt, Val, A};
use super::melee::{self, Swing};
use super::menu_rt as r;
use crate::blade_natives::{self as bn, WorldTransform, V3, V4};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(super) type Sh<T> = Rc<RefCell<T>>;
pub(super) fn sh<T>(v: T) -> Sh<T> {
    Rc::new(RefCell::new(v))
}

// ---------------------------------------------------------------- Vector4 arithmetic, as redscript's
pub(super) const ZERO: V4 = [0.0; 4];
pub(super) fn sub(a: &V4, b: &V4) -> V4 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}
pub(super) fn add(a: &V4, b: &V4) -> V4 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}
pub(super) fn mul(a: &V4, k: f32) -> V4 {
    [a[0] * k, a[1] * k, a[2] * k, a[3] * k]
}
/// a + (b - a) * t, all four components (a point stays a point).
pub(super) fn lerp(a: &V4, b: &V4, t: f32) -> V4 {
    add(a, &mul(&sub(b, a), t))
}
pub(super) fn dot(a: &V4, b: &V4) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn len(a: &V4) -> f32 {
    dot(a, a).sqrt()
}
pub(super) fn dist(a: &V4, b: &V4) -> f32 {
    len(&sub(a, b))
}
pub(super) fn norm(a: &V4) -> V4 {
    let l = len(a);
    if l > 1e-12 {
        [a[0] / l, a[1] / l, a[2] / l, 0.0]
    } else {
        ZERO
    }
}
pub(super) fn with_w(a: V4, w: f32) -> V4 {
    [a[0], a[1], a[2], w]
}
pub(super) fn p3(a: &V4) -> V3 {
    [a[0], a[1], a[2]]
}
pub(super) fn p4(a: &V3, w: f32) -> V4 {
    [a[0], a[1], a[2], w]
}
/// VrCyberwareContactMath.Closest: the closest point of the segment a..b to p.
pub(super) fn closest(p: &V4, a: &V4, b: &V4) -> V4 {
    let ab = sub(b, a);
    let d = dot(&ab, &ab);
    let t = if d > 0.000_000_1 { (dot(&sub(p, a), &ab) / d).clamp(0.0, 1.0) } else { 0.0 };
    add(a, &mul(&ab, t))
}
/// The Hamilton product (RED4ext's Quaternion operator*).
pub(super) fn qmul(a: &V4, b: &V4) -> V4 {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] + a[1] * b[3] + a[2] * b[0] - a[0] * b[2],
        a[3] * b[2] + a[2] * b[3] + a[0] * b[1] - a[1] * b[0],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
/// Quaternion.Transform(q, v), w 0.
pub(super) fn rotate(q: &V4, v: &V4) -> V4 {
    p4(&bn::qrot(q, &p3(v)), 0.0)
}
/// WorldTransform.TransformPoint (a point of the frame in the world), w 1.
pub(super) fn to_world(frame: &WorldTransform, p: &V4) -> V4 {
    let r = bn::qrot(&frame.orientation(), &p3(p));
    let o = frame.position();
    [r[0] + o[0], r[1] + o[1], r[2] + o[2], 1.0]
}
/// WorldTransform.TransformInvPoint, w 1.
pub(super) fn to_local(frame: &WorldTransform, p: &V4) -> V4 {
    p4(&bn::to_local(frame, &p3(p)), 1.0)
}

// ---------------------------------------------------------------- the engine
pub(super) fn sim_time() -> f64 {
    melee::sim_time()
}
pub(super) fn real_time() -> f64 {
    melee::real_time()
}
pub(super) fn eid(o: &Handle) -> u64 {
    if o.is_null() {
        0
    } else {
        melee::entity_id(o)
    }
}
pub(super) fn b(o: &Handle, name: &str) -> bool {
    !o.is_null() && r::b(o, name, &[])
}
pub(super) fn v4_of(o: &Handle, name: &str) -> V4 {
    super::belt::m::<V4>(o, name, &[]).unwrap_or(ZERO)
}
pub(super) fn sb(class: &str, name: &str, args: &[A]) -> bool {
    rt::static_call(class, name, args).is_some_and(|o| Val::of(&o).bool())
}
pub(super) fn ev(en: &str, member: &str) -> f64 {
    rt::enum_value(en, member)
}
/// The entity's GetWorldTransform.
pub(super) fn world_transform(o: &Handle) -> Option<WorldTransform> {
    let r = rt::call(o, "GetWorldTransform", &[])?;
    // SAFETY: a WorldTransform result (32 bytes).
    Some(WorldTransform(unsafe { *(r.as_ptr() as *const [u32; 8]) }))
}
pub(super) fn hit_representation(e: &Handle) -> Handle {
    if e.is_null() {
        return Handle::null();
    }
    rt::obj(e, "FindComponentByName", &[A::Name(CName::hashed("HitRepresentation").0)])
}
/// The TraceResult of a query (0x60 bytes: the position, the normal, the hit's ids).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub(super) struct Trace(pub [u8; 0x60]);
impl Trace {
    pub(super) fn zeroed() -> Trace {
        Trace([0; 0x60])
    }
    fn f(&self, i: usize) -> f32 {
        f32::from_le_bytes([self.0[i * 4], self.0[i * 4 + 1], self.0[i * 4 + 2], self.0[i * 4 + 3]])
    }
    pub(super) fn position(&self) -> V4 {
        [self.f(0), self.f(1), self.f(2), 1.0]
    }
    pub(super) fn normal(&self) -> V4 {
        [self.f(3), self.f(4), self.f(5), 0.0]
    }
    /// TraceResult.GetHitEntity.
    pub(super) fn entity(&self) -> Handle {
        bn::trace_entity(&self.0)
    }
}
pub(super) fn queries() -> Handle {
    super::game_system("GetSpatialQueriesSystem")
}
/// SpatialQueriesSystem.SyncRaycastByQueryPreset(start, end, preset, out trace, staticOnly false, dynamicOnly).
pub(super) fn raycast(sq: &Handle, start: &V4, end: &V4, preset: &str, dynamic_only: bool, trace: &mut Trace) -> bool {
    if sq.is_null() {
        return false;
    }
    let (s, e) = (with_w(*start, 1.0), with_w(*end, 1.0));
    let values = [
        crate::script_call::Value::Ptr(s.as_ptr().cast()),
        crate::script_call::Value::Ptr(e.as_ptr().cast()),
        crate::script_call::Value::U64(CName::hashed(preset).0),
        crate::script_call::Value::Out((trace as *mut Trace).cast()),
        crate::script_call::Value::Bool(false),
        crate::script_call::Value::Bool(dynamic_only),
    ];
    // SAFETY: the spatial queries system, held; the trace's storage outlives the call.
    (unsafe { crate::script_call::method::<bool>(sq.instance, "SyncRaycastByQueryPreset", &values) }) == Some(true)
}
/// VrCyberwareContactMath.SweptTrace: rays along material points of the moving segment (one each 4 cm of its length,
/// at most 33), the earliest contact's time along the motion (t); without one, the segment's end pose traced.
pub(super) fn swept_trace(sq: &Handle, old_a: &V4, old_b: &V4, a: &V4, b: &V4, passive: bool, trace: &mut Trace) -> Option<f32> {
    let count = ((dist(old_a, old_b).max(dist(a, b)) / 0.04).ceil() as i32).clamp(1, 32);
    let mut t = 2.0f32;
    if !passive {
        let mut hit = Trace::zeroed();
        for i in 0..=count {
            let f = i as f32 / count as f32;
            let start = lerp(old_a, old_b, f);
            let end = lerp(a, b, f);
            let delta = sub(&end, &start);
            let squared = dot(&delta, &delta);
            if squared > 0.000_001 && raycast(sq, &start, &end, "Melee", false, &mut hit) {
                let when = (dot(&sub(&hit.position(), &start), &delta) / squared).clamp(0.0, 1.0);
                if when < t {
                    t = when;
                    *trace = hit;
                }
            }
        }
    }
    if t <= 1.0 {
        return Some(t);
    }
    raycast(sq, a, b, "Melee", false, trace).then_some(1.0)
}

// ---------------------------------------------------------------- the scripts' classes
/// VrBladeEntry: one crossing, shared by the probes it sends.
#[derive(Default)]
pub(super) struct Entry {
    pub claimed: bool,
    pub expired: bool,
    pub ended_clear: bool,
    pub prior_serial: u32,
}
/// VrBladeImpact (with vrport_blade_rpg's fields): one native effect's contact.
#[derive(Default)]
pub(super) struct Impact {
    pub entry: Option<Sh<Entry>>,
    pub thrust: bool,
    pub attack_edge: V4,
    pub bleed_on_hit: bool,
    pub physical_before: f32,
    pub physical_after: f32,
    pub previous_a: V4,
    pub previous_b: V4,
    pub sweep_from: V4,
    pub sweep_to: V4,
    pub end_a: V4,
    pub end_b: V4,
    pub target: u64,
    pub a: V4,
    pub b: V4,
    pub claimed: bool,
    pub cancelled: bool,
    pub created: f64,
    pub events: i32,
    pub last_shapes: i32,
    pub last_distance: f32,
    pub cut_proof: V4,
    pub no_dismember: bool,
    /// The target's hit shapes are boxes (a mech): the attack's sweep was widened by melee::WIDE_HALF.
    pub wide: bool,
    pub has_neck_slot: bool,
    pub contact_metrics: V4,
    /// The swing that sent it (its side and the stroke as begun).
    pub swing: Option<(i32, Swing)>,
    pub stealth_intent: bool,
    pub stealth_applied: bool,
    pub port_at_start: V4,
    pub rear_at_start: bool,
    pub port_matched: bool,
    pub stealth_stage: i32,
    pub parent_serial: u32,
    pub overlap_point: V4,
    pub published: bool,
    pub available_health: f32,
    pub strength_reaction: bool,
    pub accepted_shape: u64,
    pub part_notified: bool,
    pub edge_stroke: u32,
    pub cut_slot: u64,
    pub cut_point: V4,
    pub cut_shape: Option<Rc<cut::CutShape>>,
    pub cut_candidates: Vec<Rc<cut::CutShape>>,
}
/// VrBladeSlotAttachment: the material point of the blade held in a slot's frame while it is lodged.
#[derive(Default)]
pub(super) struct SlotAttachment {
    pub slots: WeakHandle,
    pub slots_ptr: usize,
    pub shape: u64,
    pub slot: u64,
    pub capsule: (V4, V4),
    pub local_point: V4,
    pub fraction: f32,
    pub depth: f32,
    pub edge_frames: Vec<Rc<cut::VisiblePose>>,
    pub slide_tracking: bool,
    slide_station: V4,
    slide_grip: V4,
    slide_last_grip: V4,
    slide_moving_grip: V4,
    slide_moving_station: V4,
    slide_start: f64,
    slide_sample: f64,
    slide_last_motion: f64,
    slide_last_slip: f64,
}
impl SlotAttachment {
    /// SlideFeedback: a pulse when a material point of the blade slid 2.4 cm in the slot while the grip moved 1.8 cm.
    pub fn slide_feedback(&mut self, station: &V4, grip: &V4, now: f64) -> bool {
        let gap = now - self.slide_sample;
        let elapsed = now - self.slide_start;
        let reset = !self.slide_tracking || !(0.0..=0.10).contains(&gap) || grip[3] != self.slide_last_grip[3] || dist(grip, &self.slide_last_grip) > 0.20;
        if reset || dist(grip, &self.slide_moving_grip) > 0.002 {
            self.slide_moving_grip = *grip;
            self.slide_last_motion = now;
        }
        if reset || dist(station, &self.slide_moving_station) > 0.002 {
            self.slide_moving_station = *station;
            self.slide_last_slip = now;
        }
        let moving = now - self.slide_last_motion <= 0.08 && now - self.slide_last_slip <= 0.08;
        let pulse = !reset && moving && elapsed >= 0.075 && dist(station, &self.slide_station) >= 0.024 && dist(grip, &self.slide_grip) >= 0.018;
        if reset || pulse || !moving {
            self.slide_station = *station;
            self.slide_grip = *grip;
            self.slide_start = now;
        }
        self.slide_tracking = true;
        self.slide_last_grip = *grip;
        self.slide_sample = now;
        pulse
    }
}
/// VrBladeReceipt: a hit the health ledger confirmed, and the lodged blade's state.
#[derive(Default)]
pub(super) struct Receipt {
    pub entry: Option<Sh<Entry>>,
    pub released: bool,
    pub lodged: bool,
    pub awaiting_seat: bool,
    pub lodged_at: f64,
    pub serial: u32,
    pub joint_serial: u32,
    pub last_edge_stroke: u32,
    pub seat_slot: u64,
    pub seat_part_point: V4,
    pub seat_point: V4,
    pub seat_axis: V4,
    pub seat_edge: V4,
    pub attachment: Option<Sh<SlotAttachment>>,
    pub owner: WeakHandle,
    pub target: WeakHandle,
    pub target_id: u64,
    pub point: V4,
    /// The person's HitRepresentation (its SlotComponent).
    pub slots: WeakHandle,
    pub slots_ptr: usize,
    pub slot: u64,
    pub shape_name: u64,
    pub shape_data: Handle,
    pub part_point: V4,
    pub mesh_start: V4,
    pub mesh_end: V4,
    pub zone: f64,
    pub time: f64,
    pub follow: bool,
    pub puncture: bool,
    pub stealth: bool,
    pub spent: bool,
    pub capsule: Option<(V4, V4)>,
    pub material: i32,
    pub requested_bodies: bool,
    pub joint_trace_kind: i32,
    pub joint_probe_index: i32,
    pub joint_probe_until: f64,
    pub pending: Option<Sh<Impact>>,
    pub health_loss: f32,
    pub load_time: f64,
    pub mesh_requests: u32,
    pub trail_point: V4,
    pub trail_normal: V4,
    pub trail_deadline: f64,
    pub trail_segments: i32,
    pub trail_ready: bool,
    pub trail_on_mesh: bool,
}
impl Receipt {
    pub(super) fn target(&self) -> Handle {
        self.target.lock()
    }
    pub(super) fn owner(&self) -> Handle {
        self.owner.lock()
    }
    pub(super) fn slots(&self) -> Handle {
        self.slots.lock()
    }
}

/// The WeaponObject's fields.
#[derive(Default)]
pub(super) struct Weapon {
    end: V4,
    measured: bool,
    probe_attempts: i32,
    retry_at: f64,
    family: i32,
    touch: WeakHandle,
    last_probe: Option<Sh<Impact>>,
    attempt: Option<Sh<Impact>>,
    sweep_sample: V4,
    pub receipt: Option<Sh<Receipt>>,
    pub follow_gate: &'static str,
    pub follow_geometry: V4,
    pub follow_window: V4,
    pub follow_counts: V4,
    pub follow_serial: u32,
    pub follow_attempt: u32,
    pub slide_ticks: u32,
    pub resume_hold_joint: u32,
}
/// An Attack_GameEffect's fields.
struct AttackInfo {
    _attack: Handle,
    /// its record's id: the hit's attack is the effect's copy of it, found by its record
    record: u64,
    contact_target: u64,
    contact_claimed: bool,
    impact: Option<Sh<Impact>>,
    light_chain: bool,
    at: f64,
}
#[derive(Default)]
struct Npc {
    neck_configured: bool,
    hold: Option<hold::Hold>,
}
#[derive(Default)]
struct Store {
    weapons: HashMap<u64, Sh<Weapon>>,
    attacks: HashMap<usize, AttackInfo>,
    npcs: HashMap<u64, Npc>,
    /// gameHitEvent.vrSeatedBladeReaction, by the event (and when it was set).
    seated: HashMap<usize, f64>,
    /// The effect's copies of the attacks (the copy's object -> the attack made).
    copies: HashMap<usize, usize>,
    next_joint_serial: u32,
}
/// THE BLADE'S LOCK. The damage pipeline runs on the engine's job threads, the mechanics on the game's: everything of
/// this module (the store, and the Rc/RefCell records it hands out) is touched only while this re-entrant lock is
/// held -- every entry point takes it for its whole call, so no record's count or borrow is ever shared by two threads.
pub(in crate::mechanics) struct Guard;
static LOCK_HELD: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static LOCK_OWNER: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static LOCK_DEPTH: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
fn thread_key() -> usize {
    thread_local! {
        static KEY: u8 = const { 0 };
    }
    KEY.with(|k| k as *const u8 as usize)
}
/// The lock, taken (again, on the thread that holds it).
pub(in crate::mechanics) fn lock() -> Guard {
    use core::sync::atomic::Ordering::*;
    let me = thread_key();
    if LOCK_OWNER.load(Acquire) == me {
        LOCK_DEPTH.fetch_add(1, Relaxed);
        return Guard;
    }
    let mut spins = 0u32;
    while LOCK_HELD.compare_exchange_weak(false, true, Acquire, Relaxed).is_err() {
        spins = spins.wrapping_add(1);
        if spins % 64 == 0 {
            std::thread::yield_now();
        } else {
            core::hint::spin_loop();
        }
    }
    LOCK_OWNER.store(me, Release);
    LOCK_DEPTH.store(1, Relaxed);
    Guard
}
impl Drop for Guard {
    fn drop(&mut self) {
        use core::sync::atomic::Ordering::*;
        if LOCK_DEPTH.fetch_sub(1, Relaxed) == 1 {
            LOCK_OWNER.store(0, Release);
            LOCK_HELD.store(false, Release);
        }
    }
}
struct StoreCell(core::cell::UnsafeCell<Option<Store>>);
// SAFETY: only touched under the blade's lock.
unsafe impl Sync for StoreCell {}
static STORE: StoreCell = StoreCell(core::cell::UnsafeCell::new(None));
fn store<T>(f: impl FnOnce(&mut Store) -> T) -> T {
    let _g = lock();
    // SAFETY: the lock is held; no other reference to the store lives across this call (the closures do not
    // re-enter `store`).
    let s = unsafe { &mut *STORE.0.get() };
    f(s.get_or_insert_with(Store::default))
}
/// The weapon's fields (made on first use).
pub(super) fn weapon(w: &Handle) -> Sh<Weapon> {
    let _g = lock();
    let id = eid(w);
    store(|s| s.weapons.entry(id).or_insert_with(|| sh(Weapon { follow_gate: "", ..Weapon::default() })).clone())
}
pub(super) fn receipt(w: &Handle) -> Option<Sh<Receipt>> {
    let _g = lock();
    if w.is_null() {
        return None;
    }
    weapon(w).borrow().receipt.clone()
}
fn next_joint_serial() -> u32 {
    store(|s| {
        s.next_joint_serial = s.next_joint_serial.wrapping_add(1);
        if s.next_joint_serial == 0 {
            s.next_joint_serial = 1;
        }
        s.next_joint_serial
    })
}
/// The attack's fields set when it is made (VRCyberwareAttack): the person it is for, its blade impact, whether it is
/// a light stroke of the combo chain. Kept while its ticket can be matched (and a while after).
pub(super) fn note_attack(attack: &Handle, target: u64, impact: Option<Sh<Impact>>, light_chain: bool) {
    let _g = lock();
    let now = sim_time();
    let record = record_of(attack);
    store(|s| {
        s.attacks.retain(|_, a| (now - a.at).abs() <= 20.0);
        let live: std::collections::HashSet<usize> = s.attacks.keys().copied().collect();
        s.copies.retain(|_, k| live.contains(k));
        s.seated.retain(|_, t| (now - *t).abs() <= 5.0);
        s.attacks.insert(attack.instance as usize, AttackInfo { _attack: attack.clone(), record, contact_target: target, contact_claimed: false, impact, light_chain, at: now });
    });
}
fn record_of(attack: &Handle) -> u64 {
    let record = r::o(attack, "GetRecord", &[]);
    // SAFETY: a TweakDBID result.
    rt::call(&record, "GetID", &[]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
/// The attack made that this one is (itself, or the effect's copy of it: the newest attack noted within 2 s with its
/// record, the copy remembered).
fn key_of(attack: &Handle) -> Option<usize> {
    if attack.is_null() {
        return None;
    }
    let ptr = attack.instance as usize;
    if let Some(k) = store(|s| if s.attacks.contains_key(&ptr) { Some(ptr) } else { s.copies.get(&ptr).copied() }) {
        return Some(k);
    }
    let record = record_of(attack);
    if record == 0 {
        return None;
    }
    let now = sim_time();
    store(|s| {
        let k = s.attacks.iter().filter(|(_, a)| a.record == record && (0.0..=2.0).contains(&(now - a.at))).max_by(|x, y| x.1.at.total_cmp(&y.1.at)).map(|(k, _)| *k)?;
        s.copies.insert(ptr, k);
        Some(k)
    })
}
/// The two attacks are the same one made (main's `definition == ticket.attack`, across the effect's copy).
pub(super) fn same_attack(a: &Handle, b: &Handle) -> bool {
    let _g = lock();
    if a.is_null() || b.is_null() {
        return false;
    }
    if a.instance == b.instance {
        return true;
    }
    match (key_of(a), key_of(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}
/// The attack carries a blade impact (its record not handed out).
pub(super) fn attack_has_impact(attack: &Handle) -> bool {
    let _g = lock();
    let Some(k) = key_of(attack) else { return false };
    store(|s| s.attacks.get(&k).is_some_and(|a| a.impact.is_some()))
}
/// Attack_GameEffect.vrBladeImpact.
pub(super) fn attack_impact(attack: &Handle) -> Option<Sh<Impact>> {
    let _g = lock();
    let k = key_of(attack)?;
    store(|s| s.attacks.get(&k).and_then(|a| a.impact.clone()))
}
fn attack_target(attack: &Handle) -> Option<u64> {
    let k = key_of(attack)?;
    store(|s| s.attacks.get(&k).map(|a| a.contact_target))
}
fn attack_light_chain(attack: &Handle) -> bool {
    let Some(k) = key_of(attack) else { return false };
    store(|s| s.attacks.get(&k).is_some_and(|a| a.light_chain))
}
/// The attack's contact claim (Attack_GameEffect.vrMeleeContactClaimed): taken now (true) or already (false).
fn claim_contact(attack: &Handle) -> bool {
    let Some(k) = key_of(attack) else { return false };
    store(|s| match s.attacks.get_mut(&k) {
        Some(a) if !a.contact_claimed => {
            a.contact_claimed = true;
            true
        }
        _ => false,
    })
}
/// The impact's blade impact is a follow-through (its parent's serial set).
pub(super) fn attack_is_follow(attack: &Handle) -> bool {
    let _g = lock();
    attack_impact(attack).is_some_and(|i| i.borrow().parent_serial != 0)
}
fn set_seated(hit: &Handle, on: bool) {
    let now = sim_time();
    store(|s| {
        if on {
            s.seated.insert(hit.instance as usize, now);
        } else {
            s.seated.remove(&(hit.instance as usize));
        }
    });
}
fn seated(hit: &Handle) -> bool {
    store(|s| s.seated.contains_key(&(hit.instance as usize)))
}
fn neck_configured(npc: &Handle) -> bool {
    let id = eid(npc);
    store(|s| s.npcs.get(&id).is_some_and(|n| n.neck_configured))
}

// ---------------------------------------------------------------- the weapon
/// WeaponObject.VRBladeKind: 1 a knife or short blade, 2 a long blade (katana, sword, machete, chainsword, axe),
/// -1 anything else (cached once the record answers).
pub(super) fn kind(w: &Handle) -> i32 {
    let _g = lock();
    if w.is_null() {
        return -1;
    }
    let state = weapon(w);
    if state.borrow().family != 0 {
        return state.borrow().family;
    }
    let record = r::o(w, "GetWeaponRecord", &[]);
    let item_type = r::o(&record, "ItemType", &[]);
    if record.is_null() || item_type.is_null() {
        return -1;
    }
    let t = rt::num(&item_type, "Type", &[]).unwrap_or(-1.0);
    let is = |name: &str| t == ev("gamedataItemType", name);
    let family = if is("Wea_Knife") || is("Wea_ShortBlade") {
        1
    } else if ["Wea_Katana", "Wea_Sword", "Wea_LongBlade", "Wea_Machete", "Wea_Chainsword", "Wea_Axe"].iter().any(|n| is(n)) {
        2
    } else {
        -1
    };
    state.borrow_mut().family = family;
    family
}
/// WeaponObject.VRBladeLocalEnd: the authored Impact marker in the item's frame (retried for the first seconds of a
/// fresh draw), else a short local segment (16 cm for a knife, 35 cm otherwise) -- never the machine's lunge range.
pub(super) fn local_end(w: &Handle) -> V4 {
    let _g = lock();
    let state = weapon(w);
    let (measured, attempts, retry_at, end) = {
        let s = state.borrow();
        (s.measured, s.probe_attempts, s.retry_at, s.end)
    };
    if measured || attempts >= 16 {
        return end;
    }
    let now = sim_time();
    if attempts > 0 && now < retry_at {
        return end;
    }
    {
        let mut s = state.borrow_mut();
        s.probe_attempts += 1;
        s.retry_at = now + 0.20;
    }
    let mut slots = rt::obj(w, "FindComponentByName", &[A::Name(CName::hashed("item_slot_attachments").0)]);
    if slots.is_null() {
        slots = rt::obj(w, "FindComponentByName", &[A::Name(CName::hashed("item_slots_attachments").0)]);
    }
    let mut found = None;
    if let Some(marker) = bn::slot_frame(slots.instance.cast(), CName::hashed("Impact").0) {
        if let Some(xf) = world_transform(w) {
            let p = with_w(to_local(&xf, &p4(&marker.position(), 1.0)), 0.0);
            if p[2] > 0.04 && len(&p) <= 1.25 {
                found = Some(p);
            }
        }
    }
    let family = kind(w);
    let mut s = state.borrow_mut();
    match found {
        Some(p) => {
            s.end = p;
            s.measured = true;
        }
        None => s.end = [0.0, 0.0, if family == 1 { 0.16 } else { 0.35 }, 0.0],
    }
    s.end
}
/// WeaponObject.VRBladeNoDismember: a knife, an unmarked asset or a blade under 45 cm never takes a part off.
pub(super) fn no_dismember(w: &Handle) -> bool {
    let _g = lock();
    let span = len(&local_end(w));
    kind(w) == 1 || !weapon(w).borrow().measured || span < 0.45
}
fn psm() -> Option<(Handle, Handle)> {
    let defs = super::boards::defs().ok()?;
    let def = super::boards::definition(&defs, "PlayerStateMachine").ok()?.clone();
    Some((defs, def))
}
fn psm_board(pl: &Handle) -> Handle {
    rt::obj(pl, "GetPlayerStateMachineBlackboard", &[])
}
pub(in crate::mechanics) fn psm_int(pl: &Handle, field: &str) -> Option<f64> {
    let (_, def) = psm()?;
    let board = psm_board(pl);
    let id = super::boards::field(&def, field).ok()?;
    if board.is_null() {
        return None;
    }
    rt::num(&board, "GetInt", &[A::P(id)])
}
fn psm_bool(pl: &Handle, field: &str) -> bool {
    let Some((_, def)) = psm() else { return false };
    let board = psm_board(pl);
    let Ok(id) = super::boards::field(&def, field) else { return false };
    !board.is_null() && rt::call(&board, "GetBool", &[A::P(id)]).is_some_and(|o| Val::of(&o).bool())
}
fn psm_entity(pl: &Handle, field: &str) -> u64 {
    let Some((_, def)) = psm() else { return 0 };
    let board = psm_board(pl);
    let Ok(id) = super::boards::field(&def, field) else { return 0 };
    // SAFETY: an EntityID result.
    rt::call(&board, "GetEntityID", &[A::P(id)]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
pub(in crate::mechanics) fn has_effect_tag(o: &Handle, tag: &str) -> bool {
    sb("StatusEffectSystem", "ObjectHasStatusEffectWithTag", &[A::H(o), A::Name(CName::hashed(tag).0)])
}
fn held(pl: &Handle) -> Handle {
    let ts = super::belt::transactions();
    rt::obj(&ts, "GetItemInSlot", &[A::H(pl), A::Name(super::left_handed::held_slot(pl))])
}
/// The weapon in the hand that holds (VRLeftHandedWeapon.HeldSlot) is this one.
pub(super) fn in_hand(pl: &Handle, w: &Handle) -> bool {
    let h = held(pl);
    !h.is_null() && !w.is_null() && h.instance == w.instance
}
/// PlayerPuppet.VRBladeAllowed: the blade in the holding hand may strike -- a melee state that is not an attack the
/// game runs itself, no scene, finisher, stun or no-combat zone, the weapon not thrown away.
pub(super) fn allowed(pl: &Handle, w: &Handle) -> bool {
    if pl.is_null() || w.is_null() || psm_board(pl).is_null() || b(pl, "IsDead") || super::melee_throw::recovering(w) || !in_hand(pl, w) {
        return false;
    }
    let Some(state) = psm_int(pl, "MeleeWeapon") else { return false };
    let ok_state = ["Idle", "PublicSafe", "Hold", "ChargedHold", "Block", "Deflect", "Targeting"].iter().any(|m| state == ev("gamePSMMeleeWeapon", m));
    ok_state
        && !psm_bool(pl, "SceneSafeForced")
        && psm_entity(pl, "FinisherTarget") == 0
        && !has_effect_tag(pl, "NoCombat")
        && !has_effect_tag(pl, "NoWeapons")
        && !has_effect_tag(pl, "VehicleScene")
        && !sb("StatusEffectSystem", "ObjectHasStatusEffectOfType", &[A::H(pl), A::N(ev("gamedataStatusEffectType", "Stunned"))])
}
/// PlayerPuppet.VRPhysicalMeleeWeapon: the weapon in the holding hand's slot.
pub(super) fn physical_weapon(pl: &Handle) -> Handle {
    let h = held(pl);
    if r::is_a(&h, "WeaponObject") {
        h
    } else {
        Handle::null()
    }
}
/// WeaponObject.VRPhysicalWeaponRecordReady.
pub(super) fn record_ready(w: &Handle) -> bool {
    let record = r::o(w, "GetWeaponRecord", &[]);
    !record.is_null() && !r::o(&record, "ItemType", &[]).is_null()
}
pub(super) fn confirmed_serial(w: &Handle) -> u32 {
    let _g = lock();
    receipt(w).map_or(0, |r| r.borrow().serial)
}
/// WeaponObject.VRBladeConfirmedAge: the receipt's age (-1 without one).
pub(super) fn confirmed_age(w: &Handle) -> f64 {
    let _g = lock();
    let Some(r) = receipt(w) else { return -1.0 };
    let (owner, time) = {
        let r = r.borrow();
        (r.owner(), r.time)
    };
    if owner.is_null() {
        return -1.0;
    }
    let age = sim_time() - time;
    if age >= 0.0 {
        age
    } else {
        -1.0
    }
}
/// VRLeftHandedWeapon.InHand: the weapon is the left hand's (0), else the right's (1).
pub(super) fn hand_side(_pl: &Handle) -> i32 {
    if super::belt::native::<bool>("VRWeaponInLeftHand", &[]) == Some(true) {
        0
    } else {
        1
    }
}

// ---------------------------------------------------------------- the entry
/// WeaponObject.VRBladeEntryBlocked: no new entry while the blade is just lodged, held by its joint, or a
/// follow-through is in flight.
pub(super) fn entry_blocked(w: &Handle) -> bool {
    let _g = lock();
    let Some(rc) = receipt(w) else { return false };
    let r = rc.borrow();
    let (target, owner) = (r.target(), r.owner());
    if r.released || target.is_null() || !b(&target, "IsAttached") || owner.is_null() {
        return false;
    }
    let elapsed = real_time() - r.lodged_at;
    if (r.awaiting_seat || r.lodged) && (0.0..0.25).contains(&elapsed) {
        return true;
    }
    if r.lodged && bn::joint_point(hand_side(&owner), r.joint_serial)[3] > 0.5 {
        return true;
    }
    r.pending.as_ref().is_some_and(|p| {
        let p = p.borrow();
        !p.cancelled && sim_time() - p.created < 0.20
    })
}
/// WeaponObject.VRBladeCanClaimEntry: the impact's crossing may enter the damage pipeline -- the one in flight, or a
/// completed pass-through queued behind its predecessor within the probe window.
pub(super) fn can_claim_entry(w: &Handle, impact: &Impact, pl: &Handle) -> bool {
    let current = weapon(w).borrow().attempt.clone();
    let (Some(current), Some(entry)) = (current, impact.entry.as_ref()) else { return false };
    let Some(current_entry) = current.borrow().entry.clone() else { return false };
    if entry_blocked(w) {
        return false;
    }
    if Rc::ptr_eq(&current_entry, entry) {
        return true;
    }
    let e = entry.borrow();
    if !e.ended_clear || current_entry.borrow().claimed || e.prior_serial != confirmed_serial(w) || pl.is_null() || !allowed(pl, w) {
        return false;
    }
    let age = sim_time() - impact.created;
    (0.0..=0.12).contains(&age)
}
/// PlayerPuppet.VRBladeSweep: the blade swept from (oldA, oldB) to (a, b): a new contact with an eligible person's
/// hit shape sends one native effect along the crossing (1); a held contact, a pending probe or no crossing, 0.
#[allow(clippy::too_many_arguments)]
pub(super) fn sweep(pl: &Handle, w: &Handle, old_a: &V4, old_b: &V4, a: &V4, b_: &V4, passive: bool, mut begin_on_contact: bool, tracked: V4, strong: bool, speed: f64) -> i32 {
    let _g = lock();
    // CVR_MELEE_TRACE=1: why an active sweep made no contact
    let why = |what: &str| {
        if !passive && std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1") {
            cvr_host::log!("[melee-trace] blade sweep: {what}\n");
        }
    };
    if entry_blocked(w) {
        why("the entry is blocked (lodged, held or a follow-through in flight)");
        return 0;
    }
    let state = weapon(w);
    let mut last = state.borrow().touch.lock();
    let mut previous = state.borrow().attempt.clone();
    let now = sim_time();
    let cur_start = lerp(a, b_, 0.10);
    if !passive {
        state.borrow_mut().sweep_sample = ZERO;
    }
    // a retained crossing has a weak touch: a despawn can clear it between samples
    let prev_entry = previous.as_ref().and_then(|p| p.borrow().entry.clone());
    if let Some(e) = &prev_entry {
        if !e.borrow().ended_clear && (last.is_null() || !self::b(&last, "IsAttached")) {
            e.borrow_mut().expired = true;
            let mut s = state.borrow_mut();
            s.attempt = None;
            s.touch = WeakHandle::null();
            previous = None;
            last = Handle::null();
        }
    }
    let prev_entry = previous.as_ref().and_then(|p| p.borrow().entry.clone());
    if let (Some(p), Some(e)) = (&previous, &prev_entry) {
        if !e.borrow().claimed {
            // a delayed effect must not eat the frames between; the probes of one crossing share one claim
            last = Handle::null();
            let created = p.borrow().created;
            if now < created || now - created > 0.12 {
                e.borrow_mut().expired = true;
                let mut s = state.borrow_mut();
                s.attempt = None;
                s.touch = WeakHandle::null();
                previous = None;
            }
        }
    }
    if !last.is_null() {
        // a reversal is not a new impact while the blade stays at the same moving contact
        let comp = hit_representation(&last);
        let present = bn::swept_contact(comp.instance.cast(), &[p3(a), p3(b_), p3(a), p3(b_)], None);
        if present[3] < 0.5 {
            let mut s = state.borrow_mut();
            s.touch = WeakHandle::null();
            s.attempt = None;
        }
        why("a held contact");
        return 0;
    }
    if passive {
        return 0;
    }
    if !allowed(pl, w) {
        state.borrow_mut().touch = WeakHandle::null();
        why("not allowed");
        return 0;
    }
    if dist(a, old_a) < 0.001 && dist(b_, old_b) < 0.001 {
        why("no motion");
        return 0;
    }
    let prev_start = lerp(old_a, old_b, 0.10);
    let sq = queries();
    let mut trace = Trace::zeroed();
    if let Some(t) = swept_trace(&sq, &prev_start, old_b, &cur_start, b_, passive, &mut trace) {
        let npc = trace.entity();
        if r::is_a(&npc, "NPCPuppet")
            && self::b(&npc, "IsActive")
            && !self::b(&npc, "IsDead")
            && !sb("ScriptedPuppet", "IsDefeated", &[A::H(&npc)])
            && !sb("GameObject", "IsFriendlyTowardsPlayer", &[A::H(&npc)])
        {
            why(&format!("the trace hit a person at {t:.2}"));
            state.borrow_mut().sweep_sample = [t, 0.0, 0.0, 1.0];
            let comp = hit_representation(&npc);
            let overlap = bn::swept_contact(comp.instance.cast(), &[p3(&prev_start), p3(old_b), p3(&cur_start), p3(b_)], Some(&p3(a)));
            if overlap[3] < 0.5 {
                why(&format!("the trace hit the person but no hit shape crossed ({overlap:?})"));
                return 0;
            }
            state.borrow_mut().sweep_sample = [overlap[0], overlap[1], overlap[2], 2.0];
            let p = lerp(&prev_start, &cur_start, overlap[0]);
            let q = lerp(old_b, b_, overlap[0]);
            let point = lerp(&p, &q, overlap[1]);
            let separated = prev_entry.as_ref().is_some_and(|e| e.borrow().ended_clear);
            // the newest probe may lose to a sibling: only their shared entry can reserve this crossing or retry it
            let pending = prev_entry.as_ref().is_some_and(|e| {
                let e = e.borrow();
                !e.claimed && !e.expired
            });
            let npc_id = eid(&npc);
            let retry = pending && previous.as_ref().is_some_and(|p| p.borrow().target == npc_id) && !separated;
            if pending && !retry && !separated {
                why("a pending probe");
                return 0;
            }
            if begin_on_contact && !retry && !melee::begin_swing(pl, w, 1, strong, speed) {
                why("the swing did not begin");
                return 0;
            }
            begin_on_contact = false;
            let _ = begin_on_contact;
            let mut direction = mul(&trace.normal(), -1.0);
            direction[3] = 0.0;
            if len(&direction) < 0.50 {
                direction = norm(&sub(&q, &p));
            }
            let entry = if retry { prev_entry.clone().unwrap_or_else(|| sh(Entry::default())) } else { sh(Entry { prior_serial: confirmed_serial(w), ..Entry::default() }) };
            let axis = norm(&sub(&q, &p));
            let tip_motion = with_w(sub(b_, old_b), 0.0);
            let hand_motion = with_w(sub(a, old_a), 0.0);
            let forward = dot(&tip_motion, &axis);
            let mut thrust = forward > 0.005 && forward >= 0.75 * len(&tip_motion) && dot(&hand_motion, &axis) > 0.0;
            // the prop can stop or deflect on a solver frame: the sampled controller motion classifies the contact
            let tracked = with_w(tracked, 0.0);
            let tracked_length = len(&tracked);
            if (0.018..=1.5).contains(&tracked_length) {
                thrust = dot(&tracked, &axis) >= 0.75 * tracked_length;
            }
            let stealth_intent = thrust || super::belt::shared(if hand_side(pl) == 0 { 154 } else { 160 }) > 0.50;
            let mut impact = Impact {
                entry: Some(entry.clone()),
                target: npc_id,
                a: p,
                b: q,
                attack_edge: with_w(v4_of(w, "GetWorldForward"), 0.0),
                previous_a: prev_start,
                previous_b: *old_b,
                end_a: cur_start,
                end_b: *b_,
                sweep_from: lerp(&prev_start, old_b, overlap[1]),
                sweep_to: lerp(&cur_start, b_, overlap[1]),
                thrust,
                created: now,
                no_dismember: no_dismember(w),
                stealth_intent,
                ..Impact::default()
            };
            if stealth_intent {
                let hit_slots = hit_representation(&npc);
                impact.has_neck_slot = neck_configured(&npc) || bn::slot_frame(hit_slots.instance.cast(), CName::hashed("Neck").0).is_some();
                impact.port_at_start = service_port(&npc);
                impact.rear_at_start = rpg::behind(pl, &npc);
            }
            let ended_clear = overlap[3] > 1.5;
            entry.borrow_mut().ended_clear = ended_clear;
            let impact = sh(impact);
            let prior_touch = state.borrow().touch.clone();
            {
                let mut s = state.borrow_mut();
                s.attempt = Some(impact.clone());
                s.touch = if ended_clear { WeakHandle::null() } else { WeakHandle::from_handle(&npc) };
            }
            let hit = add(&point, &mul(&direction, 0.12));
            if melee::ordinary_contact(pl, w, &npc, &point, &hit, false, Some(impact.clone())) > 0 {
                let mut s = state.borrow_mut();
                s.last_probe = Some(impact);
                s.sweep_sample = [overlap[0], overlap[1], overlap[2], 3.0];
                return 1;
            }
            impact.borrow_mut().cancelled = true;
            why("the ordinary contact was not made");
            let mut s = state.borrow_mut();
            s.attempt = previous.clone();
            s.touch = if retry && ended_clear { WeakHandle::null() } else { prior_touch };
        }
    }
    if previous.is_none() {
        state.borrow_mut().touch = WeakHandle::null();
    }
    0
}
/// WeaponObject.VRBladeLodgeContact: the seated puncture is lodged (the person held in place once).
pub(super) fn lodge_contact(w: &Handle, serial: u32) {
    let _g = lock();
    let Some(rc) = receipt(w) else { return };
    let (begin, joint) = {
        let r = rc.borrow();
        if r.serial != serial || !(r.puncture || r.lodged) || r.released {
            return;
        }
        let resume = weapon(w).borrow().resume_hold_joint;
        (!r.lodged || resume == r.joint_serial, r.joint_serial)
    };
    if std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1") {
        cvr_host::log!("[melee-trace] lodge: serial {serial}, joint {joint}, the hold begun {begin}\n");
    }
    if begin {
        weapon(w).borrow_mut().resume_hold_joint = 0;
        hold::begin(w, &rc);
    }
    let mut r = rc.borrow_mut();
    r.lodged = true;
    r.awaiting_seat = false;
    r.lodged_at = real_time();
}
/// WeaponObject.VRBladeReleaseContact: the receipt's contact let go.
pub(super) fn release_contact(w: &Handle, serial: u32) {
    let _g = lock();
    let Some(rc) = receipt(w) else { return };
    if rc.borrow().serial != serial {
        return;
    }
    if rc.borrow().lodged {
        hold::end(w, &rc);
    }
    let entry = {
        let mut r = rc.borrow_mut();
        r.released = true;
        r.lodged = false;
        r.awaiting_seat = false;
        if let Some(p) = r.pending.take() {
            p.borrow_mut().cancelled = true;
        }
        if let Some(e) = &r.entry {
            e.borrow_mut().expired = true;
        }
        r.entry.clone()
    };
    if let Some(e) = entry {
        let state = weapon(w);
        let same = state.borrow().attempt.as_ref().and_then(|a| a.borrow().entry.clone()).is_some_and(|x| Rc::ptr_eq(&x, &e));
        if same {
            let mut s = state.borrow_mut();
            s.touch = WeakHandle::null();
            s.attempt = None;
        }
    }
}
/// The weapon's entity left the world: its contact released and its fields dropped.
pub(super) fn weapon_detached(w: &Handle) {
    let _g = lock();
    if let Some(serial) = receipt(w).map(|r| r.borrow().serial) {
        release_contact(w, serial);
    }
    let id = eid(w);
    store(|s| s.weapons.remove(&id));
}
pub(super) fn note_probe(w: &Handle, impact: &Sh<Impact>) {
    weapon(w).borrow_mut().last_probe = Some(impact.clone());
}

// ---------------------------------------------------------------- the person
/// NPCPuppet.VRBladeServicePort: the neck's point -- the hit representation's Neck slot, the configured neck sphere's
/// centre in the Head slot, else three quarters of the way from the Chest to the Head attachment slot.
pub(super) fn service_port(npc: &Handle) -> V4 {
    let shapes = hit_representation(npc);
    let sp = shapes.instance.cast();
    if let Some(upper) = bn::slot_frame(sp, CName::hashed("Neck").0) {
        return p4(&upper.position(), 1.0);
    }
    if neck_configured(npc) {
        if let Some(upper) = bn::slot_frame(sp, CName::hashed("Head").0) {
            return to_world(&upper, &[-0.070, 0.0135, 0.0, 1.0]);
        }
    }
    let slots = rt::obj(npc, "FindComponentByName", &[A::Name(CName::hashed("Item_Attachment_Slot").0)]);
    let (Some(lower), Some(upper)) = (bn::slot_frame(slots.instance.cast(), CName::hashed("Chest").0), bn::slot_frame(slots.instance.cast(), CName::hashed("Head").0)) else {
        return ZERO;
    };
    let (a, b_) = (p4(&lower.position(), 1.0), p4(&upper.position(), 1.0));
    let span = dist(&a, &b_);
    if !(span > 0.14 && span < 0.50) {
        return ZERO;
    }
    with_w(lerp(&a, &b_, 0.75), 1.0)
}

/// The mechanics' frame: the people's movement holds checked.
pub(super) fn tick() {
    let _g = lock();
    hold::tick();
}
/// WeaponObject.VRBladeLastSweep: the last active query (time along the sweep, point along the blade, depth, stage:
/// 0 clear, 1 a person's proxy, 2 a measured overlap, 3 the effect dispatched).
pub(super) fn last_sweep(w: &Handle) -> V4 {
    let _g = lock();
    weapon(w).borrow().sweep_sample
}
