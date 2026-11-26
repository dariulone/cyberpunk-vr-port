//! mods/redscript/CyberpunkVRPort_ScannerHud: the scanner's layout and its editor.
//!
//! Seven movable pieces -- 0 the scanner frame (scannerGameController), 1 the details panel
//! (scannerDetailsGameController), 2 the quickhack list (QuickhacksListGameController), 3 the hint line
//! (ScannerHintInkGameController), and three children of the quickhack panel: 4 top_panel (the cyberdeck's memory),
//! 5 left_panel (the script list), 6 right_panel (the description). Each controller's root is registered as it
//! initialises and again on the hooks that rebuild it, and gets the plugin's slot (VRScannerSlotGet: x, y, scale) as a
//! render transform about its middle, the scale halved in a braindance; a slot scale of 1 leaves the authored one.
//! The quickhack panel's key hints (input_container) and the details panel's tab hints are hidden. The selected
//! script's status chips move from its row under the description's name, coloured as the row coloured them.
//!
//! The editor while scanning: Right Shift (VRPortScannerEditor, r6/input) tapped enters it or steps to the next piece
//! (the others dim to 15 %), held leaves and saves; the mouse drags, W A S D nudge a pixel, the wheel scales by a
//! tenth (0.1 .. 5.0); the game is paused while it is up. The script's editor was a listener method added to the
//! details panel; here the listener is the quickhack panel, whose own OnAction (hooked) hands the editor's actions over
//! and stays mute while the editor is up, as the script's wrapper kept it. The layout is put back 0.35 s and 1.2 s
//! after the panel opens, changes tab, a scan starts or the editor closes (the script's VRPortScannerReapply event).
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::{Owned, Value};
use crate::script_hooks::Call;
use cvr_host::log;
use red4ext_abi::cname::CName;
use core::ffi::c_void;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::sync::Mutex;

const PIECES: usize = 7;
const EDITOR: &str = "VRPortScannerEditor";
/// The editor's own while it is up, consumed: the mouse is the camera in gameplay and W A S D the legs.
const OWNED: [&str; 12] = [
    "CameraMouseX", "CameraMouseY", "mouse_x", "mouse_y", "VRPortScannerMoveX", "VRPortScannerMoveY", "VRPortScannerScaleUp",
    "VRPortScannerScaleDown", "Forward", "Back", "Left", "Right",
];
const NAMES: [&str; PIECES] =
    ["SCANNER FRAME", "DETAILS PANEL", "QUICKHACK LIST", "HINT LINE", "CYBERDECK MEMORY", "SCRIPT LIST", "SCRIPT DESCRIPTION"];
const REAPPLY: [f64; 2] = [0.35, 1.20];
const CYAN: [f64; 3] = [0.369, 0.965, 1.000];
const RED: [f64; 3] = [1.176, 0.381, 0.348];
const YELLOW: [f64; 3] = [1.119, 0.844, 0.257];

#[derive(Default)]
struct Scan {
    roots: [Option<WeakHandle>; PIECES],
    /// The quickhack panel: the editor's input listener.
    panel: Option<WeakHandle>,
    editing: bool,
    listening: bool,
    active: usize,
    /// How many of each mouse axis arrived since the editor opened: ours x / y, the camera's, the UI's.
    hits: [i32; 6],
    /// The moved chips and the row they came from.
    chips: Option<(WeakHandle, WeakHandle)>,
    /// Seconds to each pending re-apply.
    reapply: Vec<f64>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread (the script hooks, the mechanics' tick).
unsafe impl Send for Scan {}
static SCAN: Mutex<Option<Scan>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Scan) -> T) -> T {
    let mut g = SCAN.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(Scan::default))
}
fn lock(w: &Option<WeakHandle>) -> Handle {
    w.as_ref().map_or_else(Handle::null, WeakHandle::lock)
}
fn root(idx: usize) -> Handle {
    with(|s| lock(&s.roots[idx]))
}
fn name_hash(n: &str) -> u64 {
    CName::hashed(n).0
}
fn widget_name(w: &Handle) -> u64 {
    rt::call(w, "GetName", &[]).map_or(0, |o| {
        // SAFETY: GetName's CName result.
        unsafe { *(o.as_ptr() as *const u64) }
    })
}
/// GetPlayerControlledObject as a PlayerPuppet (null for anything else).
fn player_of(ctrl: &Handle) -> Handle {
    let p = rt::obj(ctrl, "GetPlayerControlledObject", &[]);
    if super::menu_rt::is_a(&p, "PlayerPuppet") {
        p
    } else {
        Handle::null()
    }
}
fn slot(idx: usize, comp: i32) -> f64 {
    super::belt::native::<f32>("VRScannerSlotGet", &[Value::Int32(idx as i32), Value::Int32(comp)]).map_or(0.0, f64::from)
}

