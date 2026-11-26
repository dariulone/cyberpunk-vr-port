//! mods/cet/CyberpunkVRPort_BraindanceHud (the two braindance HUD pieces neither an asset nor redscript can carry):
//! the "You can't do that right now" plate (NotificationRoot, a spawn slot in no extractable widget file) centred,
//! scaled to 0.8 and moved 250 down, and a box behind "Close" (input_hints' button row: two tinted rectangles created
//! and put under the row). Event-driven, never a timer over the HUD: the braindance controller's OnInitialize arms the
//! work only in a braindance (BraindanceSystem.GetIsInBraindance), then every 0.5 s -- at most 23 times, until both
//! pieces are placed, and only while the braindance lasts -- one walk over the ink layers' virtual windows (10 levels)
//! for both names. OnUnInitialize disarms. The hooks stay the Lua host's (VRBraindanceHudArm/VRBraindanceHudDisarm).
use crate::script_call::{call_handle, enum_value, method, method_typed, native_class_function, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Read back out of the game after the picture was approved.
const NOTIF_SCALE: f32 = 0.8;
/// 200 on screen: the container above it carries a 0.80 scale.
const NOTIF_Y: f32 = 250.0;
/// The button row: icon 64 + label 181.
const BTN_W: f32 = 285.0;
const BTN_H: f32 = 64.0;
/// Its own margins, so the box is placed from the button, not by taste.
const BTN_MR: f32 = 204.0;
const BTN_MB: f32 = 250.0;
const PAD_X: f32 = 26.0;
const PAD_Y: f32 = 20.0;
const FRAME: f32 = 4.0;
/// The tutorial overlay appears later than the braindance itself.
const ATTEMPTS: i32 = 24;
const INTERVAL: f32 = 0.5;
/// Tints copied from the plates' own bg and fg (a widget made at runtime cannot take their atlas).
const TINT_FRAME: [f32; 4] = [0.0, 1.18, 0.92, 1.0];
const TINT_FILL: [f32; 4] = [0.02, 0.03, 0.07, 1.0];

struct State {
    armed: bool,
    left: i32,
    timer: f32,
    plate_done: bool,
    box_done: bool,
}
static STATE: Mutex<State> = Mutex::new(State { armed: false, left: 0, timer: 0.0, plate_done: false, box_done: false });
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Game.GetScriptableSystemsContainer():Get("BraindanceSystem"):GetIsInBraindance(); false when any step fails.
pub(in crate::mechanics) fn in_braindance() -> bool {
    let Some(f) = native_class_function("ScriptGameInstance", "GetScriptableSystemsContainer") else { return false };
    let game = crate::script_call::game_instance();
    // SAFETY: the game instance's static; each object held by its handle for the calls after it.
    unsafe {
        let Some(container) = crate::script_call::invoke_handle(
            core::ptr::null_mut(),
            f,
            &[Value::Ptr((&game as *const crate::script_call::GameInstance).cast())],
        )
        .filter(|h| !h.is_null()) else {
            return false;
        };
        let Some(system) = call_handle(container.instance, "Get", &[Value::U64(CName::hashed("BraindanceSystem").0)]).filter(|h| !h.is_null())
        else {
            return false;
        };
        method::<bool>(system.instance, "GetIsInBraindance", &[]).unwrap_or(false)
    }
}

/// VRBraindanceHudArm(): the braindance controller initialised (armed only in a braindance).
pub(in crate::mechanics) fn arm() {
    let on = in_braindance();
    if !REPORTED.swap(true, Ordering::Relaxed) {
        log!("[mechanics] braindance hud: controller seen from Rust (in braindance: {on})\n");
    }
    if !on {
        return;
    }
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    *s = State { armed: true, left: ATTEMPTS, timer: 0.0, plate_done: false, box_done: false };
}
/// VRBraindanceHudDisarm(): the controller uninitialised.
pub(in crate::mechanics) fn disarm() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.armed = false;
    s.left = 0;
}

