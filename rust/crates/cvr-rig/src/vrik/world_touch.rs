//! Physical contacts on the game's own world screens (formerly src/Natives/WorldTouch.cpp): the game's mesh/UV picker
//! answers where a fingertip meets a screen -- mesh-bound curved displays included -- rather than an assumed rectangle
//! or the gaze. Only the world-screen picker's call to the triangle leaf (0x29EFD49 -> 0x20FB94) is redirected, through
//! src/Hooks/WorldTouchTriangleThunk.asm: the leaf has a stronger-than-Win64 register contract, the thunk keeps all of
//! its outputs and hands the hit point to `CvrWorldTouchTriangleAfter`. The registration stays in
//! src/Natives/WorldTouch.cpp.
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use cvr_engine::code::{image_base, read_bytes};
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::object_class;
use red4ext_abi::stack::StackFrame;

type Vector4 = [f32; 4];

extern "C" {
    static g_menuModeValue: i32;
    fn cvr_cpp_vrmenu_captures_buttons() -> bool;
    fn cvr_cpp_vrui_captures_input() -> bool;
    fn cvr_cpp_vrmenu_finger_world(side: i32, world: *mut f32) -> bool;
    fn CvrWorldTouchTriangleThunk();
}
#[link(name = "kernel32")]
extern "system" {
    fn VirtualProtect(address: *mut c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

/// The triangle leaf the thunk calls (its EXTERN).
#[no_mangle]
pub static mut CvrWorldTouchTriangleOriginal: usize = 0;

const TRIANGLE: usize = 0x20FB94;
const MESH_PICK: usize = 0x29EF804;
const PLANE_PICK: usize = 0x29F006C;
const PICKER_CALL: usize = 0x29EFD49;
/// Verified in the 2.5 database matching the installed executable.
const TRIANGLE_PROLOGUE: [u8; 13] = [0x40, 0x55, 0x48, 0x8B, 0xEC, 0x48, 0x83, 0xEC, 0x40, 0x48, 0x8B, 0x45, 0x30];
const MESH_PROLOGUE: [u8; 11] = [0x48, 0x8B, 0xC4, 0x4C, 0x89, 0x48, 0x20, 0x4C, 0x89, 0x40, 0x18];
const PLANE_PROLOGUE: [u8; 15] = [0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x18, 0x48, 0x89, 0x70, 0x20, 0x48, 0x89, 0x48, 0x08];

/// ink::Widget (RED4ext.SDK inkWidget.hpp) and ent::IComponent::owner.
const WIDGET_PARENT: usize = 0x168;
const WIDGET_OPACITY: usize = 0x1F0;
const WIDGET_VISIBLE: usize = 0x1F8;
const COMPONENT_OWNER: usize = 0x50;

/// The world-widget hit record (see keypad.rs).
#[repr(C)]
#[derive(Default)]
struct WorldHit {
    window: WeakHandle,
    component: WeakHandle,
    local: [f32; 2],
    cursor: [f32; 2],
    screen: [f32; 2],
}
const _: () = assert!(core::mem::size_of::<WorldHit>() == 56);
#[repr(C)]
struct Ray {
    origin: [f32; 3],
    direction: [f32; 3],
}
#[repr(C)]
#[derive(Default)]
struct Contact {
    hit: bool,
    point: Vector4,
}
/// The pickers return a C++ bool: only its low byte is defined.
type PickFn = unsafe extern "C" fn(*mut c_void, *const Handle, *const Ray, *mut WorldHit, *mut bool) -> u8;

static MESH: AtomicUsize = AtomicUsize::new(0);
static PLANE: AtomicUsize = AtomicUsize::new(0);
static READY: AtomicBool = AtomicBool::new(false);
thread_local! {
    static QUERY_CONTACT: Cell<*mut Contact> = const { Cell::new(core::ptr::null_mut()) };
}

fn matches(at: usize, want: &[u8]) -> bool {
    let mut got = vec![0u8; want.len()];
    read_bytes(at, &mut got) && got == want
}

/// Points the picker's call at a relay within rel32 reach that jumps to the thunk.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_world_touch_install() -> bool {
    if !cvr_engine::game_build::supported() || cvr_engine::game_build::overridden() {
        return false;
    }
    let base = image_base();
    if !matches(base + TRIANGLE, &TRIANGLE_PROLOGUE) || !matches(base + MESH_PICK, &MESH_PROLOGUE) || !matches(base + PLANE_PICK, &PLANE_PROLOGUE) {
        return false;
    }
    // Patch ONLY the world-screen picker's call.
    let site = base + PICKER_CALL;
    let mut call = [0u8; 5];
    if !read_bytes(site, &mut call) || call[0] != 0xE8 {
        return false;
    }
    let rel = i32::from_le_bytes([call[1], call[2], call[3], call[4]]);
    if (site as i64 + 5 + rel as i64) as usize != base + TRIANGLE {
        return false;
    }
    let Some(relay) = cvr_engine::trampoline::allocate(site, 14) else {
        return false;
    };
    let delta = relay as i64 - (site as i64 + 5);
    if delta < i32::MIN as i64 || delta > i32::MAX as i64 {
        return false;
    }
    // jmp [rip+0] / the thunk.
    let mut jump = [0u8; 14];
    jump[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    jump[6..].copy_from_slice(&(CvrWorldTouchTriangleThunk as *const () as u64).to_le_bytes());
    core::ptr::copy_nonoverlapping(jump.as_ptr(), relay as *mut u8, jump.len());
    FlushInstructionCache(GetCurrentProcess(), relay as *const c_void, jump.len());
    let mut old = 0u32;
    if VirtualProtect(site as *mut c_void, 5, 0x40, &mut old) == 0 {
        return false;
    }
    CvrWorldTouchTriangleOriginal = base + TRIANGLE;
    core::ptr::copy_nonoverlapping((delta as i32).to_le_bytes().as_ptr(), (site + 1) as *mut u8, 4);
    FlushInstructionCache(GetCurrentProcess(), site as *const c_void, 5);
    let mut ignored = 0u32;
    VirtualProtect(site as *mut c_void, 5, old, &mut ignored);
    MESH.store(base + MESH_PICK, Ordering::Relaxed);
    PLANE.store(base + PLANE_PICK, Ordering::Relaxed);
    READY.store(true, Ordering::Release);
    true
}

/// Called by the thunk after the leaf with its full result register (only the low byte is the bool) and its hit.
#[no_mangle]
pub unsafe extern "C" fn CvrWorldTouchTriangleAfter(found: usize, hit: *const Vector4) {
    let contact = QUERY_CONTACT.with(|c| c.get());
    if found as u8 != 0 && !contact.is_null() {
        (*contact).hit = true;
        (*contact).point = *hit;
    }
}

unsafe fn at<T: Copy>(object: *const c_void, offset: usize) -> T {
    core::ptr::read_unaligned((object as *const u8).add(offset) as *const T)
}
/// The widget and every parent up to the root are visible and not transparent.
unsafe fn visible(widget: &Handle) -> bool {
    let mut current = widget.clone();
    let mut i = 0;
    while !current.is_null() && i < 32 {
        let w = current.instance as *const c_void;
        if !at::<bool>(w, WIDGET_VISIBLE) || at::<f32>(w, WIDGET_OPACITY) <= 0.0 {
            return false;
        }
        current = (*((w as *const u8).add(WIDGET_PARENT) as *const WeakHandle)).lock();
        i += 1;
    }
    current.is_null()
}
unsafe fn is_a(object: *const c_void, name: &str) -> bool {
    !object.is_null() && object_class(object).is_some_and(|c| c.is_a(CName::hashed(name)))
}
/// The mesh pick with this thread's contact published for the thunk, the previous one restored after.
unsafe fn mesh_pick(f: usize, component: &Handle, ray: &Ray, hit: &mut WorldHit, from_behind: &mut bool, contact: *mut Contact) -> bool {
    let previous = QUERY_CONTACT.with(|c| c.replace(contact));
    let found = core::mem::transmute::<usize, PickFn>(f)(core::ptr::null_mut(), component, ray, hit, from_behind) != 0;
    QUERY_CONTACT.with(|c| c.set(previous));
    found
}

/// VRWorldTouchPointer(component, centre, normal) -> array<Vector4>: the world widget component, its world centre and
/// outward unit normal (its current transform). Each hand: (native ink X, Y, signed metres in front of the REAL
/// screen, valid), followed by its contact point in world space. No click or button input is generated.
#[no_mangle]
pub unsafe extern "C" fn VRWorldTouchPointer(_this: *mut c_void, frame: *mut StackFrame, out: *mut DynArray<Vector4>, _unused: i64) {
    let mut component = Handle::null();
    let (mut centre, mut normal) = (Vector4::default(), Vector4::default());
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut component);
        frame.get_parameter(&mut centre);
        frame.get_parameter(&mut normal);
        frame.finish_params();
    }
    let Some(out) = out.as_mut() else { return };
    out.clear();
    for v in pointer(&component, centre, normal) {
        out.push(v);
    }
}
/// VRWorldTouchPointer's work: per hand (ink X, Y, signed metres in front of the real screen, valid) and its contact
/// point in world space; empty when nothing may be touched.
///
/// # Safety
/// On the game's main thread; `component` is null or a live component's handle.
pub unsafe fn pointer(component: &Handle, centre: Vector4, mut normal: Vector4) -> Vec<Vector4> {
    let mut out = Vec::with_capacity(4);
    if !READY.load(Ordering::Acquire)
        || component.is_null()
        || core::ptr::read_volatile(&raw const g_menuModeValue) != 0
        || cvr_cpp_vrmenu_captures_buttons()
        || cvr_cpp_vrui_captures_input()
        || !is_a(component.instance, "WorldWidgetComponent")
    {
        return out;
    }
    let n = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if !n.is_finite() || n < 0.9 || n > 1.1 {
        return out;
    }
    normal[0] /= n;
    normal[1] /= n;
    normal[2] /= n;
    // Drop points have a recessed screen: their script uses a 10 cm approach press and 13 cm rearm. Keep ordinary
    // touch screens at the original reach.
    let owner = at::<*const c_void>(component.instance, COMPONENT_OWNER);
    let reach = if is_a(owner, "DropPoint") { 0.18f32 } else { 0.075 };
    let (mesh, plane) = (MESH.load(Ordering::Relaxed), PLANE.load(Ordering::Relaxed));
    for side in 0..2 {
        let mut sample: Vector4 = [-1.0, -1.0, 0.0, 0.0];
        let mut point = Vector4::default();
        let mut finger = [0f32; 3];
        if cvr_cpp_vrmenu_finger_world(side, finger.as_mut_ptr()) {
            // Limit the search in front of the finger. A far hit cannot activate a button.
            let ray = Ray {
                origin: [finger[0] + normal[0] * 0.08, finger[1] + normal[1] * 0.08, finger[2] + normal[2] * 0.08],
                direction: [-normal[0], -normal[1], -normal[2]],
            };
            let mut hit = WorldHit::default();
            let mut contact = Contact::default();
            let mut from_behind = false;
            let mut found = mesh_pick(mesh, component, &ray, &mut hit, &mut from_behind, &mut contact);
            if found && !contact.hit {
                found = false;
            }
            if !found && core::mem::transmute::<usize, PickFn>(plane)(core::ptr::null_mut(), component, &ray, &mut hit, &mut from_behind) != 0 {
                // The native planar fallback refuses components with a mesh hit surface.
                let d = (finger[0] - centre[0]) * normal[0] + (finger[1] - centre[1]) * normal[1] + (finger[2] - centre[2]) * normal[2];
                contact.point = [finger[0] - normal[0] * d, finger[1] - normal[1] * d, finger[2] - normal[2] * d, 1.0];
                found = true;
            }
            let window = hit.window.lock();
            if found && !window.is_null() && visible(&window) && hit.component.instance == component.instance {
                point = contact.point;
                let d = (finger[0] - point[0]) * normal[0] + (finger[1] - point[1]) * normal[1] + (finger[2] - point[2]) * normal[2];
                if (hit.local[0] + hit.local[1] + d).is_finite() && (-0.035..=reach).contains(&d) {
                    sample = [hit.local[0], hit.local[1], d, 1.0];
                }
            }
        }
        out.push(sample);
        out.push(point);
    }
    out
}
