#pragma once
#include <cstdint>
// PhysHands -- each hand a rigid body of its own in the game's PhysX world, steered toward the controller with a hand's
// force, and the drawn hand where that body is. A wall stops it because PhysX stops it; a cup it meets is pushed by
// it; a crate does not move for it. Nothing here asks a ray.
//
// THREADS. Nothing heavy runs on a game thread:
//   * the animation thread (VRIK_ApplyHandStop) publishes tracking and reads back an offset: a few copies; the finger
//     layer (Anim/FingerContact.cpp) publishes the fingers as drawn and turns their joints by what comes back;
//   * the physics thread, in PhysXStep's hooks, writes two velocities before each step and reads two poses after it
//     (the bodies are made there once, the filter census runs there a slice a frame, and so does the list of the
//     world's shapes round the player that the fingers meet);
//   * a worker thread of its own turns each step's result into the drawn hand's offset, solves the fingers against
//     that list (PxGeometryQuery, no scene access), decides when the hands are on (vehicles, menus, ladders, water),
//     reloads config/physhands.ini when it changes, and keeps the statistics.
// They meet in lock-free snapshots (a sequence counter around each copy), one writer each.
namespace cvr::physhands {
// PhysXStep, before PxScene::simulate, on the thread that steps: the scene is not simulating.
void BeforeSimulate(void* scene, float dt);
// PhysXStep, after PxScene::fetchResults handed over this step's results.
void AfterFetch(void* scene);
// VRIK_ApplyHandStop: where tracking puts the hand bone this frame, model space (the rotation as it is solved).
void NoteTarget(int side, const float* modelPos, const float* modelRot);
// The same place, right after: the drawn hand moved to where its body is. True when it did, and then the script's
// stops are not applied on top.
bool Adjust(int side, float* modelPos, float* modelRot);
// A body held by the hand at the point it closed on, as a pistol is (the script's carry of a body lying dead): the body's
// PhysX actor and its scene (Natives/DebugView.hpp RigidDynamicOf), its pose in the hand bone's frame (`pos` xyz, `rot`
// x y z w), `grip` xyz the point held in the body's frame, and `pos` w the hand's grip torque (N m). At each step the
// point held is put where the hand bone is (eased in from where it lay when taken) and the body is turned toward the turn
// the hand has it at by no more than that torque: the rest of its ragdoll, hanging from it, turns it further by its
// weight, and the drawn hand is turned with it (Adjust), so the body stays in the hand as it is. Two hands on one body
// hold it together, their torques added. Asked every frame it is held: unasked 0.3 s, or with a null actor, it is let
// go. False when its scene is not the hands'. Script thread (one writer).
bool HoldBody(int side, void* actor, void* actorScene, uint32_t proxy, uint32_t body, const float* pos, const float* rot,
              const float* grip, void* ref = nullptr, uint32_t refBody = 0);
// The point `local` of an actor's frame in the game's world (`origin` its scene's). False when it faults. Script thread.
bool ActorPoint(void* actor, const float* origin, const float* local, float* world);
// The point of a body's own shapes nearest `point` (the game's world; `origin` is its scene's, Natives/DebugView.hpp
// RigidDynamicOf), and how far: 0 inside. False when it has no shape PxGeometryQuery measures. Script thread.
bool BodySurface(void* actor, const float* origin, const float* point, float* closest, float* distance);
// The body of this hand is in the world and steered (0 left, 1 right).
bool On(int side);
bool Tracing();           // trace=1 (the finger layer's diagnostics)
// physhands.ini finger_hinge_only: the fingers' joints drawn as hinges while they lie on something
bool FingerHingeOnly();
// The velocity the hands held off by the world give the player's body (world x y, m/s; Bonelab's -F on the body). False
// when there is none. Any thread.
bool BodyPush(float* velocityXY);
// The velocity the hands that hold on (the grip closed on a top or an edge, climbing) give the player's body so that
// tracking comes back to where they took hold (world x y z, m/s). False when no hand holds on. Any thread.
bool BodyPull(float* velocity);
// This hand holds on (its grip closed on a top or an edge, the player pulled after it). Any thread.
bool HoldsOn(int side);
// A fresh weighted body grip owns the wrist pose; arm IK must not shorten it after load compensation.
bool HoldsBody(int side);
// VRIK_SolveArm, once the arm is solved: the elbow, the wrist and the hand's turn as drawn, model space. The forearm's
// capsule on the hand's body is posed by them; the shoulder says whether an arm reaches where tracking is, and
// `reach` (m) is the farthest the drawn hand gets from that shoulder (the solve's stretch caps), 0 for none.
void NoteForearm(int side, const float* elbowModel, const float* wristModel, const float* handModelRot,
                 const float* shoulderModel, float reach);

// THE FINGERS. A hand's five chains (index, middle, ring, little, thumb) in its own bone's frame, as the finger layers
// left them this pass, before any bend from here.
struct FingerChain {
    float base[7];         // the first bone's parent in the hand's frame: rotation x y z w, then position
    float local[3][7];     // each bone's local pose, the same way
    float tip[3];          // the fingertip in the last bone's frame
    float hinge[3][3];     // each joint's flexion axis in its own bone's frame; turning about it by +a flexes
    float lo[3], hi[3];    // how far each joint may turn from where it is now, radians (its anatomical range)
    float flex[3];         // each joint's flexion from the straight finger as the animation has it now, radians
    float radius[3];       // the segments' capsules, m
    float offset[3][3];    // each segment's capsule off its bone's line, in that bone's frame (the skin's centre)
    uint8_t moves[3];      // 0: that joint is not turned from here
};
struct FingerHand {
    FingerChain f[5];
    bool free;             // nothing in the hand: no held thing, no weapon, no carried body
    uint64_t at;
};
void PublishFingers(int side, const FingerHand& hand);
// Each joint's turn about its hinge, radians: [finger][joint] in the chains' order, and the flexion the animation had
// when they were solved (FingerChain::flex). False when there is none to make.
bool FingerBends(int side, float bend[5][3], float flexAt[5][3] = nullptr);

// HeldBody, on the physics thread, as a hand starts to steer a loose thing by its body: what stood on it comes down.
void Taken(void* actor);
// HeldBody, physics thread: the filter a held thing's shape gets while held (from its own); false: leave it as it is.
bool HeldAlone(const uint32_t* original, uint32_t* out);
// After changing an actor's filter (physics thread, between steps): its pairs filtered again (PxScene::resetFiltering).
void FilterAgain(void* scene, void* actor);
// A test tool: the few light loose things nearest this point of the game's world stood one on another (next step).
void TestStack(const float* worldAt, bool dump, bool dumpShapes = false);
// The gun in a hand: `count` boxes (each set before by SetWeaponPart, corners in the gun's frame), the gun's frame in the
// hand bone's (rotation x y z w, then position). Count 0: none. Script thread.
bool SetWeapon(int side, int count, const float* frame);
bool SetWeaponPart(int side, int index, const float* lo, const float* hi);
// The people near the hands: capsule `index` between a and b (the game's world), its radius; then the count of them. Every
// frame, script thread.
bool SetPerson(int index, const float* a, const float* b, float radius, int owner = 0);
void SetPeople(int count);
// V's own body as the hands meet it (the game's world): capsule `index` of them, its radius and the hands that meet it
// (bits: 1 the left, 2 the right). False past the last. Any thread.
bool SelfShape(int index, float* a, float* b, float* radius, int* meets);
// The person's part a hand holds (the script's id, 0 none) and the controller's place off the hold (the game's world).
int GrabbedPerson(int side, float* pull);
// A hold pulled off by itself: the grip counts as open until it is let go.
bool GripSpent(int side);
// How much V's clothes add round V's own trunk for the hands, m (the script, from what is worn).
void SetSelfPad(float metres);
// The grasp a person's part asks for while a hand holds it (the script's id for it; 1 a can, 2 wide, 3 gently, 4 the palm).
void SetGrabStyle(int side, int owner, int style);
}
