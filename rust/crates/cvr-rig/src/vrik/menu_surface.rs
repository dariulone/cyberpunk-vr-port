//! Dedicated native ink surfaces for the port's own screens (formerly src/Natives/MenuSurface.cpp): the VR menu's and
//! the lobby's world widgets get a whole DynamicTexture of their canvas's size instead of a shared world-widget atlas
//! slot (no coverage LOD, full UVs), and stay painted while their display mesh is still disabled. The engine still
//! renders the menu's world geometry with each eye's scene depth; only the UI allocation changes. VRMenuSurfaceReady
//! answers once the surface's first ink write has finished on its GPU queue: the command-list hooks of the stereo half
//! report the writes (Barriers, Written), their submission (Submitted) and list resets (ResetList), and a fence is
//! signalled after the producing list. The exports, the registration and the C++ names those hooks call stay in
//! src/Natives/MenuSurface.cpp.
use cvr_host::fast::FastMap;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use cvr_engine::code::{expect, image_base, Pattern};
use cvr_engine::hooks::{install_chain, Detour};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::resource::resource_path;
use red4ext_abi::rtti::object_class;
use red4ext_abi::stack::StackFrame;
use std::sync::{Arc, Mutex};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource, D3D12_FENCE_FLAG_NONE, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
};

extern "C" {
    /// Allocations, native updates, readiness polls, armed polls, recorded writes, submitted producer fences, completed
    /// surfaces, currently retained surfaces.
    static CyberpunkVR_MenuSurfaceStats: [AtomicU64; 8];
}
fn stat(i: usize) -> &'static AtomicU64 {
    // SAFETY: the C++ half's exported counters.
    unsafe { &CyberpunkVR_MenuSurfaceStats[i] }
}
fn log(text: &str) {
    super::env::log(text);
}

// ---- the engine's addresses (2.5) ----
const ADD_RVA: usize = 0x6FE754;
const UPDATE_RVA: usize = 0x6067C0;
const CREATE_TEXTURE_RVA: usize = 0x5EA8A4;
const SET_SCALE_RVA: usize = 0x54A748;
const INVALIDATE_RVA: usize = 0x2805FC;
const ADD_PROLOGUE: &str = "48 89 5C 24 10 48 89 74 24 18 48 89 7C 24 20 55";
const UPDATE_PROLOGUE: &str = "48 8B C4 48 89 58 08 48 89 70 10 48 89 78 18 4C";
const CREATE_TEXTURE_PROLOGUE: &str = "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 30 48";
const SET_SCALE_PROLOGUE: &str = "40 53 48 83 EC 20 F3 0F 10 0A 48 8B D9 0F 2E 89";
const INVALIDATE_PROLOGUE: &str = "48 89 5C 24 18 55 48 8B EC 48 83 EC 60 80 89 FD 01 00 00 10";
/// The native world entry's vtable; its weak wrapper (+0x190), whose weak component is at +8.
const WORLD_ENTRY_VTABLE: usize = 0x3111DC8;
const ENTRY_WRAPPER: usize = 0x190;
const WRAPPER_COMPONENT: usize = 8;
/// The entry's ink window, its paint distance and its "paint" flag; the wrapper's enable byte.
const ENTRY_WINDOW: usize = 128;
const ENTRY_PAINT_DISTANCE: usize = 56;
const ENTRY_PAINT: usize = 152;
const WRAPPER_ENABLED: usize = 108;
/// ink::Widget: renderTransform.scale; the window's atlas crop.
const WIDGET_RENDER_SCALE: usize = 0xB8 + 0x08;
const WINDOW_CROP: usize = 0x528;
/// WorldWidgetComponent: widgetResource's path; the applied texture, the render proxy's weak count block, what was
/// last sent, the "shows its texture" byte, the dirty flags.
const COMPONENT_WIDGET_RESOURCE: usize = 0x2D0;
const COMPONENT_TEXTURE: usize = 0x230;
const COMPONENT_PROXY_COUNTS: usize = 0x228;
const COMPONENT_SENT: usize = 0x250;
const COMPONENT_SHOWS: usize = 0x8B;
const COMPONENT_DIRTY: usize = 0x2A4;
/// DynamicTexture: width, height, the native backing (vtable 0x30646A8, pool id at +0x18); the texture pool.
const TEXTURE_WIDTH: usize = 0x40;
const TEXTURE_HEIGHT: usize = 0x44;
const TEXTURE_BACKING: usize = 0x70;
const BACKING_VTABLE: usize = 0x30646A8;
const TEXTURE_POOL: usize = 0x3BCDCA8;

