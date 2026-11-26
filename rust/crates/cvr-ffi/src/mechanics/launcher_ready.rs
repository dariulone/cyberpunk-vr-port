//! The projectile launcher kept ready (formerly CyberpunkVRPort_Engine's vrport_cyberware_launcher_state.reds): a
//! selected launcher stays deployed after its native shot or cancel. The left-hand cyberware state machine's state
//! vrLauncherReady (its definition: native_tweaks.rs) has the classes VrLauncherReadyDecisions / VrLauncherReadyEvents,
//! made here (script_classes.rs), and the decisions it is entered from get ToVrLauncherReady. The native action states
//! still own projectiles, charges and costs; no held input is made up and no equip is requested again.
//!
//! The player's vrLauncher* flags (@addField PlayerPuppet) are Rust's: one player's at a time, cleared when another
//! player is asked about or the player detaches.
use super::lobby_rtti::{self as rt, Val, A};
use super::melee::n;
use super::menu_rt as r;
use crate::script_call::Value;
use crate::script_classes::{Added, ClassDef, Method, FUNC, VIRTUAL};
use crate::script_hooks::{Call, Sig};
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

pub(super) const CONSUME_SWITCH_RELEASE: &str = "vrLauncherConsumeSwitchRelease";
pub(super) const KEEP_OPEN: &str = "vrLauncherKeepOpen";
pub(super) const SELECTED_AS_WEAPON: &str = "vrLauncherSelectedAsWeapon";
pub(super) const FROM_WEAPON_CYCLE: &str = "vrLauncherFromWeaponCycle";
pub(super) const SHOT_REJECTED: &str = "vrLauncherShotRejected";
pub(super) const SHOT_QUEUED: &str = "vrLauncherShotQueued";
/// PlayerPuppet.vrArmBLauncherPending (main's vrport_launcher_selection.reds): B's draw asked the launcher (arm_b.rs).
pub(super) const ARM_B_LAUNCHER_PENDING: &str = "vrArmBLauncherPending";
const NAMES: [&str; 7] = [CONSUME_SWITCH_RELEASE, KEEP_OPEN, SELECTED_AS_WEAPON, FROM_WEAPON_CYCLE, SHOT_REJECTED, SHOT_QUEUED, ARM_B_LAUNCHER_PENDING];

/// (the player's instance, its flags' bits).
static FLAGS: Mutex<(usize, u8)> = Mutex::new((0, 0));
fn bit(f: &str) -> u8 {
    NAMES.iter().position(|x| *x == f).map_or(0, |i| 1 << i)
}
/// The player's flag (false for another player than the one last set, or none).
pub(super) fn flag(pl: &Handle, f: &str) -> bool {
    let s = FLAGS.lock().unwrap_or_else(|p| p.into_inner());
    !pl.is_null() && s.0 == pl.instance as usize && s.1 & bit(f) != 0
}
pub(super) fn set_flag(pl: &Handle, f: &str, v: bool) {
    if pl.is_null() {
        return;
    }
    let mut s = FLAGS.lock().unwrap_or_else(|p| p.into_inner());
    if s.0 != pl.instance as usize {
        *s = (pl.instance as usize, 0);
    }
    if v {
        s.1 |= bit(f);
    } else {
        s.1 &= !bit(f);
    }
}
/// The player detached: its fields went with it.
pub(super) fn forget() {
    *FLAGS.lock().unwrap_or_else(|p| p.into_inner()) = (0, 0);
}

