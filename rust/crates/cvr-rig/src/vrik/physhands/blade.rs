//! A blade lodged in a body (physics thread, with the script's requests): a D6 joint the game's own factory makes
//! between the hand's body and the struck actor, its local X the one slide axis -- the shaft while inserting or
//! withdrawing, the authored cutting edge while cutting, locked while the depth is held -- its rail travel sampled for
//! the animated slot the visible weapon is pinned to (the animation thread), and the edge drive that carries the wrist
//! along a cut.
use super::body::{actor, simulating, weapon_half, weapon_local, weapon_on};
use super::math::{qconj, qmul, qnorm, qrot};
use super::pose::pending_frame;
use super::px::{self, at, PxTransform, P};
use super::tick_ms;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use cvr_engine::code::{guarded_call4, read};
use cvr_host::log;
use cvr_physhands::blade::{self as bl, BladeAttachment, BladeWristLeash, RailTravel};
use cvr_physhands::handspace::{self, conjugate, length_squared, multiply, rotate, Rotation, Vector};
use std::sync::Mutex;

fn v3(p: &[f32]) -> Vector {
    Vector::new(p[0], p[1], p[2])
}
fn r4(q: &[f32]) -> Rotation {
    Rotation::new(q[0], q[1], q[2], q[3])
}
fn weapon_revision() -> u64 {
    super::super::xinput::PRIMARY_WEAPON_REVISION.load(Relaxed)
}

#[derive(Clone, Copy, Debug)]
pub struct BladeRequest {
    pub at: u64,
    pub side: i32,
    pub serial: u32,
    pub proxy: u32,
    pub body: u32,
    pub point: [f32; 3],
    pub edge: [f32; 3],
    pub resistance: f32,
    pub hold_depth: bool,
    pub cut: bool,
    pub inserting: bool,
    pub edge_advance: f32,
    pub cutting_edge: [f32; 3],
}
const REQUEST0: BladeRequest = BladeRequest { at: 0, side: -1, serial: 0, proxy: 0, body: 0, point: [0.0; 3], edge: [0.0; 3], resistance: 0.0, hold_depth: false, cut: false, inserting: false, edge_advance: -1.0, cutting_edge: [0.0; 3] };
static BLADE_REQUEST: Mutex<BladeRequest> = Mutex::new(REQUEST0);
static BLADE_REQUESTED: AtomicBool = AtomicBool::new(false);
static BLADE_BOUND: AtomicU64 = AtomicU64::new(0);
fn blade_key(side: i32, serial: u32) -> u64 {
    ((side + 1) as u64) << 32 | u64::from(serial)
}

/// HeldAlive: the actor still the one it was taken as -- PhysX's dynamic table, and the (proxy, body) words the game
/// wrote in its userData.
pub unsafe fn held_alive(a: P, proxy: u32, body: u32) -> bool {
    !a.is_null() && px::dll() != 0 && *(a as *const usize) == px::dll() + px::DYNAMIC_TABLE && *((a as *const u8).add(0x10) as *const u32) == proxy && *((a as *const u8).add(0x14) as *const u16) == body as u16
}
/// HeldPose: an actor's pose, read under a guard; false when it faults or is not finite.
pub unsafe fn held_pose(a: P) -> Option<PxTransform> {
    let table = read::<usize>(a as usize)?;
    let f = read::<usize>(table + 8 * px::GET_GLOBAL_POSE)?;
    let mut out = PxTransform::default();
    guarded_call4(f, a, (&mut out as *mut PxTransform).cast(), core::ptr::null_mut(), core::ptr::null_mut())?;
    (out.p[0] + out.p[1] + out.p[2] + out.q[0] + out.q[1] + out.q[2] + out.q[3]).is_finite().then_some(out)
}

/// The game's own D6 factory, checked to be the one read: the image's stamp and size, the joint's table's slots, and the
/// create call's bytes (Cyberpunk2077.exe 2.5).
fn retail_d6() -> usize {
    static EXE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *EXE.get_or_init(|| {
        let base = cvr_engine::code::image_base();
        let Some(lfanew) = read::<u32>(base + 0x3c) else { return 0 };
        let nt = base + lfanew as usize;
        if read::<u32>(nt + 8) != Some(0x6AD5_5DAF) || read::<u32>(nt + 24 + 56) != Some(0x0581_B000) {
            return 0;
        }
        let slots = base + 0x342_3198;
        let slot = |i: usize| read::<usize>(slots + 8 * i);
        if slot(0) != Some(base + 0x1a1_5c20)
            || slot(6) != Some(base + 0x1a1_4040)
            || slot(12) != Some(base + 0x1a1_6040)
            || slot(16) != Some(base + 0x1a1_42e0)
            || slot(28) != Some(base + 0x1a1_5160)
            || slot(29) != Some(base + 0x1a1_1a50)
            || slot(30) != Some(base + 0x1a1_1a30)
        {
            return 0;
        }
        let mut create_call = [0u8; 10];
        if !cvr_engine::code::read_bytes(base + 0x1a1_138c, &mut create_call) || create_call != [0xff, 0x90, 0xf0, 0, 0, 0, 0x48, 0x89, 0x43, 0x60] {
            return 0;
        }
        base
    })
}

