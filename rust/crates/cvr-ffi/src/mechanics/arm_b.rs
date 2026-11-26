//! mods/cet/CyberpunkVRPort_ArmCyberwareB and mods/redscript/CyberpunkVRPort_ArmCyberwareB (main's 0.2.4): B draws the
//! installed arm cyberware from empty hands and holsters it on the next press. Mantis blades, the monowire and the
//! gorilla arms are the active meleeware; the projectile launcher is the left hand's cyberware (asked with the
//! launcher's own restrictions, VRArmBLauncherAllowed; its selection's flag, vrArmBLauncherPending, tells the launcher
//! selection the request was a weapon's). Not with a weapon in hand (but fists), not in a vehicle (shared [31]) or
//! with a device screen up ([164]); a press every 0.75 s at most.
//!
//! THE INPUT GUARD (VRArmBInputGuard, a scripted class Rust defines): while an arm cyberware B can draw is installed,
//! the game's Dodge and ToggleCrouch are consumed when the VR port reports a physical B (shared [157]) -- the port's
//! own stick dash is a synthesised B and does not raise it -- and kept consumed until their release.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_classes::{ClassDef, Method, CB};
use crate::script_hooks::{Call, Sig};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

fn shared(i: i32) -> f32 {
    super::belt::shared(i)
}
fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
fn ev(e: &str, m: &str) -> f64 {
    rt::enum_value(e, m)
}

// ---------------------------------------------------------------- the input guard
/// The guard's capture of Dodge and ToggleCrouch (its instance's fields: one player, one guard).
static CAPTURED: Mutex<[bool; 2]> = Mutex::new([false; 2]);
fn on_action(call: &mut Call) {
    // ListenerAction and ListenerActionConsumer are native STRUCTS, passed in place: read as handles they had a
    // reference count taken at a word of the struct (a crash the first time an arm cyberware owner dodged)
    let (action, consumer) = (rt::listener_arg(call, 0), rt::listener_arg(call, 1));
    let name = rt::listener_name(action);
    let which = if name == CName::hashed("Dodge").0 {
        0
    } else if name == CName::hashed("ToggleCrouch").0 {
        1
    } else {
        call.set_result_bool(false);
        return;
    };
    let raw_down = shared(157) > 0.5;
    let mut captured = CAPTURED.lock().unwrap_or_else(|p| p.into_inner());
    if raw_down {
        captured[which] = true;
    }
    if captured[which] {
        rt::listener_static(true, "Consume", consumer);
        if rt::listener_static(false, "IsButtonJustReleased", action).is_some_and(|o| Val::of(&o).bool()) {
            captured[which] = false;
        }
        call.set_result_bool(true);
        return;
    }
    call.set_result_bool(false);
}
const ACTION: Sig = Sig::Of(&[("action", "gameinputScriptListenerAction"), ("consumer", "gameinputScriptListenerActionConsumer")], "Bool");
pub(crate) const CLASSES: [ClassDef; 1] = [ClassDef {
    name: "VRArmBInputGuard",
    parent: "IScriptable",
    methods: &[Method { name: "OnAction", sig: ACTION, flags: CB, hook: on_action }],
}];

// ---------------------------------------------------------------- the mod
struct State {
    previous_down: bool,
    cooldown: f64,
    launcher_pending: f64,
    /// The guard listening to the player's input, and the player it listens to.
    guard: Option<(Handle, Handle)>,
}
// SAFETY: the game thread's alone (the mechanics' frame and the player's detach hook).
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

/// PlayerPuppet.VRArmBSetInputGuard: the guard made and attached (registered for Dodge and ToggleCrouch), or detached.
fn set_input_guard(s: &mut State, pl: &Handle, enabled: bool) {
    let attached_here = s.guard.as_ref().is_some_and(|(_, p)| p.instance == pl.instance);
    if enabled && !attached_here {
        detach(s);
        let guard = super::lobby_ink::new("VRArmBInputGuard");
        if guard.is_null() || pl.is_null() {
            return;
        }
        rt::call(pl, "RegisterInputListener", &[A::H(&guard), n("Dodge")]);
        rt::call(pl, "RegisterInputListener", &[A::H(&guard), n("ToggleCrouch")]);
        s.guard = Some((guard, pl.clone()));
    } else if !enabled {
        detach(s);
    }
}
fn detach(s: &mut State) {
    if let Some((guard, pl)) = s.guard.take() {
        if !pl.is_null() {
            rt::call(&pl, "UnregisterInputListener", &[A::H(&guard)]);
        }
    }
    *CAPTURED.lock().unwrap_or_else(|p| p.into_inner()) = [false; 2];
}
/// PlayerPuppet.OnDetach (game_hooks.rs): the guard goes with the player.
pub(in crate::mechanics) fn player_detach() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.as_mut() {
        detach(s);
    }
}
/// PlayerPuppet.VRArmBLauncherAllowed: no status effect forbidding weapons, arms or combat, the RB hotkey free.
pub(in crate::mechanics) fn launcher_allowed(pl: &Handle) -> bool {
    for tag in ["NoWeapons", "NoArmsCW", "NoCombat", "Melee", "Fists", "OneHandedFirearms", "DriverCombatFirearms", "DriverCombatBikeWeapons"] {
        if rt::static_call("StatusEffectSystem", "ObjectHasStatusEffectWithTag", &[A::H(pl), n(tag)]).is_some_and(|o| Val::of(&o).bool()) {
            return false;
        }
    }
    !super::menu_rt::static_gi("PlayerGameplayRestrictions", "IsHotkeyRestricted", &[A::N(ev("EHotkey", "RB"))]).is_some_and(|o| Val::of(&o).bool())
}
fn item_of(o: &crate::script_call::Owned) -> [u64; 2] {
    // SAFETY: an ItemID result.
    unsafe { *(o.as_ptr() as *const [u64; 2]) }
}
fn is_type(item: &[u64; 2], ty: &str) -> bool {
    rt::static_call("WeaponObject", "IsOfType", &[A::P(item.as_ptr().cast()), A::N(ev("gamedataItemType", ty))]).is_some_and(|o| Val::of(&o).bool())
}
fn same(a: &[u64; 2], b: &[u64; 2]) -> bool {
    a[0] == b[0] && a[1] == b[1]
}

