//! mods/redscript/CyberpunkVRPort_LeftHanded: with the left-handed controls (xr_left_handed) a weapon drawn on its
//! own is the left hand's.
//!
//! The one decision -- which hand holds what after a request (EquipmentSystemPlayerData's
//! GetItemIDfromEquipmentManipulationAction) -- gives a weapon alone in the right hand to the left, and everything
//! downstream is the game's own left-hand path; HandleEquipWeapon (a weapon equipped straight into the hand) sends it
//! there too, and the weapon in the left hand equipped again where it is is answered as done. The game's rules that
//! assume a right-hand weapon beside a left one answer here for the left hand while it holds the only weapon: the
//! temporary equip over it, the weapon to re-request or put away, the restored hands of a save, the equip cycle's
//! dual-wield marks and the left hand's own single-wield animations (the plugin's VRLeftHandedAnimations), the left
//! hand equipping with the right one empty (a melee weapon too), the upper body's single wield and empty hands, the
//! exploration hints and the weapon's input hints, the HUD's weapon data (rightWeapon), and the melee machine's
//! questions of the right hand. DefaultTransition.HasSingleWeaponEquipped, a scripted static, is answered through its
//! six callers in the upper body's decisions. The weapon mod's rehand / redraw / restore are [`rehand`], [`redraw`] and
//! [`draw_restored`].
//!
//! The script class VRLeftHandedWeapon (CyberpunkVRPort_Engine's vrport_left_handed_shared.reds, compiled only in the
//! C++ build) is these functions here, its answers cached by record.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::Owned;
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::sync::Mutex;

type Item = [u64; 2];
const NONE: Item = [0, 0];

fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
fn tdb(s: &str) -> u64 {
    red4ext_abi::tweakdbid::from_name(s)
}
fn ev(en: &str, m: &str) -> A<'static> {
    A::N(rt::enum_value(en, m))
}
fn native_bool(name: &str) -> bool {
    super::belt::native::<bool>(name, &[]) == Some(true)
}
/// VRLeftHanded(): the controls are left-handed.
pub(super) fn left_handed() -> bool {
    native_bool("VRLeftHanded")
}
fn item_of(o: &Owned) -> Item {
    // SAFETY: an ItemID result (16 bytes).
    unsafe { *(o.as_ptr() as *const Item) }
}
fn item_at(p: *mut core::ffi::c_void) -> Item {
    // SAFETY: an ItemID value in place.
    unsafe { *(p as *const Item) }
}
fn valid(i: &Item) -> bool {
    i[0] != 0
}
/// ItemID equality: the record, the seed and the counter (the bytes before the structure and the flags).
fn same(a: &Item, b: &Item) -> bool {
    a[0] == b[0] && (a[1] & 0xFFFF_FFFF_FFFF) == (b[1] & 0xFFFF_FFFF_FFFF)
}
fn ip(i: &Item) -> A<'_> {
    A::P(i.as_ptr().cast())
}
fn owner(si: &Handle) -> Handle {
    rt::field(si, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn flag(o: &Handle, f: &str, args: &[A]) -> bool {
    super::menu_rt::b(o, f, args)
}
fn st_flag(class: &str, f: &str, args: &[A]) -> bool {
    rt::static_call(class, f, args).is_some_and(|o| Val::of(&o).bool())
}

/// The record's answers, cached: (a weapon, a weapon that may hang on WeaponLeft).
static RECORDS: Mutex<Option<HashMap<u64, (bool, bool)>>> = Mutex::new(None);
fn record_facts(i: &Item) -> (bool, bool) {
    if let Some(f) = RECORDS.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|m| m.get(&i[0]).copied()) {
        return f;
    }
    let record = rt::static_obj("TweakDBInterface", "GetItemRecord", &[A::Name(i[0])]);
    let category = rt::obj(&record, "ItemCategory", &[]);
    let weapon = !record.is_null() && rt::num(&category, "Type", &[]) == Some(rt::enum_value("gamedataItemCategory", "Weapon"));
    let mut left_slot = false;
    if weapon {
        if let Some((_, outs)) = rt::call_outs(&record, "PlacementSlots", &[], 1) {
            if let Some(slots) = outs.first() {
                let v = Val::of(slots);
                let want = tdb("AttachmentSlots.WeaponLeft");
                left_slot = (0..v.len()).filter_map(|k| v.at(k)).any(|s| {
                    rt::call(&s.handle(), "GetID", &[]).is_some_and(|o| {
                        // SAFETY: a TweakDBID result.
                        unsafe { *(o.as_ptr() as *const u64) == want }
                    })
                });
            }
        }
    }
    let f = (weapon, left_slot);
    let mut g = RECORDS.lock().unwrap_or_else(|p| p.into_inner());
    g.get_or_insert_with(HashMap::new).insert(i[0], f);
    f
}
/// VRLeftHandedWeapon.IsWeapon.
fn is_weapon(i: &Item) -> bool {
    valid(i) && record_facts(i).0
}
/// VRLeftHandedWeapon.Movable: a hand-held weapon that may hang on the left slot (no implant, no fists, no heavy).
fn movable(i: &Item) -> bool {
    if !valid(i) || st_flag("WeaponObject", "IsCyberwareWeapon", &[ip(i)]) || st_flag("WeaponObject", "IsFists", &[ip(i)]) {
        return false;
    }
    let (weapon, left_slot) = record_facts(i);
    if !weapon {
        return false;
    }
    let area = rt::static_num("EquipmentSystem", "GetEquipAreaType", &[ip(i)]);
    area != Some(rt::enum_value("gamedataEquipmentArea", "WeaponHeavy")) && left_slot
}
fn slot_item(owner: &Handle, left: bool) -> Item {
    rt::static_call("EquipmentSystem", "GetSlotActiveItem", &[A::H(owner), ev("EquipmentManipulationRequestSlot", if left { "Left" } else { "Right" })])
        .map_or(NONE, |o| item_of(&o))
}
/// VRLeftHandedWeapon.Alone: the left hand holds the only weapon, as the equipment system decided.
fn alone(owner: &Handle) -> bool {
    movable(&slot_item(owner, true)) && !is_weapon(&slot_item(owner, false))
}
fn weapon_in(owner: &Handle, slot: &str) -> Handle {
    let ts = super::belt::transactions();
    let w = rt::obj(&ts, "GetItemInSlot", &[A::H(owner), A::Name(tdb(slot))]);
    if super::menu_rt::is_a(&w, "WeaponObject") {
        w
    } else {
        Handle::null()
    }
}
/// VRLeftHandedWeapon.InHand: the weapon in hand is the left one (WeaponLeft holds a weapon, WeaponRight none).
pub(super) fn in_hand(owner: &Handle) -> bool {
    if owner.is_null() || !left_handed() {
        return false;
    }
    let left = weapon_in(owner, "AttachmentSlots.WeaponLeft");
    !left.is_null()
        && weapon_in(owner, "AttachmentSlots.WeaponRight").is_null()
        && rt::call(&left, "GetItemID", &[]).is_some_and(|o| movable(&item_of(&o)))
}
/// VRLeftHandedWeapon.HeldSlot.
pub(super) fn held_slot(owner: &Handle) -> u64 {
    tdb(if in_hand(owner) { "AttachmentSlots.WeaponLeft" } else { "AttachmentSlots.WeaponRight" })
}
/// VRLeftHandedWeapon.SetAnimations: the single wield on the left hand on while the weapon is alone there.
fn set_animations(owner: &Handle) {
    let on = left_handed() && alone(owner);
    rt::static_call("AnimationControllerComponent", "SetAnimWrapperWeight", &[A::H(owner), n("vr_left_handed"), A::N(if on { 1.0 } else { 0.0 })]);
    if !super::menu_rt::is_a(owner, "ScriptedPuppet") {
        return;
    }
    let controller = rt::obj(owner, "GetAnimationControllerComponent", &[]);
    let name = |path: &str| {
        rt::static_call("TweakDBInterface", "GetCName", &[A::Name(tdb(path)), n("None")]).map_or(CName::hashed("None").0, |o| {
            // SAFETY: a CName result.
            unsafe { *(o.as_ptr() as *const u64) }
        })
    };
    let graph = name("playerStateMachineDefinitions.WeaponRight.animGraphName");
    let slot = name("playerStateMachineDefinitions.WeaponRight.animGraphSlot");
    rt::global_call("VRLeftHandedAnimations", &[A::H(&controller), A::B(on), A::Name(graph), A::Name(slot)]);
}
/// A StateMachineIdentifier { definitionName, referenceName }, made by its own type and filled by field name.
fn sm_id(definition: &str, reference: &str) -> Option<Owned> {
    let rtti = red4ext_abi::rtti::RttiSystem::get()?;
    let ty = ["gamestateMachineStateMachineIdentifier", "StateMachineIdentifier"].iter().find_map(|t| rtti.get_type(CName::hashed(t)))?;
    let mut o = Owned::of_type(ty)?;
    // SAFETY: the struct's own properties in its constructed value.
    unsafe {
        let class = &*(ty as *const red4ext_abi::rtti::IType as *const red4ext_abi::rtti::Class);
        let base = o.as_mut_ptr().cast::<core::ffi::c_void>();
        for (f, v) in [("definitionName", definition), ("referenceName", reference)] {
            *(class.property_named(f)?.value_ptr::<core::ffi::c_void>(base) as *mut u64) = CName::hashed(v).0;
        }
    }
    Some(o)
}
fn psm_set_bool(si: &Handle, field: &str, v: bool) {
    let bb = rt::field(si, "localBlackboard").map(|x| x.handle()).unwrap_or_else(Handle::null);
    let Ok(defs) = super::boards::defs() else { return };
    let Ok(def) = super::boards::definition(&defs, "PlayerStateMachine") else { return };
    let Ok(id) = super::boards::field(def, field) else { return };
    rt::call(&bb, "SetBool", &[A::P(id), A::B(v)]);
}
fn psm_get_bool(si: &Handle, field: &str) -> bool {
    let bb = rt::field(si, "localBlackboard").map(|x| x.handle()).unwrap_or_else(Handle::null);
    let (Ok(defs), false) = (super::boards::defs(), bb.is_null()) else { return false };
    let Ok(def) = super::boards::definition(&defs, "PlayerStateMachine") else { return false };
    let Ok(id) = super::boards::field(def, field) else { return false };
    flag(&bb, "GetBool", &[A::P(id)])
}
fn psm_get_int(si: &Handle, field: &str) -> f64 {
    let bb = rt::field(si, "localBlackboard").map(|x| x.handle()).unwrap_or_else(Handle::null);
    let (Ok(defs), false) = (super::boards::defs(), bb.is_null()) else { return 0.0 };
    let Ok(def) = super::boards::definition(&defs, "PlayerStateMachine") else { return 0.0 };
    let Ok(id) = super::boards::field(def, field) else { return 0.0 };
    rt::num(&bb, "GetInt", &[A::P(id)]).unwrap_or(0.0)
}

// ---------------------------------------------------------------- the equipment system
/// The SSlotActiveItems' two hands in place.
fn hands(v: &Val) -> Option<(*mut core::ffi::c_void, *mut core::ffi::c_void)> {
    Some((v.f("rightHandItem")?.ptr, v.f("leftHandItem")?.ptr))
}
/// The hand decision's left-hand part, called by the Cyberware module's hook on GetItemIDfromEquipmentManipulationAction
/// right after the original ran (the LeftHanded wrapper sat inside that module's).
pub(super) fn decide(call: &mut Call) {
    if !left_handed() {
        return;
    }
    let Some((p, ty)) = call.result() else { return };
    let Some((r, l)) = hands(&Val { ptr: p, ty }) else { return };
    let (right, left) = (item_at(r), item_at(l));
    if movable(&right) && !is_weapon(&left) {
        // SAFETY: the result's two ItemIDs in place.
        unsafe {
            *(l as *mut Item) = right;
            *(r as *mut Item) = NONE;
        }
    }
    given_back(call, l);
}
/// The weapon alone in the left hand that a combat gadget or a consumable took the hand from.
static TAKEN_FOR_ITEM: Mutex<Item> = Mutex::new(NONE);
/// THE WEAPON GIVEN BACK AFTER A GADGET OR A HEAL. Either goes to the left hand, and the left hand's weapon makes way
/// for it; put away, the left hand takes what the game pairs with the right one (TryGetSecondaryItemID) -- for a
/// pair its second gun, for the weapon alone in the left hand nothing: the weapon stayed away after every grenade and
/// heal (2026-11-25). The weapon the request took the hand from is the left hand's again when the item is put away --
/// at each put-away request: the gadget's machine sends one as it waits for the unequip and another as it leaves.
fn given_back(call: &Call, l: *mut core::ffi::c_void) {
    let action = call.arg(0).and_then(|(p, ty)| Val { ptr: p, ty }.num()).unwrap_or(-1.0);
    let is = |m: &str| action == rt::enum_value("EquipmentManipulationAction", m);
    let (now_right, now_left) = call.arg(1).and_then(|(p, ty)| hands(&Val { ptr: p, ty })).map_or((NONE, NONE), |(r, l)| (item_at(r), item_at(l)));
    let Some((p, ty)) = call.result() else { return };
    let Some((r, _)) = hands(&Val { ptr: p, ty }) else { return };
    let (right, left) = (item_at(r), item_at(l));
    let mut taken = TAKEN_FOR_ITEM.lock().unwrap_or_else(|e| e.into_inner());
    if is("RequestGadget") || is("RequestConsumable") {
        *taken = if movable(&now_left) && !valid(&now_right) && valid(&left) && !same(&left, &now_left) { now_left } else { NONE };
    } else if is("UnequipGadget") || is("UnequipConsumable") {
        let weapon = *taken;
        if valid(&weapon) && !valid(&right) && !valid(&left) {
            // SAFETY: the result's left ItemID in place.
            unsafe { *(l as *mut Item) = weapon };
            static SAID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
            if SAID.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 6 {
                cvr_host::log!("[left-handed] the weapon the gadget or heal took the left hand from is given back (from Rust)\n");
            }
        }
    } else if is_weapon(&right) || is_weapon(&left) {
        *taken = NONE;
    }
}
fn data_slot_item(data: &Handle, left: bool) -> Item {
    rt::call(data, "GetSlotActiveItem", &[ev("EquipmentManipulationRequestSlot", if left { "Left" } else { "Right" })]).map_or(NONE, |o| item_of(&o))
}
/// HandleEquipWeapon: the left hand's weapon equipped again where it is answered as done; a weapon the game sends to
/// the right hand alone goes to the left.
fn handle_equip(call: &mut Call) {
    let data = call.this();
    let item = call.arg(0).map_or(NONE, |(p, _)| item_at(p));
    let block = call.arg_bool(2);
    if !block && left_handed() && movable(&item) && same(&data_slot_item(&data, true), &item) && !valid(&data_slot_item(&data, false)) {
        let area = rt::static_call("EquipmentSystem", "GetEquipAreaType", &[ip(&item)]);
        let index = call.arg_number(1).unwrap_or(-1.0);
        let in_slot = area.and_then(|a| {
            let av = Val::of(&a).num().unwrap_or(0.0);
            rt::call(&data, "GetItemInEquipSlot", &[A::N(av), A::N(index)])
        });
        if in_slot.is_some_and(|o| same(&item_of(&o), &item)) {
            call.set_arg_bool(4, false);
            rt::call(&data, "UpdateEquipAreaActiveIndex", &[ip(&item)]);
            rt::call(&data, "SetLastUsedItem", &[ip(&item)]);
            call.set_result_bool(true);
            return;
        }
    }
    call.original();
    if call.result_bool() && left_handed() && same(&data_slot_item(&data, false), &item) && movable(&item) && !valid(&data_slot_item(&data, true)) {
        let slot = |s: &str| ev("EquipmentManipulationRequestSlot", s);
        let kind = |k: &str| ev("EquipmentManipulationRequestType", k);
        rt::call(&data, "SetSlotActiveItem", &[slot("Right"), ip(&NONE)]);
        rt::call(&data, "SendPSMWeaponManipulationRequest", &[kind("Unequip"), slot("Right"), ev("gameEquipAnimationType", "Default")]);
        rt::call(&data, "SetSlotActiveItem", &[slot("Left"), ip(&item)]);
        rt::call(&data, "SendPSMWeaponManipulationRequest", &[kind("Equip"), slot("Left"), ev("gameEquipAnimationType", "Default")]);
    }
}
/// WeaponManipulation: a weapon asked for over the left hand's only weapon is no temporary equip.
fn manipulation(call: &mut Call) {
    let item = call.arg(1).map_or(NONE, |(p, _)| item_at(p));
    // EquipmentManipulationRequestSlot: an enum, read by its own size
    let slot = call.arg(2).map(|(p, ty)| Val { ptr: p, ty }.num().unwrap_or(-1.0)).unwrap_or(-1.0);
    if call.arg_bool(4)
        && !call.arg_bool(3)
        && slot == rt::enum_value("EquipmentManipulationRequestSlot", "Left")
        && left_handed()
        && movable(&item)
    {
        call.set_arg_bool(4, false);
    }
}
fn active_weapon(call: &mut Call) {
    let Some((p, _)) = call.result() else { return };
    if valid(&item_at(p)) || !left_handed() {
        return;
    }
    let left = data_slot_item(&call.this(), true);
    if movable(&left) {
        call.set_result_from(left.as_ptr().cast());
    }
}
fn weapon_to_unequip(call: &mut Call) {
    let data = call.this();
    if valid(&data_slot_item(&data, false)) || !left_handed() {
        return;
    }
    let left = data_slot_item(&data, true);
    if !movable(&left) {
        return;
    }
    let area = rt::call(&data, "GetEquipAreaFromItemID", &[ip(&left)]).and_then(|o| Val::of(&o).f("areaType").and_then(|v| v.num()));
    if let Some(item) = area.and_then(|a| rt::call(&data, "GetActiveItem", &[A::N(a)])) {
        call.set_result_from(item.as_ptr().cast());
    }
}
/// OnRestored: a save's weapon alone in the hand the controls no longer use moves to the other one.
fn restored(call: &mut Call) {
    let data = call.this();
    let pl = rt::field(&data, "m_owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if !super::menu_rt::is_a(&pl, "PlayerPuppet") {
        return;
    }
    let Some(held) = rt::field(&data, "m_slotActiveItemsInHands") else { return };
    let Some((r, l)) = hands(&held) else { return };
    let (right, left) = (item_at(r), item_at(l));
    // SAFETY: the persistent hands' two ItemIDs in place.
    unsafe {
        if left_handed() {
            if movable(&right) && !is_weapon(&left) {
                *(l as *mut Item) = right;
                *(r as *mut Item) = NONE;
            }
        } else if movable(&left) && !is_weapon(&right) {
            *(r as *mut Item) = left;
            *(l as *mut Item) = NONE;
        }
    }
}

// ---------------------------------------------------------------- the state machines
fn smid(this: &Handle) -> Option<Val> {
    rt::field(this, "stateMachineInstanceData")
}
/// IsLeftHandLogic: the state machine's referenceName is LeftHand (read in place, as the script reads it).
fn is_left_logic(_this: &Handle, smid: &Val) -> bool {
    // SAFETY: a CName field of the struct value.
    smid.f("referenceName").is_some_and(|v| unsafe { *(v.ptr as *const u64) } == CName::hashed("LeftHand").0)
}
fn equip_cycle_before(call: &mut Call) {
    set_animations(&owner(&call.arg_handle(1)));
}
fn equip_cycle_after(call: &mut Call) {
    let this = call.this();
    let si = call.arg_handle(1);
    let pl = owner(&si);
    if smid(&this).is_some_and(|s| is_left_logic(&this, &s)) && alone(&pl) {
        psm_set_bool(&si, "IsDualWielding", false);
        rt::static_call("AnimationControllerComponent", "SetAnimWrapperWeight", &[A::H(&pl), n("dual_wield"), A::N(0.0)]);
    }
}
fn right_weapon(pl: &Handle) -> Handle {
    rt::static_obj("GameObject", "GetRightActiveWeapon", &[A::H(pl)])
}
/// CanEquipRequestedWeapon: the left hand equips its weapon with the right one empty.
fn can_equip(call: &mut Call) {
    call.original();
    if call.result_bool() {
        return;
    }
    let this = call.this();
    let (si, sc) = (call.arg_handle(0), call.arg_handle(1));
    let Some((p, ty)) = call.arg(2) else { return };
    let smid = Val { ptr: p, ty };
    if !left_handed() || !is_left_logic(&this, &smid) {
        return;
    }
    // GetSlotActiveItem(si, smid) of the left hand's machine: the equipment system's left hand
    let item = slot_item(&owner(&si), true);
    if !movable(&item) || !right_weapon(&owner(&si)).is_null() {
        return;
    }
    let ok = flag(&this, "IsRightHandInUnequippedState", &[A::H(&sc), A::B(true)]) && flag(&this, "CanProcessEquip", &[A::H(&sc), A::H(&si)]);
    call.set_result_bool(ok);
}
/// EquipCycleInitDecisions.EnterCondition: the left equipment machine checks a weapon as the offhand of a pair, which
/// excludes melee weapons and, under DriverCombatFirearms, the Nomad passenger's quest revolver (main's 48b8fb5cd,
/// #266): a lone primary in the left hand is checked against the game's primary-weapon restrictions instead, then the
/// game's own condition for everything else.
fn equip_cycle_enter(call: &mut Call) {
    let this = call.this();
    let si = call.arg_handle(1);
    if left_handed() && smid(&this).is_some_and(|s| is_left_logic(&this, &s)) {
        let pl = owner(&si);
        if alone(&pl) && right_weapon(&pl).is_null() {
            let item = slot_item(&pl, true);
            let data = rt::static_obj("EquipmentSystem", "GetData", &[A::H(&pl)]);
            if !data.is_null() && (st_flag("WeaponObject", "IsRanged", &[ip(&item)]) || st_flag("WeaponObject", "IsMelee", &[ip(&item)])) {
                call.set_result_bool(flag(&data, "CheckWeaponAgainstGameplayRestrictions", &[ip(&item), A::B(true), A::B(false)]));
                return;
            }
        }
    }
    call.original();
}
// DefaultTransition.HasSingleWeaponEquipped is a scripted static (a global function), so its callers answer instead:
// for the weapon alone in the left hand each takes it as the single weapon it is.
fn sw_force_safe(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if in_hand(&owner(&si)) {
        call.set_result_bool(!flag(&call.this(), "IsSafeStateForced", &[A::H(&sc), A::H(&si)]));
    }
}
fn sw_yes(call: &mut Call) {
    if in_hand(&owner(&call.arg_handle(1))) {
        call.set_result_bool(true);
    }
}
fn sw_temporary_unequip(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if !in_hand(&owner(&si)) {
        return;
    }
    let p = |name: &str| flag(&sc, "GetBoolParameter", &[n(name), A::B(true)]);
    let gadget = rt::num(&si, "GetActionValue", &[n("UseCombatGadget")]).unwrap_or(0.0) != 0.0;
    if (p("cgCached") || gadget && !p("invalidTempUnequipThrow")) && !p("ChargeCancelled") {
        call.set_result_bool(false);
        return;
    }
    call.set_result_bool(!flag(&call.this(), "IsTemporaryUnequipRequested", &[A::H(&sc), A::H(&si)]));
}
fn sw_ad_hoc(call: &mut Call) {
    let si = call.arg_handle(1);
    if in_hand(&owner(&si)) {
        let this = call.this();
        let t = rt::num(&this, "GetInStateTime", &[]).unwrap_or(0.0);
        call.set_result_bool(t > rt::num(&this, "GetAnimationDuration", &[A::H(&si)]).unwrap_or(f64::MAX));
    }
}
fn empty_hands_enter(call: &mut Call) {
    if in_hand(&owner(&call.arg_handle(1))) {
        call.set_result_bool(false);
    }
}
fn holstered_stim(pl: &Handle) {
    let b = rt::obj(pl, "GetStimBroadcasterComponent", &[]);
    if !b.is_null() {
        rt::call(&b, "TriggerSingleBroadcast", &[A::H(pl), ev("gamedataStimType", "WeaponHolstered")]);
    }
}
fn unequip_asked(sc: &Handle) -> bool {
    flag(sc, "GetBoolParameter", &[n("requestWeaponUnequip"), A::B(false)])
}
fn single_to_empty(call: &mut Call) {
    let this = call.this();
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    let pl = owner(&si);
    if !in_hand(&pl) {
        return;
    }
    let held = rt::num(&this, "GetInStateTime", &[]).unwrap_or(0.0) >= 0.45 && flag(&this, "EmptyHandsCondition", &[A::H(&sc), A::H(&si)]);
    if held || unequip_asked(&sc) {
        holstered_stim(&pl);
        call.set_result_bool(true);
    } else {
        call.set_result_bool(false);
    }
}
fn empty_condition(call: &mut Call) {
    call.original();
    let this = call.this();
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if call.result_bool() || !in_hand(&owner(&si)) {
        return;
    }
    if flag(&this, "IsInSafeSceneTier", &[A::H(&si)]) && (psm_get_bool(&si, "SceneAimForced") || psm_get_bool(&si, "SceneSafeForced")) {
        call.set_result_bool(false);
        return;
    }
    call.set_result_bool(unequip_asked(&sc));
}
fn force_safe_to_empty(call: &mut Call) {
    let this = call.this();
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if !in_hand(&owner(&si)) {
        return;
    }
    if !flag(&this, "IsSafeStateForced", &[A::H(&sc), A::H(&si)])
        || rt::num(&this, "GetInStateTime", &[]).unwrap_or(0.0) >= 0.45 && flag(&this, "EmptyHandsCondition", &[A::H(&sc), A::H(&si)])
    {
        call.set_result_bool(true);
        return;
    }
    let name = super::menu_rt::Arg::new("timeToAutoUnequipWeapon");
    let auto = rt::num(&this, "GetStaticFloatParameterDefault", &[name.a(), A::N(-1.0)]).unwrap_or(-1.0);
    let multiplayer = rt::global_call("IsMultiplayer", &[]).is_some_and(|o| Val::of(&o).bool());
    let cond = |c: &str| rt::num(&sc, "GetConditionFloat", &[n(c)]).unwrap_or(0.0);
    call.set_result_bool(!multiplayer && auto > 0.0 && cond("ForceSafeCurrentTimeToAutoUnequip") >= auto + cond("ForceSafeTimeStampToAutoUnequip"));
}
fn not_in_hand(call: &mut Call) {
    if in_hand(&owner(&call.arg_handle(1))) {
        call.set_result_bool(false);
    }
}
/// An enum argument's value (its storage is 1, 2, 4 or 8 bytes).
fn arg_enum(call: &Call, i: usize) -> Option<i64> {
    let (p, ty) = call.arg(i)?;
    // SAFETY: the value read by its own size.
    unsafe {
        Some(match ty.size() {
            1 => i64::from(*(p as *const i8)),
            2 => i64::from(*(p as *const i16)),
            4 => i64::from(*(p as *const i32)),
            8 => *(p as *const i64),
            _ => return None,
        })
    }
}
// THE WEAPON'S GATES FOR THE COMBAT GADGET (RB) AND A CONSUMABLE (the D-pad's up) WITH A WEAPON OUT: singleWield,
// aimingState and forceSafe take them only with the RIGHT hand's equipment state machine equipped
// (DefaultTransition.CheckEquipmentStateMachineState(Right, Equipped)), so with the weapon alone in the left hand RB
// and the heal did nothing (2026-11-25: the gadget chord's RB held 0.9 s, CombatGadget stayed 0). Left-handed, the
// left hand's machine equipped answers for the right one; the weapon cycle beside them also asks
// HasRightWeaponEquipped and stays the right hand's.
fn equipment_state(call: &mut Call) {
    call.original();
    if call.result_bool() {
        return;
    }
    let right = rt::enum_value("EEquipmentSide", "Right") as i64;
    let equipped = rt::enum_value("EEquipmentState", "Equipped") as i64;
    if arg_enum(call, 1) != Some(right) || arg_enum(call, 2) != Some(equipped) || !left_handed() {
        return;
    }
    let sc = call.arg_handle(0);
    let Some(id) = sm_id("Equipment", "LeftHand") else { return };
    if flag(&sc, "IsStateActiveWithIdentifier", &[A::P(id.as_ptr().cast()), n("equipped")]) {
        call.set_result_bool(true);
    }
}
/// InputContextTransitionEvents.UpdateWeaponInputHints: the weapon's hints taken for the left hand.
fn input_hints(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    let pl = owner(&si);
    if !in_hand(&pl) {
        return;
    }
    let ctx = weapon_input_hints(&call.this(), &sc, &si, &pl);
    call.set_result_enum(ctx as i64);
}
/// VRLeftHandedWeaponInputHints: InputContextTransitionEvents' own decisions over the left hand's equipment machine
/// and weapon (the charge hint stays the right hand's).
fn weapon_input_hints(this: &Handle, sc: &Handle, si: &Handle, pl: &Handle) -> f64 {
    let ctx = |m: &str| rt::enum_value("ActiveBaseContext", m);
    let exploring = flag(this, "IsInHighLevelState", &[A::H(sc), n("exploration")]);
    let examining = flag(this, "IsExaminingDevice", &[A::H(si)]) || st_flag("DefaultTransition", "IsInteractingWithTerminal", &[A::H(si)]);
    let takeover = rt::obj(pl, "GetTakeOverControlSystem", &[]);
    let controlled = flag(&takeover, "IsDeviceControlled", &[]);
    let in_equip = st_flag("DefaultTransition", "IsLeftHandInEquippedState", &[A::H(sc)])
        || sm_id("Equipment", "LeftHand").is_some_and(|id| flag(sc, "IsStateActiveWithIdentifier", &[A::P(id.as_ptr().cast()), n("equipCycle")]));
    let weapon = rt::static_obj("GameObject", "GetLeftActiveWeapon", &[A::H(pl)]);
    let ranged = in_equip && !weapon.is_null() && flag(&weapon, "IsRanged", &[]);
    let melee = in_equip && !weapon.is_null() && !ranged && flag(&weapon, "IsMelee", &[]);
    let can_charge = ranged && st_flag("DefaultTransition", "IsChargeRangedWeapon", &[A::H(si)]);
    let aiming = flag(sc, "IsStateActive", &[n("UpperBody"), n("aimingState")]);
    let can_throw = melee && flag(&weapon, "IsThrowable", &[]) && aiming;
    let can_dodge = !ranged || !aiming || flag(si, "HasStatFlag", &[ev("gamedataStatType", "CanAimWhileDodging")]);
    let can_crouch = psm_get_int(si, "Combat") != rt::enum_value("gamePSMCombat", "InCombat");
    let can_carry_throw = psm_get_bool(si, "CanThrowCarriedNPC");
    let shown = |p: &str| flag(sc, "GetBoolParameter", &[n(p), A::B(true)]);
    let (melee_shown, throw_shown, ranged_shown, charge_shown) =
        (shown("isMeleeInputHintDisplayed"), shown("isThrowInputHintDisplayed"), shown("isRangedInputHintDisplayed"), shown("isChargeInputHintDisplayed"));
    let (dodge_shown, crouch_shown, carry_shown) =
        (shown("isRangedDodgeInputHintDisplayed"), shown("isCrouchInputHintDisplayed"), shown("isThrowCarriedNPCInputHintDisplayed"));
    let field = |f: &str| rt::field(this, f).is_some_and(|v| v.bool());
    let changed = field("m_hasControllerChanged") || field("m_hasControllerSchemeChanged");
    let remove = |action: &str, group: &str, param: &str| {
        rt::call(this, "RemoveInputHint", &[A::H(si), n(action), n(group)]);
        rt::call(sc, "RemovePermanentBoolParameter", &[n(param)]);
    };
    if melee && exploring && !examining && !controlled {
        if crouch_shown && !can_crouch {
            remove("ToggleCrouch", "Melee", "isCrouchInputHintDisplayed");
        }
        if !melee_shown || changed || throw_shown != can_throw || !crouch_shown && can_crouch {
            return ctx("MeleeWeapon");
        }
    } else if melee_shown && (!melee || !exploring || examining || controlled) {
        rt::call(this, "RemoveMeleeInputHints", &[A::H(sc), A::H(si)]);
    }
    if ranged && !flag(&weapon, "IsHeavyWeapon", &[]) && exploring && !examining && !controlled {
        if charge_shown && !can_charge {
            remove("RangedAttack", "Ranged", "isChargeInputHintDisplayed");
        }
        if dodge_shown && !can_dodge {
            remove("Dodge", "Ranged", "isRangedInputHintDisplayed");
        }
        if crouch_shown && !can_crouch {
            remove("ToggleCrouch", "Ranged", "isCrouchInputHintDisplayed");
        }
        if !ranged_shown
            || changed
            || !charge_shown && can_charge
            || !dodge_shown && can_dodge
            || !crouch_shown && can_crouch
            || !carry_shown && can_carry_throw
        {
            if shown("isBodyCarryInputHintDisplayed") {
                return ctx("BodyCarringWithRangedWeapon");
            }
            return ctx("RangedWeapon");
        }
    } else if ranged_shown && (!ranged || !exploring || examining || controlled) {
        rt::call(this, "RemoveRangedInputHints", &[A::H(sc), A::H(si)]);
    }
    0.0
}

// ---------------------------------------------------------------- the HUD's weapon data
/// VRLeftHandedWeapon.InHandData: the equipment UI data with the weapon in hand under rightWeapon.
fn in_hand_data(call: &mut Call) {
    if !left_handed() {
        return;
    }
    let data = call.arg_variant_handle(0);
    if data.is_null() {
        return;
    }
    let weapon_id = |side: &str| rt::field(&data, side).and_then(|w| w.f("weaponID")).map_or(NONE, |v| item_at(v.ptr));
    if valid(&weapon_id("rightWeapon")) || !valid(&weapon_id("leftWeapon")) {
        return;
    }
    let held = super::lobby_ink::new("SlotDataHolder");
    if held.is_null() {
        return;
    }
    // SAFETY: the two objects' struct properties, each assigned by its own type.
    unsafe {
        for (to, from) in [("ammoData", "ammoData"), ("rightWeapon", "leftWeapon")] {
            if let (Some(t), Some(f)) = (rt::field(&held, to), rt::field(&data, from)) {
                t.ty.assign(t.ptr, f.ptr);
            }
        }
    }
    let Some(ty) = red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_type(CName::hashed("handle:SlotDataHolder"))) else { return };
    let Some(mut h) = Owned::of_type(ty) else { return };
    // SAFETY: a handle value of its own type, assigned from ours.
    unsafe { ty.assign(h.as_mut_ptr().cast(), (&held as *const Handle).cast()) };
    if let Some(var) = super::menu_rt::variant_of(&h) {
        call.set_arg_from(0, var.as_ptr().cast());
    }
}

