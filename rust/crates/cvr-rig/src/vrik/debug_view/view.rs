//! The collision view's lines (main's DebugView.cpp): what the script and PhysX put in the world, and the overlay draws
//! for both eyes (cvr-render's debug view reads them through cvr_debug_view_copy). The world's own shapes near a point
//! by PhysX's overlap query, each hit's shape drawn from its geometry; a body's real shape in a colour (the outline).
use super::*;
use cvr_settings::lc;

/// A line of the view, in the game's world, and its colour's index.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub color: u32,
}
pub const MAX_SEGMENTS: usize = 12000;
/// Colours by index, as the script passes them.
pub const WHITE: i32 = 0;
pub const GREEN: i32 = 1;
pub const RED: i32 = 2;
pub const BLUE: i32 = 3;
pub const YELLOW: i32 = 4;
pub const CYAN: i32 = 5;
pub const MAGENTA: i32 = 6;
pub const ORANGE: i32 = 7;
pub const WORLD_STATIC: i32 = 8;
pub const WORLD_DYNAMIC: i32 = 9;
pub const COLOR_COUNT: i32 = 10;

const SEG0: Segment = Segment { a: [0.0; 3], b: [0.0; 3], color: 0 };
/// A ray is sent once in many frames, in turn with the others. Each stays up a moment, or the view flickers while
/// nothing is wrong.
const MAX_HELD: usize = 1024;
const HELD_MS: u64 = 150;
const MAX_HITS: usize = 256;
const MAX_TRIANGLES: i32 = 1500;
const MAX_ACTOR_SHAPES: usize = 16;

pub(super) struct ViewState {
    back: [Segment; MAX_SEGMENTS],
    back_count: usize,
    held: [Segment; MAX_HELD],
    held_at: [u64; MAX_HELD],
    held_next: usize,
    geometry_count: [i32; 8],
    shape_samples: i32,
    scene_said: [i32; MAX_SCENES],
    scene_last: [i32; MAX_SCENES],
    shape_proved: bool,
}
pub(super) static VIEW: Mutex<ViewState> = Mutex::new(ViewState {
    back: [SEG0; MAX_SEGMENTS],
    back_count: 0,
    held: [SEG0; MAX_HELD],
    held_at: [0; MAX_HELD],
    held_next: 0,
    geometry_count: [0; 8],
    shape_samples: 0,
    scene_said: [0; MAX_SCENES],
    scene_last: [0; MAX_SCENES],
    shape_proved: false,
});
struct Front {
    segments: Vec<Segment>,
    stamp: u64,
}
static FRONT: Mutex<Front> = Mutex::new(Front { segments: Vec::new(), stamp: 0 });
pub(super) fn view() -> std::sync::MutexGuard<'static, ViewState> {
    VIEW.lock().unwrap_or_else(|e| e.into_inner())
}
fn now() -> u64 {
    super::super::physhands::tick_ms()
}

impl ViewState {
    pub(super) fn add(&mut self, a: &Vec3, b: &Vec3, color: i32) {
        if self.back_count >= MAX_SEGMENTS {
            return;
        }
        self.back[self.back_count] = Segment { a: *a, b: *b, color: color as u32 };
        self.back_count += 1;
    }
    pub(super) fn hold_ray(&mut self, a: &Vec3, b: &Vec3, color: i32) {
        self.held[self.held_next] = Segment { a: *a, b: *b, color: color as u32 };
        self.held_at[self.held_next] = now();
        self.held_next = (self.held_next + 1) % MAX_HELD;
    }
    /// The frame's lines out to the overlay (the held rays still up added), the next frame's begun.
    pub(super) fn flush(&mut self) {
        let t = now();
        for i in 0..MAX_HELD {
            if self.held_at[i] != 0 && t - self.held_at[i] <= HELD_MS {
                let s = self.held[i];
                self.add(&s.a, &s.b, s.color as i32);
            }
        }
        {
            let mut f = FRONT.lock().unwrap_or_else(|e| e.into_inner());
            f.segments.clear();
            f.segments.extend_from_slice(&self.back[..self.back_count]);
            f.stamp = now();
        }
        self.back_count = 0;
    }
}
/// Copy: the last finished frame's lines, or none once the script has stopped sending them (250 ms).
pub fn copy_segments(out: &mut [Segment]) -> usize {
    let f = FRONT.lock().unwrap_or_else(|e| e.into_inner());
    if f.segments.is_empty() || now() - f.stamp > 250 {
        return 0;
    }
    let n = f.segments.len().min(out.len());
    out[..n].copy_from_slice(&f.segments[..n]);
    n
}
/// cvr_debug_view_copy: the overlay's read of the view's lines (cvr-render's debug view).
///
/// # Safety
/// `out` holds `capacity` segments.
#[no_mangle]
pub unsafe extern "C" fn cvr_debug_view_copy(out: *mut Segment, capacity: i32) -> i32 {
    if out.is_null() || capacity <= 0 {
        return 0;
    }
    copy_segments(core::slice::from_raw_parts_mut(out, capacity as usize)) as i32
}

