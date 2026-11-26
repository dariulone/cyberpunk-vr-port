//! Making and steering the hands' bodies (physics thread): their shapes and filters, the palm's mass, the target
//! tracking gives, the player's own pace, the steering toward the target, the parking.
//!
//! THE PALM AND A GUN'S BOXES meet what the hand may touch (its filter); THE FINGERS AND THE FOREARM the world alone,
//! always (their filter is set when they are made). A finger capsule is put where the finger is drawn each step, not
//! moved there: closed in a stack of plates it appeared inside them, and PhysX threw them apart.
//! PHYSX 3.4 KEEPS A PAIR MADE UNDER THE OLD FILTER until it is asked to filter it again: a held plate set apart from
//! the stack it came out of went on striking it, a hand that closed on a thing went on pushing it. So every change is
//! followed by refilter, on the thread that steps the scene, between steps.
use super::census::{bit_left, bit_person_left, bit_person_right, bit_right, bit_self_left, bit_self_right, census, player_pos, player_valid, KIND_STATIC};
use super::math::{cross, qconj, qmul, qnorm, qrot};
use super::pose::step_target;
use super::px::{self, at, BoxGeometry, CapsuleGeometry, PxTransform, P};
use super::settings::Settings;
use super::touch::{touches_world, TOUCH_ALL, TOUCH_PEOPLE};
use super::types::MotorProbe;
use super::{qpc, tick_ms};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use cvr_host::log;
use cvr_physhands::hand_skin::skin_for;

// ---- physics thread only ----
pub static mut scene: P = core::ptr::null_mut();
pub static mut material: P = core::ptr::null_mut();
pub static mut actor: [P; 2] = [core::ptr::null_mut(); 2];
pub static mut shape: [P; 2] = [core::ptr::null_mut(); 2];
/// each finger's segments, capsules on the hand's body (the thumb's first is in the palm)
pub static mut finger_shape: [[[P; 3]; 5]; 2] = [[[core::ptr::null_mut(); 3]; 5]; 2];
pub static mut finger_half: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
pub static mut forearm_shape: [P; 2] = [core::ptr::null_mut(); 2];
pub static mut forearm_half: [f32; 2] = [0.0; 2];
/// the forearm tapers to the wrist: two capsules, the wrist's and the elbow's
pub static mut forearm_upper: [P; 2] = [core::ptr::null_mut(); 2];
pub static mut forearm_upper_half: [f32; 2] = [0.0; 2];
pub static mut forearm_radius: [[f32; 2]; 2] = [[0.0; 2]; 2];
pub static mut finger_radius: [[[f32; 3]; 5]; 2] = [[[0.0; 3]; 5]; 2];
/// A GUN'S BOXES, on the body of the hand that holds it (weapon_shapes' boxes, measured in that hand by the script):
/// parked small in the palm while there is none -- a gun's fitted boxes (up to 12) and a mounted muzzle's and scope's.
pub const MOST_WEAPON_BOXES: usize = 14;
pub static mut weapon_shape: [[P; MOST_WEAPON_BOXES]; 2] = [[core::ptr::null_mut(); MOST_WEAPON_BOXES]; 2];
/// as posed this step (the boxes in use come first)
pub static mut weapon_half: [[[f32; 3]; MOST_WEAPON_BOXES]; 2] = [[[0.0; 3]; MOST_WEAPON_BOXES]; 2];
/// each shape's geometry as last set
pub static mut weapon_made: [[[f32; 3]; MOST_WEAPON_BOXES]; 2] = [[[0.0; 3]; MOST_WEAPON_BOXES]; 2];
pub static mut weapon_local: [[PxTransform; MOST_WEAPON_BOXES]; 2] = [[PxTransform::ZERO; MOST_WEAPON_BOXES]; 2];
/// boxes in use, as posed this step
pub static mut weapon_on: [i32; 2] = [0; 2];
pub static mut material_grip: P = core::ptr::null_mut();
pub static mut grip_material: [bool; 2] = [false; 2];
/// the palm meets loose things again from then (ms): it closed on something
pub static mut loose_from: [u64; 2] = [0; 2];
pub static mut loose_met: [bool; 2] = [true; 2];
pub static mut simulating: [bool; 2] = [false; 2];
pub static mut filter_now: [[u32; 4]; 2] = [[0; 4]; 2];
pub static mut sent: [[f32; 7]; 2] = [[0.0; 7]; 2];
pub static mut sent_track: [[f32; 3]; 2] = [[0.0; 3]; 2];
/// the entity of the tracking a step was sent to (PhysX's space)
pub static mut sent_entity: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut target_entity: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut sent_valid: [bool; 2] = [false; 2];
/// the velocities each body was given before the step: linear (the wrist's), angular
pub static mut gave: [[f32; 6]; 2] = [[0.0; 6]; 2];
/// ...and the linear one as PhysX was given it: the centre of mass's
pub static mut gave_com: [[f32; 3]; 2] = [[0.0; 3]; 2];
/// Bonelab's settling, on the world
pub static mut settle_lin: [f32; 2] = [1.0; 2];
pub static mut settle_ang: [f32; 2] = [1.0; 2];
pub static mut last_ask: [f32; 2] = [0.0; 2];
/// this step, on the world: tracking's palm in nothing (the gap closes at full pace)
pub static mut track_clear: [bool; 2] = [false; 2];
pub static mut gave_valid: [bool; 2] = [false; 2];
/// touching something until then (ms)
pub static mut touch_until: [u64; 2] = [0; 2];
/// the way the solver last pushed each body: out of what it touches
pub static mut pushed_out: [[f32; 3]; 2] = [[0.0; 3]; 2];
/// as each body was made or last set
pub static mut applied_mass: [f32; 2] = [0.0; 2];
pub static mut applied_depenetration: [f32; 2] = [0.0; 2];
pub static mut made_half: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut made_along: [f32; 2] = [0.0; 2];
/// the palm's box's middle in the hand bone's frame
pub static mut made_centre: [[f32; 3]; 2] = [[0.0; 3]; 2];
/// the skin the box was fitted to: 0 a man's, 1 a woman's, -1 the ini's
pub static mut made_skin: [i32; 2] = [-2; 2];
pub static mut applied_ccd: [i32; 2] = [-1; 2];
/// as set; PhysX's own, read as made
pub static mut applied_contact_offset: [f32; 2] = [-1.0; 2];
pub static mut own_contact_offset: [f32; 2] = [0.0; 2];
pub static LIVE: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
pub static LIVE_SCANNER_EPOCH: AtomicU64 = AtomicU64::new(0);
/// physics thread: the epoch of the step being fetched
pub static mut step_scanner_epoch: u64 = 0;
pub static TOUCHED_STEPS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
pub static WORLD_STEPS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// this step's touch was on the world (physics thread)
pub static mut on_world: [bool; 2] = [false; 2];
/// ...and on what the player may push off from: the world or a person, not V's own body or the other hand -- a hand at
/// V's side pushed V away from V, and V walked off on its own (2026-11-10)
pub static mut on_pushable: [bool; 2] = [false; 2];
pub static mut motor_probes: [MotorProbe; 2] = [MotorProbe { before: [0.0; 4], before_spin: [0.0; 3], raw_spin: [0.0; 3], asked_spin: [0.0; 3], given_spin: [0.0; 3], dt: 0.0, angle: 0.0, gain: 0.0, budget: 0.0, flags: 0, touch: super::types::TouchInfo { kind: 0, owner: 0, index: 0, simulates: 0, depth: 0.0 } }; 2];
pub static mut hand_filter: [u32; 4] = [0; 4];
pub static mut hand_filter_static: [u32; 4] = [0; 4];
pub static mut hands_meet_on: bool = false;
pub static mut self_on: [bool; 2] = [false; 2];
/// physics thread: scripted(), as last seen
pub static mut scripted_on: [bool; 2] = [false; 2];
/// THIS HAND'S GUN BOXES MEET THE OTHER HAND: off while the other hand is a script's (scripted), on again once that hand
/// is out of the boxes (other_in_gun) -- not once the palms are 26 cm apart, as the hands meeting each other needs: a
/// pistol held in both hands never gets that far, and the gun's collision was gone for good (2026-11-11).
pub static mut gun_meet_on: [bool; 2] = [false; 2];
/// the same, for the worker
pub static HANDS_MEET_SHOWN: AtomicBool = AtomicBool::new(false);
pub static SELF_ON_SHOWN: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
pub static mut weapon_filter_now: [[u32; 4]; 2] = [[0; 4]; 2];
pub static mut finger_filter_now: [[u32; 4]; 2] = [[0; 4]; 2];
pub static mut forearm_filter_now: [[u32; 4]; 2] = [[0; 4]; 2];

