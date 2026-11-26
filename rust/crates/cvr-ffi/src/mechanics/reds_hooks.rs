//! The port's redscript modules (mods/redscript/CyberpunkVRPort_*) whose work Rust does: their @wrapMethods become
//! hooks of crate::script_hooks, and the Rust build leaves the modules out of the script compilation (PORTED, read by
//! the plugin's start: cvr_host::scripts::install_except). Each wrapper keeps its side: code before wrappedMethod is a
//! Before hook (or an Override when it decides whether the original runs), code after it an After hook.
//!
//! - CyberpunkVRPort_BodyCarry: CarriedObjectEvents.SetAnimFeature_Carry -- the carry feature's useBothHands to
//!   SetVRBodyCarryBothHands.
//! - CyberpunkVRPort_PopupVisualState: UISystem.RequestNewVisualState / RestorePreviousVisualState -- the original only
//!   when VRPopupVisualState lets it.
//! - CyberpunkVRPort_Prompts: InteractionsInputView.SetInputButton -- the part through VRPromptPart; ResetInputButton --
//!   the default part through VRPromptPart onto the input image (inkImageRef.SetTexturePart).
//! - CyberpunkVRPort_Finisher: PlayerPuppet.OnWorkspotStartedEvent / OnWorkspotFinishedEvent --
//!   SetVRFinisherActive(GetIsInWorkspotFinisher()).
//! - CyberpunkVRPort_WorldMap: WorldMapMenuGameController.OnInitialize / OnUninitialize -- SetVRMenuOpen(1 / 0) and the
//!   module's FTLog line.
//! - CyberpunkVRPort_WeaponUp: DefaultTransition.ShouldEnterSafe -- false while a melee weapon is in the held hand's
//!   slot (VRLeftHandedWeapon.HeldSlot); MeleePublicSafeEvents.OnTick does nothing (its @replaceMethod).
//! - CyberpunkVRPort_EngineSettings (the ChangeValue wrapper split off CyberpunkVRPort_Engine, whose native
//!   declarations other modules still use): SettingsSelectorControllerListName.ChangeValue steps past Fullscreen in
//!   /video/display WindowMode.
//! - CyberpunkVRPort_StoryAttention and CyberpunkVRPort_HudClearance: their @addMethods are Rust functions of
//!   story_attention.rs and hud_clearance.rs, their only callers.
//! - CyberpunkVRPort_Haptics: DamageSystem.ModifyHitData -- a hit on the player that was blocked or deflected (bullet or
//!   melee) to VRHapticEvent with the guarding hands (VRPortGuardHands, now in CyberpunkVRPort_Engine with the native's
//!   declaration: the Cyberware module calls both).
//! - CyberpunkVRPort_DualWield: the shoot decisions (RightWeaponDecisions.ToLeftWeapon, LeftWeaponDecisions.ToRightWeapon,
//!   BothWeaponsDecisions.EnterCondition, ExitToNextShootDecisionsBase.ToLeftWeapon / ToRightWeapon) follow the dual
//!   wield's triggers (shared slot 165: bit 0 active, bit 1 left, bit 2 right) while it is active.
//! - CyberpunkVRPort_Saves: CyberwareCustomizationSystem.OnRestored / RestoreCyberwareCustomization only with a valid
//!   game instance; EquipmentSystemPlayerData.OnRestored first takes off a wardrobe set naming clothes the game does
//!   not have (VRPortDropLostWardrobeSet).
//! - CyberpunkVRPort_DeviceCam: Device.ToggleCameraZoom (its @replaceMethod) -- the device's zoom flag, the screen
//!   reported (SetVRDeviceScreen), the UI zoom listener, the DeviceZoom context; the zoom camera itself is never
//!   activated (VRPortDeviceCamZoomWanted is false), only deactivated.
//! - CyberpunkVRPort_Optics: with the physical optics (VRPhysicalOpticsEnabled) the ADS controller aims and the iron
//!   sight updates; the reload animation's end loop is caught again; ZoomTransition.SendZoomAnimFeatureData keeps the
//!   world's FOV (a zoom feature at 1.0) for a ranged, non-smart-sniper weapon outside vision mode, temporary unequip
//!   and device control. Its controller class VRPhysicalScopeController stays compiled, in CyberpunkVRPort_Engine.
//! - CyberpunkVRPort_Holster: its equip helpers are Rust's (holster::equip_request); the holster's intent field,
//!   VRHolsterMelee and VRHolsterIntentActive (the Melee and Cyberware modules read it) moved into CyberpunkVRPort_Engine.
//! - CyberpunkVRPort_EngineStereo: SingleplayerMenuGameController.OnInitialize -- VREngineStereoMenuShown, and with the
//!   lobby up the main menu's backdrop (bganim, backgroundContainer: its video stopped) hidden; OnUninitialize --
//!   VREngineStereoMenuClosed. Its lobby exposure service (a script class the callback system calls) moved into
//!   CyberpunkVRPort_Engine.
//! - CyberpunkVRPort_Menu: PlayerPuppet.OnDetach first detaches the VR menu's journal watch and drops it; the watch (a
//!   script class the journal calls back) with its field and getter moved into CyberpunkVRPort_Engine.
//! - CyberpunkVRPort_Basketball: no hooks -- its PlayerPuppet methods and fields are mechanics/ball.rs (the bridge's
//!   tick calls it), its console the plugin's VRBall* natives.
//! - CyberpunkVRPort_ScannerHud: the scanner's layout and its editor, mechanics/scanner_hud.rs (its hooks in the table
//!   below); the editor listens through the quickhack panel's OnAction, the re-apply is a timer of the mechanics' tick.
//! - CyberpunkVRPort_Smoking: its PlayerPuppet methods and fields are mechanics/smoke.rs (the bridge's tick, the
//!   holsters and the weapon wheel call it); EquipmentSystemPlayerData.OnEquipmentSystemWeaponManipulationRequest is
//!   in the table below.
//! - CyberpunkVRPort_NoAnims: the weapon state machines' hooks, mechanics/no_anims.rs (in the table below); its
//!   VRPortFirearm / VRPortFirearmIsLeft (CyberpunkVRPort_LeftHanded calls them) moved into CyberpunkVRPort_Engine.
//!   DefaultTransition.ShouldEnterSafe carries two Overrides, WeaponUp's (a melee weapon) and NoAnims' (a firearm):
//!   script_hooks chains them as redscript chained the wrappers.
//! - CyberpunkVRPort_LeftHanded: mechanics/left_handed.rs (its hooks in the table below; DefaultTransition's static
//!   HasSingleWeaponEquipped answered by its callers); the class VRLeftHandedWeapon with the natives' declarations (the Melee and
//!   Cyberware modules call it) moved into CyberpunkVRPort_Engine.
//! - CyberpunkVRPort_Hold: no hooks -- its PlayerPuppet methods are mechanics/hold.rs (the belt, its grenades and the
//!   reload call it).
//! - CyberpunkVRPort_Melee and CyberpunkVRPort_Cyberware (each calls the other): melee.rs (the swings, the attacks, the
//!   enhanced melee, the sounds, the damage hooks), melee_throw.rs (the physical throw), cyberware.rs (the launcher,
//!   the monowire, the iconic chord) and world_markers.rs (the breach marker, nameplates, damage digits); their hooks
//!   below, the scripted statics' (MeleeTransition::WantsToLightAttack / WantsToStrongAttack / GetAimAssistMeleeRecord,
//!   ProjectileLaunchHelper::SpawnProjectile) by install_static. The launcher-ready state (script classes the state
//!   machine makes by name, their transitions and fields) moved into CyberpunkVRPort_Engine.
//! - CyberpunkVRPort_Engine, what the modules above moved there: the classes the engine makes or calls by name (the
//!   launcher-ready state's -- launcher_ready.rs --, the journal watch's -- journal_watch.rs --, the lobby exposure
//!   service's and the scope controller's -- engine_services.rs) are made by Rust before InitializeScriptRuntime
//!   (crate::script_classes); its helpers (VRHolsterMelee / VRHolsterIntentActive -- holster.rs --, VRPortGuardHands --
//!   melee.rs --, VRLeftHandedWeapon, VRPortFirearm) are Rust functions. With it the Rust build compiles no script:
//!   the plugin's compiler is not started.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_hooks::{install_inner as install, Call, HookFn, Kind};
use std::sync::atomic::{AtomicBool, Ordering};