// ---------------------------------------------------------------- the melee machine
fn melee_idle(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if in_hand(&owner(&si)) {
        call.set_result_bool(st_flag("DefaultTransition", "IsLeftHandInEquippedState", &[A::H(&sc)]));
    }
}
fn melee_ready(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if in_hand(&owner(&si)) && st_flag("DefaultTransition", "IsLeftHandInUnequippingState", &[A::H(&sc)]) {
        call.set_result_bool(false);
    }
}
/// IsInMeleeState: the one melee machine there is, whichever slot runs it.
fn in_melee_state(call: &mut Call) {
    call.original();
    if call.result_bool() {
        return;
    }
    let sc = call.arg_handle(0);
    let want = call.arg_u64(1);
    let Some(id) = sm_id("Melee", "WeaponLeft") else { return };
    let state = rt::call(&sc, "GetStateMachineCurrentStateWithIdentifier", &[A::P(id.as_ptr().cast())]).map_or(0, |o| {
        // SAFETY: a CName result.
        unsafe { *(o.as_ptr() as *const u64) }
    });
    call.set_result_bool(state == want);
}
/// SpawnMeleeWeaponProjectile: the thrown weapon is the machine's own, launched as the game launches one from the right.
fn melee_projectile(call: &mut Call) {
    let si = call.arg_handle(0);
    let pl = owner(&si);
    if !in_hand(&pl) {
        return;
    }
    call.skip();
    let weapon = rt::static_obj("MeleeTransition", "GetWeaponObject", &[A::H(&si)]);
    if weapon.is_null() {
        return;
    }
    let mut appearance = 0u64;
    if flag(&weapon, "WeaponHasTag", &[n("Throwable")]) && flag(&weapon, "GetBoolPropertyFromWeaponDefinition", &[A::Name(tdb(".useProjectileAppearance"))]) {
        appearance = rt::call(&weapon, "GetAppearanceNameFromComponent", &[n("Grip")]).map_or(0, |o| {
            // SAFETY: a CName result.
            unsafe { *(o.as_ptr() as *const u64) }
        });
    }
    let template = rt::call(&weapon, "GetNamePropertyFromWeaponDefinition", &[A::Name(tdb(".projectileTemplateName"))]).map_or(0, |o| {
        // SAFETY: a CName result.
        unsafe { *(o.as_ptr() as *const u64) }
    });
    if template == 0 || template == CName::hashed("None").0 {
        return;
    }
    let listener = rt::field(&call.this(), "m_driverCombatListener").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let tpp = flag(&listener, "IsMountedInTPP", &[]);
    let launch = if tpp { "SpawnProjectileFromRightHand" } else { "SpawnProjectileFromScreenCenter" };
    rt::static_call("ProjectileLaunchHelper", launch, &[A::H(&pl), A::Name(template), A::Name(appearance), A::H(&weapon)]);
}