#[derive(Default)]
struct Found {
    plate: Option<Handle>,
    hints: Option<Handle>,
}

/// The mod's collect(): both names in one walk, stopping once it has them (10 levels deep).
fn collect(w: &Handle, depth: i32, out: &mut Found) {
    if depth > 10 {
        return;
    }
    // SAFETY: the widget, held by the caller; each child held by its handle.
    let n = unsafe { method::<i32>(w.instance, "GetNumChildren", &[]) }.unwrap_or(0);
    for i in 0..n {
        if out.plate.is_some() && out.hints.is_some() {
            return;
        }
        // SAFETY: as above.
        let Some(c) = (unsafe { call_handle(w.instance, "GetWidgetByIndex", &[Value::Int32(i)]) }).filter(|c| !c.is_null()) else { continue };
        // SAFETY: as above.
        let name = unsafe { method::<u64>(c.instance, "GetName", &[]) }.map(CName);
        if name == Some(CName::hashed("NotificationRoot")) {
            out.plate.get_or_insert_with(|| c.clone());
        } else if name == Some(CName::hashed("input_hints")) {
            out.hints.get_or_insert_with(|| c.clone());
        }
        collect(&c, depth + 1, out);
    }
}

/// `widget:<setter>(value)` for a Vector2.
///
/// # Safety
/// `widget` is a live inkWidget.
unsafe fn set_v2(widget: &Handle, setter: &str, x: f32, y: f32) {
    let v = [x, y];
    // SAFETY: the caller's widget; the value lives for the call.
    unsafe { method_typed::<()>(widget.instance, setter, &["Vector2"], &[Value::Ptr(v.as_ptr().cast())]) };
}
/// `widget:SetMargin(inkMargin{...})`.
///
/// # Safety
/// As [`set_v2`].
unsafe fn set_margin(widget: &Handle, margin: [f32; 4]) {
    // SAFETY: as above.
    unsafe { method_typed::<()>(widget.instance, "SetMargin", &["inkMargin"], &[Value::Ptr(margin.as_ptr().cast())]) };
}
/// `widget:SetAnchor(inkEAnchor.<anchor>)`.
///
/// # Safety
/// As [`set_v2`].
unsafe fn set_anchor(widget: &Handle, anchor: &str) {
    if let Some(value) = enum_value("inkEAnchor", anchor) {
        // SAFETY: as above.
        unsafe { method::<()>(widget.instance, "SetAnchor", &[Value::U64(value as u64)]) };
    }
}

/// The mod's placePlate(): a persistent slot (shown and hidden in place), so placing it once is enough.
fn place_plate(plate: &Handle) -> bool {
    // SAFETY: the plate, held.
    unsafe {
        set_anchor(plate, "Centered");
        set_v2(plate, "SetAnchorPoint", 0.5, 0.5);
        set_margin(plate, [0.0; 4]);
        set_v2(plate, "SetRenderTransformPivot", 0.5, 0.5);
        set_v2(plate, "SetScale", NOTIF_SCALE, NOTIF_SCALE);
        set_v2(plate, "SetTranslation", 0.0, NOTIF_Y);
    }
    true
}

/// The mod's rect(): a tinted rectangle anchored bottom right, added to `parent`.
fn rect(parent: &Handle, w: f32, h: f32, mr: f32, mb: f32, tint: [f32; 4]) {
    let Some(class) = red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_class(CName::hashed("inkRectangleWidget"))) else { return };
    // SAFETY: a new instance owned by the handle made for it; each call on the live widget.
    unsafe {
        let x = Handle::from_instance(class.create_instance());
        if x.is_null() {
            return;
        }
        set_anchor(&x, "BottomRight");
        set_v2(&x, "SetAnchorPoint", 1.0, 1.0);
        set_v2(&x, "SetSize", w, h);
        set_margin(&x, [0.0, 0.0, mr, mb]);
        method_typed::<()>(x.instance, "SetTintColor", &["HDRColor"], &[Value::Ptr(tint.as_ptr().cast())]);
        method::<()>(x.instance, "SetOpacity", &[Value::Float(1.0)]);
        method::<()>(x.instance, "SetVisible", &[Value::Bool(true)]);
        method::<()>(parent.instance, "AddChildWidget", &[Value::Handle(&x)]);
    }
}

