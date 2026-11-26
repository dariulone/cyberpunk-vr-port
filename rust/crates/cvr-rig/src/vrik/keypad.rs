//! The numeric keypad's cursor (formerly src/Natives/KeypadInput.cpp): while the script reports a visible world
//! keypad panel (VRKeypadUpdate) the world-widget hit tester's query (0x750AD4) keeps the native cursor on that panel
//! -- head movement relative to the panel's last real hit, the left stick in panel pixels (cvr-input's keypad
//! cursor) -- so the game's own picker and cursor renderer go on handling hover and click. The panel is held by weak
//! references only, as the game holds its own hit records. The exports and the hook's registration stay in
//! src/Natives/KeypadInput.cpp.
use super::env::GetTickCount64;
use super::xinput::InputControls;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use cvr_engine::hooks::{install_chain, Detour};
use cvr_input::keypad::{self, Cursor, Point};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::object_class;
use red4ext_abi::stack::StackFrame;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[allow(non_upper_case_globals)]
extern "C" {
    static mut CyberpunkVR_KeypadRoot: u64;
    static mut CyberpunkVR_KeypadLayer: u64;
    static CyberpunkVR_KeypadHeadHits: AtomicU64;
    static CyberpunkVR_KeypadHeldHits: AtomicU64;
    static mut CyberpunkVR_KeypadCursor: [f32; 2];
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_menuModeValue: i32;
    fn cvr_cpp_vrui_captures_input() -> bool;
}

const QUERY_RVA: usize = 0x750AD4;
/// The query's whole prologue: a game version that differs leaves the native input path untouched.
const QUERY_PROLOGUE: &str = "48 8B C4 48 89 58 10 55 56 57 41 56 41 57 48 8D A8 F8 FD FF FF 48 81 EC E0 02 00 00";

/// ink::Widget fields (RED4ext.SDK inkWidget.hpp).
const WIDGET_RENDER_SCALE: usize = 0xB8 + 0x08;
const WIDGET_LAYER_PROXY: usize = 0x148;
const WIDGET_PARENT: usize = 0x168;
const WIDGET_SIZE: usize = 0x198;
const WIDGET_OPACITY: usize = 0x1F0;
const WIDGET_VISIBLE: usize = 0x1F8;
/// ink::LayerProxy::layer.
const PROXY_LAYER: usize = 0x38;
/// The world layer's hit tester.
const LAYER_TESTER: usize = 0x1A8;
/// The tester's selected device (the DeviceZoom entity) and its last genuine hit.
const TESTER_DEVICE: usize = 0xA8;
const TESTER_LAST_HIT: usize = 0x40;

static ACTIVE_STAMP: AtomicU64 = AtomicU64::new(0);
static STICK_STAMP: AtomicU64 = AtomicU64::new(0);
static STICK_BITS: AtomicU64 = AtomicU64::new(0);
static HOOK_READY: AtomicBool = AtomicBool::new(false);
static mut ORIGINAL_QUERY: *mut c_void = core::ptr::null_mut();

/// The panel the script reported, by weak reference.
struct Scope {
    root: WeakHandle,
    window: WeakHandle,
    layer: WeakHandle,
}
// SAFETY: weak references to engine objects; the C++ held them in an atomic shared_ptr read from any thread.
unsafe impl Send for Scope {}
unsafe impl Sync for Scope {}
static SCOPE: Mutex<Option<Arc<Scope>>> = Mutex::new(None);

/// CP2077's world-widget hit record (RVA 8BB5B0 / 2446050 / 24468B8): WEAK handles, the game's temporary output
/// included, copied with the engine's own weak-reference ownership.
#[repr(C)]
#[derive(Clone, Default)]
struct WorldHit {
    window: WeakHandle,
    component: WeakHandle,
    local: [f32; 2],
    cursor: [f32; 2],
    screen: [f32; 2],
}
const _: () = assert!(core::mem::size_of::<WorldHit>() == 56);
type QueryFn = unsafe extern "C" fn(usize, *mut WorldHit, *const [f32; 2]) -> *mut WorldHit;

#[derive(Default)]
struct CursorState {
    owner: Option<Arc<Scope>>,
    hit: WorldHit,
    motion: Cursor,
    time: Option<Instant>,
    tester: usize,
    device: u64,
}
// SAFETY: as Scope; only touched under its mutex.
unsafe impl Send for CursorState {}
static CURSOR: Mutex<Option<CursorState>> = Mutex::new(None);

