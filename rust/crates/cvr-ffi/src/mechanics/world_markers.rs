//! CyberpunkVRPort_Cyberware's world markers: the HUD's markers drawn where their thing is in the world.
//!
//! - The cyberware breach marker: the tracked breach's world point (the player's breach finder) projected by the plugin
//!   (VRWorldMarkerScreenPoint) for the controller's screen position and fluff, its canvas at half scale, the root
//!   anchored there (VRWorldMarkerAnchor); its canvas and anchor kept for VRBreachGeometry.
//! - NPC nameplates, damage digits, accumulated damage digits and chatter lines take their projection from the plugin
//!   (VRWorldMarkerProjection); mappins put nothing into the nameplate's slot, and the hostile stealth, quest and
//!   quickhack mappins project to screen space when their nameplate shows.
use super::lobby_rtti::{self as rt, A};
use super::menu_rt as r;
use crate::script_call::Value;
use crate::script_hooks::{Call, HookFn, Kind};
use red4ext_abi::handle::{Handle, WeakHandle};
use std::sync::Mutex;

/// The breach controller's canvas (its breachCanvasWRef) and anchor, the visible flag.
struct Breach {
    canvas: Option<WeakHandle>,
    anchor: [f32; 4],
    visible: bool,
}
// SAFETY: the weak handle is used on the game's main thread only.
unsafe impl Send for Breach {}
static BREACH: Mutex<Breach> = Mutex::new(Breach { canvas: None, anchor: [0.0; 4], visible: false });

