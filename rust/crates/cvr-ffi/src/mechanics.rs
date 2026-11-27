//! The mechanics the port's CET mods ran in Lua, moved to Rust (stage 6 of docs/rust-migration-plan.md). They tick on
//! the game's main thread every frame -- a Running game state's update, where CET's running task ticked the mods'
//! onUpdate -- with the time since the previous tick, as CET measured it. A mod yields its part when the plugin says
//! it owns it (the native VRRustMechanics: a bit per mechanic); the reference build has no such native, so its mods
//! run as before.
use core::ffi::c_void;
use std::sync::Mutex;
use std::time::Instant;

use red4ext_abi::api::GameApplication;
use red4ext_abi::stack::StackFrame;

/// VRRustMechanics' bits: the mechanics whose Lua the plugin replaces.
pub const SMOKING: i32 = 1 << 0;
pub const BASKETBALL: i32 = 1 << 1;
pub const WORLD_MAP: i32 = 1 << 2;
pub const PHYSICAL_CROUCH: i32 = 1 << 3;
pub const SWIMMING: i32 = 1 << 4;
pub const LADDER: i32 = 1 << 5;
pub const CROSSHAIR: i32 = 1 << 6;
pub const DRIVING: i32 = 1 << 7;
pub const HUD_CLEARANCE: i32 = 1 << 8;
pub const INTERACTION_DEPTH: i32 = 1 << 9;
pub const STORY_ATTENTION: i32 = 1 << 10;
pub const KEYPAD_INPUT: i32 = 1 << 11;
pub const BRAINDANCE_HUD: i32 = 1 << 12;
pub const HUD_PANEL: i32 = 1 << 13;
pub const VRCAM_SELECT: i32 = 1 << 14;
pub const VR_OVERLAY: i32 = 1 << 15;
pub const WORLD_TOUCH: i32 = 1 << 16;
pub const REACH: i32 = 1 << 17;
pub const OPTICS: i32 = 1 << 18;
pub const VRIK_PUMP: i32 = 1 << 19;
pub const HANDS_ONLY: i32 = 1 << 20;
pub const HAND_COLLISION: i32 = 1 << 21;
pub const WRIST_HUD: i32 = 1 << 22;
pub const CAMERA_BRIDGE: i32 = 1 << 23;
pub const WEAPON_FRAME: i32 = 1 << 24;
pub const WEAPON_COMBAT: i32 = 1 << 25;
pub const HOLSTER: i32 = 1 << 26;
pub const BELT: i32 = 1 << 27;
pub const RELOAD: i32 = 1 << 28;
pub const LOBBY: i32 = 1 << 29;
pub const MENU: i32 = 1 << 30;
const OWNED: i32 = SMOKING
    | BASKETBALL
    | WORLD_MAP
    | PHYSICAL_CROUCH
    | SWIMMING
    | LADDER
    | CROSSHAIR
    | DRIVING
    | HUD_CLEARANCE
    | INTERACTION_DEPTH
    | STORY_ATTENTION
    | KEYPAD_INPUT
    | BRAINDANCE_HUD
    | HUD_PANEL
    | VRCAM_SELECT
    | VR_OVERLAY
    | WORLD_TOUCH
    | REACH
    | OPTICS
    | VRIK_PUMP
    | HANDS_ONLY
    | HAND_COLLISION
    | WRIST_HUD
    | CAMERA_BRIDGE
    | WEAPON_FRAME
    | WEAPON_COMBAT
    | HOLSTER
    | BELT
    | RELOAD
    | LOBBY
    | MENU;

mod ball;
mod belt;
mod belt_flight;
mod ent_spawn;
mod belt_grenade;
mod belt_pin_contact;
mod belt_poses;
mod boards;
mod braindance_hud;
mod force_fpp;
mod hand_collision;
mod vehicle_hands;
mod blade;
mod blade_probe;
mod subtitle_probe;
mod car_hands_probe;
mod hold;
mod holster;
mod holster_data;
mod hands_only;
mod hands_only_data;
mod hud_clearance;
mod hud_panel;
mod hud_hints;
mod interaction_depth;
mod keypad_input;
mod optics;
mod cyberware;
mod left_handed;
mod melee;
mod melee_throw;
mod world_markers;
mod lobby;
mod no_anims;
mod fight_test;
mod reload_probe;
mod reload_reach;
mod dev_cmd;
pub(crate) mod quickboot;
pub(crate) mod arm_b;
mod aim_threat;
mod attachment_recoil;
pub(crate) mod launcher_ready;
pub(crate) mod journal_watch;
pub(crate) mod engine_services;
mod workspot;
mod wilson_range;
pub(crate) mod hotkeys;
pub(crate) mod dev_overlay;
mod dev_windows;
mod wrist_hud_ui;
mod reload_recorder;
#[allow(dead_code)]
mod lobby_creation;
#[allow(dead_code)]
mod lobby_engrams;
mod lobby_hands;
#[allow(dead_code)]
mod lobby_holo;
#[allow(dead_code)]
mod lobby_ink;
mod lobby_intro;
#[allow(dead_code)]
mod lobby_items;
mod lobby_lights;
#[allow(dead_code)]
mod lobby_menu;
mod lobby_controls;
mod lobby_coop;
mod lobby_keyboard;
mod lobby_rtti;
mod lobby_scene;
#[allow(dead_code)]
mod lobby_scroll;
mod lobby_setup;
mod lobby_spawn;
#[allow(dead_code)]
mod menu_bits;
#[allow(dead_code)]
mod menu_core;
#[allow(dead_code)]
mod menu_data;
#[allow(dead_code)]
mod menu_pictures;
#[allow(dead_code)]
mod menu_ink;
#[allow(dead_code)]
mod menu_main;
#[allow(dead_code)]
mod menu_wheel;
#[allow(dead_code)]
mod menu_crash_probe;
#[allow(dead_code)]
mod menu_invview;
#[allow(dead_code)]
mod menu_character;
#[allow(dead_code)]
mod menu_journal;
#[allow(dead_code)]
mod menu_inventory;
#[allow(dead_code)]
mod menu_cyberware;
#[allow(dead_code)]
mod menu_crafting;
#[allow(dead_code)]
mod menu_trade;
mod menu_appearance;
pub(crate) mod menu_tooltip;
mod menu_glamdoc;
#[allow(dead_code)]
mod menu_stashview;
#[allow(dead_code)]
mod menu_load;
#[allow(dead_code)]
mod menu_rt;
#[allow(dead_code)]
mod menu_jobs;
#[allow(dead_code)]
mod menu_gpdata;
#[allow(dead_code)]
mod menu_perk_layout;
#[allow(dead_code)]
mod menu_messages;
#[allow(dead_code)]
mod menu_setview;
#[allow(dead_code)]
mod menu_gameplay;
#[allow(dead_code)]
mod menu_invdata;
#[allow(dead_code)]
mod menu_vendor;
#[allow(dead_code)]
mod menu_stash;
mod menu_locale;
#[allow(dead_code)]
mod menu_settings;
mod reach;
mod reload;
mod reload_core;
mod reload_data;
mod reload_dual;
mod reload_flick;
mod reload_frame;
mod reload_glow;
mod reload_mag;
mod reload_parts;
mod reload_revolver;
mod reload_shotgun;
mod scanner_hud;
mod smoke;
mod story_attention;
mod view;
mod vr_overlay;
mod vrcam_select;
mod vrik;
mod vrik_mirror;
mod coop_gate;
mod coop_props;
mod game_hooks;
pub(crate) mod reds_hooks;
mod weapon;
mod weapon_combat;
mod weapon_wrist;
mod world_touch;
mod wrist_hud;
mod wrist_hud_spec;

/// VRRustMechanics() -> Int32: the mechanics Rust owns, a bit each (mods/cet/*: their onUpdate yields).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRRustMechanics(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<i32>() = OWNED;
        }
    }
}

/// VRRustMechanics2's bits (VRRustMechanics' 31 are taken): the mechanics whose Lua the plugin replaces.
pub const VRIK_REST: i32 = 1 << 0;
/// The game hooks of the CET shims (mechanics/game_hooks.rs): the Menu's and VRIK's Observe / Override.
pub const GAME_HOOKS: i32 = 1 << 1;
const OWNED2: i32 = VRIK_REST | GAME_HOOKS;

/// VRRustMechanics2() -> Int32: more mechanics Rust owns, a bit each (VRIK_REST: CyberpunkVRPort_VRIK's mirror double
/// and EquipCyberware).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRRustMechanics2(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<i32>() = OWNED2;
        }
    }
}

/// VRIKEquipCyberware(kind) -> String: GetMod("CyberpunkVRPort_VRIK").EquipCyberware's change begun from Rust --
/// "ok:<record>", else the reason.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRIKEquipCyberware(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let mut kind = red4ext_abi::cstring::CString::empty();
    // SAFETY: the engine's frame and result slot (a constructed String).
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut kind);
            frame.finish_params();
        }
        let answer = match vrik_mirror::equip_cyberware(kind.as_str()) {
            Ok(record) => format!("ok:{record}"),
            Err(reason) => reason,
        };
        if !out.is_null() {
            red4ext_abi::cstring::CString::assign(out.cast(), &answer);
        }
    }
}

/// The basketball's console (formerly PlayerPuppet's VRBallToggle, VRBallSetCorrection, VRBallDropTest,
/// VRBallTestThrow, VRBallReport and VRBallStatus of mods/redscript/CyberpunkVRPort_Basketball): global natives.
///
/// # Safety
/// The engine's native calls.
#[allow(non_snake_case)]
pub unsafe extern "C" fn VRBallToggle(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    if let Some(frame) = unsafe { frame.as_mut() } {
        frame.finish_params();
    }
    ball::toggle();
}
/// See [`VRBallToggle`].
///
/// # Safety
/// The engine's native call.
#[allow(non_snake_case)]
pub unsafe extern "C" fn VRBallSetCorrection(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut on = false;
    // SAFETY: the engine's frame: one Bool.
    if let Some(frame) = unsafe { frame.as_mut() } {
        unsafe { frame.get_parameter(&mut on) };
        frame.finish_params();
    }
    ball::set_correction(on);
}
/// See [`VRBallToggle`].
///
/// # Safety
/// The engine's native call.
#[allow(non_snake_case)]
pub unsafe extern "C" fn VRBallDropTest(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut height = 0f32;
    // SAFETY: the engine's frame: one Float.
    if let Some(frame) = unsafe { frame.as_mut() } {
        unsafe { frame.get_parameter(&mut height) };
        frame.finish_params();
    }
    ball::drop_test(f64::from(height));
}
/// See [`VRBallToggle`].
///
/// # Safety
/// The engine's native call.
#[allow(non_snake_case)]
pub unsafe extern "C" fn VRBallTestThrow(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut a = [0f32; 4];
    // SAFETY: the engine's frame: four Floats.
    if let Some(frame) = unsafe { frame.as_mut() } {
        for v in &mut a {
            unsafe { frame.get_parameter(v) };
        }
        frame.finish_params();
    }
    ball::test_throw([f64::from(a[0]), f64::from(a[1]), f64::from(a[2])], f64::from(a[3]));
}
/// See [`VRBallToggle`].
///
/// # Safety
/// The engine's native call.
#[allow(non_snake_case)]
pub unsafe extern "C" fn VRBallReport(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot (a constructed String).
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            red4ext_abi::cstring::CString::assign(out.cast(), &ball::report());
        }
    }
}
/// See [`VRBallToggle`].
///
/// # Safety
/// The engine's native call.
#[allow(non_snake_case)]
pub unsafe extern "C" fn VRBallStatus(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot (a constructed String).
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            red4ext_abi::cstring::CString::assign(out.cast(), &ball::status());
        }
    }
}

/// VRIKCyberwareStatus() -> String: the change's state (idle, holstering, equipping, drawing, ready, an error).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRIKCyberwareStatus(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot (a constructed String).
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            red4ext_abi::cstring::CString::assign(out.cast(), &vrik_mirror::cyberware_status());
        }
    }
}

