//! mods/cet/CyberpunkVRPort_ForceFPP (the camera bridge: it never forces a perspective, it publishes scene, device and
//! UI state to the plugin), every frame:
//! - which UI overlay owns the B button (VRUiPopup): the phone (UI_ComDevice.ContactsActive, every frame), the radio
//!   port and the vehicle list (their marker widgets -- RadioportTitle, vehicleIconContainer -- under a visible slot
//!   of the HUD's NotificationsContainer, ten times a second; the container found once through the ink layers and
//!   kept while it answers). The two popups' Root sits lower in VR, a share of the window height (8 % / 15 %);
//! - a braindance (VRInBraindance) and the scene gate (VRBraindance, VRSceneCamera, VRPlayerCamera, VRBdSceneFov): open
//!   in a braindance, or at SceneTier 2+ latched on the scene's first frame when the active camera's fov differs from
//!   the player's camera component's and the scene reports a camera of its own (let go after 0.5 s without one), never
//!   while a device takeover owns the second eye; the active camera's fov published during a takeover even closed;
//! - the taken-over camera (VRRemoteCamera, VRTakeoverEntity, VRDevCamOffset): the controlled object's lens (the
//!   camera component named cameraComponent, else the first camera component), its world position every frame, the
//!   entity id when it changes, the headset's fov held in the lens's own field (the authored value handed back), the
//!   viewpoint offset by context (an AV turret's baked framing, devcam.json's overrides, read again on a change);
//! - twice a second: the legacy GameplayRestriction.VehicleFPP removed once per player, and the woman's body stand-in
//!   (vrp_body_stand_in, in the body's skin tone while t0_000_pwa_base__full is gone but its shadow is there; not in
//!   the hands-only mode), between the player's attach and detach (the Lua observers report them: VRCameraBridgeAttach).
//!
//! The OnInitialize hook of the popups and the CET overlay (its A/B knobs report through VRCameraBridgeKnobs, the
//! taken-over context through VRCameraBridgeContext) stay the Lua host's.
use crate::script_call::{call_handle, call_native, method, method_typed, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

const RESTRICTION: &str = "GameplayRestriction.VehicleFPP";
const SCENE_CAM_LOST_SEC: f64 = 0.5;
/// The taken-over camera's viewpoint offset per kind (right, forward, up): the AV turret's tuned framing, zero else.
const BAKED_AV: [f32; 3] = [-0.514, 0.100, -0.224];
const BAKED_DEFAULT: [f32; 3] = [0.0, 0.0, 0.0];

/// The overlay's knobs: the scene camera path in ordinary scenes, and during a takeover (an A/B).
pub(in crate::mechanics) static SCENE_GATE: AtomicBool = AtomicBool::new(true);
pub(in crate::mechanics) static SCENE_IN_TAKEOVER: AtomicBool = AtomicBool::new(false);
/// What the overlay's viewpoint sliders edit: bit 0 the AV context, bit 1 a takeover is live.
pub(in crate::mechanics) static CONTEXT: AtomicI32 = AtomicI32::new(0);
/// The stand-in runs between the player's attach and detach.
static ATTACHED: AtomicI32 = AtomicI32::new(-1);

struct State {
    ui_acc: f64,
    ui_widget: bool,
    ui_what: &'static str,
    ui_sent: Option<i32>,
    notif: Handle,
    notif_window: Handle,
    cleaned: Option<u64>,
    cleanup_time: f64,
    stand_on: bool,
    stand_tone: Option<u64>,
    stand_shown: bool,
    dev_obj: usize,
    dev_cam: Handle,
    dev_fov0: Option<f32>,
    ctx_av: bool,
    last_entity: Option<u64>,
    latched: bool,
    scene_cam_lost: f64,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static REPORT: super::PerPlayer = super::PerPlayer::new();

type R<T> = Result<T, String>;
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    if o.is_null() {
        return Err(format!("{name}: no object"));
    }
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn h(o: &Handle, name: &str, values: &[Value]) -> Handle {
    if o.is_null() {
        return Handle::null();
    }
    // SAFETY: as above.
    unsafe { call_handle(o.instance, name, values) }.unwrap_or_else(Handle::null)
}
fn native(name: &str, values: &[Value]) {
    // SAFETY: one of the plugin's own natives, called as the script VM calls it.
    unsafe { call_native::<i32>(name, values) };
}
fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
/// Game.GetScriptableSystemsContainer():Get(name).
fn scriptable_system(name: &str) -> Handle {
    let container = super::game_system("GetScriptableSystemsContainer");
    h(&container, "Get", &[Value::U64(CName::hashed(name).0)])
}
fn name_of(w: &Handle) -> String {
    m::<u64>(w, "GetName", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default()
}
fn children(w: &Handle) -> i32 {
    // a leaf widget has no GetNumChildren (the Lua walk's pcall): no children, and no call made
    // SAFETY: the widget, held.
    if w.is_null() || unsafe { crate::script_call::find_function(w.instance, "GetNumChildren", 0) }.is_none() {
        return 0;
    }
    m::<i32>(w, "GetNumChildren", &[]).unwrap_or(0)
}
fn child(w: &Handle, i: i32) -> Handle {
    h(w, "GetWidgetByIndex", &[Value::Int32(i)])
}
fn subtree_has(w: &Handle, want: &str, depth: i32) -> bool {
    depth <= 6
        && (0..children(w)).any(|k| {
            let c = child(w, k);
            !c.is_null() && (name_of(&c) == want || subtree_has(&c, want, depth + 1))
        })
}
fn find_named(w: &Handle, want: &str, depth: i32) -> Handle {
    if depth > 4 {
        return Handle::null();
    }
    for k in 0..children(w) {
        let c = child(w, k);
        if c.is_null() {
            continue;
        }
        if name_of(&c) == want {
            return c;
        }
        let deeper = find_named(&c, want, depth + 1);
        if !deeper.is_null() {
            return deeper;
        }
    }
    Handle::null()
}
/// The HUD's NotificationsContainer: kept while it answers, found again through the ink layers when it does not.
fn notifications_container(s: &mut State) -> Handle {
    if !s.notif.is_null() && m::<i32>(&s.notif, "GetNumChildren", &[]).is_ok_and(|n| n >= 0) {
        return s.notif.clone();
    }
    s.notif = Handle::null();
    for layer in crate::engine_objects::layer_wrappers() {
        let vw = h(&layer, "GetVirtualWindow", &[]);
        if vw.is_null() {
            continue;
        }
        let cont = find_named(&vw, "NotificationsContainer", 0);
        if !cont.is_null() {
            s.notif = cont;
            s.notif_window = vw;
            break;
        }
    }
    s.notif.clone()
}
/// The radio port's and the vehicle list's Root, lowered by a share of the window height.
fn drop_popup(s: &State, root: &Handle, what: &str) {
    let share = match what {
        "radio" => 0.08,
        "vehicle" => 0.15,
        _ => return,
    };
    let Ok(size) = m::<[f32; 2]>(&s.notif_window, "GetSize", &[]) else { return };
    if size[1] <= 0.0 || root.is_null() {
        return;
    }
    let want = share * size[1];
    let Ok(t) = m::<[f32; 2]>(root, "GetTranslation", &[]) else { return };
    if (t[1] - want).abs() > 0.5 || t[0].abs() > 0.5 {
        let v = [0.0f32, want];
        // SAFETY: the widget, held; the value lives for the call.
        unsafe { method_typed::<()>(root.instance, "SetTranslation", &["Vector2"], &[Value::Ptr(v.as_ptr().cast())]) };
    }
}
fn popup_widget_open(s: &mut State) -> (bool, &'static str) {
    let cont = notifications_container(s);
    if cont.is_null() {
        return (false, "-");
    }
    for k in 0..children(&cont) {
        let slot = child(&cont, k);
        if slot.is_null() || !m::<bool>(&slot, "IsVisible", &[]).unwrap_or(false) {
            continue;
        }
        let what = if subtree_has(&slot, "RadioportTitle", 0) {
            "radio"
        } else if subtree_has(&slot, "vehicleIconContainer", 0) {
            "vehicle"
        } else {
            continue;
        };
        drop_popup(s, &child(&slot, 0), what);
        return (true, what);
    }
    (false, "-")
}
fn phone_open() -> bool {
    (|| -> R<bool> {
        let defs = super::boards::defs()?;
        let d = super::boards::definition(&defs, "UI_ComDevice")?;
        let board = super::boards::system_board(d)?;
        if board.is_null() {
            return Ok(false);
        }
        super::boards::get::<bool>(&board, "GetBool", super::boards::field(d, "ContactsActive")?)
    })()
    .unwrap_or(false)
}
fn publish_ui_popup(s: &mut State, dt: f64) {
    s.ui_acc += dt;
    if s.ui_acc >= 0.1 {
        s.ui_acc = 0.0;
        (s.ui_widget, s.ui_what) = popup_widget_open(s);
    }
    let open = s.ui_widget || phone_open();
    let want = i32::from(open);
    if s.ui_sent != Some(want) {
        native("VRUiPopup", &[Value::Int32(want)]);
        s.ui_sent = Some(want);
    }
}

fn scene_interface() -> Handle {
    h(&super::game_system("GetSceneSystem"), "GetScriptInterface", &[])
}
fn scene_tier(p: &Handle) -> i32 {
    (|| -> R<i32> {
        let bb = h(p, "GetPlayerStateMachineBlackboard", &[]);
        if bb.is_null() {
            return Ok(-1);
        }
        let defs = super::boards::defs()?;
        let d = super::boards::definition(&defs, "PlayerStateMachine")?;
        super::boards::get::<i32>(&bb, "GetInt", super::boards::field(d, "SceneTier")?)
    })()
    .unwrap_or(-1)
}
/// The scene's camera position (WorldPosition.ToVector4: fixed point, 17 fraction bits); None for the origin the getter
/// returns outside a scene.
fn scene_camera_position(si: &Handle) -> Option<[f32; 3]> {
    let fixed = m::<[i32; 3]>(si, "GetSceneSystemCameraLastCameraPosition", &[]).ok()?;
    let v = fixed.map(|b| b as f32 / 131072.0);
    (v[0].abs() + v[1].abs() + v[2].abs() > 1.0).then_some(v)
}
fn publish_braindance(s: &mut State, p: &Handle, dt: f64) {
    let bd = m::<bool>(&scriptable_system("BraindanceSystem"), "GetIsInBraindance", &[]).unwrap_or(false);
    native("VRInBraindance", &[Value::Int32(i32::from(bd))]);
    let mut on = bd;
    let tier = if p.is_null() { -1 } else { scene_tier(p) };
    let active_fov = m::<f32>(&super::game_system("GetCameraSystem"), "GetActiveCameraFOV", &[]).unwrap_or(0.0);
    let comp_fov = m::<f32>(&h(p, "GetFPPCameraComponent", &[]), "GetFOV", &[]).unwrap_or(0.0);
    let mismatch = active_fov > 1.0 && comp_fov > 1.0 && (active_fov - comp_fov).abs() > 2.0;
    let takeover = !h(&scriptable_system("TakeOverControlSystem"), "GetControlledObject", &[]).is_null();
    let blocked = takeover && !SCENE_IN_TAKEOVER.load(Ordering::Relaxed);
    let scene_cam = tier >= 2 && scene_camera_position(&scene_interface()).is_some();
    if scene_cam {
        s.scene_cam_lost = 0.0;
    } else {
        s.scene_cam_lost += dt;
    }
    if tier >= 2 && !blocked {
        if mismatch && scene_cam {
            s.latched = true;
        }
        if s.scene_cam_lost >= SCENE_CAM_LOST_SEC {
            s.latched = false;
        }
    } else {
        s.latched = false;
    }
    if SCENE_GATE.load(Ordering::Relaxed) && tier >= 2 && s.latched && !blocked {
        on = true;
    }
    if takeover && active_fov > 1.0 {
        native("VRBdSceneFov", &[Value::Float(active_fov)]);
    }
    let no_scene = [Value::Int32(0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(1.0)];
    if !on {
        native("VRBraindance", &[Value::Int32(0), Value::Float(0.0)]);
        native("VRSceneCamera", &no_scene);
        return;
    }
    // The live player's camera (the replacer carries one named `camera` too).
    match m::<[[f32; 4]; 4]>(&h(p, "FindComponentByName", &[Value::U64(CName::hashed("camera").0)]), "GetLocalToWorld", &[]) {
        Ok(l2w) => native("VRPlayerCamera", &[Value::Int32(1), Value::Float(l2w[3][0]), Value::Float(l2w[3][1]), Value::Float(l2w[3][2])]),
        Err(_) => native("VRPlayerCamera", &[Value::Int32(0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)]),
    }
    // The scene's own camera pose, only while the scene owns the camera and reports a real one.
    let si = scene_interface();
    if !si.is_null() {
        let owns = m::<bool>(&si, "GetSceneSystemCameraControlEnabled", &[]).unwrap_or(false);
        let pose = owns
            .then(|| scene_camera_position(&si).zip(m::<[f32; 4]>(&si, "GetSceneSystemCameraLastCameraOrientation", &[]).ok()))
            .flatten();
        match pose {
            Some((v, q)) => native(
                "VRSceneCamera",
                &[Value::Int32(1), Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2]), Value::Float(q[0]), Value::Float(q[1]), Value::Float(q[2]), Value::Float(q[3])],
            ),
            None => native("VRSceneCamera", &no_scene),
        }
    }
    native("VRBdSceneFov", &[Value::Float(active_fov)]);
    native("VRBraindance", &[Value::Int32(1), Value::Float(active_fov)]);
}

/// devcam.json's override for a context (`"av": { "right": .., "forward": .., "up": .. }`; a flat file is the AV's).
fn devcam_user(av: bool) -> Option<[f32; 3]> {
    let text = std::fs::read_to_string(cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_ForceFPP\\devcam.json")).ok()?;
    let triple = |chunk: &str| -> Option<[f32; 3]> {
        let mut out = [0.0f32; 3];
        for (i, k) in ["right", "forward", "up"].iter().enumerate() {
            let at = chunk.find(&format!("\"{k}\""))? + k.len() + 2;
            let rest = chunk[at..].trim_start().strip_prefix(':')?.trim_start();
            let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.')).unwrap_or(rest.len());
            out[i] = rest[..end].parse().ok()?;
        }
        Some(out)
    };
    let block = |ctx: &str| -> Option<&str> {
        let at = text.find(&format!("\"{ctx}\""))?;
        let open = at + text[at..].find('{')?;
        let close = open + text[open..].find('}')?;
        Some(&text[open + 1..close])
    };
    let any = block("av").and_then(triple).is_some() || block("default").and_then(triple).is_some();
    match (av, any) {
        (_, true) => block(if av { "av" } else { "default" }).and_then(triple),
        (true, false) => triple(&text),
        (false, false) => None,
    }
}
fn devcam_apply(s: &mut State, av: bool) {
    s.ctx_av = av;
    let v = devcam_user(av).unwrap_or(if av { BAKED_AV } else { BAKED_DEFAULT });
    native("VRDevCamOffset", &[Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])]);
}
fn class_name(o: &Handle) -> String {
    // SAFETY: a live scriptable, held.
    unsafe { red4ext_abi::rtti::object_class(o.instance) }.map(|c| c.class_name().text().to_string()).unwrap_or_default()
}
fn release_lens_fov(s: &mut State) {
    if let (Some(f), false) = (s.dev_fov0, s.dev_cam.is_null()) {
        // SAFETY: the camera component's Float property.
        if let Some(p) = unsafe { crate::script_call::property(s.dev_cam.instance, "fov") } {
            unsafe { *(p as *mut f32) = f };
        }
    }
    s.dev_fov0 = None;
}
/// The headset's fov (the player's camera component's, which the port forces) held in the lens's own field.
fn hold_lens_fov(s: &mut State, p: &Handle) {
    if s.dev_cam.is_null() {
        return;
    }
    let want = m::<f32>(&h(p, "GetFPPCameraComponent", &[]), "GetFOV", &[]).unwrap_or(0.0);
    if !(want > 1.0 && want < 179.0) {
        return;
    }
    // SAFETY: the camera component's Float property.
    let Some(f) = (unsafe { crate::script_call::property(s.dev_cam.instance, "fov") }) else { return };
    let f = f as *mut f32;
    // SAFETY: as above.
    let cur = unsafe { *f };
    if s.dev_fov0.is_none() && cur > 1.0 && cur < 179.0 && (cur - want).abs() > 0.01 {
        s.dev_fov0 = Some(cur);
    }
    if (cur - want).abs() > 0.01 {
        // SAFETY: as above.
        unsafe { *f = want };
    }
}
fn publish_remote_camera(s: &mut State, p: &Handle) {
    let obj = h(&scriptable_system("TakeOverControlSystem"), "GetControlledObject", &[]);
    if obj.is_null() {
        if s.ctx_av {
            devcam_apply(s, false);
        }
        release_lens_fov(s);
        s.dev_obj = 0;
        s.dev_cam = Handle::null();
        CONTEXT.store(0, Ordering::Relaxed);
        native("VRRemoteCamera", &[Value::Int32(0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)]);
        if s.last_entity.take().is_some() {
            set_takeover_entity(0);
        }
        return;
    }
    // The framing belongs to the mount: a class with "AV" in it (vehicleAVBaseObject) is the AV turret.
    let av = class_name(&obj).contains("AV");
    if av != s.ctx_av {
        devcam_apply(s, av);
    }
    CONTEXT.store(2 | i32::from(av), Ordering::Relaxed);
    if s.dev_obj != obj.instance as usize {
        release_lens_fov(s);
        s.dev_obj = obj.instance as usize;
        s.dev_cam = Handle::null();
        // The lens by name first (a SurveillanceCamera carries a virtualcamera and a cameraComponent), then by class.
        let mut any = Handle::null();
        for c in crate::engine_objects::entity_components(&obj) {
            if !class_name(&c).contains("CameraComponent") {
                continue;
            }
            if name_of(&c) == "cameraComponent" {
                s.dev_cam = c;
                break;
            }
            if any.is_null() {
                any = c;
            }
        }
        if s.dev_cam.is_null() {
            s.dev_cam = any;
        }
    }
    let lens = m::<[[f32; 4]; 4]>(&s.dev_cam, "GetLocalToWorld", &[]).ok().map(|l2w| [l2w[3][0], l2w[3][1], l2w[3][2]]);
    // The identity, not the place: the entity id, when it changes.
    if let Ok(id) = m::<u64>(&obj, "GetEntityID", &[]) {
        if s.last_entity != Some(id) {
            set_takeover_entity(id);
            s.last_entity = Some(id);
        }
    }
    hold_lens_fov(s, p);
    let at = lens.or_else(|| m::<[f32; 4]>(&obj, "GetWorldPosition", &[]).ok().map(|v| [v[0], v[1], v[2]]));
    match at {
        Some(v) => native("VRRemoteCamera", &[Value::Int32(1), Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])]),
        None => native("VRRemoteCamera", &[Value::Int32(0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)]),
    }
}
fn set_takeover_entity(id: u64) {
    let Ok(text) = std::ffi::CString::new(id.to_string()) else { return };
    let Some(cs) = CString::new(&text) else { return };
    native("VRTakeoverEntity", &[Value::Ptr((&cs as *const CString).cast())]);
}