/// The modules left out of the Rust build's script compilation.
pub(crate) const PORTED: &[&str] = &[
    "CyberpunkVRPort_Engine",
    "CyberpunkVRPort_BodyCarry",
    "CyberpunkVRPort_PopupVisualState",
    "CyberpunkVRPort_Prompts",
    "CyberpunkVRPort_Finisher",
    "CyberpunkVRPort_WorldMap",
    "CyberpunkVRPort_WeaponUp",
    "CyberpunkVRPort_EngineSettings",
    "CyberpunkVRPort_StoryAttention",
    "CyberpunkVRPort_HudClearance",
    "CyberpunkVRPort_Haptics",
    "CyberpunkVRPort_DualWield",
    "CyberpunkVRPort_Saves",
    "CyberpunkVRPort_DeviceCam",
    "CyberpunkVRPort_Optics",
    "CyberpunkVRPort_Holster",
    "CyberpunkVRPort_EngineStereo",
    "CyberpunkVRPort_Menu",
    "CyberpunkVRPort_Basketball",
    "CyberpunkVRPort_ScannerHud",
    "CyberpunkVRPort_Smoking",
    "CyberpunkVRPort_NoAnims",
    "CyberpunkVRPort_LeftHanded",
    "CyberpunkVRPort_Hold",
    "CyberpunkVRPort_Melee",
    "CyberpunkVRPort_Cyberware",
];
static DONE: AtomicBool = AtomicBool::new(false);