fn native_v4(name: &str, v: [f32; 4]) -> [f32; 4] {
    super::belt::native::<[f32; 4]>(name, &[Value::Ptr(v.as_ptr().cast())]).unwrap_or([0.0; 4])
}
/// gameaiCyberwareBreachGameController.OnBreachDataChanged (after).
fn breach_changed(call: &mut Call) {
    let this = call.this();
    let params = call.arg_variant_handle(0);
    let tracking = !params.is_null() && rt::field(&params, "tracking").is_some_and(|v| v.bool());
    let visible = tracking && rt::field(&params, "visible").is_some_and(|v| v.bool());
    BREACH.lock().unwrap_or_else(|p| p.into_inner()).visible = visible;
    // its name in the RTTI (the script dump's m_breachCanvasWRef is the decompiler's): looked up as the dump has it the
    // canvas was never found and the diamond kept the game's placement, sliding with the head
    let Ok(canvas) = super::world_touch::widget_ref(&this, "breachCanvasWRef") else { return };
    if canvas.is_null() {
        return;
    }
    let mut world = [0.0f32; 4];
    let pl = super::belt::player();
    if tracking && !pl.is_null() {
        let finder = r::o(&pl, "GetBreachFinderComponent", &[]);
        let breach = r::o(&finder, "GetTrackedBreachComponent", &[]);
        if let Some(p) = rt::call(&breach, "GetPosition", &[]) {
            // SAFETY: a Vector4 result.
            world = unsafe { *(p.as_ptr() as *const [f32; 4]) };
            world[3] = 1.0;
        }
    }
    let mut screen = [0.0f32; 4];
    if world[3] > 0.5 {
        screen = native_v4("VRWorldMarkerScreenPoint", world);
        if screen[3] > 0.5 {
            let v2 = [screen[0], screen[1]];
            super::lobby_ink::call(&this, "ChangeScreenPosition", &[Value::Ptr(v2.as_ptr().cast())]);
            let radius = rt::field(&params, "radius").and_then(|v| v.num()).unwrap_or(0.0);
            super::lobby_ink::call(&this, "ChangeFluff", &[Value::Ptr(v2.as_ptr().cast()), Value::Float(radius as f32)]);
        }
    }
    // a line each time the outcome changes: what the placement was made from, or why the game's stays
    let outcome = (params.is_null(), tracking, world[3] > 0.5, screen[3] > 0.5);
    static SAID: Mutex<Option<(bool, bool, bool, bool)>> = Mutex::new(None);
    let mut said = SAID.lock().unwrap_or_else(|p| p.into_inner());
    if *said != Some(outcome) {
        *said = Some(outcome);
        cvr_host::vlog!(
            "[world-markers] breach: parameters {}, tracking {tracking}, breach point {:?}, UI camera point {:?}
",
            if params.is_null() { "not read" } else { "read" },
            world,
            screen
        );
    }
    drop(said);
    super::lobby_ink::v2(&canvas, "SetScale", 0.5, 0.5);
    {
        let mut b = BREACH.lock().unwrap_or_else(|p| p.into_inner());
        b.canvas = Some(WeakHandle::from_handle(&canvas));
        b.anchor = world;
    }
    let root = rt::obj(&this, "GetRootWidget", &[]);
    super::belt::native::<()>("VRWorldMarkerAnchor", &[Value::Handle(&root), Value::Ptr(world.as_ptr().cast())]);
}
fn projection(widget: &Handle, projection: &Handle) {
    super::belt::native::<()>("VRWorldMarkerProjection", &[Value::Handle(widget), Value::Handle(projection)]);
}
/// NpcNameplateGameController.OnScreenProjectionUpdate (after): the nameplate's projection (the device one for a
/// device) to the plugin.
fn nameplate(call: &mut Call) {
    let this = call.this();
    let field = |f: &str| rt::field(&this, f).map(|v| v.handle()).unwrap_or_else(Handle::null);
    let main = field("m_nameplateProjection");
    let mut selected = Handle::null();
    if !main.is_null() && !r::o(&main, "GetEntity", &[]).is_null() {
        selected = main;
        let buffered = field("m_bufferedGameObject");
        if !buffered.is_null() && r::b(&buffered, "IsDevice", &[]) {
            selected = field("m_nameplateProjectionDevice");
        }
    }
    if let Ok(w) = super::world_touch::widget_ref(&this, "projection") {
        projection(&w, &selected);
    }
}
fn no_nameplate_slot(call: &mut Call) {
    let null = [0u8; 16];
    call.set_result_from(null.as_ptr().cast());
}
fn screen_space(this: &Handle, scale: bool) {
    if scale {
        rt::call(this, "OverrideScaleByDistance", &[A::B(true)]);
    }
    rt::call(this, "SetProjectToScreenSpace", &[A::B(true)]);
}
fn stealth_nameplate(call: &mut Call) {
    let this = call.this();
    let f = |n: &str| rt::field(&this, n).is_some_and(|v| v.bool());
    if call.arg_bool(0) && (f("m_isAggressive") || f("m_isHostile") || f("m_isTurret")) {
        screen_space(&this, true);
    }
}
fn quest_nameplate(call: &mut Call) {
    if call.arg_bool(0) {
        screen_space(&call.this(), true);
    }
}
fn quickhack_nameplate(call: &mut Call) {
    if call.arg_bool(0) {
        screen_space(&call.this(), false);
    }
}
fn digit_projection(call: &mut Call) {
    let this = call.this();
    let root = rt::field(&this, "m_rootWidget").map(|v| v.handle()).unwrap_or_else(Handle::null);
    projection(&root, &call.arg_handle(0));
}
fn chatter_projection(call: &mut Call) {
    let this = call.this();
    let p = rt::field(&this, "m_projection").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if !p.is_null() {
        let root = rt::field(&this, "m_rootWidget").map(|v| v.handle()).unwrap_or_else(Handle::null);
        projection(&root, &p);
    }
}
/// VRBreachGeometry: the breach canvas's scale and size, and its anchor (empty without the canvas).
#[allow(dead_code)]
pub(super) fn breach_geometry() -> Vec<[f32; 4]> {
    let b = BREACH.lock().unwrap_or_else(|p| p.into_inner());
    let Some(c) = b.canvas.as_ref().map(WeakHandle::lock).filter(|h| !h.is_null()) else { return Vec::new() };
    let v2 = |f: &str| super::belt::m::<[f32; 2]>(&c, f, &[]).unwrap_or([0.0; 2]);
    let (s, z) = (v2("GetScale"), v2("GetSize"));
    vec![[s[0], s[1], z[0], z[1]], b.anchor]
}

/// gameCameraSystem.ProjectPoint (after). Under the engine's stereo the HUD canvas is laid out for one eye through
/// that eye's narrowed camera, while ProjectPoint answers through the plain one: half the angle across. The game's
/// scripts never call it; mods do, to place HUD widgets -- NightCoop's player markers among them, which slid off
/// their partner as the head turned. The point the UI's own camera gives the canvas (VRWorldMarkerScreenPoint, y
/// down) is answered instead, in ProjectPoint's terms (y up, its depth kept). Without a fresh UI camera, or for a
/// point the plain camera cannot project (w 0), the game's answer stays.
fn project_point(call: &mut Call) {
    let Some((arg, _)) = call.arg(0) else { return };
    // SAFETY: the Vector4 argument.
    let world = unsafe { *(arg as *const [f32; 4]) };
    let Some((out, _)) = call.result() else { return };
    // SAFETY: the Vector4 result.
    let mut point = unsafe { *(out as *const [f32; 4]) };
    if point[3] <= 0.5 {
        return;
    }
    let ui = native_v4("VRWorldMarkerScreenPoint", [world[0], world[1], world[2], 1.0]);
    if ui[3] <= 0.5 {
        return;
    }
    point[0] = ui[0];
    point[1] = -ui[1];
    call.set_result_from(point.as_ptr().cast());
}