fn diagnostics() -> bool {
    // SAFETY: a plain read of the C++ half's flag.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn menu_mode() -> i32 {
    // SAFETY: the C++ half's volatile int.
    unsafe { core::ptr::read_volatile(&raw const g_menuModeValue) }
}
unsafe fn at<T: Copy>(object: *const c_void, offset: usize) -> T {
    core::ptr::read_unaligned((object as *const u8).add(offset) as *const T)
}
unsafe fn weak_at<'a>(object: *const c_void, offset: usize) -> &'a WeakHandle {
    &*((object as *const u8).add(offset) as *const WeakHandle)
}
unsafe fn class_is(object: *const c_void, name: &str) -> bool {
    object_class(object).is_some_and(|c| c.class_name() == CName::hashed(name))
}
/// The widget and every parent up to the root are visible and not transparent.
unsafe fn visible(widget: &Handle) -> bool {
    let mut current = widget.clone();
    let mut i = 0;
    while !current.is_null() && i < 24 {
        let w = current.instance as *const c_void;
        if !at::<bool>(w, WIDGET_VISIBLE) || at::<f32>(w, WIDGET_OPACITY) <= 0.0 {
            return false;
        }
        current = weak_at(w, WIDGET_PARENT).lock();
        i += 1;
    }
    current.is_null()
}
fn alive(weak: &WeakHandle) -> bool {
    !weak.lock().is_null()
}

unsafe extern "C" fn query(tester: usize, out: *mut WorldHit, screen: *const [f32; 2]) -> *mut WorldHit {
    let result = core::mem::transmute::<*mut c_void, QueryFn>(ORIGINAL_QUERY)(tester, out, screen);
    let current = SCOPE.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let mut guard = CURSOR.lock().unwrap_or_else(|p| p.into_inner());
    let state = guard.get_or_insert_with(CursorState::default);
    let Some(current) = current.filter(|_| keypad_input_active()) else {
        *state = CursorState::default();
        return result;
    };
    let root = current.root.lock();
    let window = current.window.lock();
    let layer = current.layer.lock();
    // The native tester belongs to this world layer and already has the exact DeviceZoom entity selected. Do not keep
    // a panel after native zoom exits.
    let device = at::<u64>(tester as *const c_void, TESTER_DEVICE);
    if root.is_null() || window.is_null() || layer.is_null() || !visible(&root) || device == 0 || at::<usize>(layer.instance, LAYER_TESTER) != tester {
        *state = CursorState::default();
        return result;
    }
    if !state.owner.as_ref().is_some_and(|o| Arc::ptr_eq(o, &current)) || state.tester != tester || state.device != device {
        *state = CursorState {
            owner: Some(current.clone()),
            tester,
            device,
            ..CursorState::default()
        };
    }
    let out_hit = &mut *out;
    let head_hit = out_hit.window.instance == window.instance && alive(&out_hit.component);
    if head_hit {
        state.hit = out_hit.clone();
    }
    // CET may identify the keypad a frame after the player looks away. Use the tester's last genuine hit to arm it,
    // but only for this exact window.
    if state.hit.window.instance.is_null() {
        let previous = &*((tester + TESTER_LAST_HIT) as *const WorldHit);
        if previous.window.instance == window.instance && alive(&previous.component) {
            state.hit = previous.clone();
        }
    }
    if state.hit.window.instance != window.instance || !alive(&state.hit.component) {
        *state = CursorState::default();
        return result;
    }
    let now = Instant::now();
    let dt = state.time.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
    state.time = Some(now);
    let mut stick = Point::default();
    let stamp = STICK_STAMP.load(Ordering::Acquire);
    if stamp != 0 && GetTickCount64().wrapping_sub(stamp) <= 100 {
        let bits = STICK_BITS.load(Ordering::Relaxed);
        stick = Point {
            x: f32::from_bits(bits as u32),
            y: f32::from_bits((bits >> 32) as u32),
        };
    }
    // Native hit mapping: local = authored cursor * window render scale. The native picker and cursor renderer keep
    // handling hover/click events.
    let scale = at::<[f32; 2]>(window.instance, WIDGET_RENDER_SCALE);
    if !scale[0].is_finite() || !scale[1].is_finite() || scale[0] <= 0.0 || scale[1] <= 0.0 {
        *state = CursorState::default();
        return result;
    }
    let size = at::<[f32; 2]>(window.instance, WIDGET_SIZE);
    let size = Point { x: size[0], y: size[1] };
    if !state.motion.initialized {
        let start = Point {
            x: state.hit.cursor[0],
            y: state.hit.cursor[1],
        };
        state.motion.update(true, start, Point::default(), size, 0.0);
        state.motion.had_head = head_hit;
    }
    let head = Point {
        x: out_hit.cursor[0],
        y: out_hit.cursor[1],
    };
    if !state.motion.update(head_hit, head, stick, size, dt) {
        return result;
    }
    WeakHandle::assign(&mut out_hit.window, &state.hit.window);
    WeakHandle::assign(&mut out_hit.component, &state.hit.component);
    out_hit.cursor = [state.motion.position.x, state.motion.position.y];
    out_hit.local = [out_hit.cursor[0] * scale[0], out_hit.cursor[1] * scale[1]];
    out_hit.screen = *screen;
    if diagnostics() {
        (if head_hit { &CyberpunkVR_KeypadHeadHits } else { &CyberpunkVR_KeypadHeldHits }).fetch_add(1, Ordering::Relaxed);
        CyberpunkVR_KeypadCursor = out_hit.cursor;
    }
    result
}

