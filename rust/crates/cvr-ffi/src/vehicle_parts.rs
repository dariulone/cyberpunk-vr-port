//! main's VRVehiclePartNear (src/Natives/VehicleHands.cpp): which moving panel of a car a point is at -- each enabled
//! physical mesh component of the vehicle, its rig binding followed (a component bound to another one, a mirror, too)
//! to the slot it hangs from (door_front_left .. hood, or a door's window), the point's distance to that mesh's scaled
//! local bounds. The indices are EVehicleDoor's. Only numbers are kept: the mesh's bounds, cached by its path (its
//! token given back at once, a mesh still streaming never waited for).
use crate::native_args::args;
use core::ffi::c_void;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::resource::TokenRef;
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

const COMPONENT_NAME: usize = 0x40;
const COMPONENT_ENABLED: usize = 0x8B;
const ENTITY_COMPONENTS: usize = 0xA0;
const PLACED_PARENT: usize = 0x90; // entIPlacedComponent: Handle<entITransformBinding>
const PLACED_WORLD: usize = 0xE0; // WorldTransform
const MESH_PATH: usize = 0x150; // entMeshComponent: RaRef<CMesh>
const MESH_SCALE: usize = 0x178; // Vector3
const MESH_CHUNKS: usize = 0x198; // u64
const BINDING_NAME: usize = 0x68;
const HARD_SLOT: usize = 0x70;
const CMESH_BOX: usize = 0x40; // Box: Min, Max (Vector4 each)