/// VRPortScannerApply: the slot's translation and scale (halved in a braindance) about the piece's middle.
fn apply(idx: usize, w: &Handle) {
    if w.is_null() {
        return;
    }
    let mut scale = slot(idx, 2);
    // zero is what an unregistered native returns: it would erase the piece
    if scale <= 0.01 {
        scale = 1.0;
    }
    if super::braindance_hud::in_braindance() {
        scale *= 0.5;
    }
    ink::v2(w, "SetRenderTransformPivot", 0.5, 0.5);
    ink::v2(w, "SetTranslation", slot(idx, 0), slot(idx, 1));
    // a slot scale of 1 means "do not touch it": the quickhack panel is halved in its asset
    if (scale - 1.0).abs() > 0.001 {
        ink::v2(w, "SetScale", scale, scale);
    }
}
fn apply_all() {
    for idx in 0..PIECES {
        apply(idx, &root(idx));
    }
}
/// VRPortScannerRegister: the piece's root kept (when its owner is the player) and laid out.
fn register(idx: usize, owner: &Handle, w: &Handle) {
    if owner.is_null() || w.is_null() {
        return;
    }
    with(|s| s.roots[idx] = Some(WeakHandle::from_handle(w)));
    apply(idx, w);
}
fn register_own_root(idx: usize, ctrl: &Handle) {
    register(idx, &player_of(ctrl), &rt::obj(ctrl, "GetRootWidget", &[]));
}
/// VRPortScannerRegisterPanels: the quickhack panel's three containers, its key hints hidden.
fn register_panels(ctrl: &Handle) {
    let r = rt::obj(ctrl, "GetRootCompoundWidget", &[]);
    if r.is_null() {
        return;
    }
    let owner = player_of(ctrl);
    let child = |n: &str| rt::obj(&r, "GetWidget", &[A::Name(name_hash(n))]);
    register(4, &owner, &child("top_panel"));
    register(5, &owner, &child("left_panel"));
    register(6, &owner, &child("right_panel"));
    let hints = child("input_container");
    if !hints.is_null() {
        ink::visible(&hints, false);
    }
}
fn schedule_reapply() {
    with(|s| s.reapply.extend(REAPPLY));
}

// ---------------------------------------------------------------- the pieces register themselves
fn frame_root(call: &mut Call) {
    register_own_root(0, &call.this());
}
fn details_init(call: &mut Call) {
    let c = call.this();
    register_own_root(1, &c);
    // the scanner's key hints name keys a headset does not have: both halves hidden
    for f in ["m_changeTabButtonHints", "m_changeTabInlineHint"] {
        if let Ok(w) = super::world_touch::widget_ref(&c, f) {
            if !w.is_null() {
                ink::visible(&w, false);
            }
        }
    }
}
fn panel_init(call: &mut Call) {
    let c = call.this();
    with(|s| s.panel = Some(WeakHandle::from_handle(&c)));
    register_own_root(2, &c);
    register_panels(&c);
}
fn hint_root(call: &mut Call) {
    register_own_root(3, &call.this());
}
fn reapply_later(_call: &mut Call) {
    schedule_reapply();
}