// ---------------------------------------------------------------- the weapon mod's
fn request(pl: &Handle, action: &str) -> bool {
    super::holster::equip_request(pl, action);
    true
}
/// VRLeftHandedRehand: the controls turned over with a weapon out -- a weapon alone in the hand it no longer belongs
/// in is put away (true); the weapon mod draws it again once both hands are empty.
pub(super) fn rehand(pl: &Handle) -> bool {
    let to_left = left_handed();
    let right = right_weapon(pl);
    let left = rt::static_obj("GameObject", "GetLeftActiveWeapon", &[A::H(pl)]);
    let (held, other) = if to_left { (right, left) } else { (left, right) };
    set_animations(pl);
    if held.is_null() || !other.is_null() || !rt::call(&held, "GetItemID", &[]).is_some_and(|o| movable(&item_of(&o))) {
        return false;
    }
    request(pl, "UnequipWeapon")
}
pub(super) fn hands_free(pl: &Handle) -> bool {
    right_weapon(pl).is_null() && rt::static_obj("GameObject", "GetLeftActiveWeapon", &[A::H(pl)]).is_null()
}
pub(super) fn redraw(pl: &Handle) -> bool {
    request(pl, "RequestLastUsedWeapon")
}
/// VRLeftHandedRestorePending: the left hand's weapon from a save the load did not draw.
pub(super) fn restore_pending(pl: &Handle) -> bool {
    if !left_handed() || !hands_free(pl) {
        return false;
    }
    let data = rt::static_obj("EquipmentSystem", "GetData", &[A::H(pl)]);
    !data.is_null() && movable(&data_slot_item(&data, true)) && !is_weapon(&data_slot_item(&data, false))
}
pub(super) fn draw_restored(pl: &Handle) -> bool {
    restore_pending(pl) && redraw(pl)
}

