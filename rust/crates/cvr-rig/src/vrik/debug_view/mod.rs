//! DebugView (main's src/Natives/DebugView.cpp) -- what the hands and the physics actually see, drawn in the world, and
//! the scene walks the hands stand on.
//!
//! THE SHAPES COME FROM PHYSX ITSELF. The game keeps them out of reach: its body interface answers dimensions and no
//! shape, and static collision is compiled into sector data. But it links PhysX 3.4 from PhysX3_x64.dll, which exports
//! PxGetPhysics, and from there everything is the public API: the scenes, an overlap query, and each hit's actor, shape
//! and geometry, for the world as much as for loose props. BY TABLE SLOT, because the port carries no PhysX headers; a
//! slot that has a setter beside it is checked against the function's address in this very DLL before the first call
//! (counted from the headers they were one off once, and the view wrote a zero pose over live collision shapes).
//!
//! PHYSX'S SPACE IS NOT THE WORLD'S: every query goes in less the scene owner's vector at +0x134, every shape comes out
//! plus it.
//!
//! Here: a body's actor found in the scenes (rigid_dynamic_of, actor_of), its spin ceiling lifted (raise_spin), and the
//! fates of bodies of broken things (body_gone, note_actor, note_gone); pick.rs the near and cone pickers and the
//! proxies' kinds; view.rs the collision view's lines; gun.rs the gun's boxes against the world; natives.rs the VRDebug*
//! and VRGun* natives. Every call into PhysX or the game is guarded, as main's __try had them.
pub mod gun;
pub mod natives;
pub mod pick;
pub mod view;

use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering::Relaxed};
use cvr_engine::code::{guarded_call4, guarded_call4x, guarded_call6, read, read_bytes};
use cvr_host::log;
use std::sync::Mutex;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

pub(crate) type P = *mut c_void;
const PHYSICS_GET_NB_SCENES: usize = 19;
const PHYSICS_GET_SCENES: usize = 20;
const SCENE_GET_PHYSICS: usize = 6;
const SCENE_GET_ACTORS: usize = 19;
const SCENE_GET_NB_AGGREGATES: usize = 26;
const SCENE_GET_AGGREGATES: usize = 27;
const AGGREGATE_GET_ACTORS: usize = 11;
const MAX_SCENES: usize = 32;
const MAX_ACTORS: usize = 40_000;
const RIGID_DYNAMIC_TABLE: usize = 0x20B2E8;
const OWNER_INSTANCED_DESTRUCTIBLE: u32 = 6;

/// `object`'s function at `slot`, called with up to five more pointer-sized arguments inside a guarded range: rax, or
/// None on a fault.
pub(super) unsafe fn slot_call(object: P, slot: usize, args: [usize; 5]) -> Option<usize> {
    let table = read::<usize>(object as usize)?;
    let f = read::<usize>(table + 8 * slot)?;
    guarded_call6(f, [object, args[0] as P, args[1] as P, args[2] as P, args[3] as P, args[4] as P]).map(|r| r as usize)
}
pub(super) fn physx_dll() -> usize {
    // SAFETY: a module handle by name.
    unsafe { GetModuleHandleA(c"PhysX3_x64.dll".as_ptr().cast()) as usize }
}

