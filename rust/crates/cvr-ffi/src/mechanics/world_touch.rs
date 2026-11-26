//! mods/cet/CyberpunkVRPort_Stereo/modules/world_touch.lua with world_touch_sources.lua, computer_touch.lua and
//! touch_contact.lua (the original device screens touched with the rendered finger: the finger's native mesh UV ->
//! the live ink button bounds -> that button's own device action; never UI_confirm/A). Every 0.1 s the screens within
//! reach are discovered -- the loaded world screens' owners (VRInkWorldControllers, walked once a second or after the
//! eyes moved 4 m: Devices within 15 m, a LiftDevice within 250 m when one of its screens is within 15 m), their
//! enabled, interactable WorldWidgetComponents within 1.5 m of the eyes -- and each one's buttons collected from its
//! ink tree (keypad digits, device action widgets, computer menus/documents/banners/links/window closes, a sleeping
//! computer's wake target) with its vertical scroll areas. Every frame each hand's contact (VRWorldTouchPointer: ink
//! X/Y and signed metres in front of the real screen) is stepped through the press/release hysteresis (a drop
//! point's recessed screen its own); a press on a usable button activates it as the game's own pointer callbacks
//! would (keypad HandleButtonClicked, OnExecuteButtonAction, the computer's GoToMenu/OpenDocument/... and the
//! browser's link), with the button's press/hover animations, a haptic and a vending machine's price. Inside an
//! overflowing list a touch that moves 6 mm drags the scroll (and glides after the lift); one that does not is a tap
//! at the lift. Nothing runs with the VR menu open, in a game menu or with the port's Menu open. The math is in f64,
//! as Lua's numbers were.
use crate::script_call::{call_handle, method, method_owned, property, Value};
use core::ffi::c_void;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{Class, IType, RttiKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

type R<T> = Result<T, String>;
type V4 = [f64; 4];

// ---- script calls ------------------------------------------------------------------------------------------------

fn h(o: &Handle, name: &str, values: &[Value]) -> R<Handle> {
    // SAFETY: the caller's live object, held.
    unsafe { call_handle(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    // SAFETY: as above.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn is_a(o: &Handle, class: &str) -> R<bool> {
    m::<bool>(o, "IsA", &[Value::U64(CName::hashed(class).0)])
}
fn opt_is_a(o: &Option<Handle>, class: &str) -> R<bool> {
    match o {
        Some(o) if !o.is_null() => is_a(o, class),
        _ => Ok(false),
    }
}
fn some(handle: Handle) -> Option<Handle> {
    (!handle.is_null()).then_some(handle)
}
fn v2(v: [f32; 2]) -> [f64; 2] {
    [v[0] as f64, v[1] as f64]
}
fn v4(v: [f32; 4]) -> V4 {
    [v[0] as f64, v[1] as f64, v[2] as f64, v[3] as f64]
}
/// A property of the object, in place (scripted ones from the value holder).
fn field(o: &Handle, name: &str) -> R<*mut c_void> {
    // SAFETY: the object, held; its property by RTTI.
    unsafe { property(o.instance, name) }.ok_or_else(|| format!("no {name}"))
}
/// A handle property (a wref locked, a ref cloned).
fn handle_field(o: &Handle, name: &str) -> R<Handle> {
    // SAFETY: the object, held; the property's own kind.
    unsafe {
        let class = red4ext_abi::rtti::object_class(o.instance).ok_or("no class")?;
        let p = class.property_named(name).ok_or_else(|| format!("no {name}"))?;
        let kind = p.ty.as_ref().ok_or("no type")?.kind();
        let at = p.value_ptr::<c_void>(o.instance);
        if kind == RttiKind::WeakHandle as u8 {
            Ok((*(at as *const red4ext_abi::handle::WeakHandle)).lock())
        } else if kind == RttiKind::Handle as u8 {
            Ok((*(at as *const Handle)).clone())
        } else {
            Err(format!("{name} is no reference"))
        }
    }
}
/// `inkWidgetRef.Get(object.<name>)`.
pub(in crate::mechanics) fn widget_ref(o: &Handle, name: &str) -> R<Handle> {
    let at = field(o, name)?;
    let f = crate::script_call::native_class_function("inkWidgetRef", "Get").ok_or("no inkWidgetRef.Get")?;
    // SAFETY: the static with the reference in place.
    unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Ptr(at)]) }.ok_or_else(|| "inkWidgetRef.Get failed".into())
}
/// A struct's field inside a struct value (worlduiSceneWidgetProperties' isInteractable ...).
///
/// # Safety
/// `value` is a live value of the struct type `ty`.
unsafe fn struct_field(value: *mut c_void, ty: &IType, name: &str) -> R<*mut c_void> {
    if ty.kind() != RttiKind::Class as u8 {
        return Err("no struct".into());
    }
    // SAFETY: the caller's struct value; its own property.
    unsafe {
        let class = &*(ty as *const IType as *const Class);
        let p = class.property_named(name).ok_or_else(|| format!("no {name}"))?;
        Ok(p.value_ptr::<c_void>(value))
    }
}
/// The component's sceneWidgetProperties: (isInteractable, useCustomFaceVector, faceVector).
fn scene_widget(c: &Handle) -> R<(bool, bool, [f64; 3])> {
    // SAFETY: the component, held; its struct property and that struct's fields.
    unsafe {
        let class = red4ext_abi::rtti::object_class(c.instance).ok_or("no class")?;
        let p = class.property(CName::hashed("sceneWidgetProperties")).ok_or("no sceneWidgetProperties")?;
        let ty = p.ty.as_ref().ok_or("no type")?;
        let value = p.value_ptr::<c_void>(c.instance);
        let interactable = *(struct_field(value, ty, "isInteractable")? as *const bool);
        let custom = *(struct_field(value, ty, "useCustomFaceVector")? as *const bool);
        let face = *(struct_field(value, ty, "faceVector")? as *const [f32; 3]);
        Ok((interactable, custom, [face[0] as f64, face[1] as f64, face[2] as f64]))
    }
}
fn name_text(w: &Handle) -> R<String> {
    let n = CName(m::<u64>(w, "GetName", &[])?);
    Ok(if n.0 == 0 { "None".into() } else { n.text().to_string() })
}
fn entity_key(o: &Handle) -> R<(u64, String)> {
    let id = m::<u64>(o, "GetEntityID", &[])?;
    Ok((id, id.to_string()))
}
fn haptic(kind: i32, hand: usize) {
    // SAFETY: the haptics' event queue.
    unsafe { cvr_rig::vrik::haptics::cvr_haptics_play(kind, if hand == 2 { 2 } else { 1 }) };
}