/// The hooks, in reds_hooks' table.
pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 29] = [
    ("EquipmentSystemPlayerData", "HandleEquipWeapon", Kind::Override, handle_equip),
    ("EquipmentSystemPlayerData", "WeaponManipulation", Kind::Before, manipulation),
    ("EquipmentSystemPlayerData", "GetSlotActiveWeapon", Kind::After, active_weapon),
    ("EquipmentSystemPlayerData", "GetActiveWeaponToUnequip", Kind::Override, weapon_to_unequip),
    ("EquipmentSystemPlayerData", "OnRestored", Kind::After, restored),
    ("EquipCycleInitEvents", "OnEnter", Kind::Before, equip_cycle_before),
    ("EquipCycleInitEvents", "OnEnter", Kind::After, equip_cycle_after),
    ("EquipmentBaseTransition", "CanEquipRequestedWeapon", Kind::Override, can_equip),
    ("EquipCycleInitDecisions", "EnterCondition", Kind::Override, equip_cycle_enter),
    ("ForceSafeDecisions", "ToSingleWield", Kind::Override, sw_force_safe),
    ("EmptyHandsDecisions", "ToSingleWield", Kind::Override, sw_yes),
    ("SingleWieldDecisions", "EnterCondition", Kind::Override, sw_yes),
    ("TemporaryUnequipDecisions", "ToSingleWield", Kind::Override, sw_temporary_unequip),
    ("AdHocAnimationDecisions", "ToSingleWield", Kind::Override, sw_ad_hoc),
    ("EmptyHandsDecisions", "EnterCondition", Kind::Override, empty_hands_enter),
    ("SingleWieldDecisions", "ToEmptyHands", Kind::Override, single_to_empty),
    ("UpperBodyTransition", "EmptyHandsCondition", Kind::Override, empty_condition),
    ("ForceSafeDecisions", "ToEmptyHands", Kind::Override, force_safe_to_empty),
    ("AdHocAnimationDecisions", "ToEmptyHands", Kind::Override, not_in_hand),
    ("BaseContextEvents", "IsStateValidForExploration", Kind::Override, not_in_hand),
    ("DefaultTransition", "CheckEquipmentStateMachineState", Kind::Override, equipment_state),
    ("InputContextTransitionEvents", "UpdateWeaponInputHints", Kind::Override, input_hints),
    ("WeaponRosterGameController", "OnWeaponDataChanged", Kind::Before, in_hand_data),
    ("IronsightGameController", "OnWeaponDataChanged", Kind::Before, in_hand_data),
    ("InventoryDataManagerV2", "OnWeaponDataChanged", Kind::Before, in_hand_data),
    ("MeleeEquippingDecisions", "ToMeleeIdle", Kind::Override, melee_idle),
    ("MeleeTransition", "IsWeaponReady", Kind::Override, melee_ready),
    ("DefaultTransition", "IsInMeleeState", Kind::Override, in_melee_state),
    ("MeleeTransition", "SpawnMeleeWeaponProjectile", Kind::Override, melee_projectile),
];

