//! THE SLOTS. The port carries no PhysX headers, so every call is by table slot, each read in PhysX3_x64.dll itself
//! (2026-11-08): the scene's and the rigid body's tables by the error messages their functions hold and by the slots
//! the port already calls; PxPhysics's table read live (PxGetPhysics -> its table at RVA 0x20A310) and its functions told
//! apart by their arguments (createRigidStatic/createRigidDynamic take a transform, createShape a geometry, materials, a
//! count, a flag and the shape flags by address, createMaterial three floats and then adds the material to the
//! physics). Before the first call every slot used is checked against the address of its function in this very DLL;
//! any other DLL leaves the hands as they were.
#![allow(dead_code)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering::*};
use cvr_host::log;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

pub type P = *mut c_void;

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct PxTransform {
    pub q: [f32; 4],
    pub p: [f32; 3],
}
impl Default for PxTransform {
    fn default() -> PxTransform {
        PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: [0.0; 3] }
    }
}
impl PxTransform {
    pub const ZERO: PxTransform = PxTransform { q: [0.0; 4], p: [0.0; 3] };
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct PxBounds {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct BoxGeometry {
    pub kind: i32,
    pub half: [f32; 3],
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CapsuleGeometry {
    pub kind: i32,
    pub radius: f32,
    pub half_height: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct SphereGeometry {
    pub kind: i32,
    pub radius: f32,
}

pub const PHYSICS_TABLE: usize = 0x20A310;
pub const SCENE_TABLE: usize = 0x20CAF0;
pub const DYNAMIC_TABLE: usize = 0x20B2E8;
pub const SHAPE_TABLE: usize = 0x20E9A8;
pub const MATERIAL_TABLE: usize = 0x209820;
/// What PxGetPhysics returns.
pub const PHYSICS_GLOBAL: usize = 0x25B5D8;
// PxPhysics
pub const PHYSICS_GET_NB_SCENES: usize = 19;
pub const PHYSICS_GET_SCENES: usize = 20;
pub const CREATE_RIGID_DYNAMIC: usize = 22;
pub const CREATE_SHAPE: usize = 27;
pub const CREATE_MATERIAL: usize = 32;
// PxScene
pub const ADD_ACTOR: usize = 10;
pub const REMOVE_ACTOR: usize = 13;
pub const SCENE_GET_NB_ACTORS: usize = 18;
pub const SCENE_GET_ACTORS: usize = 19;
pub const SCENE_GET_NB_AGGREGATES: usize = 26;
pub const SCENE_GET_AGGREGATES: usize = 27;
pub const AGGREGATE_GET_ACTORS: usize = 11;
/// resetFiltering(PxActor&): MSVC puts the two overloads in reverse, so the actor's is 55 (the one switching on the
/// actor's type with "only PxParticleBase and PxRigidActor support this operation!"), the shapes' 54.
pub const SCENE_RESET_FILTERING: usize = 55;
// PxActor / PxRigidActor / PxRigidBody / PxRigidDynamic
pub const RELEASE: usize = 0;
pub const GET_TYPE: usize = 5;
pub const GET_SCENE: usize = 6;
pub const GET_WORLD_BOUNDS: usize = 9;
pub const SET_ACTOR_FLAG: usize = 10;
pub const GET_GLOBAL_POSE: usize = 20;
pub const SET_GLOBAL_POSE: usize = 21;
pub const ATTACH_SHAPE: usize = 23;
pub const GET_SHAPES: usize = 26;
pub const SET_CMASS_LOCAL_POSE: usize = 29;
pub const SET_MASS: usize = 31;
pub const GET_MASS: usize = 32;
pub const SET_INERTIA: usize = 34;
pub const GET_LINEAR_VELOCITY: usize = 37;
pub const SET_LINEAR_VELOCITY: usize = 38;
pub const GET_ANGULAR_VELOCITY: usize = 39;
pub const SET_ANGULAR_VELOCITY: usize = 40;
pub const SET_RIGID_BODY_FLAG: usize = 45;
pub const GET_RIGID_BODY_FLAGS: usize = 47;
pub const SET_MAX_DEPENETRATION: usize = 50;
pub const SET_LINEAR_DAMPING: usize = 56;
pub const SET_ANGULAR_DAMPING: usize = 58;
pub const SET_MAX_ANGULAR_VELOCITY: usize = 60;
pub const IS_SLEEPING: usize = 62;
pub const WAKE_UP: usize = 72;
pub const PUT_TO_SLEEP: usize = 73;
pub const SET_SOLVER_ITERATIONS: usize = 74;
/// getCMassLocalPose (the body-to-actor transform) and getLinearDamping, for a body held by its point (HoldBody).
pub const GET_CMASS_LOCAL_POSE: usize = 30;
pub const GET_LINEAR_DAMPING: usize = 57;
/// getMassSpaceInertiaTensor (the buffered tensor, like setMassSpaceInertiaTensor's 34) and getMaxAngularVelocity.
pub const GET_INERTIA: usize = 35;
pub const GET_MAX_ANGULAR_VELOCITY: usize = 61;
// PxShape
pub const SHAPE_RELEASE: usize = 0;
pub const SHAPE_GET_GEOMETRY_TYPE: usize = 7;
pub const SHAPE_SET_GEOMETRY: usize = 8;
pub const SHAPE_GET_BOX: usize = 10;
pub const SHAPE_GET_SPHERE: usize = 11;
pub const SHAPE_GET_CAPSULE: usize = 12;
pub const SHAPE_GET_CONVEX: usize = 14;
pub const SHAPE_GET_TRIANGLES: usize = 15;
pub const SHAPE_GET_HEIGHT_FIELD: usize = 16;
pub const SHAPE_SET_LOCAL_POSE: usize = 18;
pub const SHAPE_GET_LOCAL_POSE: usize = 19;
pub const SHAPE_SET_SIM_FILTER: usize = 20;
pub const SHAPE_GET_SIM_FILTER: usize = 21;
pub const SHAPE_SET_MATERIALS: usize = 24;
pub const SHAPE_SET_CONTACT_OFFSET: usize = 28;
pub const SHAPE_GET_CONTACT_OFFSET: usize = 29;
pub const SHAPE_SET_FLAGS: usize = 33;
pub const SHAPE_GET_FLAGS: usize = 34;
/// PxMaterial (NpMaterial's table, found by the one function holding "PxMaterial::setRestitution", slot 11; the 3.4
/// header's order from there: get/set friction, restitution, flags, then the combine modes).
pub const MATERIAL_SET_FRICTION_COMBINE: usize = 16;
/// PxCombineMode::eMIN.
pub const COMBINE_MIN: i32 = 1;
pub const GEOM_SPHERE: i32 = 0;
pub const GEOM_PLANE: i32 = 1;
pub const GEOM_CAPSULE: i32 = 2;
pub const GEOM_BOX: i32 = 3;
pub const GEOM_CONVEX: i32 = 4;
pub const GEOM_TRIANGLES: i32 = 5;
pub const GEOM_HEIGHT_FIELD: i32 = 6;
// PxActorFlag, PxRigidBodyFlag
pub const DISABLE_GRAVITY: i32 = 2;
pub const DISABLE_SIMULATION: i32 = 8;
pub const ENABLE_CCD: i32 = 4;
pub const ENABLE_SPECULATIVE_CCD: i32 = 32;

struct SlotCheck {
    table: usize,
    slot: usize,
    rva: usize,
}
const fn c(table: usize, slot: usize, rva: usize) -> SlotCheck {
    SlotCheck { table, slot, rva }
}
const CHECKS: &[SlotCheck] = &[
    c(PHYSICS_TABLE, PHYSICS_GET_NB_SCENES, 0x47fc0),
    c(PHYSICS_TABLE, PHYSICS_GET_SCENES, 0x48000),
    c(PHYSICS_TABLE, CREATE_RIGID_DYNAMIC, 0x48150),
    c(PHYSICS_TABLE, CREATE_SHAPE, 0x48270),
    c(PHYSICS_TABLE, CREATE_MATERIAL, 0x48410),
    c(SCENE_TABLE, ADD_ACTOR, 0x520d0),
    c(SCENE_TABLE, REMOVE_ACTOR, 0x521e0),
    c(SCENE_TABLE, SCENE_GET_NB_ACTORS, 0x52b60),
    c(SCENE_TABLE, SCENE_GET_ACTORS, 0x52cb0),
    c(SCENE_TABLE, SCENE_GET_NB_AGGREGATES, 0x52790),
    c(SCENE_TABLE, SCENE_GET_AGGREGATES, 0x527a0),
    c(DYNAMIC_TABLE, RELEASE, 0x4a800),
    c(DYNAMIC_TABLE, GET_TYPE, 0x4df20),
    c(DYNAMIC_TABLE, GET_WORLD_BOUNDS, 0x4df30),
    c(DYNAMIC_TABLE, SET_ACTOR_FLAG, 0x4e430),
    c(DYNAMIC_TABLE, SET_GLOBAL_POSE, 0x4a8e0),
    c(DYNAMIC_TABLE, ATTACH_SHAPE, 0x4d400),
    c(DYNAMIC_TABLE, SET_MASS, 0x4edc0),
    c(DYNAMIC_TABLE, GET_MASS, 0x4dce0),
    c(DYNAMIC_TABLE, SET_INERTIA, 0x4ee60),
    // setCMassLocalPose: the large one before getCMassLocalPose (30, the body-to-actor transform, buffered or not) and
    // setMass
    c(DYNAMIC_TABLE, SET_CMASS_LOCAL_POSE, 0x4b4d0),
    c(DYNAMIC_TABLE, GET_LINEAR_VELOCITY, 0x4dcc0),
    c(DYNAMIC_TABLE, SET_LINEAR_VELOCITY, 0x4bd90),
    c(DYNAMIC_TABLE, GET_ANGULAR_VELOCITY, 0x4d800),
    c(DYNAMIC_TABLE, SET_ANGULAR_VELOCITY, 0x4be10),
    c(DYNAMIC_TABLE, SET_RIGID_BODY_FLAG, 0x4f1f0),
    c(DYNAMIC_TABLE, GET_RIGID_BODY_FLAGS, 0x4ded0),
    // writes +0xfc under the 0x400 dirty bit, as its getter (51) reads
    c(DYNAMIC_TABLE, SET_MAX_DEPENETRATION, 0x4eff0),
    c(DYNAMIC_TABLE, SET_LINEAR_DAMPING, 0x4bc10),
    c(DYNAMIC_TABLE, SET_ANGULAR_DAMPING, 0x4bcd0),
    c(DYNAMIC_TABLE, SET_MAX_ANGULAR_VELOCITY, 0x4be90),
    c(DYNAMIC_TABLE, WAKE_UP, 0x4c2d0),
    c(DYNAMIC_TABLE, SET_SOLVER_ITERATIONS, 0x4c300),
    c(DYNAMIC_TABLE, GET_CMASS_LOCAL_POSE, 0x4d820),
    c(DYNAMIC_TABLE, GET_LINEAR_DAMPING, 0x4bca0),
    c(DYNAMIC_TABLE, GET_INERTIA, 0x4dd20),
    c(DYNAMIC_TABLE, GET_MAX_ANGULAR_VELOCITY, 0x4bf20),
    c(SHAPE_TABLE, SHAPE_RELEASE, 0x661e0),
    c(SHAPE_TABLE, SHAPE_GET_GEOMETRY_TYPE, 0x66250),
    c(SHAPE_TABLE, SHAPE_SET_GEOMETRY, 0x66260),
    c(SHAPE_TABLE, SHAPE_GET_BOX, 0x66480),
    c(SHAPE_TABLE, SHAPE_GET_SPHERE, 0x664d0),
    c(SHAPE_TABLE, SHAPE_GET_CAPSULE, 0x66500),
    c(SHAPE_TABLE, SHAPE_GET_CONVEX, 0x66570),
    c(SHAPE_TABLE, SHAPE_GET_TRIANGLES, 0x66600),
    c(SHAPE_TABLE, SHAPE_GET_HEIGHT_FIELD, 0x66680),
    c(SHAPE_TABLE, SHAPE_SET_LOCAL_POSE, 0x66700),
    c(SHAPE_TABLE, SHAPE_GET_LOCAL_POSE, 0x66910),
    c(SHAPE_TABLE, SHAPE_SET_SIM_FILTER, 0x66960),
    c(SHAPE_TABLE, SHAPE_GET_SIM_FILTER, 0x66a80),
    c(SHAPE_TABLE, SHAPE_SET_FLAGS, 0x671b0),
    c(SHAPE_TABLE, SHAPE_GET_FLAGS, 0x67200),
    // setMaterials: the one before getNbMaterials, getMaterials and getMaterialFromInternalFaceIndex (its message)
    c(SHAPE_TABLE, SHAPE_SET_MATERIALS, 0x66b10),
    // the contact offset's setter and getter (the getter reads the buffered value under flag 0x10, as getRestOffset's
    // reads its own under 0x20 two slots on)
    c(SHAPE_TABLE, SHAPE_SET_CONTACT_OFFSET, 0x66ed0),
    c(SHAPE_TABLE, SHAPE_GET_CONTACT_OFFSET, 0x66fd0),
    // isSleeping reads the body's sleep word; putToSleep is the jump into Scb::Body right after wakeUp's
    c(DYNAMIC_TABLE, IS_SLEEPING, 0x4c090),
    c(DYNAMIC_TABLE, PUT_TO_SLEEP, 0x4c2f0),
    c(MATERIAL_TABLE, MATERIAL_SET_FRICTION_COMBINE, 0x42da0),
    c(SCENE_TABLE, SCENE_RESET_FILTERING, 0x53530),
];

/// PxGeometryQuery, exported by PhysX3Common_x64.dll: no scene in it, so the worker may call it.
pub type PenetrationFn = unsafe extern "C" fn(direction: *mut f32, depth: *mut f32, geom0: *const c_void, pose0: *const c_void, geom1: *const c_void, pose1: *const c_void) -> bool;
/// ...and the square of a point's distance from a shape, with the nearest point on it (0 inside).
pub type PointDistanceFn = unsafe extern "C" fn(point: *const f32, geom: *const c_void, pose: *const c_void, closest: *mut f32) -> f32;
static PENETRATION: AtomicUsize = AtomicUsize::new(0);
static POINT_DISTANCE: AtomicUsize = AtomicUsize::new(0);
pub fn penetration() -> Option<PenetrationFn> {
    let f = PENETRATION.load(Acquire);
    // SAFETY: the export's address, set once by prove().
    (f != 0).then(|| unsafe { core::mem::transmute::<usize, PenetrationFn>(f) })
}
pub fn point_distance() -> Option<PointDistanceFn> {
    let f = POINT_DISTANCE.load(Acquire);
    // SAFETY: as above.
    (f != 0).then(|| unsafe { core::mem::transmute::<usize, PointDistanceFn>(f) })
}

/// The function in `object`'s table at `slot`.
///
/// # Safety
/// `object` a live PhysX object whose table holds a function of type `F` at `slot` (checked once by prove()).
#[inline]
pub unsafe fn at<F: Copy>(object: P, slot: usize) -> F {
    debug_assert_eq!(core::mem::size_of::<F>(), core::mem::size_of::<usize>());
    // SAFETY: the caller's object; its first word is its table.
    unsafe {
        let table = *(object as *const *const usize);
        core::mem::transmute_copy::<usize, F>(&*table.add(slot))
    }
}

static DLL: AtomicUsize = AtomicUsize::new(0);
static PHYSICS: AtomicUsize = AtomicUsize::new(0);
/// 0 not yet, 1 yes, -1 no.
static PROVED: AtomicI32 = AtomicI32::new(0);
pub fn dll() -> usize {
    DLL.load(Relaxed)
}
pub fn physics() -> P {
    PHYSICS.load(Relaxed) as P
}
/// Prove: every slot used where it was read in this very PhysX3_x64.dll, PxPhysics where it was read, and
/// PxGeometryQuery's two functions from PhysX3Common_x64.dll.
pub fn prove() -> bool {
    let p = PROVED.load(Relaxed);
    if p != 0 {
        return p > 0;
    }
    PROVED.store(-1, Relaxed);
    // SAFETY: a module handle by name.
    let dll = unsafe { GetModuleHandleA(c"PhysX3_x64.dll".as_ptr().cast()) } as usize;
    if dll == 0 {
        return false;
    }
    DLL.store(dll, Relaxed);
    for ch in CHECKS {
        // SAFETY: the table's slot in the loaded DLL's image.
        let at = unsafe { *((dll + ch.table + 8 * ch.slot) as *const usize) };
        if at != dll + ch.rva {
            log!("[physhands] PhysX3_x64.dll is not the one read (table 0x{:x} slot {}): the hands stay as they were\n", ch.table, ch.slot);
            return false;
        }
    }
    // SAFETY: PxGetPhysics's global in the image.
    let physics = unsafe { *((dll + PHYSICS_GLOBAL) as *const usize) };
    // SAFETY: a nonzero PxPhysics's first word.
    if physics == 0 || unsafe { *(physics as *const usize) } != dll + PHYSICS_TABLE {
        log!("[physhands] no PxPhysics where it was read: the hands stay as they were\n");
        return false;
    }
    PHYSICS.store(physics, Relaxed);
    // SAFETY: a module handle by name and its exports by their decorated names.
    unsafe {
        let common = GetModuleHandleA(c"PhysX3Common_x64.dll".as_ptr().cast());
        if !common.is_null() {
            if let Some(f) = GetProcAddress(common, c"?computePenetration@PxGeometryQuery@physx@@SA_NAEAVPxVec3@2@AEAMAEBVPxGeometry@2@AEBVPxTransform@2@23@Z".as_ptr().cast()) {
                PENETRATION.store(f as usize, Release);
            }
            if let Some(f) = GetProcAddress(common, c"?pointDistance@PxGeometryQuery@physx@@SAMAEBVPxVec3@2@AEBVPxGeometry@2@AEBVPxTransform@2@PEAV32@@Z".as_ptr().cast()) {
                POINT_DISTANCE.store(f as usize, Release);
            }
        }
    }
    if penetration().is_none() {
        log!("[physhands] no PxGeometryQuery::computePenetration: the fingers are not bent\n");
    }
    PROVED.store(1, Relaxed);
    log!("[physhands] PhysX checked: {} slots where they were read\n", CHECKS.len());
    true
}

/// WorldScene: the game's world scene -- the one whose userData is the game's object for it (that object holds it at
/// +0x18) -- and that object's +0x134, where PhysX's origin is in the game's world.
///
/// # Safety
/// After prove(); on a thread that may ask PxPhysics for its scenes.
pub unsafe fn world_scene(origin: &mut [f32; 3]) -> P {
    let physics = physics();
    if physics.is_null() {
        return core::ptr::null_mut();
    }
    let mut found = [core::ptr::null_mut::<c_void>(); 32];
    // SAFETY: PxPhysics's slots, proved.
    unsafe {
        let n = at::<unsafe extern "C" fn(P) -> u32>(physics, PHYSICS_GET_NB_SCENES)(physics);
        let got = if n != 0 { at::<unsafe extern "C" fn(P, *mut P, u32, u32) -> u32>(physics, PHYSICS_GET_SCENES)(physics, found.as_mut_ptr(), 32, 0) } else { 0 };
        for &scene in found.iter().take(got.min(32) as usize) {
            if let Some(o) = origin_of(scene) {
                *origin = o;
                return scene;
            }
        }
    }
    core::ptr::null_mut()
}
/// The scene's owner (its userData at +8) when that owner holds it back at +0x18: the game's world scene.
unsafe fn owner_of(scene: P) -> Option<*const u8> {
    if scene.is_null() {
        return None;
    }
    // SAFETY: a live scene's userData word, and its owner's word.
    unsafe {
        let owner = *((scene as *const u8).add(8) as *const *const u8);
        (!owner.is_null() && *(owner.add(0x18) as *const P) == scene).then_some(owner)
    }
}
unsafe fn origin_of(scene: P) -> Option<[f32; 3]> {
    // SAFETY: the owner's three floats at +0x134.
    unsafe {
        let owner = owner_of(scene)?;
        let o = owner.add(0x134) as *const f32;
        Some([*o, *o.add(1), *o.add(2)])
    }
}
/// Origin: the scene's origin in the game's world, when it is the world's scene and the origin is finite.
///
/// # Safety
/// `scene` a live PxScene.
pub unsafe fn origin(scene: P) -> Option<[f32; 3]> {
    // SAFETY: the caller's scene.
    let o = unsafe { origin_of(scene) }?;
    (o[0] + o[1] + o[2]).is_finite().then_some(o)
}

// ---- typed calls, each through a proved slot ----
// SAFETY (all below): the object a live PhysX object of the kind the slot belongs to, after prove().
pub unsafe fn actor_type(a: P) -> i32 {
    unsafe { at::<unsafe extern "C" fn(P) -> i32>(a, GET_TYPE)(a) }
}
pub unsafe fn world_bounds(a: P) -> PxBounds {
    let mut b = PxBounds::default();
    unsafe { at::<unsafe extern "C" fn(P, *mut PxBounds, f32) -> P>(a, GET_WORLD_BOUNDS)(a, &mut b, 1.0) };
    b
}
pub unsafe fn rigid_body_flags(a: P) -> u8 {
    let mut f = 0u8;
    unsafe { at::<unsafe extern "C" fn(P, *mut u8) -> P>(a, GET_RIGID_BODY_FLAGS)(a, &mut f) };
    f
}
pub unsafe fn mass(a: P) -> f32 {
    unsafe { at::<unsafe extern "C" fn(P) -> f32>(a, GET_MASS)(a) }
}
pub unsafe fn shapes(a: P, out: &mut [P]) -> u32 {
    unsafe { at::<unsafe extern "C" fn(P, *mut P, u32, u32) -> u32>(a, GET_SHAPES)(a, out.as_mut_ptr(), out.len() as u32, 0) }
}
pub unsafe fn sim_filter(s: P) -> [u32; 4] {
    let mut w = [0u32; 4];
    unsafe { at::<unsafe extern "C" fn(P, *mut u32) -> P>(s, SHAPE_GET_SIM_FILTER)(s, w.as_mut_ptr()) };
    w
}
pub unsafe fn set_sim_filter(s: P, w: &[u32; 4]) {
    unsafe { at::<unsafe extern "C" fn(P, *const u32)>(s, SHAPE_SET_SIM_FILTER)(s, w.as_ptr()) };
}
pub unsafe fn geometry_type(s: P) -> i32 {
    unsafe { at::<unsafe extern "C" fn(P) -> i32>(s, SHAPE_GET_GEOMETRY_TYPE)(s) }
}
pub unsafe fn capsule(s: P) -> Option<CapsuleGeometry> {
    let mut c = CapsuleGeometry::default();
    unsafe { at::<unsafe extern "C" fn(P, *mut CapsuleGeometry) -> bool>(s, SHAPE_GET_CAPSULE)(s, &mut c) }.then_some(c)
}
pub unsafe fn box_geometry(s: P) -> Option<BoxGeometry> {
    let mut b = BoxGeometry::default();
    unsafe { at::<unsafe extern "C" fn(P, *mut BoxGeometry) -> bool>(s, SHAPE_GET_BOX)(s, &mut b) }.then_some(b)
}
pub unsafe fn sphere(s: P) -> Option<SphereGeometry> {
    let mut b = SphereGeometry::default();
    unsafe { at::<unsafe extern "C" fn(P, *mut SphereGeometry) -> bool>(s, SHAPE_GET_SPHERE)(s, &mut b) }.then_some(b)
}
pub unsafe fn scene_actors(sc: P, types: u16, out: &mut [P], start: u32) -> u32 {
    unsafe { at::<unsafe extern "C" fn(P, *const u16, *mut P, u32, u32) -> u32>(sc, SCENE_GET_ACTORS)(sc, &types, out.as_mut_ptr(), out.len() as u32, start) }
}
pub unsafe fn scene_aggregates(sc: P, out: &mut [P], start: u32) -> u32 {
    unsafe { at::<unsafe extern "C" fn(P, *mut P, u32, u32) -> u32>(sc, SCENE_GET_AGGREGATES)(sc, out.as_mut_ptr(), out.len() as u32, start) }
}
pub unsafe fn aggregate_actors(ag: P, out: &mut [P]) -> u32 {
    unsafe { at::<unsafe extern "C" fn(P, *mut P, u32, u32) -> u32>(ag, AGGREGATE_GET_ACTORS)(ag, out.as_mut_ptr(), out.len() as u32, 0) }
}
pub unsafe fn reset_filtering(sc: P, a: P) {
    unsafe { at::<unsafe extern "C" fn(P, P)>(sc, SCENE_RESET_FILTERING)(sc, a) };
}