extern "C" {
    static g_pSharedHands: *mut f32;
    static g_hasWeaponEquipped: bool;
    static CyberpunkVR_TwoHandActive: i32;
}
pub unsafe fn shared_hand(i: usize) -> Option<f32> {
    let p = core::ptr::read_volatile(&raw const g_pSharedHands);
    (!p.is_null()).then(|| core::ptr::read_volatile(p.add(i)))
}
pub unsafe fn weapon_equipped() -> bool {
    core::ptr::read_volatile(&raw const g_hasWeaponEquipped)
}
const RELOAD_OWNED_HAND: usize = 162;
const DUAL_WIELD_ACTIVE: usize = 165;
const LEFT_GRIP_PRESSED: usize = 155;
const RIGHT_GRIP_PRESSED: usize = 49;

/// A HAND A SCRIPT IS PLACING: the wrist the reload, the holsters or the grenade claimed (shared slot 162, with their
/// VRHandStopModel hold on it), the two-hand hold's support hand, or the free hand closing its grip with a gun out
/// (taking the foregrip, a magazine, a holstered gun). Such a hand does not meet the gun's boxes nor the other hand: it
/// is sent into them on purpose, and stopped on them it hung a few cm short of where the script put it (2026-11-11). Not
/// by g_VRHandStopValid: HandCollision's own pass writes a stop on every unclaimed wrist it pushes and clears it again.
pub unsafe fn scripted(side: usize) -> bool {
    if side > 1 {
        return false;
    }
    if let Some(owned) = shared_hand(RELOAD_OWNED_HAND) {
        if owned > -0.5 && (owned + 0.5) as i32 == side as i32 {
            return true;
        }
    }
    if !weapon_equipped() {
        return false;
    }
    if shared_hand(DUAL_WIELD_ACTIVE).is_some_and(|v| v > 0.5) {
        return false;
    }
    let gun_hand = if super::super::weapon_arm::cvr_anim_weapon_in_left_hand() { 0 } else { 1 };
    if side == gun_hand {
        return false;
    }
    if core::ptr::read_volatile(&raw const CyberpunkVR_TwoHandActive) != 0 {
        return true;
    }
    shared_hand(if side == 1 { RIGHT_GRIP_PRESSED } else { LEFT_GRIP_PRESSED }).is_some_and(|v| v > 0.5)
}

pub unsafe fn refilter(actor_to_filter: P) {
    if !scene.is_null() && !actor_to_filter.is_null() {
        px::reset_filtering(scene, actor_to_filter);
    }
}
/// SetFilter: the filters of a hand's shapes -- its palm's, its gun's boxes', its fingers', its forearm's.
pub unsafe fn set_filter(side: usize, palm: &[u32; 4], weapon: &[u32; 4], fingers: &[u32; 4], forearm: &[u32; 4]) {
    if shape[side].is_null() {
        return;
    }
    let mut changed = false;
    if filter_now[side] != *palm {
        px::set_sim_filter(shape[side], palm);
        filter_now[side] = *palm;
        changed = true;
    }
    if weapon_filter_now[side] != *weapon {
        for &sh in &weapon_shape[side] {
            if !sh.is_null() {
                px::set_sim_filter(sh, weapon);
            }
        }
        weapon_filter_now[side] = *weapon;
        changed = true;
    }
    if finger_filter_now[side] != *fingers {
        for finger in &finger_shape[side] {
            for &sh in finger {
                if !sh.is_null() {
                    px::set_sim_filter(sh, fingers);
                }
            }
        }
        finger_filter_now[side] = *fingers;
        changed = true;
    }
    if forearm_filter_now[side] != *forearm {
        for sh in [forearm_shape[side], forearm_upper[side]] {
            if !sh.is_null() {
                px::set_sim_filter(sh, forearm);
            }
        }
        forearm_filter_now[side] = *forearm;
        changed = true;
    }
    if changed {
        refilter(actor[side]);
    }
}