// ---- touch_contact.lua -------------------------------------------------------------------------------------------

/// Metres from the actual display surface (not trigger/confirm button state).
#[derive(Clone, Copy)]
struct Profile {
    press: f64,
    release: f64,
}
const CONTACT: Profile = Profile { press: 0.004, release: 0.025 };
const DROP_POINT: Profile = Profile { press: 0.10, release: 0.13 };
const THROUGH: f64 = -0.025;
const MAX_GAP: f64 = 0.10;

struct Drag {
    control: Handle,
    scale: f64,
    range: f64,
    at: V4,
    y0: f64,
    v: f64,
    moved: bool,
    pos0: f64,
    last: f64,
}
struct Glide {
    control: Handle,
    v: f64,
}
struct Tap {
    panel: usize,
    button: usize,
}
#[derive(Default)]
struct Hand {
    armed: bool,
    down: bool,
    previous: Option<f64>,
    panel: Option<String>,
    drag: Option<Drag>,
    pending: Option<Tap>,
    glide: Option<Glide>,
}
fn contact_reset(s: &mut Hand) {
    s.armed = false;
    s.down = false;
    s.previous = None;
}
/// The contact's step: true on a press of a target (even empty or disabled glass consumes the approach; sliding a
/// finger already inside the screen onto another button is no new press).
fn contact_step(s: &mut Hand, sample: Option<V4>, has_target: bool, dt: f64, profile: Option<Profile>) -> bool {
    let Some(sample) = sample.filter(|s| !(s[3] < 0.5 || dt <= 0.0 || dt > MAX_GAP || s[2] < THROUGH)) else {
        contact_reset(s);
        return false;
    };
    let d = sample[2];
    let p = profile.unwrap_or(CONTACT);
    if d >= p.release {
        s.armed = true;
        s.down = false;
    }
    let click = s.armed && !s.down && s.previous.is_some_and(|prev| prev > p.press) && d <= p.press;
    s.previous = Some(d);
    if click {
        s.armed = false;
        s.down = true;
        return has_target;
    }
    false
}

// ---- world_touch_sources.lua -------------------------------------------------------------------------------------

/// An owner farther than this from the eyes holds no screen within touch reach.
const OWNER_REACH_SQ: f64 = 15.0 * 15.0;
/// A lift's floor panels belong to its LiftDevice, whose origin can lie far down the shaft.
const FAR_OWNER_REACH_SQ: f64 = 250.0 * 250.0;
const REWALK_SECONDS: f64 = 1.0;
const REWALK_MOVE_SQ: f64 = 4.0 * 4.0;

struct Near {
    id: u64,
    key: String,
    drop_point: bool,
}
struct Source {
    component: Handle,
    owner: String,
    key: String,
    drop_point: bool,
}

fn widget_components(owner: &Handle) -> Vec<Handle> {
    crate::engine_objects::entity_components_of(owner, CName::hashed("WorldWidgetComponent"))
}
fn centre_of(c: &Handle) -> R<V4> {
    Ok(v4(m::<[[f32; 4]; 4]>(c, "GetLocalToWorld", &[])?[3]))
}
fn dist_sq(a: V4, b: V4) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}
fn screen_within(owner: &Handle, view: V4, reach_sq: f64) -> R<bool> {
    for c in widget_components(owner) {
        if dist_sq(centre_of(&c)?, view) < reach_sq {
            return Ok(true);
        }
    }
    Ok(false)
}
/// The once-a-second walk over the loaded world screens' owners.
fn walk(s: &mut State, view: V4) -> R<()> {
    s.near.clear();
    let mut seen = std::collections::HashSet::new();
    let controllers = crate::engine_objects::ink_world_controller_handles();
    let walked = controllers.len();
    for controller in controllers {
        if controller.is_null() || !is_a(&controller, "DeviceInkGameControllerBase")? {
            continue;
        }
        let owner = h(&controller, "GetOwnerEntity", &[])?;
        if owner.is_null() || !is_a(&owner, "Device")? {
            continue;
        }
        let (id, key) = entity_key(&owner)?;
        if !seen.insert(id) {
            continue;
        }
        let o = v4(m::<[f32; 4]>(&owner, "GetWorldPosition", &[])?);
        let d2 = dist_sq(o, view);
        if d2 < OWNER_REACH_SQ || (d2 < FAR_OWNER_REACH_SQ && is_a(&owner, "LiftDevice")? && screen_within(&owner, view, OWNER_REACH_SQ)?) {
            s.near.push(Near { id, key, drop_point: is_a(&owner, "DropPoint")? });
        }
    }
    if walked > 0 && !WALK_REPORTED.swap(true, Ordering::Relaxed) {
        log!("[mechanics] world touch: first walk from Rust ({walked} world screen controllers, {} device owners, {} within reach)\n", seen.len(), s.near.len());
    }
    Ok(())
}
/// The first walk that saw the world's screens.
static WALK_REPORTED: AtomicBool = AtomicBool::new(false);
/// The screens within reach: the near owners' enabled, interactable world widgets within 1.5 m of the eyes.
fn resolve(s: &mut State, view: V4) -> R<Vec<Source>> {
    let now = s.clock.elapsed().as_secs_f64();
    let moved = s.walked_from.is_none_or(|from| dist_sq(from, view) > REWALK_MOVE_SQ);
    if moved || now - s.walked_at >= REWALK_SECONDS {
        walk(s, view)?;
        s.walked_at = now;
        s.walked_from = Some(view);
    }
    let find = crate::script_call::native_class_function("ScriptGameInstance", "FindEntityByID").ok_or("no FindEntityByID")?;
    let game = crate::script_call::game_instance();
    let mut result = Vec::new();
    for entry in &s.near {
        // SAFETY: the game instance's static.
        let owner = unsafe {
            crate::script_call::invoke_handle(
                core::ptr::null_mut(),
                find,
                &[Value::Ptr((&game as *const crate::script_call::GameInstance).cast()), Value::U64(entry.id)],
            )
        }
        .ok_or("FindEntityByID failed")?;
        if owner.is_null() {
            continue;
        }
        for c in widget_components(&owner) {
            if m::<bool>(&c, "IsEnabled", &[])? && scene_widget(&c)?.0 && dist_sq(centre_of(&c)?, view) < 2.25 {
                let key = format!("{}/{}", entry.key, name_text(&c)?);
                result.push(Source { component: c, owner: entry.key.clone(), key, drop_point: entry.drop_point });
            }
        }
    }
    Ok(result)
}