/// Lines drawn with a transform, PhysX's space taken to the world's by `offset`; `outline` overrides the colour.
pub(super) struct Draw<'a> {
    pub(super) st: &'a mut ViewState,
    pub(super) offset: Vec3,
    pub(super) outline: i32,
}
impl Draw<'_> {
    fn to_world(&self, t: &Transform, v: &Vec3) -> Vec3 {
        let r = apply(t, v);
        [r[0] + self.offset[0], r[1] + self.offset[1], r[2] + self.offset[2]]
    }
    fn line(&mut self, t: &Transform, a: &Vec3, b: &Vec3, color: i32) {
        let (wa, wb) = (self.to_world(t, a), self.to_world(t, b));
        self.st.add(&wa, &wb, color);
    }
    fn ring(&mut self, t: &Transform, center: &Vec3, u: &Vec3, v: &Vec3, radius: f32, color: i32, parts: i32, from: f32, to: f32) {
        let mut last = [0.0f32; 3];
        for i in 0..=parts {
            let angle = from + (to - from) * i as f32 / parts as f32;
            let (c, s) = (angle.cos() * radius, angle.sin() * radius);
            let p = [center[0] + u[0] * c + v[0] * s, center[1] + u[1] * c + v[1] * s, center[2] + u[2] * c + v[2] * s];
            if i != 0 {
                self.line(t, &last, &p, color);
            }
            last = p;
        }
    }
    pub(super) fn box_lines(&mut self, t: &Transform, h: &Vec3, color: i32) {
        let c = [
            [-h[0], -h[1], -h[2]],
            [h[0], -h[1], -h[2]],
            [h[0], h[1], -h[2]],
            [-h[0], h[1], -h[2]],
            [-h[0], -h[1], h[2]],
            [h[0], -h[1], h[2]],
            [h[0], h[1], h[2]],
            [-h[0], h[1], h[2]],
        ];
        for i in 0..4 {
            self.line(t, &c[i], &c[(i + 1) % 4], color);
            self.line(t, &c[i + 4], &c[(i + 1) % 4 + 4], color);
            self.line(t, &c[i], &c[i + 4], color);
        }
    }
    fn sphere_lines(&mut self, t: &Transform, r: f32, color: i32) {
        let tau = 2.0 * core::f32::consts::PI;
        self.ring(t, &[0.0; 3], &[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], r, color, 12, 0.0, tau);
        self.ring(t, &[0.0; 3], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0], r, color, 12, 0.0, tau);
        self.ring(t, &[0.0; 3], &[0.0, 0.0, 1.0], &[1.0, 0.0, 0.0], r, color, 12, 0.0, tau);
    }
    /// Along local X, as PhysX has them.
    fn capsule_lines(&mut self, t: &Transform, r: f32, h: f32, color: i32) {
        let pi = core::f32::consts::PI;
        let (y, z, x) = ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let (a, b) = ([-h, 0.0, 0.0], [h, 0.0, 0.0]);
        self.ring(t, &a, &y, &z, r, color, 8, 0.0, 2.0 * pi);
        self.ring(t, &b, &y, &z, r, color, 8, 0.0, 2.0 * pi);
        self.line(t, &[-h, r, 0.0], &[h, r, 0.0], color);
        self.line(t, &[-h, -r, 0.0], &[h, -r, 0.0], color);
        self.line(t, &[-h, 0.0, r], &[h, 0.0, r], color);
        self.line(t, &[-h, 0.0, -r], &[h, 0.0, -r], color);
        self.ring(t, &b, &y, &x, r, color, 4, 0.0, pi);
        self.ring(t, &b, &z, &x, r, color, 4, 0.0, pi);
        self.ring(t, &a, &x, &y, -r, color, 4, -pi / 2.0, pi / 2.0);
        self.ring(t, &a, &x, &z, -r, color, 4, -pi / 2.0, pi / 2.0);
    }
    unsafe fn convex_lines(&mut self, t: &Transform, g: &ConvexGeometry, color: i32) -> Option<()> {
        if g.mesh.is_null() {
            return Some(());
        }
        let verts = slot_call(g.mesh, CONVEX_GET_VERTICES, [0; 5])?;
        let indices = slot_call(g.mesh, CONVEX_GET_INDEX_BUFFER, [0; 5])?;
        let polygons = slot_call(g.mesh, CONVEX_GET_NB_POLYGONS, [0; 5])? as u32;
        if verts == 0 || indices == 0 {
            return Some(());
        }
        let vertex = |i: usize| read::<[f32; 3]>(verts + 12 * i);
        for p in 0..polygons.min(256) {
            let mut h = HullPolygon::default();
            if slot_call(g.mesh, CONVEX_GET_POLYGON, [p as usize, (&raw mut h) as usize, 0, 0, 0])? as u8 == 0 || h.count < 2 {
                continue;
            }
            for i in 0..h.count {
                let ia = read::<u8>(indices + (h.base + i) as usize)? as usize;
                let ib = read::<u8>(indices + (h.base + (i + 1) % h.count) as usize)? as usize;
                let (a, b) = (vertex(ia)?, vertex(ib)?);
                let s = g.scale;
                self.line(t, &[a[0] * s[0], a[1] * s[1], a[2] * s[2]], &[b[0] * s[0], b[1] * s[1], b[2] * s[2]], color);
            }
        }
        Some(())
    }
    unsafe fn triangle_lines(&mut self, t: &Transform, g: &TriangleGeometry, center: &Vec3, radius: f32, color: i32) -> Option<()> {
        if g.mesh.is_null() {
            return Some(());
        }
        let verts = slot_call(g.mesh, TRIANGLES_GET_VERTICES, [0; 5])?;
        let vertex_count = slot_call(g.mesh, TRIANGLES_GET_NB_VERTICES, [0; 5])? as u32;
        let triangles = slot_call(g.mesh, TRIANGLES_GET_NB_TRIANGLES, [0; 5])? as u32;
        let data = slot_call(g.mesh, TRIANGLES_GET_TRIANGLES, [0; 5])?;
        let mut flags = 0u8;
        slot_call(g.mesh, TRIANGLES_GET_FLAGS, [(&raw mut flags) as usize, 0, 0, 0, 0])?;
        if verts == 0 || data == 0 {
            return Some(());
        }
        // PxTriangleMeshFlag::e16_BIT_INDICES
        let short_indices = (flags & 2) != 0;
        let mut drawn = 0;
        let limit = radius * radius;
        for i in 0..triangles {
            if drawn >= MAX_TRIANGLES {
                break;
            }
            let mut k = [0u32; 3];
            for c in 0..3 {
                let at = (i * 3 + c as u32) as usize;
                k[c] = if short_indices { u32::from(read::<u16>(data + 2 * at)?) } else { read::<u32>(data + 4 * at)? };
            }
            if k.iter().any(|&v| v >= vertex_count) {
                continue;
            }
            let mut w = [[0.0f32; 3]; 3];
            let mut close = false;
            for c in 0..3 {
                let v = read::<[f32; 3]>(verts + 12 * k[c] as usize)?;
                w[c] = self.to_world(t, &[v[0] * g.scale[0], v[1] * g.scale[1], v[2] * g.scale[2]]);
                let d = [w[c][0] - center[0], w[c][1] - center[1], w[c][2] - center[2]];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < limit {
                    close = true;
                }
            }
            if !close {
                continue;
            }
            self.st.add(&w[0], &w[1], color);
            self.st.add(&w[1], &w[2], color);
            self.st.add(&w[2], &w[0], color);
            drawn += 1;
        }
        Some(())
    }
    /// A hit's shape in the view, from its own geometry. False when the shape's table is not the one these slots were
    /// read from: the world's shapes are off from then on.
    pub(super) unsafe fn draw_hit(&mut self, actor: P, shape: P, center: &Vec3, radius: f32) -> Option<bool> {
        if shape.is_null() || actor.is_null() {
            return Some(true);
        }
        if !self.st.shape_proved {
            let dll = physx_dll();
            let table = read::<usize>(shape as usize)?;
            let name = slot_call(shape, BASE_GET_CONCRETE_TYPE_NAME, [0; 5])?;
            let mut head = [0u8; 7];
            let named = name != 0 && read_bytes(name, &mut head) && &head == b"PxShape";
            let at = |slot: usize| read::<usize>(table + 8 * slot).map(|f| f.wrapping_sub(dll));
            if dll == 0 || !named || at(SHAPE_GET_LOCAL_POSE) != Some(SHAPE_GET_LOCAL_POSE_RVA) || at(SHAPE_GET_GEOMETRY_TYPE) != Some(SHAPE_GET_GEOMETRY_TYPE_RVA) {
                PROVED.store(-1, Relaxed);
                log!("[debugview] the shape's table is not the one these slots were read from: world shapes off\n");
                return Some(false);
            }
            self.st.shape_proved = true;
        }
        let global = global_pose(actor)?;
        let mut local = Transform::default();
        slot_call(shape, SHAPE_GET_LOCAL_POSE, [(&raw mut local) as usize, 0, 0, 0, 0])?;
        let at = apply(&global, &local.p);
        let t = Transform { q: multiply(&global.q, &local.q), p: at };
        let actor_kind = actor_type(actor)?;
        let color = if self.outline >= 0 {
            self.outline
        } else if actor_kind == 0 {
            WORLD_STATIC
        } else {
            WORLD_DYNAMIC
        };
        let geometry = slot_call(shape, SHAPE_GET_GEOMETRY_TYPE, [0; 5])? as i32;
        if (0..8).contains(&geometry) {
            self.st.geometry_count[geometry as usize] += 1;
        }
        if self.st.shape_samples < 12 {
            self.st.shape_samples += 1;
            let w = [at[0] + self.offset[0], at[1] + self.offset[1], at[2] + self.offset[2]];
            let d = [w[0] - center[0], w[1] - center[1], w[2] - center[2]];
            log!(
                "[debugview] a shape: geometry {}, actor type {}, in the world at ({:.2} {:.2} {:.2}), {:.2} m from the middle\n",
                geometry,
                actor_kind,
                w[0],
                w[1],
                w[2],
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            );
        }
        match geometry {
            GEOM_BOX => {
                let mut g = [0.0f32; 4];
                if slot_call(shape, SHAPE_GET_BOX, [g.as_mut_ptr() as usize, 0, 0, 0, 0])? as u8 != 0 {
                    self.box_lines(&t, &[g[1], g[2], g[3]], color);
                }
            }
            GEOM_SPHERE => {
                let mut g = [0.0f32; 2];
                if slot_call(shape, SHAPE_GET_SPHERE, [g.as_mut_ptr() as usize, 0, 0, 0, 0])? as u8 != 0 {
                    self.sphere_lines(&t, g[1], color);
                }
            }
            GEOM_CAPSULE => {
                let mut g = [0.0f32; 3];
                if slot_call(shape, SHAPE_GET_CAPSULE, [g.as_mut_ptr() as usize, 0, 0, 0, 0])? as u8 != 0 {
                    self.capsule_lines(&t, g[1], g[2], color);
                }
            }
            GEOM_CONVEX => {
                let mut g = ConvexGeometry::default();
                if slot_call(shape, SHAPE_GET_CONVEX, [(&raw mut g) as usize, 0, 0, 0, 0])? as u8 != 0 {
                    self.convex_lines(&t, &g, color)?;
                }
            }
            GEOM_TRIANGLES => {
                let mut g = TriangleGeometry::default();
                if slot_call(shape, SHAPE_GET_TRIANGLES, [(&raw mut g) as usize, 0, 0, 0, 0])? as u8 != 0 {
                    self.triangle_lines(&t, &g, center, radius, color)?;
                }
            }
            _ => {}
        }
        Some(true)
    }
    /// Every shape of an actor, if its bounds come near.
    unsafe fn actor_near(&mut self, actor: P, center: &Vec3, radius: f32) -> Option<i32> {
        let b = world_bounds(actor)?;
        let c = [center[0] - self.offset[0], center[1] - self.offset[1], center[2] - self.offset[2]];
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
        if !(dx * dx + dy * dy + dz * dz <= radius * radius) {
            return Some(0);
        }
        let mut shapes = [core::ptr::null_mut::<c_void>(); MAX_ACTOR_SHAPES];
        let n = actor_shapes(actor, &mut shapes, 0)?;
        let mut drawn = 0;
        for &s in &shapes[..n] {
            self.draw_hit(actor, s, center, radius)?;
            if PROVED.load(Relaxed) < 0 {
                return Some(drawn);
            }
            drawn += 1;
        }
        Some(drawn)
    }
}