/// A report made once per player: the main menu's puppet, then the loaded save's player (the A/B's QuickBoot loads
/// the latest save), so a chain proven at the menu is proven again in the world, where the rays hit and the boards
/// hold a session's state.
pub(crate) struct PerPlayer(std::sync::atomic::AtomicUsize);
impl PerPlayer {
    pub(crate) const fn new() -> PerPlayer {
        PerPlayer(std::sync::atomic::AtomicUsize::new(0))
    }
    /// The current player is not reported yet (nothing marked).
    pub(crate) fn pending(&self) -> bool {
        let player = crate::anim_lookup::player_entity();
        player != 0 && self.0.load(std::sync::atomic::Ordering::Relaxed) != player
    }
    /// Marks the current player reported: true when it was not.
    pub(crate) fn first(&self) -> bool {
        let player = crate::anim_lookup::player_entity();
        player != 0 && self.0.swap(player, std::sync::atomic::Ordering::Relaxed) != player
    }
}

/// The built-in Lua host is not installed (plugin.rs: the Rust build): what its start did is Rust's -- the state
/// machine's action reads among it (reds_hooks.rs).
pub(crate) static NO_LUA_HOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The time between ticks (CET's running task: the clock since its previous run).
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether the mechanics tick yet: from the first frame with a player (the main menu's puppet included). The mods'
/// onUpdate began only once CET was up at the main menu; during the boot before it the menu scenario and the game's
/// systems are not there to be asked (calling inkMenuScenario's statics then took the game down).
static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The Running state's update: every mechanic, once a frame on the game's main thread.
///
/// # Safety
/// RED4ext's game state callback.
pub unsafe extern "C" fn update(_app: *mut GameApplication) -> bool {
    // Nothing unwinds out of an extern "C" callback: a panic here aborted the game with no crash report (a blade's
    // RefCell borrowed twice did), and the guard round the call in the loader never saw it. False keeps the callback
    // for the next frame.
    let span = cvr_host::hitch::start();
    let keep = cvr_host::guard::guard("the mechanics' update", false, update_frame);
    cvr_host::hitch::end(span, 8.0, || "the mechanics' whole update".to_string());
    keep
}

/// Each mechanic's tick under its own guard, as CET ran each mod's onUpdate under its own pcall: a panic in one is
/// logged (log/panic.txt names it) and the others still tick this frame.
macro_rules! each {
    ($($tick:expr;)*) => {
        $(
            let span = cvr_host::hitch::start();
            cvr_host::guard::guard(stringify!($tick), (), || { $tick; });
            cvr_host::hitch::end(span, 4.0, || format!("mechanic {}", stringify!($tick)));
        )*
    };
}

fn update_frame() -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    {
        // The lobby is the main menu's: it ticks before the world's player exists.
        static LOBBY_LAST: Mutex<Option<Instant>> = Mutex::new(None);
        let now = Instant::now();
        let mut last = LOBBY_LAST.lock().unwrap_or_else(|p| p.into_inner());
        let dt = last.map_or(0.016, |then| now.duration_since(then).as_secs_f32());
        *last = Some(now);
        drop(last);
        // the order of a function's hooks is the hosts' (script_hooks: Hook::inner), whichever is installed first;
        // with NightCoop in the process, after its own wraps (coop_gate.rs)
        if coop_gate::hooks_may_install() {
            reds_hooks::install_all();
            game_hooks::install_all();
            coop_gate::tick();
            // the hooked and defined functions' signatures as the engine has them, for tools/audit/check_reads.py
            // (written again only when their number changed; with the verbose diagnostics)
            if cvr_host::log::verbose() {
                crate::script_hooks::dump_signatures(&cvr_host::paths::dir(cvr_host::paths::Folder::Log).join("audit_signatures.txt"));
            }
            // the guarded reads' first-chance handler at the head of the list again, once the game has added its own
            static AHEAD: std::sync::Once = std::sync::Once::new();
            AHEAD.call_once(cvr_engine::install_first_chance);
            crate::audit::probe_read();
        }
        each! {
            lobby::tick(dt);
            // the VR menu and the weapon wheel tick in the main menu too (their own goodbye when the player goes)
            menu_main::tick_frame(dt);
        }
    }
    if !READY.load(Relaxed) {
        if crate::anim_lookup::player_entity() == 0 {
            return false;
        }
        READY.store(true, Relaxed);
        cvr_host::log!("[mechanics] ticking from the first frame with a player\n");
        check_lookups();
    }
    let now = Instant::now();
    let dt = {
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        let dt = last.map(|then| now.duration_since(then).as_secs_f32());
        *last = Some(now);
        dt
    };
    let dt = dt.unwrap_or(0.016);
    each! {
        vrik::tick(dt);
        vrik_mirror::tick(f64::from(dt));
        coop_props::tick();
        world_markers::coop_markers_tick();
        force_fpp::tick(dt);
        weapon::tick(dt);
        weapon_combat::tick(dt);
        holster::tick(dt);
        belt::tick(dt);
        reload::tick(dt);
        smoking::tick(dt);
        basketball::tick(dt);
        scanner_hud::tick(f64::from(dt));
        no_anims::self_test(f64::from(dt));
        fight_test::tick(f64::from(dt));
        reload_probe::drive(f64::from(dt));
        left_handed::self_test(f64::from(dt));
        hold::self_test(f64::from(dt));
        melee::self_test(f64::from(dt));
        quickboot::tick(f64::from(dt));
        launcher_ready::self_test(f64::from(dt));
        hotkeys::tick();
        reload_recorder::self_test(f64::from(dt));
        reload::ammo_test(f64::from(dt));
        menu_vendor::vendor_test(f64::from(dt));
        dev_cmd::tick(f64::from(dt));
        reload_recorder::tick(f64::from(dt));
        world_map::tick();
        physical_crouch::tick(dt);
        swimming::tick();
        ladder::tick(dt);
        driving::tick(dt);
        vehicle_hands::tick(dt);
        arm_b::tick(dt);
        aim_threat::tick();
        wilson_range::tick();
        vrcam_select::tick();
        hud_panel::tick();
        hud_hints::tick(dt);
        vr_overlay::tick();
        keypad_input::tick(dt);
        world_touch::tick(dt);
        reach::tick(dt);
        optics::tick(dt);
        hands_only::tick(dt);
        hand_collision::tick(dt);
        blade_probe::tick();
        blade_probe::combat_tick();
        blade_probe::damage_tick(f64::from(dt));
        subtitle_probe::tick();
        car_hands_probe::tick();
        wrist_hud::tick(dt);
        braindance_hud::tick(dt);
        hud_clearance::tick(dt);
        story_attention::tick(dt);
        interaction_depth::tick(dt);
    }
    false
}

/// Once, from the first frame with a player: every class static, scripted static, game system getter and plugin
/// native the mechanics call, resolved -- a missing one reported by name (a path the A/B never takes fails here
/// visibly instead of silently in a player's game).
fn check_lookups() {
    use crate::script_call::static_function;
    const STATICS: &[(&str, &str)] = &[
        ("ScriptedPuppet", "GetActiveWeapon"),
        ("WidgetUtils", "LocalToGlobal"),
        ("StatusEffectHelper", "ApplyStatusEffect"),
        ("StatusEffectSystem", "ObjectHasStatusEffectWithTag"),
        ("AnimationControllerComponent", "SetInputFloat"),
        ("AnimationControllerComponent", "SetInputVector"),
        ("AnimationControllerComponent", "ApplyFeatureToReplicate"),
        ("inkWidgetRef", "Get"),
        ("TS_TargetPartInfo", "GetComponent"),
        ("Matrix", "GetTranslation"),
        ("Matrix", "ToQuat"),
        ("inkMenuScenario", "GetSystemRequestsHandler"),
        ("ScriptGameInstance", "FindEntityByID"),
        ("ScriptGameInstance", "GetScriptableSystemsContainer"),
        ("ScriptGameInstance", "GetSettingsSystem"),
    ];
    const SYSTEMS: &[&str] = &[
        "GetTargetingSystem",
        "GetTransactionSystem",
        "GetBlackboardSystem",
        "GetStatusEffectSystem",
        "GetCameraSystem",
        "GetWorkspotSystem",
        "GetQuestsSystem",
        "GetSpatialQueriesSystem",
        "GetStatsSystem",
    ];
    const NATIVES: &[&str] = &[
        "UpdateVRIKAnimInputs",
        "SetVRPlayerYaw",
        "RestoreVRFppArms",
        "SetVRCamAck",
        "VRNeckCapSync",
        "SetVRLocomotionState",
        "SetVRLocomotionDetailed",
        "SetVRScannerState",
        "SetVRWorldTimeScale",
        "SetVRPairedAnimationState",
        "GetVRMouseYDisabled",
        "SetVRDiagCapture",
        "LogVRDiag",
    ];
    let mut missing = Vec::new();
    for (class, name) in STATICS {
        if static_function(class, name).is_none() && crate::script_call::native_class_function(class, name).is_none() {
            missing.push(format!("{class}.{name}"));
        }
    }
    for getter in SYSTEMS {
        if crate::script_call::native_class_function("ScriptGameInstance", getter).is_none() {
            missing.push(format!("ScriptGameInstance.{getter}"));
        }
    }
    let rtti = red4ext_abi::rtti::RttiSystem::get();
    for native in NATIVES {
        if rtti.and_then(|r| r.get_function(red4ext_abi::cname::CName::hashed(native))).is_none() {
            missing.push((*native).to_string());
        }
    }
    let total = STATICS.len() + SYSTEMS.len() + NATIVES.len();
    cvr_host::log!(
        "[mechanics] lookups: {}/{total} resolved{}\n",
        total - missing.len(),
        if missing.is_empty() { String::new() } else { format!(" -- MISSING: {}", missing.join(", ")) }
    );
}

/// The Running state's exit (CET's onShutdown for the mods): what a mechanic leaves set, cleared.
///
/// # Safety
/// RED4ext's game state callback.
pub unsafe extern "C" fn exit(_app: *mut GameApplication) -> bool {
    each! {
        world_map::shutdown();
        physical_crouch::shutdown();
        swimming::shutdown();
        ladder::shutdown();
        driving::shutdown();
        vehicle_hands::shutdown();
        arm_b::shutdown();
        aim_threat::shutdown();
        wilson_range::shutdown();
        story_attention::shutdown();
        keypad_input::shutdown();
        hud_panel::shutdown();
        vr_overlay::shutdown();
        world_touch::shutdown();
        reach::shutdown();
        optics::shutdown();
        hands_only::shutdown();
        hand_collision::shutdown();
        weapon::shutdown();
        holster::shutdown();
        belt::shutdown();
        reload::shutdown();
        lobby::shutdown();
        menu_main::shutdown();
        vrik_mirror::shutdown();
        reload_recorder::shutdown();
    }
    true
}

/// VRLadderCapture(owner, description) -> Void: a ladder state's OnUpdate seen by the CET mod's observers -- the
/// state machine's owner and its "usingLadder" description (the hooks and the parameter's lookup stay the Lua host's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRLadderCapture(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut owner, mut description) = (red4ext_abi::handle::Handle::null(), red4ext_abi::handle::Handle::null());
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut owner as *mut red4ext_abi::handle::Handle);
            frame.get_parameter(&mut description as *mut red4ext_abi::handle::Handle);
            frame.finish_params();
        }
    }
    ladder::capture(owner, description);
}
/// VRCrosshairContainer(container) -> Void: the crosshair container controller initialised (the CET mod's Observe
/// on its OnInitialize reports it; the hook stays the Lua host's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRCrosshairContainer(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut container = red4ext_abi::handle::Handle::null();
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut container as *mut red4ext_abi::handle::Handle);
            frame.finish_params();
        }
    }
    crosshair::capture(&container);
}
/// VRCrosshairUpdate() -> Void: a crosshair state change or update (the CET mod's two Observe callbacks).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRCrosshairUpdate(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    crosshair::update();
}
/// VRBraindanceHudArm() -> Void: the braindance controller's OnInitialize (the CET mod's Observe; the hook stays the
/// Lua host's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBraindanceHudArm(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    braindance_hud::arm();
}
/// VRBraindanceHudDisarm() -> Void: the braindance controller's OnUnInitialize.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBraindanceHudDisarm(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    braindance_hud::disarm();
}
/// VRVrcamReload() -> Void: the Stereo mod's hotkey (vrcam.json read again, applied once more, the status logged).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRVrcamReload(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    vrcam_select::reload();
    cvr_host::log!("[Stereo.VRCAM] {}\n", vrcam_select::status());
}
/// VRVrcamSettled() -> Bool: the eyes are settled on the player in hand (Optics' stereoBound, from the VRCAM selection
/// the plugin runs).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRVrcamSettled(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<bool>() = vrcam_select::settled();
        }
    }
}
/// VRStereoPreGame() -> Bool: the menu or no session, as the VRCAM selection last saw it (the Lua host's GC pacing).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRStereoPreGame(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<bool>() = vrcam_select::pre_game();
        }
    }
}
/// The Reach mod's mark as its Lua shim hands it: a hand (1 left, 2 right), the thing's centre (W 0: no mark, for a
/// pin), its radius and lift (below -1e8: not given), the kind (0 near, 1 aim, 2 lock, 3 pull), ready, the hand's place
/// (W 0: not given).
unsafe fn reach_mark_args(frame: *mut StackFrame) -> (i32, Option<reach::Mark>) {
    let (mut hand, mut pos, mut radius, mut lift, mut kind, mut ready, mut from) = (0i32, [0f32; 4], -1e9f32, -1e9f32, 0i32, true, [0f32; 4]);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut hand as *mut i32);
            frame.get_parameter(&mut pos as *mut [f32; 4]);
            frame.get_parameter(&mut radius as *mut f32);
            frame.get_parameter(&mut lift as *mut f32);
            frame.get_parameter(&mut kind as *mut i32);
            frame.get_parameter(&mut ready as *mut bool);
            frame.get_parameter(&mut from as *mut [f32; 4]);
            frame.finish_params();
        }
    }
    let given = |v: f32| (v > -1e8).then_some(v as f64);
    let kind = match kind {
        1 => reach::Kind::Aim,
        2 => reach::Kind::Lock,
        3 => reach::Kind::Pull,
        _ => reach::Kind::Near,
    };
    let mark = (pos[3] > 0.5).then(|| reach::Mark {
        pos: [pos[0] as f64, pos[1] as f64, pos[2] as f64],
        radius: given(radius),
        kind,
        ready,
        from: (from[3] > 0.5).then(|| [from[0] as f64, from[1] as f64, from[2] as f64]),
        lift: given(lift),
    });
    (hand, mark)
}
/// VRReachMark(hand, pos, radius, lift, kind, ready, from) -> Void: another mod's mark for this frame (Reach's M.mark).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRReachMark(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    let (hand, mark) = unsafe { reach_mark_args(frame) };
    if let Some(mark) = mark {
        reach::mark(hand as usize, mark);
    }
}
/// VRReachPin(hand, pos, radius, lift, kind, ready, from) -> Void: Reach's M.pin (tests; pos W 0 unpins).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRReachPin(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    let (hand, mark) = unsafe { reach_mark_args(frame) };
    reach::pin(hand as usize, mark);
}
/// VRReachHold(point, seconds) -> Void: Reach's M.hold (tests).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRReachHold(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut point, mut seconds) = ([0f32; 4], 1f32);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut point as *mut [f32; 4]);
            frame.get_parameter(&mut seconds as *mut f32);
            frame.finish_params();
        }
    }
    reach::hold([point[0] as f64, point[1] as f64, point[2] as f64], seconds as f64);
}
/// VRIKGameAttached() -> Void: the player attached to the world (the VRIK mod's observer on OnGameAttached).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRIKGameAttached(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    vrik::game_attached();
}
/// VRIKTrackingEnabled(on) -> Void: the ToggleVRHands hotkey's state.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRIKTrackingEnabled(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut on = false;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut on as *mut bool);
            frame.finish_params();
        }
    }
    vrik::TRACKING.store(on, std::sync::atomic::Ordering::Relaxed);
}
/// VRIKLogDiag() -> Void: the LogVRDiag hotkey, with the camera pose the pump last read.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRIKLogDiag(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    vrik::log_diag();
}
/// VRHandsOnlyDetach() -> Void: the player detaches (the HandsOnly mod's ObserveBefore on OnDetach).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRHandsOnlyDetach(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    hands_only::detach();
}
/// VRHandsOnlyForce(force, scene) -> Void: the console's force(v)/scene(v) (1 true, 0 false, -1 follow, -2 unchanged).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRHandsOnlyForce(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut force, mut scene) = (-2i32, -2i32);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut force as *mut i32);
            frame.get_parameter(&mut scene as *mut i32);
            frame.finish_params();
        }
    }
    hands_only::force(force, scene);
}
/// VRBeltAttached() -> Void: the player attached (the belt's observer): its caches dropped.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBeltAttached(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    belt::attached();
}
/// VRBeltMagTaken(on) -> Void: the reload has the weapon's magazine from the pouch in a hand (the pouch draws empty).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBeltMagTaken(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut on = false;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut on as *mut bool);
            frame.finish_params();
        }
    }
    belt::set_mag_taken(on);
}
/// VRBeltShellTaken(on, side) -> Void: the reload carries the nearest loose shell of that hand (it draws empty).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBeltShellTaken(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut on, mut side) = (false, 0i32);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut on as *mut bool);
            frame.get_parameter(&mut side as *mut i32);
            frame.finish_params();
        }
    }
    belt::set_shell_taken(on, side);
}
/// VRLobbyMenu(what, object) -> Void: the lobby's observers of the game's menus (CyberpunkVRPort_Lobby/init.lua): 0 the
/// main menu's controller up, 1 down, 2 the new game's screen up, 3 its dispatcher, 4 its screen down, 5 the menu at
/// its root, 6 on one of its own screens.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyMenu(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut what, mut obj) = (0i32, red4ext_abi::handle::Handle::null());
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut what as *mut i32);
            frame.get_parameter(&mut obj as *mut red4ext_abi::handle::Handle);
            frame.finish_params();
        }
    }
    lobby::menu_event(what, obj);
}
/// VRMenuHook(kind, a, b, name) -> Bool: one of the game's hooks the VR menu's Lua host keeps (the vendor routing, the
/// stash, the in-game menu controller; CyberpunkVRPort_Menu/init.lua) -- true where an Override gives its own result.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRMenuHook(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let (mut kind, mut a, mut b, mut name) = (0i32, red4ext_abi::handle::Handle::null(), red4ext_abi::handle::Handle::null(), 0u64);
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut kind as *mut i32);
            frame.get_parameter(&mut a as *mut red4ext_abi::handle::Handle);
            frame.get_parameter(&mut b as *mut red4ext_abi::handle::Handle);
            frame.get_parameter(&mut name as *mut u64);
            frame.finish_params();
        }
        let r = if kind == 16 {
            menu_load::metadata_ready(&a, &b);
            true
        } else {
            menu_vendor::hook(kind, &a, &b, name)
        };
        if !out.is_null() {
            *out.cast::<bool>() = r;
        }
    }
}
/// VRMenuLoadNames(marker, names) -> Bool: the menu's own save listener's OnSavesForLoadReady (one name a line).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRMenuLoadNames(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let mut marker = 0f32;
    let mut text = red4ext_abi::cstring::CString::empty();
    // SAFETY: the engine's frame and result slot; the text is the engine's, destroyed by its own destructor.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut marker as *mut f32);
            frame.get_parameter(&mut text);
            frame.finish_params();
        }
        let r = menu_load::names_ready(f64::from(marker), text.as_str());
        if !out.is_null() {
            *out.cast::<bool>() = r;
        }
    }
}
/// VRLobbySaves(names) -> Void: the main menu's saves, newest first, one per line (OnSavesForLoadReady).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRLobbySaves(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut text = red4ext_abi::cstring::CString::empty();
    // SAFETY: the engine's frame; the text is the engine's, destroyed by its own destructor.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut text);
            frame.finish_params();
        }
    }
    lobby::saves(text.as_str().to_string());
}
/// VRReloadTune(hold, dx, dy, dz, rx, ry, rz, stage, seat, lead, rev) -> Void: the Lua host's magazine tuner window
/// (M.tune) as it stands -- the reload's frame reads it.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRReloadTune(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut hold = false;
    let mut f = [0f32; 9];
    let mut rev = 0i32;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut hold as *mut bool);
            for v in f.iter_mut() {
                frame.get_parameter(v as *mut f32);
            }
            frame.get_parameter(&mut rev as *mut i32);
            frame.finish_params();
        }
    }
    reload::set_tune(hold, [f[0], f[1], f[2]], [f[3], f[4], f[5]], f[6], f[7], f[8], rev);
}
/// VRReloadTuneJoint(i, w) -> Void: one finger joint's weight in the tuner (1..19).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRReloadTuneJoint(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut i, mut w) = (0i32, 0f32);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut i as *mut i32);
            frame.get_parameter(&mut w as *mut f32);
            frame.finish_params();
        }
    }
    reload::set_tune_joint(i, w);
}
/// VRBeltReach(what, side) -> Float: the reload's questions -- 0 the weapon's magazine pouch key (its index in the belt's
/// list, -1 none), 1 the palm's distance to it, 2 to the nearest loose shell (negative: none), 3 the reach radius.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBeltReach(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let (mut what, mut side) = (0i32, 0i32);
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut what as *mut i32);
            frame.get_parameter(&mut side as *mut i32);
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<f32>() = match what {
                0 => belt::mag_key() as f32,
                1 => belt::mag_dist(side),
                2 => belt::shell_dist(side),
                _ => belt::reach_radius_published(),
            };
        }
    }
}
/// VRHolsterRelease() -> Void: the player detaching (the Holster mod's observer): the copies and shells let go.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRHolsterRelease(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    holster::ask_release();
}
/// VRHolsterNear(x, y, z, side) -> Float: the belt's question -- the distance from a model-space point to the nearest
/// holstered weapon hand `side` could take there, within its reach; negative when none.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRHolsterNear(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z, mut side) = (0f32, 0f32, 0f32, 0i32);
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut x as *mut f32);
            frame.get_parameter(&mut y as *mut f32);
            frame.get_parameter(&mut z as *mut f32);
            frame.get_parameter(&mut side as *mut i32);
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<f32>() = holster::near(x, y, z, side);
        }
    }
}
/// VRBeltGrenadeReach(left, right) -> Void: the belt's lit grenade holders per hand (the grip is the grenade's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRBeltGrenadeReach(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut left, mut right) = (false, false);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut left as *mut bool);
            frame.get_parameter(&mut right as *mut bool);
            frame.finish_params();
        }
    }
    holster::belt_reach(left, right);
}
/// VRWeaponAttached() -> Void: the player attached (the Weapon mod's observer): a loaded save's weapon is watched for.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponAttached(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    weapon::attached();
}
/// VRCameraBridgeAttach(attached) -> Void: the player attached or is detaching (the camera bridge's Lua observers).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRCameraBridgeAttach(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut attached = false;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut attached as *mut bool);
            frame.finish_params();
        }
    }
    force_fpp::attach(attached);
}
/// VRCameraBridgeKnobs(sceneGate, sceneInTakeover) -> Void: the camera bridge overlay's A/B knobs.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRCameraBridgeKnobs(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let (mut gate, mut takeover) = (true, false);
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut gate as *mut bool);
            frame.get_parameter(&mut takeover as *mut bool);
            frame.finish_params();
        }
    }
    force_fpp::SCENE_GATE.store(gate, std::sync::atomic::Ordering::Relaxed);
    force_fpp::SCENE_IN_TAKEOVER.store(takeover, std::sync::atomic::Ordering::Relaxed);
}
/// VRCameraBridgeContext() -> Int32: what the overlay's viewpoint sliders edit (bit 0 the AV context, bit 1 a takeover).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRCameraBridgeContext(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<i32>() = force_fpp::CONTEXT.load(std::sync::atomic::Ordering::Relaxed);
        }
    }
}
/// VRMenuModOpen(open) -> Void: the port's Menu (mods/cet/CyberpunkVRPort_Menu) opened or closed, as it reports it.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRMenuModOpen(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut open = false;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut open as *mut bool);
            frame.finish_params();
        }
    }
    world_touch::MENU_MOD_OPEN.store(open, std::sync::atomic::Ordering::Relaxed);
}
/// VRStereoOverlayOpen() -> Bool: whether the VR menu is open (VrOverlay.active for the Lua modules still in Lua).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRStereoOverlayOpen(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<bool>() = hud_clearance::OVERLAY_ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
        }
    }
}
/// VRStereoOverlayActive(active) -> Void: whether the VR menu is open (the Stereo mod's VrOverlay.active, reported
/// every frame; its menu stays the Lua host's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRStereoOverlayActive(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut active = false;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut active as *mut bool);
            frame.finish_params();
        }
    }
    hud_clearance::OVERLAY_ACTIVE.store(active, std::sync::atomic::Ordering::Relaxed);
}
/// VRLadderClear() -> Void: the capture failed, or the player was attached again (the mod's clear()).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRLadderClear(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
    }
    ladder::clear_captured();
}