/// The mod's buildCloseBox(): the authored input_hints holds exactly one child (its button row); more means built.
fn build_close_box(hints: &Handle) -> bool {
    // SAFETY: the hints widget, held; the row held by its handle.
    unsafe {
        let n = method::<i32>(hints.instance, "GetNumChildren", &[]).unwrap_or(0);
        if n != 1 {
            return n > 1;
        }
        let Some(row) = call_handle(hints.instance, "GetWidgetByIndex", &[Value::Int32(0)]).filter(|r| !r.is_null()) else { return false };
        rect(hints, BTN_W + 2.0 * PAD_X + 2.0 * FRAME, BTN_H + 2.0 * PAD_Y + 2.0 * FRAME, BTN_MR - PAD_X - FRAME, BTN_MB - PAD_Y - FRAME, TINT_FRAME);
        rect(hints, BTN_W + 2.0 * PAD_X, BTN_H + 2.0 * PAD_Y, BTN_MR - PAD_X, BTN_MB - PAD_Y, TINT_FILL);
        // Appending puts the rectangles on top of the button: the row goes back to the end so it draws last.
        let m = method::<i32>(hints.instance, "GetNumChildren", &[]).unwrap_or(0);
        method::<()>(hints.instance, "ReorderChild", &[Value::Handle(&row), Value::Int32(m - 1)]);
    }
    true
}

/// The mod's pass(): one walk over the ink layers' virtual windows.
fn pass(s: &mut State) {
    let mut out = Found::default();
    for layer in crate::engine_objects::layer_wrappers() {
        if out.plate.is_some() && out.hints.is_some() {
            break;
        }
        // SAFETY: the layer's wrapper, held.
        if let Some(vw) = unsafe { call_handle(layer.instance, "GetVirtualWindow", &[]) }.filter(|w| !w.is_null()) {
            collect(&vw, 0, &mut out);
        }
    }
    if let Some(plate) = out.plate.as_ref().filter(|_| !s.plate_done) {
        s.plate_done = place_plate(plate);
        log!("[mechanics] braindance hud: notification plate placed\n");
    }
    if let Some(hints) = out.hints.as_ref().filter(|_| !s.box_done) {
        s.box_done = build_close_box(hints);
        log!("[mechanics] braindance hud: close box {}\n", if s.box_done { "built" } else { "not built" });
    }
}

/// The mod's onUpdate: armed work every 0.5 s.
pub(in crate::mechanics) fn tick(dt: f32) {
    // The chain the arming asks, proven once in any session (the controller's hook may not fire in it).
    static PROBED: super::PerPlayer = super::PerPlayer::new();
    if PROBED.first() {
        let system = native_class_function("ScriptGameInstance", "GetScriptableSystemsContainer").is_some();
        log!("[mechanics] braindance hud: BraindanceSystem asked from Rust (container function: {system}, in braindance: {})\n", in_braindance());
    }
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if !s.armed {
        return;
    }
    if s.plate_done && s.box_done {
        s.armed = false;
        s.left = 0;
        return;
    }
    s.timer -= dt;
    if s.timer > 0.0 {
        return;
    }
    s.timer = INTERVAL;
    // Re-checked on every attempt: a braindance that ends before the budget is spent stops the walk.
    if !in_braindance() {
        s.armed = false;
        s.left = 0;
        return;
    }
    s.left -= 1;
    if s.left <= 0 {
        s.armed = false;
        s.left = 0;
        return;
    }
    pass(&mut s);
}