fn native_bool(name: &str, args: &[A]) -> bool {
    rt::global_call(name, args).is_some_and(|o| Val::of(&o).bool())
}
fn native_name(name: &str, args: &[A]) -> u64 {
    // SAFETY: a CName result.
    rt::global_call(name, args).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
fn ft_log(text: &str) {
    let a = super::menu_rt::Arg::new(text);
    rt::global_call("FTLog", &[a.a()]);
}

// ---- BodyCarry
fn carry(call: &mut Call) {
    let feature = rt::field(&call.this(), "m_animCarryFeature").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    if feature.is_null() {
        return;
    }
    let both = rt::field(&feature, "useBothHands").is_some_and(|v| v.bool());
    rt::global_call("SetVRBodyCarryBothHands", &[A::B(both)]);
}
// ---- PopupVisualState
fn visual_state_request(call: &mut Call) {
    if !native_bool("VRPopupVisualState", &[A::Name(call.arg_u64(0)), A::B(true)]) {
        call.skip();
    }
}
fn visual_state_restore(call: &mut Call) {
    if !native_bool("VRPopupVisualState", &[A::Name(call.arg_u64(0)), A::B(false)]) {
        call.skip();
    }
}
// ---- Prompts
fn input_button(call: &mut Call) {
    let part = native_name("VRPromptPart", &[A::Name(call.arg_u64(0))]);
    call.set_arg_u64(0, part);
}
fn input_button_reset(call: &mut Call) {
    let this = call.this();
    let default = rt::field(&this, "m_DefaultInputPartName").map_or(0, |v| {
        // SAFETY: a CName field.
        unsafe { *(v.ptr as *const u64) }
    });
    let part = native_name("VRPromptPart", &[A::Name(default)]);
    if part != default {
        if let Some(image) = rt::field(&this, "m_InputImage") {
            rt::static_call("inkImageRef", "SetTexturePart", &[A::P(image.ptr.cast_const()), A::Name(part)]);
        }
    }
}
// ---- Finisher
fn workspot_changed(call: &mut Call) {
    let finisher = rt::call(&call.this(), "GetIsInWorkspotFinisher", &[]).is_some_and(|o| Val::of(&o).bool());
    rt::global_call("SetVRFinisherActive", &[A::B(finisher)]);
}
// ---- WorldMap
fn map_open(_call: &mut Call) {
    rt::global_call("SetVRMenuOpen", &[A::N(1.0)]);
    ft_log("[VRPortMap] world map OPEN -> head-lock on");
}
fn map_close(_call: &mut Call) {
    rt::global_call("SetVRMenuOpen", &[A::N(0.0)]);
    ft_log("[VRPortMap] world map CLOSE -> head-lock off");
}

// ---- WeaponUp
/// VRWeaponUpHasMelee: a melee weapon in the held hand's slot of the state machine's owner.
fn has_melee(interface: &red4ext_abi::handle::Handle) -> bool {
    let ts = rt::obj(interface, "GetTransactionSystem", &[]);
    let owner = rt::field(interface, "executionOwner").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    if ts.is_null() || owner.is_null() {
        return false;
    }
    let slot = super::left_handed::held_slot(&owner);
    let weapon = rt::obj(&ts, "GetItemInSlot", &[A::H(&owner), A::Name(slot)]);
    if weapon.is_null() || !super::menu_rt::is_a(&weapon, "WeaponObject") {
        return false;
    }
    // SAFETY: a CName result.
    let tag = rt::static_call("WeaponObject", "GetMeleeWeaponTag", &[]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) });
    let id = super::menu_rt::id16(&weapon, "GetItemID", &[]);
    rt::call(&ts, "HasTag", &[A::H(&owner), A::Name(tag), id.a()]).is_some_and(|o| Val::of(&o).bool())
}
fn should_enter_safe(call: &mut Call) {
    if has_melee(&call.arg_handle(1)) {
        call.set_result_bool(false);
    }
}
/// DefaultTransition.IsDisplayingInputHintBlocked: the quick melee (a gun's blow) has no VR control -- R3 is the port's
/// own and LB the scanner's -- so its hint is not shown while the pad is used.
fn hint_blocked(call: &mut Call) {
    call.original();
    if call.result_bool() || call.arg_u64(1) != red4ext_abi::cname::CName::hashed("QuickMelee").0 || cvr_settings::lc!(xrXInputHook) == 0 {
        return;
    }
    let owner = rt::field(&call.arg_handle(0), "executionOwner").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    if rt::call(&owner, "PlayerLastUsedPad", &[]).is_some_and(|o| Val::of(&o).bool()) {
        call.set_result_bool(true);
    }
}
fn melee_safe_tick(call: &mut Call) {
    call.skip();
}
// ---- EngineSettings
fn settings_change_value(call: &mut Call) {
    call.original();
    let value = rt::field(&call.this(), "m_realValue").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    let name = |f: &str| rt::call(&value, f, &[]).map(|o| Val::of(&o).text()).unwrap_or_default();
    if name("GetName") == "WindowMode" && name("GetGroupPath") == "/video/display" && name("GetValue") == "Fullscreen" {
        call.original();
    }
}

