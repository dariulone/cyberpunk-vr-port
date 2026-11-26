//! Scene-driven vendor and mirror-appearance events, routed before the engine creates a menu instance (formerly
//! src/Natives/VendorRoute.cpp). With the VR interface off (xr_vr_interface=0) nothing is held back.
//! CET's RequestVendorMenu override sees script calls only; native quest requests reach the shared ink event queue
//! (0xB22F18: GameSystemUI::QueueEvent at 0xB22EF8 and the quest UI command at 0x2C60920 both call it) before any
//! MenuScenario callback runs. While the VR menu's script polls (VRMenuVendorPoll, within the last second) a vendor
//! scenario switch is held back for it to take (VRMenuVendorResolve); refused, timed out (1.5 s, the main thread's
//! watchdog) or with the menu gone, the held event goes on to the native menu. The registration stays in
//! src/Natives/VendorRoute.cpp, the watchdog's game state in rust/crates/cvr-ffi/src/plugin.rs.
use super::env::GetTickCount64;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use cvr_engine::hooks::{install_chain, Detour};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::object_class;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

unsafe extern "C" {
    /// FindInkLayer (cvr-ffi engine_objects.rs): the layer of a native type, or null.
    fn cvr_find_ink_layer(type_name: u64) -> *mut c_void;
}
fn interface_on() -> bool {
    cvr_settings::lc!(xrVrInterface) != 0
}

const QUEUE_RVA: usize = 0xB22F18;
const QUEUE_PROLOGUE: &str = "48 8B C4 48 89 58 10 48 89 70 18 57 48 83 EC 30";
/// GameSystemUI's vtable slot that must hold QueueEvent (0xB22EF8).
const QUEUE_EVENT_SLOT: usize = 0x30BD9C0;
const QUEUE_EVENT: usize = 0xB22EF8;
/// The UI system's internal queue.
const SYSTEM_QUEUE: usize = 0x48;
/// inkMenuInstance_SwitchToScenario (2.5 constructor 27B2D1C): the scenario's CName and the retained userdata handle.
const EVENT_SCENARIO: usize = 0x40;
const EVENT_USERDATA: usize = 0x48;
/// The menu layer's instance (2.5 SwitchToScenario handler 27B3558 reads it at inkMenuLayer +0x168). Raw pointers only:
/// the layer owns its menu instance and the instance owns its scenario.
const LAYER_INSTANCE: usize = 0x168;
/// The instance's switch (vtable +0x108, 0xC6EF4C) identifies scenarios by class name; the current one is at +0xF0.
const SWITCH_SLOT: usize = 0x108;
const SWITCH: usize = 0xC6EF4C;
const CURRENT_SCENARIO: usize = 0xF0;

