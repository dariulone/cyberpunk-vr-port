//! The physics hands' pure pieces (main's include/Natives headers that hold their own logic): what the hands' worker,
//! the physics step and the animation thread share without touching the game. The engine side -- the PhysX bodies,
//! the step hooks, the natives -- lives in cvr-rig's vrik and calls these.
//!
//!   handspace          include/Natives/HandReferenceFrame.hpp: an avatar's frame, a frame from the view, the motion between two
//!   held_grip          include/Natives/HeldGripPose.hpp: a held ragdoll part's angular drive, its targets for one and two hands
//!   held_grip_contact  include/Natives/HeldGripContact.hpp: one physics step's gate, the held point's error in the world
//!   held_body_mass     include/Natives/HeldBodyMass.hpp: the held actor's identity and its mass before the solver's
//!   person             include/Natives/PersonGrip.hpp and PersonCollisionFilter.hpp: the NPC grips' snapshots, the
//!                      reach limit, the capsules' filter per hand
//!   blade              include/Anim/BladeAttachment.hpp and BladeContactState.cpp's rail helpers: the lodged blade's rail
//!   blade_contact      include/Anim/BladeContactState.hpp: a blade's depth and angle in a capsule, the swept segment, the shape lookups
//!   climb_support      include/Natives/ClimbSupport.hpp: a top or an edge the hand holds on to
//!   hand_skin          include/Natives/HandSkin.hpp: the palm's box, the fingers' and the forearm's capsules fitted to V's skin
//!   mesh_profile       src/Natives/HeldBody.cpp's MeshProfile: the drawn radius profiles of round props (GraspProfiles.inc)
pub mod blade;
pub mod blade_contact;
pub mod blade_mesh_blob;
pub mod blade_mesh_cut;
pub mod climb_support;
pub mod hand_skin;
pub mod handspace;
pub mod held_body_mass;
pub mod held_grip;
pub mod held_grip_contact;
pub mod mesh_profile;
pub mod person;
