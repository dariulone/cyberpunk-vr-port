//! mods/cet/CyberpunkVRPort_Stereo/modules/hud_panel.lua (the live HUD layer's roots for the HUD panels): every frame
//! the HUD layer (inkHUDLayer) goes to the panels' update (VRHudPanelUpdate's work) with its virtual window. Once a
//! second its game controllers are scanned: the persistent dot's and the braindance HUD's roots are moved onto the gaze,
//! the subtitles' root keeps its crop (inkHudEntryInfo) and mainPanel, and the phone, interaction hub, vehicle, briefing,
//! scanner, camera, sniper nest, custom animation (a story identity root only) and hit marker roots -- a root named
//! "Root" (or the hit marker's) in a virtual window -- get a crop (an inkHudEntryInfo made when they have none), with
//! the padding, scanner and window-origin rules. When the plugin does the per-frame upkeep (VRHudPanelRootsNative's
//! answer) every root is handed over to it (a refused one hands over nothing); otherwise the upkeep below runs every
//! frame: a hidden root's image cleared once, the visible ones' crops measured (the scanner's real overlay bounds, the
//! window origin, the padding), the window sized to the root, both invalidated, and the subtitles' crops grown around
//! their mainPanel; the loot plate's visibility is reported with the update. A failing scan drops the roots (reported
//! once until it changes). The math is in f64, as Lua's numbers were.
use crate::script_call::{call_handle, method, method_owned, method_typed, Value};
use core::ffi::c_void;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{IType, RttiKind};
use std::sync::Mutex;