// ---------------------------------------------------------------- the selected script's chips
fn find_by_name(w: &Handle, want: u64, depth: u32) -> Handle {
    if w.is_null() || depth > 12 {
        return Handle::null();
    }
    if widget_name(w) == want {
        return w.clone();
    }
    if !super::menu_rt::is_a(w, "inkCompoundWidget") {
        return Handle::null();
    }
    let n = rt::num(w, "GetNumChildren", &[]).unwrap_or(0.0) as i64;
    for i in 0..n {
        let found = find_by_name(&rt::obj(w, "GetWidgetByIndex", &[A::N(i as f64)]), want, depth + 1);
        if !found.is_null() {
            return found;
        }
    }
    Handle::null()
}
fn children(w: &Handle) -> Vec<Handle> {
    let n = rt::num(w, "GetNumChildren", &[]).unwrap_or(0.0) as i64;
    (0..n).map(|i| rt::obj(w, "GetWidgetByIndex", &[A::N(i as f64)])).collect()
}
/// VRPortRowChips: the row's text_container child that is not the name canvas.
fn row_chips(row: &Handle) -> Handle {
    let host = find_by_name(row, name_hash("text_container"), 0);
    if !super::menu_rt::is_a(&host, "inkCompoundWidget") {
        return Handle::null();
    }
    let canvas = name_hash("qh_name_canvas");
    children(&host).into_iter().find(|c| !c.is_null() && widget_name(c) != canvas).unwrap_or_else(Handle::null)
}
/// VRPortTintTexts: the texts and the chip's frame (fg_Status and its ghosts) in one colour; bg_Status left alone.
fn tint_texts(w: &Handle, c: [f64; 3], depth: u32) {
    if w.is_null() || depth > 6 {
        return;
    }
    let frame = ["fg_Status", "fg_Status_ghost", "fg_Status_ghost2", "fg_Status_ghost3"].map(name_hash);
    if super::menu_rt::is_a(w, "inkText") || frame.contains(&widget_name(w)) {
        ink::tint(w, c);
    }
    if super::menu_rt::is_a(w, "inkCompoundWidget") {
        for ch in children(w) {
            tint_texts(&ch, c, depth + 1);
        }
    }
}
/// VRPortColourChips: each chip its authored colour back (a style resolves along the parent chain, gone once moved).
fn colour_chips(chips: &Handle, ready: bool) {
    if !super::menu_rt::is_a(chips, "inkCompoundWidget") {
        return;
    }
    for chip in children(chips) {
        if chip.is_null() {
            continue;
        }
        let n = widget_name(&chip);
        if n == name_hash("status_container") {
            tint_texts(&chip, if ready { CYAN } else { RED }, 0);
        } else if n == name_hash("reveal_container") {
            tint_texts(&chip, YELLOW, 0);
        } else if n == name_hash("cooldown_panel") {
            tint_texts(&chip, RED, 0);
            let icon = find_by_name(&chip, name_hash("icon"), 0);
            if !icon.is_null() {
                ink::tint(&icon, RED);
            }
        } else {
            tint_texts(&chip, CYAN, 0);
        }
    }
}
fn item_selected(call: &mut Call) {
    let c = call.this();
    let item = call.arg_handle(1);
    let r = rt::obj(&c, "GetRootWidget", &[]);
    let data = rt::field(&c, "m_selectedData").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if r.is_null() || data.is_null() || item.is_null() {
        return;
    }
    let block = find_by_name(&r, name_hash("description_text_container"), 0);
    if !super::menu_rt::is_a(&block, "inkCompoundWidget") {
        return;
    }
    ink::visible(&block, true);
    let label = find_by_name(&block, name_hash("name_tooltip"), 0);
    if super::menu_rt::is_a(&label, "inkText") {
        ink::visible(&label, true);
        let title = rt::field(&data, "m_title").map(|v| v.text()).unwrap_or_default();
        ink::set_text(&label, &super::menu_rt::loc(&title));
    }
    // the previous row gets its chips back before this row hands over its own
    if let Some((chips, home)) = with(|s| s.chips.take()) {
        let (chips, home) = (chips.lock(), home.lock());
        if !chips.is_null() && !home.is_null() {
            ink::reparent(&chips, &home);
        }
    }
    let row = rt::obj(&item, "GetRootWidget", &[]);
    let chips = row_chips(&row);
    let home = find_by_name(&row, name_hash("text_container"), 0);
    if chips.is_null() || !super::menu_rt::is_a(&home, "inkCompoundWidget") {
        return;
    }
    with(|s| s.chips = Some((WeakHandle::from_handle(&chips), WeakHandle::from_handle(&home))));
    ink::reparent(&chips, &block);
    ink::visible(&chips, true);
    // grouped under the name, from the centre (the block is a vertical panel)
    rt::call(&chips, "SetHAlign", &[A::N(rt::enum_value("inkEHorizontalAlign", "Center"))]);
    rt::call(&chips, "SetVAlign", &[A::N(rt::enum_value("inkEVerticalAlign", "Top"))]);
    ink::move_to(&chips, 0.0, 6.0);
    let state = rt::field(&data, "m_actionState").and_then(|v| v.num()).unwrap_or(-1.0);
    colour_chips(&chips, state == rt::enum_value("EActionInactivityReson", "Ready"));
}
fn panel_target(call: &mut Call) {
    let c = call.this();
    register_own_root(2, &c);
    register_panels(&c);
}