pub(super) struct Scenes {
    pub(super) scenes: [P; MAX_SCENES],
    /// the game's object for each, when it is proved to be
    pub(super) owners: [P; MAX_SCENES],
    pub(super) count: usize,
}
// SAFETY: plain addresses, used under the lock.
unsafe impl Send for Scenes {}
/// THE GAME MAKES AND DROPS SCENES, so the list is asked for each time. A pointer kept from an earlier call was a call
/// through freed memory.
pub(super) unsafe fn read_scenes() -> Option<Scenes> {
    let dll = physx_dll();
    if dll == 0 {
        return None;
    }
    let get_physics = GetProcAddress(dll as _, c"PxGetPhysics".as_ptr().cast())?;
    let physics = guarded_call4(get_physics as usize, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())?;
    if physics.is_null() {
        log!("[debugview] no PxGetPhysics\n");
        return None;
    }
    let count = slot_call(physics, PHYSICS_GET_NB_SCENES, [0; 5])? as u32;
    if count > 64 {
        log!("[debugview] {} scenes: not believed\n", count);
        return None;
    }
    let mut found = [core::ptr::null_mut::<c_void>(); MAX_SCENES];
    let got = if count != 0 { slot_call(physics, PHYSICS_GET_SCENES, [found.as_mut_ptr() as usize, MAX_SCENES, 0, 0, 0])? as u32 } else { 0 };
    let mut out = Scenes { scenes: [core::ptr::null_mut(); MAX_SCENES], owners: [core::ptr::null_mut(); MAX_SCENES], count: 0 };
    for (i, &scene) in found.iter().take((got as usize).min(MAX_SCENES)).enumerate() {
        if scene.is_null() || slot_call(scene, SCENE_GET_PHYSICS, [0; 5])? as P != physics {
            log!("[debugview] scene {} does not know its PhysX: the table is not what was counted\n", i);
            return None;
        }
        let owner = read::<usize>(scene as usize + 8)? as P;
        let is_owner = !owner.is_null() && read::<usize>(owner as usize + 0x18) == Some(scene as usize);
        out.owners[out.count] = if is_owner { owner } else { core::ptr::null_mut() };
        out.scenes[out.count] = scene;
        out.count += 1;
    }
    Some(out)
}
pub(super) unsafe fn find_actor_in(list: &[P], proxy: u32, body: u32) -> Option<P> {
    list.iter().copied().find(|&a| read::<u32>(a as usize + 0x10) == Some(proxy) && read::<u16>(a as usize + 0x14) == Some(body as u16))
}
static mut ACTOR_BUFFER: [P; MAX_ACTORS] = [core::ptr::null_mut(); MAX_ACTORS];
static mut AGGREGATE_BUFFER: [P; MAX_ACTORS] = [core::ptr::null_mut(); MAX_ACTORS];
/// The buffers above are one walk's at a time (main's were the script thread's alone; here any thread may ask).
pub(super) static WALK: Mutex<()> = Mutex::new(());
/// 0 not yet, 1 yes, -1 no: the scene list was once not what was counted (the collision view's query sets it).
pub(super) static PROVED: AtomicI32 = AtomicI32::new(0);
/// The actor (proxy, body) among the scene's aggregates' members.
pub(super) unsafe fn find_in_aggregates(sc: P, proxy: u32, body: u32) -> Option<Option<P>> {
    let aggregates = slot_call(sc, SCENE_GET_NB_AGGREGATES, [0; 5])? as u32;
    let got = if aggregates as usize <= MAX_ACTORS { slot_call(sc, SCENE_GET_AGGREGATES, [(&raw mut AGGREGATE_BUFFER) as usize, MAX_ACTORS, 0, 0, 0])? as u32 } else { 0 };
    for i in 0..(got as usize).min(MAX_ACTORS) {
        let mut members = [core::ptr::null_mut::<c_void>(); 64];
        let n = slot_call(AGGREGATE_BUFFER[i], AGGREGATE_GET_ACTORS, [members.as_mut_ptr() as usize, 64, 0, 0, 0])? as u32;
        if let Some(a) = find_actor_in(&members[..(n as usize).min(64)], proxy, body) {
            return Some(Some(a));
        }
    }
    Some(None)
}
/// The actor (proxy, body) among the scene's own actors of `types` (PxActorTypeFlags: 1 static, 2 dynamic; by address,
/// in this ABI).
pub(super) unsafe fn find_in_actors(sc: P, types: u16, proxy: u32, body: u32) -> Option<Option<P>> {
    let got = slot_call(sc, SCENE_GET_ACTORS, [(&types as *const u16) as usize, (&raw mut ACTOR_BUFFER) as usize, MAX_ACTORS, 0, 0])? as u32;
    Some(find_actor_in(&(&*(&raw const ACTOR_BUFFER))[..(got as usize).min(MAX_ACTORS)], proxy, body))
}
/// A body's actor and the index of its scene, aggregates first, then the scene's actors (dynamic and static).
unsafe fn find_body(scenes: &Scenes, proxy: u32, body: u32) -> Option<(P, usize)> {
    for s in 0..scenes.count {
        let sc = scenes.scenes[s];
        let mut found = find_in_aggregates(sc, proxy, body)?;
        if found.is_none() {
            found = find_in_actors(sc, 1 | 2, proxy, body)?;
        }
        if let Some(a) = found {
            return Some((a, s));
        }
    }
    None
}
/// RigidDynamicOf: the PhysX actor of a body, when it is a PxRigidDynamic of the PhysX this was read from, with its
/// scene and where that scene's origin is in the game's world (PhysX's poses are relative to it). None if not.
pub unsafe fn rigid_dynamic_of(proxy: u32, body: u32) -> Option<(P, P, [f32; 3])> {
    if PROVED.load(Relaxed) < 0 {
        return None;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    let scenes = read_scenes()?;
    let (a, s) = find_body(&scenes, proxy, body)?;
    // Only a PxRigidDynamic of this PhysX, and only in a scene whose origin is known: its poses are relative to it.
    if read::<usize>(a as usize)?.wrapping_sub(physx_dll()) != RIGID_DYNAMIC_TABLE || scenes.owners[s].is_null() {
        return None;
    }
    let o = scenes.owners[s] as usize + 0x134;
    Some((a, scenes.scenes[s], [read::<f32>(o)?, read::<f32>(o + 4)?, read::<f32>(o + 8)?]))
}
/// RaiseSpin: lifts PhysX's ceiling on how fast a body may turn, which this body was held to: (raised, the old
/// ceiling). Not raised if it is not a body of the PhysX this was read from.
pub fn raise_spin(proxy: u32, body: u32, most: f32) -> (bool, f32) {
    // PxRigidDynamic's table and the two functions, read in this DLL (2026-11-04): the setter squares its argument, the
    // getter takes the root.
    const SET_RVA: usize = 0x4BE90;
    const GET_RVA: usize = 0x4BF20;
    const SET_SLOT: usize = 60;
    const GET_SLOT: usize = 61;
    if PROVED.load(Relaxed) < 0 {
        return (false, 0.0);
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: guarded calls into PhysX's lists and its actor.
    unsafe {
        let Some(scenes) = read_scenes() else { return (false, 0.0) };
        let Some((a, _)) = find_body(&scenes, proxy, body) else { return (false, 0.0) };
        let dll = physx_dll();
        let Some(table) = read::<usize>(a as usize) else { return (false, 0.0) };
        let (Some(set), Some(get)) = (read::<usize>(table + 8 * SET_SLOT), read::<usize>(table + 8 * GET_SLOT)) else { return (false, 0.0) };
        if table.wrapping_sub(dll) != RIGID_DYNAMIC_TABLE || set.wrapping_sub(dll) != SET_RVA || get.wrapping_sub(dll) != GET_RVA {
            return (false, 0.0);
        }
        let Some((_, was)) = guarded_call4x(get, [a as u64, 0, 0, 0]) else { return (false, 0.0) };
        let was = f32::from_bits(was as u32);
        if guarded_call4x(set, [a as u64, u64::from(most.to_bits()), 0, 0]).is_none() {
            return (false, was);
        }
        (true, was)
    }
}
/// ActorOf: PhysX's actor of a body (a test tool's look at its shapes), or None if it has none. Only the game's world's
/// scenes; dynamic actors first, then the aggregates.
pub fn actor_of(proxy: u32, body: u32) -> Option<P> {
    if PROVED.load(Relaxed) < 0 {
        return None;
    }
    let _walk = WALK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: guarded calls into PhysX's lists.
    unsafe {
        let scenes = read_scenes()?;
        for s in 0..scenes.count {
            if scenes.owners[s].is_null() {
                continue;
            }
            let sc = scenes.scenes[s];
            if let Some(a) = find_in_actors(sc, 2, proxy, body)? {
                return Some(a);
            }
            if let Some(a) = find_in_aggregates(sc, proxy, body)? {
                return Some(a);
            }
        }
    }
    None
}

// ---- shared by the walks: PhysX 3.4 by slot
pub(super) type Vec3 = [f32; 3];
/// PxTransform: the rotation x y z w, then the position.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Transform {
    pub(super) q: [f32; 4],
    pub(super) p: Vec3,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Bounds {
    pub(super) lo: Vec3,
    pub(super) hi: Vec3,
}
pub(super) const ACTOR_GET_TYPE: usize = 5;
pub(super) const ACTOR_GET_WORLD_BOUNDS: usize = 9;
pub(super) const ACTOR_GET_GLOBAL_POSE: usize = 20;
pub(super) const ACTOR_GET_SHAPES: usize = 26;
pub(super) const SCENE_OVERLAP: usize = 96;
pub(super) fn rotate(q: &[f32; 4], v: &Vec3) -> Vec3 {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + q[1] * tz - q[2] * ty, v[1] + q[3] * ty + q[2] * tx - q[0] * tz, v[2] + q[3] * tz + q[0] * ty - q[1] * tx]
}
pub(super) fn apply(t: &Transform, v: &Vec3) -> Vec3 {
    let r = rotate(&t.q, v);
    [r[0] + t.p[0], r[1] + t.p[1], r[2] + t.p[2]]
}
pub(super) fn multiply(a: &[f32; 4], b: &[f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub(super) unsafe fn actor_type(a: P) -> Option<i32> {
    slot_call(a, ACTOR_GET_TYPE, [0; 5]).map(|r| r as i32)
}
pub(super) unsafe fn global_pose(a: P) -> Option<Transform> {
    let mut t = Transform::default();
    slot_call(a, ACTOR_GET_GLOBAL_POSE, [(&raw mut t) as usize, 0, 0, 0, 0])?;
    Some(t)
}
/// getWorldBounds(inflation 1.01), returned through a hidden pointer, the float in its own register.
pub(super) unsafe fn world_bounds(a: P) -> Option<Bounds> {
    let table = read::<usize>(a as usize)?;
    let f = read::<usize>(table + 8 * ACTOR_GET_WORLD_BOUNDS)?;
    let mut b = Bounds::default();
    guarded_call4x(f, [a as u64, (&raw mut b) as u64, u64::from(1.01f32.to_bits()), 0])?;
    Some(b)
}
pub(super) unsafe fn actor_shapes(a: P, out: &mut [P], start: u32) -> Option<usize> {
    slot_call(a, ACTOR_GET_SHAPES, [out.as_mut_ptr() as usize, out.len(), start as usize, 0, 0]).map(|n| (n as u32 as usize).min(out.len()))
}
/// The scene's own actors of `types` (1 static, 2 dynamic) in the shared buffer (WALK held), at most `most`, from
/// `start`.
pub(super) unsafe fn scene_actors(sc: P, types: u16, most: usize, start: u32) -> Option<&'static [P]> {
    let most = most.min(MAX_ACTORS);
    let got = slot_call(sc, SCENE_GET_ACTORS, [(&types as *const u16) as usize, (&raw mut ACTOR_BUFFER) as usize, most, start as usize, 0])? as u32;
    Some(&(&*(&raw const ACTOR_BUFFER))[..(got as usize).min(most)])
}
/// The scene's aggregates in the shared buffer (WALK held); none past MAX_ACTORS of them.
pub(super) unsafe fn scene_aggregates(sc: P) -> Option<&'static [P]> {
    let aggregates = slot_call(sc, SCENE_GET_NB_AGGREGATES, [0; 5])? as u32;
    let got = if aggregates as usize <= MAX_ACTORS { slot_call(sc, SCENE_GET_AGGREGATES, [(&raw mut AGGREGATE_BUFFER) as usize, MAX_ACTORS, 0, 0, 0])? as u32 } else { 0 };
    Some(&(&*(&raw const AGGREGATE_BUFFER))[..(got as usize).min(MAX_ACTORS)])
}
/// An aggregate's first 64 members.
pub(super) unsafe fn aggregate_members(ag: P) -> Option<([P; 64], usize)> {
    let mut members = [core::ptr::null_mut::<c_void>(); 64];
    let n = slot_call(ag, AGGREGATE_GET_ACTORS, [members.as_mut_ptr() as usize, 64, 0, 0, 0])? as u32;
    Some((members, (n as usize).min(64)))
}
/// Where a scene's owner has the scene's origin in the game's world.
pub(super) fn owner_origin(owner: P) -> Option<Vec3> {
    let o = owner as usize + 0x134;
    Some([read::<f32>(o)?, read::<f32>(o + 4)?, read::<f32>(o + 8)?])
}
/// The (proxy, body) the game wrote in an actor's userData.
pub(super) fn ids_of(a: P) -> (u32, u32) {
    (read::<u32>(a as usize + 0x10).unwrap_or(0), u32::from(read::<u16>(a as usize + 0x14).unwrap_or(0)))
}

/// ProxyOwner: the owner kind of a physics proxy (the game's proxy system, Cyberpunk2077.exe 2.5: the system at
/// 0x3BFCDD0, its lookup 0x27D430 and the handle's release 0x27D374, the lookup's prologue checked first); 0 when there is
/// none or a call faults.
pub(super) unsafe fn proxy_owner(proxy: u32) -> u32 {
    const SYSTEM_RVA: usize = 0x3BF_CDD0;
    const LOOKUP_RVA: usize = 0x27_D430;
    const RELEASE_RVA: usize = 0x27_D374;
    const PROLOGUE: [u8; 14] = [0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x20, 0x55, 0x57, 0x41, 0x56];
    let exe = cvr_engine::code::image_base();
    let mut head = [0u8; 14];
    if !read_bytes(exe + LOOKUP_RVA, &mut head) || head != PROLOGUE {
        return 0;
    }
    let Some(system) = read::<usize>(exe + SYSTEM_RVA).filter(|s| *s != 0) else { return 0 };
    let mut handle = [0usize; 2];
    if guarded_call4(exe + LOOKUP_RVA, system as P, handle.as_mut_ptr().cast(), proxy as usize as P, core::ptr::null_mut()).is_none() {
        return 0;
    }
    // a live object: the handle holds it (no VirtualQuery, see actor_is)
    let owner = if handle[0] != 0 { read::<u8>(handle[0] + 0x35).map_or(0, u32::from) } else { 0 };
    guarded_call4(exe + RELEASE_RVA, handle.as_mut_ptr().cast(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
    owner
}

/// BODIES OF BROKEN THINGS. A plate, a bowl, a box of an InstancedDestructibleNode breaks, and the game releases its PhysX
/// actor; but a body interface for it still reaches that actor, freed. Read, it answers rubbish; told a velocity, it
/// WRITES into the freed actor, and the scene's query structure took it back (2026-11-08 21:21, PhysX3CharacterKinematic
/// +0xA592). So every call on a body interface first asks this. Only destructible owners are asked about: a body is
/// looked up once (its actor kept and checked each time after: the table still PxRigidDynamic's, the userData still this
/// body), one not found is gone for good, and a body of any other owner is never gone.
#[derive(Clone, Copy)]
struct Fate {
    proxy: u32,
    body: u32,
    actor: usize,
    /// 1 alive (actor), 2 gone, 3 not destructible
    state: u8,
}
const FATES: usize = 512;
struct Fates {
    f: [Fate; FATES],
    next: usize,
}
static FATE: Mutex<Fates> = Mutex::new(Fates { f: [Fate { proxy: 0, body: 0, actor: 0, state: 0 }; FATES], next: 0 });
impl Fates {
    fn of(&mut self, proxy: u32, body: u32) -> Option<usize> {
        self.f.iter().position(|f| f.state != 0 && f.proxy == proxy && f.body == body)
    }
    fn new_fate(&mut self, proxy: u32, body: u32) -> usize {
        let i = self.next;
        self.next = (self.next + 1) % FATES;
        self.f[i] = Fate { proxy, body, actor: 0, state: 0 };
        i
    }
}
/// ActorIs: NO VirtualQuery HERE (every call on a destructible's body cost 7 to 12 ms with it, carrying a plate took the
/// frame to 10 FPS). The actor was found in PhysX's own lists, so its memory is PhysX's pool, which keeps its slabs: the
/// words can be read.
fn actor_is(actor: usize, proxy: u32, body: u32) -> bool {
    let dll = physx_dll();
    actor != 0 && dll != 0 && read::<usize>(actor) == Some(dll + RIGID_DYNAMIC_TABLE) && read::<u32>(actor + 0x10) == Some(proxy) && read::<u16>(actor + 0x14) == Some(body as u16)
}
/// BodyGone: true once a body of a destructible thing has broken and its actor is gone: nothing may call its body
/// interface after. Any other owner's body is never gone. Any thread.
pub fn body_gone(proxy: u32, body: u32) -> bool {
    if proxy == 0 {
        return false;
    }
    {
        let mut g = FATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = g.of(proxy, body) {
            let f = &mut g.f[i];
            let mut gone = f.state == 2;
            if f.state == 1 && !actor_is(f.actor, proxy, body) {
                f.state = 2;
                gone = true;
                log!("[bodies] proxy {} body {}: its actor {:#x} was released (broken); nothing touches it from now on\n", proxy, body, f.actor);
            }
            return gone;
        }
    }
    // First sight: whose it is, and for a destructible its actor. Outside the lock: the walk over the scenes is the slow
    // part.
    // SAFETY: guarded engine and PhysX calls.
    let owner = unsafe { proxy_owner(proxy) };
    let actor = if owner == OWNER_INSTANCED_DESTRUCTIBLE {
        // SAFETY: as above.
        unsafe { rigid_dynamic_of(proxy, body) }.map_or(0, |(a, _, _)| a as usize)
    } else {
        0
    };
    let mut g = FATE.lock().unwrap_or_else(|e| e.into_inner());
    let i = match g.of(proxy, body) {
        Some(i) => i,
        None => {
            let i = g.new_fate(proxy, body);
            let f = &mut g.f[i];
            f.state = if owner != OWNER_INSTANCED_DESTRUCTIBLE {
                3
            } else if actor != 0 {
                1
            } else {
                2
            };
            f.actor = actor;
            if f.state == 2 {
                log!("[bodies] proxy {} body {}: destructible, and no actor of it is left (broken); nothing touches it\n", proxy, body);
            }
            i
        }
    };
    g.f[i].state == 2
}
/// NoteActor: a body whose actor is known (HeldBody, as it takes hold): body_gone checks that actor on every call from
/// now on, whatever the body's owner. A stack of street cups is an aggregate's, not a destructible's, so its actor was
/// never watched: it broke in the hand (2026-11-10 16:27). Any thread.
pub fn note_actor(proxy: u32, body: u32, actor: usize) {
    if proxy == 0 || actor == 0 {
        return;
    }
    let mut g = FATE.lock().unwrap_or_else(|e| e.into_inner());
    let i = g.of(proxy, body).unwrap_or_else(|| g.new_fate(proxy, body));
    let f = &mut g.f[i];
    if f.state != 2 {
        f.state = 1;
        f.actor = actor;
    }
}
/// NoteGone: told by whoever saw its actor die (HeldBody).
pub fn note_gone(proxy: u32, body: u32) {
    let mut g = FATE.lock().unwrap_or_else(|e| e.into_inner());
    let i = g.of(proxy, body).unwrap_or_else(|| g.new_fate(proxy, body));
    g.f[i].state = 2;
    g.f[i].actor = 0;
}