pub(super) const BASE_GET_CONCRETE_TYPE_NAME: usize = 1;
pub(super) const SHAPE_GET_GEOMETRY_TYPE: usize = 7;
pub(super) const SHAPE_GET_BOX: usize = 10;
pub(super) const SHAPE_GET_SPHERE: usize = 11;
pub(super) const SHAPE_GET_CAPSULE: usize = 12;
pub(super) const SHAPE_GET_CONVEX: usize = 14;
pub(super) const SHAPE_GET_TRIANGLES: usize = 15;
pub(super) const SHAPE_GET_LOCAL_POSE: usize = 19;
pub(super) const SHAPE_GET_LOCAL_POSE_RVA: usize = 0x66910;
pub(super) const SHAPE_GET_GEOMETRY_TYPE_RVA: usize = 0x66250;
const CONVEX_GET_VERTICES: usize = 6;
const CONVEX_GET_INDEX_BUFFER: usize = 7;
const CONVEX_GET_NB_POLYGONS: usize = 8;
const CONVEX_GET_POLYGON: usize = 9;
const TRIANGLES_GET_NB_VERTICES: usize = 5;
const TRIANGLES_GET_VERTICES: usize = 6;
const TRIANGLES_GET_NB_TRIANGLES: usize = 9;
const TRIANGLES_GET_TRIANGLES: usize = 10;
const TRIANGLES_GET_FLAGS: usize = 11;
pub(super) const GEOM_SPHERE: i32 = 0;
pub(super) const GEOM_CAPSULE: i32 = 2;
pub(super) const GEOM_BOX: i32 = 3;
pub(super) const GEOM_CONVEX: i32 = 4;
pub(super) const GEOM_TRIANGLES: i32 = 5;
pub(super) const GEOM_HEIGHT_FIELD: i32 = 6;
#[repr(C)]
#[derive(Clone, Copy)]
struct ConvexGeometry {
    kind: i32,
    scale: Vec3,
    scale_rot: [f32; 4],
    mesh: P,
    flags: u8,
    pad: [u8; 3],
}
impl Default for ConvexGeometry {
    fn default() -> Self {
        ConvexGeometry { kind: 0, scale: [0.0; 3], scale_rot: [0.0; 4], mesh: core::ptr::null_mut(), flags: 0, pad: [0; 3] }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
struct TriangleGeometry {
    kind: i32,
    scale: Vec3,
    scale_rot: [f32; 4],
    flags: u8,
    pad: [u8; 3],
    mesh: P,
}
impl Default for TriangleGeometry {
    fn default() -> Self {
        TriangleGeometry { kind: 0, scale: [0.0; 3], scale_rot: [0.0; 4], flags: 0, pad: [0; 3], mesh: core::ptr::null_mut() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct HullPolygon {
    plane: [f32; 4],
    count: u16,
    base: u16,
}

// ---- the overlap query: PxHitCallback<PxOverlapHit>, the virtuals in this order, the members in this order
#[repr(C)]
#[derive(Clone, Copy)]
struct OverlapHit {
    actor: P,
    shape: P,
    face_index: u32,
    pad: u32,
}
const HIT0: OverlapHit = OverlapHit { actor: core::ptr::null_mut(), shape: core::ptr::null_mut(), face_index: 0, pad: 0 };
#[repr(C)]
struct CallbackTable {
    process_touches: unsafe extern "C" fn(*mut OverlapCallback, *const OverlapHit, u32) -> bool,
    finalize_query: unsafe extern "C" fn(*mut OverlapCallback),
    destroy: unsafe extern "C" fn(*mut OverlapCallback, u32) -> *mut OverlapCallback,
}
#[repr(C)]
struct OverlapCallback {
    table: *const CallbackTable,
    block: OverlapHit,
    has_block: bool,
    touches: *mut OverlapHit,
    max_touches: u32,
    touch_count: u32,
    reported: u32,
}
unsafe extern "C" fn process_touches(this: *mut OverlapCallback, _: *const OverlapHit, count: u32) -> bool {
    (*this).reported = count;
    false
}
unsafe extern "C" fn finalize_query(_: *mut OverlapCallback) {}
unsafe extern "C" fn destroy(this: *mut OverlapCallback, _: u32) -> *mut OverlapCallback {
    this
}
static CALLBACK_TABLE: CallbackTable = CallbackTable { process_touches, finalize_query, destroy };
#[repr(C)]
struct FilterData {
    word: [u32; 4],
    flags: u16,
    client: u8,
    pad: u8,
}

/// A query answered nothing: every actor of the scene, by its bounds. Slow, and only while the view is on.
unsafe fn actors_near(d: &mut Draw, sc: P, center: &Vec3, radius: f32) -> Option<(i32, u32)> {
    // PxActorTypeFlags has a copy constructor: by address, in this ABI
    let types: u16 = 1 | 2;
    let count = slot_call(sc, SCENE_GET_NB_ACTORS, [(&types as *const u16) as usize, 0, 0, 0, 0])? as u32;
    if count == 0 || count as usize > MAX_ACTORS {
        return Some((0, count));
    }
    let mut drawn = 0;
    for &a in scene_actors(sc, types, MAX_ACTORS, 0)? {
        if PROVED.load(Relaxed) < 0 {
            break;
        }
        drawn += d.actor_near(a, center, radius)?;
    }
    Some((drawn, count))
}
/// LOOSE PROPS ARE NOT AMONG THE SCENE'S ACTORS: they sit in aggregates (the proxy kind 10 of a ray's hit), which the
/// scene lists apart.
unsafe fn aggregates_near(d: &mut Draw, sc: P, center: &Vec3, radius: f32) -> Option<(i32, u32, u32)> {
    let count = slot_call(sc, SCENE_GET_NB_AGGREGATES, [0; 5])? as u32;
    if count == 0 || count as usize > MAX_ACTORS {
        return Some((0, count, 0));
    }
    let mut drawn = 0;
    let mut actors = 0u32;
    for &ag in scene_aggregates(sc)? {
        if PROVED.load(Relaxed) < 0 {
            break;
        }
        let (members, n) = aggregate_members(ag)?;
        actors += n as u32;
        for &m in &members[..n] {
            if PROVED.load(Relaxed) < 0 {
                break;
            }
            drawn += d.actor_near(m, center, radius)?;
        }
    }
    Some((drawn, count, actors))
}
const SCENE_GET_NB_ACTORS: usize = 18;
static mut HIT_BUFFER: [OverlapHit; MAX_HITS] = [HIT0; MAX_HITS];

/// The real collision shapes within `radius` of `center`, in the game's world, into the view: how many, or -1 when they
/// are out of reach (from then on).
pub fn world(center: &Vec3, radius: f32) -> i32 {
    if PROVED.load(Relaxed) < 0 {
        return -1;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    let mut st = view();
    // SAFETY: guarded calls into PhysX's scenes, queries and shapes.
    let total = unsafe { world_inner(&mut st, center, radius) };
    match total {
        Some(n) => n,
        None => {
            PROVED.store(-1, Relaxed);
            log!("[debugview] an exception reading PhysX: world shapes off\n");
            -1
        }
    }
}
unsafe fn world_inner(st: &mut ViewState, center: &Vec3, radius: f32) -> Option<i32> {
    let Some(scenes) = read_scenes() else {
        PROVED.store(-1, Relaxed);
        return Some(-1);
    };
    PROVED.store(1, Relaxed);
    #[repr(C)]
    struct SphereGeometry {
        kind: i32,
        radius: f32,
    }
    let sphere = SphereGeometry { kind: GEOM_SPHERE, radius };
    // static, dynamic, every hit a touch
    let filter = FilterData { word: [0; 4], flags: 1 | 2 | 32, client: 0, pad: 0 };
    let mut total = 0;
    for s in 0..scenes.count {
        // not the game's world: five planes each
        if scenes.owners[s].is_null() {
            continue;
        }
        let offset = owner_origin(scenes.owners[s])?;
        let mut d = Draw { st: &mut *st, offset, outline: -1 };
        let pose = Transform { q: [0.0, 0.0, 0.0, 1.0], p: [center[0] - offset[0], center[1] - offset[1], center[2] - offset[2]] };
        let mut callback = OverlapCallback {
            table: &CALLBACK_TABLE,
            block: HIT0,
            has_block: false,
            touches: (&raw mut HIT_BUFFER).cast(),
            max_touches: MAX_HITS as u32,
            touch_count: 0,
            reported: 0,
        };
        let sc = scenes.scenes[s];
        slot_call(
            sc,
            SCENE_OVERLAP,
            [(&sphere as *const SphereGeometry) as usize, (&pose as *const Transform) as usize, (&raw mut callback) as usize, (&filter as *const FilterData) as usize, 0],
        )?;
        let n = callback.touch_count.max(callback.reported) as i32;
        total += n;
        for i in 0..(n as usize).min(MAX_HITS) {
            let hit = HIT_BUFFER[i];
            d.draw_hit(hit.actor, hit.shape, center, radius)?;
            if PROVED.load(Relaxed) < 0 {
                return Some(-1);
            }
        }
        let (mut actors, mut aggregates, mut in_aggregates, mut by_actors) = (0u32, 0u32, 0u32, 0);
        if n == 0 {
            let (drawn, count) = actors_near(&mut d, sc, center, radius)?;
            if PROVED.load(Relaxed) < 0 {
                return Some(-1);
            }
            by_actors = drawn;
            actors = count;
            total += by_actors;
        }
        let mut by_aggregates = 0;
        if n == 0 {
            let (drawn, count, inside) = aggregates_near(&mut d, sc, center, radius)?;
            by_aggregates = drawn;
            aggregates = count;
            in_aggregates = inside;
        }
        if PROVED.load(Relaxed) < 0 {
            return Some(-1);
        }
        total += by_aggregates;
        // Once for each scene, and again each of the first few times it has something new to show.
        let found = n + by_actors + by_aggregates;
        let st = &mut *d.st;
        if st.scene_said[s] < 12 && found != st.scene_last[s] {
            st.scene_said[s] += 1;
            st.scene_last[s] = found;
            let g = st.geometry_count;
            log!(
                "[debugview] scene {} at ({:.1} {:.1} {:.1}): query {} hits (touch {} reported {} block {}); {} actors, {} shapes; {} aggregates of {} actors, {} shapes; so far sphere {} plane {} capsule {} box {} convex {} mesh {} field {}\n",
                s,
                center[0],
                center[1],
                center[2],
                n,
                callback.touch_count,
                callback.reported,
                i32::from(callback.has_block),
                actors,
                by_actors,
                aggregates,
                in_aggregates,
                by_aggregates,
                g[0],
                g[1],
                g[2],
                g[3],
                g[4],
                g[5],
                g[6]
            );
        }
    }
    Some(total)
}

/// Outline: a body's real shape in the view's lines, in a colour. False if it has no actor.
pub fn outline(proxy: u32, body: u32, color: i32) -> bool {
    if PROVED.load(Relaxed) < 0 {
        return false;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    let mut st = view();
    // SAFETY: guarded calls into PhysX's lists, actors and shapes.
    unsafe {
        (|| -> Option<bool> {
            let scenes = read_scenes()?;
            for s in 0..scenes.count {
                if scenes.owners[s].is_null() {
                    continue;
                }
                let offset = owner_origin(scenes.owners[s])?;
                let sc = scenes.scenes[s];
                let mut found = find_in_actors(sc, 2, proxy, body)?;
                if found.is_none() {
                    found = find_in_aggregates(sc, proxy, body)?;
                }
                let Some(a) = found else { continue };
                let mut shapes = [core::ptr::null_mut::<c_void>(); MAX_ACTOR_SHAPES];
                let n = actor_shapes(a, &mut shapes, 0)?;
                let mut d = Draw { st: &mut st, offset, outline: color };
                for &shape in &shapes[..n] {
                    let pose = global_pose(a)?;
                    d.draw_hit(a, shape, &pose.p, 5.0)?;
                }
                return Some(true);
            }
            Some(false)
        })()
        .unwrap_or(false)
    }
}

/// The view's switch (the overlay's checkbox).
pub fn on() -> bool {
    lc!(xrDebugView) != 0
}