unsafe fn find_blade_body(sc: P, request: &BladeRequest) -> P {
    let matches = |c: P| held_alive(c, request.proxy, request.body);
    let mut batch = [core::ptr::null_mut(); 128];
    let mut start = 0u32;
    while start < 4096 {
        let count = px::scene_aggregates(sc, &mut batch, start);
        for &ag in batch.iter().take(count as usize) {
            let mut members = [core::ptr::null_mut(); 128];
            let size = px::aggregate_actors(ag, &mut members);
            if let Some(&m) = members.iter().take(size as usize).find(|&&m| matches(m)) {
                return m;
            }
        }
        if count < 128 {
            break;
        }
        start += 128;
    }
    let mut start = 0u32;
    while start < 16384 {
        let count = px::scene_actors(sc, 2, &mut batch, start);
        if let Some(&a) = batch.iter().take(count as usize).find(|&&a| matches(a)) {
            return a;
        }
        if count < 128 {
            break;
        }
        start += 128;
    }
    core::ptr::null_mut()
}
unsafe fn joint_frame(body: P, origin: &[f32; 3], request: &BladeRequest) -> Option<PxTransform> {
    let mut pose = held_pose(body)?;
    if !qnorm(&mut pose.q) {
        return None;
    }
    let inverse = qconj(&pose.q);
    let rel = [request.point[0] - origin[0] - pose.p[0], request.point[1] - origin[1] - pose.p[1], request.point[2] - origin[2] - pose.p[2]];
    let mut frame = PxTransform { q: [0.0; 4], p: qrot(&inverse, &rel) };
    let edge = request.edge;
    let mut world = [0.0f32, -edge[2], edge[1], 1.0 + edge[0]];
    if world[3] < 1e-6 {
        world = [0.0, 0.0, 1.0, 0.0];
    }
    if !qnorm(&mut world) {
        return None;
    }
    frame.q = qmul(&inverse, &world);
    qnorm(&mut frame.q).then_some(frame)
}

// ---- the joint (physics thread)
static mut blade_joint: P = core::ptr::null_mut();
static mut blade_depth_held: bool = false;
static mut blade_cutting: bool = false;
static mut cut_stroke: u32 = 0;
static mut coupled_stroke: u32 = 0;
static mut joint_proxy: u32 = 0;
static mut joint_body: u32 = 0;
static mut joint_key: u64 = 0;
static mut lookup_key: u64 = 0;
static mut lookup_after: u64 = 0;
#[derive(Clone, Copy, Debug, Default)]
struct JointSample {
    key: u64,
    at: u64,
    point: [f32; 3],
    cut_start: [f32; 3],
    stroke: u32,
    coupled_stroke: u32,
    generation: u64,
    body_rotation: Rotation,
    travel: Vector,
    edge_axis: [f32; 3],
    edge_stage: i32,
    holding_depth: bool,
}
#[derive(Clone, Copy, Debug, Default)]
struct AnimatedPin {
    key: u64,
    generation: u64,
    at: u64,
    applied_at: u64,
    origin: u64,
    owner: usize,
    attachment: BladeAttachment,
    leash: BladeWristLeash,
    settled: bool,
    quiet_since: u64,
    previous_controller: Vector,
    previous_anchor: Vector,
    last_controller_offset: Vector,
    settle_offset: Vector,
    weapon_revision: u64,
    goal: Vector,
    edge_offset: Vector,
    edge_axis: Vector,
    edge_stroke: u32,
}
struct Joint {
    sample: JointSample,
    pin: AnimatedPin,
}
/// jointSampleLock: the physics thread's sample and the animation thread's pin.
static JOINT: Mutex<Option<Joint>> = Mutex::new(None);
fn joint_lock() -> std::sync::MutexGuard<'static, Option<Joint>> {
    let mut g = JOINT.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(Joint { sample: JointSample::default(), pin: AnimatedPin::default() });
    }
    g
}
static mut rail_generation: u64 = 0;
static mut travel_station: Vector = Vector { x: 0.0, y: 0.0, z: 0.0 };
static mut rail_travel: RailTravel = RailTravel { last: Vector { x: 0.0, y: 0.0, z: 0.0 }, total: Vector { x: 0.0, y: 0.0, z: 0.0 } };
static ANIMATED_KEY: AtomicU64 = AtomicU64::new(0);
static ANIMATED_RELEASE_AT: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static DRAWN_BLADE_PIN: core::cell::RefCell<[(u64, u64, Vector); 2]> = const { core::cell::RefCell::new([(0, 0, Vector { x: 0.0, y: 0.0, z: 0.0 }); 2]) };
}
static mut joint_body_frame: PxTransform = PxTransform::ZERO;
static mut joint_hand_frame: PxTransform = PxTransform::ZERO;
/// last occupied shaft coordinate in the hand actor
static mut last_shaft_point: [f32; 3] = [0.0; 3];
static mut shaft_body_rotation: [f32; 4] = [0.0; 4];
static mut cut_start_command: f32 = 0.0;