/// VRSwimmingFast(fast) -> Void: the swim's fast-stroke state as the CET mod's observers see it (1 entering a fast
/// surface/dive state, 0 a slow one or leaving a fast one, -1 the player attached); the hooks stay the Lua host's.
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRSwimmingFast(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut fast = -1i32;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut fast as *mut i32);
            frame.finish_params();
        }
    }
    swimming::FAST.store(fast, std::sync::atomic::Ordering::Relaxed);
}

/// VRPhysicalCrouchEnter(owner) -> Void: a crouch state's OnEnter for `owner` (the CET mod's Observe on CrouchEvents
/// reports it; the hook stays the Lua host's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRPhysicalCrouchEnter(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut owner = red4ext_abi::handle::Handle::null();
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut owner as *mut red4ext_abi::handle::Handle);
            frame.finish_params();
        }
    }
    physical_crouch::crouch_enter(&owner);
}

/// VRWorldMapController(alive) -> Void: the world map's controller initialised or uninitialised (the CET mod's two
/// ObserveAfter callbacks report it; the hook stays the Lua host's).
///
/// # Safety
/// The engine's native call.
#[no_mangle]
pub unsafe extern "C" fn VRWorldMapController(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    let mut alive = false;
    // SAFETY: the engine's frame.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut alive as *mut bool);
            frame.finish_params();
        }
    }
    world_map::CONTROLLER.store(alive, std::sync::atomic::Ordering::Relaxed);
}

