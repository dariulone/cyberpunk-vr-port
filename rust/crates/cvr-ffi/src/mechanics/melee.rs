//! mods/redscript/CyberpunkVRPort_Melee and the combat half of CyberpunkVRPort_Cyberware: VR motion melee.
//!
//! A swing is the weapon's real motion: the blade (grip to tip, a hit zone around it) or the arm cyberware's points are
//! swept from the last frame to this one against the NPCs the targeting system finds (each body an axis from 0.25 m to
//! 1.65 m), and a touch fires the game's own melee Attack_GameEffect at the touched body -- its reactions, blood, sound,
//! impulse and damage. A swing is begun on the stroke (its record: the combo chain's next light stroke, a strong one,
//! the weapon's own), each target hit once a stroke, the stamina its record costs spent, the strong arms' and mantis
//! blades' muscles pulsed for its duration. Enhanced melee multiplies the native damage of the player's own hits by
//! the combo (1..10 hits on one target within 5 s: x1..x2), two hands on a katana or a club (x2) and a fast stroke
//! (x1.5), matched in CalculateDamageVariants to the attack the swing registered and confirmed by the damage events.
//! The whoosh and the draw sound are the audio configs' events; the melee machine's light and strong attack inputs are
//! refused while physical melee is on; a melee weapon never idle-unequips; a deflected blow on the player counts toward
//! the deflect tutorial.
//!
//! The PlayerPuppet fields of the scripts are [`Combat`], per player; the gameHitEvent's ticket a map by the event.
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_rt as r;
use crate::script_call::{Owned, Value};
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::sync::Mutex;

pub(super) type V3 = [f64; 3];
pub(super) type Item = [u64; 2];