// ---- Haptics
fn hit_data(call: &mut Call) {
    let hit = call.arg_handle(0);
    if hit.is_null() || rt::field(&hit, "projectionPipeline").is_some_and(|v| v.bool()) {
        return;
    }
    let data = rt::field(&hit, "attackData").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    let target = rt::field(&hit, "target").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    if data.is_null() || target.is_null() || !super::menu_rt::is_a(&target, "PlayerPuppet") {
        return;
    }
    let was = |f: &str| rt::call(&data, f, &[]).is_some_and(|o| Val::of(&o).bool());
    let kind = if was("WasBulletDeflected") {
        13
    } else if was("WasBulletBlocked") {
        12
    } else if was("WasDeflected") {
        11
    } else if was("WasBlocked") {
        10
    } else {
        0
    };
    if kind != 0 {
        let hands = super::melee::guard_hands(&target);
        rt::global_call("VRHapticEvent", &[A::N(f64::from(kind)), A::N(f64::from(hands))]);
    }
}
// ---- DualWield
/// (active, left, right) of the dual wield's triggers (shared slot 165).
fn triggers() -> (bool, bool, bool) {
    let (active, left, right, _) = dual_slot();
    (active, left, right)
}
/// The slot's bits: active, left, right and the Hurricane's request (VRDualWieldTriggers).
fn dual_slot() -> (bool, bool, bool, bool) {
    let v = rt::global_call("GetVRSharedSlot", &[A::N(165.0)]).and_then(|o| Val::of(&o).num()).unwrap_or(0.0);
    let bits = v as i64;
    (v > 0.5, (bits / 2) % 2 == 1, (bits / 4) % 2 == 1, (bits / 8) % 2 == 1)
}
pub(super) fn dual_triggers() -> (bool, bool, bool) {
    triggers()
}
// HEYWOOD HURRICANE is ValentinosFrenzy in the game's state machine. A rapid pair of squeezes on EACH trigger requests
// it; all perk, reload, safe-state, ammo and equipment checks remain the game's own CanStartValentinosFrenzy. The merged
// fire action is listened for as well as the native QuickMelee binding.
fn frenzy_attach(call: &mut Call) {
    let (this, si) = (call.this(), call.arg_handle(1));
    let owner = rt::field(&si, "executionOwner").map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null);
    rt::call(&owner, "RegisterInputListener", &[A::H(&this), A::Name(red4ext_abi::cname::CName::hashed("RangedAttack").0)]);
}
fn frenzy_action(call: &mut Call) {
    // A deliberate squeeze can follow a full release of both hands: the previous release was evaluated before the
    // cadence request existed, and waiting for this squeeze's release would miss a held last trigger entirely.
    // ListenerAction is a struct passed in place (as a handle it read as null: this never ran)
    let action = rt::listener_arg(call, 0);
    if !triggers().0 || action.is_null() {
        return;
    }
    if rt::listener_name(action) == red4ext_abi::cname::CName::hashed("RangedAttack").0
        && rt::listener_static(false, "IsButtonJustPressed", action).is_some_and(|o| Val::of(&o).bool())
    {
        rt::call(&call.this(), "EnableOnEnterCondition", &[A::B(true)]);
    }
}
fn frenzy_enter(call: &mut Call) {
    let (sc, si) = (call.arg_handle(0), call.arg_handle(1));
    call.original();
    let native = call.result_bool();
    let (active, _, _, hurricane) = dual_slot();
    let v = native
        || (active
            && hurricane
            && rt::call(&call.this(), "CanStartValentinosFrenzy", &[A::H(&sc), A::H(&si)]).is_some_and(|o| Val::of(&o).bool()));
    call.set_result_bool(v);
}
fn transition(name: &str, call: &Call) -> bool {
    rt::static_call("DefaultTransition", name, &[A::H(&call.arg_handle(1))]).is_some_and(|o| Val::of(&o).bool())
}
fn ready(name: &str, call: &Call) -> bool {
    rt::static_call("ShootTransition", name, &[A::H(&call.arg_handle(0)), A::H(&call.arg_handle(1))]).is_some_and(|o| Val::of(&o).bool())
}
fn right_to_left(call: &mut Call) {
    let (active, left, right) = triggers();
    if active && transition("HasLeftWeaponEquipped", call) {
        let v = left && (!right || !ready("IsRightWeaponReady", call));
        call.set_result_bool(v);
    }
}
fn left_to_right(call: &mut Call) {
    let (active, left, right) = triggers();
    if active && transition("HasRightWeaponEquipped", call) {
        let v = right && (!left || !ready("IsLeftWeaponReady", call));
        call.set_result_bool(v);
    }
}
fn both_enter(call: &mut Call) {
    let (active, left, right) = triggers();
    if active {
        let v = left
            && right
            && transition("HasRightWeaponEquipped", call)
            && transition("HasLeftWeaponEquipped", call)
            && ready("IsRightWeaponReady", call)
            && ready("IsLeftWeaponReady", call);
        call.set_result_bool(v);
    }
}
fn exit_to_left(call: &mut Call) {
    let (active, left, right) = triggers();
    if active && super::menu_rt::is_a(&call.this(), "BothWeaponsDecisions") {
        let v = left && !right && ready("IsLeftWeaponReady", call);
        call.set_result_bool(v);
    }
}
fn exit_to_right(call: &mut Call) {
    let (active, left, right) = triggers();
    if active && super::menu_rt::is_a(&call.this(), "BothWeaponsDecisions") {
        let v = right && !left && ready("IsRightWeaponReady", call);
        call.set_result_bool(v);
    }
}
// ---- Saves
fn game_valid(call: &Call) -> bool {
    let Some(gi) = rt::call(&call.this(), "GetGameInstance", &[]) else { return false };
    rt::static_call("GameInstance", "IsValid", &[A::P(gi.as_ptr().cast())]).is_some_and(|o| Val::of(&o).bool())
}
fn cyberware_restored(call: &mut Call) {
    if !game_valid(call) {
        call.skip();
    }
}
/// VRPortDropLostWardrobeSet: a wardrobe set naming a record the game does not have (a removed mod's clothes) is
/// taken off before the equipment restores it: every visual but the underwear cleared, the active set index INVALID.
fn equipment_restored(call: &mut Call) {
    let this = call.this();
    let Some(list) = rt::field(&this, "m_clothingVisualsInfo") else { return };
    let entries = super::menu_rt::items(&list);
    let missing = |v: &Val| -> bool {
        let Some(item) = v.f("visualItem") else { return false };
        let id = super::menu_rt::Id16::of(&item);
        let valid = rt::static_call("ItemID", "IsValid", &[id.a()]).is_some_and(|o| Val::of(&o).bool());
        valid && super::menu_rt::so("TweakDBInterface", "GetItemRecord", &[A::Name(id.tdbid())]).is_null()
    };
    if !entries.iter().any(missing) {
        return;
    }
    let none = rt::static_call("ItemID", "None", &[]);
    let (top, bottom) = (super::menu_rt::ev("gamedataEquipmentArea", "UnderwearTop"), super::menu_rt::ev("gamedataEquipmentArea", "UnderwearBottom"));
    for e in &entries {
        let area = super::menu_rt::fnum(e, "areaType");
        if area == top || area == bottom {
            continue;
        }
        if let (Some(item), Some(none)) = (e.f("visualItem"), none.as_ref()) {
            // SAFETY: the ItemID field and an ItemID value of the same type.
            unsafe { item.ty.assign(item.ptr, none.as_ptr().cast()) };
        }
        super::menu_rt::set(e, "isHidden", 0.0);
    }
    let wardrobe = super::game_system("GetWardrobeSystem");
    rt::call(&wardrobe, "SetActiveClothingSetIndex", &[A::N(super::menu_rt::ev("gameWardrobeClothingSetIndex", "INVALID"))]);
    ft_log("[VRPort] wardrobe set taken off on load: it names clothes the game does not have");
}

