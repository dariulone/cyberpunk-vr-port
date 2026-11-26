//! The loose things round the player (physics thread): dynamic bodies of the game within reach of the hands, kept by
//! address with the userData the game gave each. A released actor keeps neither its table (the pool's free list is
//! written over it) nor, once its memory is reused, that userData, so both are checked before anything touches it (the
//! rule HeldBody's Alive keeps). For three things: none of them is thrown out of an overlap (its depenetration is
//! capped), the palm meets them again only once it is in none of them (as Bonelab's PhysHand.CheckFingerEnable gives
//! the fingers their collider back), and what stood on a thing taken off a stack comes down. And the kinematic
//! enclosures round the player, the stack drop, and the test stack.
use super::body::{actor, hand_filter, hand_filter_static, loose_from, made_centre, made_half, refilter, Enclosed, MOST_ENCLOSED_GROUPS};
use super::census::{listed, player_pos, MOST_LISTED};
use super::math::{qmul, qrot};
use super::near::{world_bounds_fn, NearShape};
use super::px::{self, at, BoxGeometry, PxBounds, PxTransform, P};
use super::seq::Seq;
use super::settings::{self, Settings};
use super::tick_ms;
use super::types::Step;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering::*};
use cvr_engine::code::{guarded_call4, guarded_call6, read};
use cvr_host::log;

#[derive(Clone, Copy)]
pub struct Loose {
    pub actor: P,
    pub user: u64,
    pub b: PxBounds,
    pub stacked: bool,
}
impl Loose {
    const ZERO: Loose = Loose { actor: core::ptr::null_mut(), user: 0, b: PxBounds { lo: [0.0; 3], hi: [0.0; 3] }, stacked: false };
}
pub const MOST_LOOSE: usize = 384;
pub static mut loose: [[Loose; MOST_LOOSE]; 2] = [[Loose::ZERO; MOST_LOOSE]; 2];
pub static mut loose_n: [usize; 2] = [0; 2];
/// the list in use; the walk fills the other
pub static mut loose_front: usize = 0;
struct LooseWalk {
    phase: i32,
    next: u32,
    at: u64,
    center: [f32; 3],
}
static mut loose_walk: LooseWalk = LooseWalk { phase: 0, next: 0, at: 0, center: [0.0; 3] };
/// the list whose things had their depenetration capped (its walk's time)
static mut loose_capped_at: u64 = 0;
pub static LOOSE_LISTED: AtomicU64 = AtomicU64::new(0);
pub static LOOSE_CAPPED: AtomicU64 = AtomicU64::new(0);
pub static PALM_REFUSED: AtomicU64 = AtomicU64::new(0);
pub static LOOSE_LET_THROUGH: AtomicU64 = AtomicU64::new(0);
pub static LOOSE_STACKED: AtomicU64 = AtomicU64::new(0);
/// as DebugView and HeldBody call it
pub const SCENE_GET_GRAVITY: usize = 67;

pub unsafe fn user_of(a: P) -> u64 {
    *((a as *const u8).add(0x10) as *const u64)
}
pub unsafe fn loose_alive(a: P, user: u64) -> bool {
    !a.is_null() && *(a as *const usize) == px::dll() + px::DYNAMIC_TABLE && user_of(a) == user
}

/// KINEMATIC ENCLOSURES: a kinematic body the palm meets whose bounds hold the player's chest is a volume round the
/// player, not a surface -- a lift car's solid 6 x 4.4 x 4 m block (group 18000, 38.9 t, met for the sake of people and
/// doors). The solver pushed the palm out of it at 0.25 m/s, 150 ms later the loose let-through put it back inside, and
/// both hands jumped 9 cm every 0.55 s standing still in an H10 lift (2026-11-16). While the palm is inside one the
/// hand's filters leave out that body's group bits (safe_to_drop keeps the world met). Listed by the loose walk (the
/// chest at its centre), bounds read each step.
#[derive(Clone, Copy)]
pub struct Enclosure {
    pub actor: P,
    pub user: u64,
}
pub const MOST_ENCLOSURES: usize = 8;
/// m: the bounds' shortest side at least, longest
pub const ENCLOSURE_LEAST: f32 = 1.0;
pub const ENCLOSURE_LONGEST: f32 = 2.0;
pub static mut enclosures: [[Enclosure; MOST_ENCLOSURES]; 2] = [[Enclosure { actor: core::ptr::null_mut(), user: 0 }; MOST_ENCLOSURES]; 2];
pub static mut enclosure_n: [usize; 2] = [0; 2];
pub static ENCLOSURE_STEPS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static mut enclosure_fill: usize = 1;

/// LOOSE THINGS FOR THE FINGERS: the shapes of the loose things by either hand, posed as the step left them (physics
/// thread, after the fetch), for the worker's finger solve -- a finger that brushes a bottle or a crate bends on it as
/// on the world. Only shapes with no mesh behind them: a convex hull goes as its own box (its local bounds), since the
/// worker reads the list a moment later and a thing's mesh can be gone by then.
pub const MOST_LOOSE_NEAR: usize = 48;
#[derive(Clone, Copy)]
pub struct LooseShapesNear {
    pub s: [NearShape; MOST_LOOSE_NEAR],
    pub n: usize,
    pub at: u64,
}
pub static LOOSE_SHAPES_NEAR: Seq<LooseShapesNear> = Seq::new(LooseShapesNear { s: [NearShape::ZERO; MOST_LOOSE_NEAR], n: 0, at: 0 });
/// Gu::ConvexMesh::getLocalBounds (PhysX3Common), as HeldBody reads it.
pub const CONVEX_LOCAL_BOUNDS: usize = 13;

/// `object`'s function at `slot` called (this, a, b) inside a guarded range: rax, or None when the table read or the
/// call faulted.
unsafe fn guarded_slot(object: P, slot: usize, a: *mut c_void, b: *mut c_void) -> Option<usize> {
    let table = read::<usize>(object as usize)?;
    let f = read::<usize>(table + 8 * slot)?;
    guarded_call4(f, object, a, b, core::ptr::null_mut()).map(|r| r as usize)
}

