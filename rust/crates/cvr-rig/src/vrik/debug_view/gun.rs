//! A GUN AS BOXES OF ITS OWN SHAPE, against the world's own shapes (main's DebugView.cpp: GunScanSlice, GunQueryInner,
//! GunTether, GunMove, GunTouchInner). Rays from the wrist to a box's corners see nothing that stands between them; this
//! asks PhysX how far each box is inside each shape near it (PxGeometryQuery::computePenetration, exported by
//! PhysX3Common_x64.dll) and sweeps the boxes from where the gun was to where the hand wants it (PxGeometryQuery::sweep).
//! No scene query is involved.
//!
//! WALKING 15000 ACTORS A FRAME WOULD SHOW, so the near list is built a slice at a time, a cycle taking a few frames, and
//! only statics: what is loose gets pushed, not stopped at. It holds copies of the geometry, whose mesh pointers live as
//! long as their sector: so it is thrown away when the gun jumps or the calls have stopped, as they do over a loading
//! screen.
use super::view::{view, Draw, GEOM_BOX, GEOM_CAPSULE, GEOM_CONVEX, GEOM_HEIGHT_FIELD, GEOM_SPHERE, GEOM_TRIANGLES, ORANGE, RED};
use super::view::{BASE_GET_CONCRETE_TYPE_NAME, SHAPE_GET_BOX, SHAPE_GET_CAPSULE, SHAPE_GET_CONVEX, SHAPE_GET_GEOMETRY_TYPE, SHAPE_GET_LOCAL_POSE, SHAPE_GET_SPHERE, SHAPE_GET_TRIANGLES};
use super::view::{SHAPE_GET_GEOMETRY_TYPE_RVA, SHAPE_GET_LOCAL_POSE_RVA};
use super::*;
use cvr_engine::code::guarded_call10x;

const GUN_NEAR: usize = 2048;
const GUN_SLICE: usize = 1500;
const GUN_MOST_BOXES: usize = 12;
const GUN_ACTOR_SHAPES: usize = 64;
const GUN_MOST_TESTS: i32 = 1024;
const GUN_PASSES: i32 = 3;
const GUN_ACTOR_MOST_SHAPES: u32 = 8192;
const SHAPE_GET_SIM_FILTER_RVA: usize = 0x66a80;
const SHAPE_GET_QUERY_FILTER_RVA: usize = 0x66ae0;
const SHAPE_GET_FLAGS_RVA: usize = 0x67200;
const SHAPE_GET_HEIGHT_FIELD: usize = 16;
const SHAPE_GET_SIM_FILTER: usize = 21;
const SHAPE_GET_QUERY_FILTER: usize = 23;
const SHAPE_GET_FLAGS: usize = 34;
const GUN_MOST_TOUCHES: usize = 4;

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct Geometry([u8; 64]);
#[derive(Clone, Copy)]
struct NearShape {
    geometry: Geometry,
    pose: Transform,
    bounds: Bounds,
    origin: Vec3,
    query: [u32; 4],
    sim: [u32; 4],
    flags: u8,
    kind: i32,
}
const NEAR0: NearShape = NearShape {
    geometry: Geometry([0; 64]),
    pose: Transform { q: [0.0; 4], p: [0.0; 3] },
    bounds: Bounds { lo: [0.0; 3], hi: [0.0; 3] },
    origin: [0.0; 3],
    query: [0; 4],
    sim: [0; 4],
    flags: 0,
    kind: 0,
};
#[derive(Clone, Copy, Default)]
struct GunBox {
    center: Vec3,
    half: Vec3,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct GunTouch {
    pub proxy: u32,
    pub body: u32,
    pub away: Vec3,
    pub depth: f32,
}
#[repr(C)]
struct BoxGeometry {
    kind: i32,
    half: Vec3,
}
struct Gun {
    near: [NearShape; GUN_NEAR],
    next: [NearShape; GUN_NEAR],
    near_count: usize,
    next_count: usize,
    ready: bool,
    next_full: bool,
    boxes: [GunBox; GUN_MOST_BOXES],
    box_count: usize,
    radius: f32,
    scene: i32,
    start: u32,
    last_call: u64,
    last_center: Vec3,
    proved: i32,
    penetration: usize,
    bounds: usize,
    sweep: usize,
    hit_normal_at: i32,
    prev: Vec3,
    prev_on: bool,
    sweep_bad: bool,
    said: i32,
    shapes_most: u32,
    tethered: u32,
    gave_up: u32,
    anchor: Vec3,
    anchor_on: bool,
    touches: [GunTouch; GUN_MOST_TOUCHES],
    touch_count: usize,
    touch_total: f64,
    touch_calls: u32,
    push_total: f64,
    push_most: f64,
    push_calls: u32,
    said_at: u64,
}
static GUN: Mutex<Gun> = Mutex::new(Gun {
    near: [NEAR0; GUN_NEAR],
    next: [NEAR0; GUN_NEAR],
    near_count: 0,
    next_count: 0,
    ready: false,
    next_full: false,
    boxes: [GunBox { center: [0.0; 3], half: [0.0; 3] }; GUN_MOST_BOXES],
    box_count: 0,
    radius: 0.5,
    scene: -1,
    start: 0,
    last_call: 0,
    last_center: [0.0; 3],
    proved: 0,
    penetration: 0,
    bounds: 0,
    sweep: 0,
    hit_normal_at: -1,
    prev: [0.0; 3],
    prev_on: false,
    sweep_bad: false,
    said: 0,
    shapes_most: 0,
    tethered: 0,
    gave_up: 0,
    anchor: [0.0; 3],
    anchor_on: false,
    touches: [GunTouch { proxy: 0, body: 0, away: [0.0; 3], depth: 0.0 }; GUN_MOST_TOUCHES],
    touch_count: 0,
    touch_total: 0.0,
    touch_calls: 0,
    push_total: 0.0,
    push_most: 0.0,
    push_calls: 0,
    said_at: 0,
});
fn gun() -> std::sync::MutexGuard<'static, Gun> {
    GUN.lock().unwrap_or_else(|e| e.into_inner())
}
fn now() -> u64 {
    super::super::physhands::tick_ms()
}
fn extent_of(turn: &[f32; 4], half: &Vec3) -> Vec3 {
    let ax = rotate(turn, &[half[0], 0.0, 0.0]);
    let ay = rotate(turn, &[0.0, half[1], 0.0]);
    let az = rotate(turn, &[0.0, 0.0, half[2]]);
    [ax[0].abs() + ay[0].abs() + az[0].abs(), ax[1].abs() + ay[1].abs() + az[1].abs(), ax[2].abs() + ay[2].abs() + az[2].abs()]
}
fn bound_gap(c: &Vec3, b: &Bounds) -> f32 {
    let d = |k: usize| {
        if c[k] < b.lo[k] {
            b.lo[k] - c[k]
        } else if c[k] > b.hi[k] {
            c[k] - b.hi[k]
        } else {
            0.0
        }
    };
    let (dx, dy, dz) = (d(0), d(1), d(2));
    dx * dx + dy * dy + dz * dz
}

