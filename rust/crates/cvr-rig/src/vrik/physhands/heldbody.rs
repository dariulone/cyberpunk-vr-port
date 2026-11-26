//! HeldBody (main's src/Natives/HeldBody.cpp) -- a carried thing in the hand, the same frame as the hand: held by its own
//! body (hold_body, below), or its picture drawn on the hand while its body follows by velocity (hold, this first part).
//!
//! WHY. A body can only follow the hand from one physics step to the next, so it is a frame behind at best: 7 to 10 cm at
//! 3 m/s (measured 2026-11-04). The body has to stay a body, for walls and for the throw. Only its picture has to be on the
//! hand, and the picture is a separate thing.
//!
//! HOW THE PICTURE GETS THERE, read in the 2.5 exe. Loose props are worldInstancedDestructibleMeshNodeInstance. Each has a
//! listener at +0x100, registered with the physics system (at 0x143BFCDD0) in the table at +0x222018, by the low 16 bits
//! of the proxy id; its pose records are at +0x1A2018, {records, count}, 64 bytes each: position as a 32-bit fixed point
//! number at +0, quaternion at +0x10. sub_1405C3454(instance, span of records, count) turns those into one transform per
//! instance and sends them to the render proxy. It keeps nothing of them and reads the span only, so what is in the span
//! is what is drawn, and nobody else sees it.
//!   1. A detour on that function gives it a copy of the span with the carried instance changed, so the engine's own
//!      message never undoes the picture.
//!   2. It may run before the hand is animated, and does not run for a body at rest. So once more at the frame submit
//!      (sub_1403D6D84, game thread, before the proxy flush), with the hand as animated for this frame.
//!
//! A wrong picture is the most this can do: nothing here writes to the game's memory.
use super::super::debug_view;
use super::super::env::{g_VRFKSnapCount, g_VRFKSnapPos, g_VRFKSnapRot, g_VRLeftBoneIdx, g_VRRightBoneIdx};
use super::super::grasp;
use super::hold::{f_of, fbits, gcall};
use super::math::{qmul, qrot};
use super::px::{PxTransform, P};
use super::tick_ms;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use cvr_engine::code::{image_base, peek, read, read_bytes};
use cvr_host::log;
use cvr_physhands::mesh_profile::{profile_of, profile_points, PROFILE_POINTS};
use cvr_settings::lc;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::stack::StackFrame;
use std::sync::{Mutex, MutexGuard, OnceLock};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

unsafe extern "C" {
    static CyberpunkVR_PlayerEntityPos: [f32; 3];
    static CyberpunkVR_PlayerEntityQuat: [f32; 4];
    static CyberpunkVR_PlayerEntityValid: i32;
}

const UPDATE_RVA: usize = 0x5C3454;
const LISTENER_ENTRY_RVA: usize = 0x5C3010;
const SYSTEM_RVA: usize = 0x3BF_CDD0;
const LISTENERS: usize = 0x222018;
const RECORDS: usize = 0x1A2018;
const PROXY_ID: usize = 0x160;
const LISTENER_AT: usize = 0x100;
const RECORD_SIZE: usize = 64;
/// the instance's alive mask has 4 words
const MOST_INSTANCES: u32 = 256;
const FIXED: f64 = 131072.0;
/// s
const BLEND_IN: f32 = 0.08;
const BLEND_OUT: f32 = 0.1;
/// m: a picture further than this means this file has misread something, or the player has been moved
const FAR_FROM_BODY: f32 = 3.0;
const UNASKED_MS: u64 = 150;
/// rad/s: a wrist flick is above PhysX's 7, measured held to exactly that
const MOST_SPIN: f32 = 50.0;

#[derive(Clone, Copy, Default)]
struct Pose {
    p: [f32; 3],
    q: [f32; 4],
}
#[derive(Clone, Copy)]
struct Slot {
    used: bool,
    asked: bool,
    proxy: u32,
    body: u32,
    bone: i32,
    offset: [f32; 3],
    turn: [f32; 4],
    blocked: bool,
    reach: f32,
    /// The setting is off: the slot only measures (the debug view, the log), the picture stays on the body and nothing
    /// is changed.
    measure_only: bool,
    /// Held by its own body (hold_body): its own PhysX actor, dynamic, given at each step the velocities that bring it
    /// into the hand by the step's end, as far as a hand's force can (steer). The actor, its scene and that scene's
    /// origin in the game's world; whether it is being steered (its damping and spin ceiling changed, kept in `saved`);
    /// the throw it gets when let go (w 1: given, 0: it keeps the velocity it has).
    own_body: bool,
    driving: bool,
    saved: [f32; 3],
    throw_given: f32,
    actor: usize,
    scene: usize,
    origin: [f32; 3],
    throw_v: [f32; 3],
    throw_w: [f32; 3],
    targets: u32,
    drive_failed: u32,
    /// Its shapes' simulation filter data as it was, put back when it is let go (see filter).
    shapes: [usize; 8],
    filters: [[u32; 4]; 8],
    shape_count: u32,
    /// IN ONE HAND, SEATED FOR ITS GRASP (grasp.rs): the script asks for it while one hand holds; the hand's fingers
    /// close on its shape the way the hand came to it, and it eases from where it was taken (seat_from) to where that
    /// grasp has it (seat), both in the hand's frame (x y z, then x y z w). The script's offset and turn are not used
    /// while it is seated.
    grasp_wanted: bool,
    seated: bool,
    grasp_tried: bool,
    grasp_side: i32,
    grasp_kind: i32,
    seat_from: [f32; 7],
    seat: [f32; 7],
    seat_t: f32,
    asked_at: u64,
    /// 0 the body, 1 the hand
    weight: f32,
    posed: bool,
    hand: Pose,
    /// Let go: what is between the picture and the body, carried along with the body and closing.
    let_go: bool,
    rel_set: bool,
    spun: bool,
    rel_p: [f32; 3],
    rel_q: [f32; 4],
    // what was seen, for the log
    frames: u32,
    sends: u32,
    detours: u32,
    too_far: u32,
    blocked_frames: u32,
    instances: u32,
    first_tick: u64,
    detour_thread: u32,
    last_detour: u64,
    last_submit: u64,
    behind_sum: f64,
    behind_most: f64,
    detour_gap_sum: f64,
    detour_gaps: u32,
}
impl Slot {
    const EMPTY: Slot = Slot {
        used: false,
        asked: false,
        proxy: 0,
        body: 0,
        bone: 0,
        offset: [0.0; 3],
        turn: [0.0; 4],
        blocked: false,
        reach: 0.0,
        measure_only: false,
        own_body: false,
        driving: false,
        saved: [0.0; 3],
        throw_given: 0.0,
        actor: 0,
        scene: 0,
        origin: [0.0; 3],
        throw_v: [0.0; 3],
        throw_w: [0.0; 3],
        targets: 0,
        drive_failed: 0,
        shapes: [0; 8],
        filters: [[0; 4]; 8],
        shape_count: 0,
        grasp_wanted: false,
        seated: false,
        grasp_tried: false,
        grasp_side: 0,
        grasp_kind: 0,
        seat_from: [0.0; 7],
        seat: [0.0; 7],
        seat_t: 0.0,
        asked_at: 0,
        weight: 0.0,
        posed: false,
        hand: Pose { p: [0.0; 3], q: [0.0; 4] },
        let_go: false,
        rel_set: false,
        spun: false,
        rel_p: [0.0; 3],
        rel_q: [0.0; 4],
        frames: 0,
        sends: 0,
        detours: 0,
        too_far: 0,
        blocked_frames: 0,
        instances: 0,
        first_tick: 0,
        detour_thread: 0,
        last_detour: 0,
        last_submit: 0,
        behind_sum: 0.0,
        behind_most: 0.0,
        detour_gap_sum: 0.0,
        detour_gaps: 0,
    };
}
/// Fingers closed on a held thing, per hand: on while it is seated, eased off when it is let go or the other hand takes
/// it too.
#[derive(Clone, Copy)]
struct Fingers {
    blend: f32,
    on: bool,
    used: bool,
    at: u64,
}
struct State {
    slots: [Slot; 2],
    fingers: [Fingers; 2],
}
static LOCK: Mutex<State> = Mutex::new(State { slots: [Slot::EMPTY; 2], fingers: [Fingers { blend: 0.0, on: false, used: false, at: 0 }; 2] });
static ANY_USED: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);
static mut ORIGINAL_UPDATE: Option<UpdateFn> = None;
type UpdateFn = unsafe extern "C" fn(*mut u8, *mut c_void, u32);
#[repr(C)]
struct Span {
    begin: *mut u8,
    end: *mut u8,
}
fn lock() -> MutexGuard<'static, State> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
fn base() -> usize {
    if INSTALLED.load(Acquire) { image_base() } else { 0 }
}

