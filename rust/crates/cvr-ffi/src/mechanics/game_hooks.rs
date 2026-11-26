//! The game's script functions the port's mechanics hook, hooked from Rust (crate::script_hooks) instead of the Lua
//! host's Observe / Override in the CET mods' shims:
//!
//! - mods/cet/CyberpunkVRPort_Menu/init.lua: the world screens' glitch (IWorldWidgetComponent.StartGlitching does
//!   nothing: the menu's entity is neither a Device nor a Vehicle), the in-game menu controller, the vendor's routing
//!   (MenuScenario_Idle / MenuScenario_Vendor / UISystem.RequestVendorMenu / VendorHubMenuGameController), the stash
//!   (OnOpenStorage), the menu's own save listener (LoadGameMenuGameController's two callbacks, for a listener whose
//!   sourceIndex carries the menu's marker) -- each reports to menu_vendor::hook / menu_load as VRMenuHook /
//!   VRMenuLoadNames did;
//! - mods/cet/CyberpunkVRPort_VRIK/init.lua: PlayerPuppet.OnGameAttached (the recenter after a load);
//! - the other shims' observers, each reporting what its native did: the lobby (the main menu's controller, its saves,
//!   its root scenario, the character creation's screens -- lobby::menu_event / lobby::saves), the braindance HUD (its
//!   controller's lifetime), the crosshair (its container, its state), the world map (its controller's lifetime), the
//!   physical crouch (CrouchEvents.OnEnter's owner), the swimming's fast states, the ladder's state (owner and
//!   description on the ladder states' OnUpdate), the player's attach (the camera bridge, the belt, the weapon frame,
//!   the ladder, the swimming) and detach (the camera bridge, the hands-only mode, the holsters), the radio / vehicle
//!   popups (lowered at once).
//!
//! `Observe` in the CET host is ObserveBefore: each hook keeps the side its shim used. Installed once, at the first
//! frame of the Running state (the main menu); the shims stand down while VRRustMechanics2 has bit 1 (GAME_HOOKS).
use crate::script_hooks::{install, Call, Kind};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::atomic::{AtomicBool, Ordering};

/// The menu's listener marker (menu_load: MARKER - epoch).
const MARKER: f64 = -7392000.0;
static DONE: AtomicBool = AtomicBool::new(false);

fn menu(kind: i32, call: &Call, b: &Handle, name: u64) -> bool {
    super::menu_vendor::hook(kind, &call.this(), b, name)
}
fn none() -> Handle {
    Handle::null()
}
/// The listener's marker when it is the menu's own.
fn marker(call: &Call) -> Option<f64> {
    let m = super::lobby_rtti::field(&call.this(), "sourceIndex").and_then(|v| v.num())?;
    (m <= MARKER).then_some(m)
}

