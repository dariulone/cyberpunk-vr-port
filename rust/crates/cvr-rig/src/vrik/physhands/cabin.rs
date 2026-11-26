//! THE CAR'S CABIN FOR THE HANDS (physics thread). Seated in a vehicle the hands' bodies had nothing to stop on: the
//! cabin the player sees -- the dashboard, the wheel's column, the seats, the roof, the pillars -- is the car's query
//! shapes (flags 0a: rays, sweeps and the camera see them, the simulation does not), and the car's simulated shapes are
//! its outer hull and its doors, which the hands' filter never met (2026-11-26, an Archer Hella: no contact reaching
//! into the dashboard, the door or the roof). So the hands get copies of their own, as the people's capsules are
//! (people.rs): each of the cabin's boxes and convex hulls a kinematic simulation-only shape in the hands' people bits
//! -- the hands meet them and nothing else does -- posed every step IN THE PLAYER'S FRAME, the frame the hands' targets
//! come from: posed from the car's own actors, a frame's lag between the two is 30 cm at 70 km/h. Each shape's place in
//! the player's frame is taken once the car stands (still, both frames are the same moment's), kept while it drives,
//! and the copies go when the player gets out. THE CAR'S OWN SIMULATED PARTS round the seat (the hull, the doors) the
//! hands do not meet while seated (CAR_GROUPS, body::hand_filters): the game moves them a frame off the player's frame,
//! and the hull met shoved both palms 60 cm forward through the cabin as the car pulled away (2026-11-26).
use super::body::{actor, hand_filter, material, scene};
use super::census::{bit_person_left, bit_person_right, filter_ready};
use super::loose::{global_pose, shape_flags, user_of};
use super::math::{qconj, qmul, qnorm, qrot};
use super::near::{world_bounds_fn, NearShape};
use super::people::own_body;
use super::pose::step_target;
use super::px::{self, at, PxBounds, PxTransform, P};
use super::tick_ms;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use cvr_host::log;
use cvr_physhands::person::filter_person_hands;

extern "C" {
    static g_isInVehicle: bool;
}

/// the car's actors copied at most, and shapes of each
const MOST_ACTORS: usize = 24;
const MOST_SHAPES: usize = 16;
/// an actor of the cabin: its bounds within this of the chest (m), and no longer than a car
const REACH: f32 = 1.2;
const LONGEST: f32 = 6.0;
/// a shape's bounds holding a point this far round the chest or the hips is a volume round the player (the hull)
const AROUND: f32 = 0.05;
/// a box side thinner than this (half, m) is grown outward to it
const SLAB_HALF: f32 = 0.06;
/// the cabin taken once a mount while the car stands (the player's pace under STILL, m/s), again this often (ms) while
/// none was found
const TAKE_EVERY_MS: u64 = 2000;
const STILL: f32 = 0.25;
/// the scene's dynamic actors read this many at a time
const PAGE: usize = 512;

#[derive(Clone, Copy)]
struct Copy {
    /// the game's actor and its userData (gone when either changes), the copy, its place in the player's frame
    source: P,
    source_user: u64,
    actor: P,
    rel: PxTransform,
    shapes: u32,
}
const COPY0: Copy = Copy { source: core::ptr::null_mut(), source_user: 0, actor: core::ptr::null_mut(), rel: PxTransform::ZERO, shapes: 0 };
static mut COPIES: [Copy; MOST_ACTORS] = [COPY0; MOST_ACTORS];
static mut COPY_N: usize = 0;
static mut TAKEN_AT: u64 = 0;
static mut LAST_SAID: (u32, u32, [i32; 3]) = (u32::MAX, u32::MAX, [0; 3]);
static mut WAIT_TOLD_AT: u64 = 0;
static mut PACE_FROM: [f32; 3] = [0.0; 3];
static mut PACE_AT: u64 = 0;
static mut PACE_NOW: f32 = 0.0;
static mut FILTER: [u32; 4] = [0; 4];
static mut PAGE_BUF: [P; PAGE] = [core::ptr::null_mut(); PAGE];
/// how many shapes the cabin has now (statistics, the self-test)
pub static CABIN_SHAPES: AtomicU32 = AtomicU32::new(0);
/// the simulation groups of the car's own simulated shapes round the seat, taken with the cabin: seated, the hands'
/// masks leave them out (body::hand_filters)
pub static CAR_GROUPS: AtomicU64 = AtomicU64::new(0);

