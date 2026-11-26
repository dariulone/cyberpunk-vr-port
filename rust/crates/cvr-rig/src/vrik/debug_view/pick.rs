//! What moves near a point, what a hand points at, where PhysX has a body, the nearest body of a proxy, a proxy's kind
//! and bodies, and a body's filters to the log (main's DebugView.cpp: FindNear, PickCone, ActorWorld, NearestBodyOf,
//! ProxyKind, ProxyBodies, FilterInfo, ProbeProxy, ProbeBody).
use super::*;
use cvr_engine::code::image_base;
use std::cell::RefCell;

/// A body near: its proxy and body, `distance` (m from its bounds; for the cone, the angle off the line), its bounds'
/// volume (for the cone: how far it is).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NearBody {
    pub proxy: u32,
    pub body: u32,
    pub distance: f32,
    pub volume: f32,
}
/// Kept sorted by distance (then the smaller volume), at most `capacity`: the last one goes when a nearer comes.
fn insert_sorted(out: &mut Vec<NearBody>, capacity: usize, n: NearBody, by_volume: bool) {
    if capacity == 0 {
        return;
    }
    let mut at = if out.len() < capacity {
        out.push(n);
        out.len() - 1
    } else {
        if out[capacity - 1].distance < n.distance {
            return;
        }
        capacity - 1
    };
    while at > 0 && (out[at - 1].distance > n.distance || (by_volume && out[at - 1].distance == n.distance && out[at - 1].volume > n.volume)) {
        out[at] = out[at - 1];
        at -= 1;
    }
    out[at] = n;
}
fn bounds_distance2(b: &Bounds, c: &Vec3) -> f32 {
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
/// Every dynamic actor of the world's scenes and of their aggregates, with its scene's origin (WALK held).
unsafe fn each_dynamic(scenes: &Scenes, mut consider: impl FnMut(P, &Vec3, usize)) {
    for s in 0..scenes.count {
        if scenes.owners[s].is_null() {
            continue;
        }
        let Some(origin) = owner_origin(scenes.owners[s]) else { continue };
        let sc = scenes.scenes[s];
        if let Some(actors) = scene_actors(sc, 2, MAX_ACTORS, 0) {
            for &a in actors {
                consider(a, &origin, s);
            }
        }
        if let Some(aggregates) = scene_aggregates(sc) {
            for &ag in aggregates {
                let Some((members, n)) = aggregate_members(ag) else { continue };
                for &m in &members[..n] {
                    consider(m, &origin, s);
                }
            }
        }
    }
}

/// FindNear: what moves near a point, from PhysX itself -- for a hand closing where no ray happened to go. Nearest first,
/// by its bounds.
pub fn find_near(center: &Vec3, radius: f32, capacity: usize) -> Vec<NearBody> {
    let mut out = Vec::with_capacity(capacity);
    if PROVED.load(Relaxed) < 0 {
        return out;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: guarded calls into PhysX's lists and actors.
    unsafe {
        let Some(scenes) = read_scenes() else { return out };
        each_dynamic(&scenes, |actor, origin, _| {
            // only what is dynamic to PhysX
            if actor_type(actor) != Some(1) {
                return;
            }
            let Some(place) = global_pose(actor) else { return };
            let f = [place.p[0] + origin[0] - center[0], place.p[1] + origin[1] - center[1], place.p[2] + origin[2] - center[2]];
            if f[0] * f[0] + f[1] * f[1] + f[2] * f[2] > (radius + 4.0) * (radius + 4.0) {
                return;
            }
            let Some(b) = world_bounds(actor) else { return };
            let c = [center[0] - origin[0], center[1] - origin[1], center[2] - origin[2]];
            let d2 = bounds_distance2(&b, &c);
            if !(d2 <= radius * radius) {
                return;
            }
            let (proxy, body) = ids_of(actor);
            let volume = (b.hi[0] - b.lo[0]) * (b.hi[1] - b.lo[1]) * (b.hi[2] - b.lo[2]);
            insert_sorted(&mut out, capacity, NearBody { proxy, body, distance: d2.sqrt(), volume }, true);
        });
    }
    out
}

/// PickCone: what a hand points at -- of the moving actors, the one whose middle is nearest the line of either direction,
/// by angle, within the cone. Not a thin ray: small things on a shelf could hardly be hit. The one marked last is kept
/// unless another is clearly nearer the line, or two neighbours would flicker.
pub fn pick_cone(from: &Vec3, dir_a: &Vec3, dir_b: &Vec3, cone: f32, range: f32, keep_proxy: u32, keep_body: u32, capacity: usize) -> Vec<NearBody> {
    let mut out = Vec::with_capacity(capacity);
    if PROVED.load(Relaxed) < 0 {
        return out;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    let cos_cone = cone.cos();
    // SAFETY: guarded calls into PhysX's lists and actors.
    unsafe {
        let Some(scenes) = read_scenes() else { return out };
        each_dynamic(&scenes, |actor, origin, _| {
            if actor_type(actor) != Some(1) {
                return;
            }
            let Some(place) = global_pose(actor) else { return };
            let f = [place.p[0] + origin[0] - from[0], place.p[1] + origin[1] - from[1], place.p[2] + origin[2] - from[2]];
            if f[0] * f[0] + f[1] * f[1] + f[2] * f[2] > (range + 2.0) * (range + 2.0) {
                return;
            }
            let Some(b) = world_bounds(actor) else { return };
            let size = [b.hi[0] - b.lo[0], b.hi[1] - b.lo[1], b.hi[2] - b.lo[2]];
            // no loose thing to pull
            if size[0] > 2.0 || size[1] > 2.0 || size[2] > 2.0 {
                return;
            }
            let m = [
                (b.lo[0] + b.hi[0]) * 0.5 + origin[0] - from[0],
                (b.lo[1] + b.hi[1]) * 0.5 + origin[1] - from[1],
                (b.lo[2] + b.hi[2]) * 0.5 + origin[2] - from[2],
            ];
            let d = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
            if d > range || d < 0.15 {
                return;
            }
            let c1 = (m[0] * dir_a[0] + m[1] * dir_a[1] + m[2] * dir_a[2]) / d;
            let c2 = (m[0] * dir_b[0] + m[1] * dir_b[1] + m[2] * dir_b[2]) / d;
            let c = if c1 > c2 { c1 } else { c2 };
            if c < cos_cone {
                return;
            }
            let (proxy, body) = ids_of(actor);
            let mut angle = (if c < 1.0 { c } else { 1.0 }).acos();
            if proxy == keep_proxy && body == keep_body {
                angle -= 0.05;
            }
            insert_sorted(&mut out, capacity, NearBody { proxy, body, distance: angle, volume: d }, false);
        });
    }
    out
}

/// The actor (proxy, body) in the world's scenes: the dynamic actors first, then the aggregates; with its scene's index.
unsafe fn find_dynamic(scenes: &Scenes, proxy: u32, body: u32) -> Option<(P, usize)> {
    for s in 0..scenes.count {
        if scenes.owners[s].is_null() {
            continue;
        }
        let sc = scenes.scenes[s];
        if let Some(a) = find_in_actors(sc, 2, proxy, body)? {
            return Some((a, s));
        }
        if let Some(a) = find_in_aggregates(sc, proxy, body)? {
            return Some((a, s));
        }
    }
    None
}
/// ActorWorld: where PhysX has a body, in the game's world, or None if it has none.
pub fn actor_world(proxy: u32, body: u32) -> Option<Vec3> {
    if PROVED.load(Relaxed) < 0 {
        return None;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: guarded calls into PhysX's lists and actors.
    unsafe {
        let scenes = read_scenes()?;
        let (a, s) = find_dynamic(&scenes, proxy, body)?;
        let pose = global_pose(a)?;
        let o = owner_origin(scenes.owners[s])?;
        Some([pose.p[0] + o[0], pose.p[1] + o[1], pose.p[2] + o[2]])
    }
}

/// What nearest_body_of found: the body's index, its actor (a PxRigidDynamic), that actor's scene and the scene's
/// origin, and how far (0 inside).
#[derive(Clone, Copy, Debug)]
pub struct NearestBody {
    pub body: u32,
    pub actor: P,
    pub scene: P,
    pub origin: Vec3,
    pub distance: f32,
}
/// NearestBodyOf: the body of `proxy` whose own shapes are nearest `world` (the game's world), within `radius`, by its
/// own shapes (PxGeometryQuery through physhands' body_surface), the bounds only to pass over the far ones: a point
/// inside a shin's capsule lies inside the bounds of the foot and the thigh as well. Of two that have it inside, the
/// smaller.
pub fn nearest_body_of(proxy: u32, world: &Vec3, radius: f32) -> Option<NearestBody> {
    if PROVED.load(Relaxed) < 0 {
        return None;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: guarded calls into PhysX's lists and actors.
    unsafe {
        let scenes = read_scenes()?;
        let dll = physx_dll();
        let mut best = radius;
        let mut best_volume = 0.0f32;
        let mut found: Option<NearestBody> = None;
        for s in 0..scenes.count {
            if scenes.owners[s].is_null() {
                continue;
            }
            let Some(o) = owner_origin(scenes.owners[s]) else { continue };
            let sc = scenes.scenes[s];
            let mut consider = |actor: P| {
                let (p, b) = ids_of(actor);
                if p != proxy {
                    return;
                }
                if read::<usize>(actor as usize).map(|t| t.wrapping_sub(dll)) != Some(RIGID_DYNAMIC_TABLE) {
                    return;
                }
                let Some(bounds) = world_bounds(actor) else { return };
                let c = [world[0] - o[0], world[1] - o[1], world[2] - o[2]];
                if bounds_distance2(&bounds, &c) > best * best {
                    return;
                }
                let Some((_, d)) = super::super::physhands::hold::body_surface(actor, &o, world) else { return };
                let volume = (bounds.hi[0] - bounds.lo[0]) * (bounds.hi[1] - bounds.lo[1]) * (bounds.hi[2] - bounds.lo[2]);
                if d > radius || (found.is_some() && (d > best || (d == best && volume >= best_volume))) {
                    return;
                }
                best = d;
                best_volume = volume;
                found = Some(NearestBody { body: b, actor, scene: sc, origin: o, distance: d });
            };
            if let Some(aggregates) = scene_aggregates(sc) {
                for &ag in aggregates {
                    let Some((members, n)) = aggregate_members(ag) else { continue };
                    for &m in &members[..n] {
                        consider(m);
                    }
                }
            }
            if let Some(actors) = scene_actors(sc, 2, MAX_ACTORS, 0) {
                for &a in actors {
                    consider(a);
                }
            }
        }
        found
    }
}

const SYSTEM_RVA: usize = 0x3BF_CDD0;
const LOOKUP_RVA: usize = 0x27_D430;
const RELEASE_RVA: usize = 0x27_D374;
const LOOKUP_PROLOGUE: [u8; 14] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x20, 0x55, 0x57, 0x41, 0x56];
/// The proxy's record through the game's lookup (its handle held across `read`): what `read` makes of it, None when
/// the lookup is not where it was read or there is no system.
unsafe fn with_proxy<T>(proxy: u32, read_it: impl FnOnce(usize) -> T) -> Option<T> {
    let exe = image_base();
    let mut head = [0u8; 14];
    if !read_bytes(exe + LOOKUP_RVA, &mut head) || head != LOOKUP_PROLOGUE {
        return None;
    }
    let system = read::<usize>(exe + SYSTEM_RVA).filter(|s| *s != 0)?;
    let mut handle = [0usize; 2];
    guarded_call4(exe + LOOKUP_RVA, system as P, handle.as_mut_ptr().cast(), proxy as usize as P, core::ptr::null_mut())?;
    // live while the handle holds it; no VirtualQuery (7-12 ms here)
    let out = read_it(handle[0]);
    guarded_call4(exe + RELEASE_RVA, handle.as_mut_ptr().cast(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
    Some(out)
}
/// physics::ProxyType of a proxy (the record's byte +0x34), 0 if there is none.
fn proxy_kind_asked(proxy: u32) -> i32 {
    // SAFETY: guarded calls into the game's proxy system.
    unsafe { with_proxy(proxy, |object| if object != 0 { read::<u8>(object + 0x34).map_or(0, i32::from) } else { 0 }) }.unwrap_or(0)
}
thread_local! {
    static KINDS: RefCell<[(u32, i32, u64); 64]> = const { RefCell::new([(0, 0, 0); 64]) };
}
/// ProxyKind: asked of the game's table, which is shared with what the physics does; what a proxy is does not change
/// from one moment to the next (kept 20 s, an unknown one 1 s).
pub fn proxy_kind(proxy: u32) -> i32 {
    let now = super::super::physhands::tick_ms();
    KINDS.with(|k| {
        let mut k = k.borrow_mut();
        let s = &mut k[(proxy % 64) as usize];
        if s.0 == proxy && now - s.2 < if s.1 != 0 { 20000 } else { 1000 } {
            return s.1;
        }
        *s = (proxy, proxy_kind_asked(proxy), now);
        s.1
    })
}
/// ProxyBodies: how many bodies PhysX has of a proxy, as rigid bodies; a ragdoll made of articulation links would be
/// none.
pub fn proxy_bodies(proxy: u32) -> i32 {
    if PROVED.load(Relaxed) < 0 {
        return 0;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    let (mut count, mut type_seen) = (0, -1);
    // SAFETY: guarded calls into PhysX's lists and actors.
    unsafe {
        let Some(scenes) = read_scenes() else { return 0 };
        each_dynamic(&scenes, |actor, _, _| {
            if ids_of(actor).0 == proxy {
                count += 1;
                type_seen = actor_type(actor).unwrap_or(-1);
            }
        });
    }
    log!("[proxy] {} has {} bodies, actor type {}\n", proxy, count, type_seen);
    count
}

const OWNERS: [&str; 29] = [
    "Unknown",
    "BakedDestructionNode",
    "ClothMeshNode",
    "CollisionAreaNode",
    "DecorationMeshNode",
    "DynamicMeshNode",
    "InstancedDestructibleNode",
    "PhysicalDestructionNode",
    "PhysicalTriggerNode",
    "StaticMeshNode",
    "TerrainCollisionNode",
    "WaterPatchNode",
    "WorldCollisionNode",
    "BakedDestructionComponent",
    "ClothComponent",
    "ColliderComponent",
    "PhysicalDestructionComponent",
    "PhysicalMeshComponent",
    "PhysicalSkinnedMeshComponent",
    "PhysicalTriggerComponent",
    "SimpleColliderComponent",
    "SkinnedClothComponent",
    "StateMachineComponent",
    "VehicleChassisComponent",
    "PhysicalParticleSystem",
    "PhotoModeSystem",
    "RagdollBinder",
    "FoliageDestruction",
    "EntityProxy",
];
/// ProbeProxy: does a prop in the hand belong to an entity? The proxy says who owns it: its kind (+0x34) and its owner
/// byte (+0x35, physics::PhysicalSystemOwner), to the log.
pub fn probe_proxy(proxy: u32) {
    // SAFETY: guarded calls into the game's proxy system.
    let got = unsafe {
        with_proxy(proxy, |object| {
            if object == 0 {
                return;
            }
            let owner = read::<u8>(object + 0x35).map_or(0, u32::from);
            log!(
                "[proxy] {}: kind {}, owner byte {} ({})\n",
                proxy,
                read::<u8>(object + 0x34).map_or(0, u32::from),
                owner,
                OWNERS.get(owner as usize).copied().unwrap_or("?")
            );
        })
    };
    if got.is_none() {
        log!("[proxy] the lookup is not where it was: another game version, or no physics system\n");
    }
}
/// ProbeBody: the body's actor by what the game writes in its userData: where PhysX has it, against where the game
/// says, to the log.
pub fn probe_body(proxy: u32, body: u32, world: &Vec3) {
    if PROVED.load(Relaxed) < 0 {
        return;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: guarded calls into PhysX's lists and actors.
    unsafe {
        let Some(scenes) = read_scenes() else { return };
        for s in 0..scenes.count {
            let sc = scenes.scenes[s];
            let mut found = find_in_aggregates(sc, proxy, body).flatten();
            let mut place = "an aggregate";
            if found.is_none() {
                found = find_in_actors(sc, 1 | 2, proxy, body).flatten();
                place = "the scene's list";
            }
            if let Some(a) = found {
                let pose = global_pose(a).unwrap_or_default();
                log!(
                    "[debugview] body {}/{} is an actor of scene {} in {}: PhysX has it at ({:.2} {:.2} {:.2}), the game at ({:.2} {:.2} {:.2}), so the origin is ({:.2} {:.2} {:.2})\n",
                    proxy,
                    body,
                    s,
                    place,
                    pose.p[0],
                    pose.p[1],
                    pose.p[2],
                    world[0],
                    world[1],
                    world[2],
                    world[0] - pose.p[0],
                    world[1] - pose.p[1],
                    world[2] - pose.p[2]
                );
                return;
            }
        }
    }
    log!("[debugview] body {}/{} is no actor of any scene\n", proxy, body);
}

const ACTOR_GET_ACTOR_FLAGS: usize = 12;
const ACTOR_GET_DOMINANCE: usize = 14;
const ACTOR_GET_AGGREGATE: usize = 19;
const RIGID_BODY_GET_FLAGS: usize = 47;
const AGGREGATE_GET_SELF_COLLISION: usize = 13;
const SHAPE_GET_FLAGS: usize = 34;
const SHAPE_GET_SIM_FILTER: usize = 21;
const SHAPE_GET_QUERY_FILTER: usize = 23;
/// FilterInfo: what decides a body's contacts, to the log: the actor's flags and dominance group, whether its aggregate
/// collides with itself, and each shape's flags and simulation and query filter data.
pub fn filter_info(proxy: u32, body: u32) {
    // SAFETY: guarded calls into PhysX's lists and the actor.
    let done = unsafe {
        (|| -> Option<()> {
            let Some((actor, _, _)) = super::rigid_dynamic_of(proxy, body) else {
                log!("[filterinfo] {}/{}: no PxRigidDynamic\n", proxy, body);
                return Some(());
            };
            let (mut actor_flags, mut body_flags) = (0u8, 0u8);
            slot_call(actor, ACTOR_GET_ACTOR_FLAGS, [(&raw mut actor_flags) as usize, 0, 0, 0, 0])?;
            slot_call(actor, RIGID_BODY_GET_FLAGS, [(&raw mut body_flags) as usize, 0, 0, 0, 0])?;
            let dominance = slot_call(actor, ACTOR_GET_DOMINANCE, [0; 5])? as u8;
            let aggregate = slot_call(actor, ACTOR_GET_AGGREGATE, [0; 5])? as P;
            let self_collision = if aggregate.is_null() { -1 } else { i32::from(slot_call(aggregate, AGGREGATE_GET_SELF_COLLISION, [0; 5])? as u8 != 0) };
            let mut shapes = [core::ptr::null_mut::<c_void>(); 16];
            let n = actor_shapes(actor, &mut shapes, 0)?;
            log!(
                "[filterinfo] {}/{}: actor {:#x} flags {:02x}, body flags {:02x} (kinematic {}), dominance {}, aggregate {:#x} self-collision {}, {} shapes\n",
                proxy,
                body,
                actor as usize,
                actor_flags,
                body_flags,
                body_flags & 1,
                dominance,
                aggregate as usize,
                self_collision,
                n
            );
            for (i, &shape) in shapes[..n].iter().enumerate() {
                let mut flags = 0u8;
                let (mut sim, mut query) = ([0u32; 4], [0u32; 4]);
                slot_call(shape, SHAPE_GET_FLAGS, [(&raw mut flags) as usize, 0, 0, 0, 0])?;
                slot_call(shape, SHAPE_GET_SIM_FILTER, [sim.as_mut_ptr() as usize, 0, 0, 0, 0])?;
                slot_call(shape, SHAPE_GET_QUERY_FILTER, [query.as_mut_ptr() as usize, 0, 0, 0, 0])?;
                log!(
                    "[filterinfo]   shape {}: flags {:02x} (simulation {}, query {}, trigger {}), sim {:08x} {:08x} {:08x} {:08x}, query {:08x} {:08x} {:08x} {:08x}\n",
                    i,
                    flags,
                    flags & 1,
                    (flags >> 1) & 1,
                    (flags >> 2) & 1,
                    sim[0],
                    sim[1],
                    sim[2],
                    sim[3],
                    query[0],
                    query[1],
                    query[2],
                    query[3]
                );
            }
            Some(())
        })()
    };
    if done.is_none() {
        log!("[filterinfo] {}/{}: faulted\n", proxy, body);
    }
}