/// Inside a kinematic enclosure (loose.rs) a hand gives up that enclosure's group bits, physics thread: all of the ones
/// the mask shares with it, or none (half of them still meets it) -- and none when that would leave a static group the
/// mask met unmet: the world stays solid even if it shares the enclosure's bits.
pub const MOST_ENCLOSED_GROUPS: usize = 4;
#[derive(Clone, Copy, Default)]
pub struct Enclosed {
    pub group: [u64; MOST_ENCLOSED_GROUPS],
    pub n: usize,
}
pub static mut enclosed: [Enclosed; 2] = [Enclosed { group: [0; MOST_ENCLOSED_GROUPS], n: 0 }; 2];
pub unsafe fn keeps_statics(mask: u64, rest: u64) -> bool {
    !census.seen[..census.groups as usize].iter().any(|g| g.n[KIND_STATIC] != 0 && (g.group & mask) != 0 && (g.group & rest) == 0)
}
extern "C" {
    static g_isInVehicle: bool;
}
pub unsafe fn safe_to_drop(mask: u64, e: &Enclosed) -> u64 {
    let mut drop = 0u64;
    // SEATED IN A CAR its hull round the seats is the enclosure, and the cabin the hands stop on is their own copies
    // (cabin.rs): its bits go whatever statics share them -- kept, the hull pushed both palms out of the seat, they were
    // let through as from a loose thing and caught again, the hands jumping up and down (2026-11-26)
    let seated = core::ptr::read_volatile(&raw const g_isInVehicle);
    for &g in &e.group[..e.n] {
        let need = g & mask & !drop;
        if need != 0 && (seated || keeps_statics(mask, mask & !(drop | need))) {
            drop |= need;
        }
    }
    drop
}
fn wide(w: &[u32; 4], i: usize) -> u64 {
    u64::from(w[i]) | (u64::from(w[i + 1]) << 32)
}
/// THE PALM'S AND THE FINGERS' FILTERS: the world's (with the loose things, or the world alone), and V's own body and
/// the other hand while the hand meets them. The fingers meet the world alone, as made. A GUN'S BOXES meet nothing of
/// the world -- a gun stopped by a wall turned the aim -- and the other hand by this hand's own bit while the two meet:
/// in that bit's group, meeting nothing themselves (the pair rule is an or). THE FOREARM meets the world, and the other
/// hand's palm and fingers by the same bit (the other hand went through it); not V's own body (it lies along the trunk)
/// and not the other forearm (arms cross). (palm, fingers, weapon, forearm)
pub unsafe fn hand_filters(side: usize, loose: bool) -> ([u32; 4], [u32; 4], [u32; 4], [u32; 4]) {
    let base = if loose { hand_filter } else { hand_filter_static };
    let mut group = wide(&hand_filter, 0);
    let mut mask = wide(&base, 2);
    let mut mask_f = wide(&hand_filter_static, 2);
    let mut gun_group = 0u64;
    let mut fore_group = wide(&hand_filter_static, 0);
    let person_bit = if side == 1 { bit_person_right } else { bit_person_left };
    group |= person_bit;
    fore_group |= person_bit;
    let (bit, other_bit) = if side == 1 { (bit_right, bit_left) } else { (bit_left, bit_right) };
    if hands_meet_on {
        group |= bit;
        mask |= other_bit;
        mask_f |= other_bit;
        fore_group |= bit;
    }
    // the gun's boxes carry this hand's bit while they meet the other hand; this hand meets the other's gun by its bit
    if gun_meet_on[side] {
        gun_group = bit;
    }
    if gun_meet_on[1 - side] {
        mask |= other_bit;
        mask_f |= other_bit;
    }
    let mut fore_mask = wide(&hand_filter_static, 2);
    if core::ptr::read_volatile(&raw const g_isInVehicle) {
        // SEATED, NOT THE CAR'S OWN PARTS: their copies in the player's frame are what the hands stop on (cabin.rs)
        let car = super::cabin::CAR_GROUPS.load(Relaxed);
        mask &= !car;
        mask_f &= !car;
        fore_mask &= !car;
    }
    if enclosed[side].n != 0 {
        mask &= !safe_to_drop(mask, &enclosed[side]);
        mask_f &= !safe_to_drop(mask_f, &enclosed[side]);
        fore_mask &= !safe_to_drop(fore_mask, &enclosed[side]);
    }
    let weapon = [gun_group as u32, (gun_group >> 32) as u32, 0, 0];
    let forearm = [fore_group as u32, (fore_group >> 32) as u32, fore_mask as u32, (fore_mask >> 32) as u32];
    if self_on[side] {
        let own = if side == 1 { bit_self_right } else { bit_self_left };
        mask |= own;
        mask_f |= own;
    }
    let palm = [group as u32, (group >> 32) as u32, mask as u32, (mask >> 32) as u32];
    let fingers = [group as u32, (group >> 32) as u32, mask_f as u32, (mask_f >> 32) as u32];
    (palm, fingers, weapon, forearm)
}

