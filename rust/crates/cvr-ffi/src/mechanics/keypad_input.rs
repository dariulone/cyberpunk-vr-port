//! mods/cet/CyberpunkVRPort_Stereo/modules/keypad_input.lua (input claimed only for a visible numeric keypad owned by
//! the current DeviceZoom entity; looking past the device does not change that identity): every 50 ms -- a tick of
//! no time counts as 50 ms -- the keypad's root widget, when the player zooms into a device (PlayerStateMachine's
//! IsUIZoomDevice and UIZoomDeviceID) whose camera zoom is active and whose game controller takes input, among its
//! device widgets one whose logic controller is a KeypadDeviceController and which is visible up its parents, goes to
//! the keypad's input (VRKeypadUpdate's work); none without the VR menu closed, a player, the zoom or the keypad. A
//! failure counts as no keypad. The Running state's exit releases it.
use crate::script_call::{call_handle, method, native_class_function, property_of_kind, Value};
use core::ffi::c_void;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{Class, IType, RttiKind};
use std::sync::atomic::Ordering;
use std::sync::Mutex;

struct State {
    elapsed: f64,
    error: Option<String>,
}
static STATE: Mutex<State> = Mutex::new(State { elapsed: 0.0, error: None });
static REPORTED: super::PerPlayer = super::PerPlayer::new();

/// The mod's visible(): the widget and its parents up to 24 levels shown and not transparent (a missing parent ends
/// the walk visible; 24 levels without one, not).
fn visible(widget: &Handle) -> Result<bool, String> {
    let mut current = widget.clone();
    for _ in 0..24 {
        if current.is_null() {
            return Ok(true);
        }
        // SAFETY: the widget, held by its handle for the calls.
        unsafe {
            if !method::<bool>(current.instance, "IsVisible", &[]).ok_or("IsVisible failed")?
                || method::<f32>(current.instance, "GetOpacity", &[]).ok_or("GetOpacity failed")? <= 0.0
            {
                return Ok(false);
            }
            current = call_handle(current.instance, "GetParentWidget", &[]).ok_or("GetParentWidget failed")?;
        }
    }
    Ok(false)
}

/// `controller.deviceWidgetsData`'s widgets (each package's `widget`, a weak reference, locked).
///
/// # Safety
/// `controller` is a live DeviceInkGameControllerBase.
unsafe fn device_widgets(controller: *mut c_void) -> Result<Vec<Handle>, String> {
    // SAFETY: the controller's array property; its element type's own layout.
    unsafe {
        let class = red4ext_abi::rtti::object_class(controller).ok_or("the controller has no class")?;
        let property = class.property(CName::hashed("deviceWidgetsData")).ok_or("no deviceWidgetsData")?;
        let array_type = property.ty.as_ref().ok_or("no deviceWidgetsData type")?;
        if array_type.kind() != RttiKind::Array as u8 {
            return Err("deviceWidgetsData is no array".into());
        }
        let array = property.value_ptr::<c_void>(controller);
        let inner = array_type.array_inner().as_ref().ok_or("no package type")?;
        if inner.kind() != RttiKind::Class as u8 {
            return Err("the package is no struct".into());
        }
        let package = &*(inner as *const IType as *const Class);
        let widget = package.property(CName::hashed("widget")).ok_or("no package widget")?;
        if widget.ty.as_ref().ok_or("no widget type")?.kind() != RttiKind::WeakHandle as u8 {
            return Err("the package widget is no weak reference".into());
        }
        let mut widgets = Vec::new();
        for i in 0..array_type.array_length(array) {
            let element = array_type.array_element(array, i);
            if !element.is_null() {
                widgets.push((*widget.value_ptr::<WeakHandle>(element)).lock());
            }
        }
        Ok(widgets)
    }
}