// ---- DeviceCam
fn handle_field(o: &red4ext_abi::handle::Handle, f: &str) -> red4ext_abi::handle::Handle {
    rt::field(o, f).map(|v| v.handle()).unwrap_or_else(red4ext_abi::handle::Handle::null)
}
fn toggle_camera_zoom(call: &mut Call) {
    call.skip();
    let this = call.this();
    let zoom = handle_field(&this, "m_cameraZoomComponent");
    if zoom.is_null() {
        return;
    }
    let (toggle, instant) = (call.arg_bool(0), call.arg_bool(1));
    let active = rt::field(&this, "m_cameraZoomActive").is_some_and(|v| v.bool());
    if toggle == active {
        return;
    }
    let ui = super::game_system("GetUISystem");
    let context = super::menu_rt::ev("UIGameContext", "DeviceZoom");
    if toggle {
        // VRPortDeviceCamZoomWanted() is false: the zoom camera is not activated
        super::menu_rt::hset(&this, "m_cameraZoomActive", 1.0);
        rt::global_call("SetVRDeviceScreen", &[A::N(1.0)]);
        let board = (|| {
            let defs = super::boards::defs().ok()?;
            let psm = super::boards::definition(&defs, "PlayerStateMachine").ok()?.clone();
            let player = rt::obj(&super::game_system("GetPlayerSystem"), "GetLocalPlayerControlledGameObject", &[]);
            let id = super::belt::entity_hash(&player)?;
            let bb = super::boards::local_board(id, &psm).ok()?;
            let field = super::boards::field(&psm, "IsUIZoomDevice").ok()?;
            Some((bb, field))
        })();
        if let Some((bb, field)) = board.filter(|(bb, _)| !bb.is_null()) {
            let listener = rt::obj(&bb, "RegisterListenerBool", &[A::P(field.cast_const()), A::H(&this), A::Name(super::menu_rt::cname("OnIsUIZoomDeviceChange"))]);
            super::menu_main::set_handle_prop(&this, "m_ZoomUIListenerID", &listener);
        }
        rt::call(&ui, "PushGameContext", &[A::N(context)]);
    } else {
        rt::call(&zoom, "Deactivate", &[A::N(if instant { 0.0 } else { 0.5 })]);
        super::menu_rt::hset(&this, "m_cameraZoomActive", 0.0);
        rt::global_call("SetVRDeviceScreen", &[A::N(0.0)]);
        rt::call(&ui, "PopGameContext", &[A::N(context)]);
    }
}
// ---- Optics
fn optics() -> bool {
    native_bool("VRPhysicalOpticsEnabled", &[])
}
fn ads_attach(call: &mut Call) {
    if optics() {
        rt::call(&call.this(), "OnAim", &[A::B(true)]);
    }
}
fn ads_aim(call: &mut Call) {
    let aiming = call.arg_bool(0) || optics();
    call.set_arg_bool(0, aiming);
}
fn ironsight_update(call: &mut Call) {
    if optics() {
        rt::call(&call.this(), "EnableUpdate", &[A::B(true)]);
    }
}
fn ironsight_ammo(call: &mut Call) {
    let this = call.this();
    let reload = handle_field(&this, "m_animReload");
    let state = rt::field(&this, "m_crosshairState").and_then(|v| v.num()).unwrap_or(-1.0);
    if call.arg_number(0).unwrap_or(0.0) > 0.0 && !reload.is_null() && state != super::menu_rt::ev("gamePSMCrosshairStates", "Reload") {
        let end_loop = super::menu_rt::ev("inkanimEventType", "OnEndLoop");
        rt::call(&reload, "RegisterToCallback", &[A::N(end_loop), A::H(&this), A::Name(super::menu_rt::cname("OnReloadEndLoop"))]);
        super::menu_main::set_handle_prop(&this, "m_animReload", &red4ext_abi::handle::Handle::null());
    }
}
/// VRIsSmartSniper: a smart weapon that is a sniper rifle.
fn smart_sniper(weapon: &red4ext_abi::handle::Handle) -> bool {
    let record = rt::obj(weapon, "GetWeaponRecord", &[]);
    let evolution = rt::obj(&record, "Evolution", &[]);
    if evolution.is_null() {
        return false;
    }
    let smart = rt::call(&evolution, "Type", &[]).and_then(|o| Val::of(&o).num()) == Some(super::menu_rt::ev("gamedataWeaponEvolution", "Smart"));
    let id = super::menu_rt::id16(weapon, "GetItemID", &[]);
    let sniper = rt::static_call("WeaponObject", "GetWeaponType", &[id.a()]).and_then(|o| Val::of(&o).num()) == Some(super::menu_rt::ev("gamedataItemType", "Wea_SniperRifle"));
    smart && sniper
}
fn zoom_feature(call: &mut Call) {
    let (context, interface) = (call.arg_handle(0), call.arg_handle(1));
    let this = call.this();
    let owner = handle_field(&interface, "executionOwner");
    let weapon = super::menu_rt::so("ScriptedPuppet", "GetActiveWeapon", &[A::H(&owner)]);
    let flag = |o: &red4ext_abi::handle::Handle, f: &str, args: &[A]| rt::call(o, f, args).is_some_and(|r| Val::of(&r).bool());
    let keep_game = !optics()
        || !super::menu_rt::is_a(&owner, "PlayerPuppet")
        || weapon.is_null()
        || !flag(&weapon, "IsRanged", &[])
        || smart_sniper(&weapon)
        || flag(&this, "IsInVisionModeActiveState", &[A::H(&context), A::H(&interface)])
        || flag(&context, "IsStateActive", &[A::Name(super::menu_rt::cname("UpperBody")), A::Name(super::menu_rt::cname("temporaryUnequip"))])
        || flag(&this, "IsControllingDevice", &[A::H(&context), A::H(&interface)]);
    if keep_game {
        return; // the original runs
    }
    call.skip();
    let fov = rt::num(&super::game_system("GetCameraSystem"), "GetActiveCameraFOV", &[]).unwrap_or(60.0);
    let feature = super::lobby_ink::new("AnimFeature_Zoom");
    for (f, v) in [("finalZoomLevel", 1.0), ("weaponZoomLevel", 1.0), ("weaponScopeFov", fov), ("weaponAimFOV", fov), ("worldFOV", fov)] {
        super::menu_rt::hset(&feature, f, v);
    }
    let level = rt::num(&this, "GetZoomLevelNumber", &[A::H(&context)]).unwrap_or(0.0);
    super::menu_rt::hset(&feature, "zoomLevelNum", level);
    for (f, name) in [("noWeaponAimInTime", "noWeaponAimInTime"), ("noWeaponAimOutTime", "noWeaponAimOutTime")] {
        let a = super::menu_rt::Arg::new(name);
        let v = rt::num(&this, "GetStaticFloatParameterDefault", &[a.a(), A::N(0.20)]).unwrap_or(0.20);
        super::menu_rt::hset(&feature, f, v);
    }
    let use_zoom = flag(&this, "GetShouldUseWeaponZoomData", &[A::H(&context)]);
    super::menu_rt::hset(&feature, "shouldUseWeaponZoomStats", if use_zoom { 1.0 } else { 0.0 });
    super::menu_rt::hset(&feature, "focusModeActive", 0.0);
    rt::call(&interface, "SetAnimationParameterFeature", &[A::Name(super::menu_rt::cname("ZoomAnimData")), A::H(&feature)]);
}