/// NightCoop's player markers, found in the HUD window: (the marker canvas, its partner's nickname).
struct CoopMarkers {
    markers: Vec<(WeakHandle, String)>,
    scanned: Option<std::time::Instant>,
}
// SAFETY: the weak handles are used on the game's main thread only.
unsafe impl Send for CoopMarkers {}
static COOP_MARKERS: Mutex<CoopMarkers> = Mutex::new(CoopMarkers { markers: Vec::new(), scanned: None });
/// How high over a partner's body its marker is anchored, m: the depth the marker is lifted to (the marker stays where
/// NightCoop's projection puts it on the canvas; only its distance comes from here).
const COOP_MARKER_HEIGHT: f32 = 2.0;

/// A partner of the session: (nickname, the point its marker is anchored at).
fn coop_partners() -> Vec<(String, [f32; 4])> {
    let Some(peers) = rt::static_call("NightCoopSdk", "Peers", &[]) else { return Vec::new() };
    let peers = rt::Val::of(&peers);
    let mut out = Vec::new();
    for i in 0..peers.len() {
        let Some(peer) = peers.at(i).and_then(|v| v.num()) else { continue };
        let name = rt::static_call("NightCoopSdk", "PeerName", &[A::N(peer)]).map(|o| rt::Val::of(&o).text()).unwrap_or_default();
        let Some(body) = rt::static_call("NightCoopSdk", "PartnerBody", &[A::N(peer)]) else { continue };
        // SAFETY: an EntityID result: its hash first.
        let id = unsafe { *(body.as_ptr() as *const u64) };
        if name.is_empty() || id == 0 {
            continue;
        }
        let Ok(entity) = super::find_entity(id) else { continue };
        if entity.is_null() {
            continue;
        }
        let Some(p) = rt::call(&entity, "GetWorldPosition", &[]) else { continue };
        // SAFETY: a Vector4 result.
        let p = unsafe { *(p.as_ptr() as *const [f32; 4]) };
        out.push((name, [p[0], p[1], p[2] + COOP_MARKER_HEIGHT, 1.0]));
    }
    out
}
/// The HUD window (the top of a HUD controller's root).
fn hud_window() -> Handle {
    let layer = crate::engine_objects::layer_wrapper(red4ext_abi::cname::CName::hashed("inkHUDLayer").0);
    if layer.is_null() {
        return Handle::null();
    }
    let Ok(controllers) = super::hud_panel::game_controllers(&layer) else { return Handle::null() };
    for c in controllers.iter().filter(|c| !c.is_null()) {
        let mut w = rt::obj(c, "GetRootWidget", &[]);
        for _ in 0..32 {
            if w.is_null() {
                break;
            }
            let parent = rt::obj(&w, "GetParentWidget", &[]);
            if parent.is_null() {
                return w;
            }
            w = parent;
        }
    }
    Handle::null()
}
fn children(w: &Handle) -> Vec<Handle> {
    // SAFETY: the widget, held.
    if w.is_null() || unsafe { crate::script_call::find_function(w.instance, "GetNumChildren", 0) }.is_none() {
        return Vec::new();
    }
    let n = rt::num(w, "GetNumChildren", &[]).unwrap_or(0.0) as i32;
    (0..n).map(|i| rt::obj(w, "GetWidgetByIndex", &[A::N(f64::from(i))])).filter(|c| !c.is_null()).collect()
}
/// NightCoop's markers under the HUD window: a nickname text's parent, in a child of the window other than the HUD's
/// own Root, two levels down at most.
fn scan_coop_markers(names: &[&str]) -> Vec<(WeakHandle, String)> {
    let window = hud_window();
    let mut found = Vec::new();
    for layer in children(&window) {
        if rt::call(&layer, "GetName", &[]).map(|o| rt::Val::of(&o).text()).as_deref() == Some("Root") {
            continue;
        }
        for marker in children(&layer) {
            for part in children(&marker) {
                if rt::call(&part, "GetClassName", &[]).map(|o| rt::Val::of(&o).text()).as_deref() != Some("inkTextWidget") {
                    continue;
                }
                let text = rt::call(&part, "GetText", &[]).map(|o| rt::Val::of(&o).text()).unwrap_or_default();
                if let Some(name) = names.iter().find(|n| **n == text) {
                    found.push((WeakHandle::from_handle(&marker), (*name).to_string()));
                }
            }
        }
    }
    found
}
/// Every frame in a NightCoop session: each partner's marker anchored at its partner (VRWorldMarkerAnchor), as the
/// breach marker is -- the port lifts it to that depth in each eye instead of leaving it on the flat HUD in one place
/// for both. A partner off the canvas or behind has no marker (NightCoop's edge marker is not drawn).
pub(super) fn coop_markers_tick() {
    if !cvr_rig::vrik::nightcoop::loaded() || rt::static_num("NightCoopSdk", "InSession", &[]).is_none_or(|v| v == 0.0) {
        return;
    }
    let partners = coop_partners();
    let mut s = COOP_MARKERS.lock().unwrap_or_else(|p| p.into_inner());
    let stale = s.scanned.is_none_or(|t| t.elapsed().as_secs_f64() > 1.0)
        || s.markers.iter().any(|(m, _)| m.lock().is_null())
        || s.markers.len() < partners.len();
    if stale {
        let names: Vec<&str> = partners.iter().map(|(n, _)| n.as_str()).collect();
        s.markers = scan_coop_markers(&names);
        s.scanned = Some(std::time::Instant::now());
    }
    let mut anchored = 0usize;
    for (marker, name) in &s.markers {
        let marker = marker.lock();
        let Some((_, point)) = partners.iter().find(|(n, _)| n == name) else { continue };
        if marker.is_null() {
            continue;
        }
        let screen = native_v4("VRWorldMarkerScreenPoint", *point);
        let on_canvas = screen[3] > 0.5 && screen[0].abs() <= 1.0 && screen[1].abs() <= 1.0;
        // off the canvas or behind: not drawn (NightCoop holds it at the screen's edge, which in the headset is a
        // marker on the HUD that points nowhere)
        let anchor = [point[0], point[1], point[2], if on_canvas { 1.0 } else { -1.0 }];
        super::belt::native::<()>("VRWorldMarkerAnchor", &[Value::Handle(&marker), Value::Ptr(anchor.as_ptr().cast())]);
        anchored += on_canvas as usize;
    }
    // a line each time what is found changes (partners, markers in the HUD window, anchored on the canvas)
    let seen = (partners.len(), s.markers.len(), anchored.min(1));
    static SAID: Mutex<Option<(usize, usize, usize)>> = Mutex::new(None);
    let mut said = SAID.lock().unwrap_or_else(|p| p.into_inner());
    if *said != Some(seen) {
        *said = Some(seen);
        cvr_host::log!("[world-markers] NightCoop: {} partner(s), {} marker(s) in the HUD window, {} anchored at a partner\n", seen.0, seen.1, anchored);
    }
}

pub(super) const HOOKS: [(&str, &str, Kind, HookFn); 11] = [
    ("gameCameraSystem", "ProjectPoint", Kind::After, project_point),
    ("gameaiCyberwareBreachGameController", "OnBreachDataChanged", Kind::After, breach_changed),
    ("NpcNameplateGameController", "OnScreenProjectionUpdate", Kind::After, nameplate),
    ("BaseMappinBaseController", "GetWidgetForNameplateSlot", Kind::Override, no_nameplate_slot),
    ("StealthMappinController", "GetWidgetForNameplateSlot", Kind::Override, no_nameplate_slot),
    ("StealthMappinController", "OnNameplate", Kind::After, stealth_nameplate),
    ("QuestMappinController", "OnNameplate", Kind::After, quest_nameplate),
    ("QuickHackMappinController", "OnNameplate", Kind::After, quickhack_nameplate),
    ("DamageDigitLogicController", "OnScreenProjectionUpdate", Kind::After, digit_projection),
    ("AccumulatedDamageDigitLogicController", "OnScreenProjectionUpdate", Kind::After, digit_projection),
    ("ChatterLineLogicController", "UpdateProjection", Kind::After, chatter_projection),
];
