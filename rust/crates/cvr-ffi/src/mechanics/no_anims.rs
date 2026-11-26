//! mods/redscript/CyberpunkVRPort_NoAnims: the game's animations that fight VR neutralised, and the firearm kept in
//! the hands -- the state machines' hooks (the table below).
//!
//! - Firearms stay up (Rivarez's "Control lower and holster weapons" folded in, with the 2.5 per-hand timestamp and
//!   the left-handed hand): the hand's TurnOffPublicSafeTimeStamp pinned -- 9999999 raised (its lowering interval
//!   never runs out), 0 lowered -- on an equip, a ready tick, the raise's exit, a quick melee and a shot; the fire or
//!   aim button raises, the holster button lowers a raised one; ShouldLeaveSafe / ShouldEnterSafe follow the pin;
//!   the automatic unequip after a while never asked.
//! - Instant equip and unequip, no first-equip cinematic -- neither a weapon's first draw nor the show-off a held
//!   weapon slot's button asks for (PlayerStateMachine.ScenePreventFirstEquip held up through HandleWeaponEquip) --
//!   and no idle breaks (ReadyEvents' 20 s IdleBreak); the sprint's weapon-lower blocked from its entry to past its
//!   exit (WeaponSprintBlock, the post-sprint twirl), the plugin told (SetVRSprintActive); no camera bobbing, no melee
//!   camera shake; the post-shot recoil additive at 0 (the hand's WeaponHandlingData republished after the tick).
//! - The manual reload (NoAmmoDecisions.ToReload replaced: only the Reload button, a dry fire's click for most guns).
//! - Aim-walk only in focus or while a left-hand cyberware charges.
//! - The plugin's reload animation window (SetVRReloadAnimation: the logical duration plus the end time) and the
//!   weapon's pose state (SetVRWeaponPoseState, SetVRWeaponRaiseTransition) for the ADS muzzle stabilizer.
//! - The VR finger's cadence (main's vrport_weapon_trigger.reds and vrport_full_auto_release.reds): a semi-automatic
//!   fires once per fresh physical pull (VRCurrentFingerAction), a press the game kept from before the shot interval
//!   discarded, at most one shot per 100 ms per hand (the next-shot deadlines on the player), no fire delay; bursts,
//!   charges and the trigger mode's change to charge admitted by the same interval; a full-auto stops on the physical
//!   release even when the native action sticks; with two guns a hand let go stops its gun. The weapon and player
//!   fields of the scripts are maps here.
//!
//! VRPortFirearm and VRPortFirearmIsLeft (CyberpunkVRPort_Engine's, compiled only in the C++ build) are functions here.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::sync::Mutex;

const RAISED: f64 = 9999999.0;

fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
fn tdb(s: &str) -> A<'static> {
    A::Name(red4ext_abi::tweakdbid::from_name(s))
}
fn native<T: crate::script_call::Ret>(name: &str, values: &[crate::script_call::Value]) -> Option<T> {
    super::belt::native::<T>(name, values)
}
fn owner(si: &Handle) -> Handle {
    rt::field(si, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn is_player(o: &Handle) -> bool {
    super::menu_rt::is_a(o, "PlayerPuppet")
}
fn num(o: &Handle, f: &str, args: &[A]) -> f64 {
    rt::num(o, f, args).unwrap_or(0.0)
}
fn flag(o: &Handle, f: &str, args: &[A]) -> bool {
    super::menu_rt::b(o, f, args)
}
fn weapon_in(si: &Handle, slot: &str) -> Handle {
    let ts = rt::obj(si, "GetTransactionSystem", &[]);
    let w = rt::obj(&ts, "GetItemInSlot", &[A::H(&owner(si)), tdb(slot)]);
    if super::menu_rt::is_a(&w, "WeaponObject") {
        w
    } else {
        Handle::null()
    }
}
/// VRPortFirearmIsLeft: a weapon in the left hand alone (the left-handed controls).
fn firearm_is_left(si: &Handle) -> bool {
    weapon_in(si, "AttachmentSlots.WeaponRight").is_null() && !weapon_in(si, "AttachmentSlots.WeaponLeft").is_null()
}
fn firearm(si: &Handle) -> Handle {
    weapon_in(si, if firearm_is_left(si) { "AttachmentSlots.WeaponLeft" } else { "AttachmentSlots.WeaponRight" })
}
fn is_firearms(si: &Handle) -> bool {
    let w = firearm(si);
    !w.is_null() && flag(&w, "IsRanged", &[])
}
/// The firearm hand's TurnOffPublicSafeTimeStamp (2.5: each hand's state machine keeps its own).
fn stamp_name(si: &Handle) -> A<'static> {
    n(if firearm_is_left(si) { "LeftTurnOffPublicSafeTimeStamp" } else { "RightTurnOffPublicSafeTimeStamp" })
}
fn stamp(sc: &Handle, si: &Handle) -> f64 {
    num(sc, "GetFloatParameter", &[stamp_name(si), A::B(true)])
}
fn lowered(sc: &Handle, si: &Handle) -> bool {
    stamp(sc, si) == 0.0
}
fn raised(sc: &Handle, si: &Handle) -> bool {
    stamp(sc, si) == RAISED
}
/// SetWeaponRaised: the stamp from which the lowering interval never runs out (left alone when lowered by hand).
fn set_raised(sc: &Handle, si: &Handle, unless_lowered: bool) {
    if unless_lowered && lowered(sc, si) {
        return;
    }
    seen(1);
    rt::call(sc, "SetPermanentFloatParameter", &[stamp_name(si), A::N(RAISED), A::B(true)]);
}
fn set_lowered(sc: &Handle, si: &Handle) {
    seen(2);
    rt::call(sc, "SetPermanentFloatParameter", &[stamp_name(si), A::N(0.0), A::B(true)]);
}

/// The PlayerStateMachine board's fields, found once.
struct Psm {
    def: Handle,
    ids: HashMap<&'static str, usize>,
}
// SAFETY: the definition is made and read on the game's main thread.
unsafe impl Send for Psm {}
static PSM: Mutex<Option<Psm>> = Mutex::new(None);
/// localBlackboard.GetInt / GetFloat(PlayerStateMachine.<field>).
fn psm(si: &Handle, getter: &str, field: &'static str) -> f64 {
    let Some((bb, id)) = psm_field(si, field) else { return 0.0 };
    rt::num(&bb, getter, &[A::P(id as *const core::ffi::c_void)]).unwrap_or(0.0)
}
/// localBlackboard.SetBool(PlayerStateMachine.<field>, value).
fn psm_set_bool(si: &Handle, field: &'static str, value: bool) {
    if let Some((bb, id)) = psm_field(si, field) {
        rt::call(&bb, "SetBool", &[A::P(id as *const core::ffi::c_void), A::B(value), A::B(false)]);
    }
}
/// The state machine's local board and the field's id on it.
fn psm_field(si: &Handle, field: &'static str) -> Option<(Handle, usize)> {
    let bb = rt::field(si, "localBlackboard").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if bb.is_null() {
        return None;
    }
    let id = {
        let mut g = PSM.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_none() {
            let def = super::boards::defs().ok().and_then(|d| super::boards::definition(&d, "PlayerStateMachine").ok().cloned());
            *g = def.map(|def| Psm { def, ids: HashMap::new() });
        }
        let p = g.as_mut()?;
        let def = p.def.clone();
        *p.ids.entry(field).or_insert_with(|| super::boards::field(&def, field).map_or(0, |f| f as usize))
    };
    (id != 0).then_some((bb, id))
}

// ---------------------------------------------------------------- firearms stay up
fn equip_raise(call: &mut Call) {
    if psm_bool(&call.arg_handle(0), "IsWeaponFirstEquip") {
        seen(7);
    }
    seen(8);
    psm_set_bool(&call.arg_handle(0), "ScenePreventFirstEquip", PREVENT_WAS.with(|c| c.get()));
    set_raised(&call.arg_handle(1), &call.arg_handle(0), false);
}
fn psr_exit(call: &mut Call) {
    set_raised(&call.arg_handle(0), &call.arg_handle(1), true);
}
fn quick_melee_enter(call: &mut Call) {
    set_raised(&call.arg_handle(0), &call.arg_handle(1), false);
}
fn shoot_enter(call: &mut Call) {
    set_raised(&call.arg_handle(0), &call.arg_handle(1), false);
}
fn no_auto_unequip(call: &mut Call) {
    call.skip();
}
/// UpperBodyEventsTransition.UpdateSwitchItem: a launcher's switch release swallowed; with a firearm the fire or aim
/// button raises it and the holster button lowers a raised one.
fn update_switch_item(call: &mut Call) {
    let (sc, si) = (call.arg_handle(1), call.arg_handle(2));
    let pl = owner(&si);
    if is_player(&pl) && super::launcher_ready::flag(&pl, super::launcher_ready::CONSUME_SWITCH_RELEASE) {
        rt::call(&call.this(), "ResetEquipVars", &[A::H(&sc)]);
        if num(&si, "GetActionValue", &[n("SwitchItem")]) <= 0.0 {
            super::launcher_ready::set_flag(&pl, super::launcher_ready::CONSUME_SWITCH_RELEASE, false);
        }
        call.set_result_bool(false);
        return;
    }
    if !is_firearms(&si) {
        return;
    }
    if flag(&si, "IsActionJustTapped", &[n("RangedAttack")]) || num(&si, "GetActionValue", &[n("CameraAim")]) > 0.0 {
        set_raised(&sc, &si, false);
        call.set_result_bool(false);
        return;
    }
    if flag(&si, "IsActionJustTapped", &[n("HolsterWeapon")]) && raised(&sc, &si) {
        set_lowered(&sc, &si);
        call.set_result_bool(false);
    }
}
fn should_leave_safe(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if is_firearms(&si) {
        if raised(&sc, &si) {
            call.set_result_bool(true);
        } else if lowered(&sc, &si) {
            call.set_result_bool(false);
        }
    }
}
fn should_enter_safe(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    if is_firearms(&si) {
        if raised(&sc, &si) {
            call.set_result_bool(false);
        } else if lowered(&sc, &si) {
            call.set_result_bool(true);
        }
    }
}

// ---------------------------------------------------------------- the manual reload
/// The guns whose dry fire stays silent.
const SILENT_DRY: [&str; 15] = [
    "w_submachinegun_arasaka_shingen", "w_special_kangtao_dian", "w_handgun_arasaka_yukimura", "w_handgun_kangtao_chao",
    "w_rifle_precision_militech_achilles", "w_2020_shotgun_blunderbuss", "w_shotgun_zhuo", "w_rifle_sniper_tsunami_ashura",
    "w_rifle_assault_nokota_sidewinder", "w_rifle_sniper_tsunami_nekomata", "w_revolver_darra_quasar",
    "w_revolver_techtronika_burya", "w_handgun_arasaka_kenshin", "w_handgun_militech_lexington", "w_handgun_militech_omaha",
];
fn to_reload(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    let r = (|| {
        let weapon = rt::obj(&call.this(), "GetWeaponObject", &[A::H(&si)]);
        let data = rt::obj(&weapon, "GetItemData", &[]);
        let name = rt::call(&data, "GetName", &[]).map(|o| Val::of(&o).text()).unwrap_or_default();
        if !rt::static_call("WeaponObject", "CanReload", &[A::H(&weapon)]).is_some_and(|o| Val::of(&o).bool()) {
            return false;
        }
        let shoot = psm(&si, "GetInt", "ShootState");
        if shoot == 4.0 || shoot == 5.0 || shoot == 6.0 {
            return false;
        }
        let stat = |s: &str| A::N(rt::enum_value("gamedataStatType", s));
        if !flag(&si, "HasStatFlag", &[stat("CanWeaponReloadWhileSprinting")])
            && psm(&si, "GetInt", "Locomotion") == rt::enum_value("gamePSMLocomotionStates", "Sprint")
        {
            return false;
        }
        for (s, state) in [("CanWeaponReloadWhileVaulting", "vault"), ("CanWeaponReloadWhileSliding", "slide")] {
            if !flag(&si, "HasStatFlag", &[stat(s)]) && flag(&sc, "IsStateActive", &[n("Locomotion"), n(state)]) {
                return false;
            }
        }
        if flag(&si, "IsActionJustPressed", &[n("QuickMelee")]) {
            return false;
        }
        if flag(&si, "IsActionJustPressed", &[n("RangedAttack")]) {
            if !SILENT_DRY.contains(&name.as_str()) {
                rt::static_call("GameObject", "PlaySound", &[A::H(&weapon), n("w_gun_shotgun_power_tactician_dry_fire")]);
            }
            return false;
        }
        flag(&si, "IsActionJustPressed", &[n("Reload")])
    })();
    call.set_result_bool(r);
}
fn aim_walk(call: &mut Call) {
    let si = call.arg_handle(1);
    if rt::field(&si, "localBlackboard").is_none_or(|v| v.handle().is_null()) {
        return;
    }
    let focus = psm(&si, "GetInt", "Vision") == 1.0;
    let charging = psm(&si, "GetInt", "LeftHandCyberware") == 5.0;
    if !focus && !charging {
        call.set_result_bool(false);
    }
}

// ---------------------------------------------------------------- no animations
fn zero_duration(call: &mut Call) {
    seen(5);
    call.set_result_float(0.0);
}
fn first_equip_played(call: &mut Call) {
    call.set_result_bool(true);
}
thread_local! {
    /// ScenePreventFirstEquip as it was before HandleWeaponEquip held it up.
    static PREVENT_WAS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// EquipmentBaseTransition.HandleWeaponEquip, before: the first-equip animation is played for a request that asks
/// for it -- a weapon slot's button held, the weapon wheel's display -- and for a "forceFirstEquip" state, whatever
/// HasPlayedFirstEquip says; only the board's ScenePreventFirstEquip stops them all. Held up through the call and
/// put back after it (equip_raise), so a scene's own prevention stands.
fn first_equip_prevent(call: &mut Call) {
    let si = call.arg_handle(0);
    PREVENT_WAS.with(|c| c.set(psm_bool(&si, "ScenePreventFirstEquip")));
    psm_set_bool(&si, "ScenePreventFirstEquip", true);
}
/// ReadyEvents.OnTick, before: a weapon held standing out of combat got an IdleBreak (the weapon looked at, turned,
/// a slide checked) once its timestamp was 20 s old; pushed past any time, the break never comes.
fn no_idle_break(call: &mut Call) {
    seen(6);
    super::menu_rt::hset(&call.this(), "m_timeStamp", 1.0e9);
}
/// AnimFeature_WeaponSprintBlock.active: the sprint's weapon-lower blocked, the weapon up for VRIK.
fn sprint_block(si: &Handle, active: bool) {
    seen(3);
    let f = super::lobby_ink::new("AnimFeature_WeaponSprintBlock");
    if !f.is_null() {
        super::menu_rt::hset(&f, "active", 1.0);
        rt::static_call("AnimationControllerComponent", "ApplyFeatureToReplicate", &[A::H(&owner(si)), n("WeaponSprintBlock"), A::H(&f)]);
    }
    native::<i32>("SetVRSprintActive", &[crate::script_call::Value::Int32(i32::from(active))]);
}
fn sprint_enter(call: &mut Call) {
    sprint_block(&call.arg_handle(1), true);
}
fn sprint_update(call: &mut Call) {
    sprint_block(&call.arg_handle(2), true);
}
fn sprint_exit(call: &mut Call) {
    sprint_block(&call.arg_handle(1), false);
}
fn bob_off_transition(call: &mut Call) {
    call.set_arg_bool(2, true);
}
fn bob_off_player(call: &mut Call) {
    call.set_arg_bool(0, true);
}
fn player_settings(call: &mut Call) {
    let pl = call.this();
    rt::static_call("AnimationControllerComponent", "SetInputBool", &[A::H(&pl), n("disable_camera_bobbing"), A::B(true)]);
    rt::static_call("AnimationControllerComponent", "SetInputFloat", &[A::H(&pl), n("melee_camera_shake_weight"), A::N(0.0)]);
}
/// StatsSystem.GetStatValue(Cast<StatsObjectID>(ownerEntityID), stat).
fn owner_stat(si: &Handle, stat: &str) -> f64 {
    let id = rt::field(si, "ownerEntityID").map(|v| {
        // SAFETY: an EntityID: its hash.
        unsafe { *(v.ptr as *const u64) }
    });
    let Some(id) = id else { return 0.0 };
    let stats = rt::obj(si, "GetStatsSystem", &[]);
    let sid = [id, 0u64];
    num(&stats, "GetStatValue", &[A::P(sid.as_ptr().cast()), A::N(rt::enum_value("gamedataStatType", stat))])
}
/// ReadyEvents.OnTick: the firearm pinned up, the pose state reported, the recoil additive at 0.
fn ready_tick(call: &mut Call) {
    let (sc, si) = (call.arg_handle(1), call.arg_handle(2));
    seen(0);
    set_raised(&sc, &si, true);
    publish_pose_state(&si);
    let f = super::lobby_ink::new("AnimFeature_WeaponHandlingStats");
    if f.is_null() {
        return;
    }
    super::menu_rt::hset(&f, "weaponRecoil", 0.0);
    super::menu_rt::hset(&f, "weaponSpread", owner_stat(&si, "SpreadAnimation"));
    let right = flag(&call.this(), "IsRightWeaponLogic", &[]);
    rt::call(&si, "SetAnimationParameterFeature", &[n(if right { "RightWeaponHandlingData" } else { "LeftWeaponHandlingData" }), A::H(&f), A::H(&owner(&si))]);
}

// ---------------------------------------------------------------- the plugin's reload window and pose state
/// VRPortReloadFinishIn: the logical duration plus the end time; -1 when unknown or a partial reload.
fn reload_finish_in(this: &Handle, sc: &Handle, si: &Handle) -> f64 {
    let logical_name = rt::call(this, "GetReloadLogicalDuration", &[]).map(|o| {
        // SAFETY: a CName result.
        unsafe { *(o.as_ptr() as *const u64) }
    });
    let empty_name = rt::call(this, "GetEmptyReload", &[]).map(|o| {
        // SAFETY: as above.
        unsafe { *(o.as_ptr() as *const u64) }
    });
    let (Some(logical_name), Some(empty_name)) = (logical_name, empty_name) else { return -1.0 };
    let Some(logical) = rt::call(sc, "GetPermanentFloatParameter", &[A::Name(logical_name)]) else { return -1.0 };
    let lv = Val::of(&logical);
    if !lv.f("valid").is_some_and(|v| v.bool()) {
        return -1.0;
    }
    let value = lv.f("value").and_then(|v| v.num()).unwrap_or(0.0);
    let weapon = rt::field(si, "owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let data = rt::field(this, "m_animReloadData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let amount = rt::field(&data, "amountToReload").and_then(|v| v.num()).unwrap_or(0.0);
    if amount > 0.0 && super::menu_rt::is_a(&weapon, "WeaponObject") {
        let st = |f: &str| rt::static_num("WeaponObject", f, &[A::H(&weapon)]).unwrap_or(0.0);
        if st("GetMagazineAmmoCount") + amount < st("GetMagazineCapacity") {
            return -1.0;
        }
    }
    let empty = flag(sc, "GetBoolParameter", &[A::Name(empty_name), A::B(true)]);
    value + owner_stat(si, if empty { "EmptyReloadEndTime" } else { "ReloadEndTime" }).max(0.0)
}
fn reload_animation(active: bool, finish_in: f64) {
    native::<i32>("SetVRReloadAnimation", &[crate::script_call::Value::Bool(active), crate::script_call::Value::Float(finish_in as f32)]);
}
fn reload_enter(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    reload_animation(true, reload_finish_in(&call.this(), &sc, &si));
}
fn reload_exit(_call: &mut Call) {
    reload_animation(false, 0.0);
}
/// VRPortPublishWeaponPoseState: the hand's weapon state and the aim-in time left, to the plugin.
fn publish_pose_state(si: &Handle) {
    if si.is_null() || rt::field(si, "localBlackboard").is_none_or(|v| v.handle().is_null()) {
        return;
    }
    let left = super::left_handed::in_hand(&owner(si));
    let state = psm(si, "GetInt", if left { "LeftWeapon" } else { "RightWeapon" });
    let aim = psm(si, "GetFloat", "AimInTimeRemaining");
    seen(4);
    native::<i32>("SetVRWeaponPoseState", &[crate::script_call::Value::Int32(state as i32), crate::script_call::Value::Float(aim as f32)]);
}
fn raise_transition(active: bool) {
    native::<i32>("SetVRWeaponRaiseTransition", &[crate::script_call::Value::Int32(i32::from(active))]);
}
fn pose_enter_settled(call: &mut Call) {
    raise_transition(false);
    publish_pose_state(&call.arg_handle(1));
}
fn pose_enter_raising(call: &mut Call) {
    raise_transition(true);
    publish_pose_state(&call.arg_handle(1));
}
fn aiming_state(call: &mut Call) {
    publish_pose_state(&call.arg_handle(1));
}
fn aiming_update(call: &mut Call) {
    publish_pose_state(&call.arg_handle(2));
}

// ---------------------------------------------------------------- the VR finger's cadence
/// The weapon's fields of the scripts (WeaponObject's vrLastFingerAction ... vrAutoReleased), by the weapon object.
#[derive(Clone, Copy, Default)]
struct Finger {
    last_action: u64,
    await_press_count: bool,
    has_rejected: bool,
    rejected_count: u32,
    reject_echo: bool,
    auto_slot: i32,
    auto_press_count: u32,
    auto_released: bool,
}
static FINGERS: Mutex<Option<HashMap<usize, Finger>>> = Mutex::new(None);
/// PlayerPuppet's vrNextLeftShotTime / vrNextRightShotTime: (the player object, [left, right]).
static NEXT_SHOT: Mutex<(usize, [f64; 2])> = Mutex::new((0, [0.0; 2]));
/// SemiAutoDecisions' vrPlayerListener, by the decisions object.
static LISTENERS: Mutex<Option<std::collections::HashSet<usize>>> = Mutex::new(None);

fn with_finger<T>(weapon: &Handle, f: impl FnOnce(&mut Finger) -> T) -> Option<T> {
    if weapon.is_null() {
        return None;
    }
    let mut g = FINGERS.lock().unwrap_or_else(|p| p.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() > 64 && !m.contains_key(&(weapon.instance as usize)) {
        m.clear();
    }
    Some(f(m.entry(weapon.instance as usize).or_default()))
}
fn trigger_enabled() -> bool {
    cvr_rig::vrik::xinput::cvr_input_weapon_trigger_enabled()
}
fn finger_action() -> u64 {
    cvr_rig::vrik::xinput::cvr_input_current_finger_action()
}
/// A CName a transition answers (GetQuestForceShootName, GetLastChargePressCount, GetIsChargedFullAutoName).
fn name_of(this: &Handle, getter: &str) -> Option<u64> {
    // SAFETY: a CName result.
    rt::call(this, getter, &[]).map(|o| unsafe { *(o.as_ptr() as *const u64) })
}
/// StateResultBool / StateResultInt as (valid, value).
fn state_result(o: Option<crate::script_call::Owned>) -> (bool, f64) {
    o.map_or((false, 0.0), |o| {
        let v = Val::of(&o);
        (v.f("valid").is_some_and(|x| x.bool()), v.f("value").and_then(|x| x.num()).unwrap_or(0.0))
    })
}
fn last_charge_press(this: &Handle, sc: &Handle) -> (bool, f64) {
    state_result(name_of(this, "GetLastChargePressCount").and_then(|p| rt::call(sc, "GetPermanentIntParameter", &[A::Name(p)])))
}
fn quest_forced(this: &Handle, sc: &Handle) -> bool {
    state_result(name_of(this, "GetQuestForceShootName").and_then(|p| rt::call(sc, "GetTemporaryBoolParameter", &[A::Name(p)]))).1 != 0.0
}
/// localBlackboard.GetBool(PlayerStateMachine.<field>).
fn psm_bool(si: &Handle, field: &'static str) -> bool {
    psm(si, "GetBool", field) != 0.0
}
fn ranged(si: &Handle, f: &str) -> f64 {
    num(si, f, &[n("RangedAttack")])
}
fn frenzy(this: &Handle, si: &Handle) -> bool {
    flag(this, "IsInValentinosFrenzy", &[A::H(si)])
        || rt::static_call("StatusEffectSystem", "ObjectHasStatusEffect", &[A::H(&owner(si)), tdb("BaseStatusEffect.ValentinosFrenzySE")])
            .is_some_and(|o| Val::of(&o).bool())
}
fn last_used_kbm(si: &Handle) -> bool {
    flag(&owner(si), "PlayerLastUsedKBM", &[])
}
/// EngineTime.ToDouble(GameInstance.GetEngineTime(game)).
fn engine_time() -> f64 {
    super::menu_rt::static_gi("GameInstance", "GetEngineTime", &[])
        .and_then(|t| rt::static_num("EngineTime", "ToDouble", &[A::P(t.as_ptr().cast())]))
        .unwrap_or(0.0)
}
/// VRCadencePlayer: the player whose shots the finger paces; null with the trigger route off or a quest forcing the shot.
fn cadence_player(this: &Handle, sc: &Handle, si: &Handle) -> Handle {
    if !trigger_enabled() || quest_forced(this, sc) || psm_bool(si, "QuestForceShoot") {
        return Handle::null();
    }
    let o = owner(si);
    if is_player(&o) {
        o
    } else {
        Handle::null()
    }
}
/// VRShotIntervalReady: the hand's 100 ms since its last shot (kept on the player: changing items or modes cannot
/// reset a hand).
fn shot_interval_ready(this: &Handle, sc: &Handle, si: &Handle) -> bool {
    let player = cadence_player(this, sc, si);
    if player.is_null() {
        return true;
    }
    let left = flag(this, "IsLeftWeaponLogic", &[]);
    let (who, deadlines) = *NEXT_SHOT.lock().unwrap_or_else(|p| p.into_inner());
    let deadline = if who == player.instance as usize { deadlines[usize::from(!left)] } else { 0.0 };
    engine_time() >= deadline
}
/// VRDualHandReleased: the merged attack action can stay high when one of two fingers lifts; the hand read at admission.
fn dual_hand_released(this: &Handle, sc: &Handle, si: &Handle) -> bool {
    let (active, left, right) = super::reds_hooks::dual_triggers();
    if !active || cadence_player(this, sc, si).is_null() || frenzy(this, si) {
        return false;
    }
    if flag(this, "IsLeftWeaponLogic", &[]) {
        !left
    } else {
        !right
    }
}
/// VRFingerSemiAuto: a semi-automatic of the player's, the trigger route on, no forced shot, not mounted, not in the
/// scanner, not in Valentino's Frenzy.
fn finger_semi_auto(this: &Handle, sc: &Handle, si: &Handle) -> bool {
    trigger_enabled()
        && is_player(&owner(si))
        && flag(si, "IsTriggerModeActive", &[A::N(rt::enum_value("gamedataTriggerMode", "SemiAuto"))])
        && !quest_forced(this, sc)
        && !psm_bool(si, "QuestForceShoot")
        && !psm_bool(si, "MountedToVehicle")
        && !flag(this, "IsInVisionModeActiveState", &[A::H(sc), A::H(si)])
        && !frenzy(this, si)
}
fn has_fresh_action(weapon: &Handle) -> bool {
    let action = finger_action();
    action != 0 && with_finger(weapon, |w| action != w.last_action).unwrap_or(false)
}
fn consume_finger_action(weapon: &Handle, awaiting_native_press: bool) {
    let action = finger_action();
    with_finger(weapon, |w| {
        w.last_action = action;
        w.await_press_count = action != 0 && awaiting_native_press;
        w.has_rejected = false;
    });
}
fn rejected_press(w: &Finger, count: u32, keyboard: bool) -> bool {
    w.has_rejected && (w.rejected_count == count || (!keyboard && w.reject_echo && w.rejected_count.wrapping_add(1) == count))
}
/// VRDiscardEarlyPress: a press the game kept from before the interval is remembered as rejected (and, for a pull XR
/// has not handed the game yet, the count that follows it too).
fn discard_early_press(weapon: &Handle, si: &Handle, last_shot_press: (bool, f64)) {
    let held = ranged(si, "GetActionValue") > 0.0;
    let count = ranged(si, "GetActionPressCount") as u32;
    let action = finger_action();
    with_finger(weapon, |w| {
        let fresh = action != 0 && action != w.last_action;
        let known = rejected_press(w, count, false);
        if !fresh && (!held || known) {
            return;
        }
        w.reject_echo = fresh && (!held || known || (last_shot_press.0 && last_shot_press.1 == f64::from(count as i32)));
        if fresh {
            w.last_action = action;
        }
        w.has_rejected = true;
        w.rejected_count = count;
    });
}
/// VRAutoReleaseBlocks: the native press the VR finger owns, released once its physical trigger reads under 0.20.
fn auto_release_blocks(weapon: &Handle, slot: i32, count: u32, travel: f32, fresh: bool) -> bool {
    with_finger(weapon, |w| {
        if w.auto_slot != slot || w.auto_press_count != count {
            w.auto_slot = 0;
            w.auto_released = false;
        }
        // A delayed first FullAuto transition can outlast the fresh-press token: full travel still names the source.
        if fresh || (w.auto_slot == 0 && travel >= 0.85) {
            w.auto_slot = slot;
            w.auto_press_count = count;
            w.auto_released = false;
        }
        if w.auto_slot == slot && travel <= 0.20 {
            w.auto_released = true;
        }
        w.auto_released
    })
    .unwrap_or(false)
}
/// VRReleasedFullAuto (main's vrport_full_auto_release.reds, with vrport_weapon_trigger.reds' wrap): the game can miss
/// the release callback while the physical trigger already reads zero; with two guns, the hand let go.
fn released_full_auto(this: &Handle, sc: &Handle, si: &Handle) -> bool {
    let full_auto = flag(si, "IsTriggerModeActive", &[A::N(rt::enum_value("gamedataTriggerMode", "FullAuto"))]);
    let charged = name_of(this, "GetIsChargedFullAutoName").is_some_and(|p| flag(sc, "GetBoolParameter", &[A::Name(p), A::B(true)]));
    let base = (|| {
        if !full_auto
            || !trigger_enabled()
            || charged
            || !is_player(&owner(si))
            || super::reds_hooks::dual_triggers().0
            || flag(this, "IsInVisionModeActiveState", &[A::H(sc), A::H(si)])
            || frenzy(this, si)
            || psm_bool(si, "MountedToVehicle")
            || psm_bool(si, "QuestForceShoot")
            || quest_forced(this, sc)
        {
            return false;
        }
        let weapon = rt::obj(this, "GetWeaponObject", &[A::H(si)]);
        if weapon.is_null() {
            return false;
        }
        // kLeftTriggerAnalog / kRightTriggerAnalog (SharedSlots.hpp) are physical.
        let slot = if native::<bool>("VRLeftHanded", &[]) == Some(true) { 154 } else { 160 };
        let travel = rt::global_call("GetVRSharedSlot", &[A::N(f64::from(slot))]).and_then(|o| Val::of(&o).num()).unwrap_or(0.0) as f32;
        auto_release_blocks(&weapon, slot, ranged(si, "GetActionPressCount") as u32, travel, finger_action() != 0)
    })();
    base || (full_auto && !charged && dual_hand_released(this, sc, si))
}
/// VRCanBeginCharge: entry to a charge (and a trigger mode's change to it) admitted by the interval and a fresh pull.
fn can_begin_charge(this: &Handle, sc: &Handle, si: &Handle) -> bool {
    let weapon = rt::obj(this, "GetWeaponObject", &[A::H(si)]);
    if cadence_player(this, sc, si).is_null() {
        return true;
    }
    if dual_hand_released(this, sc, si) {
        return false;
    }
    if !shot_interval_ready(this, sc, si) {
        discard_early_press(&weapon, si, last_charge_press(this, sc));
        return false;
    }
    if finger_action() != 0 {
        return has_fresh_action(&weapon);
    }
    let count = ranged(si, "GetActionPressCount") as u32;
    let kbm = last_used_kbm(si);
    ranged(si, "GetActionValue") > 0.0 && !with_finger(&weapon, |w| rejected_press(w, count, kbm)).unwrap_or(false)
}

/// WeaponTransition.CanPerformNextShotInSequence: CycleRound's exit and the semi-automatic's entry both ask it; the
/// finger's route replaces the mouse and pad CycleTime with its accepted-shot interval.
fn next_shot(call: &mut Call) {
    let (this, weapon, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1), call.arg_handle(2));
    let finger = finger_semi_auto(&this, &sc, &si);
    let ready = shot_interval_ready(&this, &sc, &si);
    if !ready && finger && flag(&this, "ShouldShoot", &[A::H(&si)]) {
        discard_early_press(&weapon, &si, last_charge_press(&this, &sc));
    }
    let v = ready && (finger || {
        call.original();
        call.result_bool()
    });
    call.set_result_bool(v);
}
/// WeaponTransition.IsSemiAutoAction: a fresh physical pull, or (no pull published) the game's own press once.
fn semi_auto_action(call: &mut Call) {
    let (this, weapon, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1), call.arg_handle(2));
    if dual_hand_released(&this, &sc, &si) {
        call.set_result_bool(false);
        return;
    }
    if !finger_semi_auto(&this, &sc, &si) {
        return; // the original
    }
    let should = flag(&this, "ShouldShoot", &[A::H(&si)]);
    if finger_action() != 0 {
        let v = should && has_fresh_action(&weapon);
        call.set_result_bool(v);
        return;
    }
    if !should || ranged(&si, "GetActionValue") <= 0.0 {
        call.set_result_bool(false);
        return;
    }
    let (valid, last) = last_charge_press(&this, &sc);
    let count = ranged(&si, "GetActionPressCount") as u32 as i32;
    let kbm = last_used_kbm(&si);
    let finger = with_finger(&weapon, |w| *w).unwrap_or_default();
    if rejected_press(&finger, count as u32, kbm) || (valid && last == f64::from(count)) {
        call.set_result_bool(false);
        return;
    }
    // ShootEvents sampled the previous engine count if XR won the race: the following pad count acknowledges that same
    // round. A new physical pull has its own identity and was handled above, even if the echo is lost.
    if finger.await_press_count && valid && last + 1.0 == f64::from(count) && !kbm && !psm_bool(&si, "IsDualWielding") {
        call.set_result_bool(false);
        return;
    }
    // ShootEvents consumes the per-hand press count: a still-held edge across a state transition, never an old request.
    let v = flag(&si, "IsActionJustPressed", &[n("RangedAttack")]) || ranged(&si, "GetActionStateTime") <= 0.10;
    call.set_result_bool(v);
}
fn full_auto_action(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(1), call.arg_handle(2));
    call.original();
    let v = call.result_bool() && !released_full_auto(&this, &sc, &si);
    call.set_result_bool(v);
}
fn full_auto_to_shoot(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    call.original();
    let v = call.result_bool() && !released_full_auto(&this, &sc, &si);
    call.set_result_bool(v);
}
fn full_auto_to_ready(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    call.original();
    let v = call.result_bool() || released_full_auto(&this, &sc, &si);
    call.set_result_bool(v);
}
/// SemiAutoDecisions: the native input listener can stay silent for the first fresh press after the pause menu; the
/// player's decision stays eligible to observe the routed XR press. The state machine still owns the entry, the ammo,
/// the mechanical transitions and the shot.
fn listener(this: &Handle) -> bool {
    LISTENERS.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|s| s.contains(&(this.instance as usize)))
}
fn semi_attach(call: &mut Call) {
    let (this, si) = (call.this(), call.arg_handle(1));
    let player = is_player(&owner(&si));
    {
        let mut g = LISTENERS.lock().unwrap_or_else(|p| p.into_inner());
        let s = g.get_or_insert_with(Default::default);
        if s.len() > 64 {
            s.clear();
        }
        if player {
            s.insert(this.instance as usize);
        } else {
            s.remove(&(this.instance as usize));
        }
    }
    if player {
        consume_finger_action(&rt::obj(&this, "GetWeaponObject", &[A::H(&si)]), false);
        rt::call(&this, "EnableOnEnterCondition", &[A::B(true)]);
    }
}
fn semi_keep_enabled(call: &mut Call) {
    let this = call.this();
    if listener(&this) {
        rt::call(&this, "EnableOnEnterCondition", &[A::B(true)]);
    }
}
fn semi_enter(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    let input = rt::field(&this, "m_inputValue").or_else(|| rt::field(&this, "inputValue")).and_then(|v| v.num()).unwrap_or(0.0);
    if listener(&this) && input <= 0.0 && finger_action() != 0 && finger_semi_auto(&this, &sc, &si) {
        let weapon = rt::obj(&this, "GetWeaponObject", &[A::H(&si)]);
        let v = flag(&this, "ShouldShoot", &[A::H(&si)])
            && has_fresh_action(&weapon)
            && flag(&this, "CanPerformNextSemiAutoShot", &[A::H(&weapon), A::H(&sc), A::H(&si)]);
        call.set_result_bool(v);
    }
}
/// ShootEvents.OnEnter: the hand's next shot 100 ms on, and the finger's pull consumed by the shot it made.
fn shot_taken(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    let player = cadence_player(&this, &sc, &si);
    if !player.is_null() {
        let deadline = engine_time() + 0.10;
        let left = flag(&this, "IsLeftWeaponLogic", &[]);
        let mut g = NEXT_SHOT.lock().unwrap_or_else(|p| p.into_inner());
        if g.0 != player.instance as usize {
            *g = (player.instance as usize, [0.0; 2]);
        }
        g.1[usize::from(!left)] = deadline;
    }
    if finger_semi_auto(&this, &sc, &si) {
        consume_finger_action(&rt::obj(&this, "GetWeaponObject", &[A::H(&si)]), ranged(&si, "GetActionValue") <= 0.0);
    }
}
/// BurstDecisions.ToShoot: each committed follow-up of a burst has its native timer and the hand's interval.
fn burst_to_shoot(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    call.original();
    let v = call.result_bool() && shot_interval_ready(&this, &sc, &si);
    call.set_result_bool(v);
}
/// ChargeDecisions.EnterCondition and CycleTriggerModeDecisions.ToCharge: entry to a charge gated; once admitted the
/// release, the perfect charge and fire-when-ready stay native.
fn charge_gate(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    call.original();
    let v = call.result_bool() && can_begin_charge(&this, &sc, &si);
    call.set_result_bool(v);
}
fn no_fire_delay(call: &mut Call) {
    let (this, sc, si) = (call.this(), call.arg_handle(0), call.arg_handle(1));
    if finger_semi_auto(&this, &sc, &si) {
        call.set_arg_float(2, 0.0);
    }
}