/// A development check (CVR_LH_TEST, inherited by the game from the probe session, with xr_left_handed=1 in the ini):
/// 45 s into the loaded save the first weapon slot is drawn; at 52 s where it hangs and the hand's answers are logged,
/// and the weapon put away.
pub(super) fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    if std::env::var_os("CVR_LH_TEST").is_none() {
        return;
    }
    let pl = super::belt::player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 3 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first)));
        return;
    }
    let t = t + dt;
    let mut next = step;
    if step == 0 && t > 45.0 {
        request(&pl, "RequestWeaponSlot1");
        cvr_host::log!("[left-handed] self-test: left-handed {} ; weapon slot 1 requested (from Rust)\n", left_handed());
        next = 1;
    } else if step == 1 && t > 52.0 {
        let right = !weapon_in(&pl, "AttachmentSlots.WeaponRight").is_null();
        let left = !weapon_in(&pl, "AttachmentSlots.WeaponLeft").is_null();
        let data = rt::static_obj("EquipmentSystem", "GetData", &[A::H(&pl)]);
        let (r, l) = (data_slot_item(&data, false), data_slot_item(&data, true));
        cvr_host::log!(
            "[left-handed] self-test: WeaponRight {right} WeaponLeft {left} ; active items right {} left {} ; in hand {} alone {} (from Rust)\n",
            valid(&r), valid(&l), in_hand(&pl), alone(&pl)
        );
        request(&pl, "UnequipWeapon");
        next = 2;
    } else if step == 2 && t > 56.0 {
        cvr_host::log!("[left-handed] self-test done: hands free {} (from Rust)\n", hands_free(&pl));
        next = 3;
    }
    T.with(|c| c.set((t, next, first)));
}