unsafe fn joint_call0(joint: P, slot: usize) {
    at::<unsafe extern "C" fn(P)>(joint, slot)(joint);
}
unsafe fn create_blade_rail(hand: P, hand_frame: &PxTransform, target: P, target_frame: &PxTransform, hold: bool) -> P {
    let factory = retail_d6();
    if factory == 0 {
        return core::ptr::null_mut();
    }
    type Create = unsafe extern "C" fn(P, P, *const PxTransform, P, *const PxTransform) -> P;
    let create: Create = core::mem::transmute(factory + 0x1a1_12b0);
    let joint = create(px::physics(), hand, hand_frame, target, target_frame);
    if !joint.is_null() {
        // Tracking loss, withdrawal and verified separation release this joint. A hit-reaction impulse can otherwise
        // break it during the first solver step.
        let set_mode = at::<unsafe extern "C" fn(P, i32, i32)>(joint, 29);
        let get_mode = at::<unsafe extern "C" fn(P, i32) -> i32>(joint, 30);
        if !bl::configure_rail_translations(hold, |i, mode| set_mode(joint, i, mode), |i| get_mode(joint, i)) {
            joint_call0(joint, 0);
            return core::ptr::null_mut();
        }
    }
    joint
}
static mut PROBE: (P, P, P, P, u32) = (core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), 0);
/// ProbeBladeRail: a test tool (cpvr_test_blade_rail.flag): two bodies joined by a rail, one pushed, the axes logged.
unsafe fn probe_blade_rail(sc: P) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ENABLED.get_or_init(|| cvr_host::paths::test_marker("cpvr_test_blade_rail.flag")) {
        return;
    }
    let (owner, first, second, joint, steps) = &mut PROBE;
    if *steps > 6 || (!owner.is_null() && sc != *owner) {
        return;
    }
    let was = *steps;
    *steps += 1;
    if was == 0 {
        *owner = sc;
        let pose = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: [10.0, 10.0, 60.0] };
        let local = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: [0.0; 3] };
        let physics = px::physics();
        let make = at::<unsafe extern "C" fn(P, *const PxTransform) -> P>(physics, px::CREATE_RIGID_DYNAMIC);
        for slot in [&mut *first, &mut *second] {
            *slot = make(physics, &pose);
            if slot.is_null() {
                break;
            }
            *((*slot as *mut u8).add(0x10) as *mut usize) = !0usize;
            at::<unsafe extern "C" fn(P, i32, bool)>(*slot, px::SET_ACTOR_FLAG)(*slot, px::DISABLE_GRAVITY, true);
            at::<unsafe extern "C" fn(P, P, *const core::ffi::c_void)>(sc, px::ADD_ACTOR)(sc, *slot, core::ptr::null());
        }
        if !first.is_null() && !second.is_null() {
            *joint = create_blade_rail(*first, &local, *second, &local, false);
        }
        if !joint.is_null() {
            at::<unsafe extern "C" fn(P, *const f32, bool)>(*first, px::SET_LINEAR_VELOCITY)(*first, [0.9f32, 0.9, 0.0].as_ptr(), true);
            return;
        }
        *steps = 7;
    } else if *steps < 7 {
        return;
    }
    if !joint.is_null() {
        if let (Some(left), Some(right)) = (held_pose(*first), held_pose(*second)) {
            let mode = at::<unsafe extern "C" fn(P, i32) -> i32>(*joint, 30);
            log!(
                "[blade-rail-test] xA={:.6} xB={:.6} yA={:.6} yB={:.6} modeX={} modeY={}\n",
                left.p[0] - 10.0,
                right.p[0] - 10.0,
                left.p[1] - 10.0,
                right.p[1] - 10.0,
                mode(*joint, 0),
                mode(*joint, 1)
            );
        }
        joint_call0(*joint, 0);
    }
    if !first.is_null() {
        at::<unsafe extern "C" fn(P)>(*first, px::RELEASE)(*first);
    }
    if !second.is_null() {
        at::<unsafe extern "C" fn(P)>(*second, px::RELEASE)(*second);
    }
    *steps = 7;
}
/// ReleaseBladeJoint: the joint let go and every binding forgotten.
pub unsafe fn release_blade_joint() {
    BLADE_BOUND.store(0, Release);
    ANIMATED_KEY.store(0, Release);
    if !blade_joint.is_null() {
        joint_call0(blade_joint, 0);
    }
    blade_joint = core::ptr::null_mut();
    joint_key = 0;
    blade_depth_held = false;
    blade_cutting = false;
    coupled_stroke = 0;
}