/// THE CABIN IS THE WORLD TO THE HANDS (touch.rs touches_world): pressed on the dashboard or a seat, a hand is held soft
/// and drawn still as on a table. Taken for a loose thing it was pushed at a loose thing's strength, held 6 cm off for
/// 150 ms, let through for 0.4 s and caught again -- the hands on the lap jumped up and down (2026-11-26). Each copied
/// shape's geometry and place on its copy; posed with it every step (physics thread).
pub const MOST_NEAR: usize = 128;
static mut SHAPE_COPY: [usize; MOST_NEAR] = [0; MOST_NEAR];
static mut SHAPE_LOCAL: [PxTransform; MOST_NEAR] = [PxTransform::ZERO; MOST_NEAR];
pub static mut cabin_near: [NearShape; MOST_NEAR] = [NearShape::ZERO; MOST_NEAR];
pub static mut cabin_near_n: usize = 0;

/// Ours: a cabin copy (a census or a walk must not count it).
pub unsafe fn own_cabin(a: P) -> bool {
    !a.is_null() && COPIES[..COPY_N].iter().any(|c| c.actor == a)
}

unsafe fn alive(a: P, user: u64) -> bool {
    !a.is_null() && *(a as *const usize) == px::dll() + px::DYNAMIC_TABLE && user_of(a) == user
}

unsafe fn release_all(why: &str) {
    cabin_near_n = 0;
    if COPY_N == 0 {
        return;
    }
    for c in &mut COPIES[..COPY_N] {
        if !c.actor.is_null() {
            // PxActor::release takes it out of its scene too
            at::<unsafe extern "C" fn(P)>(c.actor, px::RELEASE)(c.actor);
        }
        *c = COPY0;
    }
    log!("[physhands] the car's cabin for the hands dropped ({why})\n");
    COPY_N = 0;
    CABIN_SHAPES.store(0, Relaxed);
    CAR_GROUPS.store(0, Relaxed);
}

