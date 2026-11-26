//! The holds a hand takes (physics thread state, the other threads' views): the hands' push and pull on the player's
//! body, the hold on the world (climb), a person's part held, the grasp a held part asks for, and the snapshots the NPC
//! grips trade with the physics step.
use super::body::actor;
use super::loose::global_pose;
use super::people::{person_near, person_near_n, person_owner};
use super::px::{self, CapsuleGeometry, PxTransform};
use super::seq::Seq;
use super::tick_ms;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering::*};
use cvr_physhands::person::{PersonAttachment, PersonContact};

/// Physics thread -> anim thread: the hold a hand has taken on the world (climb), the game's world: its place and turn.
pub static HOLD_POSES: [Seq<super::types::HoldPose>; 2] = [Seq::new(super::types::HoldPose { p: [0.0; 3], q: [0.0; 4], on: false, at: 0 }), Seq::new(super::types::HoldPose { p: [0.0; 3], q: [0.0; 4], on: false, at: 0 })];
/// Script thread -> physics thread: a body a hand holds fast and its pose in the hand bone's frame (HoldBody), asked
/// again every frame it is held.
pub static BODY_HOLDS: [Seq<super::types::BodyHold>; 2] = [Seq::new(BODY_HOLD0), Seq::new(BODY_HOLD0)];
const BODY_HOLD0: super::types::BodyHold = super::types::BodyHold { actor: 0, scene: 0, reference: 0, proxy: 0, body: 0, ref_body: 0, p: [0.0; 3], q: [0.0; 4], g: [0.0; 3], torque: 0.0, on: false, at: 0 };
/// Physics thread -> anim thread: how far a body held by the hand is turned off the turn the hand has it at
/// (HoldBody), the game's world, and the point held in the hand bone's frame: the drawn hand is turned so about that
/// point.
pub static HOLD_LOADS: [Seq<super::types::HoldLoad>; 2] = [Seq::new(HOLD_LOAD0), Seq::new(HOLD_LOAD0)];
const HOLD_LOAD0: super::types::HoldLoad = super::types::HoldLoad { wrist: [0.0; 4], seat: [0.0; 3], on: false, at: 0, proxy: 0, body: 0, own_mass: 0.0, contact_error_um: cvr_physhands::held_grip_contact::INVALID };
/// physics thread: the world step's gate, no independent polling or timer
pub static mut held_contact_step: cvr_physhands::held_grip_contact::StepGate = cvr_physhands::held_grip_contact::StepGate { scene: 0, generation: 0, prepared: false, consumed: true };
/// physics thread, the attachment used by each completed motor step
pub static mut sent_grip_revision: [u64; 2] = [0; 2];
/// physics thread: the step being simulated
pub static mut last_step_dt: f32 = 1.0 / 60.0;
/// the hands' push on the player's body (physics thread writes, the player's move reads): world x y, m/s
#[derive(Clone, Copy, Default)]
pub struct Push {
    pub v: [f32; 2],
    pub at: u64,
}
pub static BODY_PUSHES: Seq<Push> = Seq::new(Push { v: [0.0; 2], at: 0 });
/// ...and the pull of the hands that hold on (climb), world x y z m/s
#[derive(Clone, Copy, Default)]
pub struct Pull {
    pub v: [f32; 3],
    pub at: u64,
    pub on: bool,
}
pub static BODY_PULLS: Seq<Pull> = Seq::new(Pull { v: [0.0; 3], at: 0, on: false });
pub static mut climb_on: [bool; 2] = [false; 2];
pub static mut grip_was: [bool; 2] = [false; 2];
/// where each hand took hold, the game's world
pub static mut climb_anchor: [[f32; 3]; 2] = [[0.0; 3]; 2];
/// ...and how it was turned then (PhysX's turn is the game's world's: the origin only moves things). THE HOLD IS FIXED,
/// as the grenade's pin fixes the hand: the body is sent to this place and turn, and the drawn hand is them (adjust).
pub static mut climb_turn: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0]; 2];
/// a person's part held: its id, the capsule's middle and axis and the hand as they were taken hold of, when it was last
/// seen, the hold as last placed (PhysX's space) and the controller off it (the game's world); a hold pulled off spends
/// the grip until it is let go. The hand goes with the capsule's middle and the swing of its axis alone: a capsule's own
/// turn is only its axis's shortest arc from x, which for an arm lying along -x (folded arms) turned half round about
/// it from one step to the next, and the hold went round with it, 20-30 cm (2026-11-10).
pub static mut grab_on: [bool; 2] = [false; 2];
pub static mut grip_spent: [bool; 2] = [false; 2];
pub static mut grab_owner: [i32; 2] = [0; 2];
/// THE GRASP A HELD PART ASKS FOR (the script's, VRPhysHandsGrabStyle: 1 a can, 2 a can with the fingers wide, 3 gently,
/// 4 the palm), for the part it names: the fingers are closed on that part's capsule as it lies in the hand.
pub static GRAB_STYLE_OWNER: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
pub static GRAB_STYLE: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
/// physics thread
pub static mut grasp_given: [bool; 2] = [false; 2];
pub static mut grab_hand: [PxTransform; 2] = [PxTransform::ZERO; 2];
pub static mut grab_last: [PxTransform; 2] = [PxTransform::ZERO; 2];
/// THE SEAT: the hold's hand (grab_hand) eased to this, once the grasp has seated the part (SEAT_TAU)
pub static mut grab_seat: [PxTransform; 2] = [PxTransform::ZERO; 2];
pub static mut grab_seating: [bool; 2] = [false; 2];
/// s
pub const SEAT_TAU: f32 = 0.05;
pub static mut grab_mid: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut grab_axis: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut grab_seen_at: [u64; 2] = [0; 2];
pub static mut grab_pull: [[f32; 3]; 2] = [[0.0; 3]; 2];
#[derive(Clone, Copy, Default)]
pub struct Grab {
    pub owner: i32,
    pub pull: [f32; 3],
    pub at: u64,
    pub serial: u64,
}
pub static GRABS: [Seq<Grab>; 2] = [Seq::new(Grab { owner: 0, pull: [0.0; 3], at: 0, serial: 0 }), Seq::new(Grab { owner: 0, pull: [0.0; 3], at: 0, serial: 0 })];
pub static mut grab_serial: [u64; 2] = [0; 2];
const CONTACT0: PersonContact = PersonContact { owner: 0, serial: 0, at: 0, position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], capsule: [0.0; 3], capsule_rotation: [0.0, 0.0, 0.0, 1.0], radius: 0.0, half: 0.0, ready: false };
const ATTACHMENT0: PersonAttachment = PersonAttachment {
    owner: 0,
    serial: 0,
    at: 0,
    revision: 0,
    control_at: 0,
    position: [0.0; 3],
    rotation: [0.0, 0.0, 0.0, 1.0],
    capsule: [0.0; 3],
    capsule_rotation: [0.0, 0.0, 0.0, 1.0],
    radius: 0.0,
    half: 0.0,
    on: false,
    release: false,
};
pub static PERSON_CONTACTS: [Seq<PersonContact>; 2] = [Seq::new(CONTACT0), Seq::new(CONTACT0)];
pub static PERSON_ATTACHMENTS: [Seq<PersonAttachment>; 2] = [Seq::new(ATTACHMENT0), Seq::new(ATTACHMENT0)];
pub static GRIP_SPENT_SHOWN: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// climb_on, for the other threads
pub static HOLDS_ON_SHOWN: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
pub static mut push_want: [f32; 2] = [0.0; 2];
pub static mut push_shown: [f32; 2] = [0.0; 2];