/// LooseShapeNear: one shape of a loose thing posed for the fingers (a convex hull as its box); false when it faults.
pub unsafe fn loose_shape_near(sh: P, g: &PxTransform, t: &mut NearShape) -> bool {
    let mut local = PxTransform::default();
    if guarded_slot(sh, px::SHAPE_GET_LOCAL_POSE, (&mut local as *mut PxTransform).cast(), core::ptr::null_mut()).is_none() {
        return false;
    }
    let Some(kind) = guarded_slot(sh, px::SHAPE_GET_GEOMETRY_TYPE, core::ptr::null_mut(), core::ptr::null_mut()) else { return false };
    let mut kind = kind as u32 as i32;
    t.geometry.0 = [0; 64];
    let get = |slot: usize, out: *mut c_void| guarded_slot(sh, slot, out, core::ptr::null_mut()).is_some_and(|r| r as u8 != 0);
    let mut ok = false;
    match kind {
        px::GEOM_BOX => ok = get(px::SHAPE_GET_BOX, t.geometry_mut()),
        px::GEOM_SPHERE => ok = get(px::SHAPE_GET_SPHERE, t.geometry_mut()),
        px::GEOM_CAPSULE => ok = get(px::SHAPE_GET_CAPSULE, t.geometry_mut()),
        px::GEOM_CONVEX => {
            #[repr(C, align(8))]
            struct Cg([u8; 64]);
            let mut cg = Cg([0; 64]);
            if get(px::SHAPE_GET_CONVEX, cg.0.as_mut_ptr().cast()) {
                let mesh = *(cg.0.as_ptr().add(32) as *const P);
                let scale = core::ptr::read_unaligned(cg.0.as_ptr().add(4) as *const [f32; 3]);
                if !mesh.is_null() {
                    let mut b = [0.0f32; 6];
                    if guarded_slot(mesh, CONVEX_LOCAL_BOUNDS, b.as_mut_ptr().cast(), core::ptr::null_mut()).is_none() {
                        return false;
                    }
                    let mut geometry = BoxGeometry { kind: px::GEOM_BOX, half: [0.0; 3] };
                    let mut centre = [0.0f32; 3];
                    for k in 0..3 {
                        geometry.half[k] = 0.5 * (b[3 + k] - b[k]) * scale[k].abs();
                        centre[k] = 0.5 * (b[3 + k] + b[k]) * scale[k];
                    }
                    let moved = qrot(&local.q, &centre);
                    for k in 0..3 {
                        local.p[k] += moved[k];
                    }
                    ok = geometry.half[0] > 0.0 && geometry.half[1] > 0.0 && geometry.half[2] > 0.0;
                    core::ptr::copy_nonoverlapping((&geometry as *const BoxGeometry).cast::<u8>(), t.geometry.0.as_mut_ptr(), core::mem::size_of::<BoxGeometry>());
                    kind = px::GEOM_BOX;
                }
            }
        }
        _ => {}
    }
    let Some(world_bounds) = world_bounds_fn() else { return false };
    if !ok {
        return false;
    }
    t.kind = kind;
    let lp = qrot(&g.q, &local.p);
    for k in 0..3 {
        t.pose.p[k] = g.p[k] + lp[k];
    }
    t.pose.q = qmul(&g.q, &local.q);
    t.bounds = world_bounds(t.geometry_ptr(), &t.pose, 1.0);
    true
}