fn unit_like(q: &[f32]) -> bool {
    let n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    n.is_finite() && n > 0.9 && n < 1.1
}
fn v3(p: &[f32]) -> [f32; 3] {
    [p[0], p[1], p[2]]
}
fn q4(p: &[f32]) -> [f32; 4] {
    [p[0], p[1], p[2], p[3]]
}
fn mptr<T>(v: &mut T) -> u64 {
    v as *mut T as u64
}
fn ptr<T>(v: &T) -> u64 {
    v as *const T as u64
}
// PxRigidBody / PxRigidDynamic slots (PhysX 3.4, counted from its headers; set_max_angular_velocity at 60 is the one
// debug_view already calls).
const GET_SHAPES: usize = 26;
const SHAPE_SET_SIM_FILTER: usize = 20;
const SHAPE_GET_SIM_FILTER: usize = 21;
const GET_GLOBAL_POSE: usize = 20;
const GET_CMASS_LOCAL_POSE: usize = 30;
const GET_MASS: usize = 32;
const GET_LINEAR_VELOCITY: usize = 37;
const SET_LINEAR_VELOCITY: usize = 38;
const GET_ANGULAR_VELOCITY: usize = 39;
const SET_ANGULAR_VELOCITY: usize = 40;
const SET_LINEAR_DAMPING: usize = 56;
const GET_LINEAR_DAMPING: usize = 57;
const SET_ANGULAR_DAMPING: usize = 58;
const GET_ANGULAR_DAMPING: usize = 59;
const SET_MAX_ANGULAR_VELOCITY: usize = 60;
const GET_MAX_ANGULAR_VELOCITY: usize = 61;
/// HOW HARD A HAND PULLS, and how fast it turns a thing: the cap on each step's change of velocity is this force over
/// the thing's mass (a light thing follows any swing exactly, a heavy one lags it), and never more than MOST_ACCEL. What
/// it runs into answers with its own mass: a can pushes a can aside, a crate stops it, a wall stops it.
const HAND_FORCE: f32 = 250.0; // N
const MOST_ACCEL: f32 = 1500.0; // m/s^2
const MOST_SPIN_ACCEL: f32 = 4000.0; // rad/s^2
/// rad/s: PhysX's spin ceiling while held (a wrist flick turns ~20 deg a frame)
const HELD_SPIN: f32 = 200.0;
struct SteerIn {
    target: [f32; 7],
    origin: [f32; 3],
    gravity: [f32; 3],
    dt: f32,
    first: bool,
}
unsafe fn get_f(actor: P, slot: usize) -> Option<f32> {
    gcall(actor, slot, 0, 0, 0).map(|(_, x)| f_of(x))
}
unsafe fn set_f(actor: P, slot: usize, v: f32) -> Option<()> {
    gcall(actor, slot, fbits(v), 0, 0).map(|_| ())
}
unsafe fn set_v(actor: P, slot: usize, v: &[f32; 3]) -> Option<()> {
    gcall(actor, slot, ptr(v), 1, 0).map(|_| ())
}
/// One step's velocities: those that bring the body's centre of mass and turn from where they are to the target's by
/// the end of the step (PhysX adds gravity before it moves the body: taken off; damping is set to none while held),
/// their change held to what a hand can give. Returns (gap: how far the body is from the target now (m), mass), None if
/// PhysX faulted.
unsafe fn steer(actor: P, input: &SteerIn, saved: &mut [f32; 3]) -> Option<(f32, f32)> {
    if input.first {
        saved[0] = get_f(actor, GET_LINEAR_DAMPING)?;
        saved[1] = get_f(actor, GET_ANGULAR_DAMPING)?;
        saved[2] = get_f(actor, GET_MAX_ANGULAR_VELOCITY)?;
        set_f(actor, SET_LINEAR_DAMPING, 0.0)?;
        set_f(actor, SET_ANGULAR_DAMPING, 0.0)?;
        set_f(actor, SET_MAX_ANGULAR_VELOCITY, HELD_SPIN)?;
    }
    let mut pose = PxTransform::ZERO;
    gcall(actor, GET_GLOBAL_POSE, mptr(&mut pose), 0, 0)?;
    // THE FIRST STEP ONLY TAKES IT. Its filter was changed this step (held apart from the loose things), and PhysX lets
    // go of its pairs with its neighbours in the next simulate: moved at once, a plate taken out of the middle of a stack
    // struck the ones on it at 1.3 m/s before they had let go of it. Held still this step, steered from the next.
    if input.first {
        let still = [-input.gravity[0] * input.dt, -input.gravity[1] * input.dt, -input.gravity[2] * input.dt];
        set_v(actor, SET_LINEAR_VELOCITY, &still)?;
        set_v(actor, SET_ANGULAR_VELOCITY, &[0.0; 3])?;
        let mass = get_f(actor, GET_MASS)?;
        let mut g2 = 0.0;
        for k in 0..3 {
            let d = input.target[k] - input.origin[k] - pose.p[k];
            g2 += d * d;
        }
        return Some((g2.sqrt(), mass));
    }
    let mut cm = PxTransform::ZERO;
    gcall(actor, GET_CMASS_LOCAL_POSE, mptr(&mut cm), 0, 0)?;
    let (mut v, mut w) = ([0.0f32; 3], [0.0f32; 3]);
    gcall(actor, GET_LINEAR_VELOCITY, mptr(&mut v), 0, 0)?;
    gcall(actor, GET_ANGULAR_VELOCITY, mptr(&mut w), 0, 0)?;
    let m = get_f(actor, GET_MASS)?;
    // the centre of mass now and where the target puts it, PhysX's frame
    let off = qrot(&pose.q, &cm.p);
    let at = [pose.p[0] + off[0], pose.p[1] + off[1], pose.p[2] + off[2]];
    let tq = q4(&input.target[3..]);
    let off = qrot(&tq, &cm.p);
    let to = [input.target[0] - input.origin[0] + off[0], input.target[1] - input.origin[1] + off[1], input.target[2] - input.origin[2] + off[2]];
    let dt = if input.dt > 1e-4 { input.dt } else { 1.0 / 60.0 };
    let (mut need, mut dv) = ([0.0f32; 3], [0.0f32; 3]);
    let mut gap2 = 0.0;
    for k in 0..3 {
        let d = input.target[k] - input.origin[k] - pose.p[k];
        gap2 += d * d;
        need[k] = (to[k] - at[k]) / dt;
        dv[k] = need[k] - v[k];
    }
    let gap = gap2.sqrt();
    let most = MOST_ACCEL.min(HAND_FORCE / m.max(1e-3)) * dt;
    let dvn = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
    let kv = if dvn > most { most / dvn } else { 1.0 };
    let mut set = [0.0f32; 3];
    for k in 0..3 {
        set[k] = v[k] + dv[k] * kv - input.gravity[k] * dt;
    }
    // the turn: from the body's to the target's, about the shortest way, in the step
    let conj = [-pose.q[0], -pose.q[1], -pose.q[2], pose.q[3]];
    let mut dq = qmul(&tq, &conj);
    if dq[3] < 0.0 {
        for c in dq.iter_mut() {
            *c = -*c;
        }
    }
    let s = (dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]).sqrt();
    let angle = 2.0 * s.atan2(dq[3]);
    let mut spin = [0.0f32; 3];
    if s > 1e-7 {
        for k in 0..3 {
            spin[k] = dq[k] / s * angle / dt;
        }
    }
    let dw = [spin[0] - w[0], spin[1] - w[1], spin[2] - w[2]];
    let most_w = MOST_SPIN_ACCEL * dt;
    let dwn = (dw[0] * dw[0] + dw[1] * dw[1] + dw[2] * dw[2]).sqrt();
    let kw = if dwn > most_w { most_w / dwn } else { 1.0 };
    let wset = [w[0] + dw[0] * kw, w[1] + dw[1] * kw, w[2] + dw[2] * kw];
    set_v(actor, SET_LINEAR_VELOCITY, &set)?;
    set_v(actor, SET_ANGULAR_VELOCITY, &wset)?;
    (set[0] + set[1] + set[2] + wset[0] + wset[1] + wset[2]).is_finite().then_some((gap, m))
}
/// PxScene::getGravity (slot 67, as debug_view calls it).
unsafe fn scene_gravity(scene: P) -> [f32; 3] {
    let mut g = [0.0f32; 3];
    if gcall(scene, 67, mptr(&mut g), 0, 0).is_some() && (g[0] + g[1] + g[2]).is_finite() && g[2].abs() > 1.0 && g[2].abs() < 30.0 {
        return g;
    }
    [0.0, 0.0, -9.81]
}
/// Let go: damping and the spin ceiling as they were, and the throw when one is given.
unsafe fn unsteer(actor: P, saved: &[f32; 3], velocity: &[f32; 3], spin: &[f32; 3], thrown: bool) -> bool {
    let run = || -> Option<()> {
        set_f(actor, SET_LINEAR_DAMPING, saved[0])?;
        set_f(actor, SET_ANGULAR_DAMPING, saved[1])?;
        set_f(actor, SET_MAX_ANGULAR_VELOCITY, saved[2].max(50.0))?;
        if thrown {
            set_v(actor, SET_LINEAR_VELOCITY, velocity)?;
            set_v(actor, SET_ANGULAR_VELOCITY, spin)?;
        }
        Some(())
    };
    run().is_some()
}
/// LOOSE PROPS DO NOT MEET EACH OTHER. Their shapes' simulation filter data (read 2026-11-08, cups, plates, cans) is
/// group 0x1000 in word 0 and 0xEC in word 2, and 0x1000 is not in 0xEC: one carried by velocity, or held kinematic, went
/// through another hung in its way six times out of six, either way. AND THAT IS KEPT, by default: the bits below are
/// added only when asked for (VRHeldBodyFilter; word 2 0xFFFFFFFF there means "its own group"). A held box let meet its
/// own kind crashed the game (2026-11-08 20:52): instances of one InstancedDestructibleNode stand in each other, since they
/// never collide, so the box taken from a stack met its neighbours inside them, PhysX threw them apart, two of them broke,
/// and the player's character controller ran its next query into one of the actors the game had just freed for that.
/// Let go, the shapes get back what they had.
static HELD_FILTER_OR: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
/// A HELD THING MEETS NO LOOSE THING (PhysHands' held_alone): the stall's plates (group 1c008, mask 13cf) do meet each
/// other, and one pulled out of the middle of a stack threw the rest off the counter. Held, it is in the hands' group with
/// the world's mask: taken alone, it still stops at a wall and a table. Let go, it has its own filter again.
unsafe fn filter(actor: P, shapes: &mut [usize; 8], saved: &mut [[u32; 4]; 8], count: &mut u32) -> bool {
    let or = [HELD_FILTER_OR[0].load(Relaxed), HELD_FILTER_OR[1].load(Relaxed)];
    let adding = or[0] != 0 || or[1] != 0;
    let mut run = || -> Option<bool> {
        let n = gcall(actor, GET_SHAPES, shapes.as_mut_ptr() as u64, 8, 0)?.0 as u32;
        *count = n.min(8);
        let mut any = false;
        for i in 0..*count as usize {
            let shape = shapes[i] as P;
            gcall(shape, SHAPE_GET_SIM_FILTER, mptr(&mut saved[i]), 0, 0)?;
            let mut data = saved[i];
            if adding {
                data[0] |= or[0];
                data[2] |= if or[1] == 0xFFFF_FFFF { saved[i][0] } else { or[1] };
            } else {
                match super::api::held_alone(&saved[i]) {
                    Some(d) => data = d,
                    None => continue,
                }
            }
            gcall(shape, SHAPE_SET_SIM_FILTER, ptr(&data), 0, 0)?;
            any = true;
        }
        Some(any)
    };
    match run() {
        Some(true) => true,
        _ => {
            *count = 0;
            false
        }
    }
}
unsafe fn unfilter(shapes: &[usize; 8], saved: &[[u32; 4]; 8], count: u32) -> bool {
    (0..count as usize).all(|i| gcall(shapes[i] as P, SHAPE_SET_SIM_FILTER, ptr(&saved[i]), 0, 0).is_some())
}