struct Surface {
    component: WeakHandle,
    texture: Handle,
    resource: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    submitted: u64,
    armed: bool,
    ready: bool,
}
/// The C++ shared_ptr<Surface>: shared between the surfaces map and the recorded lists, touched only under STATE.
struct Shared(UnsafeCell<Surface>);
// SAFETY: every access is under the STATE mutex.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}
type SurfaceRef = Arc<Shared>;
fn surface(r: &SurfaceRef) -> &mut Surface {
    // SAFETY: only called with the STATE lock held, one borrow at a time.
    unsafe { &mut *r.0.get() }
}

#[derive(Default)]
struct State {
    surfaces: FastMap<usize, SurfaceRef>,
    recorded: FastMap<usize, Vec<SurfaceRef>>,
    next_prune: u64,
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(State::default))
}
static INSTALLED: AtomicBool = AtomicBool::new(false);
static OBSERVING: AtomicBool = AtomicBool::new(false);
static TRACKED: AtomicBool = AtomicBool::new(false);
static mut ORIGINAL_ADD: *mut c_void = core::ptr::null_mut();
static mut ORIGINAL_UPDATE: *mut c_void = core::ptr::null_mut();
type AddFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u8;
type UpdateFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;

unsafe fn at<T: Copy>(address: usize) -> T {
    core::ptr::read_unaligned(address as *const T)
}
unsafe fn set<T>(address: usize, value: T) {
    core::ptr::write_unaligned(address as *mut T, value)
}
fn same_ptr<T: Interface>(a: &Option<T>, raw: *mut c_void) -> bool {
    a.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()) == raw
}

/// The entry's widget-component wrapper while it is alive, or 0.
unsafe fn wrapper_of(entry: usize) -> usize {
    (*((entry + ENTRY_WRAPPER) as *const WeakHandle)).alive_instance() as usize
}
/// The port's own world widget of a native world entry, or null: the VR menu's screens and the lobby's terminal.
unsafe fn component_of(entry: usize) -> Handle {
    if entry == 0 || at::<usize>(entry) != image_base() + WORLD_ENTRY_VTABLE {
        return Handle::null();
    }
    // The wrapper is a WeakPtr<ink::IWidgetComponentWrapper> (not an ISerializable): looked at while alive, not locked.
    let wrapper = wrapper_of(entry);
    if wrapper == 0 {
        return Handle::null();
    }
    let component = (*((wrapper + WRAPPER_COMPONENT) as *const WeakHandle)).lock();
    if component.is_null() {
        return component;
    }
    let name = at::<CName>(component.instance as usize + 0x40);
    if (name != CName::hashed("vrp_menu_ui") && name != CName::hashed("vrp_lobby_ui"))
        || !object_class(component.instance).is_some_and(|c| c.is_a(CName::hashed("WorldWidgetComponent")))
    {
        return Handle::null();
    }
    component
}
/// The canvas size of the widget the component shows, or zero for one that is not ours.
unsafe fn size_of(component: usize) -> [u32; 2] {
    if component == 0 {
        return [0, 0];
    }
    let path = at::<u64>(component + COMPONENT_WIDGET_RESOURCE);
    if path == resource_path("vrport\\ui\\vr_menu.inkwidget") {
        return [3200, 1600];
    }
    if path == resource_path("vrport\\ui\\vr_menu_beam.inkwidget") {
        return [16, 2048];
    }
    // The lobby terminal's screen: tools/vr_lobby_assets/make_lobby_assets.py CANVAS.
    if path == resource_path("vrport\\ui\\vr_lobby.inkwidget") {
        return [2580, 1980];
    }
    // The new game's window in the lobby: lobby_layout.HOLO_WINDOW_CANVAS.
    if path == resource_path("vrport\\ui\\vr_lobby_window.inkwidget") {
        return [3190, 2900];
    }
    [0, 0]
}
/// VirtualWindow::FitTexture assumes a square atlas slot (it shrinks a portrait canvas and reports an aspect crop to
/// the mesh material); the texture already has the canvas's extent, so both transforms are identities here.
unsafe fn fit_window(entry: usize, component: usize) {
    let size = size_of(component);
    let window = at::<usize>(entry + ENTRY_WINDOW);
    if window == 0 || size[0] == 0 {
        return;
    }
    let scale = [1.0f32, 1.0];
    let current = at::<[f32; 2]>(window + WIDGET_RENDER_SCALE);
    if current[0] != 1.0 || current[1] != 1.0 {
        type SetScale = unsafe extern "C" fn(usize, *const [f32; 2]);
        core::mem::transmute::<usize, SetScale>(image_base() + SET_SCALE_RVA)(window, &scale);
    }
    let crop = at::<[f32; 2]>(window + WINDOW_CROP);
    if crop[0] != 1.0 || crop[1] != 1.0 {
        set(window + WINDOW_CROP, scale);
        set(component + COMPONENT_DIRTY, at::<u8>(component + COMPONENT_DIRTY) | 1);
    }
}
/// A replaced render proxy starts from the material's defaults: with no live proxy nothing of ours counts as applied,
/// and while what was applied is not our texture (and the component shows its texture) it stays dirty.
unsafe fn rebind(component: usize, texture: usize) {
    if texture == 0 || at::<usize>(component + COMPONENT_TEXTURE) != texture {
        return;
    }
    let alive = at::<usize>(component + COMPONENT_PROXY_COUNTS);
    if alive == 0 || core::ptr::read_volatile(alive as *const i32) == 0 {
        set(component + COMPONENT_SENT, 0usize);
    }
    if at::<usize>(component + COMPONENT_SENT) != texture && at::<u8>(component + COMPONENT_SHOWS) != 0 {
        set(component + COMPONENT_DIRTY, at::<u8>(component + COMPONENT_DIRTY) | 1);
    }
}
/// Marks a widget and its ancestors changed: ink rebuilds the window's draw data and paints it again.
unsafe fn invalidate(widget: usize) {
    if widget != 0 {
        type F = unsafe extern "C" fn(usize);
        core::mem::transmute::<usize, F>(image_base() + INVALIDATE_RVA)(widget);
    }
}