/// SetMaterials: the hand's material on every shape of it -- the open hand's or the closed one's.
pub unsafe fn set_materials(side: usize, grip: bool) {
    if grip_material[side] == grip || shape[side].is_null() || material_grip.is_null() {
        return;
    }
    let mats: [P; 1] = [if grip { material_grip } else { material }];
    let set = |sh: P| {
        if !sh.is_null() {
            at::<unsafe extern "C" fn(P, *const P, u16)>(sh, px::SHAPE_SET_MATERIALS)(sh, mats.as_ptr(), 1);
        }
    };
    set(shape[side]);
    for finger in &finger_shape[side] {
        for &sh in finger {
            set(sh);
        }
    }
    set(forearm_shape[side]);
    set(forearm_upper[side]);
    for &sh in &weapon_shape[side] {
        set(sh);
    }
    grip_material[side] = grip;
}

unsafe fn create_shape(geometry: *const core::ffi::c_void) -> P {
    let mats: [P; 1] = [material];
    let flags: u8 = 1; // a simulation shape only: no ray, no sweep, no character controller sees it
    let physics = px::physics();
    at::<unsafe extern "C" fn(P, *const core::ffi::c_void, *const P, u16, bool, *const u8) -> P>(physics, px::CREATE_SHAPE)(physics, geometry, mats.as_ptr(), 1, true, &flags)
}
/// Attach: a shape on body `a`, simulation only, at `local` (a capsule's axis is its x), with this filter.
pub unsafe fn attach(a: P, geometry: *const core::ffi::c_void, local: &PxTransform, filter: &[u32; 4]) -> P {
    let sh = create_shape(geometry);
    if sh.is_null() {
        return sh;
    }
    at::<unsafe extern "C" fn(P, *const PxTransform)>(sh, px::SHAPE_SET_LOCAL_POSE)(sh, local);
    px::set_sim_filter(sh, filter);
    at::<unsafe extern "C" fn(P, P) -> bool>(a, px::ATTACH_SHAPE)(a, sh);
    at::<unsafe extern "C" fn(P)>(sh, px::SHAPE_RELEASE)(sh); // the body holds it now
    sh
}
pub unsafe fn capsule(a: P, radius: f32, half: f32, local: &PxTransform) -> P {
    let g = CapsuleGeometry { kind: px::GEOM_CAPSULE, radius, half_height: half };
    let filter = hand_filter_static;
    attach(a, (&g as *const CapsuleGeometry).cast(), local, &filter)
}

/// The palm's box for this hand: V's own skin once the body is known (hand_skin), the ini's otherwise; (half, centre,
/// what it was fitted to: 0 a man's, 1 a woman's, -1 the ini's).
pub unsafe fn box_for(side: usize, s: &Settings) -> ([f32; 3], [f32; 3], i32) {
    let female = super::body_female();
    if let Some(k) = (s.skin != 0).then(|| skin_for(female, side as i32)).flatten() {
        let mut half = [0.0f32; 3];
        let mut centre = [0.0f32; 3];
        for i in 0..3 {
            half[i] = 0.5 * (k.box_hi[i] - k.box_lo[i]);
            centre[i] = 0.5 * (k.box_hi[i] + k.box_lo[i]);
        }
        return (half, centre, i32::from(female != 0));
    }
    (s.half, [if side == 1 { -s.along } else { s.along }, 0.0, 0.0], -1)
}
/// ...grown by the skin's margin
pub unsafe fn box_with_margin(side: usize, s: &Settings) -> ([f32; 3], [f32; 3], i32) {
    let (mut half, centre, of) = box_for(side, s);
    for h in &mut half {
        *h += s.skin_margin;
    }
    (half, centre, of)
}

/// THE HAND'S MASS IN ITS PALM: the body's centre of mass at the palm box's middle, the box's own inertia about it. At
/// the wrist (the actor's origin, PhysX's default) the steering pushed the wrist and a wall stopped the palm's face 4 cm
/// on: pressed on, the box pivoted on its heel and lifted its fingers' end 18 mm off the wall (2026-11-09).
pub unsafe fn set_mass_at(a: P, m: f32, half: &[f32; 3], c: &[f32; 3]) {
    let at_pose = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: *c };
    at::<unsafe extern "C" fn(P, *const PxTransform)>(a, px::SET_CMASS_LOCAL_POSE)(a, &at_pose);
    at::<unsafe extern "C" fn(P, f32)>(a, px::SET_MASS)(a, m);
    // ...turning as hard to turn as about the wrist (the box's own inertia and its shift there): with the box's own
    // alone, a third of it, the palm pressed on a wall rocked on it more; a hand on its forearm is slow to turn anyway
    let (x, y, z) = (2.0 * half[0], 2.0 * half[1], 2.0 * half[2]);
    let inertia = [
        m / 12.0 * (y * y + z * z) + m * (c[1] * c[1] + c[2] * c[2]),
        m / 12.0 * (x * x + z * z) + m * (c[0] * c[0] + c[2] * c[2]),
        m / 12.0 * (x * x + y * y) + m * (c[0] * c[0] + c[1] * c[1]),
    ];
    at::<unsafe extern "C" fn(P, *const f32)>(a, px::SET_INERTIA)(a, inertia.as_ptr());
}