type QueueFn = unsafe extern "C" fn(*mut c_void, *const Handle) -> usize;
static mut ORIGINAL_QUEUE: *mut c_void = core::ptr::null_mut();
static HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
static HAS_PENDING: AtomicBool = AtomicBool::new(false);
static WATCHDOG_REGISTERED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Request {
    owner: WeakHandle,
    event: Handle,
    panel: Handle,
    captured: u64,
    queue: usize,
}
struct State {
    ui_system: WeakHandle,
    pending: Request,
    /// The mirror request the world editor took: kept until it ends, so the game's own editor can still be given it.
    mirror_request: Request,
    mirror_panel: Handle,
    mirror_instance: usize,
}
// SAFETY: engine references, only touched under the mutex (as the C++ did under its own).
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn log(text: &str) {
    super::env::log(text);
}
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let state = guard.get_or_insert_with(|| State {
        ui_system: WeakHandle::null(),
        pending: Request::default(),
        mirror_request: Request::default(),
        mirror_panel: Handle::null(),
        mirror_instance: 0,
    });
    f(state)
}
unsafe fn class_named(object: *const c_void, name: &str) -> bool {
    !object.is_null() && object_class(object).is_some_and(|c| c.class_name() == CName::hashed(name))
}
unsafe fn internal_queue(system: &Handle) -> usize {
    if system.is_null() {
        return 0;
    }
    *((system.instance as *const u8).add(SYSTEM_QUEUE) as *const usize)
}
/// The panel a scenario switch carries -- a vendor's, or the mirror's appearance request -- or a null handle.
unsafe fn panel(event: &Handle) -> Handle {
    if event.is_null() || !class_named(event.instance, "inkMenuInstance_SwitchToScenario") {
        return Handle::null();
    }
    let bytes = event.instance as *const u8;
    let scenario = *(bytes.add(EVENT_SCENARIO) as *const CName);
    let panel = &*(bytes.add(EVENT_USERDATA) as *const Handle);
    if panel.is_null() {
        return Handle::null();
    }
    if (scenario == CName::hashed("MenuScenario_Vendor") && class_named(panel.instance, "questVendorPanelData"))
        || (scenario == CName::hashed("MenuScenario_CharacterCustomizationMirror") && class_named(panel.instance, "MorphMenuUserData"))
    {
        return panel.clone();
    }
    Handle::null()
}
/// The held event on to the native queue it came from, if that queue still belongs to the live UI system.
unsafe fn forward(request: Request) -> bool {
    let owner = request.owner.lock();
    let original = ORIGINAL_QUEUE;
    if owner.is_null() || internal_queue(&owner) != request.queue || request.event.is_null() || original.is_null() {
        return false;
    }
    core::mem::transmute::<*mut c_void, QueueFn>(original)(request.queue as *mut c_void, &request.event);
    true
}
/// The menu layer's instance, or 0.
unsafe fn menu_instance() -> usize {
    let layer = cvr_find_ink_layer(CName::hashed("inkMenuLayer").0) as usize;
    if layer == 0 {
        return 0;
    }
    *((layer + LAYER_INSTANCE) as *const usize)
}
/// The menu instance the mirror request was taken from, when it is still the layer's one and idle: never act on a
/// different menu that took over during editing.
unsafe fn idle_instance(instance: usize) -> usize {
    if !cvr_engine::game_build::supported() || cvr_engine::game_build::overridden() || instance == 0 || instance != menu_instance() {
        return 0;
    }
    let vtable = *(instance as *const *const usize);
    if *vtable.add(SWITCH_SLOT / 8) != cvr_engine::code::image_base() + SWITCH {
        return 0;
    }
    let scenario = *((instance + CURRENT_SCENARIO) as *const *const c_void);
    if scenario.is_null() || !class_named(scenario, "MenuScenario_Idle") {
        return 0;
    }
    instance
}
fn mirror_instance_of(panel: &Handle) -> usize {
    with_state(|state| if !panel.is_null() && panel.instance == state.mirror_panel.instance { state.mirror_instance } else { 0 })
}
fn mirror_clear() {
    let (request, panel) = with_state(|state| {
        state.mirror_instance = 0;
        (core::mem::take(&mut state.mirror_request), core::mem::take(&mut state.mirror_panel))
    });
    drop((request, panel));
}
/// The scene waits for a scenario listener, not UI_System.IsInMenu: Idle entered again through the normal switch
/// (C6EF4C -> C6F478 -> 2C61378); the public queued event drops Idle in the menu layer's handler.
unsafe fn mirror_finish(panel: &Handle) -> bool {
    let instance = idle_instance(mirror_instance_of(panel));
    mirror_clear();
    if instance == 0 {
        return false;
    }
    type SwitchFn = unsafe extern "C" fn(*mut c_void, CName, *const Handle) -> bool;
    let data = Handle::null();
    let switch: SwitchFn = core::mem::transmute(*(*(instance as *const *const usize)).add(SWITCH_SLOT / 8));
    switch(instance as *mut c_void, CName::hashed("MenuScenario_Idle"), &data)
}
/// The mirror request handed to the game's own editor after all, as the timeout fallback does: that editor then ends the
/// scene's wait itself. False, with nothing changed, when the request can no longer be handed over.
unsafe fn mirror_native(panel: &Handle) -> bool {
    if idle_instance(mirror_instance_of(panel)) == 0 {
        return false;
    }
    let request = with_state(|state| core::mem::take(&mut state.mirror_request));
    if !forward(request) {
        return false;
    }
    mirror_clear();
    true
}