// ---- THE SHAPE A HAND CLOSES ON: the actor's own shapes, as one primitive in the actor's frame.
// PxShape slots, read off NpShape's table in PhysX3_x64.dll (getConcreteTypeName's "PxShape" at 1, getReferenceCount at
// 6, so one more than the 3.4 header counts): getGeometryType 7, getBoxGeometry 10, getSphereGeometry 11,
// getCapsuleGeometry 12, getConvexMeshGeometry 14, getLocalPose 19 (returned through a hidden pointer).
// Gu::ConvexMesh (PhysX3Common_x64.dll): getLocalBounds 13, the same way; getNbVertices 5 and getVertices 6 (PxBase's
// five slots, then the mesh's own). Geometry types: 0 sphere, 2 capsule (along its X), 3 box, 4 convex mesh.
// PxConvexMeshGeometry: its scale at +4 (the scale's own turn at +16), the mesh at +32.
const SHAPE_GEOMETRY_TYPE: usize = 7;
const SHAPE_GET_BOX: usize = 10;
const SHAPE_GET_SPHERE: usize = 11;
const SHAPE_GET_CAPSULE: usize = 12;
const SHAPE_GET_CONVEX: usize = 14;
const SHAPE_LOCAL_POSE: usize = 19;
const CONVEX_LOCAL_BOUNDS: usize = 13;
const CONVEX_NB_VERTICES: usize = 5;
const CONVEX_VERTICES: usize = 6;
/// a convex mesh has at most 255 points
pub const HULL_MOST: usize = 256;
/// A convex shape's mesh and how it is scaled into the shape, and the middle of its bounds there (the box's centre).
#[derive(Clone, Copy)]
struct Hull {
    mesh: usize,
    scale: [f32; 3],
    turn: [f32; 4],
    centre: [f32; 3],
}
impl Hull {
    const NONE: Hull = Hull { mesh: 0, scale: [1.0; 3], turn: [0.0, 0.0, 0.0, 1.0], centre: [0.0; 3] };
}
/// m: a thing with a half size past this is not one hand's (a crate): no grasp
const GRASP_LARGEST: f32 = 0.3;
/// two half sizes this near (share of the larger) are a round section...
const ROUND: f32 = 0.15;
/// ...and the third this much longer: a can, a bottle, a cylinder
const LONG: f32 = 1.2;
/// s from where it was taken to its seat
const SEAT_SEC: f32 = 0.12;
#[repr(C, align(8))]
struct Geometry64([u8; 64]);
/// One shape as a box (or a ball, a capsule) in the actor's frame: (kind: 0 box, 1 sphere, 2 capsule along its X, half
/// sizes, local pose).
unsafe fn shape_box(shape: P, hull: Option<&mut Hull>) -> Option<(i32, [f32; 3], PxTransform)> {
    let kind_of = gcall(shape, SHAPE_GEOMETRY_TYPE, 0, 0, 0)?.0 as i32;
    let mut local = PxTransform::ZERO;
    gcall(shape, SHAPE_LOCAL_POSE, mptr(&mut local), 0, 0)?;
    match kind_of {
        3 => {
            let mut g = [0.0f32; 4];
            if gcall(shape, SHAPE_GET_BOX, mptr(&mut g), 0, 0)?.0 as u8 == 0 {
                return None;
            }
            Some((0, [g[1], g[2], g[3]], local))
        }
        0 => {
            let mut g = [0.0f32; 2];
            if gcall(shape, SHAPE_GET_SPHERE, mptr(&mut g), 0, 0)?.0 as u8 == 0 {
                return None;
            }
            Some((1, [g[1]; 3], local))
        }
        2 => {
            let mut g = [0.0f32; 3];
            if gcall(shape, SHAPE_GET_CAPSULE, mptr(&mut g), 0, 0)?.0 as u8 == 0 {
                return None;
            }
            Some((2, [g[2] + g[1], g[1], g[1]], local))
        }
        4 => {
            let mut g = Geometry64([0; 64]);
            if gcall(shape, SHAPE_GET_CONVEX, mptr(&mut g), 0, 0)?.0 as u8 == 0 {
                return None;
            }
            let mesh = usize::from_le_bytes(g.0[32..40].try_into().ok()?);
            let f = |o: usize| f32::from_le_bytes([g.0[o], g.0[o + 1], g.0[o + 2], g.0[o + 3]]);
            let scale = [f(4), f(8), f(12)];
            if mesh == 0 {
                return None;
            }
            let mut b = [0.0f32; 6];
            gcall(mesh as P, CONVEX_LOCAL_BOUNDS, mptr(&mut b), 0, 0)?;
            let (mut half, mut centre) = ([0.0f32; 3], [0.0f32; 3]);
            for k in 0..3 {
                half[k] = 0.5 * (b[3 + k] - b[k]) * scale[k].abs();
                centre[k] = 0.5 * (b[3 + k] + b[k]) * scale[k];
            }
            let moved = qrot(&local.q, &centre);
            for k in 0..3 {
                local.p[k] += moved[k];
            }
            if let Some(h) = hull {
                h.mesh = mesh;
                h.scale = scale;
                h.turn = [f(16), f(20), f(24), f(28)];
                h.centre = centre;
            }
            Some((0, half, local))
        }
        _ => None,
    }
}
/// The points of a convex mesh, scaled into its shape (PxMeshScale: turned into the scale's frame, scaled, turned back)
/// and taken from the middle of its bounds. Their count, 0 when there are none to read or they make no sense.
unsafe fn hull_points(h: &Hull, out: &mut [f32]) -> usize {
    let mut run = || -> Option<usize> {
        let n = gcall(h.mesh as P, CONVEX_NB_VERTICES, 0, 0, 0)?.0 as u32 as usize;
        if !(4..=HULL_MOST).contains(&n) {
            return Some(0);
        }
        let v = gcall(h.mesh as P, CONVEX_VERTICES, 0, 0, 0)?.0 as usize;
        if v == 0 {
            return Some(0);
        }
        let mut raw = vec![0u8; n * 12];
        if !read_bytes(v, &mut raw) {
            return Some(0);
        }
        let inv = [-h.turn[0], -h.turn[1], -h.turn[2], h.turn[3]];
        for i in 0..n {
            let f = |k: usize| f32::from_le_bytes(raw[12 * i + 4 * k..12 * i + 4 * k + 4].try_into().unwrap_or([0; 4]));
            let mut a = qrot(&h.turn, &[f(0), f(1), f(2)]);
            for k in 0..3 {
                a[k] *= h.scale[k];
            }
            let b = qrot(&inv, &a);
            for k in 0..3 {
                out[3 * i + k] = b[k] - h.centre[k];
                if !out[3 * i + k].is_finite() || out[3 * i + k].abs() > 1.0 {
                    return Some(0);
                }
            }
        }
        Some(n)
    };
    run().unwrap_or(0)
}
/// The actor's shapes as one primitive for the grasp (grasp.rs: 0 box, 1 sphere, 2 cylinder along its Z): (shape, its
/// half sizes, its pose in the actor's frame (x y z, x y z w), the hull's point count). One shape keeps its own frame;
/// more are one box round them all. A box whose section is round and which is longer than that is a cylinder: a can's
/// hull, a bottle's. A cylinder that is one convex shape also gives its hull's points (`points`, HULL_MOST of them at
/// most) in the cylinder's frame: the part a hand closes on is read from them (grasp_part_of: a bottle's neck or its
/// body).
unsafe fn grasp_shape(actor: P, points: Option<&mut [f32]>) -> Option<(i32, [f32; 3], [f32; 7], usize)> {
    let mut shapes = [0usize; 8];
    let mut n = gcall(actor, GET_SHAPES, shapes.as_mut_ptr() as u64, 8, 0)?.0 as u32 as usize;
    if n == 0 {
        return None;
    }
    n = n.min(8);
    let mut hull = Hull::NONE;
    let (kind, h, pose) = if n == 1 {
        shape_box(shapes[0] as P, Some(&mut hull))?
    } else {
        let (mut lo, mut hi) = ([1e9f32; 3], [-1e9f32; 3]);
        for &s in &shapes[..n] {
            let (_, sh, sp) = shape_box(s as P, None)?;
            for c in 0..8 {
                let corner = [
                    if c & 1 != 0 { sh[0] } else { -sh[0] },
                    if c & 2 != 0 { sh[1] } else { -sh[1] },
                    if c & 4 != 0 { sh[2] } else { -sh[2] },
                ];
                let at = qrot(&sp.q, &corner);
                for a in 0..3 {
                    lo[a] = lo[a].min(sp.p[a] + at[a]);
                    hi[a] = hi[a].max(sp.p[a] + at[a]);
                }
            }
        }
        let mut pose = PxTransform::ZERO;
        let mut h = [0.0f32; 3];
        for a in 0..3 {
            h[a] = 0.5 * (hi[a] - lo[a]);
            pose.p[a] = 0.5 * (hi[a] + lo[a]);
        }
        pose.q = [0.0, 0.0, 0.0, 1.0];
        (0, h, pose)
    };
    if !(h[0].max(h[1].max(h[2])) <= GRASP_LARGEST) || !(h[0].min(h[1].min(h[2])) > 0.0) {
        return None;
    }
    // the long axis of a capsule, or of a box with a round section; a cylinder's axis is its Z
    let mut axis = -1i32;
    if kind == 2 {
        axis = 0;
    } else if kind == 0 {
        for a in 0..3 {
            if axis >= 0 {
                break;
            }
            let (u, v) = (h[(a + 1) % 3], h[(a + 2) % 3]);
            if (u - v).abs() <= ROUND * u.max(v) && h[a] >= LONG * u.max(v) {
                axis = a as i32;
            }
        }
    }
    let r = 0.707_106_78f32;
    let mut turn = [0.0, 0.0, 0.0, 1.0];
    if axis == 0 {
        // its Z onto the actor's X: a quarter turn about Y
        turn[1] = r;
        turn[3] = r;
    } else if axis == 1 {
        // its Z onto Y: a quarter turn back about X
        turn[0] = -r;
        turn[3] = r;
    }
    let (shape, half) = if axis >= 0 {
        let a = axis as usize;
        (2, [h[(a + 1) % 3], h[(a + 2) % 3], h[a]])
    } else {
        (if kind == 1 { 1 } else { 0 }, h)
    };
    let q = qmul(&pose.q, &turn);
    let local = [pose.p[0], pose.p[1], pose.p[2], q[0], q[1], q[2], q[3]];
    // the hull in the cylinder's frame: the box's frame turned back by `turn`
    let mut count = 0;
    if axis >= 0 && hull.mesh != 0 {
        if let Some(points) = points {
            let m = hull_points(&hull, points);
            let back = [-turn[0], -turn[1], -turn[2], turn[3]];
            for i in 0..m {
                let v = qrot(&back, &v3(&points[3 * i..]));
                points[3 * i..3 * i + 3].copy_from_slice(&v);
            }
            count = m;
        }
    }
    Some((shape, half, local, count))
}