/// mods/cet/CyberpunkVRPort_PhysicalCrouch (the crouch bridge): every 50 ms, the player's physical crouch effect
/// (VRPort.PhysicalCrouch) applied while the headset asks for a crouch (GetVRPhysicalCrouch) and nothing forces the
/// stance (the ForceStand/KeepState tags), with the camera anchor handed to the graph first, and removed when it is no
/// longer wanted; the provider gets the applied/actual feedback (-1: the bridge is not ready). A new player releases the
/// previous one's effect. A failure releases the effect and warns once; the Running state's exit releases it.
mod physical_crouch {
    use super::Held;
    use crate::script_call::{call_handle, method, static_function, invoke, Value};
    use cvr_host::log;
    use red4ext_abi::cname::CName;
    use red4ext_abi::handle::Handle;
    use std::sync::Mutex;

    struct State {
        owner: Option<Held>,
        owner_id: Option<u64>,
        graph_ready: bool,
        elapsed: f32,
        warned: bool,
    }
    static STATE: Mutex<State> = Mutex::new(State { owner: None, owner_id: None, graph_ready: false, elapsed: 0.0, warned: false });

    fn effect() -> u64 {
        red4ext_abi::tweakdbid::from_name("VRPort.PhysicalCrouch")
    }
    fn feedback(applied: i32, actual: i32) {
        cvr_rig::vrik::provider::set_physical_crouch_feedback(applied, actual);
    }
    fn effects() -> Result<*mut core::ffi::c_void, String> {
        let system_held = super::game_system("GetStatusEffectSystem");
        let system = system_held.instance;
        if system.is_null() { Err("no StatusEffectSystem".into()) } else { Ok(system) }
    }
    /// The player's EntityID.
    fn entity_id(player: *mut core::ffi::c_void) -> Result<u64, String> {
        // SAFETY: the caller's live player.
        unsafe { method::<u64>(player, "GetEntityID", &[]) }.ok_or_else(|| "GetEntityID failed".into())
    }
    fn has_effect(system: *mut core::ffi::c_void, id: u64, effect: u64) -> Result<bool, String> {
        // SAFETY: the status effect system; the ids by pointer, as the structs they are.
        unsafe { method::<bool>(system, "HasStatusEffect", &[Value::Ptr((&id as *const u64).cast()), Value::Ptr((&effect as *const u64).cast())]) }
            .ok_or_else(|| "HasStatusEffect failed".into())
    }
    fn release(player: *mut core::ffi::c_void) -> Result<(), String> {
        if player.is_null() {
            return Ok(());
        }
        let system_held = super::game_system("GetStatusEffectSystem");
        let system = system_held.instance;
        if system.is_null() {
            return Ok(());
        }
        let id = entity_id(player)?;
        let effect = effect();
        if has_effect(system, id, effect)? {
            // SAFETY: as above.
            unsafe { method::<bool>(system, "RemoveStatusEffect", &[Value::Ptr((&id as *const u64).cast()), Value::Ptr((&effect as *const u64).cast())]) }
                .or_else(|| unsafe { method::<()>(system, "RemoveStatusEffect", &[Value::Ptr((&id as *const u64).cast()), Value::Ptr((&effect as *const u64).cast())]) }.map(|_| true))
                .ok_or("RemoveStatusEffect failed")?;
        }
        Ok(())
    }
    /// A static helper called for its effect, whatever it returns.
    fn static_call(class: &str, name: &str, values: &[Value]) -> Result<(), String> {
        let f = static_function(class, name).ok_or_else(|| format!("no {class}.{name}"))?;
        // SAFETY: a static function (no context); the values as its parameters take them.
        unsafe { invoke::<()>(core::ptr::null_mut(), f, values) }.ok_or_else(|| format!("{class}.{name} failed"))
    }
    fn has_tag(player: &Handle, tag: &core::ffi::CStr) -> Result<bool, String> {
        let f = static_function("StatusEffectSystem", "ObjectHasStatusEffectWithTag").ok_or("no ObjectHasStatusEffectWithTag")?;
        // SAFETY: as in static_call.
        unsafe { invoke::<bool>(core::ptr::null_mut(), f, &[Value::Handle(player), Value::U64(CName::add(tag).0)]) }
            .ok_or_else(|| "ObjectHasStatusEffectWithTag failed".into())
    }

    fn update(s: &mut State) -> Result<(), String> {
        let mut player = Handle::null();
        // SAFETY: GetPlayer(GameInstance) into a handle of ours.
        unsafe { crate::anim_lookup::get_player(&mut player) };
        let id = if player.is_null() { None } else { Some(entity_id(player.instance)?) };
        if s.owner.is_some() && s.owner_id != id {
            if let Some(owner) = s.owner.take() {
                let _ = release(owner.0.instance);
            }
        }
        if s.owner_id != id {
            s.graph_ready = false;
        }
        s.owner_id = id;
        let Some(id) = id else {
            s.owner = None;
            feedback(-1, 0);
            return Ok(());
        };
        s.owner = Some(Held(player));
        // The player as the owner holds it, for the calls below.
        let player: &Handle = match s.owner.as_ref() {
            Some(owner) => &owner.0,
            None => return Ok(()),
        };
        let player: *const Handle = player;
        // SAFETY: the owner's handle, alive in `s` for this update.
        let player = unsafe { &*player };
        let db = red4ext_abi::tweakdb::get();
        // SAFETY: the TweakDB's record map under its lock.
        if db.is_null() || !unsafe { red4ext_abi::tweakdb::record_exists(db, effect()) } {
            let _ = release(player.instance);
            feedback(-1, 0);
            return Ok(());
        }
        if !s.graph_ready {
            s.graph_ready = crate::anim_inputs::set_root_graph_float(CName::add(c"vr_physical_crouch_camera"), 0.0) > 0;
        }
        if !s.graph_ready {
            let _ = release(player.instance);
            feedback(-1, 0);
            return Ok(());
        }
        let Ok(system) = effects() else {
            feedback(-1, 0);
            return Ok(());
        };
        // SAFETY: the player, held for the calls.
        let ps = unsafe { call_handle(player.instance, "GetPuppetPS", &[]) }.filter(|h| !h.is_null()).ok_or("GetPuppetPS failed")?;
        // SAFETY: the puppet's persistent state.
        let actual = unsafe { method::<bool>(ps.instance, "IsCrouch", &[]) }.ok_or("IsCrouch failed")?;
        let effect = effect();
        let applied = has_effect(system, id, effect)?;
        let blocked = has_tag(player, c"ForceStand")? || has_tag(player, c"KeepState")?;
        feedback(if blocked { -1 } else { applied as i32 }, actual as i32);
        static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!("[mechanics] physical crouch: bridge ready from Rust (applied={applied} actual={actual} blocked={blocked})\n");
        }
        // SAFETY: the headset's crouch request.
        let wanted = !blocked && unsafe { cvr_rig::vrik::xr_calls::physical_crouch_requested() };
        if wanted && !applied {
            let mut anchor = [0.0f32; 4];
            // SAFETY: the native with no frame (it reads no parameters) into a Vector4 of ours.
            unsafe { cvr_rig::vrik::provider::GetVRPhysicalCrouchCameraAnchor(core::ptr::null_mut(), core::ptr::null_mut(), &mut anchor, 0) };
            if anchor[3] > 0.5 {
                static_call("AnimationControllerComponent", "SetInputVector",
                    &[Value::Handle(player), Value::U64(CName::add(c"vr_physical_crouch_camera_anchor").0), Value::Ptr(anchor.as_ptr().cast())])?;
                static_call("StatusEffectHelper", "ApplyStatusEffect", &[Value::Handle(player), Value::Ptr((&effect as *const u64).cast())])?;
            }
        } else if applied && !wanted {
            release(player.instance)?;
        }
        Ok(())
    }

    pub(super) fn tick(dt: f32) {
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        s.elapsed += dt;
        if s.elapsed < 0.05 {
            return;
        }
        s.elapsed = 0.0;
        if let Err(error) = update(&mut s) {
            if s.owner.as_ref().is_none_or(|owner| release(owner.0.instance).is_ok()) {
                feedback(-1, 0);
            }
            if !s.warned {
                s.warned = true;
                log!("[PhysicalCrouch] {error}\n");
            }
        }
    }
    pub(super) fn shutdown() {
        let s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        if s.owner.as_ref().is_none_or(|owner| release(owner.0.instance).is_ok()) {
            feedback(-1, 0);
        }
    }

    /// The crouch state's OnEnter for `owner`: when it is the player, the graph's camera input follows whether the
    /// physical effect is on.
    pub(super) fn crouch_enter(owner: &Handle) {
        let result = (|| -> Result<(), String> {
            let mut player = Handle::null();
            // SAFETY: GetPlayer(GameInstance) into a handle of ours.
            unsafe { crate::anim_lookup::get_player(&mut player) };
            if player.is_null() || owner.is_null() || entity_id(owner.instance)? != entity_id(player.instance)? {
                return Ok(());
            }
            let physical = has_effect(effects()?, entity_id(player.instance)?, effect())?;
            static_call("AnimationControllerComponent", "SetInputFloat",
                &[Value::Handle(&player), Value::U64(CName::add(c"vr_physical_crouch_camera").0), Value::Float(if physical { 1.0 } else { 0.0 })])
        })();
        if let Err(error) = result {
            log!("[PhysicalCrouch] crouch entry: {error}\n");
        }
    }
}

