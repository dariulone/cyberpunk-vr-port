//! The installed parts' factor on a gun's VR hand recoil (main's mods/cet/CyberpunkVRPort_Weapon/attachment_recoil.lua,
//! 41a2d9082): the hand spring models the peak kick, so the parts that change RecoilKickMax -- a brake, a compensator --
//! scale its pose (SetVRAttachmentRecoil; cvr-rig recoil.rs applies it after the caps). RecoilKickMin, the ADS
//! variants and the tooltip-only RecoilKickReduction describe the same parts and are not read.
//!
//! A part's rules are its record's RecoilKickMax modifiers -- constant ones, and combined ones "Self * Quality" scaled by
//! the part's quality -- and those of its unconditional modifier groups (a randomized or limited group only describes
//! candidates). The scale: the multipliers' product, times the additive and additive-multiplier buckets each against
//! the weapon record's own baseline (a plain multiplier cancels out of that ratio), clamped to 0..1. Each hand (1 the
//! weapon in hand, 0 the left gun) is read again every half second and on a part's change.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::Owned;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug)]
struct Rule {
    /// 0 Additive, 1 AdditiveMultiplier, 2 Multiplier.
    op: u8,
    value: f64,
    quality: bool,
}
#[derive(Clone, Copy)]
struct Totals {
    add: f64,
    relative: f64,
    mul: f64,
}
impl Totals {
    fn new() -> Self {
        Totals { add: 0.0, relative: 0.0, mul: 1.0 }
    }
    fn apply(&mut self, rules: &[Rule], quality: f64) {
        for r in rules {
            let v = if r.quality { r.value * quality } else { r.value };
            match r.op {
                1 => self.relative += v,
                2 => self.mul *= v,
                _ => self.add += v,
            }
        }
    }
}
#[derive(Default)]
struct Hand {
    wid: Option<u64>,
    scale: Option<f64>,
    due: f64,
    error: Option<String>,
}
struct State {
    rules: HashMap<u64, Vec<Rule>>,
    hands: [Hand; 2],
}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
fn ev(e: &str, m: &str) -> f64 {
    rt::enum_value(e, m)
}
fn cname_of(o: Option<Owned>) -> u64 {
    // SAFETY: a CName result.
    o.map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
fn tdbid_of(o: &Owned) -> u64 {
    // SAFETY: a TweakDBID result.
    unsafe { *(o.as_ptr() as *const u64) }
}

/// A record's RecoilKickMax rules (cached by the record's id).
fn rules_for(rules: &mut HashMap<u64, Vec<Rule>>, record: &Handle) -> Vec<Rule> {
    let Some(key) = rt::call(record, "GetID", &[]).map(|o| tdbid_of(&o)) else { return Vec::new() };
    if let Some(r) = rules.get(&key) {
        return r.clone();
    }
    let kick_max = ev("gamedataStatType", "RecoilKickMax");
    let quality_stat = ev("gamedataStatType", "Quality");
    let mut out: Vec<Rule> = Vec::new();
    let mut seen: Vec<u64> = Vec::new();
    let mut collect = |source: &Handle| {
        // StatModifiers(out list): an out array, not a result (asked as a result it was never found: no rule at all)
        for m in rt::objs_out(source, "StatModifiers") {
            let stat = rt::obj(&m, "StatType", &[]);
            if rt::num(&stat, "StatType", &[]) != Some(kick_max) {
                continue;
            }
            let id = rt::call(&m, "GetID", &[]).map_or(0, |o| tdbid_of(&o));
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let constant = super::menu_rt::b(&m, "IsA", &[n("gamedataConstantStatModifier_Record")]);
            let quality = !constant
                && super::menu_rt::b(&m, "IsA", &[n("gamedataCombinedStatModifier_Record")])
                && cname_of(rt::call(&m, "RefObject", &[])) == CName::hashed("Self").0
                && cname_of(rt::call(&m, "OpSymbol", &[])) == CName::hashed("*").0
                && rt::num(&rt::obj(&m, "RefStat", &[]), "StatType", &[]) == Some(quality_stat);
            if constant || quality {
                let op_name = cname_of(rt::call(&m, "ModifierType", &[]));
                let op = if op_name == CName::hashed("AdditiveMultiplier").0 {
                    1
                } else if op_name == CName::hashed("Multiplier").0 {
                    2
                } else if op_name == CName::hashed("Additive").0 {
                    0
                } else {
                    continue;
                };
                out.push(Rule { op, value: rt::num(&m, "Value", &[]).unwrap_or(0.0), quality });
            }
        }
    };
    collect(record);
    for group in rt::objs_out(record, "StatModifierGroups") {
        // Randomized or limited groups describe candidates: only unconditional rows are known active from records alone.
        let limit = rt::num(&group, "StatModsLimit", &[]).unwrap_or(0.0);
        if limit < 0.0 && rt::obj(&group, "StatModsLimitModifier", &[]).is_null() {
            collect(&group);
        }
    }
    rules.insert(key, out.clone());
    out
}

/// The gun's installed parts' scale (1 for a melee weapon or no parts).
fn read_scale(s: &mut State, player: &Handle, gun: &Handle) -> Result<f64, String> {
    let Some(weapon_id) = rt::call(gun, "GetItemID", &[]) else { return Err("no item id".into()) };
    if !rt::static_call("WeaponObject", "IsRanged", &[A::P(weapon_id.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool()) {
        return Ok(1.0);
    }
    let ts = super::game_system("GetTransactionSystem");
    let data = rt::obj(&ts, "GetItemData", &[A::H(player), A::P(weapon_id.as_ptr().cast())]);
    if data.is_null() {
        return Err("weapon item data unavailable".into());
    }
    let quality_stat = ev("gamedataStatType", "Quality");
    let mut total = Totals::new();
    let slots: Vec<u64> = rt::method_outs(&data, "GetUsedSlotsOnItem", &[], 1)
        .and_then(|(_, outs)| outs.into_iter().next())
        .map(|arr| {
            let v = Val::of(&arr);
            (0..v.len()).filter_map(|i| v.at(i)).map(|e| {
                // SAFETY: a TweakDBID element.
                unsafe { *(e.ptr as *const u64) }
            }).collect()
        })
        .unwrap_or_default();
    let mut seen: Vec<u64> = Vec::new();
    for slot in slots {
        if seen.contains(&slot) {
            continue;
        }
        seen.push(slot);
        let Some((_, outs)) = rt::method_outs(&data, "GetItemPart", &[A::Name(slot)], 1) else { continue };
        let Some(part) = outs.into_iter().next() else { continue };
        let part_ptr = part.as_ptr();
        let Some(id) = rt::static_call("InnerItemData", "GetItemID", &[A::P(part_ptr.cast())]) else { continue };
        let valid = rt::static_call("ItemID", "IsValid", &[A::P(id.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool());
        let is_part = rt::static_call("InventoryUtils", "IsPart", &[A::Name(slot)]).is_some_and(|o| Val::of(&o).bool())
            || rt::static_call("RPGManager", "IsWeaponMod", &[A::P(id.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool());
        if !valid || !is_part {
            continue;
        }
        let record = rt::static_call("InnerItemData", "GetStaticData", &[A::P(part_ptr.cast())]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        if record.is_null() || super::menu_rt::b(&record, "TagsContains", &[n("DummyPart")]) {
            continue;
        }
        let rules = rules_for(&mut s.rules, &record);
        if !rules.is_empty() {
            let quality = rt::static_num("InnerItemData", "GetStatValueByType", &[A::P(part_ptr.cast()), A::N(quality_stat)]).unwrap_or(0.0);
            total.apply(&rules, quality);
        }
    }
    let mut scale = total.mul;
    if total.add != 0.0 || total.relative != 0.0 {
        // The game's additive and additive-multiplier buckets each have an unmodified baseline: the weapon record's.
        let mut base = Totals::new();
        let tdbid = rt::static_call("ItemID", "GetTDBID", &[A::P(weapon_id.as_ptr().cast())]).map_or(0, |o| tdbid_of(&o));
        let record = rt::static_call("TweakDBInterface", "GetItemRecord", &[A::Name(tdbid)]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        if !record.is_null() {
            let rules = rules_for(&mut s.rules, &record);
            base.apply(&rules, rt::num(&data, "GetStatValueByType", &[A::N(quality_stat)]).unwrap_or(0.0));
        }
        if base.add > 0.0 {
            scale *= (1.0 + total.add / base.add).max(0.0);
        }
        let relative_base = 1.0 + base.relative;
        if relative_base > 0.0 {
            scale *= 1.0 + total.relative / relative_base;
        }
    }
    Ok(scale.clamp(0.0, 1.0))
}

/// M.invalidate: a part added or removed -- both hands read again at their next update.
pub(in crate::mechanics) fn invalidate() {
    if let Some(s) = STATE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        s.hands[0].due = 0.0;
        s.hands[1].due = 0.0;
    }
}
/// M.reset: no player (or the mod's shutdown) -- both hands back to the unscaled recoil.
pub(in crate::mechanics) fn reset() {
    update(0, &Handle::null(), None, None, 0.0);
    update(1, &Handle::null(), None, None, 0.0);
}
/// M.update: one hand's gun (`side` 1 the weapon in hand, 0 the left gun; None for none).
pub(in crate::mechanics) fn update(side: usize, player: &Handle, gun: Option<&Handle>, wid: Option<u64>, dt: f64) {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.get_or_insert_with(|| State { rules: HashMap::new(), hands: [Hand::default(), Hand::default()] });
    let gun = gun.filter(|h| !h.is_null());
    {
        let hand = &mut s.hands[side];
        if hand.wid == wid && hand.scale.is_some() {
            if gun.is_none() {
                return;
            }
            hand.due -= dt;
            if hand.due > 0.0 {
                return;
            }
        }
    }
    // Equip events invalidate on the next update; changes made directly through the TransactionSystem are reconciled
    // here, and an unavailable item retried.
    let (scale, failure) = match gun {
        Some(gun) => match read_scale(s, player, gun) {
            Ok(v) => (v, None),
            Err(e) => (1.0, Some(e)),
        },
        None => (1.0, None),
    };
    let hand = &mut s.hands[side];
    if Some(scale) != hand.scale {
        super::belt::native::<()>("SetVRAttachmentRecoil", &[crate::script_call::Value::Int32(side as i32), crate::script_call::Value::Float(scale as f32)]);
        if (scale - 1.0).abs() > 1e-6 {
            cvr_host::log!("[CyberpunkVRPort_Weapon] attachment recoil: hand {side} x{scale:.3} (from Rust)\n");
        }
    }
    hand.wid = wid;
    hand.scale = Some(scale);
    hand.error = failure;
    hand.due = 0.5;
}