/// Make: a hand's body, its palm box, its finger, forearm and gun shapes, in the world scene at `at`.
pub unsafe fn make(side: usize, at_pose: &PxTransform, s: &Settings) -> bool {
    let physics = px::physics();
    if material.is_null() {
        let create = at::<unsafe extern "C" fn(P, f32, f32, f32) -> P>(physics, px::CREATE_MATERIAL);
        material = create(physics, s.friction, s.friction * 0.85, 0.0);
        material_grip = create(physics, s.friction_grip, s.friction_grip * 0.85, 0.0);
        if material.is_null() || material_grip.is_null() {
            return false;
        }
        if *(material as *const usize) == px::dll() + px::MATERIAL_TABLE {
            at::<unsafe extern "C" fn(P, i32)>(material, px::MATERIAL_SET_FRICTION_COMBINE)(material, px::COMBINE_MIN);
        }
    }
    let a = at::<unsafe extern "C" fn(P, *const PxTransform) -> P>(physics, px::CREATE_RIGID_DYNAMIC)(physics, at_pose);
    if a.is_null() {
        return false;
    }
    let (box_half, box_centre, skin_of) = box_with_margin(side, s);
    let geometry = BoxGeometry { kind: px::GEOM_BOX, half: box_half };
    let sh = create_shape((&geometry as *const BoxGeometry).cast());
    if sh.is_null() {
        at::<unsafe extern "C" fn(P)>(a, px::RELEASE)(a);
        return false;
    }
    // the box along the fingers: -x of the right hand's bone, +x of the left's
    let local = PxTransform { q: [0.0, 0.0, 0.0, 1.0], p: box_centre };
    at::<unsafe extern "C" fn(P, *const PxTransform)>(sh, px::SHAPE_SET_LOCAL_POSE)(sh, &local);
    px::set_sim_filter(sh, &hand_filter);
    filter_now[side] = hand_filter;
    weapon_filter_now[side] = [0; 4];
    finger_filter_now[side] = hand_filter_static;
    forearm_filter_now[side] = hand_filter_static;
    at::<unsafe extern "C" fn(P, P) -> bool>(a, px::ATTACH_SHAPE)(a, sh);
    at::<unsafe extern "C" fn(P)>(sh, px::SHAPE_RELEASE)(sh); // the body holds it now
    // THE FINGERS AND THE FOREARM, capsules on the same body, posed each step as they are drawn (pose_shapes): what a
    // fingertip or the forearm meets stops the hand as the palm does, and is pushed as by the palm. Made in the palm.
    for f in 0..5 {
        for j in 0..3 {
            finger_shape[side][f][j] = if f == 4 && j == 0 { core::ptr::null_mut() } else { capsule(a, 0.008, 0.01, &local) };
            finger_half[side][f][j] = 0.01;
        }
    }
    forearm_shape[side] = capsule(a, 0.035, 0.08, &local);
    forearm_half[side] = 0.08;
    forearm_upper[side] = capsule(a, 0.035, 0.08, &local);
    forearm_upper_half[side] = 0.08;
    forearm_radius[side] = [0.035; 2];
    finger_radius[side] = [[0.008; 3]; 5];
    for i in 0..MOST_WEAPON_BOXES {
        let parked = BoxGeometry { kind: px::GEOM_BOX, half: [0.004; 3] };
        let none = [0u32; 4]; // nothing until the hands meet (hand_filters)
        weapon_shape[side][i] = attach(a, (&parked as *const BoxGeometry).cast(), &local, &none);
        weapon_made[side][i] = [0.004; 3];
    }
    weapon_on[side] = 0;
    grip_material[side] = false;
    set_mass_at(a, s.mass, &box_half, &box_centre);
    let (x, y, z) = (2.0 * box_half[0], 2.0 * box_half[1], 2.0 * box_half[2]);
    at::<unsafe extern "C" fn(P, i32, bool)>(a, px::SET_ACTOR_FLAG)(a, px::DISABLE_GRAVITY, true);
    // continuous collision and the contact offset: set in before_simulate, as they can change on a made body
    applied_ccd[side] = -1;
    applied_contact_offset[side] = -1.0;
    own_contact_offset[side] = at::<unsafe extern "C" fn(P) -> f32>(sh, px::SHAPE_GET_CONTACT_OFFSET)(sh);
    at::<unsafe extern "C" fn(P, f32)>(a, px::SET_LINEAR_DAMPING)(a, 0.0);
    at::<unsafe extern "C" fn(P, f32)>(a, px::SET_ANGULAR_DAMPING)(a, 0.05);
    at::<unsafe extern "C" fn(P, f32)>(a, px::SET_MAX_ANGULAR_VELOCITY)(a, 200.0);
    at::<unsafe extern "C" fn(P, f32)>(a, px::SET_MAX_DEPENETRATION)(a, s.depenetration);
    at::<unsafe extern "C" fn(P, u32, u32)>(a, px::SET_SOLVER_ITERATIONS)(a, 16, 4);
    // PxActor::userData (+0x10): the game reads a proxy id there (low 32 bits) and a body index (the next 16); all
    // ones finds nothing in its handle table, where 0 could name a live proxy
    *((a as *mut u8).add(0x10) as *mut usize) = !0usize;
    at::<unsafe extern "C" fn(P, P, *const core::ffi::c_void)>(scene, px::ADD_ACTOR)(scene, a, core::ptr::null());
    actor[side] = a;
    shape[side] = sh;
    simulating[side] = true;
    applied_mass[side] = s.mass;
    applied_depenetration[side] = s.depenetration;
    made_half[side] = box_half;
    made_centre[side] = box_centre;
    made_along[side] = box_centre[0].abs();
    made_skin[side] = skin_of;
    log!(
        "[physhands] {} hand made: body {:p}, a {:.1} kg box {:.3} x {:.3} x {:.3} m about ({:.3} {:.3} {:.3}), {}, group {:08x}{:08x} mask {:08x}{:08x}\n",
        if side == 1 { "right" } else { "left" },
        a,
        s.mass,
        x,
        y,
        z,
        box_centre[0],
        box_centre[1],
        box_centre[2],
        match skin_of {
            1 => "a woman's skin",
            0 => "a man's skin",
            _ => "the ini's box",
        },
        hand_filter[1],
        hand_filter[0],
        hand_filter[3],
        hand_filter[2]
    );
    true
}

/// TargetNow: where tracking puts the hand now, PhysX's space.
pub unsafe fn target_now(side: usize, origin: &[f32; 3]) -> Option<PxTransform> {
    let t = step_target(side)?;
    if tick_ms().wrapping_sub(t.at) > 150 {
        return None;
    }
    let p = qrot(&t.entity_rot, &t.pos);
    let mut out = PxTransform::default();
    for k in 0..3 {
        out.p[k] = t.entity_pos[k] + p[k] - origin[k];
        target_entity[side][k] = t.entity_pos[k] - origin[k];
    }
    out.q = qmul(&t.entity_rot, &t.rot);
    (qnorm(&mut out.q) && (out.p[0] + out.p[1] + out.p[2]).is_finite()).then_some(out)
}