/// RequestBladeJoint: the script's contact (a serial, the actor's proxy and body, the point and the edge) asked to be
/// joined -- or, with serial 0, this side's request taken back.
pub fn request_blade_joint(side: i32, serial: u32, proxy: u32, body: u32, point: Option<&[f32; 3]>, edge: Option<&[f32; 3]>) -> bool {
    if !(0..=1).contains(&side) {
        return false;
    }
    let mut next = REQUEST0;
    if let (true, Some(point), Some(edge)) = (serial != 0 && proxy != 0 && proxy != u32::MAX && body < 65536, point, edge) {
        let mut norm = 0.0f32;
        for i in 0..3 {
            if !point[i].is_finite() || !edge[i].is_finite() {
                return false;
            }
            norm += edge[i] * edge[i];
            next.point[i] = point[i];
        }
        if !(norm > 0.9 && norm < 1.1) {
            return false;
        }
        for i in 0..3 {
            next.edge[i] = edge[i] / norm.sqrt();
        }
        next.at = tick_ms();
        next.side = side;
        next.serial = serial;
        next.proxy = proxy;
        next.body = body;
        next.inserting = true;
        next.resistance = 1.0; // first solver step, before the script renews with its measured load
    }
    let mut r = BLADE_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    if next.serial == 0 && r.side != side {
        return false;
    }
    *r = next;
    BLADE_REQUESTED.store(next.serial != 0, Release);
    next.serial != 0
}
pub fn blade_joint_bound(side: i32, serial: u32) -> bool {
    serial != 0 && BLADE_BOUND.load(Acquire) == blade_key(side, serial)
}
pub fn read_blade_joint_point(side: i32, serial: u32) -> Option<[f32; 3]> {
    if !blade_joint_bound(side, serial) {
        return None;
    }
    let g = joint_lock();
    let s = &g.as_ref()?.sample;
    (s.key == blade_key(side, serial) && tick_ms().wrapping_sub(s.at) <= 100).then_some(s.point)
}
/// UpdateBladeAttachment (the animation thread): the animated slot the visible weapon is pinned to, the wrist's goal
/// from the D6's latest sample. 0 nothing, 1 recorded (no fresh D6 yet: the goal is the animated point), 2 driven, 3
/// driven and applied within 100 ms, -1 released.
pub fn update_blade_attachment(side: i32, serial: u32, animated: &[f32; 3], slot_rotation: &[f32; 4], station: &[f32; 3], bind: bool, goal: &mut [f32; 3]) -> i32 {
    if !(0..=1).contains(&side) || serial == 0 {
        return 0;
    }
    let key = blade_key(side, serial);
    let mut slot = r4(slot_rotation);
    if !handspace::normalize(&mut slot) || !handspace::finite(v3(animated)) || !handspace::finite(v3(station)) {
        return 0;
    }
    let pose = read_pose_inputs_any();
    let (input, wrist, entity) = (pose.target[side as usize], pose.drawn[side as usize], pose.frame.entity);
    let now = tick_ms();
    let mut rotation = r4(&wrist.q);
    if !entity.valid || input.at == 0 || wrist.at == 0 || now.wrapping_sub(input.at) > 100 || !handspace::normalize(&mut rotation) {
        return 0;
    }
    let controller = entity.position + rotate(entity.rotation, v3(&input.pos));
    let mut initial = AnimatedPin::default();
    if bind {
        if length_squared(v3(station) - v3(animated)) > 0.035 * 0.035 || length_squared(v3(station) - v3(&wrist.p)) > 1.65 * 1.65 {
            return 0;
        }
        initial.key = key;
        initial.owner = entity.owner;
        initial.origin = entity.origin;
        initial.weapon_revision = weapon_revision();
        initial.leash = BladeWristLeash { seated: v3(&wrist.p), controller };
        initial.previous_controller = controller;
        initial.previous_anchor = v3(animated);
        initial.quiet_since = now;
        initial.attachment.bind_wrist(v3(station), v3(&wrist.p), rotation);
    }
    let mut g = joint_lock();
    let Some(j) = g.as_mut() else { return 0 };
    if bind {
        j.pin = initial;
        ANIMATED_KEY.store(0, Release);
        ANIMATED_RELEASE_AT.store(0, Release);
    }
    if j.pin.key != key {
        return 0;
    }
    // Record the visible pair on first overlap, even when the D6 arrives a frame later. Only a fresh sample from the
    // matching D6 drives the wrist.
    if !blade_joint_bound(side, serial) || j.sample.key != key || now.wrapping_sub(j.sample.at) > 100 || j.sample.generation == 0 {
        ANIMATED_KEY.store(0, Release);
        *goal = *animated;
        return 1;
    }
    if j.pin.generation != j.sample.generation {
        j.pin.attachment.bind_rail(j.sample.travel, j.sample.body_rotation, slot);
        j.pin.generation = j.sample.generation;
    }
    j.pin.goal = j.pin.attachment.goal(v3(animated), slot, j.sample.travel) - j.pin.settle_offset;
    if !handspace::finite(j.pin.goal) {
        return 0;
    }
    let mut wrist_goal = j.pin.attachment.wrist(j.pin.goal, rotation);
    if entity.owner != j.pin.owner || entity.origin != j.pin.origin || j.pin.weapon_revision != weapon_revision() {
        ANIMATED_KEY.store(0, Release);
        return -1;
    }
    if !j.pin.leash.limit(controller, &mut wrist_goal) {
        if j.pin.applied_at != 0 && now.wrapping_sub(j.pin.applied_at) <= 100 {
            ANIMATED_RELEASE_AT.store(now, Release);
        }
        ANIMATED_KEY.store(0, Release);
        return -1;
    }
    // Finish seating at the resting pose, after hand and slot have settled. Movement of the NPC alone must never
    // continually recenter this limit.
    if length_squared(controller - j.pin.previous_controller) > 0.008 * 0.008 {
        j.pin.previous_controller = controller;
        j.pin.quiet_since = now;
        j.pin.settled = false;
    }
    if !j.pin.settled {
        if length_squared(v3(animated) - j.pin.previous_anchor) > 0.006 * 0.006 {
            j.pin.previous_anchor = v3(animated);
            j.pin.quiet_since = now;
        }
        if j.sample.holding_depth && now.wrapping_sub(j.pin.quiet_since) >= 160 {
            let correction = j.pin.attachment.wrist(j.pin.goal, rotation) - wrist_goal;
            j.pin.settle_offset = j.pin.settle_offset + correction;
            j.pin.goal = j.pin.goal - correction;
            j.pin.leash = BladeWristLeash { seated: wrist_goal, controller };
            j.pin.settled = true;
        }
    }
    // The very physics sample that positioned this rendered wrist goal; offsets expressed in the bound slot are
    // comparable across NPC turns.
    j.pin.edge_stroke = j.sample.stroke;
    if j.sample.stroke != 0 {
        let world_to_slot = multiply(j.pin.attachment.body_to_slot, conjugate(j.sample.body_rotation));
        j.pin.edge_offset = rotate(world_to_slot, v3(&j.sample.point) - v3(&j.sample.cut_start));
        j.pin.edge_axis = rotate(world_to_slot, v3(&j.sample.edge_axis));
    }
    j.pin.at = now;
    *goal = (wrist_goal + rotate(rotation, j.pin.attachment.wrist_local)).to_array();
    ANIMATED_KEY.store(key, Release);
    if j.pin.applied_at != 0 && now.wrapping_sub(j.pin.applied_at) <= 100 {
        3
    } else {
        2
    }
}
/// The published pose snapshot (under its lock), whatever its age (UpdateBladeAttachment checks the input's own).
fn read_pose_inputs_any() -> super::types::PoseInputs {
    super::pose::published_pose_inputs()
}
/// ApplyBladeAttachment (VRIK_ApplyHandStop): the wrist target put where the pin has it -- or, just released, eased from
/// the last displayed offset back to the live controller over 160 ms.
pub fn apply_blade_attachment(side: i32, target: &mut [f32; 3], rotation: &[f32; 4]) -> bool {
    let key = ANIMATED_KEY.load(Acquire);
    let Some(frame) = pending_frame() else { return false };
    let pending = super::pose::pending_inputs();
    if key == 0 {
        let start = ANIMATED_RELEASE_AT.load(Acquire);
        let now = tick_ms();
        if start == 0 || now.wrapping_sub(start) >= 160 {
            return false;
        }
        let g = joint_lock();
        let Some(j) = g.as_ref() else { return false };
        let entity = frame.entity;
        let input = pending.target[side as usize];
        if input.at == 0 || (j.pin.key >> 32) != (side + 1) as u64 || start != ANIMATED_RELEASE_AT.load(Relaxed) || entity.owner != j.pin.owner || entity.origin != j.pin.origin || j.pin.weapon_revision != weapon_revision() {
            return false;
        }
        // The hand body can still carry momentum from the NPC on the first free step. Start from the last displayed
        // offset and follow live input.
        *target = bl::release_wrist(v3(target), v3(&input.pos), j.pin.last_controller_offset, now.wrapping_sub(start) as f32 / 160.0).to_array();
        return true;
    }
    if (key >> 32) != (side + 1) as u64 || !BLADE_REQUESTED.load(Acquire) || BLADE_BOUND.load(Acquire) != key {
        return false;
    }
    let mut g = joint_lock();
    let Some(j) = g.as_mut() else { return false };
    let now = tick_ms();
    if j.pin.key != key || now.wrapping_sub(j.pin.at) > 100 || j.sample.key != key || j.pin.generation != j.sample.generation || now.wrapping_sub(j.sample.at) > 100 {
        return false;
    }
    let entity = frame.entity;
    if entity.owner != j.pin.owner || entity.origin != j.pin.origin {
        return false;
    }
    let mut wq = multiply(entity.rotation, r4(rotation));
    if !handspace::normalize(&mut wq) {
        return false;
    }
    let input = pending.target[side as usize];
    if input.at == 0 {
        return false;
    }
    let controller = entity.position + rotate(entity.rotation, v3(&input.pos));
    let mut wrist_goal = j.pin.attachment.wrist(j.pin.goal, wq);
    j.pin.leash.limit(controller, &mut wrist_goal);
    let p = rotate(conjugate(entity.rotation), wrist_goal - entity.position);
    if !handspace::finite(p) {
        return false;
    }
    *target = p.to_array();
    DRAWN_BLADE_PIN.with(|d| d.borrow_mut()[side as usize] = (key, pending.at, p));
    j.pin.applied_at = now;
    j.pin.last_controller_offset = p - v3(&input.pos);
    true
}
pub fn blade_attachment_matches(side: i32, target: &[f32; 3]) -> bool {
    let (key, at_, p) = DRAWN_BLADE_PIN.with(|d| d.borrow()[side as usize]);
    key != 0 && pending_frame().is_some() && at_ == super::pose::pending_inputs().at && key == ANIMATED_KEY.load(Acquire) && length_squared(p - v3(target)) < 1e-8
}
/// ReadBladeEdgePath: the cut's start and its current point, its axis and stage; the stroke (0 none).
pub fn read_blade_edge_path(side: i32, serial: u32) -> Option<(u32, [f32; 3], [f32; 3], [f32; 3], i32)> {
    let g = joint_lock();
    let s = &g.as_ref()?.sample;
    if !blade_joint_bound(side, serial) || s.key != blade_key(side, serial) || tick_ms().wrapping_sub(s.at) > 100 {
        return None;
    }
    Some((s.stroke, s.cut_start, s.point, s.edge_axis, s.edge_stage))
}
pub fn blade_edge_stroke_live(side: i32, serial: u32, stroke: u32) -> bool {
    let g = joint_lock();
    let Some(j) = g.as_ref() else { return false };
    stroke != 0 && blade_joint_bound(side, serial) && j.sample.key == blade_key(side, serial) && j.sample.coupled_stroke == stroke && tick_ms().wrapping_sub(j.sample.at) <= 100
}
/// ReadBladeSlotEdge: the cut's offset and axis in the bound slot; the stroke (0 none).
pub fn read_blade_slot_edge(side: i32, serial: u32) -> Option<(u32, [f32; 3], [f32; 3])> {
    let key = blade_key(side, serial);
    let g = joint_lock();
    let j = g.as_ref()?;
    if !blade_joint_bound(side, serial) || ANIMATED_KEY.load(Acquire) != key || j.pin.key != key || j.pin.generation != j.sample.generation || tick_ms().wrapping_sub(j.pin.at) > 100 || j.pin.edge_stroke == 0 || j.pin.edge_stroke != j.sample.stroke {
        return None;
    }
    Some((j.pin.edge_stroke, j.pin.edge_offset.to_array(), j.pin.edge_axis.to_array()))
}
/// RenewBladeJoint: the script's measured load, the depth held, a cut's edge, inserting, the edge's advance.
pub fn renew_blade_joint(side: i32, serial: u32, resistance: f32, hold_depth: bool, cutting_edge: Option<&[f32; 3]>, inserting: bool, edge_advance: f32) -> bool {
    let mut r = BLADE_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    if !blade_joint_bound(side, serial) || r.serial != serial || r.side != side || tick_ms().wrapping_sub(r.at) > 100 || !resistance.is_finite() || resistance < 0.0 {
        return false;
    }
    if !edge_advance.is_finite() || edge_advance > 0.35 {
        return false;
    }
    if let Some(e) = cutting_edge {
        let n = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
        if !(n > 0.99 && n < 1.01) {
            return false;
        }
        r.cutting_edge = *e;
    }
    r.cut = cutting_edge.is_some();
    r.resistance = resistance;
    r.hold_depth = hold_depth;
    r.inserting = inserting;
    r.edge_advance = edge_advance;
    r.at = tick_ms();
    true
}