struct Phone {
    root: Handle,
    window: Handle,
    info: Handle,
    padding: f64,
    scanner: bool,
    window_origin: bool,
    interaction: bool,
    was_active: Option<bool>,
}
struct Subtitle {
    root: Handle,
    window: Handle,
    info: Handle,
    panel: Handle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    was_active: Option<bool>,
}
struct State {
    phones: Vec<Phone>,
    subtitles: Vec<Subtitle>,
    gaze: Vec<Handle>,
    scan_second: i64,
    native: bool,
    phone_error: Option<String>,
    error: Option<String>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<State> = Mutex::new(State {
    phones: Vec::new(),
    subtitles: Vec::new(),
    gaze: Vec::new(),
    scan_second: -1,
    native: false,
    phone_error: None,
    error: None,
});
static REPORTED: super::PerPlayer = super::PerPlayer::new();
/// The first scan that found roots (the HUD's controllers spawn a little after its layer).
static FOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// That scan's counts, reported with the panels' first update after it.
static FOUND_PENDING: Mutex<Option<String>> = Mutex::new(None);

/// The HUD controllers whose roots get a crop of their own.
const PHONE_CLASSES: [&str; 11] = [
    "NewHudPhoneGameController",
    "HudPhoneGameController",
    "gameuiInteractionsHubGameController",
    "gameuiPanzerHUDGameController",
    "gameuiBriefingGameController",
    "scannerDetailsGameController",
    "hudCameraController",
    "hudSniperNestController",
    "gameuiScannerGameController",
    "CustomAnimationsHudGameController",
    "TargetHitIndicatorGameController",
];

type R<T> = Result<T, String>;

fn h(object: &Handle, name: &str, values: &[Value]) -> R<Handle> {
    // SAFETY: the caller's live object, held.
    unsafe { call_handle(object.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn m<T: crate::script_call::Ret>(object: &Handle, name: &str, values: &[Value]) -> R<T> {
    // SAFETY: as above.
    unsafe { method::<T>(object.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn name_of(w: &Handle) -> R<CName> {
    m::<u64>(w, "GetName", &[]).map(CName)
}
fn is_a(object: &Handle, class: &str) -> R<bool> {
    m::<bool>(object, "IsA", &[Value::U64(CName::hashed(class).0)])
}
fn by_path(w: &Handle, name: &str) -> R<Handle> {
    h(w, "GetWidgetByPathName", &[Value::U64(CName::hashed(name).0)])
}
fn shown(w: &Handle) -> R<bool> {
    Ok(m::<bool>(w, "IsVisible", &[])? && m::<f32>(w, "GetOpacity", &[])? > 0.0)
}
fn invalidate(w: &Handle) -> R<()> {
    m::<()>(w, "FlagForVisualInvalidation", &[])
}
fn v2(v: [f32; 2]) -> [f64; 2] {
    [v[0] as f64, v[1] as f64]
}
fn child_position(parent: &Handle, child: &Handle) -> R<[f64; 2]> {
    m::<[f32; 2]>(parent, "GetChildPosition", &[Value::Handle(child)]).map(v2)
}
fn child_size(parent: &Handle, child: &Handle) -> R<[f64; 2]> {
    m::<[f32; 2]>(parent, "GetChildSize", &[Value::Handle(child)]).map(v2)
}
/// An inkHudEntryInfo's Vector2 field (offset, size), in place.
fn info_field(info: &Handle, name: &str) -> R<*mut [f32; 2]> {
    // SAFETY: the info, held; its native struct property.
    unsafe { crate::script_call::property_of_kind(info.instance, name, RttiKind::Class as u8) }
        .map(|p| p as *mut [f32; 2])
        .ok_or_else(|| format!("no inkHudEntryInfo {name}"))
}
fn read_field(info: &Handle, name: &str) -> R<[f64; 2]> {
    // SAFETY: the field just found.
    Ok(v2(unsafe { *info_field(info, name)? }))
}
fn write_field(info: &Handle, name: &str, value: [f64; 2]) -> R<()> {
    // SAFETY: as above.
    unsafe { *info_field(info, name)? = [value[0] as f32, value[1] as f32] };
    Ok(())
}

/// The mod's exactPath(): each name a child of the one before, its name checked (a failed lookup can return the root).
fn exact_path(root: &Handle, names: &[&str]) -> R<Option<Handle>> {
    let mut widget = root.clone();
    for name in names {
        widget = by_path(&widget, name)?;
        if widget.is_null() || name_of(&widget)? != CName::hashed(name) {
            return Ok(None);
        }
    }
    Ok(Some(widget))
}
/// The mod's identityRoot(): a story identity card's root.
fn identity_root(root: &Handle) -> R<bool> {
    Ok(exact_path(root, &["SC_01", "CARD_00_info"])?.is_some()
        || exact_path(root, &["Main", "Pane1", "right", "right_panel", "text_up", "imprint_text"])?.is_some())
}

/// `layer:GetGameControllers()`: the weak references locked.
pub(super) fn game_controllers(layer: &Handle) -> R<Vec<Handle>> {
    // SAFETY: the layer's wrapper, held; the array result owned until it is read, its elements by their own type.
    unsafe {
        let array = method_owned(layer.instance, "GetGameControllers", &[]).ok_or("GetGameControllers failed")?;
        let class = red4ext_abi::rtti::object_class(layer.instance).ok_or("the layer has no class")?;
        let f = class.function(CName::hashed("GetGameControllers")).ok_or("no GetGameControllers")?;
        let ty: &IType = &*f.return_type.as_ref().ok_or("no result")?.ty;
        let inner = ty.array_inner().as_ref().ok_or("no element type")?;
        let weak = inner.kind() == RttiKind::WeakHandle as u8;
        if !weak && inner.kind() != RttiKind::Handle as u8 {
            return Err("the controllers are no references".into());
        }
        let storage = array.as_ptr() as *mut c_void;
        let mut out = Vec::new();
        for i in 0..ty.array_length(storage) {
            let element = ty.array_element(storage, i);
            if element.is_null() {
                continue;
            }
            out.push(if weak { (*(element as *const WeakHandle)).lock() } else { (*(element as *const Handle)).clone() });
        }
        Ok(out)
    }
}

/// The mod's handOver(): every root to the plugin; true when it took them all (else it is handed nothing).
fn hand_over(s: &State) -> R<bool> {
    use crate::hud_natives::{panel_root, panel_roots_begin, panel_subtitle};
    panel_roots_begin();
    let mut all = true;
    for root in &s.gaze {
        let parent = h(root, "GetParentWidget", &[])?;
        all = panel_root(root.clone(), parent, Handle::null(), 8, 0.0) && all;
    }
    for phone in &s.phones {
        let flags = phone.scanner as i32 + 2 * phone.window_origin as i32 + 4 * phone.interaction as i32;
        all = panel_root(phone.root.clone(), phone.window.clone(), phone.info.clone(), flags, phone.padding as f32) && all;
    }
    for subtitle in &s.subtitles {
        all = panel_subtitle(subtitle.root.clone(), subtitle.window.clone(), subtitle.info.clone(), subtitle.panel.clone()) && all;
    }
    if !all {
        panel_roots_begin();
    }
    Ok(all)
}

/// The once-a-second scan of the layer's controllers.
fn scan(s: &mut State, layer: &Handle) -> R<()> {
    s.phones.clear();
    s.gaze.clear();
    s.subtitles.clear();
    for controller in game_controllers(layer)? {
        if controller.is_null() {
            continue;
        }
        let class = CName(m::<u64>(&controller, "GetClassName", &[])?);
        super::hud_hints::found(class, &controller);
        if class == CName::hashed("CrosshairGameControllerPersistentDot") || class == CName::hashed("BraindanceGameController") {
            // Laid out about the screen's middle, as the dot is: both are moved onto the gaze (a missing root is no
            // entry, as table.insert of nil).
            let root = h(&controller, "GetRootWidget", &[])?;
            if !root.is_null() {
                s.gaze.push(root);
            }
        } else if class == CName::hashed("SubtitlesGameController") {
            let root = h(&controller, "GetRootWidget", &[])?;
            if root.is_null() {
                continue;
            }
            let window = h(&root, "GetParentWidget", &[])?;
            let info = h(&root, "GetUserData", &[Value::U64(CName::hashed("inkHudEntryInfo").0)])?;
            let panel = by_path(&root, "mainPanel")?;
            if !info.is_null() && !panel.is_null() && !window.is_null() && is_a(&window, "inkVirtualWindow")? {
                // The crop as the info holds it now. (The mod meant to keep a known root's earlier bounds, but CET's
                // references have no equality -- each call hands out a new wrapper -- so its `old.root == root` never
                // held: the info's current crop is what it always used.)
                let (offset, size) = (read_field(&info, "offset")?, read_field(&info, "size")?);
                let b = [offset[0], offset[1], size[0], size[1]];
                s.subtitles.push(Subtitle { root, window, info, panel, x: b[0], y: b[1], width: b[2], height: b[3], was_active: None });
            }
        } else if let Some(&name) = PHONE_CLASSES.iter().find(|&&c| CName::hashed(c) == class) {
            let mut root = h(&controller, "GetRootWidget", &[])?;
            let mut window = if root.is_null() { Handle::null() } else { h(&root, "GetParentWidget", &[])? };
            // The sniper HUD is a child of the shared briefing entry's root: that existing window is captured.
            if name == "hudSniperNestController" && !window.is_null() && name_of(&window)? == CName::hashed("Root") {
                root = window;
                window = h(&root, "GetParentWidget", &[])?;
            }
            // Hit and kill markers: a 1x1 root centred with anchorPoint (0,0), the markers spawned around it.
            let hit_marker = name == "TargetHitIndicatorGameController";
            if !root.is_null()
                && (hit_marker || name_of(&root)? == CName::hashed("Root"))
                && !window.is_null()
                && is_a(&window, "inkVirtualWindow")?
                && (name != "CustomAnimationsHudGameController" || identity_root(&root)?)
            {
                let mut info = h(&root, "GetUserData", &[Value::U64(CName::hashed("inkHudEntryInfo").0)])?;
                if info.is_null() {
                    let class = red4ext_abi::rtti::RttiSystem::get()
                        .and_then(|r| r.get_class(CName::hashed("inkHudEntryInfo")))
                        .ok_or("no inkHudEntryInfo class")?;
                    // SAFETY: a new instance owned by the handle made for it.
                    info = unsafe { Handle::from_instance(class.create_instance()) };
                    write_field(&info, "offset", [0.0, 0.0])?;
                    m::<()>(&root, "SetUserData", &[Value::Handle(&info)])?;
                }
                s.phones.push(Phone {
                    root,
                    window,
                    info,
                    padding: if name == "CustomAnimationsHudGameController" {
                        32.0
                    } else if hit_marker {
                        160.0
                    } else {
                        0.0
                    },
                    scanner: name == "gameuiScannerGameController",
                    window_origin: name == "scannerDetailsGameController" || name == "hudSniperNestController" || hit_marker,
                    interaction: name == "gameuiInteractionsHubGameController",
                    was_active: None,
                });
            }
        }
    }
    let was_native = s.native;
    s.native = crate::hud_natives::panel_roots_native();
    if s.native {
        s.native = hand_over(s).unwrap_or(false);
    } else if was_native {
        crate::hud_natives::panel_roots_begin();
    }
    let found = s.gaze.len() + s.phones.len() + s.subtitles.len();
    if found > 0 && !FOUND.swap(true, std::sync::atomic::Ordering::Relaxed) {
        *FOUND_PENDING.lock().unwrap_or_else(|p| p.into_inner()) = Some(format!(
            "gaze={} phones={} subtitles={} native={}",
            s.gaze.len(),
            s.phones.len(),
            s.subtitles.len(),
            s.native
        ));
    }
    if REPORTED.first() {
        log!(
            "[mechanics] hud panel: scanned from Rust (gaze={} phones={} subtitles={} native={})\n",
            s.gaze.len(),
            s.phones.len(),
            s.subtitles.len(),
            s.native
        );
    }
    Ok(())
}

/// The script's upkeep of one phone root (the plugin's when it does it); true when its loot plate shows.
fn upkeep_phone(phone: &mut Phone) -> R<bool> {
    let active = shown(&phone.root)?;
    if !active && phone.was_active != Some(false) {
        // The last visible image cleared once on hide, a new source's included.
        invalidate(&phone.root)?;
        invalidate(&phone.window)?;
    }
    phone.was_active = Some(active);
    if !active {
        return Ok(false);
    }
    let mut loot_visible = false;
    if phone.interaction {
        // The loot plate and tooltip stay together in their native window; the two single-name children resolved
        // explicitly (a slash path can return the root).
        let top = by_path(&phone.root, "topWidgets")?;
        if !top.is_null() && name_of(&top)? == CName::hashed("topWidgets") {
            let loot = by_path(&top, "looting")?;
            loot_visible = !loot.is_null() && name_of(&loot)? == CName::hashed("looting") && shown(&loot)? && shown(&top)?;
        }
    }
    let size = m::<[f32; 2]>(&phone.root, "GetSize", &[]).map(v2)?;
    if size[0] > 0.0 && size[1] > 0.0 && size[0] <= 16384.0 && size[1] <= 16384.0 {
        let bounds = read_field(&phone.info, "size")?;
        let padding = phone.padding;
        let (mut x, mut y) = (-padding, -padding);
        let (mut width, mut height) = (size[0] + padding * 2.0, size[1] + padding * 2.0);
        if phone.window_origin {
            // The root's offset within its source window is part of the crop.
            let origin = child_position(&phone.window, &phone.root)?;
            if origin[0].abs() < 16384.0 && origin[1].abs() < 16384.0 {
                (x, y) = (origin[0] - padding, origin[1] - padding);
            } else {
                [x, y] = read_field(&phone.info, "offset")?;
            }
        }
        if phone.scanner {
            // The scanner's 1920x1080 root holds a 3840x2160 overlay at (-960,-540): its real bounds captured.
            let border = exact_path(&phone.root, &["border"])?;
            let panel = exact_path(&phone.root, &["border", "scanner_overlay"])?;
            let measured = match (&border, &panel) {
                (Some(border), Some(panel)) => Some((
                    child_position(&phone.root, border)?,
                    child_position(border, panel)?,
                    child_size(border, panel)?,
                )),
                _ => None,
            };
            let origin = child_position(&phone.window, &phone.root)?;
            match measured {
                Some((a, b, sz))
                    if sz[0] > 0.0
                        && sz[1] > 0.0
                        && sz[0] < 16384.0
                        && sz[1] < 16384.0
                        && (origin[0] + a[0] + b[0]).abs() < 16384.0
                        && (origin[1] + a[1] + b[1]).abs() < 16384.0 =>
                {
                    let (px, py) = (origin[0] + a[0] + b[0], origin[1] + a[1] + b[1]);
                    x = origin[0].min((px - 32.0).floor());
                    y = origin[1].min((py - 32.0).floor());
                    width = (origin[0] + size[0]).max((px + sz[0] + 32.0).ceil()) - x;
                    height = (origin[1] + size[1]).max((py + sz[1] + 32.0).ceil()) - y;
                }
                _ if bounds[0] > 0.0 && bounds[1] > 0.0 => {
                    [x, y] = read_field(&phone.info, "offset")?;
                    (width, height) = (bounds[0], bounds[1]);
                }
                _ => {}
            }
        }
        if bounds[0] != width || bounds[1] != height {
            write_field(&phone.info, "size", [width, height])?;
            invalidate(&phone.root)?;
        }
        let offset = read_field(&phone.info, "offset")?;
        if (padding > 0.0 || phone.scanner || phone.window_origin) && (offset[0] != x || offset[1] != y) {
            write_field(&phone.info, "offset", [x, y])?;
            invalidate(&phone.root)?;
        }
        let window_size = m::<[f32; 2]>(&phone.window, "GetSize", &[]).map(v2)?;
        if window_size[0] != size[0] || window_size[1] != size[1] {
            let v = [size[0] as f32, size[1] as f32];
            // SAFETY: the window, held.
            unsafe { method_typed::<()>(phone.window.instance, "SetSize", &["Vector2"], &[Value::Ptr(v.as_ptr().cast())]) }
                .ok_or("SetSize failed")?;
        }
        // Fullscreen controllers cache their draw data differently from authored HUD tiles.
        invalidate(&phone.root)?;
        invalidate(&phone.window)?;
    }
    Ok(loot_visible)
}

/// The script's upkeep of one subtitle root: its crop grown around mainPanel (laid out above the 100px root).
fn upkeep_subtitle(subtitle: &mut Subtitle) -> R<()> {
    let active = shown(&subtitle.root)?;
    if !active && subtitle.was_active != Some(false) {
        invalidate(&subtitle.root)?;
        invalidate(&subtitle.window)?;
    }
    subtitle.was_active = Some(active);
    if !active {
        return Ok(());
    }
    let p = child_position(&subtitle.root, &subtitle.panel)?;
    let sz = child_size(&subtitle.root, &subtitle.panel)?;
    if sz[0] > 0.0 && sz[1] > 0.0 && sz[0] < 16384.0 && sz[1] < 16384.0 && p[0].abs() < 16384.0 && p[1].abs() < 16384.0 {
        let x = subtitle.x.min((p[0] - 32.0).floor());
        let y = subtitle.y.min((p[1] - 32.0).floor());
        let width = subtitle.width.max((p[0] + sz[0] + 32.0 - x).ceil());
        let height = subtitle.height.max((p[1] + sz[1] + 32.0 - y).ceil());
        let (offset, bounds) = (read_field(&subtitle.info, "offset")?, read_field(&subtitle.info, "size")?);
        if offset[0] != x || offset[1] != y || bounds[0] != width || bounds[1] != height {
            write_field(&subtitle.info, "offset", [x, y])?;
            write_field(&subtitle.info, "size", [width, height])?;
        }
        invalidate(&subtitle.root)?;
        invalidate(&subtitle.window)?;
    }
    Ok(())
}

/// The mod's updatePhoneBounds(): the scan once a second, the script's upkeep when the plugin does not do it; the loot
/// plate's visibility.
fn update_phone_bounds(s: &mut State, layer: &Handle) -> R<bool> {
    if layer.is_null() {
        s.phones.clear();
        s.subtitles.clear();
        s.gaze.clear();
        super::hud_hints::reset();
        s.scan_second = -1;
        if s.native {
            let _ = hand_over(s);
        }
        return Ok(false);
    }
    let second = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    if second != s.scan_second {
        s.scan_second = second;
        scan(s, layer)?;
    }
    if s.native {
        return Ok(false);
    }
    let mut loot_visible = false;
    for phone in &mut s.phones {
        loot_visible = upkeep_phone(phone)? || loot_visible;
    }
    for subtitle in &mut s.subtitles {
        upkeep_subtitle(subtitle)?;
    }
    Ok(loot_visible)
}

/// The mod's tickHud(): the scan and upkeep, then the panels' update; the number of sprites.
fn tick_hud(s: &mut State) -> R<i32> {
    let layer = crate::engine_objects::layer_wrapper(CName::hashed("inkHUDLayer").0);
    let loot_visible = match update_phone_bounds(s, &layer) {
        Ok(visible) => {
            s.phone_error = None;
            visible
        }
        Err(e) => {
            if s.phone_error.as_deref() != Some(e.as_str()) {
                log!("[Stereo.HUD phone] {e}\n");
                s.phone_error = Some(e);
            }
            s.phones.clear();
            s.subtitles.clear();
            s.scan_second = -1;
            false
        }
    };
    let window = if layer.is_null() { Handle::null() } else { h(&layer, "GetVirtualWindow", &[])? };
    // SAFETY: the window held for the call, on the main thread.
    let sprites = unsafe { crate::hud_natives::panel_update(window, loot_visible) };
    if let Some(found) = FOUND_PENDING.lock().unwrap_or_else(|p| p.into_inner()).take() {
        log!("[mechanics] hud panel: first roots found from Rust ({found}), the panels' update answered {sprites}\n");
    }
    Ok(sprites)
}

/// The mod's tick: every frame.
pub(in crate::mechanics) fn tick() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    match tick_hud(&mut s) {
        Ok(_) => s.error = None,
        Err(e) => {
            if s.error.as_deref() != Some(e.as_str()) {
                log!("[Stereo.HUD] {e}\n");
                s.error = Some(e);
            }
        }
    }
}

/// The mod's shutdown: the panels' update with nothing.
pub(in crate::mechanics) fn shutdown() {
    // SAFETY: a null window.
    unsafe { crate::hud_natives::panel_update(Handle::null(), false) };
}