fn update(s: &mut State, dt: f64) {
    s.cooldown = (s.cooldown - dt).max(0.0);
    let pl = super::belt::player();
    if pl.is_null() {
        s.previous_down = true;
        return;
    }
    if s.launcher_pending > 0.0 {
        s.launcher_pending = (s.launcher_pending - dt).max(0.0);
        if s.launcher_pending == 0.0 {
            super::launcher_ready::set_flag(&pl, "vrArmBLauncherPending", false);
        }
    }
    let equipment = rt::obj(&super::game_system("GetScriptableSystemsContainer"), "Get", &[n("EquipmentSystem")]);
    let data = rt::obj(&equipment, "GetPlayerData", &[A::H(&pl)]);
    let guard_off = |s: &mut State| {
        set_input_guard(s, &pl, false);
        s.previous_down = true;
    };
    if data.is_null() {
        guard_off(s);
        return;
    }
    let item = rt::call(&data, "GetActiveItem", &[A::N(ev("gamedataEquipmentArea", "ArmsCW"))]).map_or([0; 2], |o| item_of(&o));
    if item[0] == 0 {
        guard_off(s);
        return;
    }
    let launcher = is_type(&item, "Cyb_Launcher");
    let melee = is_type(&item, "Cyb_MantisBlades") || is_type(&item, "Cyb_NanoWires") || is_type(&item, "Cyb_StrongArms");
    if !launcher && !melee {
        guard_off(s);
        return;
    }
    set_input_guard(s, &pl, true);
    if shared(31) != 0.0 || shared(164) != 0.0 {
        s.previous_down = true;
        return;
    }
    let down = shared(157) > 0.5;
    let pressed = down && !s.previous_down;
    s.previous_down = down;
    if !pressed || s.cooldown > 0.0 {
        return;
    }
    // ScriptedPuppet.GetActiveWeapon(puppet) is a static of the puppet class, not a method of the player
    let active = rt::static_obj("ScriptedPuppet", "GetActiveWeapon", &[A::H(&pl)]);
    if !active.is_null() {
        let active_id = rt::call(&active, "GetItemID", &[]).map_or([0; 2], |o| item_of(&o));
        if melee && same(&active_id, &item) {
            super::holster::holster_melee(&pl);
            s.cooldown = 0.75;
            return;
        }
        if !is_type(&active_id, "Wea_Fists") && !(launcher && same(&active_id, &item)) {
            return;
        }
    }
    let action = if launcher {
        let ts = super::game_system("GetTransactionSystem");
        let left = rt::obj(&ts, "GetItemInSlot", &[A::H(&pl), A::Name(red4ext_abi::tweakdbid::from_name("AttachmentSlots.WeaponLeft"))]);
        let left_id = rt::call(&left, "GetItemID", &[]).map_or([0; 2], |o| item_of(&o));
        if !left.is_null() && same(&left_id, &item) {
            super::launcher_ready::set_flag(&pl, "vrLauncherKeepOpen", false);
            super::launcher_ready::set_flag(&pl, "vrLauncherSelectedAsWeapon", false);
            super::launcher_ready::set_flag(&pl, "vrLauncherFromWeaponCycle", true);
            "UnequipLeftHandCyberware"
        } else {
            if !launcher_allowed(&pl) {
                return;
            }
            super::launcher_ready::set_flag(&pl, "vrArmBLauncherPending", true);
            s.launcher_pending = 1.0;
            "RequestLeftHandCyberware"
        }
    } else {
        "RequestActiveMeleeware"
    };
    s.cooldown = 0.75;
    super::holster::equip_request(&pl, action);
}

pub(super) fn tick(dt: f32) {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = g.get_or_insert_with(|| State { previous_down: true, cooldown: 0.0, launcher_pending: 0.0, guard: None });
    update(s, f64::from(dt));
}
pub(super) fn shutdown() {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.as_mut() {
        detach(s);
    }
}