// ---- computer_touch.lua ------------------------------------------------------------------------------------------

/// A computer's screen: its controller, owner, layout and root.
#[derive(Clone)]
struct Computer {
    controller: Handle,
    owner: Handle,
    layout: Handle,
    root: Handle,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Wake,
    Menu,
    Document,
    BannerClose,
    MailClose,
    FileClose,
    Banner,
    Link,
    WindowClose,
}
fn computer_context(controller: &Handle, root: &Handle) -> R<Option<Computer>> {
    if !is_a(controller, "ComputerInkGameController")? {
        return Ok(None);
    }
    let owner = h(controller, "GetOwnerEntity", &[])?;
    let layout = h(controller, "GetMainLayoutController", &[])?;
    if owner.is_null() || layout.is_null() {
        return Ok(None);
    }
    Ok(Some(Computer { controller: controller.clone(), owner, layout, root: root.clone() }))
}
fn computer_entry(computer: &Computer, widget: Handle, kind: Kind, control: Option<Handle>, key: String) -> Option<Button> {
    some(widget).map(|widget| Button {
        widget,
        keypad: None,
        control,
        key,
        computer: Some((computer.clone(), kind)),
        browser: None,
        web_page: None,
    })
}
fn sleep_target(computer: &Option<Computer>) -> R<Option<Button>> {
    let Some(c) = computer else { return Ok(None) };
    if !m::<bool>(&c.owner, "IsInSleepMode", &[])? {
        return Ok(None);
    }
    let mut target = widget_ref(&c.layout, "screenSaverSlot")?;
    if target.is_null() {
        target = c.root.clone();
    }
    Ok(computer_entry(c, target, Kind::Wake, None, "computer_wake".into()))
}
/// The computer's own controls under `w`: true when the walk stops at this widget.
fn computer_collect(computer: &Option<Computer>, w: &Handle, controller: &Option<Handle>, path: &str, buttons: &mut Vec<Button>) -> R<bool> {
    let (Some(c), Some(controller)) = (computer, controller.as_ref().filter(|c| !c.is_null())) else { return Ok(false) };
    let mut b = None;
    if is_a(controller, "ComputerMenuButtonController")? {
        b = computer_entry(c, handle_field(controller, "targetWidget")?, Kind::Menu, Some(controller.clone()), path.into());
    } else if is_a(controller, "ComputerDocumentThumbnailWidgetController")? {
        b = computer_entry(c, handle_field(controller, "targetWidget")?, Kind::Document, Some(controller.clone()), path.into());
    } else if is_a(controller, "ComputerFullBannerWidgetController")? {
        b = computer_entry(c, widget_ref(controller, "closeButtonWidget")?, Kind::BannerClose, None, format!("{path}/close"));
    } else if is_a(controller, "ComputerDocumentWidgetController")? {
        let mail = crate::script_call::enum_value("EDocumentType", "MAIL");
        let kind = if mail.is_some_and(|mail| m::<i32>(controller, "GetDocumentType", &[]).is_ok_and(|t| t as i64 == mail)) {
            Kind::MailClose
        } else {
            Kind::FileClose
        };
        b = computer_entry(c, widget_ref(controller, "closeButtonWidget")?, kind, None, format!("{path}/close"));
    } else if is_a(controller, "ComputerBannerWidgetController")? {
        b = computer_entry(c, w.clone(), Kind::Banner, Some(controller.clone()), path.into());
    } else if is_a(controller, "LinkController")? {
        let browser = h(&c.layout, "GetInternetController", &[])?;
        if !browser.is_null() {
            b = computer_entry(c, w.clone(), Kind::Link, Some(controller.clone()), path.into());
            if let Some(b) = b.as_mut() {
                b.browser = Some(browser);
                let mut parent = h(w, "GetParentWidget", &[])?;
                for _ in 0..32 {
                    if parent.is_null() {
                        break;
                    }
                    let pc = h(&parent, "GetController", &[])?;
                    if !pc.is_null() && is_a(&pc, "WebPage")? {
                        b.web_page = Some(pc);
                        break;
                    }
                    if !pc.is_null() && is_a(&pc, "BrowserController")? {
                        break;
                    }
                    parent = h(&parent, "GetParentWidget", &[])?;
                }
            }
        }
    }
    let stop = b.as_ref().is_some_and(|b| matches!(b.computer.as_ref().map(|c| c.1), Some(Kind::Menu | Kind::Document | Kind::Banner | Kind::Link)));
    if let Some(b) = b {
        buttons.push(b);
    }
    Ok(stop)
}
fn add_window_close(computer: &Option<Computer>, buttons: &mut Vec<Button>) -> R<()> {
    let Some(c) = computer else { return Ok(()) };
    if let Some(b) = computer_entry(c, h(&c.layout, "GetWindowCloseButton", &[])?, Kind::WindowClose, None, "computer_window_close".into()) {
        buttons.push(b);
    }
    Ok(())
}
fn link_address(control: &Handle) -> R<String> {
    // SAFETY: the link controller, held; the String result owned until read.
    let owned = unsafe { method_owned(control.instance, "GetLinkAddress", &[]) }.ok_or("GetLinkAddress failed")?;
    // SAFETY: a CString result.
    Ok(unsafe { (*(owned.as_ptr() as *const red4ext_abi::cstring::CString)).as_str().to_string() })
}
fn computer_usable(b: &Button) -> R<bool> {
    let (c, kind) = b.computer.as_ref().expect("a computer button");
    let ps = h(&c.owner, "GetDevicePS", &[])?;
    if m::<bool>(&c.controller, "IsInteractivityBlocked", &[])? || ps.is_null() || !m::<bool>(&ps, "IsON", &[])? {
        return Ok(false);
    }
    let sleeping = m::<bool>(&c.owner, "IsInSleepMode", &[])?;
    if *kind == Kind::Wake {
        return Ok(sleeping);
    }
    if *kind == Kind::Link && link_address(b.control.as_ref().ok_or("no link")?)?.is_empty() {
        return Ok(false);
    }
    Ok(!sleeping && m::<bool>(&b.widget, "IsInteractive", &[])?)
}
fn computer_activate(b: &Button) -> R<bool> {
    if !computer_usable(b)? {
        return Ok(false);
    }
    let (c, kind) = b.computer.as_ref().expect("a computer button");
    match kind {
        Kind::Wake => {
            // The native event updates the sleep state and routes to this owner's UI; GoToMenu still applies the
            // authorization and hacking restrictions.
            let class = crate::script_call::class_any("GoToMenuEvent").ok_or("no GoToMenuEvent")?;
            // SAFETY: a new event owned by its handle; its scripted fields set through RTTI.
            let event = unsafe { Handle::from_instance(class.create_instance()) };
            let main = crate::script_call::enum_value("EComputerMenuType", "MAIN").ok_or("no EComputerMenuType.MAIN")?;
            // SAFETY: the event's enum and Bool fields.
            unsafe {
                *(field(&event, "menuType")? as *mut i32) = main as i32;
                *(field(&event, "wakeUp")? as *mut bool) = true;
            }
            m::<()>(&c.owner, "QueueEvent", &[Value::Handle(&event)])?;
        }
        Kind::Menu => {
            let control = b.control.as_ref().ok_or("no control")?;
            // SAFETY: the CName result owned until read.
            let id = unsafe { method_owned(control.instance, "GetMenuID", &[]) }.ok_or("GetMenuID failed")?;
            // SAFETY: as above; GoToMenu(menuID: String).
            unsafe { method::<()>(c.controller.instance, "GoToMenu", &[Value::Ptr(id.as_ptr().cast())]) }.ok_or("GoToMenu failed")?;
        }
        // Retains the native read flags and quest callbacks; never sets facts here.
        Kind::Document => m::<()>(&c.controller, "OpenDocument", &[Value::Handle(b.control.as_ref().ok_or("no control")?)])?,
        Kind::WindowClose => m::<()>(&c.layout, "HideWindow", &[])?,
        Kind::MailClose => m::<()>(&c.layout, "HideMails", &[])?,
        Kind::FileClose => m::<()>(&c.layout, "HideFiles", &[])?,
        Kind::BannerClose => m::<()>(&c.layout, "HideFullBanner", &[])?,
        Kind::Banner => {
            let control = b.control.as_ref().ok_or("no control")?;
            // SAFETY: the banner data result owned until the call.
            let data = unsafe { method_owned(control.instance, "GetBannerData", &[]) }.ok_or("GetBannerData failed")?;
            // SAFETY: ShowFullBanner(controller, data) with the data in place.
            unsafe { method::<()>(c.layout.instance, "ShowFullBanner", &[Value::Handle(&c.controller), Value::Ptr(data.as_ptr().cast())]) }
                .ok_or("ShowFullBanner failed")?;
        }
        Kind::Link => {
            let control = b.control.as_ref().ok_or("no control")?;
            if let Some(page) = &b.web_page {
                // WebPage's callback route, its listeners included, as its OnLinkCallback does.
                // SAFETY: the address result owned until it is assigned to the page's String field.
                unsafe {
                    let address = method_owned(control.instance, "GetLinkAddress", &[]).ok_or("GetLinkAddress failed")?;
                    let text = (*(address.as_ptr() as *const red4ext_abi::cstring::CString)).as_str().to_string();
                    red4ext_abi::cstring::CString::assign(field(page, "lastClickedLinkAddress")? as *mut red4ext_abi::cstring::CString, &text);
                }
                m::<()>(page, "CallCustomCallback", &[Value::U64(CName::hashed("OnLinkPressed").0)])?;
            } else {
                // SAFETY: the address result owned until the call.
                let address = unsafe { method_owned(control.instance, "GetLinkAddress", &[]) }.ok_or("GetLinkAddress failed")?;
                // SAFETY: LoadWebPage(address: String) with the text in place.
                unsafe { method::<()>(b.browser.as_ref().ok_or("no browser")?.instance, "LoadWebPage", &[Value::Ptr(address.as_ptr().cast())]) }
                    .ok_or("LoadWebPage failed")?;
            }
        }
    }
    Ok(true)
}

