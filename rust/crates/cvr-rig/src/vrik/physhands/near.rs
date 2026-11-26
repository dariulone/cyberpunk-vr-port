//! The world round the player, for the fingers (physics thread fills, worker reads). The game's scene answers no
//! overlap query, so the shapes near the player are found the way the census found them, by walking the scene's static
//! actors a slice a step, and each one near is copied: its geometry (a mesh's by its pointer, which lives as long as its
//! sector) and its pose. Statics only: what moves would be where it was a cycle ago, and a loose thing's mesh can go
//! with it.
use super::body::hand_filter;
use super::census::player_pos;
use super::math::{qmul, qrot};
use super::px::{self, at, PxBounds, PxTransform, P};
use super::tick_ms;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::RwLock;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

#[derive(Clone, Copy, Debug)]
#[repr(C, align(8))]
pub struct Geometry(pub [u8; 64]);
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct NearShape {
    pub geometry: Geometry,
    pub pose: PxTransform,
    pub bounds: PxBounds,
    pub kind: i32,
}
impl NearShape {
    pub const ZERO: NearShape = NearShape { geometry: Geometry([0; 64]), pose: PxTransform::ZERO, bounds: PxBounds { lo: [0.0; 3], hi: [0.0; 3] }, kind: 0 };
    pub fn geometry_ptr(&self) -> *const c_void {
        self.geometry.0.as_ptr().cast()
    }
    pub fn geometry_mut(&mut self) -> *mut c_void {
        self.geometry.0.as_mut_ptr().cast()
    }
}
impl Default for NearShape {
    fn default() -> NearShape {
        NearShape::ZERO
    }
}
pub const MOST_NEAR: usize = 1536;
pub struct NearList {
    pub s: [NearShape; MOST_NEAR],
    pub n: usize,
    pub center: [f32; 3],
    pub origin: [f32; 3],
    pub at: u64,
}
impl NearList {
    pub const NEW: NearList = NearList { s: [NearShape::ZERO; MOST_NEAR], n: 0, center: [0.0; 3], origin: [0.0; 3], at: 0 };
}
pub static mut near_lists: [NearList; 2] = [NearList::NEW, NearList::NEW];
/// the list the worker reads, -1 none yet
pub static NEAR_FRONT: AtomicI32 = AtomicI32::new(-1);
/// held shared by the worker while it reads the front; the swap takes it exclusively
pub static NEAR_LOCK: RwLock<()> = RwLock::new(());
struct NearWalk {
    phase: i32,
    next: u32,
    fill: usize,
    started_at: u64,
}
static mut near_walk: NearWalk = NearWalk { phase: 0, next: 0, fill: 0, started_at: 0 };
pub static NEAR_CYCLES: AtomicU64 = AtomicU64::new(0);
pub static NEAR_MOST_SHAPES: AtomicU64 = AtomicU64::new(0);
/// round the player's chest: both hands' reach, and a cycle's walking
pub const NEAR_RADIUS: f32 = 2.5;

/// PxGeometryQuery::getWorldBounds (PhysX3Common_x64.dll): a geometry's bounds at a pose.
pub type BoundsFn = unsafe extern "C" fn(geometry: *const c_void, pose: *const PxTransform, inflation: f32) -> PxBounds;
static WORLD_BOUNDS: AtomicUsize = AtomicUsize::new(0);
pub fn world_bounds_fn() -> Option<BoundsFn> {
    let mut f = WORLD_BOUNDS.load(Acquire);
    if f == 0 {
        // SAFETY: a module handle by name and its export by its decorated name.
        unsafe {
            let common = GetModuleHandleA(c"PhysX3Common_x64.dll".as_ptr().cast());
            if common.is_null() {
                return None;
            }
            f = GetProcAddress(common, c"?getWorldBounds@PxGeometryQuery@physx@@SA?AVPxBounds3@2@AEBVPxGeometry@2@AEBVPxTransform@2@M@Z".as_ptr().cast())
                .map_or(0, |p| p as usize);
        }
        if f == 0 {
            return None;
        }
        WORLD_BOUNDS.store(f, Release);
    }
    // SAFETY: the export's address.
    Some(unsafe { core::mem::transmute::<usize, BoundsFn>(f) })
}

fn out_of(c: f32, lo: f32, hi: f32) -> f32 {
    if c < lo {
        lo - c
    } else if c > hi {
        c - hi
    } else {
        0.0
    }
}
pub fn distance2_to_bounds(c: &[f32; 3], b: &PxBounds) -> f32 {
    (0..3).map(|k| out_of(c[k], b.lo[k], b.hi[k])).map(|e| e * e).sum()
}

/// A shape's geometry copied into `e` by its kind (the plane under the map has none to copy).
pub unsafe fn copy_geometry(sh: P, e: &mut NearShape) -> bool {
    let slot = match e.kind {
        px::GEOM_BOX => px::SHAPE_GET_BOX,
        px::GEOM_SPHERE => px::SHAPE_GET_SPHERE,
        px::GEOM_CAPSULE => px::SHAPE_GET_CAPSULE,
        px::GEOM_CONVEX => px::SHAPE_GET_CONVEX,
        px::GEOM_TRIANGLES => px::SHAPE_GET_TRIANGLES,
        px::GEOM_HEIGHT_FIELD => px::SHAPE_GET_HEIGHT_FIELD,
        _ => return false,
    };
    at::<unsafe extern "C" fn(P, *mut c_void) -> bool>(sh, slot)(sh, e.geometry_mut())
}