fn glitch(call: &mut Call) {
    call.skip();
}
fn in_game_menu_init(call: &mut Call) {
    menu(14, call, &none(), 0);
}
fn idle_leave(call: &mut Call) {
    let next = call.arg_u64(0);
    menu(1, call, &none(), next);
}
fn vendor_enter_before(call: &mut Call) {
    let data = call.arg_handle(1);
    menu(5, call, &data, 0);
}
fn vendor_enter_after(call: &mut Call) {
    menu(2, call, &none(), 0);
}
fn vendor_leave(call: &mut Call) {
    let next = call.arg_u64(0);
    menu(3, call, &none(), next);
}
fn request_vendor_menu(call: &mut Call) {
    let data = call.arg_handle(0);
    if menu(4, call, &data, 0) {
        call.skip();
        return;
    }
    let scenario = call.arg_u64(1);
    if scenario == 0 || scenario == CName::hashed("None").0 {
        call.set_arg_u64(1, CName::hashed("MenuScenario_Vendor").0);
    }
    call.original();
}
fn switch_to_vendor(call: &mut Call) {
    let data = call.arg_handle(0);
    if menu(6, call, &data, 0) {
        call.set_result_bool(true);
    }
}
fn switch_to_vendor_after(call: &mut Call) {
    menu(12, call, &none(), 0);
}
fn switch_to_ripperdoc(call: &mut Call) {
    let data = call.arg_handle(0);
    if menu(7, call, &data, 0) {
        call.set_result_bool(true);
    }
}
fn switch_to_glamdoc(call: &mut Call) {
    let data = call.arg_handle(0);
    if menu(15, call, &data, 0) {
        call.set_result_bool(true);
    }
}
fn hub_init(call: &mut Call) {
    menu(8, call, &none(), 0);
}
fn hub_user_data(call: &mut Call) {
    let data = call.arg_handle(0);
    menu(9, call, &data, 0);
}
fn hub_uninit(call: &mut Call) {
    menu(10, call, &none(), 0);
}
fn hub_tabs(call: &mut Call) {
    menu(11, call, &none(), 0);
}
fn open_storage(call: &mut Call) {
    let data = call.arg_variant_handle(0);
    if menu(13, call, &data, 0) {
        call.set_result_bool(true);
    }
}
fn saves_ready(call: &mut Call) {
    let Some(m) = marker(call) else { return };
    let names = call.arg_strings(0).join("\n");
    super::menu_load::names_ready(m, &names);
    call.set_result_bool(true);
}
fn save_metadata(call: &mut Call) {
    if marker(call).is_none() {
        return;
    }
    let info = call.arg_handle(0);
    super::menu_load::metadata_ready(&call.this(), &info);
    call.set_result_bool(true);
}
fn game_attached(_call: &mut Call) {
    super::vrik::game_attached();
}

