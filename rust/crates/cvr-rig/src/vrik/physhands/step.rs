//! The physics step (physics thread, from the PxScene::simulate / fetchResults hooks): before it, the bodies are made,
//! steered and given what the step needs; after it, their poses are read for the worker and the holds published.
use super::body::*;
use super::census::{filter_ready, player_pos, self_bits_ready};
use super::fingers::HOLDING_ON;
use super::grab::*;
use super::loose::{apart_step, drop_step, enclosure_groups, global_pose, loose_cap, palm_in_loose, test_stack_step, LOOSE_LET_THROUGH, PALM_REFUSED, ENCLOSURE_STEPS};
use super::math::{cross, qmul, qnorm, qrot};
use super::people::{people_step, person_near, person_owner};
use super::pose::{reader_pose, step_pose_inputs};
use super::px::{self, at, PxTransform, P};
use super::settings::{self, Settings};
use super::shapes::{arm_reach, far_since, pose_shapes, unstick, TELEPORT_PUT_MOST_MS, TELEPORT_PUT_MS, TELEPORT_STEP};
use super::touch::{hands_apart, hold_feature, in_self, other_hand_out, other_in_gun, path_free, person_at, person_pose, touches_world, ways_out, TOUCH_ALL};
use super::types::{HoldPose, Wanted};
use super::worker::WANTED;
use super::{qpc, tick_ms};
use core::sync::atomic::Ordering::*;
use cvr_host::log;
use cvr_physhands::climb_support::Feature;
use cvr_physhands::handspace::{self, Motion, Reference, Rotation, Vector};
use cvr_physhands::person::PersonContact;
use cvr_settings::lc;

extern "C" {
    static g_isInVehicle: bool;
}
fn v3(p: &[f32]) -> Vector {
    Vector::new(p[0], p[1], p[2])
}
fn r4(q: &[f32]) -> Rotation {
    Rotation::new(q[0], q[1], q[2], q[3])
}
fn store3(p: &mut [f32], v: Vector) {
    p[0] = v.x;
    p[1] = v.y;
    p[2] = v.z;
}
fn store4(q: &mut [f32], r: Rotation) {
    q[0] = r.x;
    q[1] = r.y;
    q[2] = r.z;
    q[3] = r.w;
}
const LEFT_GRIP_PRESSED: usize = 155;
const RIGHT_GRIP_PRESSED: usize = 49;
unsafe fn grip_pressed(side: usize) -> bool {
    shared_hand(if side == 1 { RIGHT_GRIP_PRESSED } else { LEFT_GRIP_PRESSED }).is_some_and(|v| v > 0.5)
}