impl Gun {
    fn forget(&mut self) {
        self.near_count = 0;
        self.next_count = 0;
        self.ready = false;
        self.next_full = false;
        self.scene = -1;
        self.start = 0;
        self.prev_on = false;
    }
    unsafe fn prove_shape(&mut self, shape: P) -> bool {
        if self.proved != 0 {
            return self.proved > 0;
        }
        let dll = physx_dll();
        let table = read::<usize>(shape as usize).unwrap_or(0);
        let name = slot_call(shape, BASE_GET_CONCRETE_TYPE_NAME, [0; 5]).unwrap_or(0);
        let mut head = [0u8; 7];
        let named = name != 0 && read_bytes(name, &mut head) && &head == b"PxShape";
        let at = |slot: usize| read::<usize>(table + 8 * slot).map(|f| f.wrapping_sub(dll));
        let common = GetModuleHandleA(c"PhysX3Common_x64.dll".as_ptr().cast());
        let export = |name: &core::ffi::CStr| if common.is_null() { 0 } else { GetProcAddress(common, name.as_ptr().cast()).map_or(0, |f| f as usize) };
        self.penetration = export(c"?computePenetration@PxGeometryQuery@physx@@SA_NAEAVPxVec3@2@AEAMAEBVPxGeometry@2@AEBVPxTransform@2@23@Z");
        self.bounds = export(c"?getWorldBounds@PxGeometryQuery@physx@@SA?AVPxBounds3@2@AEBVPxGeometry@2@AEBVPxTransform@2@M@Z");
        self.sweep = export(c"?sweep@PxGeometryQuery@physx@@SA_NAEBVPxVec3@2@MAEBVPxGeometry@2@AEBVPxTransform@2@12AEAUPxSweepHit@2@V?$PxFlags@W4Enum@PxHitFlag@physx@@G@2@M@Z");
        if self.sweep == 0 {
            log!("[gunshape] no sweep: the gun is pushed out of what it is in\n");
        }
        if dll == 0
            || !named
            || at(SHAPE_GET_LOCAL_POSE) != Some(SHAPE_GET_LOCAL_POSE_RVA)
            || at(SHAPE_GET_GEOMETRY_TYPE) != Some(SHAPE_GET_GEOMETRY_TYPE_RVA)
            || at(SHAPE_GET_SIM_FILTER) != Some(SHAPE_GET_SIM_FILTER_RVA)
            || at(SHAPE_GET_QUERY_FILTER) != Some(SHAPE_GET_QUERY_FILTER_RVA)
            || at(SHAPE_GET_FLAGS) != Some(SHAPE_GET_FLAGS_RVA)
            || self.penetration == 0
            || self.bounds == 0
        {
            self.proved = -1;
            log!("[gunshape] this is not the PhysX these slots were read from (penetration {:#x}, bounds {:#x}): the gun stays on its rays\n", self.penetration, self.bounds);
            return false;
        }
        self.proved = 1;
        true
    }
    /// PxGeometryQuery::getWorldBounds(geometry, pose, inflation 0), returned through a hidden pointer.
    unsafe fn shape_bounds(&self, g: &Geometry, pose: &Transform) -> Option<Bounds> {
        let mut b = Bounds::default();
        guarded_call4x(self.bounds, [(&raw mut b) as u64, g as *const Geometry as u64, pose as *const Transform as u64, 0])?;
        Some(b)
    }
    unsafe fn penetrate(&self, geom0: *const c_void, pose0: &Transform, geom1: *const c_void, pose1: &Transform) -> Option<(Vec3, f32)> {
        let mut direction = [0.0f32; 3];
        let mut depth = 0.0f32;
        let hit = guarded_call6(
            self.penetration,
            [direction.as_mut_ptr().cast(), (&raw mut depth).cast(), geom0 as P, pose0 as *const Transform as P, geom1 as P, pose1 as *const Transform as P],
        )?;
        (hit as usize as u8 != 0 && depth > 0.0 && depth.is_finite()).then_some((direction, depth))
    }
    /// PxGeometryQuery::sweep(direction, distance, geom0, pose0, geom1, pose1, hit, flags by address, inflation 0).
    unsafe fn sweep_at(&self, dir: &Vec3, distance: f32, geom0: *const c_void, pose0: &Transform, geom1: *const c_void, pose1: &Transform, hit: &mut [u8; 128], flags: &u16) -> Option<bool> {
        let (rax, _) = guarded_call10x(
            self.sweep,
            [
                dir.as_ptr() as u64,
                u64::from(distance.to_bits()),
                geom0 as u64,
                pose0 as *const Transform as u64,
                geom1 as u64,
                pose1 as *const Transform as u64,
                hit.as_mut_ptr() as u64,
                flags as *const u16 as u64,
                0,
                0,
            ],
        )?;
        Some(rax as u8 != 0)
    }
    unsafe fn add_actor(&mut self, actor: P, center: &Vec3, radius: f32, origin: &Vec3) -> Option<()> {
        let whole = world_bounds(actor)?;
        let c = [center[0] - origin[0], center[1] - origin[1], center[2] - origin[2]];
        if !(bound_gap(&c, &whole) <= radius * radius) {
            return Some(());
        }
        let global = global_pose(actor)?;
        // AN ACTOR CAN HOLD HUNDREDS OF SHAPES, a stretch of baked world for one, and only what lies near is kept.
        let mut start = 0u32;
        while start < GUN_ACTOR_MOST_SHAPES {
            let mut shapes = [core::ptr::null_mut::<c_void>(); GUN_ACTOR_SHAPES];
            let n = actor_shapes(actor, &mut shapes, start)?;
            start += n as u32;
            self.shapes_most = self.shapes_most.max(start);
            for &shape in &shapes[..n] {
                if !self.prove_shape(shape) {
                    return Some(());
                }
                let mut flags = 0u8;
                slot_call(shape, SHAPE_GET_FLAGS, [(&raw mut flags) as usize, 0, 0, 0, 0])?;
                // a trigger, or nothing at all
                if (flags & 4) != 0 || (flags & 3) == 0 {
                    continue;
                }
                if self.next_count >= GUN_NEAR {
                    self.next_full = true;
                    return Some(());
                }
                let mut s = NEAR0;
                s.kind = slot_call(shape, SHAPE_GET_GEOMETRY_TYPE, [0; 5])? as i32;
                let getter = match s.kind {
                    GEOM_BOX => SHAPE_GET_BOX,
                    GEOM_SPHERE => SHAPE_GET_SPHERE,
                    GEOM_CAPSULE => SHAPE_GET_CAPSULE,
                    GEOM_CONVEX => SHAPE_GET_CONVEX,
                    GEOM_TRIANGLES => SHAPE_GET_TRIANGLES,
                    GEOM_HEIGHT_FIELD => SHAPE_GET_HEIGHT_FIELD,
                    _ => continue,
                };
                if slot_call(shape, getter, [s.geometry.0.as_mut_ptr() as usize, 0, 0, 0, 0])? as u8 == 0 {
                    continue;
                }
                let mut local = Transform::default();
                slot_call(shape, SHAPE_GET_LOCAL_POSE, [(&raw mut local) as usize, 0, 0, 0, 0])?;
                s.pose = Transform { q: multiply(&global.q, &local.q), p: apply(&global, &local.p) };
                s.bounds = self.shape_bounds(&s.geometry, &s.pose)?;
                if !(bound_gap(&c, &s.bounds) <= radius * radius) {
                    continue;
                }
                s.origin = *origin;
                s.flags = flags;
                slot_call(shape, SHAPE_GET_QUERY_FILTER, [s.query.as_mut_ptr() as usize, 0, 0, 0, 0])?;
                slot_call(shape, SHAPE_GET_SIM_FILTER, [s.sim.as_mut_ptr() as usize, 0, 0, 0, 0])?;
                self.next[self.next_count] = s;
                self.next_count += 1;
            }
            if n < GUN_ACTOR_SHAPES {
                break;
            }
        }
        Some(())
    }
    /// A slice of the near list. Scenes are read anew each time: the game drops them.
    unsafe fn scan_slice(&mut self, center: &Vec3) -> Option<()> {
        if PROVED.load(Relaxed) < 0 || self.proved < 0 {
            return Some(());
        }
        let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(scenes) = read_scenes() else { return Some(()) };
        if self.scene < 0 {
            self.scene = 0;
            self.start = 0;
            self.next_count = 0;
            self.next_full = false;
        }
        let radius = self.radius + 1.5;
        while (self.scene as usize) < scenes.count && scenes.owners[self.scene as usize].is_null() {
            self.scene += 1;
            self.start = 0;
        }
        if (self.scene as usize) < scenes.count {
            let s = self.scene as usize;
            let sc = scenes.scenes[s];
            let origin = owner_origin(scenes.owners[s])?;
            let actors = scene_actors(sc, 1, GUN_SLICE, self.start)?;
            let got = actors.len();
            for &a in actors {
                self.add_actor(a, center, radius, &origin)?;
            }
            self.start += got as u32;
            if got < GUN_SLICE {
                self.scene += 1;
                self.start = 0;
            }
        }
        if self.scene as usize >= scenes.count {
            let n = self.next_count;
            self.near[..n].copy_from_slice(&self.next[..n]);
            self.near_count = n;
            self.ready = self.proved > 0;
            if self.next_full && self.said < 3 {
                self.said += 1;
                log!("[gunshape] more than {} shapes near\n", GUN_NEAR);
            }
            self.scene = -1;
        }
        Some(())
    }
    /// How far the gun has to go to be out of everything near it, in the world.
    unsafe fn query(&mut self, at: &Vec3, turn: &[f32; 4]) -> Option<Vec3> {
        let mut push = [0.0f32; 3];
        let mut tests = 0;
        let g = Transform { q: *turn, p: *at };
        for _ in 0..GUN_PASSES {
            let mut moved = false;
            for b in 0..self.box_count {
                let bx = self.boxes[b];
                let middle = apply(&g, &bx.center);
                let extent = extent_of(turn, &bx.half);
                let geometry = BoxGeometry { kind: GEOM_BOX, half: bx.half };
                for i in 0..self.near_count {
                    let s = self.near[i];
                    let p = [middle[0] + push[0] - s.origin[0], middle[1] + push[1] - s.origin[1], middle[2] + push[2] - s.origin[2]];
                    if p[0] + extent[0] < s.bounds.lo[0]
                        || p[0] - extent[0] > s.bounds.hi[0]
                        || p[1] + extent[1] < s.bounds.lo[1]
                        || p[1] - extent[1] > s.bounds.hi[1]
                        || p[2] + extent[2] < s.bounds.lo[2]
                        || p[2] - extent[2] > s.bounds.hi[2]
                    {
                        continue;
                    }
                    tests += 1;
                    if tests > GUN_MOST_TESTS {
                        return Some(push);
                    }
                    let pose = Transform { q: *turn, p };
                    let Some((direction, depth)) = self.penetrate((&geometry as *const BoxGeometry).cast(), &pose, s.geometry.0.as_ptr().cast(), &s.pose) else { continue };
                    for k in 0..3 {
                        push[k] += direction[k] * depth;
                    }
                    moved = true;
                    if self.said < 40 {
                        self.said += 1;
                        log!(
                            "[gunshape] box {} in a shape, geometry {}, flags {}, {:.3} m, query {:08x} {:08x} {:08x} {:08x}, sim {:08x} {:08x} {:08x} {:08x}\n",
                            b,
                            s.kind,
                            s.flags,
                            depth,
                            s.query[0],
                            s.query[1],
                            s.query[2],
                            s.query[3],
                            s.sim[0],
                            s.sim[1],
                            s.sim[2],
                            s.sim[3]
                        );
                    }
                }
            }
            if !moved {
                break;
            }
        }
        Some(push)
    }
    /// THE WRIST'S SIDE OF THE WALL. Each box's middle has nothing between it and the wrist that holds it, or it has got
    /// behind that: by a turn pushed out at the far side of a thin wall, or by the hand going too far from where the gun
    /// stopped. It comes back to the near side, and the next sweep starts from there.
    unsafe fn tether(&mut self, pos: &mut Vec3, turn: &[f32; 4]) -> Option<()> {
        if !self.anchor_on || self.sweep == 0 || self.hit_normal_at < 0 {
            return Some(());
        }
        const THIN: f32 = 0.003;
        const BACK: f32 = 0.012;
        const FLAGS: u16 = 1 | 2 | 4 | 16 | 128 | 256;
        let thin = BoxGeometry { kind: GEOM_BOX, half: [THIN; 3] };
        let none = [0.0, 0.0, 0.0, 1.0];
        let mut tests = 0;
        for b in 0..self.box_count {
            if tests >= GUN_MOST_TESTS {
                break;
            }
            let middle = apply(&Transform { q: *turn, p: *pos }, &self.boxes[b].center);
            let way = [middle[0] - self.anchor[0], middle[1] - self.anchor[1], middle[2] - self.anchor[2]];
            let length = (way[0] * way[0] + way[1] * way[1] + way[2] * way[2]).sqrt();
            if length < 0.03 {
                continue;
            }
            let dir = [way[0] / length, way[1] / length, way[2] / length];
            let mut best = length;
            let mut hit = false;
            for i in 0..self.near_count {
                if tests >= GUN_MOST_TESTS {
                    break;
                }
                let s = self.near[i];
                let p = [self.anchor[0] - s.origin[0], self.anchor[1] - s.origin[1], self.anchor[2] - s.origin[2]];
                let q = [p[0] + way[0], p[1] + way[1], p[2] + way[2]];
                if p[0].min(q[0]) - THIN > s.bounds.hi[0]
                    || p[0].max(q[0]) + THIN < s.bounds.lo[0]
                    || p[1].min(q[1]) - THIN > s.bounds.hi[1]
                    || p[1].max(q[1]) + THIN < s.bounds.lo[1]
                    || p[2].min(q[2]) - THIN > s.bounds.hi[2]
                    || p[2].max(q[2]) + THIN < s.bounds.lo[2]
                {
                    continue;
                }
                tests += 1;
                let pose = Transform { q: none, p };
                let mut h = [0u8; 128];
                if !self.sweep_at(&dir, best, (&thin as *const BoxGeometry).cast(), &pose, s.geometry.0.as_ptr().cast(), &s.pose, &mut h, &FLAGS)? {
                    continue;
                }
                let at = self.hit_normal_at as usize;
                let f3 = f32::from_le_bytes(h[at + 12..at + 16].try_into().unwrap_or([0; 4]));
                if (0.0..=best).contains(&f3) && f3.is_finite() {
                    best = f3;
                    hit = true;
                }
            }
            if hit {
                let back = length - best + BACK;
                *pos = [pos[0] - dir[0] * back, pos[1] - dir[1] * back, pos[2] - dir[2] * back];
                self.tethered += 1;
            }
        }
        Some(())
    }
    /// THE GUN IS MOVED, FROM WHERE IT WAS TO WHERE THE HAND WANTS IT, and stopped and slid along what is in the way. This
    /// never crosses a surface, as a game's character controller does not. What it turned into this frame is still pushed
    /// out; and when the hand has gone further from it than an arm should, the gun comes, on the wrist's side.
    unsafe fn move_to(&mut self, target: &Vec3, turn: &[f32; 4]) -> Option<Vec3> {
        const SKIN: f32 = 0.002;
        const GIVE_UP: f32 = 1.0;
        // position, normal, distance, assume no overlap at the start, both sides of a mesh, precise
        const FLAGS: u16 = 1 | 2 | 4 | 16 | 128 | 256;
        if !self.prev_on || self.sweep == 0 || self.sweep_bad {
            let push = self.query(target, turn)?;
            let mut prev = [target[0] + push[0], target[1] + push[1], target[2] + push[2]];
            self.tether(&mut prev, turn)?;
            self.prev = prev;
            self.prev_on = true;
            return Some([prev[0] - target[0], prev[1] - target[1], prev[2] - target[2]]);
        }
        let mut pos = self.prev;
        let fix = self.query(&pos, turn)?;
        pos = [pos[0] + fix[0], pos[1] + fix[1], pos[2] + fix[2]];
        let mut left = [target[0] - pos[0], target[1] - pos[1], target[2] - pos[2]];
        let mut tests = 0;
        for _ in 0..3 {
            let dist = (left[0] * left[0] + left[1] * left[1] + left[2] * left[2]).sqrt();
            if dist < 1e-4 {
                break;
            }
            let dir = [left[0] / dist, left[1] / dist, left[2] / dist];
            let mut best = dist;
            let mut normal = [0.0f32; 3];
            let mut hit = false;
            let g = Transform { q: *turn, p: pos };
            for b in 0..self.box_count {
                if tests >= GUN_MOST_TESTS {
                    break;
                }
                let bx = self.boxes[b];
                let middle = apply(&g, &bx.center);
                let extent = extent_of(turn, &bx.half);
                let geometry = BoxGeometry { kind: GEOM_BOX, half: bx.half };
                for i in 0..self.near_count {
                    if tests >= GUN_MOST_TESTS {
                        break;
                    }
                    let s = self.near[i];
                    let p = [middle[0] - s.origin[0], middle[1] - s.origin[1], middle[2] - s.origin[2]];
                    let q = [p[0] + left[0], p[1] + left[1], p[2] + left[2]];
                    if p[0].min(q[0]) - extent[0] > s.bounds.hi[0]
                        || p[0].max(q[0]) + extent[0] < s.bounds.lo[0]
                        || p[1].min(q[1]) - extent[1] > s.bounds.hi[1]
                        || p[1].max(q[1]) + extent[1] < s.bounds.lo[1]
                        || p[2].min(q[2]) - extent[2] > s.bounds.hi[2]
                        || p[2].max(q[2]) + extent[2] < s.bounds.lo[2]
                    {
                        continue;
                    }
                    tests += 1;
                    let pose = Transform { q: *turn, p };
                    let mut h = [0u8; 128];
                    if !self.sweep_at(&dir, best, (&geometry as *const BoxGeometry).cast(), &pose, s.geometry.0.as_ptr().cast(), &s.pose, &mut h, &FLAGS)? {
                        continue;
                    }
                    let fl = |at: usize, k: usize| f32::from_le_bytes(h[at + 4 * k..at + 4 * k + 4].try_into().unwrap_or([0; 4]));
                    if self.hit_normal_at < 0 {
                        // The hit's layout differs from the headers (the normal is not at +36, measured): the first answer
                        // is searched for three floats of length one, with the distance after them, and that place kept.
                        let mut at = 16usize;
                        while at <= 64 && self.hit_normal_at < 0 {
                            let size = fl(at, 0) * fl(at, 0) + fl(at, 1) * fl(at, 1) + fl(at, 2) * fl(at, 2);
                            if size > 0.99 && size < 1.01 && fl(at, 3) >= 0.0 && fl(at, 3) <= best + 1e-3 {
                                self.hit_normal_at = at as i32;
                            }
                            at += 4;
                        }
                        log!("[gunshape] a sweep's normal is at +{}\n", self.hit_normal_at);
                        if self.hit_normal_at < 0 {
                            self.sweep_bad = true;
                            return self.query(target, turn);
                        }
                    }
                    let at = self.hit_normal_at as usize;
                    let n = [fl(at, 0), fl(at, 1), fl(at, 2)];
                    let size = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
                    let d = fl(at, 3);
                    if !(size > 0.9 && size < 1.1) || !(d >= 0.0) || !d.is_finite() {
                        continue;
                    }
                    if d <= best {
                        best = d;
                        normal = n;
                        hit = true;
                    }
                }
            }
            if !hit {
                pos = [pos[0] + left[0], pos[1] + left[1], pos[2] + left[2]];
                left = [0.0; 3];
                break;
            }
            let go = if best > SKIN { best - SKIN } else { 0.0 };
            pos = [pos[0] + dir[0] * go, pos[1] + dir[1] * go, pos[2] + dir[2] * go];
            left = [left[0] - dir[0] * go, left[1] - dir[1] * go, left[2] - dir[2] * go];
            let into = left[0] * normal[0] + left[1] * normal[1] + left[2] * normal[2];
            if into < 0.0 {
                left = [left[0] - normal[0] * into, left[1] - normal[1] * into, left[2] - normal[2] * into];
            }
        }
        let _ = left;
        let d = [pos[0] - target[0], pos[1] - target[1], pos[2] - target[2]];
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > GIVE_UP * GIVE_UP {
            self.gave_up += 1;
            pos = *target;
        }
        self.tether(&mut pos, turn)?;
        self.prev = pos;
        Some([pos[0] - target[0], pos[1] - target[1], pos[2] - target[2]])
    }
    /// WHAT THE GUN'S BOXES TOUCH OF WHAT IS LOOSE: asked of PhysX itself, every shape of every moving actor near, the
    /// same test as against the world. There are few of those, so they are walked fresh each call and nothing is kept.
    unsafe fn touch_actor(&mut self, actor: P, g: &Transform, origin: &Vec3) -> Option<()> {
        if actor_type(actor)? != 1 {
            return Some(());
        }
        // MOST OF THEM ARE NOWHERE NEAR, and where an actor stands costs far less to ask than its bounds. 4 m: what is
        // further and still reaches the gun is no loose thing.
        let place = global_pose(actor)?;
        let f = [place.p[0] + origin[0] - g.p[0], place.p[1] + origin[1] - g.p[1], place.p[2] + origin[2] - g.p[2]];
        if f[0] * f[0] + f[1] * f[1] + f[2] * f[2] > 16.0 {
            return Some(());
        }
        let whole = world_bounds(actor)?;
        let turn = g.q;
        for b in 0..self.box_count {
            let bx = self.boxes[b];
            let middle = apply(g, &bx.center);
            let extent = extent_of(&turn, &bx.half);
            let p = [middle[0] - origin[0], middle[1] - origin[1], middle[2] - origin[2]];
            if p[0] + extent[0] < whole.lo[0]
                || p[0] - extent[0] > whole.hi[0]
                || p[1] + extent[1] < whole.lo[1]
                || p[1] - extent[1] > whole.hi[1]
                || p[2] + extent[2] < whole.lo[2]
                || p[2] - extent[2] > whole.hi[2]
            {
                continue;
            }
            let mut shapes = [core::ptr::null_mut::<c_void>(); GUN_ACTOR_SHAPES];
            let n = actor_shapes(actor, &mut shapes, 0)?;
            let global = global_pose(actor)?;
            let geometry = BoxGeometry { kind: GEOM_BOX, half: bx.half };
            let pose = Transform { q: turn, p };
            for &shape in &shapes[..n] {
                let mut flags = 0u8;
                slot_call(shape, SHAPE_GET_FLAGS, [(&raw mut flags) as usize, 0, 0, 0, 0])?;
                // a trigger, or what does not collide
                if (flags & 4) != 0 || (flags & 1) == 0 {
                    continue;
                }
                let mut stored = Geometry([0; 64]);
                let getter = match slot_call(shape, SHAPE_GET_GEOMETRY_TYPE, [0; 5])? as i32 {
                    GEOM_BOX => SHAPE_GET_BOX,
                    GEOM_SPHERE => SHAPE_GET_SPHERE,
                    GEOM_CAPSULE => SHAPE_GET_CAPSULE,
                    GEOM_CONVEX => SHAPE_GET_CONVEX,
                    _ => continue,
                };
                if slot_call(shape, getter, [stored.0.as_mut_ptr() as usize, 0, 0, 0, 0])? as u8 == 0 {
                    continue;
                }
                let mut local = Transform::default();
                slot_call(shape, SHAPE_GET_LOCAL_POSE, [(&raw mut local) as usize, 0, 0, 0, 0])?;
                let shape_pose = Transform { q: multiply(&global.q, &local.q), p: apply(&global, &local.p) };
                let Some((direction, depth)) = self.penetrate((&geometry as *const BoxGeometry).cast(), &pose, stored.0.as_ptr().cast(), &shape_pose) else { continue };
                let (proxy, body) = ids_of(actor);
                if self.touches[..self.touch_count].iter().any(|t| t.proxy == proxy && t.body == body) {
                    return Some(());
                }
                if self.touch_count < GUN_MOST_TOUCHES {
                    self.touches[self.touch_count] = GunTouch { proxy, body, away: direction, depth };
                    self.touch_count += 1;
                }
                return Some(());
            }
        }
        Some(())
    }
    unsafe fn touch(&mut self, at: &Vec3, turn: &[f32; 4]) -> Option<usize> {
        self.touch_count = 0;
        if self.box_count == 0 || self.proved <= 0 {
            return Some(0);
        }
        let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
        let scenes = read_scenes()?;
        let g = Transform { q: *turn, p: *at };
        for s in 0..scenes.count {
            if scenes.owners[s].is_null() {
                continue;
            }
            let origin = owner_origin(scenes.owners[s])?;
            let sc = scenes.scenes[s];
            for &a in scene_actors(sc, 2, MAX_ACTORS, 0)? {
                self.touch_actor(a, &g, &origin)?;
            }
            for &ag in scene_aggregates(sc)? {
                let (members, n) = aggregate_members(ag)?;
                for &m in &members[..n] {
                    self.touch_actor(m, &g, &origin)?;
                }
            }
        }
        Some(self.touch_count)
    }
}