unsafe fn at<T: Copy>(base: *const c_void, offset: usize) -> T {
    unsafe { core::ptr::read_unaligned((base as *const u8).add(offset) as *const T) }
}
fn is_a(object: *const c_void, class: &str) -> bool {
    // SAFETY: a live object.
    !object.is_null() && unsafe { red4ext_abi::rtti::object_class(object) }.is_some_and(|c| c.is_a(CName::hashed(class)))
}
fn components(entity: *const c_void) -> Vec<*mut c_void> {
    // SAFETY: entEntity's components (instance, counts).
    unsafe { &*((entity as *const u8).add(ENTITY_COMPONENTS) as *const red4ext_abi::dynarray::DynArray<[usize; 2]>) }
        .as_slice()
        .iter()
        .map(|h| h[0] as *mut c_void)
        .filter(|p| !p.is_null())
        .collect()
}
/// SlotPart: the EVehicleDoor index a slot name is (or the door a window is in).
pub(crate) fn slot_part(name: &str) -> i32 {
    const PREFIXES: [&str; 6] = ["door_front_left", "door_front_right", "door_back_left", "door_back_right", "trunk", "hood"];
    const WINDOWS: [&str; 4] = ["window_front_left", "window_front_right", "window_back_left", "window_back_right"];
    let matches = |p: &str| name == p || (name.starts_with(p) && name.as_bytes().get(p.len()) == Some(&b'_'));
    if let Some(i) = PREFIXES.iter().position(|p| matches(p)) {
        return i as i32;
    }
    WINDOWS.iter().position(|p| matches(p)).map_or(-1, |i| i as i32)
}
/// Part: the panel a placed component hangs from, along its hard transform bindings (at most 8 deep).
fn part(entity: *const c_void, mesh: *mut c_void) -> (i32, u64) {
    let all = components(entity);
    let mut current = mesh;
    for _ in 0..8 {
        if current.is_null() {
            break;
        }
        // SAFETY: the component's parent binding.
        let binding = unsafe { at::<*const c_void>(current, PLACED_PARENT) };
        if binding.is_null() || !is_a(binding, "entHardTransformBinding") {
            return (-1, 0);
        }
        // SAFETY: the binding's slot and the component it binds to.
        let (slot, bind) = unsafe { (at::<u64>(binding, HARD_SLOT), at::<u64>(binding, BINDING_NAME)) };
        let p = slot_part(CName(slot).text());
        if p >= 0 {
            return (p, slot);
        }
        current = all.iter().copied().find(|&c| unsafe { at::<u64>(c, COMPONENT_NAME) } == bind && is_a(c, "entIPlacedComponent")).unwrap_or(core::ptr::null_mut());
    }
    (-1, 0)
}
type Bounds = ([f32; 3], [f32; 3]);
static BOUNDS: Mutex<Vec<(u64, Bounds)>> = Mutex::new(Vec::new());
/// Bounds: the mesh's local box, cached by path (128), read only when its token is already loaded.
fn bounds(path: u64) -> Option<Bounds> {
    if path == 0 {
        return None;
    }
    if let Some(b) = BOUNDS.lock().ok()?.iter().find(|(p, _)| *p == path).map(|(_, b)| *b) {
        return Some(b);
    }
    let token = TokenRef::load_async(path);
    if !token.loaded() || token.resource().is_null() {
        return None;
    }
    // SAFETY: the loaded CMesh's bounding box.
    let (lo, hi) = unsafe { (at::<[f32; 4]>(token.resource(), CMESH_BOX), at::<[f32; 4]>(token.resource(), CMESH_BOX + 16)) };
    drop(token);
    let b = ([lo[0], lo[1], lo[2]], [hi[0], hi[1], hi[2]]);
    if (0..3).any(|k| !b.0[k].is_finite() || !b.1[k].is_finite() || b.1[k] < b.0[k]) {
        return None;
    }
    let mut cache = BOUNDS.lock().ok()?;
    if cache.len() >= 128 {
        cache.remove(0);
    }
    cache.push((path, b));
    Some(b)
}
/// Distance: the point to the mesh's scaled bounds, in its own frame (a negative scale allowed; it only chooses).
fn distance(mesh: *const c_void, b: &Bounds, point: &[f32; 4]) -> f32 {
    // SAFETY: the component's world transform and visual scale.
    let (wt, scale) = unsafe { (at::<[u32; 8]>(mesh, PLACED_WORLD), at::<[f32; 3]>(mesh, MESH_SCALE)) };
    let wt = crate::blade_natives::WorldTransform(wt);
    let q = wt.orientation();
    let norm = q.iter().map(|v| v * v).sum::<f32>();
    if !norm.is_finite() || (norm - 1.0).abs() > 0.02 {
        return f32::INFINITY;
    }
    let local = crate::blade_natives::to_local(&wt, &[point[0], point[1], point[2]]);
    let mut d2 = 0.0;
    for k in 0..3 {
        if !local[k].is_finite() || !scale[k].is_finite() || scale[k].abs() < 0.0001 {
            return f32::INFINITY;
        }
        let (a, c) = (b.0[k] * scale[k], b.1[k] * scale[k]);
        let d = local[k] - local[k].clamp(a.min(c), a.max(c));
        d2 += d * d;
    }
    d2.sqrt()
}
/// VRVehiclePartNear's work: (the panel, its distance, 0, 1) for the nearest panel within `radius` (15 cm at most),
/// else zeros; with the slot the panel hangs from.
pub(crate) fn part_near(entity: &Handle, point: &[f32; 4], radius: f32) -> ([f32; 4], u64) {
    let e = entity.instance;
    if e.is_null() || !is_a(e, "vehicleCarBaseObject") || !radius.is_finite() || radius <= 0.0 {
        return ([0.0; 4], 0);
    }
    let mut best = radius.min(0.15);
    let mut out = ([0.0; 4], 0);
    for c in components(e) {
        // SAFETY: the component's fields.
        if !unsafe { at::<bool>(c, COMPONENT_ENABLED) } || !is_a(c, "entPhysicalMeshComponent") || unsafe { at::<u64>(c, MESH_CHUNKS) } == 0 {
            continue;
        }
        let (p, slot) = part(e, c);
        if p < 0 {
            continue;
        }
        // SAFETY: the mesh's path.
        let Some(b) = bounds(unsafe { at::<u64>(c, MESH_PATH) }) else { continue };
        let d = distance(c, &b, point);
        if d <= best {
            best = d;
            out = ([p as f32, d, 0.0, 1.0], slot);
        }
    }
    out
}
/// VRVehiclePartNear(entity, point, radius) -> Vector4.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRVehiclePartNear(_c: *mut c_void, frame: *mut StackFrame, out: *mut [f32; 4], _: i64) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    let point = a.get::<[f32; 4]>();
    let radius = a.get::<f32>();
    a.finish();
    if let Some(o) = unsafe { out.as_mut() } {
        *o = part_near(&entity, &point, radius).0;
    }
}
/// A panel's mesh in the world (tests): the centre of its bounds, and the outward normal of its thinnest side (away
/// from the vehicle's origin) with that side's half thickness.
pub(crate) fn part_face(entity: &Handle, wanted: i32) -> Option<([f32; 3], [f32; 3], f32)> {
    let e = entity.instance;
    for c in components(e) {
        // SAFETY: the component's fields.
        if !unsafe { at::<bool>(c, COMPONENT_ENABLED) } || !is_a(c, "entPhysicalMeshComponent") || unsafe { at::<u64>(c, MESH_CHUNKS) } == 0 {
            continue;
        }
        if part(e, c).0 != wanted {
            continue;
        }
        let Some(b) = bounds(unsafe { at::<u64>(c, MESH_PATH) }) else { continue };
        let (wt, scale) = unsafe { (crate::blade_natives::WorldTransform(at::<[u32; 8]>(c, PLACED_WORLD)), at::<[f32; 3]>(c, MESH_SCALE)) };
        let centre = [0, 1, 2].map(|k| (b.0[k] + b.1[k]) * 0.5 * scale[k]);
        let half = [0, 1, 2].map(|k| ((b.1[k] - b.0[k]) * 0.5 * scale[k]).abs());
        let thin = (0..3).min_by(|&i, &j| half[i].total_cmp(&half[j])).unwrap_or(0);
        let q = wt.orientation();
        let o = wt.position();
        let world = |v: [f32; 3]| {
            let r = crate::blade_natives::qrot(&q, &v);
            [r[0] + o[0], r[1] + o[1], r[2] + o[2]]
        };
        let c_world = world(centre);
        let mut n = [0.0f32; 3];
        n[thin] = 1.0;
        let mut nw = crate::blade_natives::qrot(&q, &n);
        // away from the vehicle's own origin
        let vp = super_position(entity);
        let away = [c_world[0] - vp[0], c_world[1] - vp[1], c_world[2] - vp[2]];
        if nw[0] * away[0] + nw[1] * away[1] + nw[2] * away[2] < 0.0 {
            nw = [-nw[0], -nw[1], -nw[2]];
        }
        return Some((c_world, nw, half[thin]));
    }
    None
}
fn super_position(entity: &Handle) -> [f32; 3] {
    // SAFETY: entEntity's transform component's world transform (+0xB0, +0xE0).
    unsafe {
        let t = at::<*const c_void>(entity.instance, 0xB0);
        if t.is_null() {
            return [0.0; 3];
        }
        crate::blade_natives::WorldTransform(at::<[u32; 8]>(t, PLACED_WORLD)).position()
    }
}
