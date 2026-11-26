//! What crosses threads (PhysHands.cpp's "what crosses threads" and include/Natives/PhysHands.hpp's FingerChain /
//! FingerHand): plain copies, each through a Seq or under a lock.
use cvr_physhands::handspace::Reference;
use cvr_physhands::person::PersonAttachment;

/// One primary avatar pose, under the animation pose lock. Targets, shoulders, forearms and the solved body are
/// committed together after the final FK pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PoseFrame {
    pub entity: Reference,
    pub tracking: Reference,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Target {
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    pub entity_pos: [f32; 3],
    pub entity_rot: [f32; 4],
    pub at: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawnHand {
    pub p: [f32; 3],
    pub q: [f32; 4],
    pub at: u64,
}
/// The arm solve's shoulder, model space, and the arm's length (upper arm + forearm), m (anim thread).
#[derive(Clone, Copy, Debug, Default)]
pub struct Shoulder {
    pub model: [f32; 3],
    pub reach: f32,
    pub at: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Forearm {
    pub elbow: [f32; 3],
    pub at: u64,
}
pub const MOST_SELF: usize = 8;
#[derive(Clone, Copy, Debug, Default)]
pub struct SelfCap {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub r: f32,
    pub meets: u8,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SelfShown {
    pub c: [SelfCap; MOST_SELF],
    pub n: i32,
    pub at: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PalmFrame {
    pub skin: [f32; 3],
    pub normal: [f32; 3],
    pub along: [f32; 3],
    pub valid: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseInputs {
    pub frame: PoseFrame,
    pub target: [Target; 2],
    pub shoulder: [Shoulder; 2],
    pub forearm: [Forearm; 2],
    pub drawn: [DrawnHand; 2],
    pub palm: [PalmFrame; 2],
    pub applied_grip: [PersonAttachment; 2],
    pub applied_grip_weight: [f32; 2],
    pub body: SelfShown,
    pub at: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct TouchInfo {
    pub kind: i32,
    pub owner: i32,
    pub index: i32,
    pub simulates: i32,
    pub depth: f32,
}
/// Physics-thread input and command for one completed step.
#[derive(Clone, Copy, Debug, Default)]
pub struct MotorProbe {
    pub before: [f32; 4],
    pub before_spin: [f32; 3],
    pub raw_spin: [f32; 3],
    pub asked_spin: [f32; 3],
    pub given_spin: [f32; 3],
    pub dt: f32,
    pub angle: f32,
    pub gain: f32,
    pub budget: f32,
    pub flags: u32,
    pub touch: TouchInfo,
}
/// After a step: where each body is and where it was sent, PhysX's space; the frame's entity.
#[derive(Clone, Copy, Debug, Default)]
pub struct Step {
    pub pose: [[f32; 7]; 2],
    pub sent: [[f32; 7]; 2],
    /// tracking as that step had it, before the arm's reach brought it back (PhysX's space)
    pub sent_track: [[f32; 3]; 2],
    /// the player's entity as that tracking had it (PhysX's space)
    pub sent_entity: [[f32; 3]; 2],
    pub valid: [bool; 2],
    pub entity_rot: [f32; 4],
    pub origin: [f32; 3],
    pub touching: [bool; 2],
    pub on_world: [bool; 2],
    pub riding: [bool; 2],
    pub vel: [[f32; 3]; 2],
    pub spin: [[f32; 3]; 2],
    /// seconds (performance counter), right after the fetch
    pub when: f64,
    /// the step's own length, s
    pub dt: f32,
    pub at: u64,
    pub serial: u64,
    pub scanner_epoch: u64,
    pub grip_revision: [u64; 2],
    pub motor: [MotorProbe; 2],
    pub frame: PoseFrame,
}
/// Worker -> anim thread: the offset (model space), and, for a hand held off by something, where its body is in the
/// game's world (smoothed) and how much the drawn hand is that place rather than tracking plus the offset.
#[derive(Clone, Copy, Debug, Default)]
pub struct Offset {
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    pub body: [f32; 3],
    pub held: f32,
    pub on: bool,
    pub at: u64,
    /// the body itself, for drawn=1: its rotation, velocities (zero while it is stuck on the world), when it was there
    pub body_mode: bool,
    pub body_rot: [f32; 4],
    pub vel: [f32; 3],
    pub spin: [f32; 3],
    pub when: f64,
    pub step_dt: f32,
    /// 0..1, eased: how much a gun's hand is turned as its body is rather than as tracking has it
    pub contact: f32,
    /// for the trace: the body as PhysX has it (world), the step's touch
    pub raw: [f32; 3],
    pub touching: u8,
    pub on_world: u8,
    pub trace: u8,
    /// where that step sent the body: tracking as it was then (world)
    pub sent: [f32; 3],
    pub sent_rot: [f32; 4],
    /// tracking itself at that step (world): sent, less the arm's reach bringing it back
    pub sent_track: [f32; 3],
    /// the player's entity as that tracking had it (world)
    pub sent_entity: [f32; 3],
    /// on the world and refused: drawn at the latch, tracking's move not added
    pub pressed: bool,
    pub riding: bool,
    /// PhysX's origin in the game's world at that step
    pub origin: [f32; 3],
    /// the body's turn as PhysX has it (world), with raw its place
    pub raw_rot: [f32; 4],
    /// the skin's margin the body's shapes are grown by
    pub margin: f32,
    pub scanner_epoch: u64,
    pub frame: PoseFrame,
}
/// Physics thread -> anim thread: the hold a hand has taken on the world (climb), the game's world.
#[derive(Clone, Copy, Debug, Default)]
pub struct HoldPose {
    pub p: [f32; 3],
    pub q: [f32; 4],
    pub on: bool,
    pub at: u64,
}
/// Script thread -> physics thread: a body a hand holds fast and its pose in the hand bone's frame (HoldBody), asked
/// again every frame it is held.
#[derive(Clone, Copy, Debug)]
pub struct BodyHold {
    pub actor: usize,
    pub scene: usize,
    pub reference: usize,
    pub proxy: u32,
    pub body: u32,
    pub ref_body: u32,
    pub p: [f32; 3],
    pub q: [f32; 4],
    pub g: [f32; 3],
    pub torque: f32,
    pub on: bool,
    pub at: u64,
}
impl Default for BodyHold {
    fn default() -> BodyHold {
        BodyHold { actor: 0, scene: 0, reference: 0, proxy: 0, body: 0, ref_body: 0, p: [0.0; 3], q: [0.0; 4], g: [0.0; 3], torque: 0.0, on: false, at: 0 }
    }
}
/// Physics thread -> anim thread: how far a body held by the hand is turned off the turn the hand has it at
/// (HoldBody), the game's world, and the point held in the hand bone's frame.
#[derive(Clone, Copy, Debug)]
pub struct HoldLoad {
    pub wrist: [f32; 4],
    pub seat: [f32; 3],
    pub on: bool,
    pub at: u64,
    pub proxy: u32,
    pub body: u32,
    pub own_mass: f32,
    pub contact_error_um: u64,
}
impl Default for HoldLoad {
    fn default() -> HoldLoad {
        HoldLoad { wrist: [0.0; 4], seat: [0.0; 3], on: false, at: 0, proxy: 0, body: 0, own_mass: 0.0, contact_error_um: cvr_physhands::held_grip_contact::INVALID }
    }
}
/// Worker -> physics thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wanted {
    pub on: [bool; 2],
    pub at: u64,
}

/// THE FINGERS. A hand's five chains (index, middle, ring, little, thumb) in its own bone's frame, as the finger layers
/// left them this pass, before any bend from here.
#[derive(Clone, Copy, Debug, Default)]
pub struct FingerChain {
    /// the first bone's parent in the hand's frame: rotation x y z w, then position
    pub base: [f32; 7],
    /// each bone's local pose, the same way
    pub local: [[f32; 7]; 3],
    /// the fingertip in the last bone's frame
    pub tip: [f32; 3],
    /// each joint's flexion axis in its own bone's frame; turning about it by +a flexes
    pub hinge: [[f32; 3]; 3],
    /// how far each joint may turn from where it is now, radians (its anatomical range)
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    /// each joint's flexion from the straight finger as the animation has it now, radians
    pub flex: [f32; 3],
    /// the segments' capsules, m
    pub radius: [f32; 3],
    /// each segment's capsule off its bone's line, in that bone's frame (the skin's centre)
    pub offset: [[f32; 3]; 3],
    /// 0: that joint is not turned from here
    pub moves: [u8; 3],
}
#[derive(Clone, Copy, Debug, Default)]
pub struct FingerHand {
    pub f: [FingerChain; 5],
    /// nothing in the hand: no held thing, no weapon, no carried body
    pub free: bool,
    pub at: u64,
}