impl State {
    fn prune(&mut self) {
        let recorded = &self.recorded;
        self.surfaces.retain(|_, r| {
            let pending = recorded.values().any(|list| list.iter().any(|x| Arc::ptr_eq(x, r)));
            let s = surface(r);
            // SAFETY: a fence this file created.
            let done = s.fence.is_none() || s.submitted == 0 || unsafe { s.fence.as_ref().unwrap().GetCompletedValue() } >= s.submitted;
            pending || !s.component.lock().is_null() || !done
        });
        stat(7).store(self.surfaces.len() as u64, Ordering::Relaxed);
    }
    fn get(&mut self, component: &Handle) -> SurfaceRef {
        let key = component.instance as usize;
        if let Some(r) = self.surfaces.get(&key) {
            if surface(r).component.lock().instance == component.instance {
                return r.clone();
            }
        }
        self.prune();
        let r = Arc::new(Shared(UnsafeCell::new(Surface {
            component: WeakHandle::from_handle(component),
            texture: Handle::null(),
            resource: None,
            fence: None,
            submitted: 0,
            armed: false,
            ready: false,
        })));
        self.surfaces.insert(key, r.clone());
        TRACKED.store(true, Ordering::SeqCst);
        stat(7).store(self.surfaces.len() as u64, Ordering::Relaxed);
        r
    }
}
/// The surface's own DynamicTexture (the native descriptor the HUD's allocation uses, its constructor copying
/// dimensions, format, samples and renderer flags).
unsafe fn create(s: &mut Surface, size: [u32; 2]) -> bool {
    if !s.texture.is_null() {
        return true;
    }
    #[repr(C)]
    struct Descriptor {
        width: u32,
        height: u32,
        mips: u8,
        samples: u8,
        format: u8,
        flags: u8,
        pad: u32,
        backing: *mut c_void,
    }
    const _: () = assert!(core::mem::size_of::<Descriptor>() == 24);
    let d = Descriptor {
        width: size[0],
        height: size[1],
        mips: 0,
        samples: 1,
        format: 6,
        flags: 0x14,
        pad: 0,
        backing: core::ptr::null_mut(),
    };
    type CreateFn = unsafe extern "C" fn(*mut Handle, *const Descriptor) -> *mut Handle;
    core::mem::transmute::<usize, CreateFn>(image_base() + CREATE_TEXTURE_RVA)(&mut s.texture, &d);
    if s.texture.is_null() {
        return false;
    }
    stat(0).fetch_add(1, Ordering::SeqCst);
    log(&format!(
        "[menu-surface] native target {}x{} component={:016X} texture={:016X}\n",
        size[0],
        size[1],
        s.component.lock().instance as usize,
        s.texture.instance as usize
    ));
    true
}