// ---- EngineStereo
/// VRLobbyHideBackdrop: the main menu's backdrop widgets (bganim, backgroundContainer) hidden, a video stopped, four
/// levels down.
fn hide_backdrop(w: &red4ext_abi::handle::Handle, depth: u32) {
    if w.is_null() || depth > 4 {
        return;
    }
    let name = rt::call(w, "GetName", &[]).map(|o| Val::of(&o).text()).unwrap_or_default();
    if name == "bganim" || name == "backgroundContainer" {
        if super::menu_rt::is_a(w, "inkVideo") {
            rt::call(w, "Stop", &[]);
        }
        rt::call(w, "SetVisible", &[A::B(false)]);
        return;
    }
    if !super::menu_rt::is_a(w, "inkCompoundWidget") {
        return;
    }
    let n = rt::num(w, "GetNumChildren", &[]).unwrap_or(0.0) as i64;
    for i in 0..n {
        hide_backdrop(&rt::obj(w, "GetWidgetByIndex", &[A::N(i as f64)]), depth + 1);
    }
}
fn main_menu_shown(call: &mut Call) {
    rt::global_call("VREngineStereoMenuShown", &[]);
    if native_bool("VRLobbyActive", &[]) {
        let mut top = rt::obj(&call.this(), "GetRootWidget", &[]);
        for _ in 0..64 {
            let parent = rt::obj(&top, "GetParentWidget", &[]);
            if parent.is_null() {
                break;
            }
            top = parent;
        }
        hide_backdrop(&top, 0);
    }
}
fn main_menu_closed(_call: &mut Call) {
    rt::global_call("VREngineStereoMenuClosed", &[]);
}