unsafe extern "C" fn queue(queue: *mut c_void, event: *const Handle) -> usize {
    let now = GetTickCount64();
    let last = HEARTBEAT.load(Ordering::Acquire);
    if interface_on() && last != 0 && now.wrapping_sub(last) < 1000 {
        let panel = panel(&*event);
        if !panel.is_null() {
            let held = with_state(|state| {
                let owner = state.ui_system.lock();
                if !state.pending.event.is_null() || owner.is_null() || internal_queue(&owner) != queue as usize {
                    return false;
                }
                state.pending = Request {
                    owner: state.ui_system.clone(),
                    event: (*event).clone(),
                    panel,
                    captured: now,
                    queue: queue as usize,
                };
                HAS_PENDING.store(true, Ordering::Release);
                true
            });
            if held {
                log("[vendor-route] native request retained before menu activation\n");
                return 0;
            }
        }
    }
    core::mem::transmute::<*mut c_void, QueueFn>(ORIGINAL_QUEUE)(queue, event)
}

/// The hook (src/Natives/VendorRoute.cpp registers it at Boot, order 84): only with the watchdog that returns a
/// held request in place.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_vendor_route_install() -> bool {
    let (supported, overridden) = (cvr_engine::game_build::supported(), cvr_engine::game_build::overridden());
    let watchdog = WATCHDOG_REGISTERED.load(Ordering::Relaxed);
    if !supported || overridden || !watchdog {
        log(&format!(
            "[vendor-route] install gate supported={} overridden={} watchdog={}\n",
            supported as i32, overridden as i32, watchdog as i32
        ));
        return false;
    }
    let base = cvr_engine::code::image_base();
    let pattern = cvr_engine::code::Pattern::parse(QUEUE_PROLOGUE);
    if !pattern.is_some_and(|p| cvr_engine::code::expect(base + QUEUE_RVA, &p))
        || cvr_engine::code::read::<usize>(base + QUEUE_EVENT_SLOT) != Some(base + QUEUE_EVENT)
    {
        log("[vendor-route] UI queue byte guard mismatch\n");
        return false;
    }
    let detours = [Detour {
        name: "ink event queue sub_140B22F18",
        target: QUEUE_RVA,
        absolute: false,
        detour: queue as *const c_void,
        original: core::ptr::addr_of_mut!(ORIGINAL_QUEUE),
        expect: Some(QUEUE_PROLOGUE),
    }];
    if let Err(e) = install_chain("vendor-route", &detours) {
        log(&format!("[vendor-route] hook failed {e:?}\n"));
        return false;
    }
    INSTALLED.store(true, Ordering::Release);
    true
}

/// Whether the main thread's watchdog state was registered (rust/crates/cvr-ffi/src/plugin.rs).
#[no_mangle]
pub extern "C" fn cvr_vendor_route_watchdog_registered(ready: bool) {
    WATCHDOG_REGISTERED.store(ready, Ordering::Relaxed);
}
/// The watchdog (game state Running, update): a request not taken within 1.5 s goes back to the native menu.
#[no_mangle]
pub unsafe extern "C" fn cvr_vendor_route_update(_app: *mut c_void) -> bool {
    if !HAS_PENDING.load(Ordering::Acquire) {
        return false;
    }
    let fallback = with_state(|state| {
        if !state.pending.event.is_null() && GetTickCount64().wrapping_sub(state.pending.captured) >= 1500 {
            HAS_PENDING.store(false, Ordering::Relaxed);
            core::mem::take(&mut state.pending)
        } else {
            Request::default()
        }
    });
    if !fallback.event.is_null() {
        log("[vendor-route] timeout; restoring native request\n");
        forward(fallback);
    }
    false
}
/// The watchdog's exit (and the plugin's unload): everything held is dropped.
#[no_mangle]
pub extern "C" fn cvr_vendor_route_reset(_app: *mut c_void) -> bool {
    HEARTBEAT.store(0, Ordering::Release);
    let (pending, ui_system) = with_state(|state| {
        HAS_PENDING.store(false, Ordering::Relaxed);
        (core::mem::take(&mut state.pending), core::mem::take(&mut state.ui_system))
    });
    drop((pending, ui_system));
    mirror_clear();
    true
}