unsafe extern "C" fn add(manager: *mut c_void, owner: *mut c_void) -> u8 {
    let entry = if owner.is_null() { 0 } else { at::<usize>(owner as usize) };
    let component = component_of(entry);
    let size = size_of(component.instance as usize);
    if size[0] == 0 {
        return core::mem::transmute::<*mut c_void, AddFn>(ORIGINAL_ADD)(manager, owner);
    }
    let texture = {
        let made = with_state(|state| {
            let r = state.get(&component);
            let s = surface(&r);
            if create(s, size) {
                Some(s.texture.instance as usize)
            } else {
                None
            }
        });
        match made {
            Some(t) => t,
            None => return 0, // no low-resolution or uninitialised atlas fallback
        }
    };
    // A native atlas entry already binds its target to its ink window and mesh material: give it a whole owned
    // texture and full UVs instead of a shared slot.
    let rect = [0.0f32, 0.0, 1.0, 1.0];
    set(entry + 24, rect);
    set(entry + 40, rect);
    type Assign = unsafe extern "C" fn(usize, usize) -> usize;
    let entry_vtable = at::<usize>(entry);
    core::mem::transmute::<usize, Assign>(at::<usize>(entry_vtable + 2 * 8))(entry, texture);
    fit_window(entry, component.instance as usize);
    // Binding the texture must not wait for a disabled display to become visible (entry::UpdateBinding's wrapper API).
    let w = wrapper_of(entry);
    if w != 0 {
        let table = at::<usize>(w);
        type Uvs = unsafe extern "C" fn(usize, *const f32);
        core::mem::transmute::<usize, Uvs>(at::<usize>(table + 0x60))(w, rect.as_ptr());
        core::mem::transmute::<usize, Assign>(at::<usize>(table + 0x58))(w, texture);
    }
    1
}

unsafe extern "C" fn update(entry: *mut c_void, context: *mut c_void) -> usize {
    let entry_address = entry as usize;
    let component = component_of(entry_address);
    let own = size_of(component.instance as usize)[0] != 0;
    let enable_wrapper = || {
        let wrapper = wrapper_of(entry_address);
        if wrapper != 0 {
            set(wrapper + WRAPPER_ENABLED, 1u8);
        }
    };
    if own {
        // Streaming enables the wrapper only when its mesh becomes visible: keep it active while preparing.
        enable_wrapper();
    }
    let result = core::mem::transmute::<*mut c_void, UpdateFn>(ORIGINAL_UPDATE)(entry, context);
    if own && at::<usize>(entry_address + ENTRY_WINDOW) != 0 {
        enable_wrapper();
        let c = component.instance as usize;
        fit_window(entry_address, c);
        rebind(c, cvr_menu_surface_texture(component.instance) as usize);
        stat(1).fetch_add(1, Ordering::SeqCst);
        // ink paints while the display mesh is disabled, and these few targets stay live when looking away.
        set(entry_address + ENTRY_PAINT_DISTANCE, 100.0f32);
        (*((entry_address + ENTRY_PAINT) as *const core::sync::atomic::AtomicI32)).swap(1, Ordering::SeqCst);
        // Rebuilt every frame: a window painted from ink's cached draw data drew its atlas images at half strength
        // for a few frames every ~7 s.
        invalidate(at::<usize>(entry_address + ENTRY_WINDOW));
    }
    result
}

/// The two hooks (src/Natives/MenuSurface.cpp registers them at Boot, order 83), with the three engine functions
/// they call checked first.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_menu_surface_install() -> bool {
    if !cvr_engine::game_build::supported() {
        return false;
    }
    let base = image_base();
    for (rva, bytes) in [
        (CREATE_TEXTURE_RVA, CREATE_TEXTURE_PROLOGUE),
        (SET_SCALE_RVA, SET_SCALE_PROLOGUE),
        (INVALIDATE_RVA, INVALIDATE_PROLOGUE),
    ] {
        if !Pattern::parse(bytes).is_some_and(|p| expect(base + rva, &p)) {
            return false;
        }
    }
    let detours = [
        Detour {
            name: "world widget atlas add sub_1406FE754",
            target: ADD_RVA,
            absolute: false,
            detour: add as *const c_void,
            original: core::ptr::addr_of_mut!(ORIGINAL_ADD),
            expect: Some(ADD_PROLOGUE),
        },
        Detour {
            name: "world widget entry update sub_1406067C0",
            target: UPDATE_RVA,
            absolute: false,
            detour: update as *const c_void,
            original: core::ptr::addr_of_mut!(ORIGINAL_UPDATE),
            expect: Some(UPDATE_PROLOGUE),
        },
    ];
    if install_chain("menu-surface", &detours).is_err() {
        return false;
    }
    INSTALLED.store(true, Ordering::SeqCst);
    true
}