/// THE DRAWN SHAPE OF A ROUND PROP (cvr_physhands::mesh_profile): its profile by the mesh's depot path's hash. The mesh
/// is the instance's CMesh (a RED object among those it points at: its weak self handle at +8, its table in the game's
/// image, its class a CMesh; CResource's path at +0x30). Its frame is the body's: the instance's record is the body's
/// pose (a bottle's capsule lies along the mesh's Z, from its base at 0 to its mouth).
fn mesh_of(proxy: u32) -> u64 {
    let Some(instance) = instance_of(proxy) else { return 0 };
    let b = base();
    let mesh_class = CName::hashed("CMesh");
    // the words are not all pointers: each is peeked (a fault there would be the engine's crash, guarded or not)
    for off in (8..0x200).step_by(8) {
        let Some(v) = peek::<usize>(instance + off).filter(|v| *v >= 0x10000 && *v % 8 == 0) else { continue };
        if peek::<usize>(v + 8) != Some(v) {
            continue;
        }
        let Some(table) = peek::<usize>(v) else { continue };
        if table < b || table > b + 0x800_0000 {
            continue;
        }
        // SAFETY: a RED object: its self handle is itself and its table is in the game's image.
        let is_mesh = unsafe { red4ext_abi::rtti::object_class(v as *const c_void) }.is_some_and(|c| c.is_a(mesh_class));
        if !is_mesh {
            continue;
        }
        return peek::<u64>(v + 0x30).unwrap_or(0);
    }
    0
}
/// s
const FINGERS_IN: f32 = 0.12;
const FINGERS_OUT: f32 = 0.18;
fn ease_fingers(st: &mut State) {
    let now = tick_ms();
    for h in 0..2 {
        let f = &mut st.fingers[h];
        if !f.used {
            continue;
        }
        let dt = if f.at != 0 { ((now - f.at) as f32 * 0.001).min(0.1) } else { 0.0 };
        f.at = now;
        f.blend = if f.on { (f.blend + dt / FINGERS_IN).min(1.0) } else { (f.blend - dt / FINGERS_OUT).max(0.0) };
        if !f.on && f.blend <= 0.0 {
            grasp::grasp_let_go(h);
            f.used = false;
            f.at = 0;
            continue;
        }
        grasp::grasp_ease(h, f.blend);
    }
}
/// THE BAKED GRASPS (tools/grip_generator/pose_batch.cpp): per cooked prop mesh, grasps fitted on its drawn surface
/// against the FPP hand's skin. Read on a thread of its own at load; the first grab waits for it only if it comes first.
fn baked_grasps_path() -> &'static std::path::Path {
    static PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
    PATH.get_or_init(|| cvr_host::paths::file(cvr_host::paths::Folder::Grips, "CyberpunkVR_MeshGrasps.bin"))
}
fn baked_grasps() -> i32 {
    grasp::grasp_load_baked(baked_grasps_path())
}
fn side_of(bone: i32) -> i32 {
    // SAFETY: the rig's bone indices, set once at the rig's start.
    unsafe {
        if bone == g_VRRightBoneIdx {
            1
        } else if bone == g_VRLeftBoneIdx {
            0
        } else {
            -1
        }
    }
}
const KINDS: [&str; 8] = ["flat", "wrap", "ball", "pinch", "edge", "from above", "from below", "three fingers"];
/// Seated for its grasp, or not: the slot's hand and its offset and turn (the caller holds the lock).
unsafe fn seat_for(st: &mut State, i: usize, dt: f32) {
    let s = &mut st.slots[i];
    let side = side_of(s.bone);
    if s.grasp_wanted && !s.seated && !s.grasp_tried && side >= 0 && s.actor != 0 {
        s.grasp_tried = true;
        let mut hull = vec![0.0f32; 3 * HULL_MOST.max(PROFILE_POINTS)];
        let Some((shape, mut half, mut local, mut hull_n)) = grasp_shape(s.actor as P, Some(&mut hull)) else {
            log!("[heldbody] proxy {} body {}: no shape for the grasp (or too big for one hand)\n", s.proxy, s.body);
            return;
        };
        // the thing in the hand's frame: where the actor is there, then the shape in the actor
        let moved = qrot(&s.turn, &v3(&local));
        let mut p = [s.offset[0] + moved[0], s.offset[1] + moved[1], s.offset[2] + moved[2]];
        let q = qmul(&s.turn, &q4(&local[3..]));
        // a bottle: its neck or its body, whichever the palm is at (the palm in the cylinder's frame), a cylinder of its
        // own, and the whole of it round that for the fingers -- from its convex hull, or from its drawn mesh's profile
        let mut ends = 15;
        let mut part = false;
        let mut gp = grasp::GraspPart::default();
        let mut grip_at = [0.0f32; 3];
        let (mut mz, mut hl, mut widest) = (0.0f32, half[2], 0.0f32);
        let mesh = mesh_of(s.proxy);
        if shape == 2 {
            // A convex collider fills a glass stem's waist. Prefer a validated visible-mesh profile even when the collider
            // supplies hull points.
            if let Some(mp) = profile_of(mesh) {
                let rings = profile_points(mp, &local, &half, &mut hull);
                if rings.count >= 6 {
                    hull_n = rings.count;
                    mz = rings.mz;
                    hl = rings.half_length;
                    widest = rings.widest;
                }
            }
        }
        if shape == 2 && hull_n >= 6 {
            if let Some(skin) = grasp::grasp_palm(side as usize) {
                let rel = [skin[0] - p[0], skin[1] - p[1], skin[2] - p[2]];
                let inv = [-q[0], -q[1], -q[2], q[3]];
                let mut hand = qrot(&inv, &rel);
                hand[2] -= mz;
                grip_at = hand;
                let points = &hull[..3 * hull_n];
                let mut found = grasp::grasp_part_of(points, hl, &hand, &mut gp);
                // A GLASS ON A STEM is held by its stem from below, wherever the hand closed on it (its bowl too): the
                // stem found along its axis, the palm put beside it
                if !(found && gp.stem) {
                    let out = half[0].max(half[1]).max(widest) + 0.025;
                    for k in 0..24 {
                        let probe = [out, 0.0, -hl + (k as f32 + 0.5) * (2.0 * hl / 24.0)];
                        let mut stem = grasp::GraspPart::default();
                        if grasp::grasp_part_of(points, hl, &probe, &mut stem) && stem.stem {
                            gp = stem;
                            found = true;
                            break;
                        }
                    }
                }
                let mut dz = [0.0, 0.0, mz];
                if found {
                    dz[2] += gp.z;
                    half = [gp.radius, gp.radius, gp.half];
                    ends = gp.ends;
                    part = true;
                } else if gp.lathe.knots >= 2 && widest > 0.0 {
                    // the whole, as thick as it is drawn
                    half = [widest, widest, hl];
                }
                if found || (gp.lathe.knots >= 2 && widest > 0.0) {
                    let m = qrot(&q4(&local[3..]), &dz);
                    for k in 0..3 {
                        local[k] += m[k];
                    }
                    let m = qrot(&q, &dz);
                    for k in 0..3 {
                        p[k] += m[k];
                    }
                }
            }
        }
        let lathe = gp.lathe.knots >= 2 && (part || widest > 0.0);
        let Some((seat_p, seat_q, mut kind)) = grasp::grasp_take(side as usize, shape, &half, &p, &q, ends, part, lathe.then_some(&gp.lathe), mesh) else {
            // the palm is not measured yet: again at the next step
            s.grasp_tried = false;
            return;
        };
        // back to the actor: the seat, the shape's own pose in the actor taken off
        let inv = [-local[3], -local[4], -local[5], local[6]];
        let aq = qmul(&seat_q, &inv);
        let back = qrot(&aq, &v3(&local));
        for k in 0..3 {
            s.seat[k] = seat_p[k] - back[k];
        }
        s.seat[3..7].copy_from_slice(&aq);
        // a prop the fitter baked: the baked grasp nearest this seat instead -- the mesh's pose against the palm and the
        // fingers fitted on its drawn surface (the body's frame is its mesh's)
        let mut baked = false;
        if mesh != 0 && baked_grasps() > 0 && grasp::grasp_baked_has(mesh) {
            match grasp::grasp_take_baked(side as usize, mesh, &v3(&s.seat), &q4(&s.seat[3..])) {
                Some(b) => {
                    s.seat[..3].copy_from_slice(&b.p);
                    s.seat[3..7].copy_from_slice(&b.q);
                    kind = b.kind;
                    baked = true;
                    log!(
                        "[heldbody] proxy {} body {}: mesh {:016x} baked grasp (kind {}), {:.1} deg and {:.1} mm from the game's seat\n",
                        s.proxy,
                        s.body,
                        mesh,
                        b.kind,
                        b.deg,
                        b.dist * 1000.0
                    );
                }
                None => log!("[heldbody] proxy {} body {}: mesh {:016x} baked, none near the game's seat: the solved grasp\n", s.proxy, s.body, mesh),
            }
        }
        s.seat_from[..3].copy_from_slice(&s.offset);
        s.seat_from[3..7].copy_from_slice(&s.turn);
        s.seat_t = 0.0;
        s.seated = true;
        s.grasp_side = side;
        s.grasp_kind = kind;
        let how = grasp::debug_grasp_approach(side as usize);
        let d = [s.seat[0] - s.offset[0], s.seat[1] - s.offset[1], s.seat[2] - s.offset[2]];
        log!(
            "[heldbody] proxy {} body {}: seated in the {} hand for a {}{} grasp (shape {}{}, half {:.3} {:.3} {:.3}, hull {} points, mesh {:016x}{}), {:.3} m from where it was taken; palm to its axis {:.2}, along it {:.2} of its half, that end to up {:.2}; input palm in shape ({:.3} {:.3} {:.3}), body in hand ({:.3} {:.3} {:.3})\n",
            s.proxy,
            s.body,
            if side != 0 { "right" } else { "left" },
            if baked { "baked " } else { "" },
            if (0..8).contains(&kind) { KINDS[kind as usize] } else { "?" },
            shape,
            if part {
                if gp.stem {
                    " its stem"
                } else if gp.neck {
                    " its neck"
                } else {
                    " its body"
                }
            } else {
                ""
            },
            half[0],
            half[1],
            half[2],
            hull_n,
            mesh,
            if lathe { ", the whole drawn round it" } else { "" },
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(),
            how[0],
            how[1],
            how[2],
            grip_at[0],
            grip_at[1],
            grip_at[2],
            s.offset[0],
            s.offset[1],
            s.offset[2]
        );
        st.fingers[side as usize].on = true;
        st.fingers[side as usize].used = true;
    }
    let s = &mut st.slots[i];
    if !s.grasp_wanted {
        unseat(&mut st.fingers, &mut st.slots[i]);
        return;
    }
    if !s.seated {
        return;
    }
    s.seat_t = (s.seat_t + dt / SEAT_SEC).min(1.0);
    let w = s.seat_t * s.seat_t * (3.0 - 2.0 * s.seat_t);
    for k in 0..3 {
        s.offset[k] = s.seat_from[k] + (s.seat[k] - s.seat_from[k]) * w;
    }
    let a = q4(&s.seat_from[3..]);
    let b = q4(&s.seat[3..]);
    let sign = if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 { -1.0 } else { 1.0 };
    let mut n = 0.0;
    for k in 0..4 {
        s.turn[k] = a[k] + (sign * b[k] - a[k]) * w;
        n += s.turn[k] * s.turn[k];
    }
    let n = n.sqrt();
    if n > 1e-4 {
        for v in s.turn.iter_mut() {
            *v /= n;
        }
    }
}
fn unseat(fingers: &mut [Fingers; 2], s: &mut Slot) {
    if s.seated && s.grasp_side >= 0 {
        fingers[s.grasp_side as usize].on = false;
    }
    s.seated = false;
    s.grasp_tried = false;
}
fn record_fixed(record: &[u8], k: usize) -> i32 {
    i32::from_le_bytes(record[4 * k..4 * k + 4].try_into().unwrap_or([0; 4]))
}
fn record_quat(record: &[u8]) -> [f32; 4] {
    let f = |k: usize| f32::from_le_bytes(record[0x10 + 4 * k..0x14 + 4 * k].try_into().unwrap_or([0; 4]));
    [f(0), f(1), f(2), f(3)]
}
fn set_record(record: &mut [u8], fixed: Option<[i32; 3]>, quat: &[f32; 4]) {
    if let Some(fixed) = fixed {
        for k in 0..3 {
            record[4 * k..4 * k + 4].copy_from_slice(&fixed[k].to_le_bytes());
        }
    }
    for k in 0..4 {
        record[0x10 + 4 * k..0x14 + 4 * k].copy_from_slice(&quat[k].to_le_bytes());
    }
}
fn lround(x: f64) -> i32 {
    x.round() as i32
}
/// The record of this instance, blended from where its body is to the hand. False if it is not to be touched.
fn picture(s: &mut Slot, record: &mut [u8]) -> bool {
    if !s.posed || s.weight <= 0.0 {
        return false;
    }
    let quat = record_quat(record);
    if !unit_like(&quat) {
        return false;
    }
    let body = [0, 1, 2].map(|k| (record_fixed(record, k) as f64 / FIXED) as f32);
    let gap = [s.hand.p[0] - body[0], s.hand.p[1] - body[1], s.hand.p[2] - body[2]];
    if gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2] > FAR_FROM_BODY * FAR_FROM_BODY {
        s.too_far += 1;
        return false;
    }
    if !s.asked {
        // No longer on the hand: the body flies, and the picture with it. Holding it at the hand's last place would stop
        // it dead for the length of the blend.
        let conj = [-quat[0], -quat[1], -quat[2], quat[3]];
        if !s.rel_set {
            s.rel_p = gap;
            s.rel_q = qmul(&s.hand.q, &conj);
            if s.rel_q[3] < 0.0 {
                for v in s.rel_q.iter_mut() {
                    *v = -*v;
                }
            }
            s.rel_set = true;
        }
        let fixed = [0, 1, 2].map(|k| lround(((body[k] + s.rel_p[k] * s.weight) as f64) * FIXED));
        let mut part = [s.rel_q[0] * s.weight, s.rel_q[1] * s.weight, s.rel_q[2] * s.weight, 1.0 + (s.rel_q[3] - 1.0) * s.weight];
        let size = (part[0] * part[0] + part[1] * part[1] + part[2] * part[2] + part[3] * part[3]).sqrt();
        if size < 1e-4 {
            // the position is written before this check (main's order)
            set_record(record, Some(fixed), &quat);
            return false;
        }
        for v in part.iter_mut() {
            *v /= size;
        }
        set_record(record, Some(fixed), &qmul(&part, &quat));
        return true;
    }
    // Short of a wall: no further from the body than the wall leaves room for.
    let size = (gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2]).sqrt();
    let keep = if size > s.reach && size > 1e-5 { s.reach / size } else { 1.0 };
    let fixed = [0, 1, 2].map(|k| lround(((body[k] + gap[k] * keep * s.weight) as f64) * FIXED));
    let sign = if quat[0] * s.hand.q[0] + quat[1] * s.hand.q[1] + quat[2] * s.hand.q[2] + quat[3] * s.hand.q[3] < 0.0 { -1.0 } else { 1.0 };
    let mut out = [0.0f32; 4];
    let mut n = 0.0;
    for k in 0..4 {
        out[k] = quat[k] + (s.hand.q[k] * sign - quat[k]) * s.weight;
        n += out[k] * out[k];
    }
    let n = n.sqrt();
    if n < 1e-4 {
        set_record(record, Some(fixed), &quat);
        return false;
    }
    set_record(record, Some(fixed), &out.map(|v| v / n));
    true
}
/// Where the hand has the thing now. The player as the engine has it this frame, the bone as animated for it.
unsafe fn hand_pose(s: &Slot) -> Option<Pose> {
    if CyberpunkVR_PlayerEntityValid == 0 || s.bone < 0 {
        return None;
    }
    let side = side_of(s.bone);
    if side >= 0 {
        let (wrist, rotation) = super::pose::solved_world_pose(side as usize)?;
        let offset = qrot(&rotation, &s.offset);
        let out = Pose { p: [wrist[0] + offset[0], wrist[1] + offset[1], wrist[2] + offset[2]], q: qmul(&rotation, &s.turn) };
        return (unit_like(&out.q) && (out.p[0] + out.p[1] + out.p[2]).is_finite()).then_some(out);
    }
    // Legacy callers can attach a picture to a non-hand bone. Normal physical grips above never mix the diagnostic FK
    // cache with a later entity pose.
    if s.bone >= g_VRFKSnapCount {
        return None;
    }
    let bone = g_VRFKSnapPos[s.bone as usize];
    let rot = g_VRFKSnapRot[s.bone as usize];
    let q = CyberpunkVR_PlayerEntityQuat;
    if !unit_like(&rot) || !unit_like(&q) {
        return None;
    }
    let at = qrot(&q, &bone);
    let world = qmul(&q, &rot);
    let off = qrot(&world, &s.offset);
    let pos = CyberpunkVR_PlayerEntityPos;
    let mut out = Pose { p: [pos[0] + at[0] + off[0], pos[1] + at[1] + off[1], pos[2] + at[2] + off[2]], q: qmul(&world, &s.turn) };
    let n = (out.q[0] * out.q[0] + out.q[1] * out.q[1] + out.q[2] * out.q[2] + out.q[3] * out.q[3]).sqrt();
    if !n.is_finite() || n < 1e-4 {
        return None;
    }
    for v in out.q.iter_mut() {
        *v /= n;
    }
    (out.p[0].is_finite() && out.p[1].is_finite() && out.p[2].is_finite()).then_some(out)
}
fn system() -> usize {
    let b = base();
    if b == 0 {
        return 0;
    }
    read::<usize>(b + SYSTEM_RVA).unwrap_or(0)
}
/// The instance behind a proxy, if the listener registered for it is one of these. A listener in the table is alive;
/// the id it knows says it is still this proxy's.
pub fn instance_of(proxy: u32) -> Option<usize> {
    let system = system();
    if system == 0 {
        return None;
    }
    let listener = read::<usize>(system + LISTENERS + (proxy & 0xFFFF) as usize * 8).filter(|l| *l != 0)?;
    let table = read::<usize>(listener).filter(|t| *t != 0)?;
    if read::<usize>(table + 8)? != base() + LISTENER_ENTRY_RVA {
        return None;
    }
    let instance = listener - LISTENER_AT;
    (read::<u32>(instance + PROXY_ID)? == proxy).then_some(instance)
}
thread_local! {
    static UPDATE_COPY: core::cell::UnsafeCell<[u8; RECORD_SIZE * MOST_INSTANCES as usize]> = const { core::cell::UnsafeCell::new([0; RECORD_SIZE * MOST_INSTANCES as usize]) };
}
unsafe extern "C" fn update(instance: *mut u8, span_arg: *mut c_void, count: u32) {
    let original = ORIGINAL_UPDATE.expect("hooked");
    super::super::frame_order::note(b'R');
    if !ANY_USED.load(Relaxed) || count == 0 || count > MOST_INSTANCES {
        original(instance, span_arg, count);
        return;
    }
    let proxy = *instance.add(PROXY_ID).cast::<u32>();
    UPDATE_COPY.with(|cell| {
        let copy = &mut *cell.get();
        let bytes = count as usize * RECORD_SIZE;
        let mut changed = false;
        {
            let mut st = lock();
            for s in st.slots.iter_mut() {
                if !s.used || s.proxy != proxy || s.body >= count {
                    continue;
                }
                if !changed {
                    core::ptr::copy_nonoverlapping((*span_arg.cast::<Span>()).begin, copy.as_mut_ptr(), bytes);
                }
                let at = s.body as usize * RECORD_SIZE;
                if picture(s, &mut copy[at..at + RECORD_SIZE]) {
                    changed = true;
                }
                s.detours += 1;
                s.detour_thread = GetCurrentThreadId();
                s.last_detour = super::qpc().0 as u64;
            }
        }
        if !changed {
            original(instance, span_arg, count);
            return;
        }
        let mut span = Span { begin: copy.as_mut_ptr(), end: copy.as_mut_ptr().add(bytes) };
        original(instance, (&raw mut span).cast(), count);
    });
}
fn report(s: &Slot) {
    let took = if s.first_tick != 0 { tick_ms() - s.first_tick } else { 0 };
    log!(
        "[heldbody] proxy {} body {} of {} let go ({}): {:.0} frames a second; {} frames, {} sent at the submit, the engine's own {} (thread {}), body behind the hand mean {:.3} most {:.3} m, blocked {} frames, too far {}, submit after engine's own {:.1} ms\n",
        s.proxy,
        s.body,
        s.instances,
        if s.own_body {
            "held by its own body"
        } else if s.measure_only {
            "picture off, measured only"
        } else {
            "pictured"
        },
        if took != 0 { s.frames as f64 * 1000.0 / took as f64 } else { 0.0 },
        s.frames,
        s.sends,
        s.detours,
        s.detour_thread,
        if s.frames != 0 { s.behind_sum / s.frames as f64 } else { 0.0 },
        s.behind_most,
        s.blocked_frames,
        s.too_far,
        if s.detour_gaps != 0 { s.detour_gap_sum / s.detour_gaps as f64 } else { 0.0 }
    );
}
/// The slot of (proxy, body), or a free one made it.
fn slot_for(st: &mut State, proxy: u32, body: u32) -> Option<usize> {
    if let Some(i) = st.slots.iter().position(|s| s.used && s.proxy == proxy && s.body == body) {
        return Some(i);
    }
    let i = st.slots.iter().position(|s| !s.used)?;
    st.slots[i] = Slot::EMPTY;
    st.slots[i].used = true;
    st.slots[i].proxy = proxy;
    st.slots[i].body = body;
    ANY_USED.store(true, SeqCst);
    Some(i)
}