/// The legacy saved GameplayRestriction.VehicleFPP removed, once per player.
fn clear_legacy_restriction(s: &mut State, p: &Handle) {
    let Ok(id) = m::<u64>(p, "GetEntityID", &[]) else { return };
    if s.cleaned == Some(id) {
        return;
    }
    let sys = super::game_system("GetStatusEffectSystem");
    if sys.is_null() {
        return;
    }
    let tdb = red4ext_abi::tweakdbid::from_name(RESTRICTION);
    let ok = m::<bool>(&sys, "HasStatusEffect", &[Value::U64(id), Value::U64(tdb)])
        .and_then(|has| if has { m::<()>(&sys, "RemoveStatusEffect", &[Value::U64(id), Value::U64(tdb)]) } else { Ok(()) });
    if ok.is_ok() {
        s.cleaned = Some(id);
    }
}
fn mesh_appearance(c: &Handle) -> Option<u64> {
    if c.is_null() {
        return None;
    }
    // SAFETY: the mesh component's CName property.
    unsafe { crate::script_call::property(c.instance, "meshAppearance") }.map(|p| unsafe { *(p as *const u64) })
}
/// The woman's body stand-in: on in the body's skin tone while the game took the body part away.
fn stand_in_update(s: &mut State, p: &Handle) -> R<()> {
    let find = |n: &str| h(p, "FindComponentByName", &[Value::U64(CName::hashed(n).0)]);
    let stand = find("vrp_body_stand_in");
    if stand.is_null() {
        return Ok(());
    }
    let mut real = find("t0_000_pwa_base__full");
    if !real.is_null() && !m::<bool>(&real, "IsEnabled", &[])? {
        real = Handle::null();
    }
    if !real.is_null() {
        s.stand_tone = mesh_appearance(&real);
    }
    let mut tone = s.stand_tone;
    if real.is_null() && tone.is_none() {
        tone = mesh_appearance(&find("left_arm"));
    }
    let mut want = real.is_null() && tone.is_some() && !find("t0_000_pwa_base__full_shadow").is_null();
    // SAFETY: the shared block's read (the hands-only mode hides the body on purpose).
    if want && unsafe { cvr_rig::vrik::provider::shared_slot(173) } > 0.5 {
        want = false;
    }
    if let (true, Some(t)) = (want, tone) {
        if mesh_appearance(&stand) != Some(t) {
            crate::engine_objects::component_change_appearance(&stand, t, false);
        }
    }
    if m::<bool>(&stand, "IsEnabled", &[])? != want {
        m::<()>(&stand, "Toggle", &[Value::Bool(want)])?;
        if want != s.stand_shown {
            s.stand_shown = want;
            let tone = tone.map(|t| CName(t).text().to_string()).unwrap_or_else(|| "nil".into());
            log!("[ForceFPP] body stand-in {} ({tone}) from Rust\n", if want { "on" } else { "off" });
        }
    }
    Ok(())
}
fn pre_game() -> bool {
    super::requests_handler().ok().filter(|r| !r.is_null()).is_some_and(|r| m::<bool>(&r, "IsPreGame", &[]).unwrap_or(false))
}