// ---- the lobby (mods/cet/CyberpunkVRPort_Lobby/init.lua's observers: VRLobbyMenu / VRLobbySaves)
fn lobby_menu_init(call: &mut Call) {
    super::lobby::menu_event(0, call.this());
}
fn lobby_menu_uninit(call: &mut Call) {
    super::lobby::menu_event(1, call.this());
}
fn lobby_saves(call: &mut Call) {
    let names = call.arg_strings(0).join("\n");
    super::lobby::saves(names);
}
fn lobby_root_enter(_call: &mut Call) {
    super::lobby::menu_event(5, Handle::null());
}
fn lobby_root_leave(_call: &mut Call) {
    super::lobby::menu_event(6, Handle::null());
}
fn creation_dispatcher(call: &mut Call) {
    super::lobby::menu_event(2, call.this());
    super::lobby::menu_event(3, call.arg_handle(0));
}
fn creation_uninit(call: &mut Call) {
    super::lobby::menu_event(4, call.this());
}
// ---- the HUDs and the maps
fn braindance_arm(_call: &mut Call) {
    super::braindance_hud::arm();
}
fn braindance_disarm(_call: &mut Call) {
    super::braindance_hud::disarm();
}
fn crosshair_container(call: &mut Call) {
    super::crosshair::capture(&call.this());
}
fn crosshair_update(_call: &mut Call) {
    super::crosshair::update();
}
fn world_map_on(_call: &mut Call) {
    super::world_map::CONTROLLER.store(true, Ordering::Relaxed);
}
fn world_map_off(_call: &mut Call) {
    super::world_map::CONTROLLER.store(false, Ordering::Relaxed);
}
fn popup_init(call: &mut Call) {
    let this = call.this();
    super::force_fpp::popup_initialized(&this);
    super::driving::modal_list(Some(this));
}
fn popup_uninit(_call: &mut Call) {
    super::driving::modal_list(None);
}
// ---- the locomotion states (CrouchEvents / Swimming* / Ladder*: OnEnter(stateContext, scriptInterface),
// OnUpdate(timeDelta, stateContext, scriptInterface))
fn crouch_enter(call: &mut Call) {
    let interface = call.arg_handle(1);
    let owner = super::lobby_rtti::field(&interface, "executionOwner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    super::physical_crouch::crouch_enter(&owner);
}
fn swim_slow(_call: &mut Call) {
    super::swimming::FAST.store(0, Ordering::Relaxed);
}
fn swim_fast(_call: &mut Call) {
    super::swimming::FAST.store(1, Ordering::Relaxed);
}
fn ladder_update(call: &mut Call) {
    let (context, interface) = (call.arg_handle(1), call.arg_handle(2));
    let owner = super::lobby_rtti::field(&interface, "owner").map(|v| v.handle()).unwrap_or_else(Handle::null);
    let using = super::lobby_rtti::A::Name(CName::hashed("usingLadder").0);
    let mut description = super::lobby_rtti::obj(&context, "GetConditionScriptableParameter", &[using]);
    if description.is_null() {
        let using = super::lobby_rtti::A::Name(CName::hashed("usingLadder").0);
        description = super::lobby_rtti::obj(&context, "GetTemporaryScriptableParameter", &[using]);
    }
    if owner.is_null() {
        super::ladder::clear_captured();
    } else {
        super::ladder::capture(owner, description);
    }
}
// ---- the player's attach and detach
fn attached_before(_call: &mut Call) {
    super::belt::attached();
    super::weapon::attached();
}
fn attached_after(_call: &mut Call) {
    super::force_fpp::attach(true);
    super::ladder::clear_captured();
    super::swimming::FAST.store(-1, Ordering::Relaxed);
}
/// A weapon part added or removed: the attachment recoil read again.
fn part_changed(_call: &mut Call) {
    super::attachment_recoil::invalidate();
}
fn detach_before(_call: &mut Call) {
    super::coop_props::forget();
    super::arm_b::player_detach();
    super::force_fpp::attach(false);
    super::hands_only::detach();
    super::holster::ask_release();
}

/// Once, from the first frame of the Running state.
pub(super) fn install_all() {
    if DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let hooks: &[(&str, &str, Kind, crate::script_hooks::HookFn)] = &[
        ("IWorldWidgetComponent", "StartGlitching", Kind::Override, glitch),
        ("gameuiInGameMenuGameController", "OnInitialize", Kind::After, in_game_menu_init),
        ("MenuScenario_Idle", "OnLeaveScenario", Kind::Before, idle_leave),
        ("MenuScenario_Vendor", "OnEnterScenario", Kind::Before, vendor_enter_before),
        ("MenuScenario_Vendor", "OnEnterScenario", Kind::After, vendor_enter_after),
        ("MenuScenario_Vendor", "OnLeaveScenario", Kind::Before, vendor_leave),
        ("UISystem", "RequestVendorMenu", Kind::Override, request_vendor_menu),
        ("MenuScenario_Vendor", "OnSwitchToVendor", Kind::Override, switch_to_vendor),
        ("MenuScenario_Vendor", "OnSwitchToVendor", Kind::After, switch_to_vendor_after),
        ("MenuScenario_Vendor", "OnSwitchToRipperDoc", Kind::Override, switch_to_ripperdoc),
        ("MenuScenario_Vendor", "OnSwitchToGlamdoc", Kind::Override, switch_to_glamdoc),
        ("VendorHubMenuGameController", "OnInitialize", Kind::Before, hub_init),
        ("VendorHubMenuGameController", "OnSetUserData", Kind::After, hub_user_data),
        ("VendorHubMenuGameController", "OnUninitialize", Kind::Before, hub_uninit),
        ("VendorHubMenuGameController", "SetupMenuTabs", Kind::After, hub_tabs),
        ("gameuiInGameMenuGameController", "OnOpenStorage", Kind::Override, open_storage),
        ("LoadGameMenuGameController", "OnSavesForLoadReady", Kind::Override, saves_ready),
        ("LoadGameMenuGameController", "OnSaveMetadataReady", Kind::Override, save_metadata),
        ("PlayerPuppet", "OnGameAttached", Kind::Before, game_attached),
        ("PlayerPuppet", "OnGameAttached", Kind::Before, attached_before),
        ("PlayerPuppet", "OnGameAttached", Kind::After, attached_after),
        ("PlayerPuppet", "OnDetach", Kind::Before, detach_before),
        ("PlayerPuppet", "OnPartAddedToSlotEvent", Kind::After, part_changed),
        ("PlayerPuppet", "OnPartRemovedFromSlotEvent", Kind::After, part_changed),
        ("SingleplayerMenuGameController", "OnInitialize", Kind::After, lobby_menu_init),
        ("SingleplayerMenuGameController", "OnUninitialize", Kind::Before, lobby_menu_uninit),
        ("SingleplayerMenuGameController", "OnSavesForLoadReady", Kind::After, lobby_saves),
        ("MenuScenario_SingleplayerMenu", "OnEnterScenario", Kind::After, lobby_root_enter),
        ("MenuScenario_SingleplayerMenu", "OnLeaveScenario", Kind::After, lobby_root_leave),
        ("BaseCharacterCreationController", "OnSetMenuEventDispatcher", Kind::After, creation_dispatcher),
        ("BaseCharacterCreationController", "OnUninitialize", Kind::Before, creation_uninit),
        ("BraindanceGameController", "OnInitialize", Kind::Before, braindance_arm),
        ("BraindanceGameController", "OnUnInitialize", Kind::Before, braindance_disarm),
        ("gameuiCrosshairContainerController", "OnInitialize", Kind::Before, crosshair_container),
        ("gameuiCrosshairBaseGameController", "OnCrosshairStateChange", Kind::Before, crosshair_update),
        ("gameuiCrosshairBaseGameController", "UpdateCrosshairState", Kind::Before, crosshair_update),
        ("WorldMapMenuGameController", "OnInitialize", Kind::After, world_map_on),
        ("WorldMapMenuGameController", "OnUninitialize", Kind::After, world_map_off),
        ("BaseModalListPopupGameController", "OnInitialize", Kind::After, popup_init),
        ("BaseModalListPopupGameController", "OnUninitialize", Kind::After, popup_uninit),
        ("CrouchEvents", "OnEnter", Kind::Before, crouch_enter),
        ("SwimmingSurfaceEvents", "OnEnter", Kind::After, swim_slow),
        ("SwimmingDivingEvents", "OnEnter", Kind::After, swim_slow),
        ("SwimmingSurfaceFastEvents", "OnEnter", Kind::After, swim_fast),
        ("SwimmingSurfaceFastEvents", "OnExit", Kind::After, swim_slow),
        ("SwimmingFastDivingEvents", "OnEnter", Kind::After, swim_fast),
        ("SwimmingFastDivingEvents", "OnExit", Kind::After, swim_slow),
        ("LadderEvents", "OnUpdate", Kind::After, ladder_update),
        ("LadderSprintEvents", "OnUpdate", Kind::After, ladder_update),
        ("LadderSlideEvents", "OnUpdate", Kind::After, ladder_update),
    ];
    let mut ok = 0;
    // a development switch: CVR_HOOKS_SKIP="Class.Function,..." leaves those to the shims' absence (a bisection aid)
    let skip = std::env::var("CVR_HOOKS_SKIP").unwrap_or_default();
    for (class, name, kind, hook) in hooks {
        if skip.split(',').any(|s| s == format!("{class}.{name}")) {
            cvr_host::log!("[mechanics] game hooks: {class}.{name} skipped (CVR_HOOKS_SKIP)\n");
            continue;
        }
        if install(class, name, *kind, *hook) {
            ok += 1;
        }
    }
    cvr_host::log!("[mechanics] game hooks: {ok} of {} installed from Rust\n", hooks.len());
}