/// Hold: the picture of (proxy, body) on the hand bone `bone` at `offset`, `turn`; `blocked` while a wall stops it,
/// `reach` the room the wall leaves. True when the picture is drawn there (the setting on).
pub fn hold(proxy: u32, body: u32, bone: i32, offset: &[f32; 3], turn: &[f32; 4], blocked: bool, reach: f32) -> bool {
    if !INSTALLED.load(Acquire) || body >= MOST_INSTANCES || instance_of(proxy).is_none() {
        return false;
    }
    let picture = lc!(xrCarryPicture) != 0;
    let now = tick_ms();
    let mut st = lock();
    let Some(i) = slot_for(&mut st, proxy, body) else { return false };
    let s = &mut st.slots[i];
    s.asked = true;
    s.asked_at = now;
    s.bone = bone;
    s.blocked = blocked;
    s.reach = reach;
    s.let_go = false;
    s.rel_set = false;
    s.measure_only = !picture;
    s.offset = *offset;
    s.turn = *turn;
    picture
}
pub fn let_go(proxy: u32, body: u32) {
    let mut st = lock();
    for s in st.slots.iter_mut().filter(|s| s.used && s.proxy == proxy && s.body == body) {
        s.let_go = true;
        s.asked = false;
        s.rel_set = false;
    }
}
static MISS: Mutex<(u32, u32, u64)> = Mutex::new((0, 0, 0));
static SLOW_SAID: AtomicI32 = AtomicI32::new(0);
static MISS_SAID: AtomicI32 = AtomicI32::new(0);
/// HoldBody: the thing held by its own body, steered at each PhysX step to where the hand has it. False: it cannot be,
/// carry it as before.
pub fn hold_body(proxy: u32, body: u32, bone: i32, offset: &[f32; 3], turn: &[f32; 4]) -> bool {
    let (t0, frequency) = super::qpc();
    // A broken thing is not looked for again: each look walks every scene (0.3 to 2 ms), and the script asks every frame.
    if !INSTALLED.load(Acquire) || body >= MOST_INSTANCES || debug_view::body_gone(proxy, body) || instance_of(proxy).is_none() || !super::super::physx_step::ensure() {
        return false;
    }
    let (t1, _) = super::qpc();
    let now = tick_ms();
    let known = lock().slots.iter().any(|s| s.used && s.proxy == proxy && s.body == body && s.own_body && s.actor != 0);
    let mut found = None;
    if !known {
        // One that was not found is not looked for again for a second: the script asks every frame it is carried.
        {
            let miss = MISS.lock().unwrap_or_else(|e| e.into_inner());
            if proxy == miss.0 && body == miss.1 && now - miss.2 < 1000 {
                return false;
            }
        }
        // SAFETY: guarded walks of PhysX's scenes.
        found = unsafe { debug_view::rigid_dynamic_of(proxy, body) };
        // What taking a thing costs the frame it is taken in: the step's hooks (installed at boot, so nothing), then the
        // walk over the scenes' actors for this one. Said when it is felt.
        let (t2, _) = super::qpc();
        let look_ms = (t2 - t1) as f64 * 1000.0 / frequency as f64;
        if look_ms > 0.5 && SLOW_SAID.fetch_add(1, Relaxed) < 16 {
            log!("[heldbody] proxy {} body {}: looked up in {:.2} ms (the step's hooks {:.2} ms)\n", proxy, body, look_ms, (t1 - t0) as f64 * 1000.0 / frequency as f64);
        }
        if found.is_none() {
            *MISS.lock().unwrap_or_else(|e| e.into_inner()) = (proxy, body, now);
            if MISS_SAID.fetch_add(1, Relaxed) < 8 {
                log!("[heldbody] proxy {} body {}: no PxRigidDynamic of a scene with a known origin; it is carried the old way\n", proxy, body);
            }
            return false;
        }
    }
    let mut st = lock();
    let Some(i) = slot_for(&mut st, proxy, body) else { return false };
    let s = &mut st.slots[i];
    if found.is_none() && !(s.own_body && s.actor != 0) {
        return false;
    }
    if let Some((actor, scene, origin)) = found {
        s.own_body = true;
        s.actor = actor as usize;
        s.scene = scene as usize;
        s.driving = false;
        s.origin = origin;
        log!(
            "[heldbody] proxy {} body {}: held by its own body, actor {:#x} of scene {:#x}, origin ({:.2} {:.2} {:.2})\n",
            proxy,
            body,
            actor as usize,
            scene as usize,
            origin[0],
            origin[1],
            origin[2]
        );
        // its body interface refused the moment this actor is released
        debug_view::note_actor(proxy, body, actor as usize);
    }
    s.asked = true;
    s.asked_at = now;
    s.bone = bone;
    s.blocked = false;
    s.reach = 1000.0;
    s.let_go = false;
    s.rel_set = false;
    s.measure_only = true;
    if !s.seated {
        s.offset = *offset;
        s.turn = *turn;
    }
    true
}
pub fn set_held_filter(word0: u32, word2: u32) {
    HELD_FILTER_OR[0].store(word0, Relaxed);
    HELD_FILTER_OR[1].store(word2, Relaxed);
}
/// HoldingMask: the hands (bit 0 left, bit 1 right) that steer a body held by its own body.
pub fn holding_mask() -> i32 {
    let st = lock();
    st.slots
        .iter()
        .filter(|s| s.used && s.own_body && s.actor != 0 && s.driving && !s.let_go)
        .map(|s| match side_of(s.bone) {
            1 => 2,
            0 => 1,
            _ => 0,
        })
        .fold(0, |m, b| m | b)
}
/// How many shapes an actor has, the first one's geometry type, and its convex mesh's point count (-1 when not read).
unsafe fn shapes_of(actor: P) -> (u32, i32, i32) {
    let (mut n, mut kind, mut points) = (0u32, -1i32, -1i32);
    let _ = (|| -> Option<()> {
        let mut shapes = [0usize; 8];
        n = gcall(actor, GET_SHAPES, shapes.as_mut_ptr() as u64, 8, 0)?.0 as u32;
        if n < 1 {
            return None;
        }
        kind = gcall(shapes[0] as P, SHAPE_GEOMETRY_TYPE, 0, 0, 0)?.0 as i32;
        if kind != 4 {
            return None;
        }
        let mut g = Geometry64([0; 64]);
        if gcall(shapes[0] as P, SHAPE_GET_CONVEX, mptr(&mut g), 0, 0)?.0 as u8 == 0 {
            return None;
        }
        let mesh = usize::from_le_bytes(g.0[32..40].try_into().ok()?);
        if mesh == 0 {
            return None;
        }
        points = gcall(mesh as P, CONVEX_NB_VERTICES, 0, 0, 0)?.0 as u32 as i32;
        Some(())
    })();
    (n, kind, points)
}
/// What the grasp makes of a body, for VRBodyGraspShape (a test tool): its primitive and pose in the body, its hull or
/// its drawn mesh's profile, and the part a palm at either end and at its middle would close on.
pub fn grasp_shape_text(proxy: u32, body: u32) -> String {
    let Some(actor) = debug_view::actor_of(proxy, body) else { return String::new() };
    let mut hull = vec![0.0f32; 3 * HULL_MOST.max(PROFILE_POINTS)];
    // SAFETY: guarded PhysX calls on an actor found in PhysX's own lists.
    let Some((shape, half, local, mut hull_n)) = (unsafe { grasp_shape(actor, Some(&mut hull)) }) else { return "no grasp shape".into() };
    // SAFETY: as above.
    let (n_shapes, kind, points) = unsafe { shapes_of(actor) };
    let mesh = mesh_of(proxy);
    let mp = profile_of(mesh);
    let (mut mz, mut hl, mut widest) = (0.0f32, half[2], 0.0f32);
    if shape == 2 && hull_n < 6 {
        if let Some(mp) = mp {
            let rings = profile_points(mp, &local, &half, &mut hull);
            hull_n = rings.count;
            mz = rings.mz;
            hl = rings.half_length;
            widest = rings.widest;
        }
    }
    let mut text = format!(
        "shape {} half {:.3} {:.3} {:.3} hull {} (shapes {}, type {}, mesh points {}) at {:.3} {:.3} {:.3} turn {:.3} {:.3} {:.3} {:.3}; mesh {:016x} {}, its middle {:.3} along, half {:.3}, widest {:.3}",
        shape,
        half[0],
        half[1],
        half[2],
        hull_n,
        n_shapes,
        kind,
        points,
        local[0],
        local[1],
        local[2],
        local[3],
        local[4],
        local[5],
        local[6],
        mesh,
        if mp.is_some() { "profiled" } else { "no profile" },
        mz,
        hl,
        widest
    );
    if shape != 2 || hull_n < 6 {
        return text;
    }
    const WHERE: [&str; 3] = ["-Z end", "middle", "+Z end"];
    for (w, label) in WHERE.iter().enumerate() {
        let hand = [half[0] + 0.02, 0.0, (w as f32 - 1.0) * hl * 0.9];
        let mut gp = grasp::GraspPart::default();
        if grasp::grasp_part_of(&hull[..3 * hull_n], hl, &hand, &mut gp) {
            text += &format!("; {}: {} z {:.3} half {:.3} r {:.3} ends {}", label, if gp.neck { "neck" } else { "body" }, gp.z, gp.half, gp.radius, gp.ends);
        } else {
            text += &format!("; {}: whole", label);
        }
    }
    text.truncate(511);
    text
}
pub fn grasp_body(proxy: u32, body: u32, on: bool) -> bool {
    let mut st = lock();
    let mut known = false;
    for s in st.slots.iter_mut().filter(|s| s.used && s.proxy == proxy && s.body == body && s.own_body) {
        s.grasp_wanted = on;
        known = true;
    }
    known
}
pub fn let_go_body(proxy: u32, body: u32, velocity: &[f32; 3], spin: &[f32; 3], thrown: bool) {
    let mut st = lock();
    for s in st.slots.iter_mut().filter(|s| s.used && s.proxy == proxy && s.body == body) {
        s.throw_given = if thrown { 1.0 } else { 0.0 };
        s.throw_v = *velocity;
        s.throw_w = *spin;
        s.let_go = true;
        s.asked = false;
    }
}