/// HoldsOn: this hand holds on (its grip closed on a top or an edge, the player pulled after it). Any thread.
pub fn holds_on(side: usize) -> bool {
    side <= 1 && super::on(side) && HOLDS_ON_SHOWN[side].load(Relaxed)
}

/// HeldNearHands: the hands holding a capsule of this person near it (physics thread, PhysX's space) -- of the same
/// person (the script's id over 32), its segment within its radius and 12 cm of the hand's bone; bits 1 the left, 2 the
/// right.
pub unsafe fn held_near_hands(owner: i32, a: &[f32; 3], b: &[f32; 3], radius: f32) -> u8 {
    let mut held = 0u8;
    for sd in 0..2 {
        if !grab_on[sd] || grab_owner[sd] / 32 != owner / 32 || actor[sd].is_null() {
            continue;
        }
        let at = global_pose(actor[sd]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
        let mut t = (0..3).map(|k| (at.p[k] - a[k]) * ab[k]).sum::<f32>();
        t = if l2 > 1e-8 { (t / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d2: f32 = (0..3).map(|k| a[k] + ab[k] * t - at.p[k]).map(|d| d * d).sum();
        if d2 < (radius + 0.12) * (radius + 0.12) {
            held |= 1u8 << sd;
        }
    }
    held
}
/// PersonShape: a person's part, its capsule (physics thread): pose (its axis x), radius and half height. None: not
/// among them now.
pub unsafe fn person_shape(owner: i32) -> Option<(PxTransform, f32, f32)> {
    for i in 0..person_near_n {
        if person_owner[i] == owner {
            let g: CapsuleGeometry = core::ptr::read_unaligned(person_near[i].geometry_ptr().cast());
            if g.kind != px::GEOM_CAPSULE {
                return None;
            }
            return Some((person_near[i].pose, g.radius, g.half_height));
        }
    }
    None
}
/// ReadPersonContact: where a hand met a person's capsule, within 200 ms.
pub fn read_person_contact(side: usize) -> Option<PersonContact> {
    if side > 1 {
        return None;
    }
    PERSON_CONTACTS[side].read().filter(|c| c.owner > 0 && tick_ms().wrapping_sub(c.at) < 200)
}
/// ReadPersonAttachment: the hold on a person's part as applied, when it is the hold this hand has now.
pub fn read_person_attachment(side: usize) -> Option<PersonAttachment> {
    if side > 1 {
        return None;
    }
    let out = PERSON_ATTACHMENTS[side].read()?;
    if (!out.on && !out.release) || tick_ms().wrapping_sub(out.at) > 200 {
        return None;
    }
    let grip = GRABS[side].read()?;
    (grip.owner > 0 && grip.owner == out.owner && grip.serial == out.serial && tick_ms().wrapping_sub(grip.at) < 200).then_some(out)
}
static REVISIONS: AtomicU64 = AtomicU64::new(0);
/// PublishPersonAttachment: the NPC grip's applied hold, stamped and numbered.
pub fn publish_person_attachment(side: usize, value: &PersonAttachment) {
    if side > 1 {
        return;
    }
    let mut copy = *value;
    copy.at = tick_ms();
    copy.revision = REVISIONS.fetch_add(1, Relaxed) + 1;
    PERSON_ATTACHMENTS[side].write(&copy);
}
/// GrabbedPerson: the person's part a hand holds (the script's id, 0 none) and the controller's place off the hold (the
/// game's world).
pub fn grabbed_person(side: usize) -> (i32, [f32; 3]) {
    if side > 1 || !super::on(side) {
        return (0, [0.0; 3]);
    }
    match GRABS[side].read() {
        Some(g) if tick_ms().wrapping_sub(g.at) <= 200 => (g.owner, g.pull),
        _ => (0, [0.0; 3]),
    }
}
/// GripSpent: a hold pulled off by itself -- the grip counts as open until it is let go.
pub fn grip_spent_shown(side: usize) -> bool {
    side <= 1 && GRIP_SPENT_SHOWN[side].load(Relaxed)
}
/// SetGrabStyle: the grasp a person's part asks for while a hand holds it.
pub fn set_grab_style(side: usize, owner: i32, style: i32) {
    if side > 1 {
        return;
    }
    GRAB_STYLE[side].store(style.clamp(0, 4), Relaxed);
    GRAB_STYLE_OWNER[side].store(owner, Relaxed);
}