/// THE PLAYER'S OWN PACE (world, m/s), from the entity's place as the game thread writes it each frame: over the time
/// between two different places, not a step's (two steps can fall in one frame, the second seeing no move). A jump of
/// more than a metre is a teleport, not a pace. Physics thread.
pub static mut player_pace: [f32; 3] = [0.0; 3];
static mut PACE_LAST: [f32; 3] = [0.0; 3];
static mut PACE_LAST_AT: i64 = 0;
static mut PACE_HAVE: bool = false;
pub unsafe fn update_player_pace() {
    let (now, freq) = qpc();
    if !player_valid() {
        PACE_HAVE = false;
        player_pace = [0.0; 3];
        return;
    }
    let p = player_pos();
    if !(p[0] + p[1] + p[2]).is_finite() {
        return;
    }
    let since = (now - PACE_LAST_AT) as f64 / freq as f64;
    let mut d = [0.0f32; 3];
    let mut d2 = 0.0f32;
    for k in 0..3 {
        d[k] = p[k] - PACE_LAST[k];
        d2 += d[k] * d[k];
    }
    if !PACE_HAVE {
        PACE_LAST = p;
        PACE_LAST_AT = now;
        PACE_HAVE = true;
        return;
    }
    if d2 < 1e-10 {
        if since > 0.1 {
            player_pace = [0.0; 3]; // standing, or the game stalled
        }
        return;
    }
    PACE_LAST = p;
    PACE_LAST_AT = now;
    if d2 > 1.0 || since <= 1e-4 || since > 0.1 {
        player_pace = [0.0; 3];
        return;
    }
    for k in 0..3 {
        player_pace[k] = (f64::from(d[k]) / since) as f32;
    }
    let sp = (player_pace[0] * player_pace[0] + player_pace[1] * player_pace[1] + player_pace[2] * player_pace[2]).sqrt();
    if sp > 30.0 {
        for c in &mut player_pace {
            *c *= 30.0 / sp;
        }
    }
}

/// The bodies ride with the common tracking frame: no further than this in a step (more is a teleport, put); how many
/// steps rode (statistics) and how far this step's ride went.
pub const RIDE_MOST: f32 = 1.0;
pub static RODE_STEPS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
/// ...of them, on something: its part into what it touched taken out
pub static RODE_ALONG: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
pub static mut rode_by: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut frame_riding: [bool; 2] = [false; 2];
static mut PACE_WAS: [[f32; 3]; 2] = [[0.0; 3]; 2];

unsafe fn global_pose(a: P) -> PxTransform {
    let mut now = PxTransform::default();
    at::<unsafe extern "C" fn(P, *mut PxTransform)>(a, px::GET_GLOBAL_POSE)(a, &mut now);
    now
}
pub unsafe fn set_velocities(a: P, linear: &[f32; 3], angular: &[f32; 3], wake: bool) {
    at::<unsafe extern "C" fn(P, *const f32, bool)>(a, px::SET_LINEAR_VELOCITY)(a, linear.as_ptr(), wake);
    at::<unsafe extern "C" fn(P, *const f32, bool)>(a, px::SET_ANGULAR_VELOCITY)(a, angular.as_ptr(), wake);
}
fn rotation_vector(q: &mut [f32; 4], d: f32) -> [f32; 3] {
    if q[3] < 0.0 {
        for c in q.iter_mut() {
            *c = -*c;
        }
    }
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    let a = 2.0 * n.atan2(q[3]);
    let mut out = [0.0f32; 3];
    for k in 0..3 {
        out[k] = if n > 1e-7 { q[k] / n * a / d } else { 0.0 };
    }
    out
}