/// NearSlice: a slice a step of the statics within NEAR_RADIUS of the chest; the walked list handed to the worker when
/// it is not reading.
pub unsafe fn near_slice(sc: P, origin: &[f32; 3]) {
    const SLICE: usize = 1200;
    let now = tick_ms();
    // Do not publish a partial walk across an origin rebase or a long pause with both hands off.
    if near_walk.phase != 0 && (near_lists[near_walk.fill].origin != *origin || now - near_walk.started_at >= 5000) {
        near_walk.phase = 0;
    }
    if near_walk.phase == 0 {
        if world_bounds_fn().is_none() {
            return;
        }
        let front = NEAR_FRONT.load(Acquire);
        let p = player_pos();
        let mut center = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
        center[2] += 1.1;
        // THE WORLD STANDS STILL: walked again once the player has moved a third of the margin, or every few seconds (a
        // door, a sector come in); standing, the walk costs nothing
        if front >= 0 {
            let old = &near_lists[front as usize];
            let d2: f32 = (0..3).map(|k| (center[k] - old.center[k]) * (center[k] - old.center[k])).sum();
            if d2 < 0.5 * 0.5 && old.origin == *origin && now - old.at < 3000 {
                return;
            }
        }
        near_walk.fill = if front == 0 { 1 } else { 0 };
        let f = &mut near_lists[near_walk.fill];
        f.n = 0;
        f.center = center;
        f.origin = *origin;
        near_walk.next = 0;
        near_walk.phase = 1;
        near_walk.started_at = tick_ms();
        return;
    }
    let l = &mut near_lists[near_walk.fill];
    if near_walk.phase == 1 {
        let Some(world_bounds) = world_bounds_fn() else { return };
        let mut slice = [core::ptr::null_mut(); SLICE];
        let got = px::scene_actors(sc, 1, &mut slice, near_walk.next);
        let r2 = NEAR_RADIUS * NEAR_RADIUS;
        for &a in slice.iter().take(got as usize) {
            if l.n >= MOST_NEAR {
                break;
            }
            let b = px::world_bounds(a);
            if !(distance2_to_bounds(&l.center, &b) <= r2) {
                continue;
            }
            let mut global = PxTransform::default();
            at::<unsafe extern "C" fn(P, *mut PxTransform)>(a, px::GET_GLOBAL_POSE)(a, &mut global);
            // one actor can be a stretch of baked world of thousands of shapes: only those near are kept
            let mut start = 0u32;
            while start < 8192 && l.n < MOST_NEAR {
                let mut shapes = [core::ptr::null_mut(); 64];
                let n = at::<unsafe extern "C" fn(P, *mut P, u32, u32) -> u32>(a, px::GET_SHAPES)(a, shapes.as_mut_ptr(), 64, start);
                start += n;
                for &sh in shapes.iter().take(n as usize) {
                    if l.n >= MOST_NEAR {
                        break;
                    }
                    let mut flags = 0u8;
                    at::<unsafe extern "C" fn(P, *mut u8) -> P>(sh, px::SHAPE_GET_FLAGS)(sh, &mut flags);
                    if flags & 4 != 0 || flags & 1 == 0 {
                        continue; // a trigger, or not a simulation shape: the hand's box does not meet it
                    }
                    let w = px::sim_filter(sh);
                    let group = u64::from(w[0]) | (u64::from(w[1]) << 32);
                    let mask = u64::from(hand_filter[2]) | (u64::from(hand_filter[3]) << 32);
                    if group == !0 || group & mask == 0 {
                        continue; // what the hand's box does not meet, the fingers do not
                    }
                    let e = &mut l.s[l.n];
                    e.kind = px::geometry_type(sh);
                    if !copy_geometry(sh, e) {
                        continue;
                    }
                    let mut local = PxTransform::default();
                    at::<unsafe extern "C" fn(P, *mut PxTransform)>(sh, px::SHAPE_GET_LOCAL_POSE)(sh, &mut local);
                    let lp = qrot(&global.q, &local.p);
                    for k in 0..3 {
                        e.pose.p[k] = global.p[k] + lp[k];
                    }
                    e.pose.q = qmul(&global.q, &local.q);
                    e.bounds = world_bounds(e.geometry_ptr(), &e.pose, 1.0);
                    if distance2_to_bounds(&l.center, &e.bounds) <= r2 {
                        l.n += 1;
                    }
                }
                if n < 64 {
                    break;
                }
            }
        }
        near_walk.next += got;
        if (got as usize) < SLICE {
            near_walk.phase = 2;
        }
        return;
    }
    // done: handed to the worker when it is not reading, else tried again next step
    l.at = tick_ms();
    if let Ok(_g) = NEAR_LOCK.try_write() {
        NEAR_FRONT.store(near_walk.fill as i32, Release);
        drop(_g);
        NEAR_CYCLES.fetch_add(1, Relaxed);
        NEAR_MOST_SHAPES.fetch_max(l.n as u64, Relaxed);
        near_walk.phase = 0;
    }
}

/// The world scene changed: the world list's meshes were the old world's.
pub unsafe fn reset_for_scene() {
    {
        let _g = NEAR_LOCK.write().unwrap_or_else(|e| e.into_inner());
        NEAR_FRONT.store(-1, Release);
    }
    near_walk = NearWalk { phase: 0, next: 0, fill: 0, started_at: 0 };
}