// ---------------------------------------------------------------- the editor
fn highlight(editing: bool) {
    let active = with(|s| s.active);
    for idx in 0..PIECES {
        let r = root(idx);
        if !r.is_null() {
            ink::opacity(&r, if !editing || idx == active { 1.0 } else { 0.15 });
        }
    }
}
fn listen(panel: &Handle, names: &[&str], on: bool) {
    let player = rt::obj(panel, "GetPlayerControlledObject", &[]);
    if player.is_null() {
        return;
    }
    for n in names {
        rt::call(&player, if on { "RegisterInputListener" } else { "UnregisterInputListener" }, &[A::H(panel), A::Name(name_hash(n))]);
    }
}
/// VRPortEditorSay: a line through UI_Notifications.OnscreenMessage, the game's own notification.
fn say(text: &str) {
    let shown = (|| -> Result<(), String> {
        let defs = super::boards::defs()?;
        let def = super::boards::definition(&defs, "UI_Notifications")?.clone();
        let board = super::boards::system_board(&def)?;
        if board.is_null() {
            return Err("no board".into());
        }
        let id = super::boards::field(&def, "OnscreenMessage")?;
        let rtti = red4ext_abi::rtti::RttiSystem::get().ok_or("no RTTI")?;
        // redscript's SimpleScreenMessage is the engine's gameSimpleScreenMessage
        let ty = ["gameSimpleScreenMessage", "SimpleScreenMessage"]
            .iter()
            .find_map(|n| rtti.get_type(CName::hashed(n)))
            .ok_or("no SimpleScreenMessage")?;
        let mut msg = Owned::of_type(ty).ok_or("no message")?;
        // SAFETY: the constructed struct's own properties.
        unsafe {
            let class = &*(ty as *const red4ext_abi::rtti::IType as *const red4ext_abi::rtti::Class);
            let base = msg.as_mut_ptr().cast::<core::ffi::c_void>();
            let at = |n: &str| class.property_named(n).map(|p| p.value_ptr::<core::ffi::c_void>(base));
            if let Some(p) = at("isShown") {
                *(p as *mut bool) = true;
            }
            if let Some(p) = at("duration") {
                *(p as *mut f32) = 2.0;
            }
            if let Some(p) = at("message") {
                red4ext_abi::cstring::CString::assign(p.cast(), text);
            }
        }
        // ToVariant: the type, then the value behind a pointer (too big to be held inline); the board copies it, and
        // these bytes are never destroyed as a Variant
        let var: [usize; 3] = [ty as *const red4ext_abi::rtti::IType as usize, msg.as_ptr() as usize, 0];
        // SAFETY: the board, held; the id in place in its definition; the variant over our message for the call.
        unsafe { crate::script_call::method::<()>(board.instance, "SetVariant", &[Value::Ptr(id), Value::Ptr(var.as_ptr().cast()), Value::Bool(true)]) }
            .ok_or("SetVariant failed")?;
        Ok(())
    })();
    if let Err(e) = shown {
        log!("[scanner-hud] on-screen line not shown ({e}): {text} (from Rust)\n");
    }
}
fn pause(panel: &Handle, on: bool) {
    let handler = rt::obj(panel, "GetSystemRequestsHandler", &[]);
    if !handler.is_null() {
        rt::call(&handler, if on { "PauseGame" } else { "UnpauseGame" }, &[]);
    }
}
fn editor_start(panel: &Handle) {
    with(|s| {
        s.editing = true;
        s.active = 0;
    });
    listen(panel, &OWNED, true);
    pause(panel, true);
}
fn editor_stop(panel: &Handle, save: bool) {
    if !with(|s| std::mem::replace(&mut s.editing, false)) {
        return;
    }
    listen(panel, &OWNED, false);
    highlight(false);
    // unpaused last, so nothing the game does on resuming lands while the editor still owns input
    pause(panel, false);
    if save {
        super::belt::native::<i32>("VRScannerSlotSave", &[]);
        say("Scanner layout saved");
    }
    schedule_reapply();
}
/// The editor's OnAction (the script's on the details panel): Some(result) when it took the action.
fn editor_action(panel: &Handle, action: *const c_void, consumer: *const c_void, name: u64) -> Option<bool> {
    let la = |f: &str| rt::listener_static(false, f, action).map(|o| Val::of(&o));
    if name == name_hash(EDITOR) {
        let ty = la("GetType").and_then(|v| v.num()).unwrap_or(-1.0);
        // held leaves and saves, tapped enters or steps to the next piece
        if ty == rt::enum_value("gameinputActionType", "BUTTON_HOLD_COMPLETE") {
            editor_stop(panel, true);
            return Some(true);
        }
        if ty == rt::enum_value("gameinputActionType", "BUTTON_RELEASED") {
            if with(|s| s.editing) {
                with(|s| s.active = if s.active >= PIECES - 1 { 0 } else { s.active + 1 });
            } else {
                with(|s| s.hits = [0; 6]);
                editor_start(panel);
            }
            highlight(true);
            let (active, h) = with(|s| (s.active, s.hits));
            say(&format!("{}   own {}/{}   cam {}/{}   ui {}/{}", NAMES[active], h[0], h[1], h[2], h[3], h[4], h[5]));
        }
        return Some(true);
    }
    if !with(|s| s.editing) || !OWNED.iter().any(|n| name_hash(n) == name) {
        return None;
    }
    rt::listener_static(true, "Consume", consumer);
    let idx = with(|s| s.active);
    let (mut x, mut y, mut scale) = (slot(idx, 0), slot(idx, 1), slot(idx, 2));
    if scale <= 0.01 {
        scale = 1.0;
    }
    let value = || la("GetValue").and_then(|v| v.num()).unwrap_or(0.0) * 0.60;
    let pressed = || la("IsButtonJustPressed").is_some_and(|v| v.bool());
    let is = |n: &str| name == name_hash(n);
    let mut changed = false;
    with(|s| {
        // ours first; the camera's and the UI's names only while ours never fired, so no motion counts twice
        let h = &mut s.hits;
        if is("VRPortScannerMoveX") {
            h[0] += 1;
            x += value();
            changed = true;
        }
        if is("VRPortScannerMoveY") {
            h[1] += 1;
            y -= value();
            changed = true;
        }
        if is("CameraMouseX") {
            h[2] += 1;
            if h[0] == 0 {
                x += value();
                changed = true;
            }
        }
        if is("CameraMouseY") {
            h[3] += 1;
            if h[1] == 0 {
                y -= value();
                changed = true;
            }
        }
        if is("mouse_x") {
            h[4] += 1;
            if h[2] == 0 && h[0] == 0 {
                x += value();
                changed = true;
            }
        }
        if is("mouse_y") {
            h[5] += 1;
            if h[3] == 0 && h[1] == 0 {
                y -= value();
                changed = true;
            }
        }
    });
    for (n, dx, dy) in [("Forward", 0.0, -1.0), ("Back", 0.0, 1.0), ("Left", -1.0, 0.0), ("Right", 1.0, 0.0)] {
        if is(n) && pressed() {
            x += dx;
            y += dy;
            changed = true;
        }
    }
    // one notch, one tenth
    if is("VRPortScannerScaleUp") && pressed() {
        scale += 0.10;
        changed = true;
    }
    if is("VRPortScannerScaleDown") && pressed() {
        scale -= 0.10;
        changed = true;
    }
    let scale = scale.clamp(0.10, 5.00);
    if !changed {
        return Some(false);
    }
    super::belt::native::<i32>("VRScannerSlotSet", &[Value::Int32(idx as i32), Value::Float(x as f32), Value::Float(y as f32), Value::Float(scale as f32)]);
    apply(idx, &root(idx));
    Some(true)
}
/// QuickhacksListGameController.OnAction: the editor's actions to the editor; the panel mute while it is up.
fn panel_action(call: &mut Call) {
    // ListenerAction / ListenerActionConsumer are structs passed in place (as handles they read as null)
    let (action, consumer) = (rt::listener_arg(call, 0), rt::listener_arg(call, 1));
    let name = rt::listener_name(action);
    if let Some(r) = editor_action(&call.this(), action, consumer, name) {
        call.set_result_bool(r);
        return;
    }
    if with(|s| s.editing) {
        call.set_result_bool(false);
    }
}
/// scannerDetailsGameController.OnScannedObjectChanged: the editor's key listened for only while scanning.
fn scanned_changed(call: &mut Call) {
    let c = call.this();
    let scanning = call.arg_u64(0) != 0;
    register_own_root(1, &c);
    let panel = with(|s| lock(&s.panel));
    if scanning {
        if !with(|s| s.listening) && !panel.is_null() {
            listen(&panel, &[EDITOR], true);
            with(|s| s.listening = true);
        }
        apply_all();
        // and again once the panel has finished appearing, the only apply that sticks
        schedule_reapply();
    } else if !with(|s| s.editing) && with(|s| s.listening) {
        // not while editing: letting go of the scan gesture is the normal thing once the editor is up
        if !panel.is_null() {
            listen(&panel, &[EDITOR], false);
        }
        with(|s| s.listening = false);
    }
}