/// Steer: the body given the velocities that bring it to `to` by the step's end, as far as the hand has strength --
/// quick in the air, a hand's push on something, Bonelab's settling on the world.
pub unsafe fn steer(side: usize, to: &PxTransform, dt: f32, s: &Settings, put: bool) {
    let probe = &mut motor_probes[side];
    *probe = MotorProbe { dt, ..Default::default() };
    let a = actor[side];
    // Both bodies and their previous targets were moved together before any contact query. The drive now sees only
    // motion within that common frame.
    let rides = frame_riding[side];
    let pace: [f32; 3] = if rides { [0.0; 3] } else { player_pace };
    let mut pace_dv = [0.0f32; 3];
    for k in 0..3 {
        pace_dv[k] = pace[k] - PACE_WAS[side][k];
        PACE_WAS[side][k] = pace[k];
    }
    let now = global_pose(a);
    probe.before = now.q;
    let mut gap2 = 0.0f32;
    for k in 0..3 {
        gap2 += (to.p[k] - now.p[k]) * (to.p[k] - now.p[k]);
    }
    if put || !simulating[side] || !(gap2 < s.teleport * s.teleport) {
        probe.flags = 16;
        // put there: just made, just back on, so far off (a teleport, a load) that steering would fling it through
        // things, the player just moved by a teleport, or stuck behind something an arm does not reach round
        at::<unsafe extern "C" fn(P, *const PxTransform, bool)>(a, px::SET_GLOBAL_POSE)(a, to, true);
        set_velocities(a, &[0.0; 3], &[0.0; 3], true);
        gave_valid[side] = false;
        if !simulating[side] {
            at::<unsafe extern "C" fn(P, i32, bool)>(a, px::SET_ACTOR_FLAG)(a, px::DISABLE_SIMULATION, false);
            simulating[side] = true;
        }
        return;
    }
    let mut v = [0.0f32; 3];
    let mut w = [0.0f32; 3];
    at::<unsafe extern "C" fn(P, *mut f32)>(a, px::GET_LINEAR_VELOCITY)(a, v.as_mut_ptr());
    at::<unsafe extern "C" fn(P, *mut f32)>(a, px::GET_ANGULAR_VELOCITY)(a, w.as_mut_ptr());
    probe.before_spin = w;
    let carried = gave_valid[side]; // (the touch test below compares PhysX's velocity with the one given: not carried yet)
    // TOUCHING: nothing else acts on the body (no gravity, no damping), so a velocity that is not the one it was given
    // is the solver's doing -- something stopped or turned it. Both as PhysX has them, the centre of mass's: compared as
    // the wrist's, a quick turn read as a touch and the hand in the air floated behind a swing.
    let tick = tick_ms();
    if gave_valid[side] {
        let (mut dl, mut da) = (0.0f32, 0.0f32);
        for k in 0..3 {
            dl += (v[k] - gave_com[side][k]) * (v[k] - gave_com[side][k]);
            da += (w[k] - gave[side][3 + k]) * (w[k] - gave[side][3 + k]);
        }
        // a short window: past it the hand is quick again (at 250 ms a hand drawn off a table fell behind and caught up
        // in a jump)
        if dl > 0.05 * 0.05 || da > 0.5 * 0.5 {
            touch_until[side] = tick + 60;
        }
        // the way the solver pushed it: out of what it touches (an impulse changes the centre of mass's velocity by J/m)
        if dl > 0.05 * 0.05 {
            let n = dl.sqrt();
            for k in 0..3 {
                pushed_out[side][k] = (v[k] - gave_com[side][k]) / n;
            }
        }
    }
    // PhysX's velocity is the centre of mass's (the palm's middle); the steering below is the wrist's (the actor's
    // origin): the wrist moves at the palm's velocity less the turn's share, w x (palm - wrist)
    let com = qrot(&now.q, &made_centre[side]);
    let wc = cross(&w, &com);
    for k in 0..3 {
        v[k] -= wc[k];
    }
    let touching = tick < touch_until[side];
    if touching {
        TOUCHED_STEPS[side].fetch_add(1, Relaxed);
    }
    // ON THE WORLD OR ON A LOOSE THING. Pressed on a table the hand leans soft and turns little, so it lies still; on a
    // cup it pushes with a hand's force at the hand's own pace.
    let world = touching && touches_world(side, &now, s, TOUCH_ALL, Some(&mut motor_probes[side].touch));
    let probe = &mut motor_probes[side];
    on_world[side] = world;
    on_pushable[side] = world && touches_world(side, &now, s, TOUCH_PEOPLE, None);
    if world {
        WORLD_STEPS[side].fetch_add(1, Relaxed);
    }
    // PRESSED, it is not flung at the target every step: it leans in at a walking pace, which is still all of `force`;
    // and on the world it closes only part of the gap a step (sent all the way, a body sliding on the floor overshot)
    let settle = world && s.settle != 0;
    let gain = if world && !settle { s.press_gain } else { 1.0 };
    // ...a share of the hand's OWN gap, against the player: the player's own pace is carried whole
    let mut want = [0.0f32; 3];
    for k in 0..3 {
        want[k] = pace[k] + ((to.p[k] - now.p[k]) / dt - pace[k]) * gain;
    }
    if world && gain < 1.0 {
        // away from the surface it is all the way: lifted off a table at a share of its gap a step, the body was left
        // 3 cm behind tracking for the few steps it still touched, and the drawn hand jumped up as it let go
        let n = pushed_out[side];
        let mut out = 0.0f32;
        for k in 0..3 {
            out += ((to.p[k] - now.p[k]) / dt - pace[k]) * n[k];
        }
        if out > 0.0 {
            for k in 0..3 {
                want[k] += n[k] * out * (1.0 - gain);
            }
        }
    }
    // all the way by the step's end is tracking's own move this step plus the gap the body already had; the gap's
    // share is held to catch_speed
    if sent_valid[side] && (!world || settle) {
        let mut carry = [0.0f32; 3];
        let mut gap = [0.0f32; 3];
        let mut g = 0.0f32;
        for k in 0..3 {
            carry[k] = (to.p[k] - sent[side][4 + k]) / dt;
            gap[k] = want[k] - carry[k];
            g += gap[k] * gap[k];
        }
        g = g.sqrt();
        // ON THE WORLD, TRACKING'S PLACE CLEAR: the gap closes at catch_speed as in the air; tracking's place in the
        // world (pressed in), at press_catch
        track_clear[side] = settle && g > 0.01 && !touches_world(side, to, s, TOUCH_ALL, None);
        let catch_most = if settle && !track_clear[side] { s.press_catch } else { s.catch_speed };
        if g > catch_most {
            for k in 0..3 {
                want[k] = carry[k] + gap[k] * (catch_most / g);
            }
        }
    }
    if world {
        // only the part into the surface: along it and away from it the hand is as quick as ever
        let n = pushed_out[side];
        let mut into = 0.0f32; // (against the player: what a running player's hand touches runs with it)
        for k in 0..3 {
            into -= (want[k] - pace[k]) * n[k];
        }
        if into > s.press_speed {
            for k in 0..3 {
                want[k] += n[k] * (into - s.press_speed);
            }
        }
    }
    if carried {
        for k in 0..3 {
            v[k] += pace_dv[k];
        }
    }
    let mut dv = [0.0f32; 3];
    for k in 0..3 {
        dv[k] = want[k] - v[k];
    }
    let dvn = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
    if settle {
        // Bonelab's _jerkedMult: up while the ask grows, down toward 15% while it does not
        settle_lin[side] = if dvn > last_ask[side] + 0.005 { (settle_lin[side] + 3.2 * dt).min(1.0) } else { (settle_lin[side] - 4.0 * dt).max(0.15) };
    } else {
        settle_lin[side] = 1.0;
    }
    last_ask[side] = dvn;
    let most = (if settle {
        s.move_accel * settle_lin[side]
    } else if world {
        s.force / s.mass
    } else if touching {
        s.push_force / s.mass
    } else {
        s.move_accel
    }) * dt;
    let kv = if dvn > most { most / dvn } else { 1.0 };
    let mut set = [0.0f32; 3];
    for k in 0..3 {
        set[k] = v[k] + dv[k] * kv;
    }
    {
        // the speed cap on the hand's own move: against the player, not the world
        let rel = [set[0] - pace[0], set[1] - pace[1], set[2] - pace[2]];
        let sp = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
        if sp > s.max_speed {
            for k in 0..3 {
                set[k] = pace[k] + rel[k] * (s.max_speed / sp);
            }
        }
    }
    let inv = qconj(&now.q);
    let mut dq = qmul(&to.q, &inv);
    if dq[3] < 0.0 {
        for c in &mut dq {
            *c = -*c;
        }
    }
    let sn = (dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]).sqrt();
    let angle = 2.0 * sn.atan2(dq[3]);
    let mut spin = [0.0f32; 3];
    if sn > 1e-7 {
        for k in 0..3 {
            spin[k] = dq[k] / sn * angle / dt;
        }
    }
    probe.angle = angle;
    probe.raw_spin = spin;
    // the same for the turn: tracking's own turn this step, and the turn the body was already behind by at no more
    // than catch_spin
    let mut turn_rate = 0.0f32; // tracking's own turn this step, rad/s (for the settling)
    if sent_valid[side] && (!world || settle) {
        let sent_q = [sent[side][0], sent[side][1], sent[side][2], sent[side][3]];
        let pinv = qconj(&sent_q);
        let mut tq = qmul(&to.q, &pinv); // tracking's turn this step
        let mut gq = qmul(&sent_q, &inv); // where tracking was against where the body is: the turn it is behind by
        let carry = rotation_vector(&mut tq, dt);
        let behind = rotation_vector(&mut gq, dt);
        let b = (behind[0] * behind[0] + behind[1] * behind[1] + behind[2] * behind[2]).sqrt();
        // the turn the same way: tracking's place clear, at catch_spin
        let catch_most = if settle && !track_clear[side] { s.press_catch_spin } else { s.catch_spin };
        if b > catch_most {
            for k in 0..3 {
                spin[k] = carry[k] + behind[k] * (catch_most / b);
            }
        }
        let mut off = 0.0f32;
        for k in 0..3 {
            off += (carry[k] - w[k]) * (carry[k] - w[k]);
        }
        turn_rate = off.sqrt();
    }
    let armed = weapon_on[side] > 0;
    if settle && !armed {
        // Bonelab's _angJerkMult: up at 12/s while the turn rate is 3 rad/s off tracking's, else down toward 10% at 8/s
        settle_ang[side] = if turn_rate > 3.0 { (settle_ang[side] + 12.0 * dt).min(1.0) } else { (settle_ang[side] - 8.0 * dt).max(0.1) };
    } else {
        settle_ang[side] = 1.0;
    }
    if world && !(settle && !armed) {
        let most = if armed { s.weapon_spin } else { s.press_spin };
        let spn = (spin[0] * spin[0] + spin[1] * spin[1] + spin[2] * spin[2]).sqrt();
        if spn > most {
            for c in &mut spin {
                *c *= most / spn;
            }
        }
    }
    let mut dw = [0.0f32; 3];
    for k in 0..3 {
        dw[k] = spin[k] - w[k];
    }
    let most_w = (if settle && !armed {
        s.spin_accel * settle_ang[side]
    } else if world {
        if armed {
            s.weapon_spin_accel
        } else {
            s.spin_contact
        }
    } else {
        s.spin_accel
    }) * dt;
    let dwn = (dw[0] * dw[0] + dw[1] * dw[1] + dw[2] * dw[2]).sqrt();
    let kw = if dwn > most_w { most_w / dwn } else { 1.0 };
    let mut wset = [0.0f32; 3];
    for k in 0..3 {
        wset[k] = w[k] + dw[k] * kw;
    }
    probe.flags = u32::from(touching) | (u32::from(world) << 1) | (u32::from(settle) << 2) | (u32::from(track_clear[side]) << 3);
    probe.gain = kw;
    probe.budget = most_w;
    probe.asked_spin = spin;
    probe.given_spin = wset;
    if !(set[0] + set[1] + set[2] + wset[0] + wset[1] + wset[2]).is_finite() {
        return;
    }
    let ws = cross(&wset, &com);
    let set_com = [set[0] + ws[0], set[1] + ws[1], set[2] + ws[2]];
    set_velocities(a, &set_com, &wset, true);
    for k in 0..3 {
        gave[side][k] = set[k];
        gave[side][3 + k] = wset[k];
        gave_com[side][k] = set_com[k];
    }
    gave_valid[side] = true;
}