fn holds(b: &PxBounds, p: &[f32; 3]) -> bool {
    (0..3).all(|k| p[k] > b.lo[k] - AROUND && p[k] < b.hi[k] + AROUND)
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

/// The player's frame this step (world): the entity's place and turn as the hands' targets carry them (body::
/// target_now), fresh.
unsafe fn player_frame() -> Option<([f32; 3], [f32; 4])> {
    let t = (0..2).filter_map(|side| step_target(side)).find(|t| tick_ms().wrapping_sub(t.at) <= 150)?;
    let mut q = t.entity_rot;
    if !qnorm(&mut q) || !(t.entity_pos[0] + t.entity_pos[1] + t.entity_pos[2]).is_finite() {
        return None;
    }
    Some((t.entity_pos, q))
}

/// One shape of the car's copied onto `copy` at its local pose, when it is a cabin's: a box or a convex hull, not a
/// trigger, not a volume round the chest or the hips. Some(true) copied, Some(false) a volume round the player, None
/// not a cabin's shape.
unsafe fn copy_shape(sh: P, copy: P, global: &PxTransform, origin: &[f32; 3], chest: &[f32; 3], hips: &[f32; 3]) -> Option<bool> {
    let flags = shape_flags(sh);
    if flags & 4 != 0 {
        return None; // a trigger
    }
    let kind = px::geometry_type(sh);
    if kind != px::GEOM_BOX && kind != px::GEOM_CONVEX {
        return None;
    }
    #[repr(C, align(8))]
    struct G([u8; 64]);
    let mut g = G([0; 64]);
    let slot = if kind == px::GEOM_BOX { px::SHAPE_GET_BOX } else { px::SHAPE_GET_CONVEX };
    if !at::<unsafe extern "C" fn(P, *mut u8) -> bool>(sh, slot)(sh, g.0.as_mut_ptr()) {
        return None;
    }
    if kind == px::GEOM_CONVEX && (*(g.0.as_ptr().add(32) as *const P)).is_null() {
        return None;
    }
    let mut local = PxTransform::default();
    at::<unsafe extern "C" fn(P, *mut PxTransform)>(sh, px::SHAPE_GET_LOCAL_POSE)(sh, &mut local);
    let lp = qrot(&global.q, &local.p);
    let world = PxTransform { q: qmul(&global.q, &local.q), p: [global.p[0] + lp[0], global.p[1] + lp[1], global.p[2] + lp[2]] };
    let bounds_of = world_bounds_fn()?;
    let b = bounds_of(g.0.as_ptr().cast(), &world, 1.0);
    let chest_s = [chest[0] - origin[0], chest[1] - origin[1], chest[2] - origin[2]];
    let hips_s = [hips[0] - origin[0], hips[1] - origin[1], hips[2] - origin[2]];
    if holds(&b, &chest_s) || holds(&b, &hips_s) {
        return Some(false); // the hull round the seats: the hands are inside it
    }
    // A THIN SLAB GROWN OUTWARD: the roof is a box 2.5 cm thick a side, the doors' panels as thin; a palm 13 cm long
    // pushed up through it came out above the roof (its middle past the slab's, the solver pushes it out the far
    // side). Each side under SLAB_HALF is made that thick, the face toward the chest kept where it was.
    if kind == px::GEOM_BOX {
        let half = core::ptr::read_unaligned(g.0.as_ptr().add(4) as *const [f32; 3]);
        let mut grown = half;
        let mut shift = [0.0f32; 3];
        for k in 0..3 {
            if half[k] >= SLAB_HALF {
                continue;
            }
            let mut axis = [0.0f32; 3];
            axis[k] = 1.0;
            let axis_w = qrot(&world.q, &axis);
            let away: f32 = (0..3).map(|j| (world.p[j] - chest_s[j]) * axis_w[j]).sum();
            let grow = SLAB_HALF - half[k];
            grown[k] = SLAB_HALF;
            for j in 0..3 {
                shift[j] += axis_w[j] * grow * if away >= 0.0 { 1.0 } else { -1.0 };
            }
        }
        if grown != half {
            core::ptr::write_unaligned(g.0.as_mut_ptr().add(4) as *mut [f32; 3], grown);
            let s = qrot(&qconj(&global.q), &shift);
            for k in 0..3 {
                local.p[k] += s[k];
            }
        }
    }
    let physics = px::physics();
    let mats: [P; 1] = [material];
    let sim_only: u8 = 1;
    let made = at::<unsafe extern "C" fn(P, *const u8, *const P, u16, bool, *const u8) -> P>(physics, px::CREATE_SHAPE)(
        physics,
        g.0.as_ptr(),
        mats.as_ptr(),
        1,
        true,
        &sim_only,
    );
    if made.is_null() {
        return None;
    }
    at::<unsafe extern "C" fn(P, *const PxTransform)>(made, px::SHAPE_SET_LOCAL_POSE)(made, &local);
    px::set_sim_filter(made, &FILTER);
    at::<unsafe extern "C" fn(P, P) -> bool>(copy, px::ATTACH_SHAPE)(copy, made);
    at::<unsafe extern "C" fn(P)>(made, px::SHAPE_RELEASE)(made); // the copy holds it now
    // for touches_world: the copy this shape is on (the one consider is making, COPY_N's), its geometry and place on it
    if cabin_near_n < MOST_NEAR {
        let i = cabin_near_n;
        SHAPE_COPY[i] = COPY_N;
        SHAPE_LOCAL[i] = local;
        cabin_near[i] = NearShape::ZERO;
        cabin_near[i].geometry.0 = g.0;
        cabin_near[i].kind = kind;
        cabin_near_n += 1;
    }
    Some(true)
}

/// What a take saw: kinematic actors round the seat, shapes copied, shapes left out as volumes round the player.
#[derive(Default)]
struct Seen {
    scanned: u32,
    kinematic: u32,
    near: u32,
    shapes: u32,
    volumes: u32,
    /// the car's own simulated shapes round the seat: their groups and masks (the pair rule is an or)
    groups: u64,
    masks: u64,
}

/// One actor, the scene's or an aggregate's: copied when it is a kinematic actor round the seat with cabin shapes.
unsafe fn consider(a: P, origin: &[f32; 3], frame: &([f32; 3], [f32; 4]), chest: &[f32; 3], hips: &[f32; 3], seen: &mut Seen) {
    if COPY_N >= MOST_ACTORS || a.is_null() || a == actor[0] || a == actor[1] || *(a as *const usize) != px::dll() + px::DYNAMIC_TABLE {
        return;
    }
    seen.scanned += 1;
    if user_of(a) == !0u64 || own_body(a) || px::rigid_body_flags(a) & 1 == 0 {
        return; // ours, or not kinematic
    }
    seen.kinematic += 1;
    if COPIES[..COPY_N].iter().any(|c| c.source == a) {
        return; // an aggregate's member the scene listed too
    }
    let chest_s = [chest[0] - origin[0], chest[1] - origin[1], chest[2] - origin[2]];
    let b = px::world_bounds(a);
    let d2: f32 = (0..3).map(|k| out_of(chest_s[k], b.lo[k], b.hi[k])).map(|e| e * e).sum();
    let longest = (0..3).map(|k| b.hi[k] - b.lo[k]).fold(0.0f32, f32::max);
    if !(d2 <= REACH * REACH) || !(longest <= LONGEST) {
        return;
    }
    seen.near += 1;
    let physics = px::physics();
    let global = global_pose(a);
    let mut list = [core::ptr::null_mut(); MOST_SHAPES];
    let ns = px::shapes(a, &mut list).min(MOST_SHAPES as u32) as usize;
    for &sh in &list[..ns] {
        if !sh.is_null() && shape_flags(sh) & 5 == 1 {
            // a simulation shape, not a trigger
            let w = px::sim_filter(sh);
            seen.groups |= u64::from(w[0]) | (u64::from(w[1]) << 32);
            seen.masks |= u64::from(w[2]) | (u64::from(w[3]) << 32);
        }
    }
    let copy = at::<unsafe extern "C" fn(P, *const PxTransform) -> P>(physics, px::CREATE_RIGID_DYNAMIC)(physics, &global);
    if copy.is_null() {
        return;
    }
    at::<unsafe extern "C" fn(P, i32, bool)>(copy, px::SET_RIGID_BODY_FLAG)(copy, 1, true); // eKINEMATIC
    *((copy as *mut u8).add(0x10) as *mut usize) = !0usize; // no proxy of the game's
    let mut made = 0u32;
    for &sh in &list[..ns] {
        match copy_shape(sh, copy, &global, origin, chest, hips) {
            Some(true) => made += 1,
            Some(false) => seen.volumes += 1,
            None => {}
        }
    }
    if made == 0 {
        at::<unsafe extern "C" fn(P)>(copy, px::RELEASE)(copy);
        return;
    }
    // its place in the player's frame: the world's turn and place taken back by the entity's
    let (e_pos, e_rot) = frame;
    let inv = qconj(e_rot);
    let wp = [global.p[0] + origin[0] - e_pos[0], global.p[1] + origin[1] - e_pos[1], global.p[2] + origin[2] - e_pos[2]];
    let mut rq = qmul(&inv, &global.q);
    qnorm(&mut rq);
    let rel = PxTransform { q: rq, p: qrot(&inv, &wp) };
    at::<unsafe extern "C" fn(P, P, *const core::ffi::c_void)>(scene, px::ADD_ACTOR)(scene, copy, core::ptr::null());
    COPIES[COPY_N] = Copy { source: a, source_user: user_of(a), actor: copy, rel, shapes: made };
    COPY_N += 1;
    seen.shapes += made;
}

/// TakeCabin: the car's kinematic actors round the seat (the scene's dynamic actors and its aggregates' members), each
/// with its cabin shapes copied and placed in the player's frame.
unsafe fn take_cabin(sc: P, origin: &[f32; 3], frame: &([f32; 3], [f32; 4])) {
    let (e_pos, e_rot) = frame;
    let up = qrot(e_rot, &[0.0, 0.0, 1.0]);
    // the player's entity stands at the seat; the chest and the hips over it
    let chest = [e_pos[0] + up[0] * 0.75, e_pos[1] + up[1] * 0.75, e_pos[2] + up[2] * 0.75];
    let hips = [e_pos[0] + up[0] * 0.25, e_pos[1] + up[1] * 0.25, e_pos[2] + up[2] * 0.25];
    let mut filter = filter_person_hands(u64::from(hand_filter[0]) | (u64::from(hand_filter[1]) << 32), bit_person_left, bit_person_right, 3);
    if filter.hands == 0 {
        return;
    }
    // a pure mask: nothing reaches for the copies but the hands' people bits
    filter.words[0] = 0;
    filter.words[1] = 0;
    FILTER = filter.words;
    let mut seen = Seen::default();
    let mut start = 0u32;
    loop {
        let n = px::scene_actors(sc, 2, &mut PAGE_BUF, start).min(PAGE as u32) as usize;
        for i in 0..n {
            consider(PAGE_BUF[i], origin, frame, &chest, &hips, &mut seen);
        }
        if n < PAGE {
            break;
        }
        start += n as u32;
    }
    let ag = px::scene_aggregates(sc, &mut PAGE_BUF, 0).min(PAGE as u32) as usize;
    for i in 0..ag {
        let mut members = [core::ptr::null_mut(); 64];
        let m = px::aggregate_actors(PAGE_BUF[i], &mut members).min(64) as usize;
        for &member in &members[..m] {
            consider(member, origin, frame, &chest, &hips, &mut seen);
        }
    }
    CABIN_SHAPES.store(seen.shapes, Relaxed);
    CAR_GROUPS.store(seen.groups, Relaxed);
    // said when it differs from the last take's (an empty one is taken again every TAKE_EVERY_MS while the car stands)
    let said = (seen.shapes, seen.near, chest.map(|c| c.round() as i32));
    if said == LAST_SAID {
        return;
    }
    LAST_SAID = said;
    let hands_group = u64::from(hand_filter[0]) | (u64::from(hand_filter[1]) << 32) | bit_person_left | bit_person_right;
    log!(
        "[physhands] the car's own simulated parts round the seat, not met by the hands seated: groups {:016x}, masks {:016x}{}\n",
        seen.groups,
        seen.masks,
        if seen.masks & hands_group != 0 { " (their masks hold the hands' group: those pairs still meet)" } else { "" }
    );
    log!(
        "[physhands] the car's cabin for the hands: {} shapes of {} of its actors ({} kinematic actors round the seat, {} shapes left out as volumes round the player; {} actors seen, {} of them kinematic), in the player's frame; the chest at ({:.2} {:.2} {:.2})\n",
        seen.shapes,
        COPY_N,
        seen.near,
        seen.volumes,
        seen.scanned,
        seen.kinematic,
        chest[0],
        chest[1],
        chest[2]
    );
}

/// CabinStep: the cabin taken once the car stands, its copies posed in this step's player frame; dropped on foot.
pub unsafe fn cabin_step(sc: P, origin: &[f32; 3], hands_on: bool) {
    let seated = core::ptr::read_volatile(&raw const g_isInVehicle);
    // a motorcycle has no cabin, only the bars the hands hold
    let bike = seated && super::super::wheel_grab::cvr_anim_wheel_mounted_bike();
    if !seated || bike || !hands_on || !filter_ready || material.is_null() || scene.is_null() || sc != scene {
        release_all(if !seated {
            "out of the vehicle"
        } else if bike {
            "a motorcycle"
        } else {
            "the hands are off"
        });
        TAKEN_AT = 0;
        LAST_SAID = (u32::MAX, u32::MAX, [0; 3]);
        PACE_AT = 0;
        PACE_NOW = 0.0;
        return;
    }
    let tick = tick_ms();
    let frame = player_frame();
    // the car's pace from the player's frame itself (the player's own pace is the movement's, quiet seated)
    let mut pace = 0.0f32;
    if let Some((p, _)) = &frame {
        let dt = tick.wrapping_sub(PACE_AT) as f32 * 0.001;
        if PACE_AT != 0 && dt > 0.004 {
            let d2: f32 = (0..3).map(|k| (p[k] - PACE_FROM[k]) * (p[k] - PACE_FROM[k])).sum();
            pace = if dt < 0.5 { d2.sqrt() / dt } else { 0.0 };
            PACE_NOW = pace;
        } else {
            pace = PACE_NOW;
        }
        if PACE_AT == 0 || dt > 0.004 {
            PACE_FROM = *p;
            PACE_AT = tick;
        }
    }
    if COPY_N == 0 && (frame.is_none() || pace >= STILL) && tick.wrapping_sub(WAIT_TOLD_AT) >= 5000 {
        WAIT_TOLD_AT = tick;
        log!(
            "[physhands] the car's cabin waits: {}\n",
            if frame.is_none() { "no fresh frame of the player's".to_string() } else { format!("the player moves at {pace:.2} m/s") }
        );
    }
    let Some(frame) = frame else { return };
    // a copy whose car actor is gone (a door torn off, the car changed): all taken again at the next chance
    if COPIES[..COPY_N].iter().any(|c| !alive(c.source, c.source_user)) {
        release_all("a part of the car went");
        TAKEN_AT = 0;
    }
    if pace < STILL && COPY_N == 0 && (TAKEN_AT == 0 || tick.wrapping_sub(TAKEN_AT) >= TAKE_EVERY_MS) {
        TAKEN_AT = tick;
        take_cabin(sc, origin, &frame);
    }
    let (e_pos, e_rot) = frame;
    let mut posed = [PxTransform::ZERO; MOST_ACTORS];
    for (i, c) in COPIES[..COPY_N].iter().enumerate() {
        let p = qrot(&e_rot, &c.rel.p);
        let mut q = qmul(&e_rot, &c.rel.q);
        if !qnorm(&mut q) {
            continue;
        }
        let pose = PxTransform { q, p: [e_pos[0] + p[0] - origin[0], e_pos[1] + p[1] - origin[1], e_pos[2] + p[2] - origin[2]] };
        if (pose.p[0] + pose.p[1] + pose.p[2]).is_finite() {
            at::<unsafe extern "C" fn(P, *const PxTransform, bool)>(c.actor, px::SET_GLOBAL_POSE)(c.actor, &pose, true);
            posed[i] = pose;
        }
    }
    // the shapes as posed, for touches_world (scene space, as the near lists are)
    let bounds_of = world_bounds_fn();
    for i in 0..cabin_near_n {
        let c = &posed[SHAPE_COPY[i].min(MOST_ACTORS - 1)];
        let l = &SHAPE_LOCAL[i];
        let lp = qrot(&c.q, &l.p);
        let e = &mut cabin_near[i];
        e.pose = PxTransform { q: qmul(&c.q, &l.q), p: [c.p[0] + lp[0], c.p[1] + lp[1], c.p[2] + lp[2]] };
        e.bounds = bounds_of.map_or(PxBounds::default(), |f| f(e.geometry_ptr(), &e.pose, 1.0));
    }
}

/// Before the scene goes (a load): the copies are the scene's actors, released with it.
pub unsafe fn forget_for_scene() {
    COPIES = [COPY0; MOST_ACTORS];
    COPY_N = 0;
    cabin_near_n = 0;
    TAKEN_AT = 0;
    CABIN_SHAPES.store(0, Relaxed);
}