/// mods/cet/CyberpunkVRPort_Swimming (the state bridge): every frame the player's swimming state machine (its
/// blackboard's PlayerStateMachine.Swimming, the high level 6 or the body/head under water as the water context), the
/// pause and the fast-stroke state into the swim strokes (SetVRSwimmingState); a surface or idle state (0, 3) ends a
/// fast stroke. A failing bridge is reported once until it works again; the Running state's exit publishes "no player".
mod swimming {
    use crate::script_call::{call_global_handle, call_handle, method, property_of_kind, Value};
    use cvr_host::log;
    use red4ext_abi::handle::Handle;
    use red4ext_abi::rtti::RttiKind;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    pub(super) static FAST: AtomicI32 = AtomicI32::new(-1);
    static REPORTED_ERROR: AtomicBool = AtomicBool::new(false);
    static REPORTED: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        fn cvr_swimming_publish_state(player: usize, state: i32, fast: i32, paused: bool, water_context: bool);
    }

    /// (state, water context) from the player's state machine blackboard and its water tests.
    fn player_state(player: &Handle) -> Result<(i32, bool), String> {
        let mut state = -1;
        let mut water = false;
        // SAFETY: the player, held by the caller; each object held by its handle for the calls after it.
        unsafe {
            let board = call_handle(player.instance, "GetPlayerStateMachineBlackboard", &[]).ok_or("GetPlayerStateMachineBlackboard failed")?;
            if !board.is_null() {
                let defs = call_global_handle(core::ptr::null_mut(), "GetAllBlackboardDefs", &[]).filter(|h| !h.is_null()).ok_or("GetAllBlackboardDefs failed")?;
                let psm = &*(property_of_kind(defs.instance, "PlayerStateMachine", RttiKind::Handle as u8).ok_or("no PlayerStateMachine")? as *const Handle);
                if psm.is_null() {
                    return Err("no PlayerStateMachine".into());
                }
                let swimming = property_of_kind(psm.instance, "Swimming", RttiKind::Class as u8).ok_or("no Swimming")?;
                let high_level = property_of_kind(psm.instance, "HighLevel", RttiKind::Class as u8).ok_or("no HighLevel")?;
                state = method::<i32>(board.instance, "GetInt", &[Value::Ptr(swimming)]).ok_or("GetInt failed")?;
                water = method::<i32>(board.instance, "GetInt", &[Value::Ptr(high_level)]).ok_or("GetInt failed")? == 6;
            }
            water = water
                || method::<bool>(player.instance, "IsUnderwater", &[]).ok_or("IsUnderwater failed")?
                || method::<bool>(player.instance, "HasHeadUnderwater", &[]).ok_or("HasHeadUnderwater failed")?;
        }
        Ok((state, water))
    }

    fn bridge() -> Result<(), String> {
        let mut player = Handle::null();
        // SAFETY: GetPlayer(GameInstance) into a handle of ours.
        unsafe { crate::anim_lookup::get_player(&mut player) };
        let paused = super::game_paused()?;
        let (state, water) = if player.is_null() { (-1, false) } else { player_state(&player)? };
        if state == 0 || state == 3 {
            FAST.store(-1, Ordering::Relaxed);
        }
        let fast = FAST.load(Ordering::Relaxed);
        // SAFETY: the swim strokes' publication on the main thread.
        unsafe { cvr_swimming_publish_state(player.instance as usize, state, fast, paused, water) };
        if !player.is_null() && !REPORTED.swap(true, Ordering::Relaxed) {
            log!("[mechanics] swimming: state bridge from Rust (state={state} fast={fast} paused={paused} water={water})\n");
        }
        Ok(())
    }

    pub(super) fn tick() {
        match bridge() {
            Err(error) => {
                if !REPORTED_ERROR.swap(true, Ordering::Relaxed) {
                    log!("[swimming] state bridge unavailable: {error}\n");
                }
            }
            Ok(()) => REPORTED_ERROR.store(false, Ordering::Relaxed),
        }
    }
    pub(super) fn shutdown() {
        // SAFETY: as in bridge.
        unsafe { cvr_swimming_publish_state(0, -1, -1, true, false) };
    }
}

/// mods/cet/CyberpunkVRPort_Ladder (the state bridge): every frame the player's detailed locomotion state (off a
/// ladder, 10..12, the capture is dropped), the pause and the captured description into the ladder (SetVRLadderState),
/// then the top: while the description stays the same, the ladder looked at (TargetingSystem) whose position is the
/// description's is measured every 0.25 s -- its stacked modules' highest top (the mesh hashes' heights) and its finisher
/// rail's matrix -- and published (SetVRLadderMeasuredTop, SetVRLadderTopRails; a refusal measures again). Failures
/// are reported once until they pass; the Running state's exit publishes "no ladder".
mod ladder {
    use super::Held;
    use crate::script_call::{call_global_handle, call_handle, method, property_of_kind, Value};
    use cvr_host::log;
    use cvr_rig::ladder_climb::Vec as V3;
    use red4ext_abi::cname::CName;
    use red4ext_abi::handle::Handle;
    use red4ext_abi::rtti::RttiKind;
    use std::sync::Mutex;

    /// The stacked modules' mesh hashes and heights (a_h160 .. c_h300).
    const MODULES: [(u64, f64); 6] = [
        (13525070408616376406, 1.6),
        (12058514104176874699, 2.0),
        (6760872976104212387, 2.8),
        (14213707566087999640, 3.6),
        (5486285113709760136, 4.0),
        (13340627325582521188, 3.0),
    ];
    /// IComponent::name.
    const COMPONENT_NAME: usize = 0x40;

    struct State {
        owner: Option<Held>,
        description: Option<Held>,
        top_key: Option<String>,
        top_matrix: Option<[[f32; 4]; 4]>,
        top_found: bool,
        module_top: Option<f64>,
        top_poll: f64,
        reported_error: bool,
        top_reported_error: bool,
    }
    static STATE: Mutex<State> = Mutex::new(State {
        owner: None,
        description: None,
        top_key: None,
        top_matrix: None,
        top_found: false,
        module_top: None,
        top_poll: 0.0,
        reported_error: false,
        top_reported_error: false,
    });
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// The mod's clear(): no capture, no top.
    fn clear(s: &mut State) {
        s.owner = None;
        s.description = None;
        s.top_key = None;
        s.top_matrix = None;
        s.top_found = false;
        s.module_top = None;
        s.top_poll = 0.0;
    }
    pub(super) fn capture(owner: Handle, description: Handle) {
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        s.owner = (!owner.is_null()).then(|| Held(owner));
        s.description = (!description.is_null()).then(|| Held(description));
    }
    pub(super) fn clear_captured() {
        clear(&mut STATE.lock().unwrap_or_else(|p| p.into_inner()));
    }

    fn v3(v: [f32; 4]) -> V3 {
        V3 { x: v[0], y: v[1], z: v[2] }
    }

    /// The mod's measureModules: the highest top among the ladder's stacked modules standing on its axis.
    fn measure_modules(ladder: usize, p: [f32; 4], up: [f32; 4]) -> Option<f64> {
        let mut best: Option<f64> = None;
        // SAFETY: the ladder's components, read on the main thread; each live for the calls.
        for &[component, _] in unsafe { crate::anim_lookup::components(ladder) } {
            if component == 0 {
                continue;
            }
            // SAFETY: the component's mesh reference (a ResourceAsyncReference: its path hash first), when it has one.
            let Some(mesh) = (unsafe { property_of_kind(component as *mut core::ffi::c_void, "mesh", RttiKind::ResourceAsyncReference as u8) }) else {
                continue;
            };
            // SAFETY: as above.
            let hash = unsafe { *(mesh as *const u64) };
            let Some(&(_, height)) = MODULES.iter().find(|(h, _)| *h == hash) else { continue };
            // SAFETY: the component's world matrix.
            let Some(m) = (unsafe { method::<[[f32; 4]; 4]>(component as *mut core::ffi::c_void, "GetLocalToWorld", &[]) }) else {
                continue;
            };
            let (w, z) = (m[3], m[2]);
            let d = [w[0] as f64 - p[0] as f64, w[1] as f64 - p[1] as f64, w[2] as f64 - p[2] as f64];
            let u = [up[0] as f64, up[1] as f64, up[2] as f64];
            let along = d[0] * u[0] + d[1] * u[1] + d[2] * u[2];
            let (sx, sy, sz) = (d[0] - u[0] * along, d[1] - u[1] * along, d[2] - u[2] * along);
            let stands = z[0] as f64 * u[0] + z[1] as f64 * u[1] + z[2] as f64 * u[2];
            if sx * sx + sy * sy + sz * sz < 0.09 && stands > 0.98 {
                let top = along + height * stands;
                if best.is_none_or(|b| top > b) {
                    best = Some(top);
                }
            }
        }
        best
    }

