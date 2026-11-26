//! PEOPLE: HandCollision reads a living person's bones (their slot component's slots) and hands over a capsule for each
//! part every frame; the physics thread poses a kinematic capsule of its own for each -- and V's own body's after them
//! (self_capsules), made and posed the same way.
//!
//! THE PEOPLE NEAR THE HANDS, posed each step (physics thread). A kinematic capsule for each of HandCollision's, in the
//! hands' own group and meeting that alone: the hands (palm, fingers, forearm, a held thing) stop on them and the
//! fingers lie on them; no ray, sweep or character controller sees them (a simulation shape only), and the person is not
//! pushed -- they are the game's to move, these only follow their bones. A living NPC has no simulated shape of its own:
//! their controller's capsules are query shapes (flags 0a, filter 0), so the hands went through people (2026-11-09).
//! V'S OWN BODY after them, the same way: in the bits of the hands that meet each part (self_capsules) and meeting
//! nothing, so a palm or a finger whose mask has that bit stops on it and nothing else does (not the world, not the
//! forearms).
use super::body::{hand_filter, material, refilter, scene, self_on};
use super::census::{bit_person_left, bit_person_right, bit_self_left, bit_self_right, filter_ready, self_bits_ready};
use super::grab::{held_near_hands, read_person_attachment};
use super::math::{qnorm, qrot};
use super::near::{world_bounds_fn, NearShape};
use super::pose::{self_capsules, step_pose_inputs};
use super::px::{self, at, CapsuleGeometry, PxTransform, P};
use super::seq::Seq;
use super::settings::Settings;
use super::tick_ms;
use super::types::{SelfCap, SelfShown, MOST_SELF};
use core::sync::atomic::{AtomicI32, Ordering::*};
use cvr_host::log;
use cvr_physhands::handspace::Reference;
use cvr_physhands::person::filter_person_hands;
use cvr_settings::lc;

pub const MOST_PEOPLE: usize = 96;
/// ...and V's own body's after them
pub const MOST_BODIES: usize = MOST_PEOPLE + MOST_SELF;
/// a shape's owner: V; 0 a person the script did not name; > 0 one part of a person (the script's id: person * 32 + part)
pub const SELF_OWNER: i32 = -1;
#[derive(Clone, Copy, Default)]
pub struct Person {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub r: f32,
    pub owner: i32,
}
#[derive(Clone, Copy)]
pub struct People {
    pub c: [Person; MOST_PEOPLE],
    pub n: i32,
    pub at: u64,
}
const PEOPLE0: People = People { c: [Person { a: [0.0; 3], b: [0.0; 3], r: 0.0, owner: 0 }; MOST_PEOPLE], n: 0, at: 0 };
/// script thread
pub static mut people_staged: People = PEOPLE0;
pub static PEOPLE: Seq<People> = Seq::new(PEOPLE0);
pub static mut person_actor: [P; MOST_BODIES] = [core::ptr::null_mut(); MOST_BODIES];
pub static mut person_shape: [P; MOST_BODIES] = [core::ptr::null_mut(); MOST_BODIES];
pub static mut person_geom: [[f32; 2]; MOST_BODIES] = [[0.0; 2]; MOST_BODIES];
pub static mut person_filter: [[u32; 4]; MOST_BODIES] = [[0; 4]; MOST_BODIES];
pub static mut person_live: [bool; MOST_BODIES] = [false; MOST_BODIES];
/// Ours, not the game's: a census taken again while the hands live must not count them (their group is the hands').
pub unsafe fn own_body(a: P) -> bool {
    person_actor.iter().any(|&p| !p.is_null() && p == a)
}
/// As posed this step, PhysX's space: each one's owner and the hands that meet it (bits: 1 the left, 2 the right).
#[derive(Clone, Copy)]
pub struct PeopleNear {
    pub s: [NearShape; MOST_BODIES],
    pub owner: [i32; MOST_BODIES],
    pub meets: [u8; MOST_BODIES],
    pub simulates: [u8; MOST_BODIES],
    pub n: usize,
    pub at: u64,
    pub origin: [f32; 3],
    pub self_frame: Reference,
}
const PEOPLE_NEAR0: PeopleNear = PeopleNear {
    s: [NearShape::ZERO; MOST_BODIES],
    owner: [0; MOST_BODIES],
    meets: [0; MOST_BODIES],
    simulates: [0; MOST_BODIES],
    n: 0,
    at: 0,
    origin: [0.0; 3],
    self_frame: Reference {
        position: cvr_physhands::handspace::Vector { x: 0.0, y: 0.0, z: 0.0 },
        rotation: cvr_physhands::handspace::Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
        owner: 0,
        origin: 0,
        valid: false,
    },
};
/// physics thread
pub static mut person_near: [NearShape; MOST_BODIES] = [NearShape::ZERO; MOST_BODIES];
pub static mut person_owner: [i32; MOST_BODIES] = [0; MOST_BODIES];
pub static mut person_meets: [u8; MOST_BODIES] = [0; MOST_BODIES];
pub static mut person_simulates: [u8; MOST_BODIES] = [0; MOST_BODIES];
pub static mut person_near_n: usize = 0;
/// PersonMeets: whether a hand meets one of them now -- a person always, V's own body while that hand meets it.
pub unsafe fn person_meets_hand(i: usize, side: usize) -> bool {
    (person_meets[i] >> side) & 1 != 0 && (person_owner[i] != SELF_OWNER || self_on[side])
}
const SHOWN0: SelfShown = SelfShown { c: [SelfCap { a: [0.0; 3], b: [0.0; 3], r: 0.0, meets: 0 }; MOST_SELF], n: 0, at: 0 };
/// V's own body as last posed, the game's world, for the script's view
pub static SELF_SHOWN: Seq<SelfShown> = Seq::new(SHOWN0);
/// the same, for the worker's fingers
pub static PEOPLE_NEAR: Seq<PeopleNear> = Seq::new(PEOPLE_NEAR0);