/// The hooks, in reds_hooks' table.
pub(super) const HOOKS: [(&str, &str, crate::script_hooks::Kind, crate::script_hooks::HookFn); 12] = {
    use crate::script_hooks::Kind::{After, Override};
    [
        ("scannerGameController", "OnInitialize", After, frame_root),
        ("scannerGameController", "OnScannerHudSpawned", After, frame_root),
        ("scannerDetailsGameController", "OnInitialize", After, details_init),
        ("QuickhacksListGameController", "OnInitialize", After, panel_init),
        ("QuickhacksListGameController", "OnItemSelected", After, item_selected),
        ("QuickhacksListGameController", "OnTargetDisplayNameChanged", After, panel_target),
        ("ScannerHintInkGameController", "OnInitialize", After, hint_root),
        ("ScannerHintInkGameController", "OnVisionModeChanged", After, hint_root),
        ("QuickhacksListGameController", "OnAction", Override, panel_action),
        ("scannerDetailsGameController", "OnQuickHackPanelOpened", After, reapply_later),
        ("scannerDetailsGameController", "OnScannerTabChangeEvent", After, reapply_later),
        ("scannerDetailsGameController", "OnScannedObjectChanged", After, scanned_changed),
    ]
};

/// Every frame: the pending re-applies, and the self-test.
pub(super) fn tick(dt: f64) {
    let due = with(|s| {
        if s.reapply.is_empty() {
            return false;
        }
        let before = s.reapply.len();
        s.reapply.iter_mut().for_each(|t| *t -= dt);
        s.reapply.retain(|t| *t > 0.0);
        s.reapply.len() != before
    });
    if due {
        apply_all();
    }
    self_test(dt);
}

