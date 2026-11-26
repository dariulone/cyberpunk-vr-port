//! A workspot's primary weapon condition read from the TweakDB flats (formerly CyberpunkVRPort_Engine's
//! vrport_workspot_equipment.reds, main's 00bb7770f): crowd jobs can evaluate a workspot while a saved world is being
//! restored, and the game's condition casts each equipment entry's item to an Item_Record there -- the crash dump shows
//! the cast failing inside Item(). The condition needs only the item's tags, so they are read by ID:
//! `<entry>.item`, then `<item>.tags`.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;

pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 1] = [("PrimaryWeaponTypeCondition", "CheckCondition", Kind::Override, check_condition)];

fn is_a(h: &Handle, class: &str) -> bool {
    // SAFETY: a live object (the call's argument, held).
    !h.is_null() && unsafe { red4ext_abi::rtti::object_class(h.instance) }.is_some_and(|c| c.is_a(CName::hashed(class)))
}
/// A flat's value by its id: the value's storage (None without one).
fn flat(id: u64) -> Option<*const core::ffi::c_void> {
    let db = red4ext_abi::tweakdb::get();
    if db.is_null() {
        return None;
    }
    // SAFETY: the database's own flat lookup and the value its FlatValue holds.
    unsafe {
        let f = red4ext_abi::tweakdb::flat_value(db, id);
        if f.is_null() {
            return None;
        }
        let v = red4ext_abi::tweakdb::flat_stack_value(f);
        (!v.value.is_null()).then_some(v.value as *const core::ffi::c_void)
    }
}
/// VRWorkspotEquipment.HasTag: the equipment entry's item (`<entry>.item`) carries the tag (`<item>.tags`).
fn has_tag(entry: u64, tag: u64) -> bool {
    use red4ext_abi::tweakdbid::{derive, is_valid};
    // SAFETY: a TweakDBID flat.
    let Some(item) = flat(derive(entry, ".item")).map(|p| unsafe { *(p as *const u64) }) else { return false };
    if !is_valid(item) {
        return false;
    }
    let Some(tags) = flat(derive(item, ".tags")) else { return false };
    // SAFETY: an array:CName flat (a DynArray of 8-byte names).
    unsafe {
        let arr = &*(tags as *const red4ext_abi::dynarray::DynArray<u64>);
        arr.as_slice().contains(&tag)
    }
}
fn ev(e: &str, m: &str) -> f64 {
    rt::enum_value(e, m)
}
fn check_condition(call: &mut Call) {
    let ent = call.arg_handle(0);
    if ent.is_null() || !is_a(&ent, "gameObject") {
        call.set_result_bool(true);
        return;
    }
    if !rt::call(&ent, "IsNPC", &[]).is_some_and(|o| Val::of(&o).bool()) {
        call.set_result_bool(false);
        return;
    }
    let kind = rt::field(&call.this(), "weaponType").and_then(|v| v.num()).unwrap_or(-1.0);
    let e = "WorkspotWeaponConditionEnum";
    let tag = if kind == ev(e, "Ranged") {
        "RangedWeapon"
    } else if kind == ev(e, "OneHandedRanged") {
        "OneHandedRangedWeapon"
    } else if kind == ev(e, "Melee") {
        "MeleeWeapon"
    } else if kind == ev(e, "MeleeCyberware") {
        "Meleeware"
    } else if kind == ev(e, "LMG") {
        "LMG"
    } else if kind == ev(e, "HMG") {
        "HMG"
    } else {
        call.set_result_bool(true);
        return;
    };
    if !is_a(&ent, "NPCPuppet") {
        call.set_result_bool(false);
        return;
    }
    let tag = CName::hashed(tag).0;
    let items = rt::static_outs("AIActionTransactionSystem", "CalculateEquipmentItems", &[A::H(&ent), A::Name(CName::hashed("PrimaryEquipment").0)])
        .and_then(|(_, outs)| outs.into_iter().next());
    let mut found = false;
    if let Some(items) = items {
        let v = Val::of(&items);
        for i in 0..v.len() {
            let entry = v.at(i).map(|e| e.handle()).unwrap_or_else(Handle::null);
            if entry.is_null() {
                continue;
            }
            // SAFETY: a TweakDBID result.
            let Some(id) = rt::call(&entry, "GetID", &[]).map(|o| unsafe { *(o.as_ptr() as *const u64) }) else { continue };
            if has_tag(id, tag) {
                found = true;
                break;
            }
        }
    }
    call.set_result_bool(found);
}