/// A HELD ACTOR CAN DIE IN THE HAND. A bowl knocked on the table breaks, and the game releases its actor; PhysX puts the
/// memory back in its pool, whose free list lives in the actor's first word, where the table was. Steering it on through
/// the pointer kept here called a function out of that free list (2026-11-08 21:08), and a guarded call does not save
/// it: the engine's vectored handler takes the fault first. So before anything touches it, each step: the table is still
/// PxRigidDynamic's and the userData still this body's (proxy, body), the words find_actor_in matched it by. A pool keeps
/// its slabs, so reading them is safe. Not asked of VirtualQuery: that cost 7 to 12 ms a call on this heap.
const RIGID_DYNAMIC_TABLE_RVA: usize = 0x20B2E8;
fn actor_still(actor: usize, proxy: u32, body: u32) -> bool {
    static TABLE: OnceLock<usize> = OnceLock::new();
    // SAFETY: a module handle by name.
    let table = *TABLE.get_or_init(|| unsafe { GetModuleHandleA(c"PhysX3_x64.dll".as_ptr().cast()) } as usize + RIGID_DYNAMIC_TABLE_RVA);
    if actor == 0 || table == RIGID_DYNAMIC_TABLE_RVA {
        return false;
    }
    read::<usize>(actor) == Some(table) && read::<u32>(actor + 0x10) == Some(proxy) && read::<u16>(actor + 0x14) == Some(body as u16)
}
fn alive(s: &Slot) -> bool {
    actor_still(s.actor, s.proxy, s.body)
}
/// Gone: nothing of it is touched again, not even to put back what holding it changed (the actor that had it is no
/// more).
fn forget(fingers: &mut [Fingers; 2], s: &mut Slot, why: &str) {
    log!("[heldbody] proxy {} body {}: its actor {:#x} is {}; let go of it without touching it\n", s.proxy, s.body, s.actor, why);
    // and its body interface is refused from now on (hand_body.rs)
    debug_view::note_gone(s.proxy, s.body);
    unseat(fingers, s);
    s.grasp_wanted = false;
    s.actor = 0;
    s.driving = false;
    s.shape_count = 0;
    s.targets = 0;
    s.let_go = true;
    s.asked = false;
}

// QUERIES OFF, SET AT THE STEP. A carried thing is hidden from the player's movement and from rays (queries off) while
// it is carried, and given them back a while after it is let go. Through the game's SetIsQueryable that was queued for
// the proxy from the script thread and done whenever the game got to it: a stack of cups thrown out of the hand broke as
// it landed, the queries given back in that same moment put the freed actor's shape back into the scene's query tree,
// and the next ray through that place called through the freed shape (2026-11-10 16:27 and 21:39). No check on the
// script thread closes that: the game frees the actor between the check and the call. So a body with a PhysX actor of
// its own is hidden here, just before PxScene::simulate, when nothing else may touch the scene and a release of this
// frame has already happened (the actor's first word then the pool's free link, not the table): only on an actor that
// is still this body's. A gone one is let be, and its body interface refused.
const QUERY_WANTS: usize = 16;
const QUERY_SHAPES: usize = 8;
const SHAPE_SET_QUERY_FILTER: usize = 22;
const SHAPE_GET_QUERY_FILTER: usize = 23;
#[derive(Clone, Copy)]
struct QueryWant {
    used: bool,
    want_on: bool,
    off: bool,
    proxy: u32,
    body: u32,
    actor: usize,
    scene: usize,
    shape_count: u32,
    /// each shape's own query filter data, kept while it is hidden
    own: [[u32; 4]; QUERY_SHAPES],
}
impl QueryWant {
    const NONE: QueryWant = QueryWant { used: false, want_on: false, off: false, proxy: 0, body: 0, actor: 0, scene: 0, shape_count: 0, own: [[0; 4]; QUERY_SHAPES] };
}
static QUERY_LOCK: Mutex<[QueryWant; QUERY_WANTS]> = Mutex::new([QueryWant::NONE; QUERY_WANTS]);
static ANY_QUERY: AtomicBool = AtomicBool::new(false);
/// Hidden from the game's queries: a shape is hit only where words 2 and 3 of its query filter data meet the query's
/// (the game's pre-filter, exe+0x21BD88), so those are cleared, each shape's own kept in `w`; or put back from it. Not
/// PxShapeFlag::eSCENE_QUERY_SHAPE: PhysX 3.4 keeps the shape in its query tree whatever that flag says, and rays went on
/// hitting it. A plain write of the shape's data: nothing is put into or taken out of the tree. False if not read.
unsafe fn set_queries(w: &mut QueryWant, on: bool) -> bool {
    let mut run = || -> Option<()> {
        let mut shapes = [0usize; QUERY_SHAPES];
        let n = gcall(w.actor as P, GET_SHAPES, shapes.as_mut_ptr() as u64, QUERY_SHAPES as u64, 0)?.0 as u32;
        if !on {
            w.shape_count = n.min(QUERY_SHAPES as u32);
        }
        for i in 0..(w.shape_count.min(n) as usize) {
            let shape = shapes[i] as P;
            if !on {
                gcall(shape, SHAPE_GET_QUERY_FILTER, mptr(&mut w.own[i]), 0, 0)?;
                let hidden = [w.own[i][0], w.own[i][1], 0, 0];
                gcall(shape, SHAPE_SET_QUERY_FILTER, ptr(&hidden), 0, 0)?;
            } else {
                gcall(shape, SHAPE_SET_QUERY_FILTER, ptr(&w.own[i]), 0, 0)?;
            }
        }
        Some(())
    };
    run().is_some()
}
unsafe fn queries_at_step(scene: P) {
    if !ANY_QUERY.load(Acquire) {
        return;
    }
    let mut wants = QUERY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut any = false;
    for w in wants.iter_mut() {
        if !w.used {
            continue;
        }
        if w.scene != scene as usize {
            any = true;
            continue;
        }
        if !actor_still(w.actor, w.proxy, w.body) {
            log!("[heldbody] proxy {} body {}: its actor {:#x} is gone; its queries let be\n", w.proxy, w.body, w.actor);
            debug_view::note_gone(w.proxy, w.body);
            w.used = false;
            continue;
        }
        if w.want_on {
            if w.off {
                set_queries(w, true);
            }
            w.used = false;
            continue;
        }
        if !w.off {
            if set_queries(w, false) {
                w.off = true;
            } else {
                w.used = false;
                continue;
            }
        }
        any = true;
    }
    ANY_QUERY.store(any, Release);
}
static QUERY_FULL_SAID: AtomicBool = AtomicBool::new(false);
/// QueryWanted: the body (proxy, body) hidden from the game's queries (on false) or given them back (on true), at the
/// next step. 1 when this handles it, 0 when it is the game's.
pub fn query_wanted(proxy: u32, body: u32, on: bool) -> i32 {
    if proxy == 0 {
        return 0;
    }
    {
        let mut wants = QUERY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = wants.iter_mut().find(|w| w.used && w.proxy == proxy && w.body == body) {
            w.want_on = on;
            ANY_QUERY.store(true, Release);
            return 1;
        }
    }
    // queries back on for a body never set off here: the game's, as before (a gone one is refused by its interface)
    if on || !super::super::physx_step::ensure() {
        return 0;
    }
    // SAFETY: guarded walks of PhysX's scenes.
    let Some((actor, scene, _)) = (unsafe { debug_view::rigid_dynamic_of(proxy, body) }) else { return 0 };
    if scene.is_null() {
        return 0;
    }
    let placed = {
        let mut wants = QUERY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match wants.iter_mut().find(|w| !w.used) {
            Some(free) => {
                *free = QueryWant { used: true, proxy, body, actor: actor as usize, scene: scene as usize, ..QueryWant::NONE };
                ANY_QUERY.store(true, Release);
                true
            }
            None => false,
        }
    };
    if !placed {
        if !QUERY_FULL_SAID.swap(true, Relaxed) {
            log!("[heldbody] queries: all {} places taken; proxy {} body {} left to the game\n", QUERY_WANTS, proxy, body);
        }
        return 0;
    }
    // its body interface refused the moment this actor is released
    debug_view::note_actor(proxy, body, actor as usize);
    1
}

/// BeforeSimulate: PhysXStep, just before PxScene::simulate (inside the physics pose frame).
pub unsafe fn before_simulate(scene: P, dt: f32) {
    queries_at_step(scene);
    if !INSTALLED.load(Acquire) || !ANY_USED.load(Relaxed) {
        return;
    }
    let tick = tick_ms();
    let current_origin = super::scene_origin(scene);
    let mut guard = lock();
    let st = &mut *guard;
    for i in 0..2 {
        {
            let s = &mut st.slots[i];
            if !s.used || !s.own_body || s.actor == 0 || s.scene != scene as usize {
                continue;
            }
            if instance_of(s.proxy).is_none() {
                // gone with its sector, and its actor with it
                s.actor = 0;
                s.driving = false;
                continue;
            }
        }
        if !alive(&st.slots[i]) {
            forget(&mut st.fingers, &mut st.slots[i], "released (broken?)");
            continue;
        }
        if let Some(o) = current_origin {
            st.slots[i].origin = o;
        }
        let holding = !st.slots[i].let_go && tick - st.slots[i].asked_at <= UNASKED_MS;
        if holding {
            if current_origin.is_none() {
                continue;
            }
            seat_for(st, i, dt);
            let s = &mut st.slots[i];
            let Some(pose) = hand_pose(s) else { continue };
            let input = SteerIn {
                target: [pose.p[0], pose.p[1], pose.p[2], pose.q[0], pose.q[1], pose.q[2], pose.q[3]],
                origin: s.origin,
                gravity: scene_gravity(scene),
                dt,
                first: !s.driving,
            };
            let actor = s.actor as P;
            // taken off a stack: what stood on it comes down (before this step moves it)
            if input.first {
                super::api::taken(actor);
            }
            if input.first && filter(actor, &mut s.shapes, &mut s.filters, &mut s.shape_count) && s.shape_count != 0 {
                super::api::filter_again(scene, actor);
                log!(
                    "[heldbody] proxy {} body {}: {} shapes held apart from the loose things, or as asked (was {:08x} {:08x} {:08x} {:08x})\n",
                    s.proxy,
                    s.body,
                    s.shape_count,
                    s.filters[0][0],
                    s.filters[0][1],
                    s.filters[0][2],
                    s.filters[0][3]
                );
            }
            match steer(actor, &input, &mut s.saved) {
                Some((gap, mass)) => {
                    if input.first {
                        log!(
                            "[heldbody] proxy {} body {}: steered into the hand, {:.2} kg, {:.3} m off at the first step ({:.1} ms)\n",
                            s.proxy,
                            s.body,
                            mass,
                            gap,
                            dt * 1000.0
                        );
                    }
                    s.driving = true;
                    s.targets += 1;
                }
                None => {
                    if s.drive_failed < 4 {
                        log!("[heldbody] proxy {} body {}: PhysX refused\n", s.proxy, s.body);
                    }
                    s.drive_failed += 1;
                }
            }
        } else if st.slots[i].driving {
            unseat(&mut st.fingers, &mut st.slots[i]);
            let s = &mut st.slots[i];
            s.grasp_wanted = false;
            let actor = s.actor as P;
            let freed = unsteer(actor, &s.saved, &s.throw_v, &s.throw_w, s.throw_given > 0.5);
            if s.shape_count != 0 && unfilter(&s.shapes, &s.filters, s.shape_count) {
                super::api::filter_again(scene, actor);
            }
            s.shape_count = 0;
            log!(
                "[heldbody] proxy {} body {} let go after {} steps in the hand: {}, {} ({:.2} {:.2} {:.2}) m/s\n",
                s.proxy,
                s.body,
                s.targets,
                if freed { "its own again" } else { "PhysX refused" },
                if s.throw_given > 0.5 { "thrown at" } else { "as it went, not" },
                s.throw_v[0],
                s.throw_v[1],
                s.throw_v[2]
            );
            s.driving = false;
            s.targets = 0;
        }
    }
}