/// The hooks, in reds_hooks' table.
pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 47] = [
    ("EquipmentBaseTransition", "HandleWeaponEquip", Kind::Before, first_equip_prevent),
    ("EquipmentBaseTransition", "HandleWeaponEquip", Kind::After, equip_raise),
    ("ReadyEvents", "OnTick", Kind::Before, no_idle_break),
    ("ReadyEvents", "OnTick", Kind::After, ready_tick),
    ("PublicSafeToReadyEvents", "OnExit", Kind::After, psr_exit),
    ("QuickMeleeEvents", "OnEnter", Kind::After, quick_melee_enter),
    ("ShootEvents", "OnEnter", Kind::Before, shot_taken),
    ("ShootEvents", "OnEnter", Kind::After, shoot_enter),
    ("PublicSafeEvents", "RequestWeaponUnequipNotifyUpperBody", Kind::Override, no_auto_unequip),
    ("UpperBodyEventsTransition", "UpdateSwitchItem", Kind::Override, update_switch_item),
    ("PublicSafeDecisions", "ShouldLeaveSafe", Kind::Override, should_leave_safe),
    ("DefaultTransition", "ShouldEnterSafe", Kind::Override, should_enter_safe),
    ("NoAmmoDecisions", "ToReload", Kind::Override, to_reload),
    ("AimWalkDecisions", "EnterCondition", Kind::Override, aim_walk),
    ("EquipmentBaseTransition", "GetWeaponEquipDuration", Kind::Override, zero_duration),
    ("EquipmentBaseTransition", "GetWeaponUnEquipDuration", Kind::Override, zero_duration),
    ("FirstEquipSystem", "HasPlayedFirstEquip", Kind::Override, first_equip_played),
    ("SprintEvents", "OnEnter", Kind::After, sprint_enter),
    ("SprintEvents", "OnUpdate", Kind::After, sprint_update),
    ("SprintEvents", "OnExit", Kind::After, sprint_exit),
    ("SprintEvents", "OnForcedExit", Kind::After, sprint_exit),
    ("DefaultTransition", "DisableCameraBobbing", Kind::Before, bob_off_transition),
    ("PlayerPuppet", "DisableCameraBobbing", Kind::Before, bob_off_player),
    ("PlayerPuppet", "UpdatePlayerSettings", Kind::After, player_settings),
    ("ReloadEvents", "OnEnter", Kind::After, reload_enter),
    ("ReloadEvents", "OnExit", Kind::After, reload_exit),
    ("ReloadEvents", "OnForcedExit", Kind::After, reload_exit),
    ("ReadyEvents", "OnEnter", Kind::After, pose_enter_settled),
    ("SafeEvents", "OnEnter", Kind::After, pose_enter_settled),
    ("PublicSafeEvents", "OnEnter", Kind::After, pose_enter_settled),
    ("PublicSafeToReadyEvents", "OnEnter", Kind::After, pose_enter_raising),
    ("AimingStateEvents", "OnEnter", Kind::After, aiming_state),
    ("AimingStateEvents", "OnUpdate", Kind::After, aiming_update),
    ("AimingStateEvents", "OnExit", Kind::After, aiming_state),
    ("WeaponTransition", "CanPerformNextShotInSequence", Kind::Override, next_shot),
    ("WeaponTransition", "IsSemiAutoAction", Kind::Override, semi_auto_action),
    ("WeaponTransition", "StartShootingSequence", Kind::Before, no_fire_delay),
    ("WeaponTransition", "IsFullAutoAction", Kind::Override, full_auto_action),
    ("FullAutoDecisions", "ToShoot", Kind::Override, full_auto_to_shoot),
    ("FullAutoDecisions", "ToReady", Kind::Override, full_auto_to_ready),
    ("SemiAutoDecisions", "OnAttach", Kind::After, semi_attach),
    ("SemiAutoDecisions", "RefreshEnterCondition", Kind::After, semi_keep_enabled),
    ("SemiAutoDecisions", "OnQuestForcedShoot", Kind::After, semi_keep_enabled),
    ("SemiAutoDecisions", "EnterCondition", Kind::Override, semi_enter),
    ("BurstDecisions", "ToShoot", Kind::Override, burst_to_shoot),
    ("ChargeDecisions", "EnterCondition", Kind::Override, charge_gate),
    ("CycleTriggerModeDecisions", "ToCharge", Kind::Override, charge_gate),
];