/// A development check (CVR_SCAN_TEST, inherited by the game from the probe session): 45 s into the loaded save the
/// registered pieces and their slots are logged, the layout re-applied and the editor's on-screen line shown once.
fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, bool, usize)> = const { std::cell::Cell::new((0.0, false, 0)) };
    }
    if std::env::var_os("CVR_SCAN_TEST").is_none() {
        return;
    }
    let p = super::belt::player().instance as usize;
    let (t, done, first) = T.with(|c| c.get());
    if p == 0 || done {
        return;
    }
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, done, first)));
        return;
    }
    let t = t + dt;
    T.with(|c| c.set((t, t > 45.0, first)));
    if t <= 45.0 {
        return;
    }
    let pieces: Vec<String> = (0..PIECES)
        .map(|i| {
            let r = root(i);
            format!("{}={}({:.1},{:.1},{:.2})", i, if r.is_null() { "-" } else { "ok" }, slot(i, 0), slot(i, 1), slot(i, 2))
        })
        .collect();
    let panel = with(|s| !lock(&s.panel).is_null());
    log!("[scanner-hud] self-test: pieces {} ; quickhack panel {} ; braindance {} (from Rust)\n", pieces.join(" "), panel, super::braindance_hud::in_braindance());
    apply_all();
    say("Scanner self-test");
    log!("[scanner-hud] self-test done (from Rust)\n");
}