/// BladeEdgeTarget (physics thread): while a cut opens, the wrist driven along the edge from the solver contact. Steer
/// still applies the ordinary bounded velocity and force budgets: never moves the actor directly to an XR displacement.
pub unsafe fn blade_edge_target(side: usize, sc: P, now: &PxTransform, max_gap: f32, to: &mut PxTransform) -> bool {
    if blade_joint.is_null() || !BLADE_REQUESTED.load(Acquire) {
        return false;
    }
    let request = *BLADE_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    if request.side != side as i32 || joint_key != blade_key(side as i32, request.serial) || tick_ms().wrapping_sub(request.at) > 100 || request.proxy != joint_proxy || request.body != joint_body {
        return false;
    }
    let opening = request.cut && request.edge_advance >= 0.0;
    if !opening && !(blade_cutting && cut_start_command >= 0.0) {
        return false;
    }
    // First remove the stored raw-tracking error while the old axes are still locked. Do the same for the last step
    // before the rail relocks.
    if !blade_cutting || !opening {
        *to = *now;
        return true;
    }
    let mut hand = core::ptr::null_mut();
    let mut body = core::ptr::null_mut();
    at::<unsafe extern "C" fn(P, *mut P, *mut P)>(blade_joint, 6)(blade_joint, &mut hand, &mut body);
    if hand != actor[side] || !held_alive(body, joint_proxy, joint_body) || at::<unsafe extern "C" fn(P) -> P>(blade_joint, 28)(blade_joint) != sc {
        return false;
    }
    let Some(bp) = held_pose(body) else { return false };
    let rotation = r4(&bp.q);
    let start = v3(&bp.p) + rotate(rotation, v3(&joint_body_frame.p));
    let advance = (request.edge_advance - cut_start_command).clamp(0.0, 0.35);
    let (goal, mut wrist) = bl::edge_drive_wrist(start, multiply(rotation, r4(&joint_body_frame.q)), v3(&joint_hand_frame.p), r4(&joint_hand_frame.q), advance);
    let mut delta = goal - v3(&now.p);
    let gap = length_squared(delta).sqrt();
    if !gap.is_finite() || !handspace::normalize(&mut wrist) {
        *to = *now;
        return true;
    }
    if gap > max_gap {
        delta = delta * (max_gap / gap); // stay in the velocity drive even after a large reaction
    }
    to.p = (v3(&now.p) + delta).to_array();
    to.q = wrist.to_array();
    true
}