static mut SHOWN: SelfShown = SHOWN0;
static mut SNAP: PeopleNear = PEOPLE_NEAR0;
static MADE_SAID: AtomicI32 = AtomicI32::new(0);

unsafe fn park_person(i: usize) {
    if !person_actor[i].is_null() && person_live[i] {
        at::<unsafe extern "C" fn(P, i32, bool)>(person_actor[i], px::SET_ACTOR_FLAG)(person_actor[i], px::DISABLE_SIMULATION, true);
        person_live[i] = false;
    }
}

/// PeopleStep: each person's capsule and V's own body's posed this step (made the first time), the rest parked; the
/// snapshot for the worker.
pub unsafe fn people_step(origin: &[f32; 3], s: &Settings) {
    let enabled = lc!(xrHandPhysics) != 0 && lc!(xrHandCollision) != 0;
    let p = if enabled && s.people != 0 && filter_ready && !material.is_null() && !scene.is_null() {
        PEOPLE.read().filter(|p| tick_ms().wrapping_sub(p.at) < 300)
    } else {
        None
    };
    let n = p.map_or(0, |p| p.n.clamp(0, MOST_PEOPLE as i32) as usize);
    let shown = &mut SHOWN;
    shown.n = if enabled && s.self_ != 0 && self_bits_ready && filter_ready && !material.is_null() && !scene.is_null() { self_capsules(&mut shown.c) } else { 0 };
    shown.at = tick_ms();
    SELF_SHOWN.write(shown);
    let ns = shown.n as usize;
    let snap = &mut SNAP;
    let mut m = 0usize;
    let physics = px::physics();
    for i in 0..MOST_BODIES {
        let own = i >= MOST_PEOPLE;
        let j = i.wrapping_sub(MOST_PEOPLE);
        if if own { j >= ns } else { i >= n } {
            park_person(i);
            continue;
        }
        let (ca, cb, mut cr, owner, mut meets) = if own {
            (shown.c[j].a, shown.c[j].b, shown.c[j].r, SELF_OWNER, shown.c[j].meets)
        } else {
            let c = p.as_ref().map_or(Default::default(), |p| p.c[i]);
            (c.a, c.b, c.r, c.owner, 3u8)
        };
        let mut filter = [0u32; 4];
        if own {
            let g = (if meets & 1 != 0 { bit_self_left } else { 0 }) | (if meets & 2 != 0 { bit_self_right } else { 0 });
            filter = [g as u32, (g >> 32) as u32, 0, 0];
        }
        let mut attached = None;
        if !own && owner > 0 {
            for side in 0..2 {
                if let Some(v) = read_person_attachment(side).filter(|v| v.on && v.owner == owner) {
                    attached = Some(v);
                    break;
                }
            }
        }
        let mut axis = [0.0f32; 3];
        if let Some(at_) = &attached {
            axis = qrot(&at_.capsule_rotation, &[1.0, 0.0, 0.0]);
            cr = at_.radius;
        }
        let mut a = [0.0f32; 3];
        let mut b = [0.0f32; 3];
        let mut d = [0.0f32; 3];
        let mut len = 0.0f32;
        for k in 0..3 {
            a[k] = attached.as_ref().map_or(ca[k], |v| v.capsule[k] - axis[k] * v.half) - origin[k];
            b[k] = attached.as_ref().map_or(cb[k], |v| v.capsule[k] + axis[k] * v.half) - origin[k];
            d[k] = b[k] - a[k];
            len += d[k] * d[k];
        }
        len = len.sqrt();
        if !(len + a[0] + a[1] + a[2]).is_finite() || !(cr > 0.005 && cr < 0.5) || len > 2.0 {
            park_person(i);
            continue;
        }
        // Suppress only the hand already attached here, not the other hand. Every penetration query consumes the same
        // effective pair eligibility.
        if !own {
            let held = if owner > 0 { held_near_hands(owner, &a, &b, cr) } else { 0 };
            let common = u64::from(hand_filter[0]) | (u64::from(hand_filter[1]) << 32);
            let contact = filter_person_hands(common, bit_person_left, bit_person_right, 3 & !held);
            filter = contact.words;
            meets = contact.hands;
        }
        let mut pose = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])] };
        if let Some(v) = &attached {
            pose.q = v.capsule_rotation;
        } else if len > 1e-4 {
            // the capsule's axis is its x: the turn taking x to the line between its ends
            let u = [d[0] / len, d[1] / len, d[2] / len];
            let w = 1.0 + u[0];
            if w > 1e-6 {
                let mut q = [0.0, -u[2], u[1], w];
                if qnorm(&mut q) {
                    pose.q = q;
                }
            } else {
                pose.q = [0.0, 0.0, 1.0, 0.0]; // the other way: half a turn about z
            }
        }
        let half = (0.5 * len).max(0.001);
        let g = CapsuleGeometry { kind: px::GEOM_CAPSULE, radius: cr, half_height: half };
        let act = &mut person_actor[i];
        if act.is_null() {
            *act = at::<unsafe extern "C" fn(P, *const PxTransform) -> P>(physics, px::CREATE_RIGID_DYNAMIC)(physics, &pose);
            if act.is_null() {
                break;
            }
            at::<unsafe extern "C" fn(P, i32, bool)>(*act, px::SET_RIGID_BODY_FLAG)(*act, 1, true); // PxRigidBodyFlag::eKINEMATIC
            let mats: [P; 1] = [material];
            let flags: u8 = 1; // a simulation shape only
            let sh = at::<unsafe extern "C" fn(P, *const CapsuleGeometry, *const P, u16, bool, *const u8) -> P>(physics, px::CREATE_SHAPE)(physics, &g, mats.as_ptr(), 1, true, &flags);
            if sh.is_null() {
                at::<unsafe extern "C" fn(P)>(*act, px::RELEASE)(*act);
                *act = core::ptr::null_mut();
                break;
            }
            px::set_sim_filter(sh, &filter);
            at::<unsafe extern "C" fn(P, P) -> bool>(*act, px::ATTACH_SHAPE)(*act, sh);
            at::<unsafe extern "C" fn(P)>(sh, px::SHAPE_RELEASE)(sh);
            *((*act as *mut u8).add(0x10) as *mut usize) = !0usize; // no proxy of the game's
            at::<unsafe extern "C" fn(P, P, *const core::ffi::c_void)>(scene, px::ADD_ACTOR)(scene, *act, core::ptr::null());
            person_shape[i] = sh;
            person_geom[i] = [cr, half];
            person_filter[i] = filter;
            person_live[i] = true;
            let said = MADE_SAID.fetch_add(1, Relaxed);
            if said < 4 || (own && j == 0) {
                log!("[physhands] {} capsule made ({}): radius {:.3} m, {:.3} m long\n", if own { "V's own" } else { "a person's" }, i, cr, 2.0 * half);
            }
        } else {
            if (cr - person_geom[i][0]).abs() > 5e-4 || (half - person_geom[i][1]).abs() > 5e-4 {
                at::<unsafe extern "C" fn(P, *const CapsuleGeometry)>(person_shape[i], px::SHAPE_SET_GEOMETRY)(person_shape[i], &g);
                person_geom[i] = [cr, half];
            }
            if person_filter[i] != filter {
                px::set_sim_filter(person_shape[i], &filter);
                person_filter[i] = filter;
                refilter(*act);
            }
            if !person_live[i] {
                at::<unsafe extern "C" fn(P, i32, bool)>(*act, px::SET_ACTOR_FLAG)(*act, px::DISABLE_SIMULATION, false);
                person_live[i] = true;
            }
            at::<unsafe extern "C" fn(P, *const PxTransform, bool)>(*act, px::SET_GLOBAL_POSE)(*act, &pose, true);
        }
        let e = &mut snap.s[m];
        e.geometry.0 = [0; 64];
        core::ptr::copy_nonoverlapping((&g as *const CapsuleGeometry).cast::<u8>(), e.geometry.0.as_mut_ptr(), core::mem::size_of::<CapsuleGeometry>());
        e.pose = pose;
        e.bounds = world_bounds_fn().map_or(Default::default(), |f| f(e.geometry_ptr(), &e.pose, 1.0));
        e.kind = px::GEOM_CAPSULE;
        snap.owner[m] = owner;
        snap.meets[m] = meets;
        snap.simulates[m] = if filter.iter().any(|w| *w != 0) { meets } else { 0 };
        m += 1;
    }
    snap.n = m;
    snap.at = tick_ms();
    person_near[..m].copy_from_slice(&snap.s[..m]);
    person_owner[..m].copy_from_slice(&snap.owner[..m]);
    person_meets[..m].copy_from_slice(&snap.meets[..m]);
    person_simulates[..m].copy_from_slice(&snap.simulates[..m]);
    person_near_n = m;
    snap.origin = *origin;
    snap.self_frame = if step_pose_inputs.frame.tracking.valid { step_pose_inputs.frame.tracking } else { step_pose_inputs.frame.entity };
    PEOPLE_NEAR.write(snap);
}