// ---- Menu
/// PlayerPuppet.OnDetach: the journal watch stops listening; the player's fields Rust keeps (the launcher's flags, the
/// holster's intent) go with it.
fn journal_watch_detach(call: &mut Call) {
    if !super::menu_rt::is_a(&call.this(), "PlayerPuppet") {
        return;
    }
    super::journal_watch::detach();
    super::launcher_ready::forget();
    super::holster::forget_holster_intent();
}

static STATIC_SEEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Once, from the first frame of the Running state.
pub(super) fn install_all() {
    if DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::class_probe::run();
    crate::script_classes::report();
    let mut hooks: Vec<(&str, &str, Kind, HookFn)> = vec![
        ("CarriedObjectEvents", "SetAnimFeature_Carry", Kind::After, carry),
        ("UISystem", "RequestNewVisualState", Kind::Override, visual_state_request),
        ("UISystem", "RestorePreviousVisualState", Kind::Override, visual_state_restore),
        ("InteractionsInputView", "SetInputButton", Kind::Before, input_button),
        ("InteractionsInputView", "ResetInputButton", Kind::After, input_button_reset),
        ("PlayerPuppet", "OnWorkspotStartedEvent", Kind::After, workspot_changed),
        ("PlayerPuppet", "OnWorkspotFinishedEvent", Kind::After, workspot_changed),
        ("WorldMapMenuGameController", "OnInitialize", Kind::After, map_open),
        ("WorldMapMenuGameController", "OnUninitialize", Kind::After, map_close),
        ("DefaultTransition", "ShouldEnterSafe", Kind::Override, should_enter_safe),
        ("MeleePublicSafeEvents", "OnTick", Kind::Override, melee_safe_tick),
        ("DefaultTransition", "IsDisplayingInputHintBlocked", Kind::Override, hint_blocked),
        ("SettingsSelectorControllerListName", "ChangeValue", Kind::Override, settings_change_value),
        ("DamageSystem", "ModifyHitData", Kind::After, hit_data),
        ("ValentinosFrenzyInitDecisions", "OnAttach", Kind::After, frenzy_attach),
        ("ValentinosFrenzyInitDecisions", "OnAction", Kind::After, frenzy_action),
        ("ValentinosFrenzyInitDecisions", "EnterCondition", Kind::Override, frenzy_enter),
        ("RightWeaponDecisions", "ToLeftWeapon", Kind::Override, right_to_left),
        ("LeftWeaponDecisions", "ToRightWeapon", Kind::Override, left_to_right),
        ("BothWeaponsDecisions", "EnterCondition", Kind::Override, both_enter),
        ("ExitToNextShootDecisionsBase", "ToLeftWeapon", Kind::Override, exit_to_left),
        ("ExitToNextShootDecisionsBase", "ToRightWeapon", Kind::Override, exit_to_right),
        ("CyberwareCustomizationSystem", "OnRestored", Kind::Override, cyberware_restored),
        ("CyberwareCustomizationSystem", "RestoreCyberwareCustomization", Kind::Override, cyberware_restored),
        ("EquipmentSystemPlayerData", "OnRestored", Kind::Before, equipment_restored),
        ("Device", "ToggleCameraZoom", Kind::Override, toggle_camera_zoom),
        ("AimDownSightController", "OnPlayerAttach", Kind::After, ads_attach),
        ("AimDownSightController", "OnAim", Kind::Before, ads_aim),
        ("IronsightGameController", "OnPlayerAttach", Kind::After, ironsight_update),
        ("IronsightGameController", "OnUpperBodyChanged", Kind::After, ironsight_update),
        ("IronsightGameController", "OnAmmoCountChanged", Kind::After, ironsight_ammo),
        ("ZoomTransition", "SendZoomAnimFeatureData", Kind::Override, zoom_feature),
        ("SingleplayerMenuGameController", "OnInitialize", Kind::After, main_menu_shown),
        ("SingleplayerMenuGameController", "OnUninitialize", Kind::Before, main_menu_closed),
        ("PlayerPuppet", "OnDetach", Kind::Before, journal_watch_detach),
    ];
    hooks.extend_from_slice(&super::scanner_hud::HOOKS);
    hooks.extend_from_slice(&super::smoke::HOOKS);
    hooks.extend_from_slice(&super::no_anims::HOOKS);
    hooks.extend_from_slice(&super::aim_threat::HOOKS);
    hooks.extend_from_slice(&super::left_handed::HOOKS);
    hooks.extend_from_slice(&super::melee::HOOKS);
    hooks.extend_from_slice(&super::blade::HOOKS);
    hooks.extend_from_slice(&super::melee_throw::HOOKS);
    hooks.extend_from_slice(&super::cyberware::HOOKS);
    hooks.extend_from_slice(&super::world_markers::HOOKS);
    hooks.extend_from_slice(&super::workspot::HOOKS);
    hooks.extend_from_slice(&super::wilson_range::HOOKS);
    // a deferred one (NightCoop wraps it later: coop_gate.rs) counts as placed; it goes in from coop_gate::tick
    let mut ok = hooks
        .iter()
        .filter(|(class, name, kind, hook)| super::coop_gate::defer(class, name, *kind, *hook) || install(class, name, *kind, *hook))
        .count();
    let statics: Vec<(&str, &str, Kind, HookFn)> = super::melee::STATIC_HOOKS.iter().chain(super::melee_throw::STATIC_HOOKS.iter()).copied().collect();
    ok += statics.iter().filter(|(class, name, kind, hook)| crate::script_hooks::install_static_inner(class, name, *kind, *hook)).count();
    let total = hooks.len() + statics.len();
    if super::quickboot::ACTIVE.load(Ordering::Relaxed) {
        // the QuickBoot mod's observers (the CET host's: outer), with no Lua host to run the mod
        let n = super::quickboot::HOOKS.iter().filter(|(c, f, k, h)| crate::script_hooks::install(c, f, *k, *h)).count();
        cvr_host::log!("[quickboot] {n} of {} menu observers installed from Rust\n", super::quickboot::HOOKS.len());
    }
    if super::NO_LUA_HOST.load(Ordering::Relaxed) {
        // the state machine's action reads (the gestures' actions): the Lua host's start installed them
        // (Scripting::PostInitializeMods), the first Running frame is the equivalent moment
        // SAFETY: once, on the main thread, the RTTI up
        let ok = unsafe { cvr_rig::vrik::action_reads::CvrInstallActionReads() };
        cvr_host::log!("[input] state machine action reads installed from Rust (no Lua host): {ok}\n");
    }
    if std::env::var_os("CVR_STATIC_HOOK_TEST").is_some() {
        // a development check of the hooks on scripted statics (global functions): every call counted
        let ok = crate::script_hooks::install_static("DefaultTransition", "HasSingleWeaponEquipped", Kind::After, |call| {
            let n = STATIC_SEEN.fetch_add(1, Ordering::Relaxed);
            if n == 0 || n == 1000 {
                cvr_host::log!("[script-hooks] static test: HasSingleWeaponEquipped call {} -> {} (from Rust)\n", n + 1, call.result_bool());
            }
        });
        cvr_host::log!("[script-hooks] static test: hook on DefaultTransition::HasSingleWeaponEquipped installed {ok} (from Rust)\n");
    }
    cvr_host::log!("[mechanics] redscript modules from Rust: {ok} of {total} hooks ({} modules not compiled)\n", PORTED.len());
}