type Item = [u64; 2];
fn item_of(o: &crate::script_call::Owned) -> Item {
    // SAFETY: an ItemID result.
    unsafe { *(o.as_ptr() as *const Item) }
}
fn b(h: &Handle, name: &str, args: &[A]) -> bool {
    r::b(h, name, args)
}
fn sb(class: &str, name: &str, args: &[A]) -> bool {
    rt::static_call(class, name, args).is_some_and(|o| Val::of(&o).bool())
}
/// scriptInterface.executionOwner as PlayerPuppet.
fn player(si: &Handle) -> Handle {
    let o = rt::field(si, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if r::is_a(&o, "PlayerPuppet") {
        o
    } else {
        Handle::null()
    }
}

/// LeftHandCyberwareTransition.VrLauncherCanStay: the selected launcher is still the left hand's and nothing asks it
/// to go.
fn can_stay(this: &Handle, ctx: &Handle, si: &Handle) -> bool {
    let pl = player(si);
    let weapon = rt::obj(this, "GetLeftHandWeaponObject", &[A::H(si)]);
    if pl.is_null() || !flag(&pl, KEEP_OPEN) || weapon.is_null() {
        return false;
    }
    let item = super::melee::item_id(&weapon);
    let launcher = sb("WeaponObject", "IsOfType", &[A::P(item.as_ptr().cast()), A::N(rt::enum_value("gamedataItemType", "Cyb_Launcher"))]);
    if !launcher
        || b(this, "ShouldInstantlyUnequipCyberware", &[A::H(si), A::H(ctx)])
        || b(ctx, "GetBoolParameter", &[n("InterruptLeftHandAction"), A::B(false)])
        || b(this, "IsInVisionModeActiveState", &[A::H(ctx), A::H(si)])
        || b(this, "GetCancelChargeButtonInput", &[A::H(si)])
    {
        return false;
    }
    if b(si, "IsActionJustPressed", &[n("SwitchItem")]) || b(si, "IsActionJustTapped", &[n("HolsterWeapon")]) || b(si, "IsActionJustReleased", &[n("UseConsumable")]) {
        return false;
    }
    let data = rt::static_obj("EquipmentSystem", "GetData", &[A::H(&pl)]);
    let selected = rt::call(&data, "GetSlotActiveItem", &[A::N(rt::enum_value("EquipmentManipulationRequestSlot", "Left"))]).map_or([0, 0], |o| item_of(&o));
    selected[0] != 0 && super::melee::same_item(&selected, &item)
}
fn args(call: &Call) -> (Handle, Handle, Handle) {
    (call.this(), call.arg_handle(0), call.arg_handle(1))
}

// ---------------------------------------------------------------- ToVrLauncherReady of the states it is entered from
/// Each native action's completion kept: the transition wins over the automatic unequip only for the launcher still
/// selected.
fn ready_then(call: &mut Call, next: &str) {
    let (this, ctx, si) = args(call);
    let v = can_stay(&this, &ctx, &si) && b(&this, next, &[A::H(&ctx), A::H(&si)]);
    call.set_result_bool(v);
}
fn quick_action_ready(call: &mut Call) {
    ready_then(call, "ToLeftHandCyberwareUnequip");
}
fn charge_ready(call: &mut Call) {
    ready_then(call, "ToLeftHandCyberwareWaitForUnequip");
}
fn wait_for_unequip_ready(call: &mut Call) {
    ready_then(call, "ToLeftHandCyberwareUnequip");
}
/// LeftHandCyberwareEquipDecisions.ToVrLauncherReady: a launcher equipped as the weapon is kept open, entered once its
/// equip has run.
fn equip_ready(call: &mut Call) {
    let (this, ctx, si) = args(call);
    let pl = player(&si);
    let selected = flag(&pl, SELECTED_AS_WEAPON);
    if selected && sb("LeftHandCyberwareHelper", "IsProjectileLauncherEquipped", &[A::H(&si)]) {
        set_flag(&pl, KEEP_OPEN, true);
    }
    let v = selected
        && can_stay(&this, &ctx, &si)
        && rt::num(&this, "GetInStateTime", &[]).unwrap_or(0.0) >= rt::num(&this, "GetEquipDuration", &[A::H(&si)]).unwrap_or(f64::MAX);
    call.set_result_bool(v);
}

// ---------------------------------------------------------------- VrLauncherReadyDecisions
fn armed(ctx: &Handle) -> bool {
    b(ctx, "GetBoolParameter", &[n("vrLauncherInputArmed"), A::B(true)])
}
fn has_charge(si: &Handle) -> bool {
    sb("LeftHandCyberwareHelper", "ProjectileLauncherHasCharge", &[A::H(si)])
}
fn enter_condition(call: &mut Call) {
    count_asked();
    let (this, ctx, si) = args(call);
    call.set_result_bool(can_stay(&this, &ctx, &si));
}
fn to_unequip(call: &mut Call) {
    let (this, ctx, si) = args(call);
    call.set_result_bool(!can_stay(&this, &ctx, &si));
}
fn to_charge(call: &mut Call) {
    let (this, ctx, si) = args(call);
    let v = armed(&ctx) && has_charge(&si) && b(&this, "LeftHandCyberwareHasTag", &[A::H(&si), n("ChargeAction")]) && b(&si, "IsActionHeld", &[n("UseCombatGadget")]);
    call.set_result_bool(v);
}
fn to_quick_action(call: &mut Call) {
    let (_, ctx, si) = args(call);
    if !armed(&ctx) || !has_charge(&si) || b(&player(&si), "IsAimingAtFriendly", &[]) {
        call.set_result_bool(false);
        return;
    }
    let v = b(&si, "IsActionJustPressed", &[n("RangedAttack")]) || b(&si, "IsActionJustReleased", &[n("UseCombatGadget")]);
    call.set_result_bool(v);
}

// ---------------------------------------------------------------- VrLauncherReadyEvents
/// The parent's own function run on this object (redscript's `super.f(...)`).
fn parent_call(this: &Handle, f: &str, ctx: &Handle, si: &Handle) {
    let Some(func) = crate::script_call::class_any("LeftHandCyberwareEventsTransition").and_then(|c| c.function(red4ext_abi::cname::CName::hashed(f))) else { return };
    // SAFETY: the parent's function on its own object with the call's arguments.
    unsafe { crate::script_call::invoke_void(this.instance, func, &[Value::Handle(ctx), Value::Handle(si)]) };
}
fn set_armed(ctx: &Handle, v: bool) {
    rt::call(ctx, "SetPermanentBoolParameter", &[n("vrLauncherInputArmed"), A::B(v), A::B(true)]);
}
/// OnEnter: the native deployed visual without its charge pool, projectile preview, aiming camera or firing action.
fn events_enter(call: &mut Call) {
    count_entered();
    let (this, ctx, si) = args(call);
    set_armed(&ctx, false);
    let pl = player(&si);
    set_flag(&pl, SHOT_REJECTED, false);
    set_flag(&pl, SHOT_QUEUED, false);
    rt::call(&this, "ResetAnimFeatureParameters", &[A::H(&ctx)]);
    rt::call(&this, "SetAnimFeatureState", &[A::H(&ctx), A::N(2.0)]);
    rt::call(&this, "SetIsCharging", &[A::H(&ctx), A::B(true)]);
    rt::call(&this, "SetActionDuration", &[A::H(&ctx), A::N(2.0)]);
    rt::call(&this, "SetLeftHandItemTypeAndState", &[A::H(&si), A::N(2.0), A::N(2.0)]);
    rt::call(&this, "SetAnimEquipState", &[A::H(&si), A::B(true)]);
    if let Ok(defs) = super::boards::defs() {
        if let Ok(def) = super::boards::definition(&defs, "PlayerStateMachine") {
            if let Ok(id) = super::boards::field(def, "LeftHandCyberware") {
                rt::call(&this, "SetBlackboardIntVariable", &[A::H(&si), A::P(id.cast_const()), A::N(1.0)]);
            }
        }
    }
    rt::call(&si, "PushAnimationEvent", &[n("LeftHandCyberwareReady")]);
    parent_call(&this, "OnEnter", &ctx, &si);
    rt::call(&this, "UpdateAndSendChargeAnimData", &[A::N(1.0), A::B(false), A::H(&ctx), A::H(&si)]);
    call.skip();
}
/// OnUpdate: returning from a shot cannot recycle its release edge into another shot.
fn events_update(call: &mut Call) {
    let (ctx, si) = (call.arg_handle(1), call.arg_handle(2));
    let value = |a: &str| rt::num(&si, "GetActionValue", &[n(a)]).unwrap_or(0.0);
    if value("UseCombatGadget") == 0.0 && value("RangedAttack") == 0.0 {
        set_armed(&ctx, true);
    }
    call.skip();
}
fn events_exit(call: &mut Call) {
    let (this, ctx, si) = args(call);
    let pl = player(&si);
    if flag(&pl, KEEP_OPEN) && b(&si, "IsActionJustPressed", &[n("SwitchItem")]) {
        set_flag(&pl, CONSUME_SWITCH_RELEASE, true);
        rt::call(&this, "SendEquipmentSystemWeaponManipulationRequest", &[
            A::H(&si),
            A::N(rt::enum_value("EquipmentManipulationAction", "CycleNextWeaponWheelItem")),
            A::N(rt::enum_value("gameEquipAnimationType", "Default")),
        ]);
    }
    rt::call(&ctx, "RemovePermanentBoolParameter", &[n("vrLauncherInputArmed")]);
    rt::call(&this, "SetIsCharging", &[A::H(&ctx), A::B(false)]);
    rt::call(&this, "SetAnimFeatureState", &[A::H(&ctx), A::N(1.0)]);
    parent_call(&this, "OnExit", &ctx, &si);
    call.skip();
}

/// (stateContext, scriptInterface) -> Bool, as the machine's decisions are.
const DECISION: Sig = Sig::Like("LeftHandCyberwareQuickActionDecisions", "EnterCondition");
const EVENT: Sig = Sig::Like("LeftHandCyberwareEventsTransition", "OnEnter");
const UPDATE: Sig = Sig::Of(
    &[("timeDelta", "Float"), ("stateContext", "handle:gamestateMachineStateContextScript"), ("scriptInterface", "handle:gamestateMachineGameScriptInterface")],
    "",
);
const fn m(name: &'static str, sig: Sig, flags: u32, hook: crate::script_hooks::HookFn) -> Method {
    Method { name, sig, flags, hook }
}
pub(crate) const CLASSES: [ClassDef; 2] = [
    ClassDef {
        name: "VrLauncherReadyDecisions",
        parent: "LeftHandCyberwareTransition",
        methods: &[
            m("EnterCondition", DECISION, FUNC, enter_condition),
            m("ToLeftHandCyberwareUnequip", DECISION, FUNC, to_unequip),
            m("ToLeftHandCyberwareCharge", DECISION, FUNC, to_charge),
            m("ToLeftHandCyberwareQuickAction", DECISION, FUNC, to_quick_action),
        ],
    },
    ClassDef {
        name: "VrLauncherReadyEvents",
        parent: "LeftHandCyberwareEventsTransition",
        methods: &[m("OnEnter", EVENT, VIRTUAL, events_enter), m("OnUpdate", UPDATE, VIRTUAL, events_update), m("OnExit", EVENT, VIRTUAL, events_exit)],
    },
];
pub(crate) const ADDED: [Added; 5] = [
    Added { class: "LeftHandCyberwareQuickActionDecisions", methods: &[m("ToVrLauncherReady", DECISION, FUNC, quick_action_ready)] },
    Added { class: "LeftHandCyberwareChargeActionDecisions", methods: &[m("ToVrLauncherReady", DECISION, FUNC, charge_ready)] },
    Added { class: "LeftHandCyberwareChargeRepeatActionDecisions", methods: &[m("ToVrLauncherReady", DECISION, FUNC, charge_ready)] },
    Added { class: "LeftHandCyberwareWaitForUnequipDecisions", methods: &[m("ToVrLauncherReady", DECISION, FUNC, wait_for_unequip_ready)] },
    Added { class: "LeftHandCyberwareEquipDecisions", methods: &[m("ToVrLauncherReady", DECISION, FUNC, equip_ready)] },
];

/// How often the state was entered and its decisions asked (the self-test's evidence).
static ENTERED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static ASKED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub(super) fn count_entered() {
    ENTERED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
pub(super) fn count_asked() {
    ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// A development check (CVR_LAUNCHER_TEST): 45 s into the loaded save the launcher is installed (the console's
/// EquipCyberware), then the weapon wheel is cycled until it is the selected weapon; the state machine has to take
/// it into vrLauncherReady -- the state's OnEnter of Rust's class runs, its decisions are asked, the launcher kept open.
pub(super) fn self_test(dt: f64) {
    use std::sync::atomic::Ordering::Relaxed;
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize, f64)> = const { std::cell::Cell::new((0.0, 0, 0, 0.0)) };
    }
    if std::env::var_os("CVR_LAUNCHER_TEST").is_none() {
        return;
    }
    let pl = super::belt::player();
    let (t, mut step, first, mut next_at) = T.with(|c| c.get());
    if pl.is_null() || step >= 20 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first, next_at)));
        return;
    }
    let t = t + dt;
    let report = |what: &str| {
        cvr_host::log!(
            "[launcher] self-test {what}: cyberware {}, selected {} keep open {} from cycle {}, state entered {} decisions asked {} (from Rust)
",
            super::vrik_mirror::cyberware_status(),
            flag(&pl, SELECTED_AS_WEAPON),
            flag(&pl, KEEP_OPEN),
            flag(&pl, FROM_WEAPON_CYCLE),
            ENTERED.load(Relaxed),
            ASKED.load(Relaxed)
        );
    };
    if step == 0 && t > 45.0 {
        let r = super::vrik_mirror::equip_cyberware("launcher");
        cvr_host::log!("[launcher] self-test: EquipCyberware(launcher) -> {r:?} (from Rust)
");
        // the journal watch (journal_watch.rs) made and listening, as the VR menu's phone asks for it
        cvr_host::log!("[launcher] self-test: the journal watch listens: {} (from Rust)
", super::journal_watch::watch().is_some());
        step = 1;
        next_at = t + 15.0;
    } else if (1..12).contains(&step) && t > next_at {
        if ENTERED.load(Relaxed) > 0 {
            report("in the ready state");
            step = 12;
            next_at = t + 5.0;
        } else if step == 11 {
            report("gave up");
            step = 20;
        } else {
            super::holster::equip_request(&pl, "CycleNextWeaponWheelItem");
            report(&format!("wheel cycled ({step})"));
            step += 1;
            next_at = t + 4.0;
        }
    } else if step == 12 && t > next_at {
        report("5 s later");
        if let Some(w) = super::journal_watch::watch() {
            cvr_host::log!("[launcher] self-test: the journal watch's counters: updates {} scheduled {} (from Rust)
", w.updates, w.scheduled);
        }
        step = 20;
    }
    T.with(|c| c.set((t, step, first, next_at)));
}