/// SetPerson: capsule `index` of the people near the hands (the game's world), its radius and owner. Script thread.
pub unsafe fn set_person(index: usize, a: &[f32; 3], b: &[f32; 3], radius: f32, owner: i32) -> bool {
    if index >= MOST_PEOPLE || !(a[0] + a[1] + a[2] + b[0] + b[1] + b[2] + radius).is_finite() {
        return false;
    }
    let c = &mut people_staged.c[index];
    c.a = *a;
    c.b = *b;
    c.r = radius;
    c.owner = owner.max(0);
    true
}
/// SetPeople: the count of them, committed. Script thread.
pub unsafe fn set_people(count: i32) {
    people_staged.n = count.clamp(0, MOST_PEOPLE as i32);
    people_staged.at = tick_ms();
    PEOPLE.write(&people_staged);
}
/// SelfShape: V's own body as the hands meet it (the game's world): capsule `index` of them, its radius and the hands
/// that meet it (bits: 1 the left, 2 the right). None past the last. Any thread.
pub fn self_shape(index: usize) -> Option<([f32; 3], [f32; 3], f32, i32)> {
    let v = SELF_SHOWN.read()?;
    if tick_ms().wrapping_sub(v.at) > 300 || index >= v.n.max(0) as usize {
        return None;
    }
    let c = v.c[index];
    Some((c.a, c.b, c.r, i32::from(c.meets)))
}
/// SetSelfPad: how much V's clothes add round V's own trunk for the hands, m (the script, from what is worn).
pub fn set_self_pad(metres: f32) {
    if metres.is_finite() {
        super::pose::SELF_PAD.store(metres.clamp(0.0, 0.04).to_bits(), Relaxed);
    }
}