pub(super) fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
pub(super) fn v4(v: V3, w: f64) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w as f32]
}
pub(super) fn f3(v: [f32; 4]) -> V3 {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}
pub(super) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(super) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(super) fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(super) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub(super) fn dist(a: V3, b: V3) -> f64 {
    len(sub(a, b))
}
pub(super) fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-9 {
        mul(a, 1.0 / l)
    } else {
        [0.0; 3]
    }
}
fn item_of(o: &Owned) -> Item {
    // SAFETY: an ItemID result (16 bytes).
    unsafe { *(o.as_ptr() as *const Item) }
}
pub(super) fn same_item(a: &Item, b: &Item) -> bool {
    a[0] == b[0] && (a[1] & 0xFFFF_FFFF_FFFF) == (b[1] & 0xFFFF_FFFF_FFFF)
}
pub(super) fn item_id(o: &Handle) -> Item {
    rt::call(o, "GetItemID", &[]).map_or([0, 0], |x| item_of(&x))
}
pub(super) fn entity_id(o: &Handle) -> u64 {
    super::belt::entity_hash(o).unwrap_or(0)
}
fn name_of(o: Option<Owned>) -> u64 {
    // SAFETY: a CName / TweakDBID result (8 bytes).
    o.map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
pub(super) fn world_pos(o: &Handle) -> Option<V3> {
    super::belt::m::<[f32; 4]>(o, "GetWorldPosition", &[]).ok().map(f3)
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    super::belt::native::<T>(name, values)
}
pub(super) fn real_time() -> f64 {
    native::<f32>("VRMeleeRealTime", &[]).map_or(0.0, f64::from)
}
/// EngineTime.ToFloat(GameInstance.GetSimTime()).
pub(super) fn sim_time() -> f64 {
    r::static_gi("GameInstance", "GetSimTime", &[])
        .and_then(|t| rt::static_num("EngineTime", "ToFloat", &[A::P(t.as_ptr().cast())]))
        .unwrap_or(0.0)
}
pub(super) fn holster_intent(_pl: &Handle) -> bool {
    super::holster::holster_intent_active(1.0)
}
/// VRPortGuardHands (formerly CyberpunkVRPort_Engine's vrport_haptics_shared.reds): which hands a guard or block
/// shakes -- 3 both for fists and arm implants in the right hand, 2 the right for another right weapon, 1 the left for
/// a left weapon alone that is not an implant, else 2.
pub(super) fn guard_hands(pl: &Handle) -> i32 {
    let weapon = super::belt::in_slot(pl, "AttachmentSlots.WeaponRight");
    if r::is_a(&weapon, "WeaponObject") {
        let id = item_id(&weapon);
        return if is_type(&id, "Wea_Fists") || is_type(&id, "Cyb_StrongArms") || is_type(&id, "Cyb_MantisBlades") { 3 } else { 2 };
    }
    let left = super::belt::in_slot(pl, "AttachmentSlots.WeaponLeft");
    if r::is_a(&left, "WeaponObject") {
        let id = item_id(&left);
        if !rt::static_call("WeaponObject", "IsCyberwareWeapon", &[A::P(id.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool()) {
            return 1;
        }
    }
    2
}
/// The weapon's item is of the type (gamedataItemType member), for the damage self-test.
pub(super) fn weapon_is_type(weapon: &Handle, t: &str) -> bool {
    !weapon.is_null() && is_type(&item_id(weapon), t)
}
fn is_type(item: &Item, t: &str) -> bool {
    rt::static_call("WeaponObject", "IsOfType", &[A::P(item.as_ptr().cast()), A::N(rt::enum_value("gamedataItemType", t))]).is_some_and(|o| Val::of(&o).bool())
}
/// The stat of an object (StatsSystem.GetStatValue(Cast<StatsObjectID>(id), stat)).
pub(super) fn stat(id: u64, s: &str) -> f64 {
    let stats = super::game_system("GetStatsSystem");
    let sid = [id, 0u64];
    rt::num(&stats, "GetStatValue", &[A::P(sid.as_ptr().cast()), A::N(rt::enum_value("gamedataStatType", s))]).unwrap_or(0.0)
}

/// The NPCs (alive, each once) the targeting system finds within `max` of the player.
pub(super) fn npcs(pl: &Handle, max: f64) -> Vec<Handle> {
    let Some(mut q) = r::new_struct("TargetSearchQuery") else { return Vec::new() };
    let qv = r::val(&q);
    r::set(&qv, "testedSet", r::ev("TargetingSet", "Complete"));
    r::set(&qv, "maxDistance", max);
    r::set(&qv, "filterObjectByDistance", 1.0);
    r::set(&qv, "ignoreInstigator", 1.0);
    if let (Some(filter), Some(sf)) = (rt::global_call("TSF_NPC", &[]), qv.f("searchFilter")) {
        // SAFETY: the filter field and the global's result of the same type.
        unsafe { sf.ty.assign(sf.ptr, filter.as_ptr().cast()) };
    }
    let targeting = super::game_system("GetTargetingSystem");
    let Some((_, outs)) = rt::call_outs(&targeting, "GetTargetParts", &[A::H(pl), A::P(q.as_mut_ptr().cast_const().cast())], 1) else { return Vec::new() };
    let Some(parts) = outs.first() else { return Vec::new() };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for part in r::items(&Val::of(parts)) {
        let comp = rt::static_call("TS_TargetPartInfo", "GetComponent", &[A::P(part.ptr.cast_const())]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        let e = r::o(&comp, "GetEntity", &[]);
        if e.is_null() || !r::is_a(&e, "GameObject") || r::b(&e, "IsDead", &[]) {
            continue;
        }
        let id = entity_id(&e);
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        out.push(e);
    }
    out
}

// ---------------------------------------------------------------- the state
/// The enhanced melee's ticket of an attack the player started (VrMeleeEnhancedTicket).
struct Enhanced {
    attack: Handle,
    weapon: Item,
    target: u64,
    created: f64,
    created_sim: f64,
    speed: f64,
    two_handed: bool,
    claimed: bool,
    confirmed: bool,
    combo: i32,
    multiplier: f64,
    native_damage: f64,
    serial: u64,
}
/// A stroke of the arm cyberware or the melee weapon (VrCyberwareSwing).
#[derive(Clone)]
pub(super) struct Swing {
    pub weapon: Handle,
    pub record: Handle,
    pub targets: Vec<u64>,
    pub number: i32,
    pub peak_speed: f64,
    pub chain: bool,
    pub landed: bool,
    /// The stroke's number among the player's swings (its identity).
    pub id: i32,
}
/// An attack a swing started (VrCyberwareAttackTicket).
struct AttackTicket {
    attack: Handle,
    time: f64,
    targets: Vec<u64>,
    intended: u64,
    weapon: Item,
}
#[derive(Default)]
pub(super) struct Combat {
    owner: usize,
    enhanced: Vec<Enhanced>,
    serial: u64,
    pub enhanced_disabled: bool,
    combo_target: u64,
    combo_count: i32,
    combo_last: f64,
    pub confirmed_hits: i32,
    pub last_base: f64,
    pub last_multiplier: f64,
    pub last_dealt: f64,
    pub last_speed: f64,
    pub last_two_handed: bool,
    /// The gameHitEvent's claimed ticket (by the event object, its serial).
    hit_ticket: HashMap<usize, u64>,
    pub swings: [Option<Swing>; 2],
    tickets: Vec<AttackTicket>,
    pub swing_count: i32,
    pub attack_count: i32,
    pub hit_count: i32,
    pub last_native_damage: f64,
    pub last_hit_delay: f64,
    pub damage_events: i32,
    pub last_geometry: Vec<[f32; 4]>,
    chain_number: i32,
    chain_time: f64,
    chain_weapon: Item,
    pub chain_finals: i32,
}
// SAFETY: the handles are made, used and dropped on the game's main thread (the hooks, the mechanics' tick).
unsafe impl Send for Combat {}
static COMBAT: Mutex<Option<Combat>> = Mutex::new(None);
/// The state of `pl` (made anew for another player), the lock released before `f`'s engine calls only when `f` takes
/// care: the closures here read and write fields, the engine calls happen outside.
pub(super) fn with<T>(pl: &Handle, f: impl FnOnce(&mut Combat) -> T) -> T {
    let mut g = COMBAT.lock().unwrap_or_else(|p| p.into_inner());
    let owner = pl.instance as usize;
    if g.as_ref().is_none_or(|c| c.owner != owner) {
        *g = Some(Combat { owner, combo_last: -10.0, chain_time: -10.0, ..Combat::default() });
    }
    f(g.as_mut().expect("the combat state"))
}

// ---------------------------------------------------------------- the weapon's attacks
fn melee_record(a: &Handle) -> bool {
    !a.is_null() && r::is_a(a, "Attack_GameEffect") && r::is_a(&r::o(a, "GetRecord", &[]), "Attack_Melee_Record")
}
fn attack_named(weapon: &Handle, name: &str) -> Handle {
    let a = rt::obj(weapon, "GetAttack", &[n(name)]);
    if r::is_a(&a, "Attack_GameEffect") {
        a
    } else {
        Handle::null()
    }
}
/// WeaponObject.VRGetMeleeAttack: a native melee record (strong, the combo's first, the current, any).
pub(super) fn melee_attack(weapon: &Handle, strong: bool) -> Handle {
    let mut a = Handle::null();
    if strong {
        a = attack_named(weapon, "MeleeStrongAttack0");
    }
    if a.is_null() {
        a = attack_named(weapon, "MeleeComboAttack0");
    }
    if melee_record(&a) {
        return a;
    }
    let cur = rt::obj(weapon, "GetCurrentAttack", &[]);
    if melee_record(&cur) {
        return cur;
    }
    if let Some(all) = rt::call(weapon, "GetAttacks", &[]) {
        for x in r::items(&Val::of(&all)) {
            let h = x.handle();
            if melee_record(&h) {
                return h;
            }
        }
    }
    Handle::null()
}
/// WeaponObject.VRMeleeReach: the Impact marker's physical reach for clubs and blades (VRBladeLocalEnd); a fist's
/// knuckle segment (10 cm), even when its record has a lunge.
pub(super) fn reach(weapon: &Handle) -> f64 {
    if is_type(&item_id(weapon), "Wea_Fists") {
        return 0.10;
    }
    let e = super::blade::local_end(weapon);
    f64::from((e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt())
}
/// WeaponObject.VRMeleeDirection: along the blade to its Impact marker (the item's turn), a fist's forward.
pub(super) fn direction(weapon: &Handle) -> [f32; 4] {
    if is_type(&item_id(weapon), "Wea_Fists") {
        return super::belt::m::<[f32; 4]>(weapon, "GetWorldForward", &[]).unwrap_or([0.0, 1.0, 0.0, 0.0]);
    }
    let q = super::belt::m::<[f32; 4]>(weapon, "GetWorldOrientation", &[]).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let d = super::blade::rotate(&q, &super::blade::local_end(weapon));
    super::blade::norm(&d)
}
/// PlayerPuppet.VRCyberwareKind: 1 strong arms, 2 mantis blades, 3 monowire, 4 launcher, 0 anything else.
pub(super) fn kind(weapon: &Handle) -> i32 {
    if weapon.is_null() {
        return 0;
    }
    let id = item_id(weapon);
    for (k, t) in [(1, "Cyb_StrongArms"), (2, "Cyb_MantisBlades"), (3, "Cyb_NanoWires"), (4, "Cyb_Launcher")] {
        if is_type(&id, t) {
            return k;
        }
    }
    0
}

// ---------------------------------------------------------------- the combo chain
const CHAIN_WINDOW: f64 = 1.50;
/// VRMeleeChainLength: the weapon's AttacksNumber, and whether weapon and player combo infinitely.
fn chain_length(pl: &Handle, weapon: &Handle) -> (i32, bool) {
    let w = entity_id(weapon);
    let infinite = stat(w, "CanWeaponInfinitlyCombo") > 0.0 && stat(entity_id(pl), "CanMeleeInfinitelyCombo") > 0.0;
    (stat(w, "AttacksNumber") as i32, infinite)
}
/// VRMeleeChainAttack: the chain's next light stroke (the final one at its end), its number.
fn chain_attack(pl: &Handle, weapon: &Handle) -> (Handle, i32) {
    let now = sim_time();
    let (attacks, infinite) = chain_length(pl, weapon);
    let wid = item_id(weapon);
    let mut number = with(pl, |c| {
        if !same_item(&c.chain_weapon, &wid) || now < c.chain_time || now - c.chain_time > CHAIN_WINDOW {
            c.chain_number = 0;
        }
        c.chain_number
    });
    let mut attack = Handle::null();
    if !infinite && attacks > 1 && number + 1 >= attacks {
        attack = attack_named(weapon, "MeleeFinalAttack0");
    }
    if attack.is_null() {
        attack = attack_named(weapon, &format!("MeleeComboAttack{number}"));
    }
    if attack.is_null() {
        number = 0;
        attack = attack_named(weapon, "MeleeComboAttack0");
    }
    (attack, number)
}
/// VRMeleeChainLanded: the chain moves on once per stroke (a finisher ends it).
fn chain_landed(pl: &Handle, swing: &Swing) {
    let (attacks, infinite) = chain_length(pl, &swing.weapon);
    let subtype = r::o(&swing.record, "AttackSubtype", &[]);
    let mut next = swing.number + 1;
    let final_ = !subtype.is_null() && rt::num(&subtype, "Type", &[]) == Some(rt::enum_value("gamedataAttackSubtype", "FinalAttack"));
    if final_ {
        next = attacks;
    }
    if next >= attacks {
        next = i32::from(infinite);
    }
    let (now, wid) = (sim_time(), item_id(&swing.weapon));
    with(pl, |c| {
        if final_ {
            c.chain_finals += 1;
        }
        c.chain_number = next;
        c.chain_time = now;
        c.chain_weapon = wid;
    });
}

// ---------------------------------------------------------------- swings
fn held_item(pl: &Handle) -> Handle {
    let ts = super::belt::transactions();
    rt::obj(&ts, "GetItemInSlot", &[A::H(pl), A::Name(super::left_handed::held_slot(pl))])
}
/// PlayerPuppet.VRCyberwareBeginSwing: a stroke begun (its record, stamina, the muscles' pulse); false when refused.
pub(super) fn begin_swing(pl: &Handle, weapon: &Handle, side: i32, strong: bool, speed: f64) -> bool {
    if !(0..=1).contains(&side) || weapon.is_null() || holster_intent(pl) {
        return false;
    }
    if super::blade::kind(weapon) > 0 && !super::blade::allowed(pl, weapon) {
        return false;
    }
    let item = held_item(pl);
    if item.is_null() || !same_item(&item_id(&item), &item_id(weapon)) {
        return false;
    }
    let k = kind(weapon);
    let chained = k == 0 && !strong;
    let mut number = if side == 0 { 1 } else { 0 };
    let mut attack = Handle::null();
    if k == 0 {
        if strong {
            with(pl, |c| {
                c.chain_number = 0;
                c.chain_time = -10.0;
            });
        } else {
            (attack, number) = chain_attack(pl, weapon);
        }
    }
    if strong {
        attack = attack_named(weapon, &format!("MeleeStrongAttack{number}"));
        if attack.is_null() {
            attack = attack_named(weapon, "MeleeStrongAttack0");
        }
    }
    if attack.is_null() {
        attack = attack_named(weapon, &format!("MeleeComboAttack{number}"));
    }
    if attack.is_null() {
        attack = attack_named(weapon, "MeleeComboAttack0");
    }
    if attack.is_null() {
        attack = melee_attack(weapon, strong);
    }
    if attack.is_null() {
        return false;
    }
    let record = r::o(&attack, "GetRecord", &[]);
    if !r::is_a(&record, "Attack_Melee_Record") {
        return false;
    }
    let mut swing = Swing { weapon: weapon.clone(), record: record.clone(), targets: Vec::new(), number, peak_speed: speed.clamp(0.0, 30.0), chain: chained, landed: false, id: 0 };
    let wptr = weapon.instance as usize;
    with(pl, |c| {
        if k != 0 {
            if let Some(old) = &c.swings[side as usize] {
                if old.weapon.instance as usize == wptr {
                    swing.targets = old.targets.clone();
                }
            }
        }
        c.swing_count += 1;
        swing.id = c.swing_count;
        c.swings[side as usize] = Some(swing);
    });
    // the stamina the record costs
    if let Some((_, outs)) = rt::call_outs(&record, "StaminaCost", &[], 1) {
        if let Some(costs) = outs.first() {
            let sid = [entity_id(weapon), 0u64];
            let cost = r::static_gi("RPGManager", "CalculateStatModifiers", &[A::P(costs.as_ptr().cast()), A::H(weapon), A::P(sid.as_ptr().cast())])
                .and_then(|o| Val::of(&o).num())
                .unwrap_or(0.0);
            if cost > 0.0 {
                rt::static_call("PlayerStaminaHelpers", "ModifyStamina", &[A::H(pl), A::N(-cost)]);
            }
        }
    }
    true
}
// ---------------------------------------------------------------- the attack
fn effect_defs() -> Option<Handle> {
    let defs = super::boards::defs().ok()?;
    super::boards::definition(&defs, "EffectSharedData").ok().cloned()
}
/// EffectData.Set<kind>(effect's shared data, EffectSharedData.<field>, value).
fn set_effect(data: &Owned, defs: &Handle, field: &str, setter: &str, value: A) {
    let Ok(id) = super::boards::field(defs, field) else { return };
    rt::static_call("EffectData", setter, &[A::P(data.as_ptr().cast()), A::P(id), value]);
}
/// VRCyberwareAttack: the swing's record as an Attack_GameEffect, a box from the contact along the blow (a blade
/// impact: along its own real segment, or the edge cut's chord from outside), started; its ticket and the enhanced
/// melee's registered, the attack's person and impact noted for the damage pipeline.
/// The half of the attack's box against a target whose hit shapes are boxes (a mech): measured on the Minotaur, 0.08
/// finds no shape, 0.3 does.
pub(super) const WIDE_HALF: f64 = 0.30;
fn attack(pl: &Handle, side: i32, contact: V3, target: V3, target_id: u64, radius: f64, impact: Option<super::blade::Sh<super::blade::Impact>>) -> bool {
    let _g = super::blade::lock();
    let Some(swing) = with(pl, |c| c.swings.get(side as usize).and_then(|s| s.clone())) else { return false };
    let now = sim_time();
    with(pl, |c| c.tickets.retain(|t| now - t.time <= 2.0));
    let Some(ctx) = r::new_struct("AttackInitContext") else { return false };
    let cv = r::val(&ctx);
    r::set_h(&cv, "record", &swing.record);
    r::set_h(&cv, "instigator", pl);
    // the attacking puppet is the source, as in the flat game: hit reactions read its weapon type from it
    r::set_h(&cv, "source", pl);
    r::set_h(&cv, "weapon", &swing.weapon);
    let att = r::so("IAttack", "Create", &[A::P(r::ptr(&ctx))]);
    if !r::is_a(&att, "Attack_GameEffect") {
        return false;
    }
    let effect = rt::obj(&att, "PrepareAttack", &[A::H(pl)]);
    if effect.is_null() {
        return false;
    }
    let mut target = target;
    let mut dir = norm(sub(target, contact));
    if dist(contact, target) < 0.01 {
        dir = if kind(&swing.weapon) == 0 {
            norm(f3(direction(&swing.weapon)))
        } else {
            let p = |i: i32| native::<[f32; 4]>("VRCyberwarePoint", &[Value::Int32(side), Value::Int32(i)]).map_or([0.0; 3], f3);
            norm(sub(p(1), p(0)))
        };
    }
    // the sweep begins outside the touched body, even when the segment crossed its centre between samples
    let mut start = contact;
    let mut radius = radius;
    if let Some(im) = &impact {
        // the real segment in its own direction: a box down the broad proxy normal returns hits away from the blade
        let mut i = im.borrow_mut();
        start = f3(i.a);
        target = f3(i.b);
        if i.edge_stroke != 0 {
            // the clipped chord kept for damage; the hit representation acquired along the shaft from outside
            let Some(cut) = i.cut_shape.clone() else { return false };
            if cut.reverse_query {
                (start, target) = (f3(i.b), f3(i.a));
            }
        } else if i.sweep_from[3] > 0.5 && super::blade::dist(&i.sweep_from, &i.sweep_to) >= 0.02 {
            start = f3(i.sweep_from);
            target = f3(i.sweep_to);
        }
        dir = norm(sub(target, start));
        radius = f64::from(super::blade::box_half_extent());
        i.swing = Some((side, swing.clone()));
    }
    // A MECH'S HIT SHAPES ARE BOXES lying deeper than its armour's surface: a sweep with the blade's few centimetres
    // of box that touches the body finds none of them (the effect hands no hit at all); with a box of WIDE_HALF it
    // reaches them, as the flat game's wide melee box does. Only for a target with box shapes.
    let wide = super::find_entity(target_id).ok().is_some_and(|e| {
        let comp = super::blade::hit_representation(&e);
        !comp.is_null() && crate::blade_natives::has_box_shapes(comp.instance.cast())
    });
    if wide {
        radius = radius.max(WIDE_HALF);
        if let Some(im) = &impact {
            im.borrow_mut().wide = true;
        }
    }
    // tests: the sweep begun CVR_MELEE_EFFECT_BACK metres further back, its box's half CVR_MELEE_EFFECT_HALF
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok());
    if let Some(back) = env("CVR_MELEE_EFFECT_BACK").filter(|b| (0.0..=2.0).contains(b)) {
        start = sub(start, mul(dir, back));
    }
    if let Some(half) = env("CVR_MELEE_EFFECT_HALF").filter(|h| (0.01..=1.0).contains(h)) {
        radius = half;
    }
    let range = dist(start, target);
    if std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1") {
        cvr_host::log!("[melee-trace] the attack's sweep: from {start:?} along {dir:?}, {range:.3} m, box half {radius:.3} (box-shaped target {wide})\n");
    }
    let Some(data) = rt::call(&effect, "GetSharedData", &[]) else { return false };
    if let Some(defs) = effect_defs() {
        // ToVariant(attack) of main's ref<Attack_GameEffect>: the Variant typed by the variable's class (the native
        // name of Attack_GameEffect), not by IAttack -- the effect reads the attack out of it by its type
        let rtti = red4ext_abi::rtti::RttiSystem::get();
        let attack_type = rtti
            .and_then(|r| r.get_type(CName::hashed("handle:gameAttack_GameEffect")))
            .or_else(|| rtti.and_then(|r| r.get_type(CName::hashed("handle:IAttack"))));
        if let Some(ty) = attack_type {
            if let Some(mut h) = Owned::of_type(ty) {
                // SAFETY: a handle value of its own type, assigned from ours.
                unsafe { ty.assign(h.as_mut_ptr().cast(), (&att as *const Handle).cast()) };
                if let Some(var) = super::menu_rt::variant_of(&h) {
                    set_effect(&data, &defs, "attack", "SetVariant", A::P(var.as_ptr().cast()));
                }
            }
        }
        let bx = v4([radius * 2.0; 3], 0.0);
        let pos = v4(start, 1.0);
        let fwd = v4(dir, 0.0);
        set_effect(&data, &defs, "box", "SetVector", A::P(bx.as_ptr().cast()));
        set_effect(&data, &defs, "duration", "SetFloat", A::N(0.05));
        set_effect(&data, &defs, "position", "SetVector", A::P(pos.as_ptr().cast()));
        if let Some(q) = rt::static_call("Quaternion", "BuildFromDirectionVector", &[A::P(fwd.as_ptr().cast())]) {
            set_effect(&data, &defs, "rotation", "SetQuat", A::P(q.as_ptr().cast()));
        }
        set_effect(&data, &defs, "forward", "SetVector", A::P(fwd.as_ptr().cast()));
        set_effect(&data, &defs, "range", "SetFloat", A::N(range));
        set_effect(&data, &defs, "attackNumber", "SetInt", A::N(f64::from(swing.number)));
        set_effect(&data, &defs, "impactOrientationSlot", "SetName", A::Name(name_of(rt::call(&swing.record, "ImpactFxSlot", &[]))));
        let charge = rt::static_num("WeaponObject", "GetWeaponChargeNormalized", &[A::H(&swing.weapon)]).unwrap_or(0.0);
        set_effect(&data, &defs, "charge", "SetFloat", A::N(charge));
        set_effect(&data, &defs, "meleeCleave", "SetBool", A::B(false));
        // ToVariant(swing.weapon.GetFxPackage()): the Variant typed by the function's declared result
        let fx = rt::call(&swing.weapon, "GetFxPackage", &[]);
        let fx_set = match fx.as_ref().and_then(super::menu_rt::variant_of) {
            Some(var) => {
                set_effect(&data, &defs, "fxPackage", "SetVariant", A::P(var.as_ptr().cast()));
                true
            }
            None => false,
        };
        if std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1") {
            cvr_host::log!(
                "[melee-trace] the attack's effect: fxPackage {} (GetFxPackage {}), a blade impact {}\n",
                if fx_set { "set" } else { "NOT set" },
                if fx.is_some() { "answered" } else { "failed" },
                impact.is_some()
            );
        }
        set_effect(&data, &defs, "playerOwnedWeapon", "SetBool", A::B(true));
    }
    super::blade::note_attack(&att, target_id, impact.clone(), swing.chain);
    let geometry = vec![v4(contact, 1.0), v4(target, 1.0), v4(start, 1.0), v4(dir, range)];
    let wid = item_id(&swing.weapon);
    with(pl, |c| {
        c.last_geometry = geometry;
        c.tickets.push(AttackTicket { attack: att.clone(), time: now, targets: Vec::new(), intended: target_id, weapon: wid });
    });
    let serial = register_attack(pl, &att, &swing.weapon, target_id, swing.peak_speed);
    let started = r::b(&att, "StartAttack", &[]);
    if impact.is_some() && std::env::var("CVR_BLADE_COMBAT_TEST").is_ok_and(|v| v == "1") {
        cvr_host::log!(
            "[blade-combat] the blade's attack {:#x}: the box from {start:?} to {target:?} (half {radius:.3}, range {range:.3}), started {started}\n",
            att.instance as usize
        );
    }
    if !started {
        let a = att.instance as usize;
        with(pl, |c| c.tickets.retain(|t| t.attack.instance as usize != a));
        cancel_attack(pl, serial);
        return false;
    }
    if impact.is_none() && swing.chain && !swing.landed {
        chain_landed_once(pl, side, &swing);
    }
    with(pl, |c| c.attack_count += 1);
    true
}
/// A chained stroke reached a body: marked landed once, the chain moved on (VRMeleeChainLanded).
pub(super) fn chain_landed_once(pl: &Handle, side: i32, swing: &Swing) {
    if !swing.chain {
        return;
    }
    let first = with(pl, |c| match c.swings.get_mut(side as usize).and_then(|s| s.as_mut()) {
        Some(s) if s.id == swing.id && !s.landed => {
            s.landed = true;
            true
        }
        _ => false,
    });
    if first {
        chain_landed(pl, swing);
    }
}
fn mark_target(pl: &Handle, side: i32, id: u64) {
    with(pl, |c| {
        if let Some(s) = c.swings.get_mut(side as usize).and_then(|s| s.as_mut()) {
            s.targets.push(id);
        }
    });
}
/// VRCyberwareOrdinaryContact: a melee weapon's touch -- the right hand's stroke, a target once a deliberate stroke
/// (a blade impact may cut again on a new reversal); the hand feels it, a blade only on its damage event.
pub(super) fn ordinary_contact(pl: &Handle, weapon: &Handle, target: &Handle, contact: &[f32; 4], hit: &[f32; 4], passive: bool, impact: Option<super::blade::Sh<super::blade::Impact>>) -> i32 {
    if weapon.is_null() || holster_intent(pl) {
        return 0;
    }
    let w = weapon.instance as usize;
    if target.is_null() {
        return 0;
    }
    let tid = entity_id(target);
    let touched = with(pl, |c| c.swings[1].as_ref().filter(|s| s.weapon.instance as usize == w).map(|s| s.targets.contains(&tid)));
    let Some(touched) = touched else { return 0 };
    if passive || (touched && impact.is_none()) {
        return 0;
    }
    let local = impact.is_some();
    if attack(pl, 1, f3(*contact), f3(*hit), tid, 0.175, impact) {
        if !local {
            mark_target(pl, 1, tid);
            native::<()>("VRHapticEvent", &[Value::Int32(14), Value::Int32(guard_hands(pl))]);
        }
        return 1;
    }
    0
}
/// PlayerPuppet.VRMeleeSweepHit: a blade's sweep (VRBladeSweep); another weapon's segment as the arm's contacts.
#[allow(clippy::too_many_arguments)]
pub(super) fn sweep_hit(pl: &Handle, weapon: &Handle, start: [f32; 4], end: [f32; 4], prev_start: [f32; 4], prev_end: [f32; 4], radius: f64, passive: bool, tracked: [f32; 4], begin_on_contact: bool, strong: bool, speed: f64) -> i32 {
    if weapon.is_null() || holster_intent(pl) {
        return 0;
    }
    if super::blade::kind(weapon) > 0 {
        return super::blade::sweep(pl, weapon, &prev_start, &prev_end, &start, &end, passive, begin_on_contact, tracked, strong, speed);
    }
    contacts(pl, weapon, 1, &[f3(start), f3(end)], &[f3(prev_start), f3(prev_end)], radius, passive, begin_on_contact, strong, speed)
}
/// PlayerPuppet.VRCyberwareContacts: each segment of the arm's (or the weapon's) points swept from the last check by
/// "Melee" rays; a hostile person's hit shape crossed (VRBladeSweptContact) is struck once while it stays in touch --
/// beginning the stroke on that contact when asked.
#[allow(clippy::too_many_arguments)]
pub(super) fn contacts(pl: &Handle, weapon: &Handle, side: i32, points: &[V3], previous: &[V3], radius: f64, passive: bool, mut begin_on_contact: bool, strong: bool, speed: f64) -> i32 {
    let sq = super::game_system("GetSpatialQueriesSystem");
    if weapon.is_null() || !(0..=1).contains(&side) || points.len() < 2 || points.len() > 46 || previous.len() != points.len() || holster_intent(pl) || sq.is_null() {
        return 0;
    }
    let w = weapon.instance as usize;
    let has_swing = |pl: &Handle| with(pl, |c| c.swings[side as usize].as_ref().is_some_and(|s| s.weapon.instance as usize == w));
    if passive && !with(pl, |c| c.swings[side as usize].as_ref().is_some_and(|s| s.weapon.instance as usize == w && !s.targets.is_empty())) {
        return 0;
    }
    let item = held_item(pl);
    if item.is_null() || item.instance != weapon.instance {
        return 0;
    }
    let p4 = |v: V3| [v[0] as f32, v[1] as f32, v[2] as f32, 1.0f32];
    let (mut touching, mut count) = (Vec::new(), 0);
    for i in 0..points.len() - 1 {
        let (old_a, old_b, a, b) = (p4(previous[i]), p4(previous[i + 1]), p4(points[i]), p4(points[i + 1]));
        let moving = super::blade::dist(&old_a, &a) > 0.001 || super::blade::dist(&old_b, &b) > 0.001;
        let mut trace = super::blade::Trace::zeroed();
        if super::blade::swept_trace(&sq, &old_a, &old_b, &a, &b, passive || !moving, &mut trace).is_none() {
            continue;
        }
        let npc = trace.entity();
        let trace_on = !passive && std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1");
        if !r::is_a(&npc, "NPCPuppet") || !r::b(&npc, "IsActive", &[]) || r::b(&npc, "IsDead", &[]) || super::blade::sb("GameObject", "IsFriendlyTowardsPlayer", &[A::H(&npc)]) {
            if trace_on {
                let class = if npc.is_null() { String::from("nothing") } else { super::belt::class_name(&npc) };
                cvr_host::log!("[melee-trace] contacts: segment {i} traced {class}: not a hostile active person\n");
            }
            continue;
        }
        let id = entity_id(&npc);
        let shapes = super::blade::hit_representation(&npc);
        let overlap = crate::blade_natives::swept_contact(shapes.instance.cast(), &[super::blade::p3(&old_a), super::blade::p3(&old_b), super::blade::p3(&a), super::blade::p3(&b)], None);
        if trace_on {
            cvr_host::log!("[melee-trace] contacts: segment {i} traced a person (hit shapes {}): overlap {overlap:?}\n", !shapes.is_null());
        }
        if overlap[3] > 0.5 {
            touching.push(id);
        }
        let already = with(pl, |c| c.swings[side as usize].as_ref().filter(|s| s.weapon.instance as usize == w).is_some_and(|s| s.targets.contains(&id)));
        if overlap[3] > 0.5 && moving && !passive && !already {
            let mut from = super::blade::lerp(&old_a, &old_b, overlap[1]);
            let mut to = super::blade::lerp(&a, &b, overlap[1]);
            if super::blade::dist(&from, &to) < 0.001 {
                from = super::blade::lerp(&old_a, &a, overlap[0]);
                to = super::blade::lerp(&old_b, &b, overlap[0]);
            }
            if begin_on_contact {
                if !begin_swing(pl, weapon, side, strong, speed) {
                    return count;
                }
                begin_on_contact = false;
            }
            if has_swing(pl) && attack(pl, side, f3(from), f3(to), id, radius.clamp(0.02, 0.10), None) {
                mark_target(pl, side, id);
                count += 1;
            }
        }
    }
    with(pl, |c| {
        if let Some(s) = c.swings[side as usize].as_mut().filter(|s| s.weapon.instance as usize == w) {
            s.targets.retain(|t| touching.contains(t));
        }
    });
    count
}

// ---------------------------------------------------------------- enhanced melee
fn two_hand_eligible(weapon: &Handle) -> bool {
    let id = item_id(weapon);
    !weapon.is_null() && ["Wea_Katana", "Wea_OneHandedClub", "Wea_TwoHandedClub"].iter().any(|t| is_type(&id, t))
}
fn multiplier(hit: i32, speed: f64, fast: f64, two_handed: bool) -> f64 {
    let combo = 1.0 + f64::from(hit.clamp(1, 10) - 1) / 9.0;
    combo * if two_handed { 2.0 } else { 1.0 } * if speed >= fast { 1.5 } else { 1.0 }
}
/// VRMeleeRegisterAttack: the ticket the hit will be matched to (its serial; 0 for none).
fn register_attack(pl: &Handle, att: &Handle, weapon: &Handle, target: u64, speed: f64) -> u64 {
    let (now, sim) = (real_time(), sim_time());
    let two = two_hand_eligible(weapon) && native::<bool>("VRMeleeTwoHandedActive", &[]) == Some(true);
    let wid = item_id(weapon);
    with(pl, |c| {
        c.enhanced.retain(|t| !(now - t.created > 10.0 || sim - t.created_sim > 0.50 || t.confirmed));
        if att.is_null() || weapon.is_null() {
            return 0;
        }
        c.serial += 1;
        let serial = c.serial;
        c.enhanced.push(Enhanced {
            attack: att.clone(),
            weapon: wid,
            target,
            created: now,
            created_sim: sim,
            speed: speed.clamp(0.0, 30.0),
            two_handed: two,
            claimed: false,
            confirmed: false,
            combo: 0,
            multiplier: 1.0,
            native_damage: 0.0,
            serial,
        });
        serial
    })
}
fn cancel_attack(pl: &Handle, serial: u64) {
    with(pl, |c| c.enhanced.retain(|t| t.serial != serial));
}
fn record_id(att: &Handle) -> u64 {
    name_of(rt::call(&r::o(att, "GetRecord", &[]), "GetID", &[]))
}
fn is_melee_attack(attack_data: &Handle) -> bool {
    let t = rt::call(attack_data, "GetAttackType", &[]).and_then(|o| Val::of(&o).num()).unwrap_or(-1.0);
    rt::static_call("AttackData", "IsMelee", &[A::N(t)]).is_some_and(|o| Val::of(&o).bool())
}
/// VRMeleeApplyEnhancedHit: a hit matched to the attack it came from gets the combo, two-hand and speed multiplier.
fn apply_enhanced(pl: &Handle, hit: &Handle) {
    // hitEvent.attackData: a field (there is no method of the name: a call of one only missed first)
    let data = rt::field(hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let computed = rt::field(hit, "attackComputed").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let target = rt::field(hit, "target").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if rt::field(hit, "projectionPipeline").is_some_and(|v| v.bool()) || data.is_null() || computed.is_null() || target.is_null() {
        return;
    }
    let key = hit.instance as usize;
    if with(pl, |c| c.hit_ticket.contains_key(&key)) || !is_melee_attack(&data) {
        return;
    }
    let weapon = r::o(&data, "GetWeapon", &[]);
    let definition = r::o(&data, "GetAttackDefinition", &[]);
    if weapon.is_null() || definition.is_null() {
        return;
    }
    let (now, sim) = (real_time(), sim_time());
    let (tid, wid, rid) = (entity_id(&target), item_id(&weapon), record_id(&definition));
    // the candidate tickets' records, read outside the lock
    let candidates: Vec<(u64, u64)> = with(pl, |c| {
        c.enhanced
            .iter()
            .rev()
            .filter(|t| !t.claimed && t.target == tid && same_item(&t.weapon, &wid) && now >= t.created && now - t.created <= 10.0 && sim >= t.created_sim && sim - t.created_sim <= 0.35)
            .map(|t| (t.serial, t.attack.instance as usize as u64))
            .collect()
    });
    let attacks: Vec<(u64, Handle)> = with(pl, |c| c.enhanced.iter().filter(|t| candidates.iter().any(|x| x.0 == t.serial)).map(|t| (t.serial, t.attack.clone())).collect());
    let Some(serial) = candidates.iter().map(|x| x.0).find(|s| attacks.iter().any(|(a, h)| a == s && super::blade::same_attack(h, &definition) && record_id(h) == rid)) else { return };
    let follow = super::blade::attack_is_follow(&definition);
    let native_damage = rt::num(&computed, "GetTotalAttackValue", &[A::N(rt::enum_value("gamedataStatPoolType", "Health"))]).unwrap_or(0.0);
    let fast = if kind(&weapon) == 1 { 4.0 } else { 6.0 };
    let mult = with(pl, |c| {
        let (combo_target, combo_count, combo_last, disabled) = (c.combo_target, c.combo_count, c.combo_last, c.enhanced_disabled);
        let Some(t) = c.enhanced.iter_mut().find(|t| t.serial == serial) else { return None };
        t.claimed = true;
        t.native_damage = native_damage;
        t.combo = 1;
        if t.target == combo_target && now >= combo_last && now - combo_last <= 5.0 {
            t.combo = (combo_count + 1).min(10);
        }
        t.multiplier = if disabled || follow { 1.0 } else { multiplier(t.combo, t.speed, fast, t.two_handed) };
        let (m, speed, two) = (t.multiplier, t.speed, t.two_handed);
        c.hit_ticket.retain(|_, _| true);
        if c.hit_ticket.len() > 64 {
            c.hit_ticket.clear();
        }
        c.hit_ticket.insert(key, serial);
        c.last_base = native_damage;
        c.last_multiplier = m;
        c.last_speed = speed;
        c.last_two_handed = two;
        Some(m)
    });
    if let Some(m) = mult {
        rt::call(&computed, "MultAttackValue", &[A::N(m)]);
    }
}
/// VRMeleeConfirmEnhancedHit: the damage dealt confirms the ticket (a blade's only once its receipt is published)
/// and carries the combo on (not a follow-through's).
fn confirm_enhanced(pl: &Handle, hit: &Handle, damage: f64) {
    let key = hit.instance as usize;
    let now = real_time();
    if !(damage > 0.0) || rt::field(hit, "projectionPipeline").is_some_and(|v| v.bool()) {
        return;
    }
    let Some(view) = ticket_view(hit) else { return };
    if view.confirmed {
        return;
    }
    if super::blade::attack_has_impact(&view.attack) && !super::blade::publish_hit(pl, hit, damage, &view) {
        return;
    }
    let follow = super::blade::attack_is_follow(&view.attack);
    with(pl, |c| {
        c.hit_ticket.remove(&key);
        let disabled = c.enhanced_disabled;
        let Some(t) = c.enhanced.iter_mut().find(|t| t.serial == view.serial && !t.confirmed) else { return };
        t.confirmed = true;
        let (target, combo) = (t.target, t.combo);
        c.confirmed_hits += 1;
        c.last_dealt = damage;
        if !disabled && !follow {
            c.combo_target = target;
            c.combo_count = combo;
            c.combo_last = now;
        }
    });
}
/// gameHitEvent.vrMeleeEnhancedTicket as the blade reads it.
pub(super) struct TicketView {
    pub serial: u64,
    pub attack: Handle,
    pub weapon: Item,
    pub target: u64,
    pub confirmed: bool,
}
/// The enhanced ticket the hit claimed (its instigator's).
pub(super) fn ticket_view(hit: &Handle) -> Option<TicketView> {
    let data = rt::field(hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let pl = instigator_player(&data);
    if pl.is_null() {
        return None;
    }
    let key = hit.instance as usize;
    with(&pl, |c| {
        let serial = *c.hit_ticket.get(&key)?;
        let t = c.enhanced.iter().find(|t| t.serial == serial)?;
        Some(TicketView { serial, attack: t.attack.clone(), weapon: t.weapon, target: t.target, confirmed: t.confirmed })
    })
}
fn instigator_player(data: &Handle) -> Handle {
    let i = r::o(data, "GetInstigator", &[]);
    if r::is_a(&i, "PlayerPuppet") {
        i
    } else {
        Handle::null()
    }
}
/// DamageSystem.CalculateDamageVariants (before): the player's melee hit enhanced.
fn calculate_damage(call: &mut Call) {
    let hit = call.arg_handle(0);
    let data = rt::field(&hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let computed = rt::field(&hit, "attackComputed").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if data.is_null() || computed.is_null() || rt::field(&hit, "projectionPipeline").is_some_and(|v| v.bool()) {
        return;
    }
    let pl = instigator_player(&data);
    if !pl.is_null() && is_melee_attack(&data) {
        apply_enhanced(&pl, &hit);
    }
}
/// DamageSystem.SendDamageEvents (before): the health the enhanced hit took confirms it.
fn send_damage(call: &mut Call) {
    let hit = call.arg_handle(0);
    let key = hit.instance as usize;
    let data = rt::field(&hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let pl = instigator_player(&data);
    if pl.is_null() || !with(&pl, |c| c.hit_ticket.contains_key(&key)) {
        return;
    }
    // resourcesLost: a script_ref to an array of SDamageDealt
    let Some((p, ty)) = call.arg(1) else { return };
    let (arr, arr_ty) = if ty.name().text().starts_with("script_ref:") {
        // SAFETY: a ScriptRef's inner type and value pointer.
        unsafe {
            let w = p as *const usize;
            (*w.add(3) as *mut core::ffi::c_void, (*w.add(2) as *const red4ext_abi::rtti::IType).as_ref())
        }
    } else {
        (p, Some(ty))
    };
    let Some(arr_ty) = arr_ty else { return };
    let health = rt::enum_value("gamedataStatPoolType", "Health");
    let mut damage = 0.0;
    for e in r::items(&Val { ptr: arr, ty: arr_ty }) {
        if e.f("affectedStatPool").and_then(|v| v.num()) == Some(health) {
            damage += e.f("value").and_then(|v| v.num()).unwrap_or(0.0);
        }
    }
    confirm_enhanced(&pl, &hit, damage);
}
/// DamageSystem.ModifyHitData (after): a melee blow the player deflected (not while blocking) counts toward the
/// deflect tutorial.
fn deflect_tutorial(call: &mut Call) {
    let hit = call.arg_handle(0);
    let data = rt::field(&hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if rt::field(&hit, "projectionPipeline").is_some_and(|v| v.bool()) || data.is_null() || !r::b(&data, "WasDeflected", &[]) || !is_melee_attack(&data) {
        return;
    }
    let pl = rt::field(&hit, "target").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if !r::is_a(&pl, "PlayerPuppet") {
        return;
    }
    if let Ok(defs) = super::boards::defs() {
        if let Ok(def) = super::boards::definition(&defs, "PlayerStateMachine") {
            if let (Ok(board), Ok(id)) = (super::boards::local_board(entity_id(&pl), def), super::boards::field(def, "Melee")) {
                if !board.is_null() && rt::num(&board, "GetInt", &[A::P(id)]) == Some(2.0) {
                    return;
                }
            }
        }
    }
    let quests = super::game_system("GetQuestsSystem");
    if rt::num(&quests, "GetFact", &[n("disable_tutorials")]) == Some(0.0) {
        let count = rt::num(&quests, "GetFact", &[n("melee_deflect_tutorial")]).unwrap_or(0.0);
        rt::call(&quests, "SetFact", &[n("melee_deflect_tutorial"), A::N(count + 1.0)]);
    }
}
/// ScriptedPuppet.OnDamageReceived (after): an NPC's damage counted, the player's own swing's hit confirmed.
fn damage_received(call: &mut Call) {
    let npc = call.this();
    let ev = call.arg_handle(0);
    let total = rt::field(&ev, "totalDamageReceived").and_then(|v| v.num()).unwrap_or(0.0);
    let hit = rt::field(&ev, "hitEvent").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let data = rt::field(&hit, "attackData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if r::b(&npc, "IsPlayer", &[]) || total <= 0.0 || data.is_null() {
        return;
    }
    let pl = super::belt::player();
    if pl.is_null() {
        return;
    }
    with(&pl, |c| c.damage_events += 1);
    let instigator = r::o(&data, "GetInstigator", &[]);
    if !instigator.is_null() && entity_id(&instigator) == entity_id(&pl) {
        confirm_hit(&pl, &data, &npc, total);
    }
}
/// VRCyberwareConfirmHit: the hit matched to a swing's attack on its intended target, counted once a target.
fn confirm_hit(pl: &Handle, data: &Handle, receiver: &Handle, damage: f64) {
    let weapon = r::o(data, "GetWeapon", &[]);
    let definition = r::o(data, "GetAttackDefinition", &[]);
    if weapon.is_null() || definition.is_null() || !is_melee_attack(data) {
        if std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1") {
            cvr_host::log!(
                "[melee-trace] the player's hit is not a melee attack (weapon {}, definition {}, melee {})\n",
                !weapon.is_null(),
                !definition.is_null(),
                is_melee_attack(data)
            );
        }
        return;
    }
    let (now, rid, wid, tid) = (sim_time(), record_id(&definition), item_id(&weapon)[0], entity_id(receiver));
    let candidates: Vec<(usize, Handle)> = with(pl, |c| {
        c.tickets
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, t)| t.intended == tid && t.weapon[0] == wid && now >= t.time && now - t.time < 0.35 && !t.targets.contains(&tid))
            .map(|(i, t)| (i, t.attack.clone()))
            .collect()
    });
    let trace = std::env::var("CVR_MELEE_TRACE").is_ok_and(|v| v == "1");
    let open = candidates.len();
    let Some((i, attack)) = candidates.into_iter().find(|(_, a)| super::blade::same_attack(a, &definition) && record_id(a) == rid) else {
        if trace {
            cvr_host::log!("[melee-trace] a melee hit on the NPC not matched to a swing ({open} open ticket(s) for it)\n");
        }
        return;
    };
    with(pl, |c| {
        if let Some(t) = c.tickets.get_mut(i) {
            t.targets.push(tid);
            c.hit_count += 1;
            c.last_native_damage = damage;
            c.last_hit_delay = now - t.time;
        }
    });
    let impact = super::blade::attack_has_impact(&attack);
    if trace {
        cvr_host::log!("[melee-trace] hit confirmed: {damage:.1} damage, {}\n", if impact { "a blade impact (the haptic pulse and the sound)" } else { "the sound" });
    }
    if impact {
        native::<()>("VRHapticEvent", &[Value::Int32(14), Value::Int32(guard_hands(pl))]);
    }
    // The hit's sound for every confirmed stroke, a blade's physical contact as well: main gave a blade impact only the
    // haptic pulse, so a katana's hits were silent while a fist's were heard.
    let played = rt::static_call("GameObject", "PlaySoundEvent", &[A::H(receiver), n("w_feedback_hit_npc")]).is_some();
    if trace {
        cvr_host::log!("[melee-trace] GameObject.PlaySoundEvent(w_feedback_hit_npc) sent (a result value: {played})\n");
    }
}

// ---------------------------------------------------------------- sounds
/// PlayerPuppet.VRMeleeWhoosh: the audio configs' whoosh (and its detail layer) by the weapon family, fast or not.
pub(super) fn whoosh(weapon: &Handle, fast: bool, strong: bool) {
    let t = weapon_type(weapon);
    let pick = |f: &'static str, s: &'static str| if fast { f } else { s };
    let (a, b): (&str, &str) = match t.as_str() {
        "Wea_Katana" | "Wea_Sword" | "Wea_LongBlade" => ("w_melee_katana_whoosh", ""),
        "Wea_ShortBlade" | "Wea_Knife" | "Wea_Machete" => (pick("w_melee_blade_whoosh_fast", "w_melee_blade_whoosh_normal"), "w_melee_detail_knife_ring"),
        "Wea_Axe" => (pick("w_melee_blade_whoosh_fast", "w_melee_blade_whoosh_normal"), "w_melee_detail_tomahawk"),
        "Wea_Chainsword" => (pick("w_melee_blade_whoosh_fast", "w_melee_blade_whoosh_normal"), pick("w_melee_cut_o_matic_whoosh_fast", "w_melee_cut_o_matic_whoosh")),
        "Wea_Hammer" => (pick("w_melee_sledgehammer_whoosh_fast", "w_melee_sledgehammer_whoosh"), pick("w_melee_sledgehammer_detail_ring", "w_melee_sledgehammer_detail_ring_sharp")),
        "Wea_OneHandedClub" | "Wea_TwoHandedClub" | "Wea_Melee" => (if strong { "w_melee_blunt_whoosh_heavy" } else { pick("w_melee_blunt_whoosh_fast", "w_melee_blunt_whoosh_low") }, ""),
        "Cyb_MantisBlades" => (if strong { "w_cyb_mantis_whoosh_heavy" } else { pick("w_cyb_mantis_whoosh_fast", "w_cyb_mantis_whoosh") }, "w_cyb_mantis_whoosh_ring"),
        "Cyb_NanoWires" => (pick("w_cyb_nano_wire_whoosh_swirl", "w_cyb_nano_wire_whoosh"), ""),
        "" => return,
        _ => (pick("w_melee_fist_whoosh_fast", "w_melee_fist_whoosh_heavy"), ""),
    };
    for e in [a, b] {
        if !e.is_empty() {
            rt::static_call("GameObject", "PlaySoundEvent", &[A::H(weapon), n(e)]);
        }
    }
}
/// The weapon record's item type's name ("" without a record).
fn weapon_type(weapon: &Handle) -> String {
    if weapon.is_null() {
        return String::new();
    }
    let rec = r::o(weapon, "GetWeaponRecord", &[]);
    if rec.is_null() {
        return String::new();
    }
    let t = rt::num(&r::o(&rec, "ItemType", &[]), "Type", &[]).unwrap_or(-1.0);
    ITEM_TYPES.iter().find(|n| rt::enum_value("gamedataItemType", n) == t).map_or_else(|| "?".to_string(), |s| (*s).to_string())
}
const ITEM_TYPES: [&str; 26] = [
    "Wea_Katana", "Wea_Sword", "Wea_LongBlade", "Wea_ShortBlade", "Wea_Knife", "Wea_Machete", "Wea_Axe", "Wea_Chainsword", "Wea_Hammer",
    "Wea_OneHandedClub", "Wea_TwoHandedClub", "Wea_Melee", "Cyb_MantisBlades", "Cyb_NanoWires", "Cyb_StrongArms", "Wea_Fists", "Wea_Handgun",
    "Wea_Revolver", "Wea_SubmachineGun", "Wea_SniperRifle", "Wea_LightMachineGun", "Wea_HeavyMachineGun", "Wea_Rifle", "Wea_AssaultRifle",
    "Wea_Shotgun", "Wea_ShotgunDual",
];
const GUN_EQUIPS: [&str; 53] = [
    "w_gun_pistol_power_lexington_equip", "w_gun_pistol_power_liberty_equip", "w_gun_pistol_power_unity_equip", "w_gun_pistol_power_nue_equip",
    "w_gun_pistol_power_slaughtomatic_equip", "w_gun_pistol_power_silverhand_equip", "w_gun_pistol_tech_omaha_equip", "w_gun_pistol_tech_kenshin_equip",
    "w_gun_grit_equip", "w_gun_ticon_equip", "w_gun_pistol_toygun_equip", "w_gun_pistol_smart_chao_equip", "w_gun_pistol_smart_kappa_equip",
    "w_gun_pistol_smart_yukimura_equip", "w_gun_revol_power_overture_equip", "w_gun_revol_power_crusher_equip", "w_gun_revol_power_nova_equip",
    "w_gun_metel_equip", "w_gun_revol_tech_burya_equip", "w_gun_revol_tech_quasar_equip", "w_gun_smg_power_saratoga_equip",
    "w_gun_smg_power_guillotine_equip", "w_gun_smg_power_pulsar_equip", "w_gun_borg4a_equip", "w_gun_warden_equip", "w_gun_smg_tech_senkoh_equip",
    "w_gun_smg_smart_shingen_equip", "w_gun_smg_smart_sidewinder_equip", "w_gun_smg_smart_dian_equip", "w_gun_rifle_power_ajax_equip",
    "w_gun_rifle_power_copperhead_equip", "w_gun_rifle_power_masamune_equip", "w_gun_rifle_power_nowaki_equip", "w_gun_rifle_power_pozhar_equip",
    "w_gun_rifle_power_sor22_equip", "w_gun_rifle_power_kolac_equip", "w_gun_rifle_tech_achilles_equip", "w_gun_shotgun_power_carnage_equip",
    "w_gun_shotgun_power_tactician_equip", "w_gun_shotgun_power_igla_equip", "w_gun_shotgun_power_testera_equip", "w_gun_shotgun_tech_satara_equip",
    "w_gun_shotgun_smart_zhuo_equip", "w_gun_shotgun_smart_palica_equip", "w_gun_sniper_power_grad_equip", "w_gun_osprey_equip",
    "w_gun_sniper_tech_rasetsu_equip", "w_gun_sniper_tech_nekomata_equip", "w_gun_sniper_smart_ashura_equip", "w_gun_lmg_power_ma70_equip",
    "w_gun_lmg_power_defender_equip", "w_gun_hmg_militech_equip", "w_gun_hercules3ax_equip",
];
/// PlayerPuppet.VREquipSound: the draw sound -- melee by family, guns by the record's name token, else by weight.
pub(super) fn equip_sound(weapon: &Handle) {
    let t = weapon_type(weapon);
    if t.is_empty() {
        return;
    }
    let (a, b): (&str, &str) = match t.as_str() {
        "Wea_Katana" | "Wea_Sword" | "Wea_LongBlade" => ("w_melee_katana_equip", ""),
        "Wea_ShortBlade" | "Wea_Knife" => ("w_melee_knife_additional_equip", "w_melee_detail_knife_ring"),
        "Wea_Machete" => ("w_melee_knife_additional_equip", ""),
        "Wea_Axe" => ("w_melee_detail_tomahawk", "w_melee_knife_additional_equip"),
        "Wea_Chainsword" => ("w_melee_cut_o_matic_equip", ""),
        "Wea_Hammer" => return,
        "Wea_OneHandedClub" | "Wea_TwoHandedClub" | "Wea_Melee" => ("w_melee_detail_bat_equip", ""),
        "Wea_Fists" => ("lcm_mvs_leather_jacket_fast_light", ""),
        "Cyb_MantisBlades" => ("w_cyb_mantis_equip", ""),
        "Cyb_StrongArms" => ("w_cyb_strongarms_equip", ""),
        "Cyb_NanoWires" => ("w_cyb_nano_wire_out", ""),
        _ => ("", ""),
    };
    let play = |e: &str| {
        rt::static_call("GameObject", "PlaySoundEvent", &[A::H(weapon), n(e)]);
    };
    if !a.is_empty() || !b.is_empty() {
        for e in [a, b] {
            if !e.is_empty() {
                play(e);
            }
        }
        return;
    }
    // guns: the record name's token ("Items.Preset_Lexington_Wilson" -> "lexington") against the banks' equips
    let rid = item_id(weapon)[0];
    let text = rt::static_call("TDBID", "ToStringDEBUG", &[A::Name(rid)]).map(|o| Val::of(&o).text()).unwrap_or_default().to_lowercase();
    if let Some(token) = text.split('_').nth(1) {
        let want = format!("_{token}");
        if let Some(e) = GUN_EQUIPS.iter().find(|e| e.contains(&want)) {
            play(e);
            return;
        }
    }
    play(match t.as_str() {
        "Wea_Handgun" | "Wea_Revolver" | "Wea_SubmachineGun" => "w_gun_npc_equip_light",
        "Wea_SniperRifle" | "Wea_LightMachineGun" | "Wea_HeavyMachineGun" => "w_gun_npc_equip_heavy",
        _ => "w_gun_npc_equip_medium",
    });
}

// ---------------------------------------------------------------- the machine's guards
/// MeleeTransition::WantsToLightAttack / WantsToStrongAttack (scripted statics): refused for the player while physical
/// melee is on (the swing is the attack).
fn wants_attack(call: &mut Call) {
    let si = call.arg_handle(1);
    let owner = rt::field(&si, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if r::is_a(&owner, "PlayerPuppet") && native::<bool>("VRPhysicalMelee", &[]) == Some(true) {
        call.set_result_bool(false);
    }
}
/// MeleePublicSafeEvents.OnTick (before): no idle auto-unequip of a melee weapon.
fn no_idle_unequip(call: &mut Call) {
    r::hset(&call.this(), "m_unequipTime", -1.0);
}

/// The hooks on members, in reds_hooks' table.
pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 5] = [
    ("DamageSystem", "CalculateDamageVariants", Kind::Before, calculate_damage),
    ("DamageSystem", "SendDamageEvents", Kind::Before, send_damage),
    ("DamageSystem", "ModifyHitData", Kind::After, deflect_tutorial),
    ("ScriptedPuppet", "OnDamageReceived", Kind::After, damage_received),
    ("MeleePublicSafeEvents", "OnTick", Kind::Before, no_idle_unequip),
];
/// The hooks on scripted statics.
pub(super) const STATIC_HOOKS: [(&str, &str, Kind, HookFn); 2] =
    [("MeleeTransition", "WantsToLightAttack", Kind::Override, wants_attack), ("MeleeTransition", "WantsToStrongAttack", Kind::Override, wants_attack)];

/// A development check (CVR_MELEE_TEST, inherited by the game from the probe session): 45 s into the loaded save the
/// first melee weapon is drawn; at 52 s the weapon's kind, reach and blade, a stroke begun and the blade swept across
/// the player's front are logged, and the weapon put away.
pub(super) fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    if std::env::var_os("CVR_MELEE_TEST").is_none() {
        return;
    }
    let pl = super::belt::player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 3 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first)));
        return;
    }
    let t = t + dt;
    let mut next = step;
    if step == 0 && t > 45.0 {
        super::holster::equip_request(&pl, "RequestFirstMeleeWeapon");
        cvr_host::log!("[melee] self-test: the first melee weapon requested (from Rust)\n");
        next = 1;
    } else if step == 1 && t > 52.0 {
        let w = held_item(&pl);
        let is_weapon = r::is_a(&w, "WeaponObject");
        let d = direction(&w);
        let base = world_pos(&pl).unwrap_or([0.0; 3]);
        let fwd = super::belt::m::<[f32; 4]>(&pl, "GetWorldForward", &[]).map(f3).unwrap_or([0.0, 1.0, 0.0]);
        let start = add(base, [0.0, 0.0, 1.2]);
        let end = add(start, mul(fwd, 1.0));
        let began = is_weapon && begin_swing(&pl, &w, 1, false, 8.0);
        let near_list = npcs(&pl, 30.0);
        let near = near_list.len();
        // through the nearest NPC's chest when there is one (the whole attack path), else across the player's front
        let (s0, e0) = match near_list.first().and_then(world_pos) {
            Some(b) => (add(b, [-0.5, 0.0, 1.1]), add(b, [0.5, 0.0, 1.1])),
            None => (start, end),
        };
        let hits = if began { sweep_hit(&pl, &w, v4(s0, 1.0), v4(e0, 1.0), v4(add(s0, [0.0, 0.3, 0.0]), 1.0), v4(add(e0, [0.0, 0.3, 0.0]), 1.0), 0.08, false, [0.0; 4], false, false, 4.0) } else { 0 };
        if let Some(npc) = near_list.first() {
            let friendly = rt::static_call("GameObject", "IsFriendlyTowardsPlayer", &[A::H(npc)]).is_some_and(|o| Val::of(&o).bool());
            let god = super::game_system("GetGodModeSystem");
            let id = [entity_id(npc), 0u64];
            let mode = |m: &str| rt::call(&god, "HasGodMode", &[A::P(id.as_ptr().cast()), A::N(rt::enum_value("gameGodModeType", m))]).is_some_and(|o| Val::of(&o).bool());
            let class = red4ext_abi::rtti::RttiSystem::get().and(unsafe { red4ext_abi::rtti::object_class(npc.instance) }).map(|c| c.class_name().text().to_string()).unwrap_or_default();
            cvr_host::log!(
                "[melee] self-test: the NPC is a {class}, friendly {friendly}, invulnerable {}, immortal {} (from Rust)\n",
                mode("Invulnerable"),
                mode("Immortal")
            );
        }
        let (swings, attacks) = with(&pl, |c| (c.swing_count, c.attack_count));
        let throws = super::melee_throw::counts();
        cvr_host::log!(
            "[melee] self-test: weapon {is_weapon} kind {} reach {:.2} blade ({:.2},{:.2},{:.2}) melee attack {} ; stroke begun {began}, hits {hits}, NPCs within 30 m {near}, swings {swings} attacks {attacks}, throws {:?} (from Rust)\n",
            kind(&w), reach(&w), d[0], d[1], d[2], !melee_attack(&w, false).is_null(), throws
        );
        super::holster::equip_request(&pl, "UnequipWeapon");
        next = 2;
    } else if step == 2 && t > 55.0 {
        let c = with(&pl, |c| (c.attack_count, c.hit_count, c.confirmed_hits, c.last_multiplier, c.damage_events, c.last_native_damage));
        cvr_host::log!(
            "[melee] self-test 3 s later: attacks {} hits {} enhanced confirmed {} last multiplier {:.2} NPC damage events {} last native damage {:.1} (from Rust)\n",
            c.0, c.1, c.2, c.3, c.4, c.5
        );
        cvr_host::log!("[melee] self-test done (from Rust)\n");
        next = 3;
    }
    T.with(|c| c.set((t, next, first)));
}