/// VRMenuVendorPoll(enabled, system) -> IScriptable: the held vendor panel while the VR menu wants it; `system` is the
/// game's UI system, kept by weak reference. Polling disabled returns any held request to the native menu.
#[no_mangle]
pub unsafe extern "C" fn VRMenuVendorPoll(_this: *mut c_void, frame: *mut StackFrame, out: *mut Handle, _unused: i64) {
    let (mut enabled, mut system) = (false, Handle::null());
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut enabled);
        frame.get_parameter(&mut system);
        frame.finish_params();
    }
    let enabled = enabled && interface_on();
    let mut fallback = Request::default();
    let result = with_state(|state| {
        if !system.is_null() && class_named(system.instance, "gameuiGameSystemUI") {
            state.ui_system = WeakHandle::from_handle(&system);
        }
        let live = enabled && INSTALLED.load(Ordering::Acquire) && !state.ui_system.lock().is_null();
        HEARTBEAT.store(if live { GetTickCount64() } else { 0 }, Ordering::Release);
        if enabled {
            state.pending.panel.clone()
        } else {
            fallback = core::mem::take(&mut state.pending);
            HAS_PENDING.store(false, Ordering::Relaxed);
            Handle::null()
        }
    });
    forward(fallback);
    if !out.is_null() {
        drop(core::ptr::replace(out, result));
    }
}
/// VRMirrorAppearanceFinish(panel) -> Bool: the world editor is done; the mirror scene goes on.
#[no_mangle]
pub unsafe extern "C" fn VRMirrorAppearanceFinish(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut panel = Handle::null();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut panel);
        frame.finish_params();
    }
    let ok = mirror_finish(&panel);
    if let Some(out) = out.as_mut() {
        *out = ok;
    }
}
/// VRMirrorAppearanceNative(panel) -> Bool: the kept mirror request goes to the game's own editor.
#[no_mangle]
pub unsafe extern "C" fn VRMirrorAppearanceNative(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut panel = Handle::null();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut panel);
        frame.finish_params();
    }
    let ok = mirror_native(&panel);
    if let Some(out) = out.as_mut() {
        *out = ok;
    }
}
/// VRMenuVendorResolve(panel, accepted) -> Bool: the VR menu took the held panel, or gives it back to the native menu.
#[no_mangle]
pub unsafe extern "C" fn VRMenuVendorResolve(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let (mut panel, mut accepted) = (Handle::null(), false);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut panel);
        frame.get_parameter(&mut accepted);
        frame.finish_params();
    }
    let mirror = accepted && class_named(panel.instance, "MorphMenuUserData");
    let instance = if mirror { menu_instance() } else { 0 };
    let request = with_state(|state| {
        if panel.is_null() || panel.instance != state.pending.panel.instance {
            return None;
        }
        HAS_PENDING.store(false, Ordering::Relaxed);
        let request = core::mem::take(&mut state.pending);
        if mirror {
            // kept: the world editor may still give it to the game's own editor
            (state.mirror_panel, state.mirror_instance, state.mirror_request) = (panel.clone(), instance, request);
            return Some(Request::default());
        }
        Some(request)
    });
    let ok = match request {
        None => false,
        Some(request) => {
            if !accepted {
                forward(request);
            }
            log(if accepted {
                "[vendor-route] accepted by world menu\n"
            } else {
                "[vendor-route] returned to native menu\n"
            });
            true
        }
    };
    if let Some(out) = out.as_mut() {
        *out = ok;
    }
}