/// SetBladeRailMode: both local anchors rebased at the occupied blade point when changing rails. Local X is the only
/// slide axis. For a cut X is the authored edge; the shaft and the blade normal both lie in the locked YZ plane. 0 done
/// (or nothing to do), 3 no axis, 4 no joint.
unsafe fn set_blade_rail_mode(request: &BladeRequest, hand: P, body: P, pose: &PxTransform, palm: &PxTransform, origin: &[f32; 3], blade_point: &[f32; 3], body_point: &[f32; 3]) -> i32 {
    let hold = request.hold_depth && !request.cut;
    if request.cut == blade_cutting && hold == blade_depth_held {
        return 0;
    }
    let mut frame = *request;
    frame.point = *blade_point;
    let shaft_rotation = qmul(&pose.q, &shaft_body_rotation);
    let shaft = qrot(&shaft_rotation, &[1.0, 0.0, 0.0]);
    if request.cut && request.edge_advance >= 0.0 {
        // weaponLocal is the pose of the actually simulated weapon boxes in the hand. Both the shaft and edge were
        // orthogonalized in item space.
        let weapon_q = multiply(r4(&palm.q), r4(&weapon_local[request.side as usize][0].q));
        let axis = rotate(weapon_q, v3(&request.cutting_edge));
        if !handspace::finite(axis) || (length_squared(axis) - 1.0).abs() > 0.01 {
            return 3;
        }
        frame.edge = axis.to_array();
    } else {
        match bl::select_rail_axis(&shaft, if request.cut { Some(&request.cutting_edge) } else { None }) {
            Some(a) => frame.edge = a,
            None => return 3,
        }
    }
    let Some(next_hand) = joint_frame(hand, origin, &frame) else { return 4 };
    frame.point = *body_point;
    let Some(next_body) = joint_frame(body, origin, &frame) else { return 4 };
    let replacement = create_blade_rail(hand, &next_hand, body, &next_body, hold);
    if replacement.is_null() {
        return 4;
    }
    joint_call0(blade_joint, 0);
    blade_joint = replacement;
    joint_hand_frame = next_hand;
    joint_body_frame = next_body;
    blade_cutting = request.cut;
    blade_depth_held = hold;
    if blade_cutting {
        cut_stroke = cut_stroke % 0x00ff_ffff + 1; // exactly representable in a script Vector4
        coupled_stroke = cut_stroke;
        cut_start_command = request.edge_advance;
    }
    0
}
unsafe fn blade_contains_point(side: usize, origin: &[f32; 3], point: &[f32; 3]) -> bool {
    let Some(palm) = held_pose(actor[side]) else { return false };
    let inverse = qconj(&palm.q);
    let delta = [point[0] - origin[0] - palm.p[0], point[1] - origin[1] - palm.p[1], point[2] - origin[2] - palm.p[2]];
    let local = qrot(&inverse, &delta);
    for b in 0..weapon_on[side].max(0) as usize {
        let shape_pose = weapon_local[side][b];
        let inv = qconj(&shape_pose.q);
        let d = [local[0] - shape_pose.p[0], local[1] - shape_pose.p[1], local[2] - shape_pose.p[2]];
        if bl::inside_weapon_box(&qrot(&inv, &d), &weapon_half[side][b]) {
            return true;
        }
    }
    false
}
/// BladeCutWitness: a rotating pair can have transverse solver error at the two old anchors. Start at a material witness
/// shared by this actor and this weapon: the two existing anchors first, then the nearest point of each finite box to
/// the body anchor. Never transfer to a different actor or a remote box.
unsafe fn blade_cut_witness(side: usize, body: P, palm: &PxTransform, origin: &[f32; 3], hand_point: &[f32; 3], body_point: &[f32; 3]) -> Option<[f32; 3]> {
    let contact = |p: &[f32; 3]| -> Option<[f32; 3]> {
        let (surface, gap) = super::hold::body_surface(body, origin, p)?;
        (gap <= 0.045 && length_squared(v3(&surface) - v3(body_point)) <= 0.10 * 0.10 && blade_contains_point(side, origin, &surface)).then_some(surface)
    };
    if let Some(w) = contact(hand_point).or_else(|| contact(body_point)) {
        return Some(w);
    }
    let hand = r4(&palm.q);
    let center = v3(&palm.p) + v3(origin);
    let local = rotate(conjugate(hand), v3(body_point) - center);
    for i in 0..weapon_on[side].max(0) as usize {
        let b = weapon_local[side][i];
        let box_q = r4(&b.q);
        let p = rotate(conjugate(box_q), local - v3(&b.p));
        let half = v3(&weapon_half[side][i]);
        let in_box = Vector::new(p.x.clamp(-half.x, half.x), p.y.clamp(-half.y, half.y), p.z.clamp(-half.z, half.z));
        if length_squared(p - in_box) > 0.10 * 0.10 {
            continue;
        }
        let world = (center + rotate(hand, v3(&b.p) + rotate(box_q, in_box))).to_array();
        if let Some(w) = contact(&world) {
            return Some(w);
        }
    }
    None
}