pub unsafe fn global_pose(a: P) -> PxTransform {
    let mut g = PxTransform::default();
    at::<unsafe extern "C" fn(P, *mut PxTransform)>(a, px::GET_GLOBAL_POSE)(a, &mut g);
    g
}
pub unsafe fn shape_flags(sh: P) -> u8 {
    let mut f = 0u8;
    at::<unsafe extern "C" fn(P, *mut u8) -> P>(sh, px::SHAPE_GET_FLAGS)(sh, &mut f);
    f
}
pub unsafe fn is_sleeping(a: P) -> bool {
    at::<unsafe extern "C" fn(P) -> bool>(a, px::IS_SLEEPING)(a)
}
pub unsafe fn put_to_sleep(a: P) {
    at::<unsafe extern "C" fn(P)>(a, px::PUT_TO_SLEEP)(a);
}
pub unsafe fn scene_gravity(sc: P) -> [f32; 3] {
    let mut r = [0.0f32; 3];
    at::<unsafe extern "C" fn(P, *mut f32) -> P>(sc, SCENE_GET_GRAVITY)(sc, r.as_mut_ptr());
    r
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

static mut LOOSE_SNAP: LooseShapesNear = LooseShapesNear { s: [NearShape::ZERO; MOST_LOOSE_NEAR], n: 0, at: 0 };
/// PublishLooseNear: within 25 cm of a hand's body (as the step left it); a hand let through loose things
/// (loose_from) has none.
pub unsafe fn publish_loose_near(st: &Step) {
    let snap = &mut LOOSE_SNAP;
    snap.n = 0;
    let tick = tick_ms();
    let f = loose_front;
    for e in &loose[f][..loose_n[f]] {
        if snap.n >= MOST_LOOSE_NEAR {
            break;
        }
        if !loose_alive(e.actor, e.user) {
            continue;
        }
        let b = px::world_bounds(e.actor);
        let mut close = false;
        for side in 0..2 {
            if close {
                break;
            }
            if !st.valid[side] || tick < loose_from[side] {
                continue;
            }
            let d2: f32 = (0..3).map(|k| out_of(st.pose[side][4 + k], b.lo[k], b.hi[k])).map(|o| o * o).sum();
            close = d2 < 0.25 * 0.25;
        }
        if !close {
            continue;
        }
        let g = global_pose(e.actor);
        let mut list = [core::ptr::null_mut(); 4];
        let ns = px::shapes(e.actor, &mut list);
        for &sh in list.iter().take(ns.min(4) as usize) {
            if snap.n >= MOST_LOOSE_NEAR {
                break;
            }
            let i = snap.n;
            if loose_shape_near(sh, &g, &mut snap.s[i]) {
                snap.n += 1;
            }
        }
    }
    snap.at = tick;
    LOOSE_SHAPES_NEAR.write(snap);
}

static mut ENCLOSURE_TOLD: [P; 16] = [core::ptr::null_mut(); 16];
/// LooseSample: one dynamic actor, kept when it is near, not a hand's, not kinematic, not heavy, and a simulation shape
/// of it is one the palm meets. A kinematic one that encloses the chest goes to the enclosures instead.
unsafe fn loose_sample(a: P, center: &[f32; 3], fill: usize) {
    if loose_n[fill] >= MOST_LOOSE || a.is_null() || a == actor[0] || a == actor[1] {
        return;
    }
    if *(a as *const usize) != px::dll() + px::DYNAMIC_TABLE {
        return; // an articulation's link
    }
    let b = px::world_bounds(a);
    let d2: f32 = (0..3).map(|k| out_of(center[k], b.lo[k], b.hi[k])).map(|e| e * e).sum();
    if !(d2 <= 2.0 * 2.0) {
        return;
    }
    if loose[fill][..loose_n[fill]].iter().any(|e| e.actor == a) {
        return; // listed already (an aggregate's member is the scene's too)
    }
    let kinematic = px::rigid_body_flags(a) & 1 != 0;
    let mut enclosing = false;
    if kinematic {
        let (mut least, mut longest, mut holds) = (1e9f32, 0.0f32, true);
        for k in 0..3 {
            let e = b.hi[k] - b.lo[k];
            least = least.min(e);
            longest = longest.max(e);
            holds = holds && center[k] > b.lo[k] && center[k] < b.hi[k];
        }
        enclosing = holds && least >= ENCLOSURE_LEAST && longest >= ENCLOSURE_LONGEST && enclosure_n[enclosure_fill] < MOST_ENCLOSURES;
        if !enclosing {
            return;
        }
    } else if px::mass(a) > super::census::HEAVY {
        return;
    }
    let mut list = [core::ptr::null_mut(); 4];
    let ns = px::shapes(a, &mut list).min(4) as usize;
    let mask = u64::from(hand_filter[2]) | (u64::from(hand_filter[3]) << 32);
    let mut met = false;
    for &sh in &list[..ns] {
        if met {
            break;
        }
        if shape_flags(sh) & 1 == 0 {
            continue;
        }
        let w = px::sim_filter(sh);
        met = ((u64::from(w[0]) | (u64::from(w[1]) << 32)) & mask) != 0;
    }
    if enclosing {
        for &sh in &list[..ns] {
            if shape_flags(sh) & 1 == 0 {
                continue;
            }
            let geometry = px::geometry_type(sh);
            if geometry != px::GEOM_BOX && geometry != px::GEOM_CONVEX && geometry != px::GEOM_CAPSULE && geometry != px::GEOM_SPHERE {
                enclosing = false;
            }
        }
    }
    if enclosing {
        let told = &mut ENCLOSURE_TOLD;
        if !told.contains(&a) {
            if let Some(t) = told.iter_mut().find(|t| t.is_null()) {
                *t = a;
            }
            let w = if ns > 0 { px::sim_filter(list[0]) } else { [0; 4] };
            cvr_host::vlog!(
                "[physhands] enclosure {}: kinematic {:p}, bounds {:.2} x {:.2} x {:.2} m round the chest, group {:08x}{:08x}\n",
                if met { "listed" } else { "not met by the hands" },
                a,
                b.hi[0] - b.lo[0],
                b.hi[1] - b.lo[1],
                b.hi[2] - b.lo[2],
                w[1],
                w[0]
            );
        }
    }
    if !met {
        return;
    }
    if enclosing {
        let i = enclosure_n[enclosure_fill];
        enclosures[enclosure_fill][i] = Enclosure { actor: a, user: user_of(a) };
        enclosure_n[enclosure_fill] += 1;
        return;
    }
    let i = loose_n[fill];
    loose[fill][i] = Loose { actor: a, user: user_of(a), b, stacked: false };
    loose_n[fill] += 1;
}

/// STACKED: a thing whose bounds are most of the way inside a neighbour's (the stall's plates nest: a 9 cm bowl every 3
/// cm). Their shapes are in each other; asleep they stand, woken PhysX throws them apart and they break on the floor.
fn mark_stacked(l: &mut [Loose]) {
    for i in 0..l.len() {
        for j in i + 1..l.len() {
            let (a, b) = (l[i].b, l[j].b);
            let mut o = [0.0f32; 3];
            let mut s = [0.0f32; 3];
            for k in 0..3 {
                o[k] = a.hi[k].min(b.hi[k]) - a.lo[k].max(b.lo[k]);
                s[k] = (a.hi[k] - a.lo[k]).min(b.hi[k] - b.lo[k]);
            }
            if o[0] > 0.5 * s[0] && o[1] > 0.5 * s[1] && o[2] > 0.3 * s[2] {
                l[i].stacked = true;
                l[j].stacked = true;
            }
        }
    }
}

/// LooseSlice: a slice a step, as the census -- the scene's dynamic actors, then the aggregates' members. Walked again
/// after the player has moved 0.4 m or every 1.5 s.
pub unsafe fn loose_slice(sc: P, origin: &[f32; 3]) {
    const SLICE: usize = 400;
    let fill = 1 - loose_front;
    if loose_walk.phase == 0 {
        let p = player_pos();
        let mut center = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
        center[2] += 1.1;
        let d2: f32 = (0..3).map(|k| (center[k] - loose_walk.center[k]) * (center[k] - loose_walk.center[k])).sum();
        if loose_walk.at != 0 && d2 < 0.4 * 0.4 && tick_ms() - loose_walk.at < 1500 {
            return;
        }
        loose_walk.center = center;
        loose_n[fill] = 0;
        enclosure_fill = fill;
        enclosure_n[fill] = 0;
        loose_walk.next = 0;
        loose_walk.phase = 1;
        return;
    }
    if loose_walk.phase == 1 {
        let mut slice = [core::ptr::null_mut(); SLICE];
        let got = px::scene_actors(sc, 2, &mut slice, loose_walk.next);
        let center = loose_walk.center;
        for &a in slice.iter().take(got as usize) {
            loose_sample(a, &center, fill);
        }
        loose_walk.next += got;
        if (got as usize) < SLICE {
            loose_walk.next = 0;
            loose_walk.phase = 2;
        }
        return;
    }
    let mut aggregates = [core::ptr::null_mut(); SLICE / 8];
    let got = px::scene_aggregates(sc, &mut aggregates, loose_walk.next);
    let center = loose_walk.center;
    for &ag in aggregates.iter().take(got as usize) {
        let mut members = [core::ptr::null_mut(); 64];
        let n = px::aggregate_actors(ag, &mut members);
        for &m in members.iter().take(n.min(64) as usize) {
            if px::actor_type(m) == 1 {
                loose_sample(m, &center, fill);
            }
        }
    }
    loose_walk.next += got;
    if (got as usize) < SLICE / 8 {
        let n = loose_n[fill];
        mark_stacked(&mut loose[fill][..n]);
        loose_front = fill;
        loose_walk.at = tick_ms();
        loose_walk.phase = 0;
        LOOSE_LISTED.store(n as u64, Relaxed);
    }
}

/// LooseCap: each new list's things get the capped depenetration (once: the setting stays on the actor). A STACKED ONE
/// KEEPS ITS OVERLAPS: the cap only limits how fast PhysX undoes a penetration already there; stopping things from going
/// further into each other is the contact's job and stays. So woken a plate standing in the one under it stays standing.
pub unsafe fn loose_cap(s: &Settings) {
    if s.prop_depenetration <= 0.0 || loose_capped_at == loose_walk.at {
        return;
    }
    loose_capped_at = loose_walk.at;
    let f = loose_front;
    let (mut n, mut stacked) = (0u64, 0u64);
    for e in &loose[f][..loose_n[f]] {
        if !loose_alive(e.actor, e.user) {
            continue;
        }
        at::<unsafe extern "C" fn(P, f32)>(e.actor, px::SET_MAX_DEPENETRATION)(e.actor, if e.stacked { s.stack_depenetration } else { s.prop_depenetration });
        n += 1;
        stacked += u64::from(e.stacked);
    }
    LOOSE_CAPPED.store(n, Relaxed);
    LOOSE_STACKED.store(stacked, Relaxed);
}

/// The fingers' query faults (a mesh released under a list a cycle old), for the statistics.
pub static FINGER_FAULTS: AtomicU64 = AtomicU64::new(0);
/// Penetration: one PxGeometryQuery::computePenetration, a fault in it caught.
pub unsafe fn penetration(g0: *const c_void, p0: *const PxTransform, s: &NearShape) -> Option<([f32; 3], f32)> {
    let f = px::penetration()?;
    let mut dir = [0.0f32; 3];
    let mut depth = 0.0f32;
    let args = [
        dir.as_mut_ptr().cast::<c_void>(),
        (&mut depth as *mut f32).cast(),
        g0.cast_mut(),
        p0.cast_mut().cast(),
        s.geometry_ptr().cast_mut(),
        (&s.pose as *const PxTransform).cast_mut().cast(),
    ];
    match guarded_call6(f as usize, args) {
        None => {
            FINGER_FAULTS.fetch_add(1, Relaxed);
            None
        }
        Some(r) => (r as u8 != 0).then_some((dir, depth)),
    }
}

/// EnclosureGroups: the simulation groups of the enclosures whose bounds, as they are now, hold the palm (its box's
/// centre) and the chest.
pub unsafe fn enclosure_groups(side: usize, at_pose: &PxTransform, origin: Option<&[f32; 3]>) -> Enclosed {
    let mut out = Enclosed::default();
    let f = loose_front;
    let Some(origin) = origin else { return out };
    if enclosure_n[f] == 0 {
        return out;
    }
    let r = qrot(&at_pose.q, &made_centre[side]);
    let p = player_pos();
    let palm = [at_pose.p[0] + r[0], at_pose.p[1] + r[1], at_pose.p[2] + r[2]];
    let mut chest = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
    chest[2] += 1.1;
    for e in &enclosures[f][..enclosure_n[f]] {
        if out.n >= MOST_ENCLOSED_GROUPS {
            break;
        }
        if !loose_alive(e.actor, e.user) {
            continue;
        }
        let b = px::world_bounds(e.actor);
        let inside = (0..3).all(|k| palm[k] > b.lo[k] && palm[k] < b.hi[k] && chest[k] > b.lo[k] && chest[k] < b.hi[k]);
        if !inside {
            continue;
        }
        let mut list = [core::ptr::null_mut(); 4];
        let ns = px::shapes(e.actor, &mut list);
        let mut groups = 0u64;
        for &sh in list.iter().take(ns.min(4) as usize) {
            if shape_flags(sh) & 1 == 0 {
                continue;
            }
            let w = px::sim_filter(sh);
            groups |= u64::from(w[0]) | (u64::from(w[1]) << 32);
        }
        if groups != 0 {
            out.group[out.n] = groups;
            out.n += 1;
        }
    }
    out
}

/// PalmInLoose: the palm in a loose thing -- its box against each near one's shapes.
pub unsafe fn palm_in_loose(side: usize, at_pose: &PxTransform) -> bool {
    if px::penetration().is_none() {
        return false;
    }
    let geometry = BoxGeometry { kind: px::GEOM_BOX, half: made_half[side] };
    let r = qrot(&at_pose.q, &made_centre[side]);
    let pose = PxTransform { q: at_pose.q, p: [at_pose.p[0] + r[0], at_pose.p[1] + r[1], at_pose.p[2] + r[2]] };
    let h = geometry.half;
    let reach = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
    let f = loose_front;
    for e in &loose[f][..loose_n[f]] {
        if !loose_alive(e.actor, e.user) {
            continue;
        }
        let b = px::world_bounds(e.actor);
        if (0..3).any(|k| pose.p[k] + reach < b.lo[k] || pose.p[k] - reach > b.hi[k]) {
            continue;
        }
        let g = global_pose(e.actor);
        let mut list = [core::ptr::null_mut(); 4];
        let ns = px::shapes(e.actor, &mut list);
        for &sh in list.iter().take(ns.min(4) as usize) {
            let mut t = NearShape { kind: px::geometry_type(sh), ..NearShape::ZERO };
            let ok = matches!(t.kind, px::GEOM_BOX | px::GEOM_SPHERE | px::GEOM_CAPSULE | px::GEOM_CONVEX) && super::near::copy_geometry(sh, &mut t);
            if !ok {
                continue;
            }
            let mut local = PxTransform::default();
            at::<unsafe extern "C" fn(P, *mut PxTransform)>(sh, px::SHAPE_GET_LOCAL_POSE)(sh, &mut local);
            let lp = qrot(&g.q, &local.p);
            for k in 0..3 {
                t.pose.p[k] = g.p[k] + lp[k];
            }
            t.pose.q = qmul(&g.q, &local.q);
            if penetration((&geometry as *const BoxGeometry).cast(), &pose, &t).is_some_and(|(_, depth)| depth > 0.002) {
                return true;
            }
        }
    }
    false
}

// ---- what stood on a thing taken off a stack (physics thread) ----
// The game's loose things never meet each other (HeldBody, their filter): a stack stands because its things sleep. Taken
// out of the middle, the one above hung in the air, or, woken, fell through the rest to the table. So what stood on it
// comes down together by as much as the taken one held it up, and sleeps again there, as if that one had never been.
#[derive(Clone, Copy)]
struct Drop {
    actor: [P; 16],
    user: [u64; 16],
    rest: [f32; 16],
    n: usize,
    distance: f32,
    fallen: f32,
    speed: f32,
    watch_until: u64,
    said: i32,
    /// each thing's (first) shape, and its own filter while apart
    shape: [P; 16],
    saved: [[u32; 4]; 16],
    apart: [bool; 16],
}
impl Drop {
    const ZERO: Drop = Drop {
        actor: [core::ptr::null_mut(); 16],
        user: [0; 16],
        rest: [0.0; 16],
        n: 0,
        distance: 0.0,
        fallen: 0.0,
        speed: 0.0,
        watch_until: 0,
        said: 0,
        shape: [core::ptr::null_mut(); 16],
        saved: [[0; 4]; 16],
        apart: [false; 16],
    };
}
static mut drops: [Drop; 4] = [Drop::ZERO; 4];
/// WHAT CAME DOWN SLEEPS APART: the hands' group and the world's mask, so nothing loose reaches it. Given its own filter
/// back asleep, PhysX woke the plates standing in each other; woken and let fall by gravity, they slid off the counter.
/// It is given back when something else wakes it, or when it is taken (before HeldBody reads its filter).
#[derive(Clone, Copy)]
struct Apart {
    actor: P,
    user: u64,
    shape: P,
    saved: [u32; 4],
    rest: PxTransform,
    kept: i32,
}
const MOST_APART: usize = 64;
static mut aparts: [Apart; MOST_APART] = [Apart { actor: core::ptr::null_mut(), user: 0, shape: core::ptr::null_mut(), saved: [0; 4], rest: PxTransform::ZERO, kept: 0 }; MOST_APART];
static mut apart_n: usize = 0;
unsafe fn rejoin(i: usize) {
    if loose_alive(aparts[i].actor, aparts[i].user) {
        px::set_sim_filter(aparts[i].shape, &aparts[i].saved);
        refilter(aparts[i].actor);
    }
    apart_n -= 1;
    aparts[i] = aparts[apart_n];
}
pub unsafe fn rejoin_actor(a: P) {
    for i in (0..apart_n).rev() {
        if aparts[i].actor == a {
            rejoin(i);
        }
    }
}
/// KEPT WHERE IT WAS PUT: something wakes what came down a few steps after it is put to sleep (the game, its
/// aggregate), and apart from everything loose it would fall through a counter whose group no mask reaches. Woken, it is
/// brought back to its place and put to sleep again.
pub static APART_KEPT: AtomicU64 = AtomicU64::new(0);
fn gravity_of(r: [f32; 3]) -> Option<[f32; 3]> {
    ((r[0] + r[1] + r[2]).is_finite() && r[2].abs() > 1.0 && r[2].abs() < 30.0).then_some(r)
}
pub unsafe fn apart_step(sc: P, dt: f32) {
    let mut g = [0.0f32, 0.0, -9.81];
    let mut gravity = false;
    let zero = [0.0f32; 3];
    let mut i = apart_n;
    while i > 0 {
        i -= 1;
        let ap = &mut aparts[i];
        if !loose_alive(ap.actor, ap.user) {
            apart_n -= 1;
            aparts[i] = aparts[apart_n];
            continue;
        }
        if is_sleeping(ap.actor) {
            continue;
        }
        if !gravity {
            if let Some(r) = gravity_of(scene_gravity(sc)) {
                g = r;
            }
            gravity = true;
        }
        let at_pose = global_pose(ap.actor);
        let mut v = [0.0f32; 3];
        let mut d2 = 0.0f32;
        for k in 0..3 {
            let d = ap.rest.p[k] - at_pose.p[k];
            d2 += d * d;
            v[k] = d / dt - g[k] * dt;
        }
        if d2 < 0.001 * 0.001 {
            super::body::set_velocities(ap.actor, &zero, &zero, false);
            put_to_sleep(ap.actor);
            continue;
        }
        super::body::set_velocities(ap.actor, &v, &zero, true);
        ap.kept += 1;
        if ap.kept == 1 {
            APART_KEPT.fetch_add(1, Relaxed);
        }
    }
}
pub static DROPS_MADE: AtomicU64 = AtomicU64::new(0);

static mut TAKEN_SAID: i32 = 0;
/// LogTaken: a take, said once each (sixteen): the taken actor's flags and sleep, for knowing what the game's loose
/// things are.
unsafe fn log_taken(a: P) {
    TAKEN_SAID += 1;
    if TAKEN_SAID > 16 {
        return;
    }
    let flags = px::rigid_body_flags(a);
    let b = px::world_bounds(a);
    log!(
        "[physhands] taken: actor {:p}, userData {:016x}, {:.2} kg, flags {:02x}, asleep {}, {:.3} x {:.3} x {:.3} m\n",
        a,
        user_of(a),
        px::mass(a),
        flags,
        i32::from(is_sleeping(a)),
        b.hi[0] - b.lo[0],
        b.hi[1] - b.lo[1],
        b.hi[2] - b.lo[2]
    );
}

/// TakenFromStack: what stood on a thing a hand takes comes down together, straight, by as much as the taken one held
/// it up, apart from everything loose, and is put to sleep there.
pub unsafe fn taken_from_stack(taken: P) {
    rejoin_actor(taken);
    let s = settings::current();
    if s.stack_drop == 0 || !px::prove() || taken.is_null() || *(taken as *const usize) != px::dll() + px::DYNAMIC_TABLE {
        return;
    }
    log_taken(taken);
    let t = px::world_bounds(taken);
    let th = t.hi[2] - t.lo[2];
    let t_area = (t.hi[0] - t.lo[0]) * (t.hi[1] - t.lo[1]);
    #[derive(Clone, Copy)]
    struct Up {
        a: P,
        u: u64,
        b: PxBounds,
    }
    let mut up: Vec<Up> = Vec::with_capacity(32);
    let f = loose_front;
    for e in &loose[f][..loose_n[f]] {
        if up.len() >= 32 {
            break;
        }
        if e.actor == taken || !loose_alive(e.actor, e.user) {
            continue;
        }
        let b = px::world_bounds(e.actor);
        let ox = b.hi[0].min(t.hi[0]) - b.lo[0].max(t.lo[0]);
        let oy = b.hi[1].min(t.hi[1]) - b.lo[1].max(t.lo[1]);
        if ox <= 0.0 || oy <= 0.0 {
            continue;
        }
        let area = (b.hi[0] - b.lo[0]) * (b.hi[1] - b.lo[1]);
        if ox * oy < 0.3 * area.min(t_area) {
            continue; // beside it
        }
        if b.lo[2] < t.lo[2] + 0.2 * th || b.lo[2] > t.hi[2] + 0.3 {
            continue; // under it or well above
        }
        up.push(Up { a: e.actor, u: e.user, b });
    }
    up.sort_by(|a, b| a.b.lo[2].total_cmp(&b.b.lo[2]));
    // the ones standing on it, each on the one before: no gap of more than 2 cm up the stack
    let mut top = t.hi[2];
    let mut kept = 0usize;
    for i in 0..up.len() {
        if up[i].b.lo[2] > top + 0.02 {
            break;
        }
        top = top.max(up[i].b.hi[2]);
        up[kept] = up[i];
        kept += 1;
    }
    if kept == 0 {
        return;
    }
    // LET DOWN TOGETHER, STRAIGHT, by as much as the taken one held them up, apart from everything loose, and put to
    // sleep there: things that do not meet each other would fall through the rest to the table, and things that do
    // (the stall's plates) stand in each other and were thrown off the counter by gravity
    let distance = up[0].b.lo[2] - t.lo[2];
    if !(distance > 0.002 && distance < 0.3) {
        return;
    }
    let Some(d) = drops.iter_mut().find(|x| x.n == 0 && x.watch_until == 0) else { return };
    d.n = kept.min(16);
    for i in 0..d.n {
        d.actor[i] = up[i].a;
        d.user[i] = up[i].u;
        rejoin_actor(up[i].a);
        d.apart[i] = false;
        let mut sa = [core::ptr::null_mut(); 1];
        if px::shapes(up[i].a, &mut sa) == 0 {
            continue;
        }
        let w = px::sim_filter(sa[0]);
        d.shape[i] = sa[0];
        d.saved[i] = w;
        // and no callback bits: nothing of this is the game's to answer
        let iso = [hand_filter[0], hand_filter[1], hand_filter_static[2], hand_filter_static[3]];
        px::set_sim_filter(sa[0], &iso);
        refilter(up[i].a);
        d.apart[i] = true;
    }
    d.distance = distance;
    d.fallen = 0.0;
    d.speed = 0.0;
    d.watch_until = 0;
    d.said = 0;
    DROPS_MADE.fetch_add(1, Relaxed);
    log!("[physhands] taken off a stack: {} things stood on it (the lowest asleep {}), coming down {:.3} m together\n", d.n, i32::from(is_sleeping(up[0].a)), distance);
}

pub unsafe fn drop_step(sc: P, dt: f32) {
    if !drops.iter().any(|x| x.n > 0 || x.watch_until != 0) {
        return;
    }
    let g = gravity_of(scene_gravity(sc)).unwrap_or([0.0, 0.0, -9.81]);
    let zero = [0.0f32; 3];
    for d in drops.iter_mut() {
        if d.watch_until != 0 {
            // WATCHED AFTER IT IS DOWN: whether it stays asleep where it was put, and what it does if not
            let now = tick_ms();
            let mut i = 0;
            while i < 16 && d.rest[i] != 0.0 && d.said < 12 {
                if loose_alive(d.actor[i], d.user[i]) {
                    let gp = global_pose(d.actor[i]);
                    let asleep = is_sleeping(d.actor[i]);
                    if !asleep || (gp.p[2] - d.rest[i]).abs() > 0.005 {
                        d.said += 1;
                        log!(
                            "[physhands] let down: thing {} {}, {:.3} m from where it was put ({} ms after)\n",
                            i,
                            if asleep { "asleep" } else { "AWAKE" },
                            gp.p[2] - d.rest[i],
                            (now + 3000).wrapping_sub(d.watch_until)
                        );
                    }
                }
                i += 1;
            }
            if now > d.watch_until {
                d.watch_until = 0;
            }
            continue;
        }
        if d.n == 0 {
            continue;
        }
        if d.fallen >= d.distance - 1e-4 {
            // there: still, and asleep as it was
            for i in 0..d.n {
                if !loose_alive(d.actor[i], d.user[i]) {
                    continue;
                }
                super::body::set_velocities(d.actor[i], &zero, &zero, false);
                put_to_sleep(d.actor[i]);
                if d.apart[i] {
                    if apart_n >= MOST_APART {
                        rejoin(0);
                    }
                    let mut ap = Apart { actor: d.actor[i], user: d.user[i], shape: d.shape[i], saved: d.saved[i], rest: PxTransform::ZERO, kept: 0 };
                    ap.rest = global_pose(d.actor[i]);
                    aparts[apart_n] = ap;
                    apart_n += 1;
                    d.apart[i] = false;
                }
                d.rest[i] = global_pose(d.actor[i]).p[2];
            }
            for i in d.n..16 {
                d.rest[i] = 0.0;
            }
            d.n = 0;
            d.watch_until = tick_ms() + 3000;
            continue;
        }
        if loose_alive(d.actor[0], d.user[0]) {
            let at_pose = global_pose(d.actor[0]);
            let mut lv = [0.0f32; 3];
            at::<unsafe extern "C" fn(P, *mut f32)>(d.actor[0], px::GET_LINEAR_VELOCITY)(d.actor[0], lv.as_mut_ptr());
            log!(
                "[physhands] coming down: fallen {:.3} of {:.3} m; the first thing at z {:.3} moving {:.3} m/s (gravity {:.2}, step {:.4} s)\n",
                d.fallen,
                d.distance,
                at_pose.p[2],
                lv[2],
                g[2],
                dt
            );
        }
        d.speed = (d.speed - g[2] * dt).min(2.5);
        let mut step = d.speed * dt;
        if d.fallen + step > d.distance {
            step = d.distance - d.fallen;
        }
        // straight down by `step` over the step: gravity adds its share within it
        let v = [-g[0] * dt, -g[1] * dt, -step / dt - g[2] * dt];
        for i in 0..d.n {
            if !loose_alive(d.actor[i], d.user[i]) {
                continue;
            }
            super::body::set_velocities(d.actor[i], &v, &zero, true);
        }
        d.fallen += step;
    }
}

// ---- a test stack (physics thread, asked for by VRPhysHandsTestStack) ----
// The few light loose things nearest a point, the smallest first, brought one onto another over the first by velocities
// (a body put somewhere by setGlobalPose is put back by the game: its own idea of where the thing is wins) and put to
// sleep there: a stack to take the middle of. A test tool: nothing calls it but a test script. Ask with dump to log the
// loose things near instead.
#[derive(Clone, Copy, Default)]
pub struct StackAsk {
    pub at: [f32; 3],
    pub at_ms: u64,
    pub serial: u32,
    pub dump: bool,
    pub shapes: bool,
}
pub static STACK_ASKS: Seq<StackAsk> = Seq::new(StackAsk { at: [0.0; 3], at_ms: 0, serial: 0, dump: false, shapes: false });
static mut stack_done: u32 = 0;
static mut stack_made: [P; 4] = [core::ptr::null_mut(); 4];
static mut stack_user: [u64; 4] = [0; 4];
static mut stack_goal: [[f32; 3]; 4] = [[0.0; 3]; 4];
static mut stack_n: usize = 0;
static mut stack_steps: i32 = 0;

/// THE SHAPES ROUND A POINT (a test tool: VRPhysHandsTestStack with w <= -2): every rigid actor's shapes within 1.5 m of
/// it, the scene's and its aggregates', with what kind of actor holds them, their geometry, place and filter.
unsafe fn dump_shapes(sc: P, p: &[f32; 3], origin: &[f32; 3]) {
    let mut said = 0;
    let mut one = |a: P| {
        let kind = px::actor_type(a);
        if kind != 0 && kind != 1 && kind != 4 {
            return;
        }
        let b = px::world_bounds(a);
        let d2: f32 = (0..3).map(|k| out_of(p[k], b.lo[k], b.hi[k])).map(|e| e * e).sum();
        if !(d2 <= 1.5 * 1.5) || said > 160 {
            return;
        }
        if a == actor[0] || a == actor[1] {
            return;
        }
        let (mut kinematic, mut mass) = (false, 0.0f32);
        if kind != 0 {
            kinematic = px::rigid_body_flags(a) & 1 != 0;
            mass = px::mass(a);
        }
        let user = user_of(a);
        let global = global_pose(a);
        let mut list = [core::ptr::null_mut(); 16];
        let n = px::shapes(a, &mut list);
        for &sh in list.iter().take(n.min(16) as usize) {
            if said > 160 {
                break;
            }
            let sflags = shape_flags(sh);
            let w = px::sim_filter(sh);
            let geom = px::geometry_type(sh);
            let mut g = [0.0f32; 4];
            if geom == px::GEOM_BOX {
                if let Some(bx) = px::box_geometry(sh) {
                    g[..3].copy_from_slice(&bx.half);
                }
            }
            if geom == px::GEOM_SPHERE {
                if let Some(sp) = px::sphere(sh) {
                    g[0] = sp.radius;
                }
            }
            if geom == px::GEOM_CAPSULE {
                if let Some(c) = px::capsule(sh) {
                    g[0] = c.radius;
                    g[1] = c.half_height;
                }
            }
            let mut local = PxTransform::default();
            at::<unsafe extern "C" fn(P, *mut PxTransform)>(sh, px::SHAPE_GET_LOCAL_POSE)(sh, &mut local);
            let lp = qrot(&global.q, &local.p);
            let wp = [global.p[0] + lp[0] + origin[0], global.p[1] + lp[1] + origin[1], global.p[2] + lp[2] + origin[2]];
            said += 1;
            let off = ((wp[0] - origin[0] - p[0]).powi(2) + (wp[1] - origin[1] - p[1]).powi(2) + (wp[2] - origin[2] - p[2]).powi(2)).sqrt();
            log!(
                "[physhands] shape: actor {:p} {}{} {:.1} kg user {:016x}; geometry {} ({:.3} {:.3} {:.3}) at ({:.3} {:.3} {:.3}), {:.2} m off; flags {:02x} group {:08x}{:08x} mask {:08x}{:08x}\n",
                a,
                if kind == 0 {
                    "static"
                } else if kinematic {
                    "kinematic"
                } else {
                    "dynamic"
                },
                if kind == 4 { " (articulation link)" } else { "" },
                mass,
                user,
                geom,
                g[0],
                g[1],
                g[2],
                wp[0],
                wp[1],
                wp[2],
                off,
                sflags,
                w[1],
                w[0],
                w[3],
                w[2]
            );
        }
    };
    let count = px::scene_actors(sc, 1 | 2, &mut listed, 0).min(MOST_LISTED as u32);
    for i in 0..count as usize {
        one(listed[i]);
    }
    let ag = px::scene_aggregates(sc, &mut listed, 0).min(MOST_LISTED as u32);
    for i in 0..ag as usize {
        let mut members = [core::ptr::null_mut(); 64];
        let m = px::aggregate_actors(listed[i], &mut members);
        for &member in members.iter().take(m.min(64) as usize) {
            one(member);
        }
    }
    log!("[physhands] shapes round ({:.2} {:.2} {:.2}): {} listed (of {} actors, {} aggregates)\n", p[0] + origin[0], p[1] + origin[1], p[2] + origin[2], said, count, ag);
}

pub unsafe fn test_stack_step(sc: P, origin: &[f32; 3], dt: f32) {
    let zero = [0.0f32; 3];
    if stack_n != 0 && stack_steps != 0 {
        // brought there: each by the velocity that takes it the rest of the way within a few steps, gravity's share off
        let g = scene_gravity(sc);
        let mut there = true;
        for i in 1..stack_n {
            if !loose_alive(stack_made[i], stack_user[i]) {
                continue;
            }
            let at_pose = global_pose(stack_made[i]);
            let mut v = [0.0f32; 3];
            let mut d2 = 0.0f32;
            for k in 0..3 {
                let d = stack_goal[i][k] - at_pose.p[k];
                d2 += d * d;
                v[k] = (d / dt.max(0.008) * 0.5).clamp(-1.5, 1.5) - g[k] * dt;
            }
            if d2 > 0.002 * 0.002 {
                there = false;
            }
            super::body::set_velocities(stack_made[i], &v, &zero, true);
        }
        stack_steps -= 1;
        if there || stack_steps == 0 {
            for i in 0..stack_n {
                if !loose_alive(stack_made[i], stack_user[i]) {
                    continue;
                }
                super::body::set_velocities(stack_made[i], &zero, &zero, false);
                put_to_sleep(stack_made[i]);
            }
            log!("[physhands] test stack {}\n", if there { "in place, asleep" } else { "not all there in time, asleep where they were" });
            stack_steps = 0;
        }
    }
    let Some(ask) = STACK_ASKS.read() else { return };
    if ask.serial == stack_done {
        return;
    }
    stack_done = ask.serial;
    let p = [ask.at[0] - origin[0], ask.at[1] - origin[1], ask.at[2] - origin[2]];
    if ask.shapes {
        dump_shapes(sc, &p, origin);
        return;
    }
    let f = loose_front;
    if ask.dump {
        for e in &loose[f][..loose_n[f]] {
            if !loose_alive(e.actor, e.user) {
                continue;
            }
            let b = px::world_bounds(e.actor);
            log!(
                "[physhands] loose {:016x}: {:.2} kg, asleep {}, centre ({:.3} {:.3} {:.3}), {:.3} x {:.3} x {:.3} m\n",
                e.user,
                px::mass(e.actor),
                i32::from(is_sleeping(e.actor)),
                0.5 * (b.lo[0] + b.hi[0]) + origin[0],
                0.5 * (b.lo[1] + b.hi[1]) + origin[1],
                0.5 * (b.lo[2] + b.hi[2]) + origin[2],
                b.hi[0] - b.lo[0],
                b.hi[1] - b.lo[1],
                b.hi[2] - b.lo[2]
            );
        }
        return;
    }
    struct C {
        a: P,
        u: u64,
        b: PxBounds,
        d: f32,
    }
    let mut c: Vec<C> = Vec::with_capacity(64);
    for e in &loose[f][..loose_n[f]] {
        if c.len() >= 64 {
            break;
        }
        if !loose_alive(e.actor, e.user) || px::mass(e.actor) > 3.0 {
            continue;
        }
        let b = px::world_bounds(e.actor);
        let d2: f32 = (0..3).map(|k| 0.5 * (b.lo[k] + b.hi[k]) - p[k]).map(|m| m * m).sum();
        if d2 > 0.8 * 0.8 {
            continue;
        }
        c.push(C { a: e.actor, u: e.user, b, d: d2 });
    }
    c.sort_by(|x, y| x.d.total_cmp(&y.d));
    stack_n = c.len().min(3);
    if stack_n < 2 {
        log!("[physhands] test stack: {} light loose things within 0.8 m of ({:.2} {:.2} {:.2}), not enough\n", c.len(), ask.at[0], ask.at[1], ask.at[2]);
        stack_n = 0;
        return;
    }
    // over the nearest one, each on the top of the last
    let cx = 0.5 * (c[0].b.lo[0] + c[0].b.hi[0]);
    let cy = 0.5 * (c[0].b.lo[1] + c[0].b.hi[1]);
    let mut top = c[0].b.hi[2];
    for i in 0..stack_n {
        stack_made[i] = c[i].a;
        stack_user[i] = c[i].u;
        stack_goal[i] = global_pose(c[i].a).p;
        if i == 0 {
            continue;
        }
        let bx = 0.5 * (c[i].b.lo[0] + c[i].b.hi[0]);
        let by = 0.5 * (c[i].b.lo[1] + c[i].b.hi[1]);
        stack_goal[i][0] += cx - bx;
        stack_goal[i][1] += cy - by;
        stack_goal[i][2] += top - c[i].b.lo[2] + 0.002;
        top += c[i].b.hi[2] - c[i].b.lo[2];
    }
    stack_steps = 240;
    log!(
        "[physhands] test stack of {} at ({:.2} {:.2}), top {:.3} m over the first's foot; userData {:016x} {:016x} {:016x}\n",
        stack_n,
        cx + origin[0],
        cy + origin[1],
        top - c[0].b.lo[2],
        stack_user[0],
        stack_user[1],
        if stack_n > 2 { stack_user[2] } else { 0 }
    );
}

/// The world scene changed: the loose lists, the things set apart, the walk and the drops start again.
pub unsafe fn reset_for_scene() {
    loose_n = [0; 2];
    apart_n = 0;
    loose_walk = LooseWalk { phase: 0, next: 0, at: 0, center: [0.0; 3] };
    loose_capped_at = 0;
    for d in drops.iter_mut() {
        d.n = 0;
    }
}