/// The mod's resolve(): the visible keypad's root widget, null when there is none.
fn resolve() -> Result<Handle, String> {
    use super::boards::{definition, field, get, local_board, system_board};
    let none = Handle::null();
    if super::hud_clearance::OVERLAY_ACTIVE.load(Ordering::Relaxed) {
        return Ok(none);
    }
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() {
        return Ok(none);
    }
    let defs = super::boards::defs()?;
    let ui = definition(&defs, "UI_System")?;
    let menu = system_board(ui)?;
    if !menu.is_null() && get::<bool>(&menu, "GetBool", field(ui, "IsInMenu")?)? {
        return Ok(none);
    }
    // SAFETY: the player, held.
    let id = unsafe { method::<u64>(player.instance, "GetEntityID", &[]) }.ok_or("GetEntityID failed")?;
    let psm_def = definition(&defs, "PlayerStateMachine")?;
    let psm = local_board(id, psm_def)?;
    if psm.is_null() || !get::<bool>(&psm, "GetBool", field(psm_def, "IsUIZoomDevice")?)? {
        return Ok(none);
    }
    let device = get::<u64>(&psm, "GetEntityID", field(psm_def, "UIZoomDeviceID")?)?;
    let find = native_class_function("ScriptGameInstance", "FindEntityByID").ok_or("no FindEntityByID")?;
    let game = crate::script_call::game_instance();
    // SAFETY: the game instance's static; each object held by its handle for the calls after it.
    unsafe {
        let owner = crate::script_call::invoke_handle(
            core::ptr::null_mut(),
            find,
            &[Value::Ptr((&game as *const crate::script_call::GameInstance).cast()), Value::U64(device)],
        )
        .ok_or("FindEntityByID failed")?;
        if owner.is_null() {
            return Ok(none);
        }
        let zoom = property_of_kind(owner.instance, "cameraZoomActive", RttiKind::Fundamental as u8).ok_or("no cameraZoomActive")?;
        if !*(zoom as *const bool) {
            return Ok(none);
        }
        let controller = call_handle(owner.instance, "GetGameController", &[]).ok_or("GetGameController failed")?;
        if controller.is_null() || method::<bool>(controller.instance, "IsInteractivityBlocked", &[]).ok_or("IsInteractivityBlocked failed")? {
            return Ok(none);
        }
        let keypad_type = CName::hashed("KeypadDeviceController").0;
        for root in device_widgets(controller.instance)? {
            if root.is_null() {
                continue;
            }
            let keypad = call_handle(root.instance, "GetController", &[]).ok_or("GetController failed")?;
            if !keypad.is_null()
                && method::<bool>(keypad.instance, "IsA", &[Value::U64(keypad_type)]).ok_or("IsA failed")?
                && visible(&root)?
            {
                return Ok(root);
            }
        }
    }
    Ok(none)
}

/// The mod's tick: every 50 ms the keypad's root (or none) to the keypad input.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let step = f64::from(dt);
    s.elapsed += if step > 0.0 { step } else { 0.05 };
    if s.elapsed < 0.05 {
        return;
    }
    s.elapsed = 0.0;
    let root = match resolve() {
        Ok(root) => {
            if s.error.take().is_some() {
                log!("[mechanics] keypad input: resolving again\n");
            }
            root
        }
        Err(e) => {
            if s.error.as_deref() != Some(e.as_str()) {
                log!("[mechanics] keypad input: {e}\n");
            }
            s.error = Some(e);
            Handle::null()
        }
    };
    // SAFETY: the root held for the call.
    let active = unsafe { cvr_rig::vrik::keypad::update(&root) };
    if s.error.is_none() && REPORTED.first() {
        log!("[mechanics] keypad input: resolved from Rust (keypad={} active={active})\n", !root.is_null());
    }
}

/// The mod's shutdown: no keypad.
pub(in crate::mechanics) fn shutdown() {
    STATE.lock().unwrap_or_else(|p| p.into_inner()).elapsed = 0.0;
    // SAFETY: a null root.
    unsafe { cvr_rig::vrik::keypad::update(&Handle::null()) };
}