// ---- world_touch.lua ---------------------------------------------------------------------------------------------

struct Button {
    widget: Handle,
    keypad: Option<Handle>,
    control: Option<Handle>,
    key: String,
    computer: Option<(Computer, Kind)>,
    browser: Option<Handle>,
    web_page: Option<Handle>,
}
#[derive(Clone)]
struct Scroll {
    control: Handle,
    area: Handle,
}
struct ScrollHit {
    control: Handle,
    scale: f64,
    range: f64,
}
struct Panel {
    component: Handle,
    root: Handle,
    controller: Handle,
    buttons: Vec<Button>,
    scrolls: Vec<Scroll>,
    key: String,
    owner: String,
    contact: Option<Profile>,
}
struct Hover {
    control: Handle,
    key: String,
}
struct State {
    elapsed: f64,
    player_id: Option<u64>,
    components: Vec<Panel>,
    hover: [Option<Hover>; 2],
    hands: [Hand; 2],
    near: Vec<Near>,
    walked_at: f64,
    walked_from: Option<V4>,
    clock: std::time::Instant,
    error: Option<String>,
    clicks: u32,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static REPORTED: super::PerPlayer = super::PerPlayer::new();
/// The port's Menu (mods/cet/CyberpunkVRPort_Menu) open, as it reports it (VRMenuModOpen).
pub(in crate::mechanics) static MENU_MOD_OPEN: AtomicBool = AtomicBool::new(false);

fn visible(w: &Handle) -> R<bool> {
    let mut w = w.clone();
    for _ in 0..32 {
        if w.is_null() {
            return Ok(true);
        }
        if !m::<bool>(&w, "IsVisible", &[])? || m::<f32>(&w, "GetOpacity", &[])? <= 0.01 {
            return Ok(false);
        }
        w = h(&w, "GetParentWidget", &[])?;
    }
    Ok(false)
}
fn hover_off(s: &mut State, hand: usize) {
    if let Some(old) = s.hover[hand].take() {
        let _ = m::<()>(&old.control, "TriggerOnHoverOutAnimations", &[]);
    }
}
fn clear(s: &mut State) {
    for hand in 0..2 {
        hover_off(s, hand);
        let st = &mut s.hands[hand];
        contact_reset(st);
        st.drag = None;
        st.pending = None;
        st.glide = None;
    }
    s.components.clear();
}
/// The mod's allowed(): the player, when no menu is up (the VR menu, a game menu, the port's Menu).
fn allowed() -> R<Handle> {
    if super::hud_clearance::OVERLAY_ACTIVE.load(Ordering::Relaxed) {
        return Ok(Handle::null());
    }
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() {
        return Ok(player);
    }
    let defs = super::boards::defs()?;
    let ui = super::boards::definition(&defs, "UI_System")?;
    let board = super::boards::system_board(ui)?;
    if board.is_null() || super::boards::get::<bool>(&board, "GetBool", super::boards::field(ui, "IsInMenu")?)? {
        return Ok(Handle::null());
    }
    if MENU_MOD_OPEN.load(Ordering::Relaxed) {
        return Ok(Handle::null());
    }
    Ok(player)
}

fn collect_buttons(root: &Handle, game_controller: &Handle) -> R<(Vec<Button>, Vec<Scroll>)> {
    let (mut result, mut scrolls) = (Vec::new(), Vec::new());
    let computer = computer_context(game_controller, root)?;
    if let Some(wake) = sleep_target(&computer)? {
        return Ok((vec![wake], scrolls));
    }
    let vertical = crate::script_call::enum_value("inkEScrollDirection", "Vertical");
    let mut visited = 0;
    #[allow(clippy::too_many_arguments)]
    fn walk(
        w: Handle,
        keypad: Option<Handle>,
        depth: i32,
        path: String,
        visited: &mut i32,
        computer: &Option<Computer>,
        vertical: Option<i64>,
        result: &mut Vec<Button>,
        scrolls: &mut Vec<Scroll>,
    ) -> R<()> {
        *visited += 1;
        if w.is_null() || *visited > 1200 || depth > 32 || !m::<bool>(&w, "IsVisible", &[])? || m::<f32>(&w, "GetOpacity", &[])? <= 0.01 {
            return Ok(());
        }
        let controller = some(h(&w, "GetController", &[])?);
        // Mail lists, mail text and similar lists: the native wheel/stick scroller, parents first.
        if opt_is_a(&controller, "inkScrollController")? {
            let c = controller.as_ref().expect("checked");
            // SAFETY: the scroll controller's enum field.
            let direction = unsafe { *(field(c, "direction")? as *const u8) } as i64;
            if Some(direction) == vertical {
                let area = widget_ref(c, "ScrollArea")?;
                if !area.is_null() {
                    scrolls.push(Scroll { control: c.clone(), area });
                }
            }
        }
        if computer_collect(computer, &w, &controller, &path, result)? {
            return Ok(());
        }
        let mut keypad = keypad;
        if opt_is_a(&controller, "KeypadDeviceController")? {
            keypad = controller.clone();
        }
        let name = name_text(&w)?;
        let digit = keypad.is_some() && (name == "Cancel" || name == "AuthorizeUser" || (name.len() == 1 && name.as_bytes()[0].is_ascii_digit()));
        if digit && m::<bool>(&w, "IsInteractive", &[])? {
            result.push(Button {
                widget: w.clone(),
                keypad: keypad.clone(),
                control: controller.clone(),
                key: format!("{path}/{name}"),
                computer: None,
                browser: None,
                web_page: None,
            });
            return Ok(());
        } else if opt_is_a(&controller, "DeviceActionWidgetControllerBase")? {
            let c = controller.as_ref().expect("checked");
            let target = handle_field(c, "targetWidget")?;
            if !target.is_null() && m::<bool>(&target, "IsInteractive", &[])? {
                result.push(Button {
                    widget: target,
                    keypad: None,
                    control: Some(c.clone()),
                    key: format!("{path}/{name}"),
                    computer: None,
                    browser: None,
                    web_page: None,
                });
            }
            return Ok(());
        }
        if is_a(&w, "inkCompoundWidget")? {
            for i in 0..m::<i32>(&w, "GetNumChildren", &[])? {
                let child = h(&w, "GetWidgetByIndex", &[Value::Int32(i)])?;
                walk(child, keypad.clone(), depth + 1, format!("{path}/{i}"), visited, computer, vertical, result, scrolls)?;
            }
        }
        Ok(())
    }
    walk(root.clone(), None, 0, String::new(), &mut visited, &computer, vertical, &mut result, &mut scrolls)?;
    add_window_close(&computer, &mut result)?;
    Ok((result, scrolls))
}

fn usable(b: &Button) -> R<bool> {
    if !visible(&b.widget)? {
        return Ok(false);
    }
    if b.computer.is_some() {
        return computer_usable(b);
    }
    if !m::<bool>(&b.widget, "IsInteractive", &[])? {
        return Ok(false);
    }
    if let Some(keypad) = &b.keypad {
        return m::<bool>(keypad, "CanHandleClickAction", &[]);
    }
    let control = b.control.as_ref().ok_or("no control")?;
    if !m::<bool>(control, "CanExecuteAction", &[])? {
        return Ok(false);
    }
    // SAFETY: the controller, held; the array result owned until its length is read.
    let actions = unsafe { method_owned(control.instance, "GetActions", &[]) }.ok_or("GetActions failed")?;
    // SAFETY: a DynArray: entries, capacity, size.
    Ok(unsafe { *(actions.as_ptr().add(0x0C) as *const u32) } > 0)
}
/// The widget's global rectangle (x, y, width, height); None for a rotated or sheared one or no size.
fn rect(w: &Handle) -> R<Option<[f64; 4]>> {
    let parent = h(w, "GetParentWidget", &[])?;
    if parent.is_null() || !is_a(&parent, "inkCompoundWidget")? {
        return Ok(None);
    }
    // The actual stretch/auto-layout size (GetSize() is often zero).
    let size = v2(m::<[f32; 2]>(&parent, "GetChildSize", &[Value::Handle(w)])?);
    let f = crate::script_call::static_function("WidgetUtils", "LocalToGlobal").ok_or("no WidgetUtils.LocalToGlobal")?;
    // SAFETY: the scripted static with the widget held.
    let point = v2(unsafe { crate::script_call::invoke::<[f32; 2]>(core::ptr::null_mut(), f, &[Value::Handle(w)]) }.ok_or("LocalToGlobal failed")?);
    let (mut sx, mut sy) = (1.0, 1.0);
    let mut a = w.clone();
    for _ in 0..32 {
        if a.is_null() {
            break;
        }
        // Unsupported 2D rotations/shears fail closed: never an AABB's transparent corner pressed.
        let sh = v2(m::<[f32; 2]>(&a, "GetShear", &[])?);
        if (m::<f32>(&a, "GetRotation", &[])? as f64).abs() > 0.01 || sh[0].abs() + sh[1].abs() > 0.001 {
            return Ok(None);
        }
        let scale = v2(m::<[f32; 2]>(&a, "GetScale", &[])?);
        sx *= scale[0];
        sy *= scale[1];
        a = h(&a, "GetParentWidget", &[])?;
    }
    if sx <= 0.0 || sy <= 0.0 || size[0] <= 0.0 || size[1] <= 0.0 {
        return Ok(None);
    }
    Ok(Some([point[0], point[1], size[0] * sx, size[1] * sy]))
}
fn inside(r: [f64; 4], x: f64, y: f64) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}
fn inside_masks(widget: &Handle, x: f64, y: f64) -> R<bool> {
    let mut parent = h(widget, "GetParentWidget", &[])?;
    for _ in 0..32 {
        if parent.is_null() {
            return Ok(true);
        }
        // SAFETY: the scroll area's Bool field.
        if is_a(&parent, "inkScrollAreaWidget")? && unsafe { *(field(&parent, "useInternalMask")? as *const bool) } {
            match rect(&parent)? {
                Some(r) if inside(r, x, y) => {}
                _ => return Ok(false),
            }
        }
        parent = h(&parent, "GetParentWidget", &[])?;
    }
    Ok(false)
}
fn hit(buttons: &[Button], x: f64, y: f64) -> R<Option<usize>> {
    for i in (0..buttons.len()).rev() {
        let b = &buttons[i];
        if usable(b)? {
            if let Some(r) = rect(&b.widget)? {
                if inside(r, x, y) && inside_masks(&b.widget, x, y)? {
                    return Ok(Some(i));
                }
            }
        }
    }
    Ok(None)
}
fn v2_field(o: &Handle, name: &str) -> R<[f64; 2]> {
    // SAFETY: the object's Vector2 field.
    Ok(v2(unsafe { *(field(o, name)? as *const [f32; 2]) }))
}
/// The innermost visible scroll area under a native ink point whose content overflows it.
fn scroll_at(scrolls: &[Scroll], x: f64, y: f64) -> R<Option<ScrollHit>> {
    for s in scrolls.iter().rev() {
        let range = v2_field(&s.control, "contentSize")?[1] - v2_field(&s.control, "viewportSize")?[1];
        if range > 1.0 && visible(&s.area)? {
            if let Some(r) = rect(&s.area)? {
                if inside(r, x, y) && inside_masks(&s.area, x, y)? {
                    return Ok(Some(ScrollHit { control: s.control.clone(), scale: r[3] / v2_field(&s.control, "viewportSize")?[1], range }));
                }
            }
        }
    }
    Ok(None)
}
/// Finger travel along the glass before a touch in a scroll area becomes a drag, the fling's decay per second.
const DRAG_SLOP: f64 = 0.006;
const GLIDE_DECAY: f64 = 5.0;
const GLIDE_STOP: f64 = 0.03;
fn scroll_to(control: &Handle, pos: f64) -> R<f64> {
    let pos = pos.clamp(0.0, 1.0);
    m::<()>(control, "SetScrollPosition", &[Value::Float(pos as f32)])?;
    Ok(pos)
}
fn glide(state: &mut Hand, dt: f64) -> R<()> {
    let Some(g) = state.glide.as_mut() else { return Ok(()) };
    let Ok(old) = m::<f32>(&g.control, "GetScrollPosition", &[]) else {
        state.glide = None;
        return Ok(());
    };
    let pos = scroll_to(&g.control, old as f64 + g.v * dt)?;
    g.v *= (-GLIDE_DECAY * dt).exp();
    if g.v.abs() < GLIDE_STOP || pos <= 0.0 || pos >= 1.0 {
        state.glide = None;
    }
    Ok(())
}
fn discover(s: &mut State, view: V4) -> R<()> {
    s.components.clear();
    for source in resolve(s, view)? {
        let c = &source.component;
        let root = h(c, "GetWidget", &[])?;
        let controller = h(c, "GetGameController", &[])?;
        if !root.is_null()
            && !controller.is_null()
            && is_a(&controller, "DeviceInkGameControllerBase")?
            && visible(&root)?
            && !m::<bool>(&controller, "IsInteractivityBlocked", &[])?
        {
            let (buttons, scrolls) = collect_buttons(&root, &controller)?;
            if !buttons.is_empty() || !scrolls.is_empty() {
                s.components.push(Panel {
                    component: source.component.clone(),
                    root,
                    controller,
                    buttons,
                    scrolls,
                    key: source.key,
                    owner: source.owner,
                    contact: source.drop_point.then_some(DROP_POINT),
                });
            }
        }
    }
    Ok(())
}
fn activate(s: &mut State, panel: usize, button: usize, hand: usize) -> R<bool> {
    let p = &s.components[panel];
    let b = &p.buttons[button];
    if m::<bool>(&p.controller, "IsInteractivityBlocked", &[])? || !usable(b)? {
        return Ok(false);
    }
    let animate = opt_is_a(&b.control, "DeviceButtonLogicControllerBase")?;
    if animate {
        m::<()>(b.control.as_ref().expect("checked"), "TriggerOnPressAnimations", &[])?;
    }
    if b.computer.is_some() {
        if !computer_activate(b)? {
            return Ok(false);
        }
    } else if let Some(keypad) = &b.keypad {
        // The same per-key handler as OnMouseButtonReleased: digit limits, authorization, password checks, sounds and
        // the native door action callbacks kept.
        m::<()>(keypad, "HandleButtonClicked", &[Value::Handle(&b.widget)])?;
    } else {
        m::<()>(&b.widget, "CallCustomCallback", &[Value::U64(CName::hashed("OnExecuteButtonAction").0)])?;
    }
    // Browser navigation destroys the clicked page synchronously: the expired controller is not asked again.
    if animate {
        let _ = m::<()>(b.control.as_ref().expect("checked"), "TriggerOnReleaseAnimations", &[]);
    }
    s.clicks += 1;
    log!("[mechanics] world touch: click {} on {} by hand {hand} from Rust\n", b.key, p.owner);
    haptic(20, hand);
    // Changed or removed buttons reread before another contact.
    s.elapsed = 0.1;
    Ok(true)
}