/// The D3D12 texture behind the surface's DynamicTexture (its backing holds a pool id, not a resource pointer).
unsafe fn find_resource(s: &mut Surface) -> bool {
    if s.resource.is_some() {
        return true;
    }
    if s.texture.is_null() {
        return false;
    }
    let base = image_base();
    let texture = s.texture.instance as usize;
    let backing = at::<usize>(texture + TEXTURE_BACKING);
    if backing == 0 || at::<usize>(backing) != base + BACKING_VTABLE {
        return false;
    }
    let id = at::<u32>(backing + 0x18);
    let pool = at::<usize>(base + TEXTURE_POOL);
    if pool == 0 || id == 0 || id > 131072 {
        return false;
    }
    let raw = at::<*mut c_void>(pool + 0x2F1D8 + 0xB0 * (id as usize - 1));
    let Some(resource) = ID3D12Resource::from_raw_borrowed(&raw) else {
        return false;
    };
    let d = resource.GetDesc();
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Width != at::<u32>(texture + TEXTURE_WIDTH) as u64
        || d.Height != at::<u32>(texture + TEXTURE_HEIGHT)
    {
        return false;
    }
    s.resource = Some(resource.clone());
    true
}

/// VRMenuSurfaceReady(component, root) -> Bool: the screen's first ink write has finished on its GPU queue. Called
/// after the script built the canvas: the first call arms the watch and asks ink to paint.
#[no_mangle]
pub unsafe extern "C" fn VRMenuSurfaceReady(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let (mut component, mut root) = (Handle::null(), Handle::null());
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut component);
        frame.get_parameter(&mut root);
        frame.finish_params();
    }
    let ready = ready(&component, &root);
    if let Some(out) = out.as_mut() {
        *out = ready;
    }
}
unsafe fn ready(component: &Handle, root: &Handle) -> bool {
    if !INSTALLED.load(Ordering::SeqCst)
        || component.is_null()
        || root.is_null()
        || !object_class(component.instance).is_some_and(|c| c.is_a(CName::hashed("WorldWidgetComponent")))
        || size_of(component.instance as usize)[0] == 0
    {
        return false;
    }
    with_state(|state| {
        let Some(r) = state.surfaces.get(&(component.instance as usize)).cloned() else {
            return false;
        };
        let s = surface(&r);
        if s.component.lock().instance != component.instance {
            return false;
        }
        if !find_resource(s) {
            return false;
        }
        stat(2).fetch_add(1, Ordering::SeqCst);
        let completed = s.fence.as_ref().map_or(0, |f| f.GetCompletedValue());
        if s.submitted != 0 && completed != u64::MAX && completed >= s.submitted {
            if !s.ready {
                stat(6).fetch_add(1, Ordering::SeqCst);
            }
            s.ready = true;
            return true;
        }
        if s.submitted != 0 {
            return false;
        }
        s.armed = true;
        OBSERVING.store(true, Ordering::SeqCst);
        stat(3).fetch_add(1, Ordering::SeqCst);
        // A real ink repaint: readiness needs that target's write to finish on its actual GPU queue.
        invalidate(root.instance as usize);
        false
    })
}

/// D3D12_RESOURCE_BARRIER with its transition member.
#[repr(C)]
struct Barrier {
    ty: u32,
    flags: u32,
    resource: *mut c_void,
    subresource: u32,
    before: u32,
    after: u32,
    _pad: u32,
}
const _: () = assert!(core::mem::size_of::<Barrier>() == 32);
const BARRIER_TYPE_TRANSITION: u32 = 0;
const BARRIER_FLAG_BEGIN_ONLY: u32 = 1;
/// RENDER_TARGET | COPY_DEST | UNORDERED_ACCESS.
const WRITE_STATES: u32 = 0x4 | 0x400 | 0x8;

fn record(state: &mut State, list: usize, r: &SurfaceRef) {
    stat(4).fetch_add(1, Ordering::SeqCst);
    let pending = state.recorded.entry(list).or_default();
    if !pending.iter().any(|x| Arc::ptr_eq(x, r)) {
        pending.push(r.clone());
    }
}
fn watched(s: &Surface, resource: *mut c_void) -> bool {
    s.armed && !s.ready && s.submitted == 0 && same_ptr(&s.resource, resource)
}