    /// The mod's publishTop.
    fn publish_top(s: &mut State, player: &Handle, dt: f64) -> Result<(), String> {
        if player.is_null() {
            return Ok(());
        }
        let Some(description) = s.description.as_ref() else { return Ok(()) };
        // SAFETY: the captured description, held.
        let (p, up, height) = unsafe { cvr_rig::vrik::ladder_input::description_frame(&description.0) }.ok_or("the description is no LadderDescription")?;
        let key = format!("{:.3}/{:.3}/{:.3}/{:.3}", p[0] as f64, p[1] as f64, p[2] as f64, height as f64);
        if s.top_key.as_deref() != Some(key.as_str()) {
            s.top_key = Some(key);
            s.top_matrix = None;
            s.top_found = false;
            s.module_top = None;
            s.top_poll = 0.0;
        }
        s.top_poll -= dt;
        if !s.top_found && s.top_poll <= 0.0 {
            s.top_poll = 0.25;
            let targeting_held = super::game_system("GetTargetingSystem");
            let targeting = targeting_held.instance;
            if targeting.is_null() {
                return Err("no TargetingSystem".into());
            }
            // SAFETY: the targeting system; the ladder held by its handle for the calls after it.
            let ladder = unsafe { call_handle(targeting, "GetLookAtObject", &[Value::Handle(player), Value::Bool(false), Value::Bool(false)]) }
                .ok_or("GetLookAtObject failed")?;
            if !ladder.is_null() {
                // SAFETY: as above.
                let class = unsafe { method::<u64>(ladder.instance, "GetClassName", &[]) }.ok_or("GetClassName failed")?;
                if CName(class).text() == "Ladder" {
                    // SAFETY: as above.
                    let at = unsafe { method::<[f32; 4]>(ladder.instance, "GetWorldPosition", &[]) }.ok_or("GetWorldPosition failed")?;
                    let distance = (0..3).map(|i| (at[i] as f64 - p[i] as f64).powi(2)).sum::<f64>();
                    if distance < 0.0001 {
                        s.top_found = true;
                        s.module_top = measure_modules(ladder.instance as usize, p, up);
                        // SAFETY: as in measure_modules.
                        for &[component, _] in unsafe { crate::anim_lookup::components(ladder.instance as usize) } {
                            if component == 0 {
                                continue;
                            }
                            // SAFETY: IComponent::name of a live component.
                            let name = unsafe { *((component + COMPONENT_NAME) as *const CName) };
                            if name.text().contains("generic_ladder_f_finisher_rail") {
                                // SAFETY: the component's world matrix.
                                s.top_matrix = unsafe { method::<[[f32; 4]; 4]>(component as *mut core::ffi::c_void, "GetLocalToWorld", &[]) };
                                if s.top_matrix.is_none() {
                                    return Err("GetLocalToWorld failed".into());
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
        let player_id = player.instance as usize;
        if let Some(top) = s.module_top {
            // SAFETY: the ladder's publication.
            if !unsafe { cvr_rig::vrik::ladder_input::cvr_ladder_publish_measured_top(player_id, &v3(p), top as f32) } {
                s.module_top = None;
                s.top_found = false;
            }
        }
        if let Some(m) = s.top_matrix {
            // SAFETY: as above; the rows W (origin), X (right), Y (normal), Z (up).
            if !unsafe { cvr_rig::vrik::ladder_input::cvr_ladder_publish_top(player_id, &v3(p), &v3(m[3]), &v3(m[0]), &v3(m[1]), &v3(m[2])) } {
                s.top_matrix = None;
                s.top_found = false;
            }
        }
        Ok(())
    }

    fn bridge(s: &mut State, dt: f64) -> Result<(), String> {
        let mut player = Handle::null();
        // SAFETY: GetPlayer(GameInstance) into a handle of ours.
        unsafe { crate::anim_lookup::get_player(&mut player) };
        let mut state = -1;
        if !player.is_null() {
            // SAFETY: the player; each object held by its handle for the calls after it.
            unsafe {
                let board = call_handle(player.instance, "GetPlayerStateMachineBlackboard", &[]).ok_or("GetPlayerStateMachineBlackboard failed")?;
                if !board.is_null() {
                    let defs = call_global_handle(core::ptr::null_mut(), "GetAllBlackboardDefs", &[]).filter(|h| !h.is_null()).ok_or("GetAllBlackboardDefs failed")?;
                    let psm = &*(property_of_kind(defs.instance, "PlayerStateMachine", RttiKind::Handle as u8).ok_or("no PlayerStateMachine")? as *const Handle);
                    if psm.is_null() {
                        return Err("no PlayerStateMachine".into());
                    }
                    let detailed = property_of_kind(psm.instance, "LocomotionDetailed", RttiKind::Class as u8).ok_or("no LocomotionDetailed")?;
                    state = method::<i32>(board.instance, "GetInt", &[Value::Ptr(detailed)]).ok_or("GetInt failed")?;
                }
            }
        }
        if !(10..=12).contains(&state) {
            clear(s);
        }
        let paused = super::game_paused()?;
        let who = s.owner.as_ref().map_or(player.instance as usize, |o| o.0.instance as usize);
        let null = Handle::null();
        let description = s.description.as_ref().map_or(&null, |d| &d.0);
        // SAFETY: the ladder's publication with the captured description.
        unsafe { cvr_rig::vrik::ladder_input::publish_state(who, state, paused, description) };
        if !player.is_null() && !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!("[mechanics] ladder: state bridge from Rust (state={state} paused={paused})\n");
        }
        match publish_top(s, &player, dt) {
            Err(error) => {
                if !s.top_reported_error {
                    log!("[ladder] top rail geometry unavailable: {error}\n");
                }
                s.top_reported_error = true;
            }
            Ok(()) => s.top_reported_error = false,
        }
        Ok(())
    }

    pub(super) fn tick(dt: f32) {
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        match bridge(&mut s, dt as f64) {
            Err(error) => {
                if !s.reported_error {
                    s.reported_error = true;
                    log!("[ladder] state bridge unavailable: {error}\n");
                }
            }
            Ok(()) => s.reported_error = false,
        }
    }
    pub(super) fn shutdown() {
        let null = Handle::null();
        // SAFETY: as in bridge.
        unsafe { cvr_rig::vrik::ladder_input::publish_state(0, -1, true, &null) };
    }
}

/// mods/cet/CyberpunkVRPort_Crosshair: on every crosshair state change or update, the native crosshair hidden while the
/// VR aim is in charge -- shown only for a Smart weapon, whose HUD parts (frame, dots, background, fluff, watermark, the
/// smart FOTS and the no-link reticle) are scaled to 0.4 instead.
mod crosshair {
    use crate::script_call::{call, call_bool_with, call_handle, enum_value, method, Value};
    use cvr_host::log;
    use red4ext_abi::cname::CName;
    use red4ext_abi::handle::{Handle, WeakHandle};
    use std::sync::Mutex;

    const SMART_HUD_SCALE: f32 = 0.4;
    const SMART_HUD_PARTS: [&[&str]; 7] = [
        &["smartGun", "smartGunFrame"],
        &["smartGun", "dotContainer"],
        &["smartGun", "smartGunBG"],
        &["smartGun", "smartGunFluff"],
        &["smartGun", "wmrk"],
        &["smartFots"],
        &["reticle_no_smart_link"],
    ];

    /// The container, weakly (IsDefined fails once it is gone), and the Smart weapon cache.
    struct State {
        container: Option<Weak>,
        smart_key: Option<u64>,
        smart_value: bool,
        reported: bool,
    }
    struct Weak(WeakHandle);
    // SAFETY: made, used and dropped on the main thread (the observers and the script VM's natives).
    unsafe impl Send for Weak {}
    static STATE: Mutex<State> = Mutex::new(State { container: None, smart_key: None, smart_value: false, reported: false });

    pub(super) fn capture(container: &Handle) {
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        s.container = (!container.is_null()).then(|| Weak(WeakHandle::from_handle(container)));
        log!("[VR] Crosshair container captured\n");
    }

    /// The mod's smartSniperActive: whether the active weapon's evolution is Smart (cached per weapon).
    fn smart_sniper_active(s: &mut State) -> Result<bool, String> {
        let mut player = Handle::null();
        // SAFETY: GetPlayer(GameInstance) into a handle of ours.
        unsafe { crate::anim_lookup::get_player(&mut player) };
        let weapon = if player.is_null() {
            None
        } else {
            // ScriptedPuppet.GetActiveWeapon(puppet) is a static of the puppet class, not a method of the player (asked as
            // one it found nothing: the Smart weapons' crosshair was never shown)
            crate::script_call::static_function("ScriptedPuppet", "GetActiveWeapon")
                // SAFETY: the static with the player, held.
                .and_then(|f| unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(&player)]) })
                .filter(|w| !w.is_null())
        };
        let Some(weapon) = weapon else {
            s.smart_key = None;
            return Ok(false);
        };
        // SAFETY: the weapon, held.
        let key = unsafe { method::<u64>(weapon.instance, "GetEntityID", &[]) }.ok_or("GetEntityID failed")?;
        if s.smart_key == Some(key) {
            return Ok(s.smart_value);
        }
        s.smart_key = Some(key);
        s.smart_value = false;
        // SAFETY: as above; the record and its evolution held for the calls after them.
        let record = unsafe { call_handle(weapon.instance, "GetWeaponRecord", &[]) }.ok_or("GetWeaponRecord failed")?;
        if record.is_null() {
            return Ok(false);
        }
        // SAFETY: as above.
        let evolution = unsafe { call_handle(record.instance, "Evolution", &[]) }.ok_or("Evolution failed")?;
        if !evolution.is_null() {
            // SAFETY: as above; the enum read in an 8-byte slot (its own width zero-extended).
            let kind = unsafe { method::<u64>(evolution.instance, "Type", &[]) }.ok_or("Type failed")?;
            let smart = enum_value("gamedataWeaponEvolution", "Smart").ok_or("no gamedataWeaponEvolution.Smart")?;
            s.smart_value = kind as i64 == smart;
        }
        Ok(s.smart_value)
    }

    /// The mod's child(): `widget`'s child called `name`, when the widget is a compound one.
    fn child(widget: &Handle, name: &str) -> Option<Handle> {
        // SAFETY: the widget, held by the caller; each child held by its handle.
        unsafe {
            if !call_bool_with(widget.instance, "IsA", &[Value::U64(CName::add(c"inkCompoundWidget").0)])? {
                return None;
            }
            let count = method::<i32>(widget.instance, "GetNumChildren", &[])?;
            for i in 0..count.max(0) {
                let Some(c) = call_handle(widget.instance, "GetWidgetByIndex", &[Value::Int32(i)]).filter(|c| !c.is_null()) else { continue };
                let n = method::<u64>(c.instance, "GetName", &[])?;
                if CName(n).text() == name {
                    return Some(c);
                }
            }
        }
        None
    }

    /// The mod's scaleSmartHud: the Smart HUD's parts scaled; the number scaled.
    fn scale_smart_hud(container: &Handle, scale: f32) -> Option<usize> {
        // SAFETY: the container, held by the caller.
        let root = unsafe { call_handle(container.instance, "GetActiveCrosshairWidget", &[]) }.filter(|r| !r.is_null())?;
        let mut n = 0;
        for path in SMART_HUD_PARTS {
            let mut w = Some(root.clone());
            for name in path {
                w = w.and_then(|w| child(&w, name));
            }
            if let Some(w) = w {
                let v = [scale, scale];
                // SAFETY: the part, held; a Vector2 in place.
                unsafe { call(w.instance, "SetScale", &[Value::Ptr(v.as_ptr().cast())]) };
                n += 1;
            }
        }
        Some(n)
    }

    pub(super) fn update() {
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let Some(weak) = s.container.as_ref() else { return };
        let container = weak.0.lock();
        if container.is_null() {
            s.container = None;
            return;
        }
        // The mod's hideCrosshair (GetVRWeaponAim always answers 0, which Lua takes for true): hidden unless Smart.
        let was = s.smart_key;
        let smart = match smart_sniper_active(&mut s) {
            Ok(v) => v,
            Err(e) => {
                if s.smart_key != was {
                    log!("[mechanics] crosshair: the weapon's evolution unread: {e}
");
                }
                false
            }
        };
        let hidden = !smart;
        // SAFETY: the container, held; its widgets held for their calls.
        let shown = unsafe {
            let root = call_handle(container.instance, "GetRootWidget", &[]).filter(|w| !w.is_null());
            let active = call_handle(container.instance, "GetActiveCrosshairWidget", &[]).filter(|w| !w.is_null());
            // As the mod: the root first; a missing widget stops the update there (the Lua call errored).
            match root {
                Some(root) => {
                    call(root.instance, "SetVisible", &[Value::Bool(!hidden)]);
                    match active {
                        Some(active) => {
                            call(active.instance, "SetVisible", &[Value::Bool(!hidden)]);
                            true
                        }
                        None => false,
                    }
                }
                None => false,
            }
        };
        if !shown {
            return;
        }
        let smart = smart_sniper_active(&mut s).unwrap_or(false);
        let scaled = scale_smart_hud(&container, if smart { SMART_HUD_SCALE } else { 1.0 });
        if !s.reported {
            s.reported = true;
            log!("[mechanics] crosshair: update from Rust (hidden={hidden} smart={smart} scaled={scaled:?})\n");
        }
    }
}

/// mods/cet/CyberpunkVRPort_Driving (main's since 2026-11-17): the car seat's script side -- the player's gamepad range
/// for the hands on the wheel (every 0.5 s of a held wheel the game's stick deadzones, /controls Axis_DeadzoneInnerFix
/// and Axis_DeadzoneOuter, go to the wheel's gamepad profile) and THE CABIN WHEEL FOR THE HANDS ON IT: while a hand
/// holds the rim of a car's wheel, its steering_wheel slot in the player's model space (world = player position + player
/// turn * model) each frame, so the hand rides the wheel the car draws (a vehicle without the slot, a bike, leaves the
/// plugin on its own estimate). The road wheels' response is the plugin's (vrik/steer_curve.rs): no drive model in
/// TweakDB is changed here any more -- an edited flat reached every car spawned meanwhile, the traffic's included, and
/// never the car already driven. A failure reports once until it passes.
mod driving {
    use super::belt::{cn, h, m, model_basis, qmul, qrot, WorldTransform};
    use crate::script_call::{call_handle, game_instance, invoke_handle, method, native_class_function, Value};
    use cvr_host::log;
    use red4ext_abi::cname::CName;
    use red4ext_abi::handle::Handle;
    use std::sync::Mutex;

    unsafe extern "C" {
        fn cvr_anim_wheel_control_state() -> i32;
        fn cvr_anim_wheel_set_gamepad_profile(inner: f32, outer: f32) -> bool;
    }

    struct State {
        profile_time: f32,
        reported_error: bool,
        probed: bool,
        /// The modal list popup up (one at most: the radio's station list), forgotten when it uninitializes.
        modal_list: Option<Handle>,
        /// The plugin's count of B presses made on a popup in the car (shared slot 175), as last seen.
        popup_backs: Option<f32>,
        chord_error: bool,
        horn_on: bool,
        horn_said: bool,
    }
    // SAFETY: the game thread's alone (the mechanics' frame and the popup's script hooks).
    unsafe impl Send for State {}
    static STATE: Mutex<Option<State>> = Mutex::new(None);
    fn state(guard: &mut Option<State>) -> &mut State {
        guard.get_or_insert_with(|| State {
            profile_time: 0.5,
            reported_error: false,
            probed: false,
            modal_list: None,
            popup_backs: None,
            chord_error: false,
            horn_on: false,
            horn_said: false,
        })
    }

    /// The modal list popup's OnInitialize and OnUninitialize (game_hooks.rs).
    pub(in crate::mechanics) fn modal_list(list: Option<Handle>) {
        let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
        state(&mut guard).modal_list = list;
    }

    /// B CLOSES THE RADIO LIST IN THE CAR. The list (D-pad left held) closes on `cancel`, the pad's B, which in a car is
    /// held out of the game as the exit button, and Start closes it only to open the pause menu on the release
    /// (measured). The XInput merge counts each B press made with a popup up (shared slot 175); on a change the open
    /// list is closed the way its own cancel does.
    fn popup_back(s: &mut State) {
        unsafe extern "C" {
            fn cvr_cpp_xr_get_shared_slot(slot: i32) -> f32;
        }
        // SAFETY: the shared block's read.
        let n = unsafe { cvr_cpp_xr_get_shared_slot(175) };
        if s.popup_backs.is_none_or(|seen| seen == n) {
            s.popup_backs = Some(n);
            return;
        }
        s.popup_backs = Some(n);
        if let Some(list) = s.modal_list.take() {
            if !list.is_null() && m::<()>(&list, "Close", &[]).is_err() {
                log!("[driving] popup close: Close failed\n");
            }
        }
    }

    /// THE AUTOPILOT ON THE CYBERWARE CHORD. In the driver seat the plugin hands the chord (the left grip + the left
    /// stick's click, held) here instead of pressing the cyberware key. The game's own autodrive is L3 held for half a
    /// second, which the port never produces; this queues what the autodrive widget queues, so the system's own checks --
    /// the player's own car, a valid lane, no race -- still decide, and it says so on the HUD when it refuses.
    fn autopilot_chord(s: &mut State) {
        if !cvr_rig::vrik::cyberware_chord::cvr_autopilot_chord_take() {
            return;
        }
        let ads = h(&super::game_system("GetScriptableSystemsContainer"), "Get", &[cn("AutoDriveSystem")]);
        if ads.is_null() {
            return;
        }
        let enabled = m::<bool>(&ads, "GetAutodriveEnabled", &[]);
        let request = if enabled == Ok(true) { "DisableAutoDriveRequest" } else { "EnableAutoDriveRequest" };
        let Some(class) = crate::script_call::class_any(request) else {
            if !s.chord_error {
                log!("[driving] autopilot chord: no {request}\n");
            }
            s.chord_error = true;
            return;
        };
        // SAFETY: a new instance owned by the handle made for it.
        let r = unsafe { Handle::from_instance(class.create_instance()) };
        match m::<()>(&ads, "QueueRequest", &[Value::Handle(&r)]) {
            Ok(()) => {
                s.chord_error = false;
                log!("[driving] autopilot {}\n", if enabled == Ok(true) { "off" } else { "on" });
            }
            Err(e) => {
                if !s.chord_error {
                    log!("[driving] autopilot chord: {e}\n");
                }
                s.chord_error = true;
            }
        }
    }

    /// Game.GetSettingsSystem():GetVar(group, name):GetValue().
    fn setting(group: &core::ffi::CStr, name: &core::ffi::CStr) -> Result<f32, String> {
        let f = native_class_function("ScriptGameInstance", "GetSettingsSystem").ok_or("no GetSettingsSystem")?;
        let game = game_instance();
        // SAFETY: the game instance's static with the game instance; each object held for the calls after it.
        unsafe {
            let settings = invoke_handle(core::ptr::null_mut(), f, &[Value::Ptr((&game as *const crate::script_call::GameInstance).cast())])
                .filter(|h| !h.is_null())
                .ok_or("GetSettingsSystem failed")?;
            let var = call_handle(settings.instance, "GetVar", &[Value::U64(CName::add(group).0), Value::U64(CName::add(name).0)])
                .filter(|h| !h.is_null())
                .ok_or("GetVar failed")?;
            method::<f32>(var.instance, "GetValue", &[]).ok_or_else(|| "GetValue failed".into())
        }
    }

    /// The player's mounted vehicle: its method, else the global GetMountedVehicle(player).
    fn mounted_vehicle(player: &Handle) -> Option<Handle> {
        // SAFETY: the player, held; the vehicle handle (weak or strong) locked.
        unsafe {
            call_handle(player.instance, "GetMountedVehicle", &[]).or_else(|| {
                crate::script_call::call_global_handle(core::ptr::null_mut(), "GetMountedVehicle;GameObject", &[Value::Handle(player)])
            })
        }
        .filter(|v| !v.is_null())
    }

    /// The mod's sendCabin(player, vehicle): a car's steering_wheel slot into the player's model space, to the wheel.
    fn send_cabin(player: &Handle, vehicle: &Handle) {
        if m::<bool>(vehicle, "IsA", &[cn("vehicleCarBaseObject")]) != Ok(true) {
            return;
        }
        let slots = h(vehicle, "FindComponentByName", &[cn("vehicle_slots")]);
        if slots.is_null() {
            return;
        }
        let mut wt = WorldTransform([0; 8]);
        if m::<bool>(&slots, "GetSlotTransform", &[cn("steering_wheel"), Value::Out((&mut wt as *mut WorldTransform).cast())]) != Ok(true) {
            return;
        }
        let Some((base, inverse)) = model_basis(player) else { return };
        let world = [0, 1, 2].map(|i| f64::from(wt.0[i] as i32) / 131072.0);
        let orientation = [4, 5, 6, 7].map(|i| f64::from(f32::from_bits(wt.0[i])));
        let p = qrot(inverse, [world[0] - base[0], world[1] - base[1], world[2] - base[2]]);
        let q = qmul(inverse, orientation);
        cvr_rig::vrik::wheel_grab::set_cabin_slot(p.map(|v| v as f32), q.map(|v| v as f32));
    }

    fn update(s: &mut State, dt: f32) -> Result<(), String> {
        // SAFETY: the wheel's state.
        let state = unsafe { cvr_anim_wheel_control_state() };
        if state % 4 != 0 {
            let mut player = Handle::null();
            // SAFETY: GetPlayer(GameInstance) into a handle of ours.
            unsafe { crate::anim_lookup::get_player(&mut player) };
            if let Some(vehicle) = (!player.is_null()).then(|| mounted_vehicle(&player)).flatten() {
                send_cabin(&player, &vehicle);
            }
        }
        if state == 0 {
            s.profile_time = 0.5;
            return Ok(());
        }
        s.profile_time += dt.max(0.0);
        if s.profile_time >= 0.5 {
            let inner = setting(c"/controls", c"Axis_DeadzoneInnerFix")?;
            let outer = setting(c"/controls", c"Axis_DeadzoneOuter")?;
            // SAFETY: the wheel's profile.
            if !unsafe { cvr_anim_wheel_set_gamepad_profile(inner, outer) } {
                return Err("invalid gamepad range".into());
            }
            s.profile_time = 0.0;
        }
        Ok(())
    }

    /// THE HORN, a hand laid on the wheel's hub (cvr-rig wheel_grab.rs's mask): the car's own ToggleHorn for as long
    /// as it stays there -- no pad button (Vehicle_Horn is X, which now gets the player out of the car).
    fn horn(s: &mut State) {
        let mask = cvr_rig::vrik::shared_globals::g_wheelHornMask.load(core::sync::atomic::Ordering::Relaxed);
        let want = mask != 0;
        if want == s.horn_on {
            return;
        }
        let pl = super::belt::player();
        static CACHE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let vehicle = if pl.is_null() {
            Handle::null()
        } else {
            crate::script_call::global_by_short_name("GetMountedVehicle", &CACHE)
                // SAFETY: a global script function (no context).
                .and_then(|f| unsafe { invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(&pl)]) })
                .unwrap_or_else(Handle::null)
        };
        if vehicle.is_null() {
            s.horn_on = false;
            return;
        }
        // SAFETY: the mounted vehicle, held.
        let ok = unsafe { method::<()>(vehicle.instance, "ToggleHorn", &[Value::Bool(want), Value::Bool(false)]) }.is_some();
        s.horn_on = want;
        if !s.horn_said {
            s.horn_said = true;
            log!("[driving] the horn by the hub: {} ({})\n", if want { "on" } else { "off" }, if ok { "ToggleHorn" } else { "ToggleHorn failed" });
        }
    }

    pub(super) fn tick(dt: f32) {
        let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let s = state(&mut guard);
        horn(s);
        autopilot_chord(s);
        popup_back(s);
        if !s.probed {
            // Once in gameplay: the settings chain proven (the A/B never holds a wheel).
            s.probed = true;
            let settings = match (setting(c"/controls", c"Axis_DeadzoneInnerFix"), setting(c"/controls", c"Axis_DeadzoneOuter")) {
                (Ok(inner), Ok(outer)) => format!("deadzones {inner:.3}/{outer:.3}"),
                (Err(e), _) | (_, Err(e)) => format!("settings NOT readable: {e}"),
            };
            log!("[mechanics] driving: {settings}\n");
        }
        match update(s, dt) {
            Err(error) => {
                if !s.reported_error {
                    log!("[driving] wheel profile unavailable: {error}\n");
                }
                s.reported_error = true;
            }
            Ok(()) => s.reported_error = false,
        }
    }
    pub(super) fn shutdown() {
        let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = guard.as_mut() {
            s.modal_list = None;
        }
    }
}

/// Game.GetSystemRequestsHandler():IsGamePaused() -- CET's alias of the static inkMenuScenario.GetSystemRequestsHandler,
/// a weak handle locked for the call.
fn game_paused() -> Result<bool, String> {
    let handler = requests_handler()?;
    if handler.is_null() {
        return Err("GetSystemRequestsHandler failed".into());
    }
    // SAFETY: the handler, held for the call.
    unsafe { crate::script_call::method::<bool>(handler.instance, "IsGamePaused", &[]) }.ok_or_else(|| "IsGamePaused failed".into())
}
/// Game.Get<X>System() as CET answers it: the ScriptGameInstance static getter called with the game instance -- the
/// system the scripts get (a class name looked up in the game's system map can miss it: gameTargetingSystem is kept
/// under its interface). Null when the game has none.
pub(crate) fn game_system(getter: &str) -> red4ext_abi::handle::Handle {
    let Some(f) = crate::script_call::native_class_function("ScriptGameInstance", getter) else {
        return red4ext_abi::handle::Handle::null();
    };
    let g = crate::script_call::game_instance();
    // SAFETY: the game instance's static getter.
    unsafe {
        crate::script_call::invoke_handle(
            core::ptr::null_mut(),
            f,
            &[crate::script_call::Value::Ptr((&g as *const crate::script_call::GameInstance).cast())],
        )
    }
    .unwrap_or_else(red4ext_abi::handle::Handle::null)
}
/// Game.FindEntityByID(id): the entity, null when it is not there.
pub(crate) fn find_entity(id: u64) -> Result<red4ext_abi::handle::Handle, String> {
    let f = crate::script_call::native_class_function("ScriptGameInstance", "FindEntityByID").ok_or("no FindEntityByID")?;
    let g = crate::script_call::game_instance();
    // SAFETY: the game instance's static.
    unsafe {
        crate::script_call::invoke_handle(
            core::ptr::null_mut(),
            f,
            &[crate::script_call::Value::Ptr((&g as *const crate::script_call::GameInstance).cast()), crate::script_call::Value::U64(id)],
        )
    }
    .ok_or_else(|| "FindEntityByID failed".into())
}
/// Game.GetSystemRequestsHandler(): the static inkMenuScenario.GetSystemRequestsHandler's handler (a weak handle locked),
/// null when there is none.
pub(crate) fn requests_handler() -> Result<red4ext_abi::handle::Handle, String> {
    use crate::script_call::{invoke_handle, native_class_function};
    let f = native_class_function("inkMenuScenario", "GetSystemRequestsHandler").ok_or("no inkMenuScenario.GetSystemRequestsHandler")?;
    // SAFETY: a static function (the dummy context).
    unsafe { invoke_handle(core::ptr::null_mut(), f, &[]) }.ok_or_else(|| "GetSystemRequestsHandler failed".into())
}

/// mods/cet/CyberpunkVRPort_WorldMap (the head-lock bridge): while the world map's controller lives and the game says
/// a menu is up (UI_System.IsInMenu), the HMD stops driving the camera (SetVRMenuOpen's slot), every frame, so the flag
/// clears in gameplay; cleared at shutdown.
mod world_map {
    use crate::script_call::{call_bool_with, call_global_handle, call_handle, property_of_kind, Value};
    use red4ext_abi::rtti::RttiKind;
    use cvr_host::log;
    use red4ext_abi::handle::Handle;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub(super) static CONTROLLER: AtomicBool = AtomicBool::new(false);
    static REPORTED: AtomicBool = AtomicBool::new(false);

    /// Game.GetBlackboardSystem():Get(defs.UI_System):GetBool(defs.UI_System.IsInMenu); None when any step fails.
    pub(super) fn game_says_in_menu() -> Option<bool> {
        // SAFETY: script calls on the main thread; each object held by its handle for the calls after it.
        unsafe {
            let defs = call_global_handle(core::ptr::null_mut(), "GetAllBlackboardDefs", &[])?;
            if defs.is_null() {
                return None;
            }
            let ui = &*(property_of_kind(defs.instance, "UI_System", RttiKind::Handle as u8)? as *const Handle);
            if ui.is_null() {
                return None;
            }
            let in_menu = property_of_kind(ui.instance, "IsInMenu", RttiKind::Class as u8)?;
            let system_held = super::game_system("GetBlackboardSystem");
            let system = system_held.instance;
            if system.is_null() {
                return None;
            }
            let board = call_handle(system, "Get", &[Value::Handle(ui)])?;
            if board.is_null() {
                return None;
            }
            call_bool_with(board.instance, "GetBool", &[Value::Ptr(in_menu)])
        }
    }

    pub(super) fn tick() {
        let alive = CONTROLLER.load(Ordering::Relaxed);
        // The first tick in gameplay reads the blackboard once whatever the map does, so the chain is proven in any
        // session.
        let probe = !REPORTED.load(Ordering::Relaxed)
            && crate::anim_lookup::player_entity() != 0
            && !REPORTED.swap(true, Ordering::Relaxed);
        let in_menu = if alive || probe { game_says_in_menu() } else { None };
        if probe {
            let read = match in_menu {
                Some(true) => "true (read from Rust)",
                Some(false) => "false (read from Rust)",
                None => "NOT readable",
            };
            log!("[mechanics] world map: UI_System.IsInMenu {read}\n");
        }
        cvr_rig::vrik::pose_rig::set_vr_menu_open(alive && in_menu == Some(true));
    }
    pub(super) fn shutdown() {
        cvr_rig::vrik::pose_rig::set_vr_menu_open(false);
    }
}

/// A handle held across ticks; only the game's main thread touches it.
struct Held(red4ext_abi::handle::Handle);
// SAFETY: created, used and dropped on the main thread, where the mechanics tick.
unsafe impl Send for Held {}

/// mods/cet/CyberpunkVRPort_Basketball (the input bridge): the body's capsules kept out of physics queries (so the
/// ball's sweeps do not hit the player's own body), then -- while the player has a ball -- the grips into the ball's
/// tick (mechanics/ball.rs, formerly the player's VRBallTick of mods/redscript/CyberpunkVRPort_Basketball), a status
/// line every 0.5 s under the debug slot.
mod basketball {
    use super::Held;
    use crate::script_call::{call, call_handle, Value};
    use cvr_host::log;
    use red4ext_abi::cname::CName;
    use red4ext_abi::handle::Handle;
    use std::sync::Mutex;
    use std::time::Instant;

    unsafe extern "C" {
        fn cvr_cpp_xr_get_shared_slot(slot: i32) -> f32;
    }
    fn slot(index: i32) -> f32 {
        // SAFETY: a read of the shared block (0 when it is not there).
        unsafe { cvr_cpp_xr_get_shared_slot(index) }
    }
    /// IComponent::name.
    const COMPONENT_NAME: usize = 0x40;

    struct State {
        caps: Option<Vec<Held>>,
        caps_at: Option<Instant>,
        scan_at: Option<Instant>,
        miss_at: Option<Instant>,
        debug_cached: bool,
        debug_at: Option<Instant>,
        log_acc: f32,
        reported: bool,
    }
    static STATE: Mutex<State> = Mutex::new(State {
        caps: None,
        caps_at: None,
        scan_at: None,
        miss_at: None,
        debug_cached: false,
        debug_at: None,
        log_acc: 0.0,
        reported: false,
    });
    fn since(at: Option<Instant>, now: Instant) -> f32 {
        at.map_or(f32::INFINITY, |at| now.duration_since(at).as_secs_f32())
    }

    /// The mod's capsQueriesOff: the "VRPortBody_" components' physical bodies found (again every 5 s, not before a
    /// second after a miss) and made unqueryable every 0.5 s; a call that fails drops the list (stale after a reload).
    fn caps_queries_off(s: &mut State, now: Instant, player: *mut core::ffi::c_void) {
        if s.caps.is_none() && since(s.miss_at, now) < 1.0 {
            return;
        }
        if s.caps.is_none() || since(s.scan_at, now) > 5.0 {
            // SAFETY: the player, live for the tick.
            if !unsafe { cvr_rig::vrik::performance::should_scan_body_capsules(player) } {
                return;
            }
            let mut found = Vec::new();
            // SAFETY: the player's components, read on the main thread; each a live component for the calls.
            for &[instance, _] in unsafe { crate::anim_lookup::components(player as usize) } {
                if instance == 0 {
                    continue;
                }
                // SAFETY: IComponent::name of a live component.
                let name = unsafe { *((instance + COMPONENT_NAME) as *const CName) };
                if !name.text().contains("VRPortBody_") {
                    continue;
                }
                // SAFETY: as above.
                match unsafe { call_handle(instance as *mut core::ffi::c_void, "CreatePhysicalBodyInterface", &[Value::Int32(0)]) } {
                    Some(body) if !body.is_null() => found.push(Held(body)),
                    Some(_) => {}
                    None => {
                        s.miss_at = Some(now);
                        return;
                    }
                }
            }
            if found.is_empty() {
                s.miss_at = Some(now);
                return;
            }
            s.caps = Some(found);
            s.scan_at = Some(now);
        }
        if since(s.caps_at, now) < 0.5 {
            return;
        }
        s.caps_at = Some(now);
        let stale = s.caps.as_ref().is_some_and(|caps| {
            // SAFETY: the held bodies.
            caps.iter().any(|body| !unsafe { call(body.0.instance, "SetIsQueryable", &[Value::Bool(false)]) })
        });
        if stale {
            s.caps = None;
        }
    }

    pub(super) fn tick(dt: f32) {
        if dt <= 0.0 || dt > 0.2 {
            return;
        }
        let mut player = Handle::null();
        // SAFETY: GetPlayer(GameInstance) into a handle of ours, released when it drops.
        unsafe { crate::anim_lookup::get_player(&mut player) };
        if player.is_null() {
            return;
        }
        let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        caps_queries_off(&mut s, now, player.instance);
        if !s.reported {
            s.reported = true;
            log!("[mechanics] basketball: the ball's physics from Rust (mechanics/ball.rs)\n");
        }
        if !super::ball::has() {
            drop(s);
            super::ball::idle(f64::from(dt));
            return;
        }
        let grip_r = slot(49) > 0.5;
        let grip_l = slot(155) > 0.5;
        super::ball::tick(f64::from(dt), grip_r, grip_l);
        if since(s.debug_at, now) > 0.25 {
            s.debug_at = Some(now);
            s.debug_cached = slot(156) > 0.5;
        }
        if s.debug_cached {
            s.log_acc += dt;
            if s.log_acc >= 0.5 {
                s.log_acc = 0.0;
                log!("[VRBall] dt={dt:.4} lastImpact={:.3} m/s\n", super::ball::last_impact());
            }
        }
    }
}

/// mods/cet/CyberpunkVRPort_Smoking (the input bridge): the VR inputs from the shared slots into the smoking's tick
/// (mechanics/smoke.rs, formerly the player's VRSmokeTick of mods/redscript/CyberpunkVRPort_Smoking):
///   [154] left trigger, [160] right trigger (0..1), [49] right grip, [155] left grip (binary, > 0.5);
///   [156] the port's debug switch: a status line every 0.4 s.
mod smoking {
    use cvr_host::log;
    use red4ext_abi::handle::Handle;
    use std::sync::Mutex;
    use std::time::Instant;

    unsafe extern "C" {
        fn cvr_cpp_xr_get_shared_slot(slot: i32) -> f32;
    }
    fn slot(index: i32) -> f32 {
        // SAFETY: a read of the shared block (0 when it is not there).
        unsafe { cvr_cpp_xr_get_shared_slot(index) }
    }

    struct Debug {
        cached: bool,
        at: Option<Instant>,
        log_acc: f32,
        reported: bool,
    }
    static DEBUG: Mutex<Debug> = Mutex::new(Debug { cached: false, at: None, log_acc: 0.0, reported: false });

    /// The debug switch, read a quarter second at most (the mod's vrDebug).
    fn debug(d: &mut Debug) -> bool {
        let now = Instant::now();
        if d.at.is_none_or(|at| now.duration_since(at).as_secs_f32() > 0.25) {
            d.at = Some(now);
            d.cached = slot(156) > 0.5;
        }
        d.cached
    }

    pub(super) fn tick(dt: f32) {
        let mut player = Handle::null();
        // SAFETY: GetPlayer(GameInstance) into a handle of ours, released when it drops.
        unsafe { crate::anim_lookup::get_player(&mut player) };
        if player.is_null() {
            return;
        }
        let lt = slot(154);
        let rt = slot(160);
        let grip_r = slot(49) > 0.5;
        let grip_l = slot(155) > 0.5;
        // A trigger is 0..1; anything else (NaN included) is another subsystem's data in the slot: refused.
        if !(0.0..=1.0).contains(&lt) || !(0.0..=1.0).contains(&rt) {
            return;
        }
        let mut d = DEBUG.lock().unwrap_or_else(|p| p.into_inner());
        let (lit, light, mouth_r, mouth_l) =
            super::smoke::tick(&player, f64::from(lt), f64::from(rt), [grip_r, grip_l], f64::from(dt));
        super::smoke::self_test(f64::from(dt));
        if !d.reported {
            d.reported = true;
            log!("[mechanics] smoking: the gestures from Rust (mechanics/smoke.rs)\n");
        }
        if !debug(&mut d) {
            return;
        }
        d.log_acc += dt;
        if d.log_acc >= 0.4 {
            d.log_acc = 0.0;
            log!(
                "[Smoke] lit={lit} lt={lt:.2} rt={rt:.2} lightDist={light:.3} mouthR={mouth_r:.3} mouthL={mouth_l:.3} \
                 gripR={grip_r} gripL={grip_l}\n"
            );
        }
    }
}