// ---------------------------------------------------------------- the self-test
/// What the hooks did, for the self-test: [ready ticks, raises, lowers, sprint blocks, pose states, equip durations,
/// idle-break ticks held off, first equips that still played, weapon equips].
pub(super) static SEEN: [std::sync::atomic::AtomicU32; 9] = [const { std::sync::atomic::AtomicU32::new(0) }; 9];
fn seen(i: usize) {
    SEEN[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
/// A development check (CVR_NOANIMS_TEST, inherited by the game from the probe session): 45 s into the loaded save
/// the first weapon slot is drawn, at 52 s what the hooks did is logged and the weapon put away.
pub(super) fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    if std::env::var_os("CVR_NOANIMS_TEST").is_none() {
        return;
    }
    let pl = super::belt::player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 2 {
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
        // as a held weapon slot button asks for it: with the first-equip animation
        let c = super::game_system("GetScriptableSystemsContainer");
        let equipment = rt::obj(&c, "Get", &[n("EquipmentSystem")]);
        let req = super::lobby_ink::new("EquipmentSystemWeaponManipulationRequest");
        if !equipment.is_null() && !req.is_null() {
            super::menu_rt::hset(&req, "requestType", super::menu_rt::ev("EquipmentManipulationAction", "RequestWeaponSlot1"));
            super::menu_rt::hset(&req, "equipAnimType", super::menu_rt::ev("gameEquipAnimationType", "FirstEquip"));
            super::menu_main::set_handle_prop(&req, "owner", &pl);
            rt::call(&equipment, "QueueRequest", &[A::H(&req)]);
        }
        cvr_host::log!("[no-anims] self-test: weapon slot 1 requested with the first-equip animation (from Rust)\n");
        next = 1;
    } else if step == 1 && t > 87.0 {
        let s: Vec<u32> = SEEN.iter().map(|a| a.load(std::sync::atomic::Ordering::Relaxed)).collect();
        let ts = super::belt::transactions();
        let right = super::belt::h(&ts, "GetItemInSlot", &[crate::script_call::Value::Handle(&pl), super::belt::tdb("AttachmentSlots.WeaponRight")]);
        cvr_host::log!(
            "[no-anims] self-test: right hand {} ; ready ticks {} raises {} lowers {} sprint blocks {} pose states {} equip durations {} ; idle-break ticks held off {} ; weapon equips {}, of them with the first-equip animation {} (from Rust)\n",
            if right.is_null() { "empty" } else { "a weapon" },
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[8], s[7]
        );
        super::holster::equip_request(&pl, "UnequipWeapon");
        cvr_host::log!("[no-anims] self-test done (from Rust)\n");
        next = 2;
    }
    T.with(|c| c.set((t, next, first)));
}