/// cvr::menu_surface::Barriers: a transition into or out of a write state of an armed surface records the list (native
/// ink targets enter RT from COMMON and decay after execution, so the whole producer list is fenced).
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_surface_barriers(list: *mut c_void, count: u32, barriers: *const c_void) {
    if !OBSERVING.load(Ordering::Relaxed) || barriers.is_null() {
        return;
    }
    let barriers = core::slice::from_raw_parts(barriers as *const Barrier, count as usize);
    for b in barriers {
        if b.ty != BARRIER_TYPE_TRANSITION {
            continue;
        }
        with_state(|state| {
            let hits: Vec<SurfaceRef> = state.surfaces.values().filter(|r| watched(surface(r), b.resource)).cloned().collect();
            for r in hits {
                if b.flags == BARRIER_FLAG_BEGIN_ONLY || (b.before | b.after) & WRITE_STATES == 0 {
                    continue;
                }
                record(state, list as usize, &r);
            }
        });
    }
}
/// cvr::menu_surface::ResetList.
#[no_mangle]
pub extern "C" fn cvr_menu_surface_reset_list(list: *mut c_void) {
    if !TRACKED.load(Ordering::Relaxed) {
        return;
    }
    with_state(|state| {
        state.recorded.remove(&(list as usize));
    });
}
/// cvr::menu_surface::Written: a write into an armed surface's texture records the list.
#[no_mangle]
pub extern "C" fn cvr_menu_surface_written(list: *mut c_void, resource: *mut c_void) {
    if !OBSERVING.load(Ordering::Relaxed) || resource.is_null() {
        return;
    }
    with_state(|state| {
        let hits: Vec<SurfaceRef> = state.surfaces.values().filter(|r| watched(surface(r), resource)).cloned().collect();
        for r in hits {
            record(state, list as usize, &r);
        }
    });
}
/// cvr::menu_surface::Waiting.
#[no_mangle]
pub extern "C" fn cvr_menu_surface_waiting() -> bool {
    OBSERVING.load(Ordering::Relaxed)
}
/// cvr::menu_surface::SurfaceTexture: the dedicated texture of one of the port's screens, null before it has one.
#[no_mangle]
pub extern "C" fn cvr_menu_surface_texture(component: *mut c_void) -> *mut c_void {
    if !INSTALLED.load(Ordering::SeqCst) || component.is_null() {
        return core::ptr::null_mut();
    }
    with_state(|state| match state.surfaces.get(&(component as usize)) {
        Some(r) if surface(r).component.lock().instance == component => surface(r).texture.instance,
        _ => core::ptr::null_mut(),
    })
}
/// cvr::menu_surface::Submitted: a recorded list submitted -- its surfaces get a fence signalled after it.
#[no_mangle]
pub unsafe extern "C" fn cvr_menu_surface_submitted(queue: *mut c_void, count: u32, lists: *const *mut c_void) {
    if !TRACKED.load(Ordering::Relaxed) || queue.is_null() || lists.is_null() {
        return;
    }
    let Some(queue) = ID3D12CommandQueue::from_raw_borrowed(&queue) else { return };
    let lists = core::slice::from_raw_parts(lists, count as usize);
    with_state(|state| {
        for &list in lists {
            let Some(recorded) = state.recorded.remove(&(list as usize)) else { continue };
            for r in &recorded {
                let s = surface(r);
                if s.submitted != 0 {
                    continue;
                }
                let mut device: Option<ID3D12Device> = None;
                if let (Ok(()), Some(device)) = (queue.GetDevice(&mut device), device.as_ref()) {
                    s.fence = device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE).ok();
                    if let Some(fence) = s.fence.as_ref() {
                        if queue.Signal(fence, 1).is_ok() {
                            s.submitted = 1;
                            stat(5).fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            }
        }
        let now = super::env::GetTickCount64();
        if now >= state.next_prune {
            state.prune();
            state.next_prune = now + 250;
        }
        let observing = state.surfaces.values().any(|r| {
            let s = surface(r);
            s.armed && s.submitted == 0 && !s.ready
        });
        OBSERVING.store(observing, Ordering::SeqCst);
        TRACKED.store(!state.surfaces.is_empty() || !state.recorded.is_empty(), Ordering::SeqCst);
    });
}