static SUBMIT_LAST: AtomicU64 = AtomicU64::new(0);
static mut SUBMIT_COPY: [u8; RECORD_SIZE * MOST_INSTANCES as usize] = [0; RECORD_SIZE * MOST_INSTANCES as usize];
/// AtFrameSubmit: the frame submit (game thread, before the proxy flush): the fingers eased, and each carried thing's
/// picture sent once more with the hand as animated for this frame.
pub unsafe fn at_frame_submit() {
    super::super::frame_order::note_submit();
    if !INSTALLED.load(Acquire) {
        return;
    }
    ease_fingers(&mut lock());
    if !ANY_USED.load(Relaxed) {
        return;
    }
    let Some(original) = ORIGINAL_UPDATE else { return };
    let (now, frequency) = super::qpc();
    let last = SUBMIT_LAST.swap(now as u64, Relaxed);
    let dt = if last != 0 { ((now as u64 - last) as f64 / frequency as f64).min(0.1) as f32 } else { 0.0 };
    let tick = tick_ms();
    let copy = &mut *(&raw mut SUBMIT_COPY);
    let mut guard = lock();
    let mut used = 0;
    for i in 0..2 {
        let st = &mut *guard;
        if !st.slots[i].used {
            continue;
        }
        let s = &mut st.slots[i];
        s.asked = !s.let_go && tick - s.asked_at <= UNASKED_MS;
        if !s.spun && !s.measure_only {
            s.spun = true;
            let (raised, was) = debug_view::raise_spin(s.proxy, s.body, MOST_SPIN);
            log!("[heldbody] proxy {} body {}: most spin {}, was {:.1} rad/s\n", s.proxy, s.body, if raised { "raised" } else { "left alone" }, was);
        }
        if s.blocked && s.asked {
            s.blocked_frames += 1;
        }
        // After the last ask the picture stays where the hand last had it, and goes to the body: the hand is off elsewhere
        // by then.
        if s.asked {
            if let Some(pose) = hand_pose(s) {
                s.hand = pose;
                s.posed = true;
            }
        }
        let to_hand = s.asked && !s.blocked && s.posed && !s.measure_only;
        s.weight = if to_hand { (s.weight + dt / BLEND_IN).min(1.0) } else { (s.weight - dt / BLEND_OUT).max(0.0) };
        let instance = instance_of(s.proxy);
        // a body held by itself is not given up before the step has put its damping back, unless it is gone
        if instance.is_none() || (!s.asked && s.weight <= 0.0 && !(s.own_body && s.driving)) {
            report(s);
            unseat(&mut st.fingers, &mut st.slots[i]);
            st.slots[i].used = false;
            continue;
        }
        let instance = instance.unwrap_or(0);
        used += 1;
        let Some(holder) = read::<usize>(system() + RECORDS + (s.proxy & 0xFFFF) as usize * 8).filter(|h| *h != 0) else { continue };
        let records = read::<usize>(holder).unwrap_or(0);
        let count = read::<u32>(holder + 8).unwrap_or(0);
        if records == 0 || count == 0 || count > MOST_INSTANCES || s.body >= count {
            continue;
        }
        let bytes = count as usize * RECORD_SIZE;
        if !read_bytes(records, &mut copy[..bytes]) {
            continue;
        }
        let at = s.body as usize * RECORD_SIZE;
        // what the eye will see of the body alone
        let fixed = [0, 1, 2].map(|k| record_fixed(&copy[at..], k) as f64 / FIXED);
        let gap = [s.hand.p[0] as f64 - fixed[0], s.hand.p[1] as f64 - fixed[1], s.hand.p[2] as f64 - fixed[2]];
        let behind = (gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2]).sqrt();
        if s.posed {
            s.frames += 1;
            s.behind_sum += behind;
            s.behind_most = s.behind_most.max(behind);
            if s.frames == 1 {
                s.instances = count;
                s.first_tick = tick_ms();
                let q = record_quat(&copy[at..]);
                log!(
                    "[heldbody] proxy {} body {} of {}: first frame, body at ({:.3} {:.3} {:.3}), hand has it at ({:.3} {:.3} {:.3}), quat {:.3} {:.3} {:.3} {:.3} and {:.3} {:.3} {:.3} {:.3}, submit thread {}\n",
                    s.proxy,
                    s.body,
                    count,
                    fixed[0],
                    fixed[1],
                    fixed[2],
                    s.hand.p[0],
                    s.hand.p[1],
                    s.hand.p[2],
                    q[0],
                    q[1],
                    q[2],
                    q[3],
                    s.hand.q[0],
                    s.hand.q[1],
                    s.hand.q[2],
                    s.hand.q[3],
                    GetCurrentThreadId()
                );
            }
        }
        if s.last_detour != 0 && s.last_detour != s.last_submit {
            s.detour_gap_sum += (now as u64).wrapping_sub(s.last_detour) as i64 as f64 * 1000.0 / frequency as f64;
            s.detour_gaps += 1;
            s.last_submit = s.last_detour;
        }
        if !picture(s, &mut copy[at..at + RECORD_SIZE]) {
            continue;
        }
        s.sends += 1;
        // Two things on one instance are rare, and each is put right by the other's next frame.
        drop(guard);
        let mut span = Span { begin: copy.as_mut_ptr(), end: copy.as_mut_ptr().add(bytes) };
        original(instance as *mut u8, (&raw mut span).cast(), count);
        guard = lock();
    }
    if used == 0 {
        ANY_USED.store(false, SeqCst);
    }
}

/// The record the physics system has for a body now. A torn read gives a wrong box for one frame, nothing worse.
fn read_record(proxy: u32, body: u32) -> Option<[u8; RECORD_SIZE]> {
    let system = system();
    if system == 0 {
        return None;
    }
    let holder = read::<usize>(system + RECORDS + (proxy & 0xFFFF) as usize * 8).filter(|h| *h != 0)?;
    let records = read::<usize>(holder)?;
    let count = read::<u32>(holder + 8)?;
    if records == 0 || count == 0 || count > MOST_INSTANCES || body >= count {
        return None;
    }
    let mut out = [0u8; RECORD_SIZE];
    read_bytes(records + body as usize * RECORD_SIZE, &mut out).then_some(out)
}
fn pose_of(record: &[u8]) -> [f32; 7] {
    let q = record_quat(record);
    let p = [0, 1, 2].map(|k| (record_fixed(record, k) as f64 / FIXED) as f32);
    [p[0], p[1], p[2], q[0], q[1], q[2], q[3]]
}
/// A thing carried now, as a partner in co-op is shown it: which hand, its mesh (the resource path's hash, 0 unknown) and
/// where it is drawn in the world (x y z, then the quaternion x y z w).
#[derive(Clone, Copy)]
pub struct Carried {
    pub proxy: u32,
    pub body: u32,
    pub left: bool,
    pub mesh: u64,
    pub pose: [f32; 7],
}
/// Every thing held in a hand now (not the ones only measured), with its drawn pose; the meshes looked up once per
/// proxy and kept.
pub fn carried_now() -> Vec<Carried> {
    static MESHES: Mutex<Vec<(u32, u64)>> = Mutex::new(Vec::new());
    if !INSTALLED.load(Acquire) || !ANY_USED.load(Relaxed) {
        return Vec::new();
    }
    // one held by its own body only measures its picture, but it is in the hand all the same (its body is)
    let held: Vec<(u32, u32, i32)> =
        lock().slots.iter().filter(|s| s.used && !s.let_go && (!s.measure_only || s.own_body)).map(|s| (s.proxy, s.body, s.bone)).collect();
    // SAFETY: a read of the published bone index.
    let left_bone = unsafe { core::ptr::read_volatile(&raw const g_VRLeftBoneIdx) };
    let mut out = Vec::new();
    for (proxy, body, bone) in held {
        let Some((_, picture, _, _, _)) = view(proxy, body) else { continue };
        let mesh = {
            let mut m = MESHES.lock().unwrap_or_else(|e| e.into_inner());
            match m.iter().find(|(p, _)| *p == proxy) {
                Some((_, h)) => *h,
                None => {
                    let h = mesh_of(proxy);
                    if m.len() > 64 {
                        m.remove(0);
                    }
                    m.push((proxy, h));
                    h
                }
            }
        };
        out.push(Carried { proxy, body, left: bone == left_bone, mesh, pose: picture });
    }
    out
}
/// View: a carried thing, for a debug view: (its body's pose, its picture's, the hand's, whether the hand is known, the
/// picture's weight).
pub fn view(proxy: u32, body: u32) -> Option<([f32; 7], [f32; 7], [f32; 7], bool, f32)> {
    if !INSTALLED.load(Acquire) || !ANY_USED.load(Relaxed) {
        return None;
    }
    let mut s = *lock().slots.iter().find(|t| t.used && t.proxy == proxy && t.body == body)?;
    instance_of(proxy)?;
    // the hand as animated now, as the next submit will have it
    if s.asked {
        // SAFETY: reads of the player's published pose.
        if let Some(pose) = unsafe { hand_pose(&s) } {
            s.hand = pose;
            s.posed = true;
        }
    }
    let record = read_record(proxy, body)?;
    let mut drawn = record;
    let body_pose = pose_of(&record);
    // a copy of the slot: what picture() keeps of a let-go thing stays the submit's own
    let picture_pose = pose_of(if picture(&mut s, &mut drawn) { &drawn } else { &record });
    let hand = if s.posed { [s.hand.p[0], s.hand.p[1], s.hand.p[2], s.hand.q[0], s.hand.q[1], s.hand.q[2], s.hand.q[3]] } else { body_pose };
    Some((body_pose, picture_pose, hand, s.posed, s.weight))
}

unsafe extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}
const UPDATE_PROLOGUE: [u8; 16] = [0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x18, 0x48, 0x89, 0x50, 0x10, 0x55, 0x56, 0x57, 0x41, 0x54];
const LISTENER_PROLOGUE: [u8; 15] = [0x48, 0x83, 0xec, 0x38, 0x4c, 0x8b, 0x42, 0x08, 0x48, 0x81, 0xc1, 0x00, 0xff, 0xff, 0xff];
/// HeldBody's hook (boot, 84): the instances' pose records' send, and the baked grasps read on a thread of their own.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_held_body_install() -> bool {
    if !cvr_engine::game_build::supported() {
        return false;
    }
    let b = image_base();
    let (mut update_head, mut listener_head) = ([0u8; 16], [0u8; 15]);
    if !read_bytes(b + UPDATE_RVA, &mut update_head) || update_head != UPDATE_PROLOGUE || !read_bytes(b + LISTENER_ENTRY_RVA, &mut listener_head) || listener_head != LISTENER_PROLOGUE {
        log!("[heldbody] these are not the functions this was read from: carried things stay as they were\n");
        return false;
    }
    let target = (b + UPDATE_RVA) as *mut c_void;
    if MH_CreateHook(target, update as *const () as *mut c_void, (&raw mut ORIGINAL_UPDATE).cast()) != 0 {
        return false;
    }
    if MH_EnableHook(target) != 0 {
        MH_RemoveHook(target);
        return false;
    }
    INSTALLED.store(true, Release);
    std::thread::spawn(|| {
        let meshes = baked_grasps();
        log!("[heldbody] baked grasps: {} meshes from {}\n", meshes, baked_grasps_path().display());
    });
    true
}