/// VRCameraBridgeAttach(attached): the player attached (the legacy restriction checked again, the stand-in on) or
/// detached (the stand-in off).
/// BaseModalListPopupGameController.OnInitialize (mechanics/game_hooks.rs; the CET mod's dropPopupController): the
/// radio port's or the vehicle list's Root lowered at once, not a frame later.
pub(in crate::mechanics) fn popup_initialized(ctrl: &Handle) {
    let is = |c: &str| crate::mechanics::lobby_rtti::call(ctrl, "IsA", &[crate::mechanics::lobby_rtti::A::Name(red4ext_abi::cname::CName::hashed(c).0)]).is_some_and(|o| crate::mechanics::lobby_rtti::Val::of(&o).bool());
    let what = if is("VehicleRadioPopupGameController") {
        "radio"
    } else if is("VehiclesManagerPopupGameController") {
        "vehicle"
    } else {
        return;
    };
    let root = h(ctrl, "GetRootWidget", &[]);
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = g.as_mut() {
        notifications_container(s);
        drop_popup(s, &root, what);
    }
}
pub(in crate::mechanics) fn attach(attached: bool) {
    ATTACHED.store(i32::from(attached), Ordering::Relaxed);
}

/// The mod's onUpdate.
pub(in crate::mechanics) fn tick(dt: f32) {
    let dt = f64::from(dt);
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        ui_acc: 0.0,
        ui_widget: false,
        ui_what: "-",
        ui_sent: None,
        notif: Handle::null(),
        notif_window: Handle::null(),
        cleaned: None,
        cleanup_time: 0.5,
        stand_on: false,
        stand_tone: None,
        stand_shown: false,
        dev_obj: 0,
        dev_cam: Handle::null(),
        dev_fov0: None,
        ctx_av: false,
        last_entity: None,
        latched: false,
        scene_cam_lost: 0.0,
    });
    match ATTACHED.swap(-1, Ordering::Relaxed) {
        1 => {
            s.cleaned = None;
            (s.stand_on, s.stand_tone, s.stand_shown) = (true, None, false);
        }
        0 => (s.stand_on, s.stand_tone, s.stand_shown) = (false, None, false),
        _ => {}
    }
    let p = player();
    publish_ui_popup(s, dt);
    publish_braindance(s, &p, dt);
    publish_remote_camera(s, &p);
    s.cleanup_time += dt;
    if s.cleanup_time >= 0.5 {
        s.cleanup_time = 0.0;
        if p.is_null() {
            // The session ended: what was kept belongs to it.
            s.cleaned = None;
            s.notif = Handle::null();
            s.notif_window = Handle::null();
        } else {
            clear_legacy_restriction(s, &p);
            if s.stand_on && !pre_game() {
                if let Err(e) = stand_in_update(s, &p) {
                    log!("[ForceFPP] body stand-in failed: {e} (from Rust)\n");
                    s.stand_on = false;
                }
            }
            // Half a second in, the popup scan has run: a report per player (the menu's puppet, the save's player).
            if REPORT.first() {
                log!(
                    "[mechanics] camera bridge: publishing from Rust (popup {}, tier {}, takeover {}, notifications container {})\n",
                    s.ui_what,
                    scene_tier(&p),
                    s.dev_obj != 0,
                    !s.notif.is_null()
                );
            }
        }
    }
}