static mut PREVIOUS: [Reference; 2] = [Reference { position: Vector { x: 0.0, y: 0.0, z: 0.0 }, rotation: Rotation { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, owner: 0, origin: 0, valid: false }; 2];
static mut PREVIOUS_WAS_TRACKING: [bool; 2] = [false; 2];
static mut HAVE_ORIGIN: [bool; 2] = [false; 2];
static mut PREVIOUS_ORIGIN: [[f32; 3]; 2] = [[0.0; 3]; 2];
extern "C" {
    pub fn cvr_cpp_xr_tracking_origin_serial() -> u64;
}
/// PrepareRigBodies: both bodies and their previous targets moved together with the common tracking frame before any
/// contact query (the drive then sees only motion within that frame). ON SOMETHING TOO: the body rides the player's move
/// and turn along what it touches and away from it, only the part into it left to the drive (2026-11-16). Holding on
/// (climb) or holding a person the hand stays with what it holds.
unsafe fn prepare_rig_bodies(origin: &[f32; 3], s: &Settings, previous_tracking: &mut [[f32; 3]; 2], previous_tracking_valid: &[bool; 2]) {
    let space_origin = v3(origin);
    for side in 0..2 {
        let have_target = target_now(side, origin).is_some();
        let tracking = step_pose_inputs.frame.tracking.valid;
        let mut current = step_pose_inputs.frame.tracking;
        if !tracking && have_target {
            // seated the seat turns with the car: the bodies ride its turn too (on foot the entity's place alone)
            let seated = core::ptr::read_volatile(&raw const g_isInVehicle);
            let turn = if seated { super::pose::step_target(side).map_or(Rotation::default(), |t| r4(&t.entity_rot)) } else { Rotation::default() };
            current = handspace::make(v3(&target_entity[side]) + space_origin, turn, actor[side] as usize, cvr_cpp_xr_tracking_origin_serial());
        }
        let same_basis = tracking == PREVIOUS_WAS_TRACKING[side];
        let motion = if same_basis { handspace::between(&PREVIOUS[side], &current, RIDE_MOST) } else { Motion::default() };
        frame_riding[side] = have_target && s.ride != 0 && !climb_on[side] && !grab_on[side];
        let ride_along = frame_riding[side] && on_pushable[side];
        rode_by[side] = [0.0; 3];
        let old_origin = if HAVE_ORIGIN[side] { v3(&PREVIOUS_ORIGIN[side]) } else { space_origin };
        // Normalize cached physics-space points even when the scene rebases its origin. Their world positions do not
        // change with an origin shift.
        if sent_valid[side] {
            let v = v3(&sent[side][4..]) + old_origin - space_origin;
            store3(&mut sent[side][4..], v);
        }
        if previous_tracking_valid[side] {
            let v = v3(&previous_tracking[side]) + old_origin - space_origin;
            store3(&mut previous_tracking[side], v);
        }
        if !actor[side].is_null() && simulating[side] && frame_riding[side] && motion.valid {
            let mut pose = global_pose(actor[side]);
            let before = v3(&pose.p) + space_origin;
            let mut after = motion.point(before);
            if ride_along {
                // the ride's part into each face it touches taken out (a few rounds for a corner's two faces)
                let mut ways = [[0.0f32; 3]; 4];
                let n_ways = ways_out(side, &pose, &mut ways);
                let mut d = (after - before).to_array();
                for _ in 0..3 {
                    if n_ways == 0 {
                        break;
                    }
                    for w in &ways[..n_ways] {
                        let into = d[0] * w[0] + d[1] * w[1] + d[2] * w[2];
                        if into < 0.0 {
                            for k in 0..3 {
                                d[k] -= w[k] * into;
                            }
                        }
                    }
                }
                after = before + v3(&d);
                if n_ways != 0 {
                    RODE_ALONG[side].fetch_add(1, Relaxed);
                }
            }
            store3(&mut pose.p, after - space_origin);
            let turned = motion.orientation(r4(&pose.q));
            store4(&mut pose.q, turned);
            if qnorm(&mut pose.q) && handspace::finite(after) {
                let a = actor[side];
                at::<unsafe extern "C" fn(P, *const PxTransform, bool)>(a, px::SET_GLOBAL_POSE)(a, &pose, true);
                let mut velocity = [0.0f32; 3];
                let mut spin = [0.0f32; 3];
                at::<unsafe extern "C" fn(P, *mut f32)>(a, px::GET_LINEAR_VELOCITY)(a, velocity.as_mut_ptr());
                at::<unsafe extern "C" fn(P, *mut f32)>(a, px::GET_ANGULAR_VELOCITY)(a, spin.as_mut_ptr());
                velocity = motion.direction(v3(&velocity)).to_array();
                spin = motion.direction(v3(&spin)).to_array();
                set_velocities(a, &velocity, &spin, true);
                if sent_valid[side] {
                    let p = motion.point(v3(&sent[side][4..]) + space_origin) - space_origin;
                    store3(&mut sent[side][4..], p);
                    let q = motion.orientation(r4(&sent[side][..4]));
                    store4(&mut sent[side][..4], q);
                }
                if previous_tracking_valid[side] {
                    previous_tracking[side] = (motion.point(v3(&previous_tracking[side]) + space_origin) - space_origin).to_array();
                }
                gave_com[side] = motion.direction(v3(&gave_com[side])).to_array();
                let g0 = motion.direction(v3(&gave[side][..3]));
                store3(&mut gave[side][..3], g0);
                let g1 = motion.direction(v3(&gave[side][3..]));
                store3(&mut gave[side][3..], g1);
                pushed_out[side] = motion.direction(v3(&pushed_out[side])).to_array();
                rode_by[side] = (after - before).to_array();
                if handspace::length_squared(after - before) > 1e-12 {
                    RODE_STEPS[side].fetch_add(1, Relaxed);
                }
            }
        }
        PREVIOUS[side] = current;
        PREVIOUS_WAS_TRACKING[side] = tracking;
        PREVIOUS_ORIGIN[side] = *origin;
        HAVE_ORIGIN[side] = true;
    }
}

// ---- BeforeSimulate's own statics
static mut TRACK_WAS: [[f32; 3]; 2] = [[0.0; 3]; 2];
static mut TRACK_WAS_VALID: [bool; 2] = [false; 2];
static mut LAST_ENTITY: [f32; 3] = [0.0; 3];
static mut LAST_ENTITY_VALID: bool = false;
static mut PUT_UNTIL: u64 = 0;
static mut PUT_FROM: u64 = 0;
static mut PUT_AT: [[f32; 3]; 2] = [[0.0; 3]; 2];
static mut ENCLOSURE_TOLD_AT: [u64; 2] = [0; 2];
static mut GRIP_OFF_AT: [u64; 2] = [0; 2];
static mut GRIP_PRESS_AT: [u64; 2] = [0; 2];
static mut WORLD_FROM: [u64; 2] = [0; 2];
static mut WORLD_OFF_AT: [u64; 2] = [0; 2];
static mut WORLD_WAS: [bool; 2] = [false; 2];
static mut CATCHABLE: [bool; 2] = [false; 2];
static mut GRAB_TOLD_AT: [u64; 2] = [0; 2];
static mut HAD_EDGE_DRIVE: [bool; 2] = [false; 2];
static mut HELD_FROM: [u64; 2] = [0; 2];
static mut HELD_TOLD: [u64; 2] = [0; 2];
static mut HELD_END_TOLD: [u64; 2] = [0; 2];
static mut HELD_MOST: [f32; 2] = [0.0; 2];
static mut HELD_ON: [bool; 2] = [false; 2];
static mut LOOSE_HELD_FROM: [u64; 2] = [0; 2];
static mut LOOSE_TOLD_AT: [u64; 2] = [0; 2];
static mut PULL_TOLD_AT: u64 = 0;

/// BeforeSimulate: PhysX step hook, before PxScene::simulate, on the thread that steps (the scene is not simulating).
pub unsafe fn before_simulate(sc: P, dt: f32) {
    held_contact_step.begin(sc as usize, scene as usize);
    if !super::worker_up() || !px::prove() || sc.is_null() {
        return;
    }
    if dt > 1e-4 && dt < 0.1 {
        last_step_dt = dt;
    }
    let (t0, _) = qpc();
    let Some(origin) = super::scene_origin_of(sc) else { return }; // the hands live in the world scene only
    step_pose_inputs = reader_pose();
    super::hold::hold_steps(sc, &origin, dt);
    let fresh_w = WANTED.read().filter(|w| tick_ms().wrapping_sub(w.at) < 300);
    let fresh = fresh_w.is_some();
    let w = fresh_w.unwrap_or(Wanted::default());
    let s = settings::current();
    let mut track_was = TRACK_WAS;
    let track_was_valid = TRACK_WAS_VALID;
    prepare_rig_bodies(&origin, &s, &mut track_was, &track_was_valid);
    TRACK_WAS = track_was;
    let holding = super::heldbody::holding_mask();
    let tick = tick_ms();
    update_player_pace();
    loose_cap(&s);
    let step_dt = if dt > 1e-4 { dt } else { 1.0 / 60.0 };
    apart_step(sc, step_dt);
    drop_step(sc, step_dt);
    test_stack_step(sc, &origin, step_dt);
    people_step(&origin, &s);
    super::cabin::cabin_step(sc, &origin, w.on[0] || w.on[1]);
    // V'S OWN BODY AND THE OTHER HAND: the other hand met while nothing is held (a bottle passed from one to the other)
    // and both hands have bodies, with a gun out too: the free hand stops on the gun's boxes and lies on them; V's own
    // body while no gun is out (a reload reaches to the chest and the belt). Taken up only while out of them -- given a
    // body it is already in, PhysX throws the hand out of it
    {
        let armed = weapon_equipped();
        for side in 0..2 {
            let now = scripted(side);
            if now != scripted_on[side] {
                cvr_host::vlog!(
                    "[physhands] the {} hand is {}\n",
                    if side == 1 { "right" } else { "left" },
                    if now { "a script's: it meets neither the gun nor the other hand" } else { "its own again" }
                );
            }
            scripted_on[side] = now;
        }
        let want_hands = self_bits_ready && s.hands_meet != 0 && holding == 0 && !actor[0].is_null() && !actor[1].is_null() && simulating[0] && simulating[1] && !scripted_on[0] && !scripted_on[1];
        let hands_was = hands_meet_on;
        if !want_hands {
            hands_meet_on = false;
        } else if !hands_meet_on && hands_apart() {
            hands_meet_on = true;
        }
        if hands_meet_on != hands_was {
            cvr_host::vlog!("[physhands] the hands {} each other\n", if hands_meet_on { "meet" } else { "no longer meet" });
        }
        HANDS_MEET_SHOWN.store(hands_meet_on, Relaxed);
        for side in 0..2 {
            let want_gun = self_bits_ready && s.weapon != 0 && armed && holding == 0 && !actor[0].is_null() && !actor[1].is_null() && simulating[0] && simulating[1] && weapon_on[side] > 0 && !scripted_on[1 - side];
            let was = gun_meet_on[side];
            if !want_gun {
                gun_meet_on[side] = false;
            } else if !gun_meet_on[side] && !other_in_gun(side) {
                gun_meet_on[side] = true;
            }
            if gun_meet_on[side] != was {
                cvr_host::vlog!("[physhands] the gun in the {} hand {} the other hand\n", if side == 1 { "right" } else { "left" }, if gun_meet_on[side] { "meets" } else { "no longer meets" });
            }
        }
        for side in 0..2 {
            let want = self_bits_ready && s.self_ != 0 && !armed && !actor[side].is_null() && simulating[side];
            let was = self_on[side];
            if !want {
                self_on[side] = false;
            } else if !self_on[side] && !in_self(side) {
                self_on[side] = true;
            }
            if self_on[side] != was {
                cvr_host::vlog!("[physhands] the {} hand {} V's own body\n", if side == 1 { "right" } else { "left" }, if self_on[side] { "meets" } else { "no longer meets" });
            }
            SELF_ON_SHOWN[side].store(self_on[side], Relaxed);
        }
    }
    // THE PLAYER MOVED BY A TELEPORT (Bonelab resets its hands at the controllers when it teleports the rig): for a moment
    // both bodies are put where tracking is, step by step
    {
        let p = player_pos();
        let d2: f32 = (0..3).map(|k| (p[k] - LAST_ENTITY[k]) * (p[k] - LAST_ENTITY[k])).sum();
        if LAST_ENTITY_VALID && d2 > TELEPORT_STEP * TELEPORT_STEP {
            PUT_UNTIL = tick + TELEPORT_PUT_MS;
            PUT_FROM = tick;
            log!(
                "[physhands] the player moved {:.2} m in a step (a teleport): the hands are put where tracking is for {} ms or while it still jumps\n",
                d2.sqrt(),
                TELEPORT_PUT_MS
            );
        }
        LAST_ENTITY = p;
        LAST_ENTITY_VALID = (p[0] + p[1] + p[2]).is_finite();
    }
    // Park on either transition, even if the scanner opened and closed between two physics steps. Rearming therefore
    // uses steer's existing put-at-tracking path, never a pre-scan body or velocity.
    let scanner_epoch = cvr_input::scanner::SCANNER_HAND_PHYSICS.epoch();
    let scanner_reset = scanner_epoch != step_scanner_epoch;
    step_scanner_epoch = scanner_epoch;
    if scanner_reset {
        push_want = [0.0; 2];
        push_shown = [0.0; 2];
    }
    // BOTH HANDS' TRACKING THIS STEP, read before either is steered, and the last step's
    let mut track_is = [[0.0f32; 3]; 2];
    let mut track_is_valid = [false; 2];
    for k2 in 0..2 {
        if let Some(t) = target_now(k2, &origin) {
            track_is[k2] = t.p;
            track_is_valid[k2] = true;
        }
    }
    let live = lc!(xrHandPhysics) != 0 && lc!(xrHandCollision) != 0;
    for side in 0..2 {
        sent_grip_revision[side] = 0;
        let target = if !scanner_reset && cvr_input::scanner::SCANNER_HAND_PHYSICS.accepts(scanner_epoch) && live && fresh && w.on[side] && filter_ready { target_now(side, &origin) } else { None };
        let Some(mut to) = target else {
            // A parked body must release its climb/grab too, not keep pulling the player from a stale anchor.
            climb_on[side] = false;
            grab_on[side] = false;
            HOLDS_ON_SHOWN[side].store(false, Relaxed);
            HOLDING_ON[side].store(false, Relaxed);
            HOLD_POSES[side].write(&HoldPose::default());
            GRABS[side].write(&Grab::default());
            PERSON_CONTACTS[side].write(&PersonContact::default());
            GRIP_SPENT_SHOWN[side].store(false, Relaxed);
            grab_pull[side] = [0.0; 3];
            if grasp_given[side] && super::super::grasp::grasp_person(side, 0, None, None, 0.0, 0.0).is_some() {
                grasp_given[side] = false;
            }
            park(side);
            touch_until[side] = 0;
            on_world[side] = false;
            on_pushable[side] = false;
            grip_was[side] = grip_pressed(side);
            far_since[side] = 0;
            continue;
        };
        if actor[side].is_null() && !make(side, &to, &s) {
            park(side);
            continue;
        }
        // the settings that can change on a made body
        if applied_mass[side] != s.mass {
            set_mass_at(actor[side], s.mass, &made_half[side], &made_centre[side]);
            applied_mass[side] = s.mass;
        }
        if applied_ccd[side] != s.ccd {
            // swept and speculative are not both taken (PhysX 3.4 drops the speculative one): the one off first
            let a = actor[side];
            let flag = |f: i32, on: bool| at::<unsafe extern "C" fn(P, i32, bool)>(a, px::SET_RIGID_BODY_FLAG)(a, f, on);
            if s.ccd != 1 {
                flag(px::ENABLE_CCD, false);
            }
            if s.ccd != 2 {
                flag(px::ENABLE_SPECULATIVE_CCD, false);
            }
            if s.ccd == 1 {
                flag(px::ENABLE_CCD, true);
            }
            if s.ccd == 2 {
                flag(px::ENABLE_SPECULATIVE_CCD, true);
            }
            applied_ccd[side] = s.ccd;
            log!(
                "[physhands] {} hand: continuous collision {}\n",
                if side == 1 { "right" } else { "left" },
                match s.ccd {
                    1 => "swept",
                    2 => "speculative",
                    _ => "off",
                }
            );
        }
        if applied_contact_offset[side] != s.contact_offset && !shape[side].is_null() {
            let c = if s.contact_offset > 0.0 { s.contact_offset } else { own_contact_offset[side] };
            let set = |sh: P| {
                if !sh.is_null() {
                    at::<unsafe extern "C" fn(P, f32)>(sh, px::SHAPE_SET_CONTACT_OFFSET)(sh, c);
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
            applied_contact_offset[side] = s.contact_offset;
            log!("[physhands] {} hand's shapes: contact offset {:.3} m (PhysX's own {:.3})\n", if side == 1 { "right" } else { "left" }, c, own_contact_offset[side]);
        }
        if applied_depenetration[side] != s.depenetration {
            at::<unsafe extern "C" fn(P, f32)>(actor[side], px::SET_MAX_DEPENETRATION)(actor[side], s.depenetration);
            applied_depenetration[side] = s.depenetration;
        }
        // THE PALM AND WHAT IT CLOSES ON. A hand closing on a thing (the grip past half), or holding one by its body, does
        // not meet the loose things: the one taken would fight it and its neighbours would be shoved. It meets them again
        // a moment after it opens, and only once it is in none of them: given them back inside a thing, PhysX threw it.
        let vehicle_grip = super::vehicle_hands::busy(side);
        let grip = !vehicle_grip && grip_pressed(side);
        set_materials(side, grip);
        if grip || holding & (1 << side) != 0 {
            loose_from[side] = tick + (s.loose_after_grip * 1000.0) as u64;
        }
        let mut meet = tick >= loose_from[side];
        {
            let at_pose = global_pose(actor[side]);
            let was = enclosed[side].n != 0;
            enclosed[side] = enclosure_groups(side, &at_pose, Some(&origin));
            let inside = enclosed[side].n != 0;
            if inside {
                ENCLOSURE_STEPS[side].fetch_add(1, Relaxed);
            }
            if inside != was && tick.wrapping_sub(ENCLOSURE_TOLD_AT[side]) >= 1000 {
                ENCLOSURE_TOLD_AT[side] = tick;
                let mask = u64::from(hand_filter[2]) | (u64::from(hand_filter[3]) << 32);
                log!(
                    "[physhands] {} palm {} an enclosure (groups {:016x}): the hand gives up {:016x}\n",
                    if side == 1 { "right" } else { "left" },
                    if inside { "inside" } else { "out of" },
                    if inside { enclosed[side].group[0] } else { 0 },
                    if inside { safe_to_drop(mask, &enclosed[side]) } else { 0 }
                );
            }
        }
        if meet && !loose_met[side] {
            let at_pose = global_pose(actor[side]);
            if palm_in_loose(side, &at_pose) {
                meet = false;
                loose_from[side] = tick + 50;
                PALM_REFUSED.fetch_add(1, Relaxed);
            }
        }
        loose_met[side] = meet;
        {
            let (palm_f, finger_f, gun_f, fore_f) = hand_filters(side, meet);
            set_filter(side, &palm_f, &gun_f, &finger_f, &fore_f);
        }
        pose_shapes(side, &s);
        if tick < PUT_UNTIL {
            // the frames of a teleport move tracking's place about: put on while it still jumps, so the last put is where
            // it settles
            let j2: f32 = (0..3).map(|k| (to.p[k] - PUT_AT[side][k]) * (to.p[k] - PUT_AT[side][k])).sum();
            if j2 > 0.02 * 0.02 && tick - PUT_FROM < TELEPORT_PUT_MOST_MS {
                PUT_UNTIL = PUT_UNTIL.max(tick + 100);
            }
            PUT_AT[side] = to.p;
        }
        sent_track[side] = to.p;
        sent_entity[side] = target_entity[side];
        // THE HAND THAT HOLDS ON: the grip closed on a top (the way out of it up), on an edge the fingers curl over or on
        // a corner -- not on a wall's face -- and the body stays where it took hold while the grip lasts; the player
        // follows (below). The grip read through a 150 ms let-go. Taken hold of within 0.6 s of the press, with the hand
        // within 1.5 cm of the world. A grip held as the hand comes onto a top or an edge catches it too (no gun out). A
        // hold pulled off by itself spends the grip: it takes nothing more until it is let go.
        if grip {
            GRIP_OFF_AT[side] = 0;
        } else if GRIP_OFF_AT[side] == 0 {
            GRIP_OFF_AT[side] = tick;
        }
        if grip && !grip_was[side] {
            GRIP_PRESS_AT[side] = tick;
        }
        let hold_grip = !vehicle_grip && (grip || (GRIP_OFF_AT[side] != 0 && tick - GRIP_OFF_AT[side] < 150));
        if !hold_grip {
            grip_spent[side] = false;
        }
        // a touch that comes after 0.3 s off the world is the hand coming onto something; the touch's own flicker is not
        if on_world[side] && !WORLD_WAS[side] {
            WORLD_FROM[side] = tick;
            CATCHABLE[side] = WORLD_OFF_AT[side] != 0 && tick - WORLD_OFF_AT[side] >= 300;
        }
        if !on_world[side] && WORLD_WAS[side] {
            WORLD_OFF_AT[side] = tick;
        }
        WORLD_WAS[side] = on_world[side];
        let press_fresh = grip && !grip_spent[side] && tick.wrapping_sub(GRIP_PRESS_AT[side]) < 600;
        let caught = grip && !grip_spent[side] && !weapon_equipped() && on_world[side] && CATCHABLE[side] && tick - WORLD_FROM[side] < 150 && GRIP_PRESS_AT[side] + 150 < WORLD_FROM[side];
        let body_now = global_pose(actor[side]);
        self::grab_step(side, &s, &origin, &mut to, &body_now, hold_grip, press_fresh, holding, dt, tick);
        if !hold_grip && climb_on[side] {
            climb_on[side] = false;
            log!("[physhands] {} hand lets go (the grip opened 150 ms ago)\n", if side == 1 { "right" } else { "left" });
        } else if !grab_on[side] && !climb_on[side] && (press_fresh || caught) && s.climb != 0 && holding & (1 << side) == 0 {
            let feature = hold_feature(side, &body_now, &origin);
            if feature != Feature::None {
                for k in 0..3 {
                    climb_anchor[side][k] = body_now.p[k] + origin[k];
                }
                climb_turn[side] = body_now.q;
                climb_on[side] = true;
                log!(
                    "[physhands] {} hand holds on ({}{}) at {:.2} {:.2} {:.2}\n",
                    if side == 1 { "right" } else { "left" },
                    if feature == Feature::Top {
                        if on_world[side] {
                            "a top"
                        } else {
                            "a top within reach"
                        }
                    } else {
                        "an edge or a corner"
                    },
                    if caught {
                        ", caught with the grip held"
                    } else if tick - GRIP_PRESS_AT[side] > 30 {
                        ", after the press"
                    } else {
                        ""
                    },
                    climb_anchor[side][0],
                    climb_anchor[side][1],
                    climb_anchor[side][2]
                );
            }
        }
        grip_was[side] = grip;
        HOLDS_ON_SHOWN[side].store(climb_on[side] || grab_on[side], Relaxed);
        HOLDING_ON[side].store(climb_on[side], Relaxed);
        HOLD_POSES[side].write(&HoldPose { on: climb_on[side], p: climb_anchor[side], q: climb_turn[side], at: tick_ms() });
        GRIP_SPENT_SHOWN[side].store(grip_spent[side], Relaxed);
        {
            let g = Grab { owner: if grab_on[side] { grab_owner[side] } else { 0 }, pull: grab_pull[side], at: tick, serial: grab_serial[side] };
            GRABS[side].write(&g);
            let mut contact = PersonContact::default();
            if g.owner != 0 {
                contact.owner = g.owner;
                contact.serial = g.serial;
                contact.at = tick;
                for k in 0..3 {
                    contact.position[k] = grab_last[side].p[k] + origin[k];
                }
                contact.rotation = grab_last[side].q;
                if let Some((cap, radius, half)) = person_shape(g.owner) {
                    contact.radius = radius;
                    contact.half = half;
                    for k in 0..3 {
                        contact.capsule[k] = cap.p[k] + origin[k];
                    }
                    contact.capsule_rotation = cap.q;
                    contact.ready = grasp_given[side] && !grab_seating[side];
                }
            }
            PERSON_CONTACTS[side].write(&contact);
        }
        if climb_on[side] {
            for k in 0..3 {
                to.p[k] = climb_anchor[side][k] - origin[k];
            }
            to.q = climb_turn[side];
        }
        let edge_drive = tick >= PUT_UNTIL && !climb_on[side] && !grab_on[side] && holding & (1 << side) == 0 && super::blade::blade_edge_target(side, sc, &body_now, s.teleport * 0.5, &mut to);
        if edge_drive != HAD_EDGE_DRIVE[side] {
            sent_valid[side] = false;
            HAD_EDGE_DRIVE[side] = edge_drive;
        }
        if s.arm_reach != 0 && !edge_drive && !climb_on[side] && !grab_on[side] {
            arm_reach(side, &mut to, &origin, tick, &s);
        }
        if edge_drive {
            far_since[side] = 0;
        }
        let put = tick < PUT_UNTIL || (!edge_drive && unstick(side, &to, &origin, holding, tick, &s));
        // where the last step sent the body: a free body ended that step there, so its distance from it is what held it
        let prev_sent = [sent[side][4], sent[side][5], sent[side][6]];
        let had_sent = sent_valid[side];
        // THE OTHER HAND THIS ONE PRESSES ON: tracking's way into it taken out of where the body is sent, as a wall's is
        // -- so a hand pushed at the other stops on it and the other, held still, is not shoved. AGAINST THE OTHER HAND'S
        // TRACKING, not the player's pace: a move both hands make together is no press; the pace stays the fallback.
        if hands_meet_on && !put {
            let other = 1 - side;
            let mut together = [player_pace[0] * step_dt, player_pace[1] * step_dt, player_pace[2] * step_dt];
            if track_is_valid[other] && TRACK_WAS_VALID[other] {
                let m2: f32 = (0..3).map(|k| (track_is[other][k] - TRACK_WAS[other][k]) * (track_is[other][k] - TRACK_WAS[other][k])).sum();
                if m2 < 0.5 * 0.5 {
                    for k in 0..3 {
                        together[k] = track_is[other][k] - TRACK_WAS[other][k];
                    }
                }
            }
            let mut n = [[0.0f32; 4]; 24];
            let ways = other_hand_out(side, &body_now, &mut n);
            for _ in 0..2 {
                if ways == 0 {
                    break;
                }
                for w in &n[..ways] {
                    let mut into = 0.0f32;
                    for k in 0..3 {
                        into -= (to.p[k] - body_now.p[k] - together[k]) * w[k];
                    }
                    let past = into - w[3];
                    if past > 0.0 {
                        for k in 0..3 {
                            to.p[k] += w[k] * past;
                        }
                    }
                }
            }
        }
        steer(side, &to, step_dt, &s, put);
        sent[side][..4].copy_from_slice(&to.q);
        sent[side][4..].copy_from_slice(&to.p);
        sent_valid[side] = true;
        // HELD OFF WHERE TRACKING IS FREE (always on, a diagnostic)
        if had_sent {
            let now = global_pose(actor[side]);
            let gap = (0..3).map(|k| (prev_sent[k] - now.p[k]) * (prev_sent[k] - now.p[k])).sum::<f32>().sqrt();
            {
                // HELD BY A LOOSE THING, THE HAND GOES THROUGH IT (HIGGS turns a hand's contacts off for a moment after a
                // throw): more than 4 cm for 150 ms, not holding it: the palm meets no loose thing for 0.4 s, and again
                // only once it is out of them.
                let by_loose = tick < touch_until[side] && !on_world[side] && gap.is_finite() && gap > 0.04 && !put && holding & (1 << side) == 0 && loose_met[side];
                if !by_loose {
                    LOOSE_HELD_FROM[side] = 0;
                } else if LOOSE_HELD_FROM[side] == 0 {
                    LOOSE_HELD_FROM[side] = tick;
                } else if tick - LOOSE_HELD_FROM[side] >= 150 {
                    loose_from[side] = tick + 400;
                    LOOSE_HELD_FROM[side] = 0;
                    LOOSE_LET_THROUGH.fetch_add(1, Relaxed);
                    if tick.wrapping_sub(LOOSE_TOLD_AT[side]) >= 1000 {
                        LOOSE_TOLD_AT[side] = tick;
                        log!("[physhands] {} hand held {:.0} mm off by a loose thing for 150 ms: let through it for 0.4 s\n", if side == 1 { "right" } else { "left" }, 1000.0 * gap);
                    }
                }
            }
            let mut held = gap.is_finite() && gap > 0.03 && !put && holding & (1 << side) == 0;
            if held {
                held = !touches_world(side, &to, &s, TOUCH_ALL, None) && !palm_in_loose(side, &to);
            }
            if held {
                if HELD_FROM[side] == 0 {
                    HELD_FROM[side] = tick;
                    HELD_MOST[side] = 0.0;
                }
                HELD_MOST[side] = HELD_MOST[side].max(gap);
                if !HELD_ON[side] && tick - HELD_FROM[side] >= 250 {
                    HELD_ON[side] = true;
                    if tick.wrapping_sub(HELD_TOLD[side]) >= 1000 {
                        HELD_TOLD[side] = tick;
                        let off = made_centre[side];
                        let r = qrot(&now.q, &off);
                        let a = [now.p[0] + r[0], now.p[1] + r[1], now.p[2] + r[2]];
                        let r = qrot(&to.q, &off);
                        let b = [to.p[0] + r[0], to.p[1] + r[1], to.p[2] + r[2]];
                        let reach: f32 = (0..3).map(|k| (sent_track[side][k] - to.p[k]) * (sent_track[side][k] - to.p[k])).sum();
                        let met = match path_free(&a, &b, 0.008) {
                            Some(Err(m)) => m,
                            _ => "nothing".into(),
                        };
                        log!(
                            "[physhands] {} hand held {:.0} mm off tracking {} ms with tracking's palm in nothing: on the world {}, touching {}, the arm's reach took {:.0} mm, settling {:.2} {:.2}; between the palms: {}\n",
                            if side == 1 { "right" } else { "left" },
                            1000.0 * gap,
                            tick - HELD_FROM[side],
                            i32::from(on_world[side]),
                            i32::from(tick < touch_until[side]),
                            1000.0 * reach.sqrt(),
                            settle_lin[side],
                            settle_ang[side],
                            met
                        );
                    }
                }
            } else if HELD_FROM[side] != 0 {
                if HELD_ON[side] && tick.wrapping_sub(HELD_END_TOLD[side]) >= 1000 {
                    HELD_END_TOLD[side] = tick;
                    log!(
                        "[physhands] {} hand held off tracking for {} ms, {:.0} mm at the most, now {:.0} mm off\n",
                        if side == 1 { "right" } else { "left" },
                        tick - HELD_FROM[side],
                        1000.0 * HELD_MOST[side],
                        1000.0 * gap
                    );
                }
                HELD_FROM[side] = 0;
                HELD_ON[side] = false;
            }
        }
        // THE HAND'S PUSH ON THE BODY: how far tracking is held into the world, along the way out of it, on the floor --
        // only along the floor, the way out of the surface laid flat, scaled by how upright the surface is
        if s.body_push != 0 && on_pushable[side] && !put && holding & (1 << side) == 0 && !climb_on[side] && !grab_on[side] {
            let now = global_pose(actor[side]);
            let n = pushed_out[side];
            let nh = (n[0] * n[0] + n[1] * n[1]).sqrt();
            if nh > 0.2 {
                let into = -((to.p[0] - now.p[0]) * n[0] + (to.p[1] - now.p[1]) * n[1]) / nh;
                let past = into - s.body_push_dead;
                if past > 0.0 && past.is_finite() {
                    for k in 0..2 {
                        push_want[k] += n[k] * (s.body_push_gain * past).min(s.body_push_most);
                    }
                }
            }
        }
    }
    for k2 in 0..2 {
        TRACK_WAS_VALID[k2] = track_is_valid[k2];
        if track_is_valid[k2] {
            TRACK_WAS[k2] = track_is[k2];
        }
    }
    {
        // eased (80 ms), both hands' together, at most body_push_most; published for the player's move
        let w = 1.0 - (-step_dt / 0.08).exp();
        let m2 = push_want[0] * push_want[0] + push_want[1] * push_want[1];
        if m2 > s.body_push_most * s.body_push_most {
            for c in &mut push_want {
                *c *= s.body_push_most / m2.sqrt();
            }
        }
        for k in 0..2 {
            push_shown[k] += (push_want[k] - push_shown[k]) * w;
            push_want[k] = 0.0;
        }
        BODY_PUSHES.write(&Push { v: push_shown, at: tick });
    }
    {
        // THE PLAYER PULLED AFTER THE HANDS THAT HOLD ON: so that tracking comes back to where each hand took hold
        // (closed in 60 ms, at most climb_most m/s), both hands' together; the feet rise no higher than 10 cm over the
        // highest hold -- onto a railing, not on up past it
        let mut want = [0.0f32; 3];
        let mut top_z = -1e9f32;
        let mut holds = 0;
        for side in 0..2 {
            if !climb_on[side] {
                continue;
            }
            for k in 0..3 {
                want[k] += (climb_anchor[side][k] - (sent_track[side][k] + origin[k])) / 0.06;
            }
            top_z = top_z.max(climb_anchor[side][2]);
            holds += 1;
        }
        let mut p = Pull { at: tick, on: holds > 0, ..Default::default() };
        if holds != 0 {
            for c in &mut want {
                *c /= holds as f32;
            }
            let room = top_z + 0.10 - player_pos()[2];
            if want[2] > 0.0 {
                want[2] = want[2].min(room.max(0.0) / step_dt);
            }
            let m = (want[0] * want[0] + want[1] * want[1] + want[2] * want[2]).sqrt();
            if m > s.climb_most {
                for c in &mut want {
                    *c *= s.climb_most / m;
                }
            }
            if m.is_finite() {
                p.v = want;
            }
            if s.trace != 0 && tick.wrapping_sub(PULL_TOLD_AT) >= 250 {
                PULL_TOLD_AT = tick;
                log!("[physhands] holding on: pull {:.2} {:.2} {:.2} m/s, the feet {:.2} m under the highest hold\n", p.v[0], p.v[1], p.v[2], top_z - player_pos()[2]);
            }
        }
        BODY_PULLS.write(&p);
    }
    let allowed = fresh && !scanner_reset && tick >= PUT_UNTIL && weapon_equipped() && !core::ptr::read_volatile(&raw const g_isInVehicle) && super::census::menu_mode() == 0 && cvr_input::scanner::SCANNER_HAND_PHYSICS.accepts(scanner_epoch);
    super::blade::step_blade_joint(sc, &origin, dt, allowed, holding);
    charge(&BEFORE_US, &BEFORE_MOST_US, t0);
}

/// A PERSON TAKEN HOLD OF: the grip closed with the palm or the fingers on a part of someone (the script names each of
/// their capsules) -- the hand stays on that part as it moves, a little way toward the controller (grab_give, eased in),
/// and lets go by itself once the controller is grab_release from it. And the fingers on the part held.
unsafe fn grab_step(side: usize, s: &Settings, origin: &[f32; 3], to: &mut PxTransform, body_now: &PxTransform, hold_grip: bool, press_fresh: bool, holding: i32, dt: f32, tick: u64) {
    let name = if side == 1 { "right" } else { "left" };
    if grab_on[side] {
        let attached = read_person_attachment(side).filter(|a| a.owner == grab_owner[side]);
        let exact = attached.is_some();
        let cap = person_pose(grab_owner[side]);
        let seen = cap.is_some();
        if seen {
            grab_seen_at[side] = tick;
        }
        let mut off = !hold_grip;
        let mut why = "the grip opened 150 ms ago";
        if !off && attached.is_some_and(|a| a.release) {
            off = true;
            grip_spent[side] = true;
            why = "the bone attachment ended or became unreachable";
        }
        if !off && !seen && tick - grab_seen_at[side] > 250 {
            off = true;
            why = "the person is gone";
        }
        if !off {
            let mut hold = grab_last[side];
            if grab_seating[side] {
                let w = grab_seat[side];
                let h = &mut grab_hand[side];
                let k = 1.0 - (-(if dt > 1e-4 { dt } else { 1.0 / 60.0 }) / SEAT_TAU).exp();
                let sgn = if h.q[0] * w.q[0] + h.q[1] * w.q[1] + h.q[2] * w.q[2] + h.q[3] * w.q[3] < 0.0 { -1.0 } else { 1.0 };
                let mut left = 0.0f32;
                for k3 in 0..3 {
                    h.p[k3] += (w.p[k3] - h.p[k3]) * k;
                    left = left.max((w.p[k3] - h.p[k3]).abs());
                }
                for k4 in 0..4 {
                    h.q[k4] += (w.q[k4] * sgn - h.q[k4]) * k;
                }
                qnorm(&mut h.q);
                if left < 0.0005 {
                    *h = w;
                    grab_seating[side] = false;
                }
            }
            if let Some(a) = attached.filter(|a| a.on) {
                sent_grip_revision[side] = a.revision;
                for k in 0..3 {
                    hold.p[k] = a.position[k] - origin[k];
                }
                hold.q = a.rotation;
            } else if let Some(cap) = cap {
                let axis = qrot(&cap.q, &[1.0, 0.0, 0.0]);
                let a0 = grab_axis[side];
                let c = a0[0] * axis[0] + a0[1] * axis[1] + a0[2] * axis[2];
                let cr = cross(&a0, &axis);
                let mut swing = [cr[0], cr[1], cr[2], 1.0 + c];
                if !(c > -0.95) || !qnorm(&mut swing) {
                    swing = [0.0, 0.0, 0.0, 1.0];
                }
                let rel = [grab_hand[side].p[0] - grab_mid[side][0], grab_hand[side].p[1] - grab_mid[side][1], grab_hand[side].p[2] - grab_mid[side][2]];
                let r = qrot(&swing, &rel);
                for k in 0..3 {
                    hold.p[k] = cap.p[k] + r[k];
                }
                hold.q = qmul(&swing, &grab_hand[side].q);
                qnorm(&mut hold.q);
                // trace=1: the hold as it is put, every 150 ms (the game's world)
                if s.trace != 0 && tick.wrapping_sub(GRAB_TOLD_AT[side]) >= 150 {
                    GRAB_TOLD_AT[side] = tick;
                    let moved = (0..3).map(|k| (cap.p[k] - grab_mid[side][k]).powi(2)).sum::<f32>().sqrt();
                    log!(
                        "[physhands] {} hold on part {}: capsule middle {:.3} {:.3} {:.3} (moved {:.3}) axis {:.2} {:.2} {:.2} (swung {:.0} deg); hold {:.3} {:.3} {:.3}, controller {:.3} {:.3} {:.3}\n",
                        name,
                        grab_owner[side] % 32,
                        cap.p[0] + origin[0],
                        cap.p[1] + origin[1],
                        cap.p[2] + origin[2],
                        moved,
                        axis[0],
                        axis[1],
                        axis[2],
                        2.0 * swing[3].abs().min(1.0).acos() / 0.017_453_3,
                        hold.p[0] + origin[0],
                        hold.p[1] + origin[1],
                        hold.p[2] + origin[2],
                        to.p[0] + origin[0],
                        to.p[1] + origin[1],
                        to.p[2] + origin[2]
                    );
                }
            }
            grab_last[side] = hold;
            let d = [to.p[0] - hold.p[0], to.p[1] - hold.p[1], to.p[2] - hold.p[2]];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            grab_pull[side] = d;
            if !(dist <= s.grab_release) {
                off = true;
                grip_spent[side] = true;
                why = "the controller pulled away from it";
            } else {
                let give = if !exact && s.grab_give > 1e-4 { s.grab_give * (1.0 - (-dist / s.grab_give).exp()) } else { 0.0 };
                let k = if dist > 1e-4 { give / dist } else { 0.0 };
                for i in 0..3 {
                    to.p[i] = hold.p[i] + d[i] * k;
                }
                // turned as the part is, a quarter of the way to the controller's turn
                let sgn = if hold.q[0] * to.q[0] + hold.q[1] * to.q[1] + hold.q[2] * to.q[2] + hold.q[3] * to.q[3] < 0.0 { -1.0 } else { 1.0 };
                let mut q = [0.0f32; 4];
                for i in 0..4 {
                    q[i] = hold.q[i] + (to.q[i] * sgn - hold.q[i]) * (if exact { 0.0 } else { 0.25 });
                }
                if qnorm(&mut q) {
                    to.q = q;
                }
            }
        }
        if off {
            grab_on[side] = false;
            let gp = grab_pull[side];
            log!(
                "[physhands] {} hand lets go of a person ({}; their part {}, {:.2} m from the controller)\n",
                name,
                why,
                grab_owner[side] % 32,
                (gp[0] * gp[0] + gp[1] * gp[1] + gp[2] * gp[2]).sqrt()
            );
        }
    } else if press_fresh && s.grab_people != 0 && !climb_on[side] && holding & (1 << side) == 0 {
        if let Some(index) = person_at(side, body_now) {
            let cap = person_near[index].pose;
            grab_axis[side] = qrot(&cap.q, &[1.0, 0.0, 0.0]);
            grab_mid[side] = cap.p;
            grab_hand[side] = *body_now;
            grab_on[side] = true;
            grab_seating[side] = false;
            grab_owner[side] = person_owner[index];
            grab_serial[side] += 1;
            grab_seen_at[side] = tick;
            grab_last[side] = *body_now;
            let d2: f32 = (0..3).map(|k| (to.p[k] - body_now.p[k]) * (to.p[k] - body_now.p[k])).sum();
            log!(
                "[physhands] {} hand takes hold of a person (their part {}, id {}), {:.2} m from the controller\n",
                name,
                grab_owner[side] % 32,
                grab_owner[side] / 32,
                d2.sqrt()
            );
            to.p = body_now.p;
        }
    }
    if !grab_on[side] {
        grab_pull[side] = [0.0; 3];
    }
    // the fingers on the part held: its capsule in the hand bone's frame (the body's), its axis x turned to z
    let style = if grab_on[side] && GRAB_STYLE_OWNER[side].load(Relaxed) == grab_owner[side] { GRAB_STYLE[side].load(Relaxed) } else { 0 };
    let shape_now = if style > 0 { person_shape(grab_owner[side]) } else { None };
    if let Some((cap, radius, half)) = shape_now {
        // in the hold's frame (the hand as the hold puts it, before the give toward the controller)
        let hf = grab_last[side];
        let inv = [-hf.q[0], -hf.q[1], -hf.q[2], hf.q[3]];
        let ry = [0.0f32, 0.707_106_77, 0.0, 0.707_106_77];
        let d = [cap.p[0] - hf.p[0], cap.p[1] - hf.p[1], cap.p[2] - hf.p[2]];
        let p = qrot(&inv, &d);
        let q = qmul(&inv, &cap.q);
        let mut qz = qmul(&q, &ry);
        if qnorm(&mut qz) {
            if let Some((seat_p, seat_q, seated)) = super::super::grasp::grasp_person(side, style, Some(&p), Some(&qz), half, radius) {
                grasp_given[side] = true;
                // SEATED: the hand moved by X = (the part in the hand) * (its seat)^-1, in the hand's frame -- the hold
                // as it is this step taken on the capsule as it is now, and eased to itself times X
                if seated && read_person_attachment(side).is_none_or(|a| !a.on) {
                    let sc = [-seat_q[0], -seat_q[1], -seat_q[2], seat_q[3]];
                    let mut xq = qmul(&qz, &sc);
                    qnorm(&mut xq);
                    let t = qrot(&xq, &seat_p);
                    let xp = [p[0] - t[0], p[1] - t[1], p[2] - t[2]];
                    grab_axis[side] = qrot(&cap.q, &[1.0, 0.0, 0.0]);
                    grab_mid[side] = cap.p;
                    grab_hand[side] = grab_last[side];
                    let w = &mut grab_seat[side];
                    w.q = qmul(&grab_last[side].q, &xq);
                    qnorm(&mut w.q);
                    let t = qrot(&grab_last[side].q, &xp);
                    for k in 0..3 {
                        w.p[k] = grab_last[side].p[k] + t[k];
                    }
                    grab_seating[side] = true;
                    log!(
                        "[physhands] {} hand seats on the person's part {} for its grasp: moved {:.3} m, turned {:.0} deg\n",
                        name,
                        grab_owner[side] % 32,
                        (xp[0] * xp[0] + xp[1] * xp[1] + xp[2] * xp[2]).sqrt(),
                        2.0 * xq[3].abs().min(1.0).acos() / 0.017_453_3
                    );
                }
            }
        }
    } else if grasp_given[side] && super::super::grasp::grasp_person(side, 0, None, None, 0.0, 0.0).is_some() {
        grasp_given[side] = false;
    }
}

static mut ASKED_AT: u64 = 0;
static mut SERIAL: u64 = 0;
/// AfterFetch: PhysX step hook, after PxScene::fetchResults handed over this step's results -- the world scene found
/// (again every so often: a load can bring another), the census, the step's poses published for the worker, the slices
/// of the world and loose lists walked.
pub unsafe fn after_fetch(sc: P) {
    let contact = held_contact_step.take(sc as usize);
    super::worker::start_worker();
    if !px::prove() || sc.is_null() {
        return;
    }
    let (t0, _) = qpc();
    let now = tick_ms();
    if scene.is_null() || now - ASKED_AT > 2000 {
        ASKED_AT = now;
        let mut o = [0.0f32; 3];
        let world = px::world_scene(&mut o);
        if world != scene {
            super::blade::release_blade_joint();
            if !scene.is_null() {
                log!("[physhands] the world scene changed ({:p} -> {:p}): the hands start again\n", scene, world);
            }
            scene = world;
            // the bodies of a released scene were taken out of it, not released; they are put in the new one when made
            // again
            for side in 0..2 {
                actor[side] = core::ptr::null_mut();
                shape[side] = core::ptr::null_mut();
                simulating[side] = false;
                LIVE[side].store(false, SeqCst);
            }
            for i in 0..super::people::MOST_BODIES {
                super::people::person_actor[i] = core::ptr::null_mut();
                super::people::person_shape[i] = core::ptr::null_mut();
                super::people::person_live[i] = false;
            }
            super::people::person_near_n = 0;
            super::cabin::forget_for_scene();
            super::census::census = super::census::Census::NEW;
            super::census::census_tries_here = 0;
            super::census::filter_ready = false;
            super::near::reset_for_scene();
            super::loose::reset_for_scene();
            for side in 0..2 {
                finger_shape[side] = [[core::ptr::null_mut(); 3]; 5];
                forearm_shape[side] = core::ptr::null_mut();
                forearm_upper[side] = core::ptr::null_mut();
                made_skin[side] = -2;
                weapon_shape[side] = [core::ptr::null_mut(); MOST_WEAPON_BOXES];
                weapon_on[side] = 0;
            }
        }
    }
    let Some(origin) = super::scene_origin_of(sc) else { return };
    super::hold::publish_held_wrists(now, &origin, &contact);
    {
        use super::census::*;
        if census.phase == 3 && census.statics < THIN_CENSUS && player_valid() {
            let p = player_pos();
            let moved2: f32 = (0..3).map(|k| (p[k] - census.from[k]) * (p[k] - census.from[k])).sum();
            let elsewhere = moved2 > CENSUS_REDO_MOVE * CENSUS_REDO_MOVE && now - census_done_at > 2000;
            let waited = now - census_done_at > CENSUS_REDO_WAIT_MS && census_tries_here < CENSUS_REDO_TRIES;
            if moved2.is_finite() && (elsewhere || waited) {
                census_tries_here = if elsewhere { 0 } else { census_tries_here + 1 };
                log!(
                    "[physhands] the census saw {} static shapes{}: taken again ({}, {:.0} m away)\n",
                    census.statics,
                    if filter_ready { "" } else { " and nothing to meet" },
                    if elsewhere { "the player is elsewhere" } else { "the world had time to come in" },
                    moved2.sqrt()
                );
                census = Census::NEW;
            }
        }
        if census.phase < 3 {
            census_slice(sc, &origin);
            // the first census: no filters, nothing to step yet; one taken again: the hands go on with the old filters
            if !filter_ready {
                charge(&AFTER_US, &AFTER_MOST_US, t0);
                return;
            }
        }
    }
    let mut st: super::types::Step = core::mem::zeroed();
    st.scanner_epoch = step_scanner_epoch;
    LIVE_SCANNER_EPOCH.store(st.scanner_epoch, Release);
    for side in 0..2 {
        st.valid[side] = !actor[side].is_null() && simulating[side] && sent_valid[side] && cvr_input::scanner::SCANNER_HAND_PHYSICS.accepts(st.scanner_epoch);
        if !st.valid[side] {
            continue;
        }
        let p = global_pose(actor[side]);
        st.pose[side][..4].copy_from_slice(&p.q);
        st.pose[side][4..].copy_from_slice(&p.p);
        st.sent[side] = sent[side];
        st.sent_track[side] = sent_track[side];
        st.sent_entity[side] = sent_entity[side];
        LIVE[side].store(true, Release);
    }
    st.frame = step_pose_inputs.frame;
    st.entity_rot = if st.frame.entity.valid { st.frame.entity.rotation.to_array() } else { super::pose::pose_entity_now().1 };
    st.origin = origin;
    for side in 0..2 {
        st.touching[side] = st.valid[side] && now < touch_until[side];
        st.on_world[side] = st.touching[side] && on_world[side];
        st.riding[side] = st.valid[side] && frame_riding[side];
        st.grip_revision[side] = sent_grip_revision[side];
        st.motor[side] = motor_probes[side];
        if !st.valid[side] {
            continue;
        }
        let a = actor[side];
        at::<unsafe extern "C" fn(P, *mut f32)>(a, px::GET_LINEAR_VELOCITY)(a, st.vel[side].as_mut_ptr());
        at::<unsafe extern "C" fn(P, *mut f32)>(a, px::GET_ANGULAR_VELOCITY)(a, st.spin[side].as_mut_ptr());
        {
            // the wrist's velocity, as the steering has it (PhysX's is the palm's middle's)
            let com = qrot(&[st.pose[side][0], st.pose[side][1], st.pose[side][2], st.pose[side][3]], &made_centre[side]);
            let wc = cross(&st.spin[side], &com);
            for k in 0..3 {
                st.vel[side][k] -= wc[k];
            }
        }
    }
    st.when = super::stats::now_s();
    st.dt = last_step_dt;
    st.at = now;
    SERIAL += 1;
    st.serial = SERIAL;
    // the loose things by the hands for the fingers, before the step is told: the worker solves both together
    super::loose::publish_loose_near(&st);
    super::worker::STEPS.write(&st);
    STEPS_SEEN.fetch_add(1, Relaxed);
    super::worker::signal_stepped();
    // One shared static slice, even if climbing is the only consumer (finger conformance off).
    let s = settings::current();
    if (s.fingers != 0 || s.climb != 0) && px::penetration().is_some() && (st.valid[0] || st.valid[1]) {
        super::near::near_slice(sc, &origin);
    }
    if st.valid[0] || st.valid[1] {
        super::loose::loose_slice(sc, &origin);
    }
    charge(&AFTER_US, &AFTER_MOST_US, t0);
}