// THE GAME'S OWN OUTLINE ON A THING WITHOUT AN ENTITY. entRenderHighlightEvent ends in sub_14079FE8C(render object,
// params) and that in a command of the render scene: slot 0x2D8 (sub_1407A0190), arguments &object+8, &object+0x10, on,
// mask, opacity, 0. Read in the exe, all callers (sub_14079FE8C, sub_140402428, sub_1403DA0F4) alike. Mask: 1 an
// outline, 2 a fill, the index <<2, 0x40 through walls. The render side (sub_14067AAB4) calls slot 0xD0 of what +0x10
// holds and goes on only for what that answers with a type (+0x7D >>3) of 1, 5 or 7. A prop's object is at
// instance+0xC8 and is of the same make: sub_14020A038 reads it the same way, +8 a thing with a vtable, +0x10 the handle.
// The first try passed +0x10 first and null second, read as fill and outline: a crash on the render thread at
// sub_14067AAFF. The whole object lights up: a node's instances together.
thread_local! {
    static KNOWN_REGIONS: core::cell::RefCell<([(usize, usize); 8], usize)> = const { core::cell::RefCell::new(([(0, 0); 8], 0)) };
}
/// The regions VirtualQuery said were readable are kept (it costs 7 to 12 ms a call on this heap).
fn readable(at: usize, size: usize) -> bool {
    if !(0x10000..0x0000_7FFF_FFFF_FFFF).contains(&at) {
        return false;
    }
    KNOWN_REGIONS.with(|k| {
        let mut k = k.borrow_mut();
        if k.0.iter().any(|&(lo, hi)| at >= lo && at + size <= hi) {
            return true;
        }
        let mut m: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
        // SAFETY: a query of the process's own address space.
        if unsafe { VirtualQuery(at as *const c_void, &mut m, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0
            || m.State != MEM_COMMIT
            || (m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) != 0
        {
            return false;
        }
        let lo = m.BaseAddress as usize;
        let end = lo + m.RegionSize;
        let next = k.1;
        k.0[next] = (lo, end);
        k.1 = (next + 1) % 8;
        at + size <= end
    })
}
/// The image's .text section, read from its PE headers: (start, end).
fn text_section(image: usize) -> Option<(usize, usize)> {
    let nt = image + read::<u32>(image + 0x3C)? as usize;
    let sections = read::<u16>(nt + 6)? as usize;
    let optional = read::<u16>(nt + 20)? as usize;
    let first = nt + 24 + optional;
    (0..sections).find_map(|i| {
        let at = first + 40 * i;
        let mut name = [0u8; 8];
        (read_bytes(at, &mut name) && name.starts_with(b".text")).then(|| {
            let size = read::<u32>(at + 8).unwrap_or(0) as usize;
            let rva = read::<u32>(at + 12).unwrap_or(0) as usize;
            (image + rva, image + rva + size)
        })
    })
}
/// In the image, in .text: a slot of a vtable that has it.
fn code_of_image(at: usize) -> bool {
    static TEXT: OnceLock<(usize, usize)> = OnceLock::new();
    let (lo, hi) = *TEXT.get_or_init(|| text_section(image_base()).unwrap_or((0, 0)));
    at >= lo && at < hi
}
type Frame = StackFrame;
macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
#[no_mangle]
pub unsafe extern "C" fn VRHeldPictureLetGo(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body) = (0i32, 0i32);
    params!(frame, proxy, body);
    if proxy > 0 && body >= 0 {
        let_go(proxy as u32, body as u32);
    }
    put(out, true);
}
static SAID_TYPE: Mutex<[u8; 64]> = Mutex::new([0; 64]);
static SAID_NO_ANSWER: AtomicBool = AtomicBool::new(false);
static SAID_NOT_THERE: AtomicBool = AtomicBool::new(false);
/// VRThingHighlight(proxy, fill, outline, through) -> Bool: the game's own outline (or fill) on a thing without an entity.
#[no_mangle]
pub unsafe extern "C" fn VRThingHighlight(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut fill, mut outline, mut through) = (0i32, 0i32, 0i32, false);
    params!(frame, proxy, fill, outline, through);
    put(out, false);
    let b = base();
    if b == 0 || proxy <= 0 {
        return;
    }
    let Some(instance) = instance_of(proxy as u32) else { return };
    let Some(object) = read::<usize>(instance + 0xC8) else { return };
    if !readable(object, 0x18) {
        return;
    }
    let (Some(owner), Some(held)) = (read::<usize>(object + 8), read::<usize>(object + 0x10)) else { return };
    if !readable(owner, 16) || !readable(held, 16) {
        return;
    }
    let Some(table) = read::<usize>(held) else { return };
    if !readable(table, 0xD8) || !read::<usize>(table + 0xD0).is_some_and(code_of_image) {
        return;
    }
    let answer = read::<usize>(table + 0xD0)
        .and_then(|f| cvr_engine::code::guarded_call4(f, held as *mut c_void, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()))
        .map_or(0, |a| a as usize);
    let kind = if answer != 0 && readable(answer + 0x7D, 1) { read::<u8>(answer + 0x7D).map_or(-1, |t| i32::from(t >> 3)) } else { -1 };
    if (0..64).contains(&kind) {
        let mut said = SAID_TYPE.lock().unwrap_or_else(|e| e.into_inner());
        if said[kind as usize] < 1 {
            said[kind as usize] += 1;
            log!(
                "[heldbody] outline on proxy {}: render object type {}, {}\n",
                proxy,
                kind,
                if kind == 1 || kind == 5 || kind == 7 { "it takes one" } else { "it does not take one: its lines instead" }
            );
        }
    } else if kind < 0 && !SAID_NO_ANSWER.swap(true, Relaxed) {
        log!("[heldbody] outline on proxy {}: no render object answers\n", proxy);
    }
    if kind != 1 && kind != 5 && kind != 7 {
        return;
    }
    let Some(renderer) = read::<usize>(b + 0x3BB_CAE8).filter(|r| *r != 0) else { return };
    let get_scene: unsafe extern "C" fn(*mut c_void) -> *mut c_void = core::mem::transmute(*(*(renderer as *const *const usize)).add(0x4D0 / 8));
    let scene = get_scene(renderer as *mut c_void);
    if scene.is_null() {
        return;
    }
    let entry = *(*(scene as *const *const usize)).add(0x2D8 / 8);
    if entry != b + 0x7A_0190 {
        if !SAID_NOT_THERE.swap(true, Relaxed) {
            log!("[heldbody] the scene's highlight is not where it was read: no highlight\n");
        }
        return;
    }
    let on = fill > 0 || outline > 0;
    let mut mask: u8 = if through { 0x40 } else { 0 };
    if fill > 0 {
        mask |= ((fill << 2) as u8) | 2;
    } else if outline > 0 {
        mask |= (outline << 2) as u8;
    }
    if outline > 0 {
        mask |= 1;
    }
    let command: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u8, u8, f32, u8) = core::mem::transmute(entry);
    command(scene, (object + 8) as *mut c_void, (object + 0x10) as *mut c_void, u8::from(on), mask, 1.0, 0);
    put(out, true);
}
/// VRHeldBody(proxy, body, bone, offset, turn) -> Bool: the thing held by its own body, steered at each PhysX step to
/// where the hand has it (offset and turn in that hand's frame, as for VRHeldPicture). False: it cannot be, carry it as
/// before.
#[no_mangle]
pub unsafe extern "C" fn VRHeldBody(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body, mut bone, mut offset, mut turn) = (0i32, 0i32, -1i32, [0.0f32; 4], [0.0f32; 4]);
    params!(frame, proxy, body, bone, offset, turn);
    let o = [offset[0], offset[1], offset[2]];
    let on = proxy > 0
        && body >= 0
        && bone >= 0
        && (o[0] + o[1] + o[2] + turn[0] + turn[1] + turn[2] + turn[3]).is_finite()
        && hold_body(proxy as u32, body as u32, bone, &o, &turn);
    put(out, on);
}
/// VRHeldBodyGrasp(proxy, body, on) -> Bool: a body held by VRHeldBody is (on) or is not (off) in one hand alone: seated
/// there for the grasp its shape and the way the hand came to it make, the fingers closed on it (grasp.rs). False if it
/// is not held by its own body.
#[no_mangle]
pub unsafe extern "C" fn VRHeldBodyGrasp(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body, mut on) = (0i32, 0i32, false);
    params!(frame, proxy, body, on);
    put(out, proxy > 0 && body >= 0 && grasp_body(proxy as u32, body as u32, on));
}
/// VRBodyGraspShape(proxy, body) -> String: the shape a hand would close on and, for a long round one, the part a palm at
/// either end and at its middle would take (a test tool; empty when it has no actor).
#[no_mangle]
pub unsafe extern "C" fn VRBodyGraspShape(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _: i64) {
    let (mut proxy, mut body) = (0i32, 0i32);
    params!(frame, proxy, body);
    let text = if proxy > 0 && body >= 0 { grasp_shape_text(proxy as u32, body as u32) } else { String::new() };
    if !out.is_null() {
        CString::assign(out, &text);
    }
}
/// VRHeldBodyFilter(word0, word2): the bits a held body's shapes get added to their simulation filter, from the next
/// hold. word2 0xFFFFFFFF is "its own group"; 0, 0 (the default) leaves the game's filter alone. For finding what the
/// game's filter wants, not for play: see HELD_FILTER_OR.
#[no_mangle]
pub unsafe extern "C" fn VRHeldBodyFilter(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut w0, mut w2) = (0i32, -1i32);
    params!(frame, w0, w2);
    set_held_filter(w0 as u32, w2 as u32);
    put(out, true);
}
/// VRHeldBodyLetGo(proxy, body, velocity, spin) -> Bool: lets a body held by VRHeldBody go at the next step, with that
/// throw when velocity.w is 1 (0: it goes on as it was going).
#[no_mangle]
pub unsafe extern "C" fn VRHeldBodyLetGo(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body, mut velocity, mut spin) = (0i32, 0i32, [0.0f32; 4], [0.0f32; 4]);
    params!(frame, proxy, body, velocity, spin);
    let v = [velocity[0], velocity[1], velocity[2]];
    let w = [spin[0], spin[1], spin[2]];
    let thrown = velocity[3] > 0.5 && (v[0] + v[1] + v[2] + w[0] + w[1] + w[2]).is_finite();
    if proxy > 0 && body >= 0 {
        let_go_body(proxy as u32, body as u32, &v, &w, thrown);
    }
    put(out, proxy > 0 && body >= 0);
}
/// VRHeldPictureView(proxy, body, part): a carried thing, for a debug view. 0 its body's position and 1 its rotation, as
/// the physics system has them (what hits things); 2 its picture's position with the picture's weight as w (0 on the
/// body, 1 on the hand) and 3 its rotation, as the next submit will send them; 4 where the hand has it, w 1 once known,
/// and 5 that rotation. With the picture setting off the thing is still measured, and the picture is its body. All zero:
/// not a carried thing.
#[no_mangle]
pub unsafe extern "C" fn VRHeldPictureView(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _: i64) {
    let (mut proxy, mut body, mut part) = (0i32, 0i32, 0i32);
    params!(frame, proxy, body, part);
    if out.is_null() {
        return;
    }
    *out = [0.0; 4];
    if proxy <= 0 || body < 0 {
        return;
    }
    let Some((b, p, h, known, w)) = view(proxy as u32, body as u32) else { return };
    let v = if part < 2 {
        b
    } else if part < 4 {
        p
    } else {
        h
    };
    *out = if part % 2 == 0 {
        [
            v[0],
            v[1],
            v[2],
            if part == 0 {
                1.0
            } else if part == 2 {
                w
            } else if known {
                1.0
            } else {
                0.0
            },
        ]
    } else {
        [v[3], v[4], v[5], v[6]]
    };
}
/// VRHeldPicture(proxy, body, bone, offset with the reach as its w, turn as x y z w, blocked)
#[no_mangle]
pub unsafe extern "C" fn VRHeldPicture(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _: i64) {
    let (mut proxy, mut body, mut bone, mut offset, mut turn, mut blocked) = (0i32, 0i32, -1i32, [0.0f32; 4], [0.0f32; 4], false);
    params!(frame, proxy, body, bone, offset, turn, blocked);
    let o = [offset[0], offset[1], offset[2]];
    let reach = if offset[3].is_finite() && offset[3] >= 0.0 { offset[3] } else { 1000.0 };
    let on = proxy > 0 && body >= 0 && (o[0] + o[1] + o[2] + turn[0] + turn[1] + turn[2] + turn[3]).is_finite() && hold(proxy as u32, body as u32, bone, &o, &turn, blocked, reach);
    put(out, on);
}