/// The hook (src/Natives/KeypadInput.cpp registers it at Boot, order 87).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_keypad_cursor_install() -> bool {
    let detours = [Detour {
        name: "world widget hit query sub_140750AD4",
        target: QUERY_RVA,
        absolute: false,
        detour: query as *const c_void,
        original: core::ptr::addr_of_mut!(ORIGINAL_QUERY),
        expect: Some(QUERY_PROLOGUE),
    }];
    if install_chain("keypad-cursor", &detours).is_err() {
        return false;
    }
    HOOK_READY.store(true, Ordering::Release);
    true
}

/// The keypad owns the left stick: the hook is in, no menu or VR UI has the input, and the script reported the panel
/// within the last 350 ms.
pub fn keypad_input_active() -> bool {
    let mut controls = InputControls::default();
    // SAFETY: C++ services; the struct is filled in place.
    unsafe {
        HOOK_READY.load(Ordering::Acquire)
            && menu_mode() == 0
            && {
                super::live_reads::input_controls(&mut controls);
                controls.xinput_hook != 0
            }
            && !cvr_cpp_vrui_captures_input()
            && keypad::input_fresh(ACTIVE_STAMP.load(Ordering::Acquire), GetTickCount64())
    }
}
/// The left stick as the keypad cursor reads it (UI_MoveCursor*'s raw axes).
pub fn publish_keypad_stick(x: f32, y: f32, deadzone: f32) {
    let (x, y) = (keypad::cursor_axis(x, deadzone), keypad::cursor_axis(y, deadzone));
    STICK_BITS.store(x.to_bits() as u64 | (y.to_bits() as u64) << 32, Ordering::Relaxed);
    // SAFETY: the system clock.
    STICK_STAMP.store(unsafe { GetTickCount64() }, Ordering::Release);
}

/// cvr::input::KeypadInputActive and PublishKeypadStick (src/Natives/KeypadInput.cpp forwards them).
#[no_mangle]
pub extern "C" fn cvr_keypad_input_active() -> bool {
    keypad_input_active()
}
#[no_mangle]
pub extern "C" fn cvr_keypad_publish_stick(x: f32, y: f32, deadzone: f32) {
    publish_keypad_stick(x, y, deadzone);
}

/// VRKeypadUpdate(root: inkWidget) -> Int32: 1 while `root` is a visible panel inside a virtual window on a world
/// layer, with no menu and no VR UI holding the input.
#[no_mangle]
pub unsafe extern "C" fn VRKeypadUpdate(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut root = Handle::null();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut root);
        frame.finish_params();
    }
    let active = update(&root);
    if let Some(out) = out.as_mut() {
        *out = active;
    }
}
/// VRKeypadUpdate's work: 1 while `root` is a visible panel inside a virtual window on a world layer, with no menu and
/// no VR UI holding the input (the keypad's scope kept for the hit tester); 0 releases it.
///
/// # Safety
/// `root` is null or a live inkWidget's handle.
pub unsafe fn update(root: &Handle) -> i32 {
    let (mut layer, mut window) = (Handle::null(), Handle::null());
    let mut current = root.clone();
    let mut i = 0;
    while !current.is_null() && i < 24 {
        let w = current.instance as *const c_void;
        if class_is(w, "inkVirtualWindow") {
            window = current.clone();
        }
        let proxy = (*((w as *const u8).add(WIDGET_LAYER_PROXY) as *const Handle)).clone();
        if !proxy.is_null() {
            layer = weak_at(proxy.instance, PROXY_LAYER).lock();
        }
        current = weak_at(w, WIDGET_PARENT).lock();
        i += 1;
    }
    let active = !root.is_null()
        && !window.is_null()
        && visible(root)
        && !layer.is_null()
        && class_is(layer.instance, "inkWorldLayer")
        && menu_mode() == 0
        && HOOK_READY.load(Ordering::Acquire)
        && !cvr_cpp_vrui_captures_input();
    {
        let mut scope = SCOPE.lock().unwrap_or_else(|p| p.into_inner());
        if !active {
            ACTIVE_STAMP.store(0, Ordering::Release);
            STICK_STAMP.store(0, Ordering::Release);
            *scope = None;
        } else if !scope
            .as_ref()
            .is_some_and(|s| s.root.instance == root.instance && s.window.instance == window.instance)
        {
            STICK_STAMP.store(0, Ordering::Release);
            *scope = Some(Arc::new(Scope {
                root: WeakHandle::from_handle(&root),
                window: WeakHandle::from_handle(&window),
                layer: WeakHandle::from_handle(&layer),
            }));
        }
    }
    ACTIVE_STAMP.store(if active { GetTickCount64() } else { 0 }, Ordering::Release);
    if diagnostics() {
        CyberpunkVR_KeypadRoot = root.instance as u64;
        CyberpunkVR_KeypadLayer = layer.instance as u64;
    }
    active as i32
}