/// VRGunBoxClear: no boxes, the radius back to 30 cm.
pub fn box_clear() {
    let mut g = gun();
    g.box_count = 0;
    g.radius = 0.3;
    g.prev_on = false;
    g.touch_count = 0;
}
/// VRGunBoxAdd(lo, hi): one box of the gun in its own frame. False past twelve, or an empty one.
pub fn box_add(lo: &Vec3, hi: &Vec3) -> bool {
    let mut g = gun();
    if g.box_count >= GUN_MOST_BOXES || !(hi[0] > lo[0] && hi[1] > lo[1] && hi[2] > lo[2]) {
        return false;
    }
    let b = GunBox {
        center: [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5],
        half: [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5],
    };
    let n = g.box_count;
    g.boxes[n] = b;
    g.box_count += 1;
    let reach = (b.center[0] * b.center[0] + b.center[1] * b.center[1] + b.center[2] * b.center[2]).sqrt() + (b.half[0] * b.half[0] + b.half[1] * b.half[1] + b.half[2] * b.half[2]).sqrt();
    if reach > g.radius {
        g.radius = reach;
    }
    true
}
/// VRGunAnchor(the wrist that holds it, world): what the gun must be on this side of.
pub fn anchor(at: &Vec3) {
    let mut g = gun();
    g.anchor = *at;
    g.anchor_on = (at[0] + at[1] + at[2]).is_finite();
}
/// VRGunPush(at, turn) -> how far the gun must go to be out of the world's shapes: w 1 with an answer, 2 when moved by
/// sweeps (it cannot have gone through anything and no ray is needed), 0 while there is none yet (the near list takes a
/// few frames).
pub fn push(at: &Vec3, turn: &[f32; 4]) -> [f32; 4] {
    let mut g = gun();
    if g.box_count == 0 || PROVED.load(Relaxed) < 0 || g.proved < 0 {
        return [0.0; 4];
    }
    let (t0, f) = super::super::physhands::qpc();
    let t = now();
    let d = [at[0] - g.last_center[0], at[1] - g.last_center[1], at[2] - g.last_center[2]];
    if g.last_call == 0 || t - g.last_call > 500 || d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 1.0 {
        g.forget();
    }
    g.last_call = t;
    g.last_center = *at;
    // SAFETY: guarded calls into PhysX's lists, shapes and queries.
    let moved = unsafe {
        match g.scan_slice(at) {
            None => {
                g.forget();
                None
            }
            Some(()) if !g.ready => None,
            Some(()) => {
                let r = g.move_to(at, turn);
                if r.is_none() {
                    g.forget();
                }
                r
            }
        }
    };
    let out = match moved {
        Some(p) => [p[0], p[1], p[2], if g.prev_on && g.sweep != 0 && !g.sweep_bad { 2.0 } else { 1.0 }],
        None => [0.0; 4],
    };
    let (t1, _) = super::super::physhands::qpc();
    let ms = (t1 - t0) as f64 * 1000.0 / f as f64;
    g.push_total += ms;
    g.push_most = g.push_most.max(ms);
    g.push_calls += 1;
    if t - g.said_at > 10000 {
        log!(
            "[gunshape] {} boxes, {} shapes near, {} calls, {:.3} ms mean, {:.3} most; loose things {:.3} ms mean; brought back to the wrist's side {}, the hand too far from it {}\n",
            g.box_count,
            g.near_count,
            g.push_calls,
            g.push_total / f64::from(g.push_calls),
            g.push_most,
            if g.touch_calls != 0 { g.touch_total / f64::from(g.touch_calls) } else { 0.0 },
            g.tethered,
            g.gave_up
        );
        g.tethered = 0;
        g.gave_up = 0;
        g.touch_total = 0.0;
        g.touch_calls = 0;
        g.said_at = t;
        g.push_total = 0.0;
        g.push_most = 0.0;
        g.push_calls = 0;
    }
    if super::view::on() {
        // in the view: the boxes as they are after the push
        let p = [out[0], out[1], out[2]];
        let pushed = p != [0.0; 3];
        let gt = Transform { q: *turn, p: [at[0] + p[0], at[1] + p[1], at[2] + p[2]] };
        let mut st = view();
        let mut d = Draw { st: &mut st, offset: [0.0; 3], outline: -1 };
        for b in 0..g.box_count {
            let bx = g.boxes[b];
            d.box_lines(&Transform { q: *turn, p: apply(&gt, &bx.center) }, &bx.half, if pushed { RED } else { ORANGE });
        }
    }
    out
}
/// VRGunTouches(at, turn) -> how many loose bodies the gun's boxes are in now.
pub fn touches(at: &Vec3, turn: &[f32; 4]) -> i32 {
    let mut g = gun();
    let (t0, f) = super::super::physhands::qpc();
    // SAFETY: guarded calls into PhysX's lists and shapes.
    let n = unsafe { g.touch(at, turn) }.unwrap_or_else(|| {
        g.touch_count = 0;
        0
    });
    let (t1, _) = super::super::physhands::qpc();
    g.touch_total += (t1 - t0) as f64 * 1000.0 / f as f64;
    g.touch_calls += 1;
    n as i32
}
/// The i-th touch of the last VRGunTouches.
pub fn touch_of(i: i32) -> Option<GunTouch> {
    let g = gun();
    (i >= 0 && (i as usize) < g.touch_count).then(|| g.touches[i as usize])
}