static mut ATTEMPTS: u32 = 0;
static mut REPAIRED: (u64, u64) = (0, 0);
fn trace_blade() -> bool {
    static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *T.get_or_init(|| cvr_host::paths::test_marker("cpvr_blade_owner_trace.flag"))
}
/// StepBladeJoint (physics thread, before the step): the request held to its joint -- kept, repaired, rebased between
/// rails, sampled, its slide damped by the measured load -- or the joint made at the contact, or let go.
pub unsafe fn step_blade_joint(sc: P, origin: &[f32; 3], step: f32, allowed: bool, holding: i32) {
    probe_blade_rail(sc);
    if blade_joint.is_null() && !BLADE_REQUESTED.load(Acquire) {
        return;
    }
    let mut request = *BLADE_REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    let now = tick_ms();
    let side = request.side;
    let report = trace_blade() && request.serial != 0 && {
        ATTEMPTS += 1;
        ATTEMPTS <= 96
    };
    let su = side.max(0) as usize;
    if report {
        log!(
            "[blade-d6] serial={} side={} allowed={} sim={} boxes={} age={}\n",
            request.serial,
            side,
            i32::from(allowed),
            i32::from(side >= 0 && simulating[su]),
            if side >= 0 { weapon_on[su] } else { 0 },
            now.wrapping_sub(request.at)
        );
    }
    if !allowed || side < 0 || holding & (1 << side) != 0 || request.serial == 0 || now.wrapping_sub(request.at) > 100 || !simulating[su] || actor[su].is_null() || weapon_on[su] == 0 {
        if !blade_joint.is_null() {
            log!(
                "[blade-d6] release key={} allowed={} side={} holding={} serial={} age={} sim={} boxes={}\n",
                joint_key,
                i32::from(allowed),
                side,
                holding,
                request.serial,
                now.wrapping_sub(request.at),
                i32::from(side >= 0 && simulating[su]),
                if side >= 0 { weapon_on[su] } else { 0 }
            );
        }
        release_blade_joint();
        return;
    }
    let key = blade_key(side, request.serial);
    if !blade_joint.is_null() {
        let mut hand = core::ptr::null_mut();
        let mut body = core::ptr::null_mut();
        at::<unsafe extern "C" fn(P, *mut P, *mut P)>(blade_joint, 6)(blade_joint, &mut hand, &mut body);
        let mut joint_flags = 0u16;
        at::<unsafe extern "C" fn(P, *mut u16) -> P>(blade_joint, 16)(blade_joint, &mut joint_flags);
        let same_pair = hand == actor[su] && !body.is_null() && joint_key == key && joint_proxy == request.proxy && joint_body == request.body && held_alive(body, request.proxy, request.body);
        let get_scene = |a: P| at::<unsafe extern "C" fn(P) -> P>(a, px::GET_SCENE)(a);
        if joint_flags & 1 != 0 && same_pair && !blade_cutting && (request.hold_depth || request.inserting) && get_scene(hand) == sc && get_scene(body) == sc {
            // A broken constraint may still report both live actors. Restore their own local anchors only while they
            // are within one short reaction step. A changed actor must pass contact acquisition.
            let (repaired_key, repaired_at) = REPAIRED;
            if repaired_key != key || now.wrapping_sub(repaired_at) >= 125 {
                if let (Some(hp), Some(bp)) = (held_pose(hand), held_pose(body)) {
                    let mut hf = joint_hand_frame;
                    if !blade_depth_held {
                        hf.p = last_shaft_point;
                    }
                    let mut h = qrot(&hp.q, &hf.p);
                    let mut b = qrot(&bp.q, &joint_body_frame.p);
                    let mut gap2 = 0.0f32;
                    for i in 0..3 {
                        h[i] += hp.p[i] + origin[i];
                        b[i] += bp.p[i] + origin[i];
                        gap2 += (h[i] - b[i]) * (h[i] - b[i]);
                    }
                    if gap2 <= 0.15 * 0.15 && blade_contains_point(su, origin, &h) {
                        let next = create_blade_rail(hand, &hf, body, &joint_body_frame, true);
                        if !next.is_null() {
                            joint_call0(blade_joint, 0);
                            blade_joint = next;
                            joint_hand_frame = hf;
                            blade_depth_held = true;
                            coupled_stroke = 0;
                            REPAIRED = (key, now);
                            joint_flags = 0;
                            log!("[blade-d6] restored broken constraint key={} gap={:.3}\n", key, gap2.sqrt());
                        }
                    }
                }
            }
        }
        if joint_flags & 1 == 0 && same_pair && at::<unsafe extern "C" fn(P) -> P>(blade_joint, 28)(blade_joint) == sc {
            let body_flags = px::rigid_body_flags(body);
            if let Some(pose) = held_pose(body) {
                let mut sample = JointSample { key, at: now, ..Default::default() };
                let p = qrot(&pose.q, &joint_body_frame.p);
                for i in 0..3 {
                    sample.point[i] = p[i] + pose.p[i] + origin[i];
                }
                let Some(palm) = held_pose(hand) else {
                    release_blade_joint();
                    return;
                };
                let hp = qrot(&palm.q, &joint_hand_frame.p);
                let hand_point = [hp[0] + palm.p[0] + origin[0], hp[1] + palm.p[1] + origin[1], hp[2] + palm.p[2] + origin[2]];
                let body_q = r4(&pose.q);
                let station_in_body = rotate(conjugate(body_q), v3(&palm.p) - v3(&pose.p) + rotate(r4(&palm.q), travel_station));
                let axis_in_body = rotate(r4(&joint_body_frame.q), Vector::new(1.0, 0.0, 0.0));
                rail_travel.sample(station_in_body, axis_in_body, !blade_depth_held);
                sample.travel = rail_travel.total;
                sample.generation = rail_generation;
                sample.body_rotation = body_q;
                let mut blade_point = hand_point;
                if blade_cutting {
                    sample.edge_axis = rotate(body_q, axis_in_body).to_array();
                    sample.cut_start = sample.point;
                    sample.point = hand_point;
                    match super::hold::body_surface(body, origin, &hand_point) {
                        Some((_, gap)) if gap <= 0.045 => {}
                        _ => {
                            log!("[blade-d6] edge left actor key={}\n", key);
                            release_blade_joint();
                            return;
                        }
                    }
                    // The path endpoint is the material point on this hand's actual weapon collider. A lagging
                    // render / IK centerline can be several centimetres away during a puppet reaction.
                    if blade_contains_point(su, origin, &hand_point) {
                        sample.stroke = cut_stroke;
                        sample.edge_stage = 5;
                    } else {
                        sample.edge_stage = 6;
                    }
                } else if !blade_depth_held {
                    // Transverse solver error belongs to the D6. Testing the remote actor point against the hand boxes
                    // here released the shaft as soon as an animated target stepped sideways.
                    let rotation = qmul(&palm.q, &joint_hand_frame.q);
                    let axis = qrot(&rotation, &[1.0, 0.0, 0.0]);
                    let (slide, bp) = bl::shaft_point(&hand_point, &sample.point, &axis);
                    blade_point = bp;
                    if blade_contains_point(su, origin, &blade_point) {
                        let inverse = qconj(&palm.q);
                        let delta = [blade_point[0] - palm.p[0] - origin[0], blade_point[1] - palm.p[1] - origin[1], blade_point[2] - palm.p[2] - origin[2]];
                        last_shaft_point = qrot(&inverse, &delta);
                    } else if request.inserting || request.hold_depth || request.cut {
                        // A reaction pulled the actor off the free axis before the next script update. Keep the last
                        // physical point on each actor and let the solver close their gap.
                        let r = qrot(&palm.q, &last_shaft_point);
                        for i in 0..3 {
                            blade_point[i] = r[i] + palm.p[i] + origin[i];
                        }
                        request.cut = false;
                        request.hold_depth = true;
                    } else {
                        log!("[blade-d6] shaft withdrawn key={} slide={:.3}\n", key, slide);
                        release_blade_joint();
                        return;
                    }
                }
                if request.cut && !blade_cutting {
                    match blade_cut_witness(su, body, &palm, origin, &blade_point, &sample.point) {
                        Some(w) => {
                            blade_point = w;
                            sample.edge_stage = 1;
                        }
                        None => {
                            request.cut = false;
                            request.hold_depth = true;
                            sample.edge_stage = 2;
                        }
                    }
                }
                let body_point = if request.cut { blade_point } else { sample.point };
                let rejected = set_blade_rail_mode(&request, hand, body, &pose, &palm, origin, &blade_point, &body_point);
                if rejected != 0 && request.cut {
                    sample.edge_stage = rejected;
                }
                if !blade_cutting {
                    sample.stroke = 0;
                }
                sample.coupled_stroke = coupled_stroke;
                sample.holding_depth = blade_depth_held;
                // D6 leaves one axis free. The hand motor still needs a load on that axis when animation owns the other
                // actor's velocity.
                if !blade_depth_held && request.resistance > 0.0 {
                    let kinematic = body_flags & 1 != 0;
                    let rotation = qmul(&pose.q, &joint_body_frame.q);
                    let axis = qrot(&rotation, &[1.0, 0.0, 0.0]);
                    let mut hand_speed = [0.0f32; 3];
                    let mut body_speed = [0.0f32; 3];
                    at::<unsafe extern "C" fn(P, *mut f32)>(hand, px::GET_LINEAR_VELOCITY)(hand, hand_speed.as_mut_ptr());
                    at::<unsafe extern "C" fn(P, *mut f32)>(body, px::GET_LINEAR_VELOCITY)(body, body_speed.as_mut_ptr());
                    let hand_mass = px::mass(hand);
                    let body_mass = if kinematic { 0.0 } else { px::mass(body) };
                    let damping_rate = request.resistance * if request.inserting { 16.0 } else { 1.0 };
                    let previous_velocity = hand_speed;
                    if bl::damp_slide(&mut hand_speed, &mut body_speed, &axis, hand_mass, body_mass, damping_rate, step, kinematic) {
                        at::<unsafe extern "C" fn(P, *const f32, bool)>(hand, px::SET_LINEAR_VELOCITY)(hand, hand_speed.as_ptr(), false);
                        if !kinematic {
                            at::<unsafe extern "C" fn(P, *const f32, bool)>(body, px::SET_LINEAR_VELOCITY)(body, body_speed.as_ptr(), false);
                        }
                        if trace_blade() && request.inserting {
                            let removed = v3(&previous_velocity) - v3(&hand_speed);
                            log!(
                                "[blade-d6] insertion key={} kinematic={} load={:.3} step={:.5} velocityCorrection={:.4}\n",
                                joint_key,
                                i32::from(kinematic),
                                request.resistance,
                                step,
                                length_squared(removed).sqrt()
                            );
                        }
                    }
                }
                if let Some(j) = joint_lock().as_mut() {
                    j.sample = sample;
                }
                return;
            }
        }
        log!(
            "[blade-d6] detached key={} next={} flags={} proxy={}/{} body={}/{}\n",
            joint_key,
            key,
            joint_flags,
            joint_proxy,
            request.proxy,
            joint_body,
            request.body
        );
        release_blade_joint();
    }
    if key == lookup_key && now < lookup_after {
        return;
    }
    lookup_key = key;
    lookup_after = now + 125;
    let body = find_blade_body(sc, &request);
    if body.is_null() || body == actor[su] {
        return;
    }
    if report {
        log!("[blade-d6] target proxy={} body={} flags={}\n", request.proxy, request.body, px::rigid_body_flags(body));
    }
    match super::hold::body_surface(body, origin, &request.point) {
        Some((_, gap)) if gap <= 0.045 => {}
        _ => return,
    }
    if !blade_contains_point(su, origin, &request.point) {
        return;
    }
    let (Some(hand_frame), Some(body_frame)) = (joint_frame(actor[su], origin, &request), joint_frame(body, origin, &request)) else { return };
    blade_joint = create_blade_rail(actor[su], &hand_frame, body, &body_frame, false);
    if blade_joint.is_null() {
        return;
    }
    joint_key = key;
    joint_proxy = request.proxy;
    joint_body = request.body;
    joint_body_frame = body_frame;
    joint_hand_frame = hand_frame;
    shaft_body_rotation = body_frame.q;
    last_shaft_point = hand_frame.p;
    let Some(body_pose) = held_pose(body) else {
        release_blade_joint();
        return;
    };
    travel_station = v3(&hand_frame.p);
    rail_travel = RailTravel { last: v3(&body_frame.p), total: Vector::default() };
    rail_generation += 1;
    if let Some(j) = joint_lock().as_mut() {
        j.sample = JointSample { key, at: now, point: request.point, generation: rail_generation, body_rotation: r4(&body_pose.q), ..Default::default() };
    }
    BLADE_BOUND.store(key, Release);
    if trace_blade() {
        log!("[blade-d6] bound serial={} proxy={} body={}\n", request.serial, request.proxy, request.body);
    }
}