/// One hand's chosen contact this frame.
struct Chosen {
    sample: V4,
    point: V4,
    panel: usize,
    button: Option<usize>,
}

fn step(s: &mut State, dt: f64) -> R<()> {
    let p = allowed()?;
    if p.is_null() {
        clear(s);
        return Ok(());
    }
    let (id, _) = entity_key(&p)?;
    if Some(id) != s.player_id {
        clear(s);
        s.near.clear();
        s.walked_at = -1e9;
        s.walked_from = None;
        s.player_id = Some(id);
    }
    // SAFETY: the view pose's read.
    let view = v4(unsafe { cvr_rig::vrik::hand_body::view_world_pos() });
    s.elapsed += dt;
    if s.elapsed >= 0.1 {
        s.elapsed = 0.0;
        discover(s, view)?;
    }
    let mut chosen: [Option<Chosen>; 2] = [None, None];
    for (pi, panel) in s.components.iter().enumerate() {
        let c = &panel.component;
        if !(m::<bool>(c, "IsEnabled", &[])? && visible(&panel.root)? && !m::<bool>(&panel.controller, "IsInteractivityBlocked", &[])?) {
            continue;
        }
        let matrix = m::<[[f32; 4]; 4]>(c, "GetLocalToWorld", &[])?.map(v4);
        let centre = matrix[3];
        if dist_sq(centre, view) >= 2.25 {
            continue;
        }
        // SAFETY: the diagnostic capture's flag.
        unsafe { cvr_rig::vrik::calibration::set_diag_capture(true) };
        // The native UI geometry is X/Z with the front -Y by default; a custom face vector decides front and back.
        let (_, custom, face) = scene_widget(c)?;
        let (x, y, z) = (matrix[0], matrix[1], matrix[2]);
        let normal = if custom {
            [
                x[0] * face[0] + y[0] * face[1] + z[0] * face[2],
                x[1] * face[0] + y[1] * face[1] + z[1] * face[2],
                x[2] * face[0] + y[2] * face[1] + z[2] * face[2],
            ]
        } else {
            [-y[0], -y[1], -y[2]]
        };
        let n = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if n <= 0.001 {
            continue;
        }
        let normal = [(normal[0] / n) as f32, (normal[1] / n) as f32, (normal[2] / n) as f32, 0.0];
        let centre_f = [centre[0] as f32, centre[1] as f32, centre[2] as f32, centre[3] as f32];
        // SAFETY: the component, held.
        let samples = unsafe { cvr_rig::vrik::world_touch::pointer(c, centre_f, normal) };
        for hand in 0..2 {
            let Some(&sample) = samples.get(hand * 2) else { continue };
            let sample = v4(sample);
            if sample[3] > 0.5 && chosen[hand].as_ref().is_none_or(|ch| sample[2].abs() < ch.sample[2].abs()) {
                let point = samples.get(hand * 2 + 1).map_or([0.0; 4], |p| v4(*p));
                chosen[hand] = Some(Chosen { sample, point, panel: pi, button: hit(&panel.buttons, sample[0], sample[1])? });
            }
        }
    }
    for hand in 0..2 {
        let h1 = hand + 1;
        let c = chosen[hand].as_ref();
        let button = c.and_then(|c| c.button.map(|b| (c.panel, b)));
        let hover_key = button.map(|(p, b)| format!("{}{}", s.components[p].key, s.components[p].buttons[b].key));
        if hover_key != s.hover[hand].as_ref().map(|h| h.key.clone()) {
            hover_off(s, hand);
            if let Some((p, b)) = button {
                let control = s.components[p].buttons[b].control.clone();
                if opt_is_a(&control, "DeviceButtonLogicControllerBase")? {
                    let control = control.expect("checked");
                    m::<()>(&control, "TriggerOnHoverOverAnimations", &[])?;
                    if is_a(&control, "ImageActionButtonLogicController")? && is_a(&s.components[p].controller, "VendingMachineInkGameController")? {
                        // The native vending hover updates the price for this product.
                        let price = m::<i32>(&control, "GetPrice", &[])?;
                        m::<()>(&s.components[p].controller, "UpdatePrice", &[Value::Int32(price)])?;
                    }
                    s.hover[hand] = Some(Hover { control, key: hover_key.clone().unwrap_or_default() });
                }
            }
        }
        let panel_key = c.map(|c| s.components[c.panel].key.clone());
        if panel_key != s.hands[hand].panel {
            let st = &mut s.hands[hand];
            contact_reset(st);
            st.panel = panel_key;
            st.drag = None;
            st.pending = None;
        }
        let sample = c.map(|c| c.sample);
        glide(&mut s.hands[hand], dt)?;
        // Lifting the finger ends its drag; a touch that never dragged is the tap.
        let profile = c.and_then(|c| s.components[c.panel].contact).unwrap_or(CONTACT);
        let lifted = s.hands[hand].drag.is_some() && sample.is_none_or(|sm| sm[2] > (profile.press + profile.release) / 2.0);
        if lifted {
            let st = &mut s.hands[hand];
            let (drag, tap) = (st.drag.take().expect("checked"), st.pending.take());
            if drag.moved && drag.v.abs() > GLIDE_STOP {
                st.glide = Some(Glide { control: drag.control.clone(), v: drag.v });
            }
            if let (Some(tap), Some(_)) = (tap, sample) {
                activate(s, tap.panel, tap.button, h1)?;
            }
        }
        let was = s.hands[hand].down;
        let mut press = contact_step(&mut s.hands[hand], sample, button.is_some(), dt, c.and_then(|c| s.components[c.panel].contact));
        if let Some(c) = c {
            if s.hands[hand].down && !was {
                // Inside an overflowing list a tap waits for the lift: the same touch may become a drag.
                if let Some(scroll) = scroll_at(&s.components[c.panel].scrolls, c.sample[0], c.sample[1])? {
                    let st = &mut s.hands[hand];
                    st.glide = None;
                    st.drag = Some(Drag {
                        control: scroll.control.clone(),
                        scale: scroll.scale,
                        range: scroll.range,
                        at: c.point,
                        y0: c.sample[1],
                        v: 0.0,
                        moved: false,
                        pos0: 0.0,
                        last: 0.0,
                    });
                    if press {
                        st.pending = button.map(|(p, b)| Tap { panel: p, button: b });
                        press = false;
                    }
                }
            }
            if let Some(drag) = s.hands[hand].drag.as_mut() {
                let control = drag.control.clone();
                if !drag.moved && dist_sq(c.point, drag.at) > DRAG_SLOP * DRAG_SLOP {
                    drag.moved = true;
                    drag.y0 = c.sample[1];
                    drag.pos0 = m::<f32>(&control, "GetScrollPosition", &[])? as f64;
                    drag.last = drag.pos0;
                    s.hands[hand].pending = None;
                    haptic(21, h1);
                }
                let drag = s.hands[hand].drag.as_mut().expect("present");
                if drag.moved {
                    let pos = scroll_to(&control, drag.pos0 + (drag.y0 - c.sample[1]) / (drag.scale * drag.range))?;
                    drag.v = drag.v * 0.6 + (pos - drag.last) / dt.max(0.001) * 0.4;
                    drag.last = pos;
                }
            }
        }
        if press {
            if let (Some(c), Some((p, b))) = (c, button) {
                activate(s, c.panel, b, h1)?;
                let _ = p;
                // Two fingers landing together on the same button are one click.
                if let Some(other) = chosen[1 - hand].as_ref() {
                    if let Some(ob) = other.button {
                        if s.components[other.panel].key == s.components[p].key && s.components[other.panel].buttons[ob].key == s.components[p].buttons[b].key {
                            s.hands[1 - hand].armed = false;
                            s.hands[1 - hand].down = true;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// The mod's tick: every frame.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        elapsed: 0.0,
        player_id: None,
        components: Vec::new(),
        hover: [None, None],
        hands: [Hand::default(), Hand::default()],
        near: Vec::new(),
        walked_at: -1e9,
        walked_from: None,
        clock: std::time::Instant::now(),
        error: None,
        clicks: 0,
    });
    match step(s, f64::from(dt).max(0.0)) {
        Ok(()) => {
            s.error = None;
            if !s.components.is_empty() && REPORTED.first() {
                log!(
                    "[mechanics] world touch: screens within reach from Rust ({} panels, {} buttons, {} scrolls; first {})\n",
                    s.components.len(),
                    s.components.iter().map(|p| p.buttons.len()).sum::<usize>(),
                    s.components.iter().map(|p| p.scrolls.len()).sum::<usize>(),
                    s.components[0].key
                );
            }
        }
        Err(e) => {
            if s.error.as_deref() != Some(e.as_str()) {
                log!("[mechanics] world touch: {e}\n");
                s.error = Some(e);
            }
            clear(s);
        }
    }
}

/// The mod's shutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        clear(s);
        s.player_id = None;
        s.elapsed = 0.0;
    }
}