/// The viewpoint the camera bridge's window edits: (its context is the AV's, the offsets).
static DEV: Mutex<Option<(bool, [f32; 3])>> = Mutex::new(None);
/// devcam.json's override of a context, its block only ("av" also reads the old flat file).
fn devcam_override(av: bool) -> Option<[f32; 3]> {
    devcam_user(av)
}
/// devcam.json as the mod writes it: only the contexts the player overrode.
fn devcam_save(av: bool, now: Option<[f32; 3]>) {
    let other = devcam_override(!av);
    let (av_v, default_v) = if av { (now, other) } else { (other, now) };
    let parts: Vec<String> = [("av", av_v), ("default", default_v)]
        .iter()
        .filter_map(|(ctx, v)| v.map(|v| format!("  \"{ctx}\": {{ \"right\": {:.3}, \"forward\": {:.3}, \"up\": {:.3} }}", v[0], v[1], v[2])))
        .collect();
    let text = if parts.is_empty() { "{}\n".to_string() } else { format!("{{\n{}\n}}\n", parts.join(",\n")) };
    let _ = std::fs::write(cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_ForceFPP\\devcam.json"), text);
}
/// The mod's "VR camera bridge" window (dev_windows.rs): the B button's owner, the A/B knobs, the taken-over camera's
/// viewpoint for its context -- devcam.json's override or the baked one, pushed as it moves and saved as the mod
/// saved it.
pub(super) fn dev_window(frame: [f32; 2]) {
    use super::dev_windows as w;
    w::window("VR camera bridge", [0.01, 0.04], [0.19, 0.45], frame, || {
        w::text("published from Rust (mechanics/force_fpp.rs): the state lines are the log's");
        let what = STATE.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map_or("-", |s| s.ui_what);
        w::text(&format!("UI overlay owning B: {what}"));
        let mut gate = SCENE_GATE.load(Ordering::Relaxed);
        if w::checkbox("scene camera path in ordinary scenes", &mut gate) {
            SCENE_GATE.store(gate, Ordering::Relaxed);
        }
        w::separator();
        let mut takeover = SCENE_IN_TAKEOVER.load(Ordering::Relaxed);
        if w::checkbox("scene path during a takeover (A/B)", &mut takeover) {
            SCENE_IN_TAKEOVER.store(takeover, Ordering::Relaxed);
        }
        w::text("off = the device path owns the second eye (no jerk, no head steering?)");
        w::text("on  = the scene path writes MAIN through the located buffer as well");
        w::separator();
        w::text("Viewpoint of a taken-over camera (turret, surveillance), metres:");
        let av = CONTEXT.load(Ordering::Relaxed) & 1 == 1;
        let mut dev = DEV.lock().unwrap_or_else(|p| p.into_inner());
        if !matches!(*dev, Some((ctx, _)) if ctx == av) {
            *dev = Some((av, devcam_override(av).unwrap_or(if av { BAKED_AV } else { BAKED_DEFAULT })));
        }
        let Some((_, v)) = dev.as_mut() else { return };
        let saved = devcam_override(av).is_some();
        w::text(&format!("offsets for: {}{}", if av { "av" } else { "default" }, if saved { "  (saved override)" } else { "  (baked default)" }));
        let mut changed = false;
        changed |= w::slider("right / left", &mut v[0], -3.0, 3.0);
        changed |= w::slider("forward / back", &mut v[1], -3.0, 3.0);
        changed |= w::slider("up / down", &mut v[2], -3.0, 3.0);
        if w::button("back to the shipped default") {
            *v = if av { BAKED_AV } else { BAKED_DEFAULT };
            native("VRDevCamOffset", &[Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])]);
            devcam_save(av, None);
        }
        if w::button("reset to the lens itself") {
            *v = [0.0; 3];
            changed = true;
        }
        if changed {
            native("VRDevCamOffset", &[Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])]);
            devcam_save(av, Some(*v));
        }
        w::text("Applies only while a camera is taken over; forward is the lens's own");
        w::text("horizontal aim, up is world up, and both eyes move together.");
        w::separator();
        w::text("Blocking is the game's own: GameplayRestriction.VehicleFPP makes");
        w::text("IsVehicleCameraChangeBlocked true, so ToggleVehCamera does nothing.");
    });
}