pub unsafe fn park(side: usize) {
    if !actor[side].is_null() && simulating[side] {
        at::<unsafe extern "C" fn(P, i32, bool)>(actor[side], px::SET_ACTOR_FLAG)(actor[side], px::DISABLE_SIMULATION, true);
        simulating[side] = false;
    }
    LIVE[side].store(false, Release);
    sent_valid[side] = false;
    gave_valid[side] = false;
}

/// Costs, for the statistics: the physics thread's own work, microseconds.
pub static BEFORE_US: AtomicU64 = AtomicU64::new(0);
pub static AFTER_US: AtomicU64 = AtomicU64::new(0);
pub static BEFORE_MOST_US: AtomicU64 = AtomicU64::new(0);
pub static AFTER_MOST_US: AtomicU64 = AtomicU64::new(0);
pub static STEPS_SEEN: AtomicU64 = AtomicU64::new(0);
pub fn charge(sum: &AtomicU64, most: &AtomicU64, t0: i64) {
    let (t1, f) = qpc();
    let us = ((t1 - t0) * 1_000_000 / f.max(1)) as u64;
    sum.fetch_add(us, Relaxed);
    most.fetch_max(us, Relaxed);
}

/// THE SCENE PLAYS THE BODY: the user's cutscene tier (and the vehicle's, mounted), a grapple, a carried body, a
/// finisher, a surveillance camera -- whatever hands VRIK's arms to the animation (the pose hook's early-outs) takes the
/// hands' bodies out of the world too: an animated arm swung through a table would shove what is on it.
pub unsafe fn scene_owns_arms() -> bool {
    super::scene::scene_owns_arms()
}
pub use super::vehicle_hands;
